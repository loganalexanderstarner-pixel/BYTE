//! Taking a picture of the screen. Everything above this file talks to the `FrameSource` trait, so it is tested with fake
//! frames; the real source (Windows.Graphics.Capture, with a GDI fallback) lives in `wgc.rs`.

use super::frame::{self, Frame};
use super::policy::{self, Refusal, WindowInfo};

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureError {
    /// This platform has no capture yet.
    Unsupported,
    /// Policy said no (private chat, excluded window, BYTE's own window).
    Refused(Refusal),
    /// The capture API failed (a full-screen game that blocks it, a locked screen, no desktop).
    Failed(String),
    /// The picture came back all black (protected content or a blocked game window).
    Blank,
}

impl CaptureError {
    pub fn say(&self) -> String {
        match self {
            CaptureError::Unsupported => crate::platform_text::here("Looking at the screen isn't built for this system yet."),
            CaptureError::Refused(r) => r.say(),
            CaptureError::Failed(e) => format!("BYTE couldn't capture the window ({e})."),
            CaptureError::Blank => "The window came back black. Some apps and full-screen games block screen capture; try the game in borderless or windowed mode.".into(),
        }
    }
}

/// A frame and the window it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Grabbed {
    pub window: WindowInfo,
    pub frame: Frame,
}

/// What to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The window in front.
    Foreground,
}

pub trait FrameSource {
    fn name(&self) -> &'static str;
    /// The window in front (program and title), if one can be told.
    fn front_window(&mut self) -> Option<WindowInfo>;
    /// One full-size frame of the target, with the window it was ACTUALLY taken from (read in the same step as the capture
    /// target was chosen, never re-read afterwards), so the rules are applied to the right window.
    fn grab(&mut self, target: Target) -> Result<Grabbed, CaptureError>;
    /// BYTE's own window is the one in front (the target is then the window below it).
    fn front_is_own(&mut self) -> bool {
        false
    }
    /// Lets go of anything held from the last frame (the GPU keeps one while a capture runs).
    fn release(&mut self) {}
    /// The screen is locked (nothing to see, and nothing should be kept).
    fn locked(&mut self) -> bool {
        false
    }
}

/// A frame ready to send to a model or keep: shrunk and encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct Encoded {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Encoded {
    pub fn data_url(&self) -> String {
        use base64::Engine as _;
        format!("data:{};base64,{}", self.mime, base64::engine::general_purpose::STANDARD.encode(&self.bytes))
    }
}

/// Shrinks to `max_side` and encodes (JPEG on Windows; a plain BMP elsewhere, where nothing is captured anyway).
pub fn encode(f: &Frame, max_side: u32) -> Result<Encoded, CaptureError> {
    let small = frame::downscale(f, max_side);
    #[cfg(windows)]
    {
        let bytes = super::wgc::encode_jpeg(&small, super::config::JPEG_QUALITY).map_err(CaptureError::Failed)?;
        Ok(Encoded { mime: "image/jpeg", bytes, width: small.width, height: small.height })
    }
    #[cfg(not(windows))]
    {
        Ok(Encoded { mime: "image/bmp", bytes: frame::bmp(&small), width: small.width, height: small.height })
    }
}

/// The pixels of a picture made by `encode` (replay frames are kept encoded).
pub fn decode(bytes: &[u8]) -> Result<Frame, CaptureError> {
    #[cfg(windows)]
    {
        super::wgc::decode_image(bytes).map_err(CaptureError::Failed)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err(CaptureError::Unsupported)
    }
}

/// The window in front, without capturing anything.
pub fn front_window() -> Option<WindowInfo> {
    default_source().front_window()
}

/// A source with no capture (platforms without it).
pub struct Unsupported;

impl FrameSource for Unsupported {
    fn name(&self) -> &'static str {
        "unsupported"
    }
    fn front_window(&mut self) -> Option<WindowInfo> {
        None
    }
    fn grab(&mut self, _: Target) -> Result<Grabbed, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

/// The real source for this system.
pub fn default_source() -> Box<dyn FrameSource> {
    #[cfg(windows)]
    {
        Box::new(super::wgc::Wgc::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(Unsupported)
    }
}

/// One picture of the window in front, if the rules allow it. Returns the window it came from.
/// The rules are applied before the capture, to the window the capture was really taken from, and to the window in front afterwards.
pub fn once(src: &mut dyn FrameSource, excludes: &[String], private: bool) -> Result<(WindowInfo, Frame), CaptureError> {
    let before = src.front_window();
    policy::may_capture(before.as_ref(), excludes, private).map_err(CaptureError::Refused)?;
    let got = src.grab(Target::Foreground);
    let got = match got {
        Ok(g) => g,
        Err(e) => {
            src.release();
            return Err(e);
        }
    };
    let after = src.front_window();
    let verdict = policy::may_capture(Some(&got.window), excludes, private).and_then(|_| policy::may_capture(after.as_ref(), excludes, private));
    src.release();
    verdict.map_err(CaptureError::Refused)?;
    if got.frame.is_blank() {
        return Err(CaptureError::Blank);
    }
    Ok((got.window, got.frame))
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::collections::VecDeque;

    /// Frames and windows from a script.
    pub struct Fake {
        pub windows: VecDeque<Option<WindowInfo>>,
        pub last_window: Option<WindowInfo>,
        pub frames: VecDeque<Result<Frame, CaptureError>>,
        /// The window each grab really captures (default: the current one).
        pub grab_windows: VecDeque<Option<WindowInfo>>,
        pub released: usize,
        pub is_locked: bool,
        pub grabs: usize,
    }

    impl Fake {
        pub fn new(window: &str, title: &str) -> Fake {
            Fake {
                windows: VecDeque::new(),
                last_window: Some(WindowInfo { program: window.into(), title: title.into() }),
                frames: VecDeque::new(),
                grab_windows: VecDeque::new(),
                released: 0,
                is_locked: false,
                grabs: 0,
            }
        }
        pub fn with_frames(mut self, n: usize) -> Fake {
            for i in 0..n {
                self.frames.push_back(Ok(Frame::solid(64, 36, 40 + i as u8, 90, 140)));
            }
            self
        }
    }

    impl FrameSource for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn front_window(&mut self) -> Option<WindowInfo> {
            match self.windows.pop_front() {
                Some(w) => {
                    self.last_window = w.clone();
                    w
                }
                None => self.last_window.clone(),
            }
        }
        fn grab(&mut self, _: Target) -> Result<Grabbed, CaptureError> {
            self.grabs += 1;
            let window = match self.grab_windows.pop_front() {
                Some(w) => w,
                None => self.last_window.clone(),
            }
            .unwrap_or_default();
            let frame = self.frames.pop_front().unwrap_or_else(|| Ok(Frame::solid(64, 36, 40, 90, 140)))?;
            Ok(Grabbed { window, frame })
        }
        fn release(&mut self) {
            self.released += 1;
        }
        fn locked(&mut self) -> bool {
            self.is_locked
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Fake;
    use super::*;

    #[test]
    fn a_normal_window_is_captured_and_named() {
        let mut f = Fake::new("game", "Cities").with_frames(1);
        let (w, frame) = once(&mut f, &policy::default_excludes(), false).unwrap();
        assert_eq!(w.program, "game");
        assert_eq!(frame.width, 64);
    }

    #[test]
    fn nothing_is_grabbed_when_the_policy_says_no() {
        let mut f = Fake::new("1Password", "Vault").with_frames(1);
        assert!(matches!(once(&mut f, &policy::default_excludes(), false), Err(CaptureError::Refused(Refusal::Excluded(_)))));
        assert_eq!(f.grabs, 0, "an excluded window is never even captured");
        let mut f = Fake::new("game", "x").with_frames(1);
        assert_eq!(once(&mut f, &[], true), Err(CaptureError::Refused(Refusal::Private)));
        assert_eq!(f.grabs, 0);
    }

    #[test]
    fn a_window_that_changed_during_capture_is_dropped() {
        let mut f = Fake::new("game", "x").with_frames(1);
        f.windows.push_back(Some(WindowInfo { program: "game".into(), title: "x".into() }));
        f.windows.push_back(Some(WindowInfo { program: "bitwarden".into(), title: "".into() }));
        assert!(matches!(once(&mut f, &policy::default_excludes(), false), Err(CaptureError::Refused(_))));
    }

    #[test]
    fn the_window_really_captured_is_checked_not_a_reread_one() {
        // The front window looks fine both before and after, but the frame came from a password manager (focus moved in between).
        let mut f = Fake::new("game", "x").with_frames(1);
        f.grab_windows.push_back(Some(WindowInfo { program: "1Password".into(), title: "".into() }));
        assert!(matches!(once(&mut f, &policy::default_excludes(), false), Err(CaptureError::Refused(Refusal::Excluded(_)))));
        assert_eq!(f.released, 1, "the held frame is let go when the picture is refused");
    }

    #[test]
    fn a_black_frame_is_reported_not_sent() {
        let mut f = Fake::new("game", "x");
        f.frames.push_back(Ok(Frame::solid(64, 36, 0, 0, 0)));
        assert_eq!(once(&mut f, &[], false), Err(CaptureError::Blank));
    }

    #[test]
    fn encoding_shrinks_and_produces_a_data_url() {
        let e = encode(&Frame::solid(2000, 1000, 1, 2, 3), 1280).unwrap();
        assert_eq!((e.width, e.height), (1280, 640));
        assert!(e.data_url().starts_with("data:image/"));
        assert!(!e.bytes.is_empty());
    }

    #[test]
    fn unsupported_says_so() {
        let mut u = Unsupported;
        assert_eq!(u.grab(Target::Foreground), Err(CaptureError::Unsupported));
        assert!(!CaptureError::Unsupported.say().is_empty());
    }
}
