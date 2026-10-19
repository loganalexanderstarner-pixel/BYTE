//! The voice track's controls around "Hey BYTE" (docs/HEY-BYTE-ASSISTANT.md): the listening indicator, the chime choice, the "wait until I'm
//! done" switch, barge-in and voice focus settings, and the global talk key. Everything that is a decision lives in a small pure
//! function with a test; the rest only moves values between the settings and the audio threads.

#![cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]

use crate::platform_text::SeeThrough;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::settings::Settings;

// ------------------------------------------------------------------ what the pill shows

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Listening,
    Thinking,
    Speaking,
    /// In a call: "Hey BYTE" is off (wake.rs `watch_meeting`), so the person is not left wondering why BYTE does not answer.
    Call,
}

impl Phase {
    pub fn word(self) -> &'static str {
        match self {
            Phase::Idle => "",
            Phase::Listening => "listening",
            Phase::Thinking => "thinking",
            Phase::Speaking => "speaking",
            Phase::Call => "call",
        }
    }
}

/// What the pill shows for what the audio side knows. Speaking wins (BYTE is talking), then listening for the turn, then thinking.
pub fn phase_for(in_call: bool, speaking: bool, capturing: bool, thinking: bool) -> Phase {
    if in_call && !speaking && !capturing {
        Phase::Call
    } else if speaking {
        Phase::Speaking
    } else if capturing {
        Phase::Listening
    } else if thinking {
        Phase::Thinking
    } else {
        Phase::Idle
    }
}

/// How long "thinking" is shown without an answer being spoken (a silent answer, an error): then the pill goes away.
pub const THINKING_MAX: Duration = Duration::from_secs(25);

static WAKE_ON: AtomicBool = AtomicBool::new(false);
static INDICATOR: AtomicBool = AtomicBool::new(true);
static WAIT_MORE: AtomicBool = AtomicBool::new(true);
static CHIME: Mutex<String> = Mutex::new(String::new());
static THINKING_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
static POLLER: std::sync::Once = std::sync::Once::new();
static PHASE_SEQ: AtomicU64 = AtomicU64::new(0);

/// A turn was handed to BYTE: it is working on the answer.
pub fn set_thinking(on: bool) {
    if let Ok(mut t) = THINKING_SINCE.lock() {
        *t = on.then(Instant::now);
    }
}

fn thinking() -> bool {
    THINKING_SINCE.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < THINKING_MAX)
}

/// Whether finishing a turn should start the short "anything more?" listen (setting `waitUntilDone`).
pub fn wait_more_on() -> bool {
    WAIT_MORE.load(Ordering::Relaxed)
}

/// Takes the settings that this module follows. Called at start-up and after every settings change.
pub fn apply(app: &AppHandle, s: &Settings) {
    WAKE_ON.store(s.wake_word, Ordering::Relaxed);
    INDICATOR.store(s.wake_indicator, Ordering::Relaxed);
    WAIT_MORE.store(s.wait_until_done, Ordering::Relaxed);
    if let Ok(mut c) = CHIME.lock() {
        *c = s.wake_chime.clone();
    }
    crate::bargein::set_mode(&s.barge_in);
    crate::aec::set_wanted(s.aec_loopback); // track:aec
    crate::wakemodel::set_mode(&s.wake_detector); // track:wakemodel
    crate::voicefocus::set_mode(&s.voice_focus);
    #[cfg(any(target_os = "macos", windows))]
    start_poller(app);
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = app;
}

#[cfg(any(target_os = "macos", windows))]
fn start_poller(app: &AppHandle) {
    let app = app.clone();
    POLLER.call_once(move || {
        std::thread::spawn(move || {
            let mut shown = Phase::Idle;
            loop {
                let active = WAKE_ON.load(Ordering::Relaxed) && INDICATOR.load(Ordering::Relaxed);
                if !active {
                    if shown != Phase::Idle {
                        shown = Phase::Idle;
                        pill(&app, Phase::Idle);
                    }
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
                let speaking = crate::speech::speaking();
                let capturing = crate::wake::capturing_now();
                if speaking || capturing {
                    set_thinking(false);
                }
                let now = phase_for(crate::wake::meeting_quiet(), speaking, capturing, thinking());
                if now != shown {
                    shown = now;
                    PHASE_SEQ.fetch_add(1, Ordering::Relaxed);
                    pill(&app, now);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
    });
}

/// Shows or hides the pill (Windows only: a transparent always-on-top window needs a platform switch on a Mac that this build doesn't use).
#[cfg(windows)]
fn pill(app: &AppHandle, phase: Phase) {
    use tauri::{Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
    const LABEL: &str = "wakepill";
    let w = match app.get_webview_window(LABEL) {
        Some(w) => w,
        None => {
            if phase == Phase::Idle {
                return;
            }
            let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("indicator.html".into()))
                .title("BYTE listening")
                .inner_size(230.0, 36.0)
                .decorations(false)
                .see_through()
                .shadow(false)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focusable(false)
                .focused(false)
                .visible(false)
                .build();
            match built {
                Ok(w) => {
                    let _ = w.set_ignore_cursor_events(true);
                    if let Ok(Some(m)) = app.primary_monitor() {
                        let scale = w.scale_factor().unwrap_or(1.0);
                        let (mp, ms) = (m.position(), m.size());
                        let x = mp.x as f64 + (ms.width as f64 - 230.0 * scale) / 2.0;
                        let _ = w.set_position(PhysicalPosition::new(x, mp.y as f64 + 6.0 * scale));
                    }
                    w
                }
                Err(e) => return log::warn!("listening indicator: {e}"),
            }
        }
    };
    if phase == Phase::Idle {
        let _ = w.eval("window.setPill && window.setPill('')");
        // Fade first, then hide, unless another state came in meanwhile.
        let seq = PHASE_SEQ.load(Ordering::Relaxed);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            if PHASE_SEQ.load(Ordering::Relaxed) == seq {
                let _ = w.hide();
            }
        });
    } else {
        let _ = w.show();
        let _ = w.eval(format!("window.setPill && window.setPill('{}')", phase.word()));
    }
}

#[cfg(not(windows))]
fn pill(_app: &AppHandle, _phase: Phase) {}

// ------------------------------------------------------------------ the talk key

static TALK_ID: Mutex<Option<u32>> = Mutex::new(None);
/// Why the talk key could not be registered ("" = fine or not asked for).
static TALK_ERR: Mutex<String> = Mutex::new(String::new());

/// What is wrong with the talk key, if anything (for the settings row).
#[tauri::command]
pub fn talk_key_status() -> String {
    TALK_ERR.lock().map(|e| e.clone()).unwrap_or_default()
}

/// Registers the talk key from the settings (called by `quick::apply_shortcuts` after it cleared every shortcut). `taken`: the ids
/// already used by the other shortcuts. Needs "Hey BYTE" listening on, since that is the microphone stream the turn is cut from.
#[cfg(desktop)]
pub fn register_talk(app: &AppHandle, s: &Settings, taken: &[Option<u32>]) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let mut id = None;
    let mut err = String::new();
    if s.wake_word && !s.talk_hotkey.trim().is_empty() {
        match crate::quick::parse_keys(&s.talk_hotkey) {
            Ok(sc) if taken.contains(&Some(sc.id())) => err = format!("{} is already another BYTE shortcut.", crate::quick::pretty(&s.talk_hotkey)),
            Ok(sc) => match app.global_shortcut().register(sc) {
                Ok(()) => id = Some(sc.id()),
                Err(e) => {
                    log::warn!("couldn't register {}: {e}", s.talk_hotkey);
                    err = format!("{} couldn't be used (another program may have it).", crate::quick::pretty(&s.talk_hotkey));
                }
            },
            Err(e) => err = e,
        }
    }
    if let Ok(mut e) = TALK_ERR.lock() {
        *e = err;
    }
    if let Ok(mut t) = TALK_ID.lock() {
        *t = id;
    }
}

pub fn is_talk(id: u32) -> bool {
    TALK_ID.lock().map(|t| *t == Some(id)).unwrap_or(false)
}

/// The talk key was pressed: BYTE stops talking if it was, and listens for a turn.
pub fn talk_pressed(app: &AppHandle) {
    use tauri::Manager;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::tts::speaking() {
            crate::tts::stop();
            let _ = tauri::Emitter::emit(&app, crate::speech::DONE_EVENT, ());
        }
        let show = app.state::<crate::state::AppState>().settings.lock().await.wake_show_window;
        crate::wake::talk_now(&app, show);
    });
}

// ------------------------------------------------------------------ the chime

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chime {
    System,
    Soft,
    Off,
}

pub fn chime_for(setting: &str) -> Chime {
    match setting {
        "soft" => Chime::Soft,
        "off" => Chime::Off,
        _ => Chime::System,
    }
}

/// How long the chosen chime lasts, so the turn's capture can ignore it (the system sound is about 450 ms).
pub fn chime_skip_ms(c: Chime) -> u64 {
    match c {
        Chime::System => 450,
        Chime::Soft => SOFT_MS + 80,
        Chime::Off => 0,
    }
}

pub const SOFT_MS: u64 = 220;

/// A short, quiet two-note chime at `rate` Hz: 660 Hz then 880 Hz with a smooth fade in and out, peaking at 0.12 of full scale.
pub fn soft_chime_samples(rate: u32) -> Vec<f32> {
    let n = (rate as u64 * SOFT_MS / 1000) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let f = if i < n / 2 { 660.0 } else { 880.0 };
            // A raised-sine envelope over each note.
            let half = n / 2;
            let pos = (i % half.max(1)) as f32 / half.max(1) as f32;
            let env = (std::f32::consts::PI * pos).sin().powi(2);
            (2.0 * std::f32::consts::PI * f * t).sin() * 0.12 * env
        })
        .collect()
}

pub fn current_chime() -> Chime {
    chime_for(CHIME.lock().map(|c| c.clone()).unwrap_or_default().as_str())
}

/// Plays the chime the person chose; returns how many ms of the audio that follows to ignore.
pub fn play_chime() -> u64 {
    let c = current_chime();
    match c {
        Chime::Off => {}
        Chime::System => system_chime(),
        Chime::Soft => soft_chime(),
    }
    chime_skip_ms(c)
}

fn system_chime() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/afplay").arg("/System/Library/Sounds/Tink.aiff").spawn();
    // Windows' own notification sound; MessageBeep returns at once and plays in the background.
    #[cfg(windows)]
    {
        use windows::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
        let _ = unsafe { MessageBeep(MB_ICONASTERISK) };
    }
}

#[cfg(any(target_os = "macos", windows))]
fn soft_chime() {
    std::thread::spawn(|| {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let Some(dev) = cpal::default_host().default_output_device() else { return };
        let Ok(cfg) = dev.default_output_config() else { return };
        let (rate, channels) = (cfg.sample_rate(), cfg.channels() as usize);
        let samples = soft_chime_samples(rate);
        let mut i = 0usize;
        let stream = dev.build_output_stream(
            cfg.into(),
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in out.chunks_mut(channels.max(1)) {
                    let s = samples.get(i).copied().unwrap_or(0.0);
                    i += 1;
                    frame.iter_mut().for_each(|x| *x = s);
                }
            },
            |e| log::warn!("chime: {e}"),
            None,
        );
        if let Ok(s) = stream {
            if s.play().is_ok() {
                std::thread::sleep(Duration::from_millis(SOFT_MS + 150));
            }
        }
    });
}

#[cfg(not(any(target_os = "macos", windows)))]
fn soft_chime() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_follows_what_is_happening() {
        assert_eq!(phase_for(false, false, false, false), Phase::Idle);
        assert_eq!(phase_for(false, false, true, false), Phase::Listening);
        assert_eq!(phase_for(false, false, false, true), Phase::Thinking);
        assert_eq!(phase_for(false, true, false, true), Phase::Speaking);
        assert_eq!(phase_for(false, true, true, false), Phase::Speaking);
        // In a call the pill says that "Hey BYTE" is off; a talk-key turn or speech still shows its own state.
        assert_eq!(phase_for(true, false, false, false), Phase::Call);
        assert_eq!(phase_for(true, false, true, false), Phase::Listening);
        assert_eq!(Phase::Call.word(), "call");
        assert_eq!(Phase::Idle.word(), "");
        assert_eq!(Phase::Thinking.word(), "thinking");
    }

    #[test]
    fn chime_choices() {
        assert_eq!(chime_for("system"), Chime::System);
        assert_eq!(chime_for("soft"), Chime::Soft);
        assert_eq!(chime_for("off"), Chime::Off);
        assert_eq!(chime_for("nonsense"), Chime::System);
        assert_eq!(chime_skip_ms(Chime::Off), 0);
        assert!(chime_skip_ms(Chime::Soft) > SOFT_MS);
    }

    #[test]
    fn the_soft_chime_is_short_quiet_and_starts_and_ends_at_zero() {
        for rate in [16_000u32, 44_100, 48_000] {
            let s = soft_chime_samples(rate);
            assert_eq!(s.len() as u64, rate as u64 * SOFT_MS / 1000);
            assert!(s.iter().all(|x| x.abs() <= 0.121));
            assert!(s[0].abs() < 0.01 && s[s.len() - 1].abs() < 0.01);
            assert!(s.iter().any(|x| x.abs() > 0.05));
        }
    }
}
