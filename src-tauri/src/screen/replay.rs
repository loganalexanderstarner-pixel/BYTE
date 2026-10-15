//! The rolling replay: the last N seconds of the window in front, kept ONLY IN MEMORY.
//!
//! * Frames are shrunk before they are encoded and live in a `Ring` in RAM: never written to disk, never logged, never sent.
//! * The buffer is emptied when it is switched off, when BYTE quits, when a private chat is open, and the moment an excluded
//!   window (password manager, bank, private browser window, anything on the person's list) comes to the front.
//! * While it runs the tray says so and the app gets a `screen://recording` event (a small badge shows it).
//! * It pauses while the screen is locked and backs off when the window cannot be captured (a full-screen game that blocks it).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::capture::{self, FrameSource, Target};
use super::config::{self, Config};
use super::policy::{self, Refusal};
use super::ring::{Ring, Stored, MAX_BYTES};

pub const EVENT: &str = "screen://recording";

/// What the recorder is doing, for the badge and the tray.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The buffer is switched on.
    pub on: bool,
    /// Frames are being kept right now (on, and not paused).
    pub recording: bool,
    /// Why it is paused, in plain words.
    pub paused: Option<String>,
    pub seconds: u32,
}

impl Status {
    pub fn off() -> Status {
        Status { on: false, recording: false, paused: None, seconds: 0 }
    }
}

/// One step of the recorder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tick {
    Kept,
    Paused(&'static str),
}

/// The last look before a frame goes into the ring (done under the ring's lock): a private chat or a stop that arrived while
/// the frame was being taken turns the push into a clear or a drop, so nothing from that moment is left behind.
pub fn guard(action: Action, private: bool, stopped: bool) -> Action {
    match action {
        Action::Push(_) if stopped => Action::Nothing,
        Action::Push(_) if private => Action::Clear,
        other => other,
    }
}

/// What the ring should do after a tick.
#[derive(Debug, PartialEq)]
pub enum Action {
    Push(Vec<u8>),
    Clear,
    Nothing,
}

/// Applies an action to the ring (the one place that changes it).
pub fn act(ring: &mut Ring, action: Action, cfg: &Config, now_ms: u64) {
    ring.set_window(cfg.seconds, now_ms);
    match action {
        Action::Push(bytes) => ring.push(now_ms, bytes),
        Action::Clear => ring.clear(),
        Action::Nothing => {}
    }
}

/// The recorder's decisions, with the screen and the rules handed in, so it is tested with fake frames.
#[derive(Default)]
pub struct Recorder {
    fails: u32,
}

impl Recorder {
    pub fn new() -> Recorder {
        Recorder::default()
    }

    /// Decides what to do for one tick: take a frame, forget everything, or leave things alone.
    pub fn tick(&mut self, src: &mut dyn FrameSource, cfg: &Config, private: bool) -> (Tick, Action) {
        if private {
            return (Tick::Paused("a private chat is open"), Action::Clear);
        }
        if src.locked() {
            src.release();
            return (Tick::Paused("the screen is locked"), Action::Nothing);
        }
        let win = src.front_window();
        match policy::may_capture(win.as_ref(), &cfg.excludes, false) {
            Ok(()) => {}
            // Whatever came just before may have shown the same app: forget it all (and let go of the GPU's last frame).
            Err(Refusal::Excluded(_)) => {
                src.release();
                return (Tick::Paused("a window on your never-look-at list is in front"), Action::Clear);
            }
            // BYTE's own window, or none: nothing to keep, nothing to forget; nothing stays held either.
            Err(_) => {
                src.release();
                return (Tick::Paused("no other app is in front"), Action::Nothing);
            }
        }
        match src.grab(Target::Foreground) {
            Ok(g) if !g.frame.is_blank() => {
                // The rules apply to the window the frame was taken from, and to the one in front now.
                let after = src.front_window();
                let bad = policy::may_capture(Some(&g.window), &cfg.excludes, false).is_err() || policy::may_capture(after.as_ref(), &cfg.excludes, false).is_err();
                if bad {
                    src.release();
                    return (Tick::Paused("a window on your never-look-at list is in front"), Action::Clear);
                }
                match capture::encode(&g.frame, config::REPLAY_SIDE) {
                    Ok(e) => {
                        self.fails = 0;
                        (Tick::Kept, Action::Push(e.bytes))
                    }
                    Err(_) => self.failed(),
                }
            }
            Ok(_) | Err(_) => self.failed(),
        }
    }

    fn failed(&mut self) -> (Tick, Action) {
        self.fails = self.fails.saturating_add(1);
        (Tick::Paused("this window can't be captured (a full-screen game may be blocking it)"), Action::Nothing)
    }

    /// Consecutive failures: the loop waits longer after a few.
    pub fn failures(&self) -> u32 {
        self.fails
    }
}

// ------------------------------------------------------------------ the background thread

struct Shared {
    ring: Mutex<Option<Ring>>,
    status: Mutex<Status>,
}

static SHARED: Shared = Shared { ring: Mutex::new(None), status: Mutex::new(Status { on: false, recording: false, paused: None, seconds: 0 }) };
static PRIVATE: AtomicBool = AtomicBool::new(false);
static RUNNER: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

/// A private chat is open (or was left): while it is, nothing is kept and what was kept is forgotten at once.
pub fn set_private(on: bool) {
    PRIVATE.store(on, Ordering::SeqCst);
    if on {
        clear();
    }
}

pub fn is_private() -> bool {
    PRIVATE.load(Ordering::SeqCst)
}

/// Empties the buffer (memory is released).
pub fn clear() {
    if let Ok(mut r) = SHARED.ring.lock() {
        if let Some(ring) = r.as_mut() {
            ring.clear();
        }
    }
}

/// The frames, oldest first (copies), for a question. Empty if the buffer is off.
pub fn snapshot() -> Vec<Stored> {
    SHARED.ring.lock().ok().and_then(|r| r.as_ref().map(|ring| ring.snapshot())).unwrap_or_default()
}

pub fn status() -> Status {
    SHARED.status.lock().map(|s| s.clone()).unwrap_or_else(|_| Status::off())
}

/// The replay buffer is running and keeping frames right now.
pub fn recording() -> bool {
    status().recording
}

fn publish(app: Option<&tauri::AppHandle>, next: Status) {
    let changed = match SHARED.status.lock() {
        Ok(mut s) => {
            let changed = *s != next;
            *s = next.clone();
            changed
        }
        Err(_) => false,
    };
    if changed {
        if let Some(app) = app {
            super::announce(app, &next);
        }
    }
}

/// Starts, changes or stops the recorder to match the settings. Safe to call whenever settings change.
pub fn apply(app: &tauri::AppHandle, cfg: &Config) {
    if let Ok(mut c) = CONFIG.lock() {
        *c = Some(cfg.clone());
    }
    let running = RUNNER.lock().ok().is_some_and(|r| r.is_some());
    if cfg.replay && !running {
        start(app.clone(), cfg.clone());
    } else if !cfg.replay && running {
        stop(Some(app));
    }
}

fn start(app: tauri::AppHandle, cfg: Config) {
    if !cfg!(windows) {
        return;
    }
    let stop_flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut r) = RUNNER.lock() {
        *r = Some(stop_flag.clone());
    }
    if let Ok(mut ring) = SHARED.ring.lock() {
        *ring = Some(Ring::new(cfg.seconds, MAX_BYTES));
    }
    std::thread::Builder::new()
        .name("byte-replay".into())
        .spawn(move || run_loop(app, stop_flag))
        .map_err(|e| log::warn!("replay: {e}"))
        .ok();
}

/// Switches the recorder off and forgets everything it kept.
pub fn stop(app: Option<&tauri::AppHandle>) {
    if let Ok(mut r) = RUNNER.lock() {
        if let Some(flag) = r.take() {
            flag.store(true, Ordering::SeqCst);
        }
    }
    if let Ok(mut ring) = SHARED.ring.lock() {
        *ring = None;
    }
    publish(app, Status::off());
}

fn run_loop(app: tauri::AppHandle, stop_flag: Arc<AtomicBool>) {
    let mut src = capture::default_source();
    let mut rec = Recorder::new();
    let t0 = Instant::now();
    while !stop_flag.load(Ordering::SeqCst) {
        let cfg = CONFIG.lock().ok().and_then(|c| c.clone()).unwrap_or_else(|| Config { ask: false, excludes: policy::default_excludes(), replay: true, seconds: 30, fps: 1, advisor: false, read_it: false });
        let now = t0.elapsed().as_millis() as u64;
        // The grab happens outside the lock; the ring is touched only for the instant it is changed.
        let (tick, action) = rec.tick(src.as_mut(), &cfg, is_private());
        if let Ok(mut shared) = SHARED.ring.lock() {
            // Switched off while the tick ran (None): the frame is dropped, nothing is left behind.
            if let Some(ring) = shared.as_mut() {
                // Rechecked here, under the lock `set_private`/`stop` also take: a frame in flight cannot slip in after them.
                act(ring, guard(action, is_private(), stop_flag.load(Ordering::SeqCst)), &cfg, now);
            }
        }
        if stop_flag.load(Ordering::SeqCst) {
            break;
        }
        let status = match tick {
            Tick::Kept => Status { on: true, recording: true, paused: None, seconds: cfg.seconds },
            Tick::Paused(why) => Status { on: true, recording: false, paused: Some(why.to_string()), seconds: cfg.seconds },
        };
        publish(Some(&app), status);
        // Wait for the next frame; after repeated failures only look every few seconds (no busy work for a blocked game).
        let wait = if rec.failures() >= 5 { 5000 } else { cfg.interval_ms() };
        let mut slept = 0;
        while slept < wait && !stop_flag.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50.min(wait - slept)));
            slept += 50;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::capture::fake::Fake;
    use super::super::frame::Frame;
    use super::super::policy::WindowInfo;
    use super::*;

    fn cfg() -> Config {
        Config { ask: false, excludes: policy::default_excludes(), replay: true, seconds: 30, fps: 1, advisor: false, read_it: false }
    }

    /// One tick applied to a ring.
    fn step(r: &mut Recorder, ring: &mut Ring, f: &mut Fake, c: &Config, private: bool, now: u64) -> Tick {
        let (t, a) = r.tick(f, c, private);
        act(ring, a, c, now);
        t
    }

    fn setup() -> (Recorder, Ring, Fake, Config) {
        let c = cfg();
        (Recorder::new(), Ring::new(c.seconds, MAX_BYTES), Fake::new("game", "Cities"), c)
    }

    #[test]
    fn frames_are_kept_one_per_tick_and_old_ones_fall_away() {
        let (mut r, mut ring, mut f, c) = setup();
        for s in 0..100u64 {
            assert_eq!(step(&mut r, &mut ring, &mut f, &c, false, s * 1000), Tick::Kept);
        }
        assert_eq!(ring.len(), 31, "30 seconds at 1 fps (+ the newest)");
    }

    #[test]
    fn an_excluded_window_pauses_and_forgets_everything() {
        let (mut r, mut ring, mut f, c) = setup();
        for s in 0..10u64 {
            step(&mut r, &mut ring, &mut f, &c, false, s * 1000);
        }
        assert_eq!(ring.len(), 10);
        f.last_window = Some(WindowInfo { program: "Bitwarden".into(), title: "".into() });
        assert!(matches!(step(&mut r, &mut ring, &mut f, &c, false, 10_000), Tick::Paused(_)));
        assert!(ring.is_empty(), "nothing from before is kept once a password manager comes up");
        let grabs = f.grabs;
        step(&mut r, &mut ring, &mut f, &c, false, 11_000);
        assert_eq!(f.grabs, grabs, "an excluded window is never captured");
    }

    #[test]
    fn own_window_in_front_pauses_but_keeps_what_there_is() {
        let (mut r, mut ring, mut f, c) = setup();
        step(&mut r, &mut ring, &mut f, &c, false, 0);
        f.last_window = Some(WindowInfo { program: "byte".into(), title: "BYTE".into() });
        assert!(matches!(step(&mut r, &mut ring, &mut f, &c, false, 1000), Tick::Paused(_)));
        assert_eq!(ring.len(), 1);
    }

    #[test]
    fn a_private_chat_stops_and_clears() {
        let (mut r, mut ring, mut f, c) = setup();
        step(&mut r, &mut ring, &mut f, &c, false, 0);
        assert_eq!(step(&mut r, &mut ring, &mut f, &c, true, 1000), Tick::Paused("a private chat is open"));
        assert!(ring.is_empty());
        assert_eq!(f.grabs, 1);
    }

    #[test]
    fn a_locked_screen_pauses_without_capturing() {
        let (mut r, mut ring, mut f, c) = setup();
        f.is_locked = true;
        assert!(matches!(step(&mut r, &mut ring, &mut f, &c, false, 0), Tick::Paused(_)));
        assert_eq!(f.grabs, 0);
    }

    #[test]
    fn a_window_that_turns_excluded_while_capturing_is_not_kept() {
        let (mut r, mut ring, mut f, c) = setup();
        f.windows.push_back(Some(WindowInfo { program: "game".into(), title: "x".into() }));
        f.windows.push_back(Some(WindowInfo { program: "KeePass".into(), title: "".into() }));
        assert!(matches!(step(&mut r, &mut ring, &mut f, &c, false, 0), Tick::Paused(_)));
        assert!(ring.is_empty());
    }

    #[test]
    fn repeated_blank_frames_count_as_failures_and_a_good_one_resets() {
        let (mut r, mut ring, mut f, c) = setup();
        for _ in 0..6 {
            f.frames.push_back(Ok(Frame::solid(64, 36, 0, 0, 0)));
        }
        for s in 0..6u64 {
            assert!(matches!(step(&mut r, &mut ring, &mut f, &c, false, s * 1000), Tick::Paused(_)));
        }
        assert!(r.failures() >= 5);
        assert!(ring.is_empty());
        assert_eq!(step(&mut r, &mut ring, &mut f, &c, false, 7000), Tick::Kept);
        assert_eq!(r.failures(), 0);
    }

    #[test]
    fn shrinking_the_length_drops_old_frames_at_the_next_tick() {
        let (mut r, mut ring, mut f, mut c) = setup();
        for s in 0..30u64 {
            step(&mut r, &mut ring, &mut f, &c, false, s * 1000);
        }
        c.seconds = 10;
        step(&mut r, &mut ring, &mut f, &c, false, 30_000);
        assert_eq!(ring.len(), 11);
    }

    #[test]
    fn a_frame_in_flight_is_not_kept_after_a_private_chat_or_a_stop() {
        assert_eq!(guard(Action::Push(vec![1]), true, false), Action::Clear);
        assert_eq!(guard(Action::Push(vec![1]), false, true), Action::Nothing);
        assert_eq!(guard(Action::Push(vec![1]), true, true), Action::Nothing);
        assert_eq!(guard(Action::Push(vec![1]), false, false), Action::Push(vec![1]));
        assert_eq!(guard(Action::Clear, false, true), Action::Clear);
    }

    #[test]
    fn a_picture_taken_from_a_window_that_is_now_excluded_is_dropped_and_released() {
        let (mut r, mut ring, mut f, c) = setup();
        step(&mut r, &mut ring, &mut f, &c, false, 0);
        f.grab_windows.push_back(Some(WindowInfo { program: "Bitwarden".into(), title: "".into() }));
        assert!(matches!(step(&mut r, &mut ring, &mut f, &c, false, 1000), Tick::Paused(_)));
        assert!(ring.is_empty());
        assert!(f.released >= 1);
    }

    #[test]
    fn the_global_switches_work_without_a_screen() {
        set_private(true);
        assert!(is_private());
        assert!(snapshot().is_empty());
        set_private(false);
        stop(None);
        assert_eq!(status(), Status::off());
        assert!(!recording());
    }
}
