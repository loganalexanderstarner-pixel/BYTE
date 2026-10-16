//! Dictate anywhere (Windows): hold the key, speak, let go, and the words are typed into whatever app has the keyboard.
//!
//! The microphone is opened only while the key is held (this helper never listens in the background), the speech is turned
//! into text by the speech model on this computer, and the text is typed with Unicode key events. Nothing is typed into a
//! password field, a window on the never-look-at list, over a full-screen game or in a private chat; newlines and control
//! characters are never typed, so dictation cannot press Enter or run a command.

use crate::screen::policy::WindowInfo;

/// Longest dictation typed in one go, in characters.
pub const MAX_CHARS: usize = 2_000;
/// Longest time the key can be held, in seconds (a stuck key must not record forever).
pub const MAX_SECONDS: u64 = 60;
/// Shortest recording worth sending to the speech model, in samples at 16 kHz.
pub const MIN_SAMPLES: usize = 16_000 * 4 / 10;

pub fn supported() -> bool {
    cfg!(windows)
}

pub fn default_keys() -> String {
    "Control+Alt+KeyD".into()
}

/// The Windows virtual-key code of the last key of a shortcut ("Control+Alt+KeyD" is 0x44), for the keys whose release can be
/// watched: letters, digits, F1 to F12 and Space.
pub fn vk_of(keys: &str) -> Option<u16> {
    let last = keys.split('+').next_back()?.trim();
    if let Some(l) = last.strip_prefix("Key") {
        let c = l.chars().next().filter(|c| c.is_ascii_alphabetic() && l.len() == 1)?;
        return Some(c.to_ascii_uppercase() as u16);
    }
    if let Some(d) = last.strip_prefix("Digit") {
        let c = d.chars().next().filter(|c| c.is_ascii_digit() && d.len() == 1)?;
        return Some(c as u16);
    }
    if let Some(n) = last.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()).filter(|n| (1..=12).contains(n)) {
        return Some(0x6F + n);
    }
    (last == "Space").then_some(0x20)
}

/// What is typed: the words on one line, no control characters, a sensible length.
pub fn sanitize(text: &str) -> String {
    let one: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let tidy = one.split_whitespace().collect::<Vec<_>>().join(" ");
    // Whisper writes bracketed noises for silence and music ("[BLANK_AUDIO]", "(music)"): not words.
    let t = tidy.trim();
    if t.starts_with('[') && t.ends_with(']') || t.starts_with('(') && t.ends_with(')') && t.chars().count() < 30 {
        return String::new();
    }
    t.chars().take(MAX_CHARS).collect()
}

/// May dictation type into the focused app?
pub fn gate(front: Option<&WindowInfo>, excludes: &[String], private: bool, shell: Option<i32>, password_focused: bool) -> Result<(), String> {
    crate::screen::policy::may_capture(front, excludes, private).map_err(|r| r.say())?;
    if crate::screen::read::game_in_front(shell) {
        return Err("A full-screen game or program is in front, so BYTE won't type into it.".into());
    }
    if password_focused {
        return Err("That looks like a password box, so BYTE won't type into it.".into());
    }
    Ok(())
}

/// How the text reaches the app. The real one sends Unicode key events; tests record.
pub trait Typer {
    fn type_text(&mut self, text: &str) -> Result<(), String>;
}

/// Types `text` after the last check, returns how many words went in.
pub fn type_into(typer: &mut dyn Typer, raw: &str) -> Result<usize, String> {
    let text = sanitize(raw);
    if text.is_empty() {
        return Err("BYTE didn't catch any words.".into());
    }
    typer.type_text(&text)?;
    Ok(text.split_whitespace().count())
}

/// The UTF-16 units to send as Unicode key events.
pub fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// A hotkey that cannot be watched for release is replaced by the default.
pub fn valid_keys(keys: &str) -> bool {
    crate::quick::parse_keys(keys).is_ok() && vk_of(keys).is_some()
}

#[cfg(desktop)]
mod flow {
    use super::*;
    use crate::helpers::popup::{self, Kind, Payload};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::{AppHandle, Manager};

    static ACTIVE: AtomicBool = AtomicBool::new(false);

    fn say(app: &AppHandle, title: &str, body: String, hide_ms: u32) {
        popup::show(app, Kind::Explain, Payload { title: title.into(), body, auto_hide: hide_ms, buttons: vec![popup::button("close", "Close")], ..Default::default() }, None);
    }

    pub fn start(app: &AppHandle) {
        if !supported() || ACTIVE.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            run(&app).await;
            ACTIVE.store(false, Ordering::SeqCst);
        });
    }

    #[cfg(windows)]
    async fn run(app: &AppHandle) {
        let state = app.state::<crate::state::AppState>();
        let (on, keys, excludes, model, language) = {
            let s = state.settings.lock().await;
            (s.helper_dictate, s.helper_dictate_keys.clone(), s.screen_exclude_apps.clone(), s.voice_model.clone(), s.voice_language.clone())
        };
        if !on {
            return;
        }
        if let Some(why) = crate::helpers::blocked_now() {
            return say(app, "Dictate", why.into(), 5_000);
        }
        let Some(vk) = vk_of(&keys) else { return say(app, "Dictate", "Pick a letter, number or F key for the dictation shortcut.".into(), 6_000) };
        let front = crate::screen::capture::front_window();
        let focused_password = tokio::task::spawn_blocking(win::password_focused).await.unwrap_or(false);
        if let Err(why) = gate(front.as_ref(), &excludes, false, crate::screen::read::shell_state(), focused_password) {
            return say(app, "Dictate", why, 6_000);
        }
        if crate::voice::pick(&state.paths.models, &model).is_none() {
            return say(app, "Dictate", "Dictation needs a speech model first: Settings > Models > Voice.".into(), 8_000);
        }
        say(app, "Listening", "Speak now. Let go of the key to type it.".into(), 0);
        let rec = crate::helpers::mic::Recording::start(crate::helpers::mic::Source::Mic, Some(MAX_SECONDS as usize + 5));
        let Some(rec) = rec else { return say(app, "Dictate", "BYTE couldn't open the microphone.".into(), 6_000) };
        // Wait for the key to be let go.
        let began = std::time::Instant::now();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            if !win::key_down(vk) || began.elapsed().as_secs() >= MAX_SECONDS {
                break;
            }
        }
        let samples = rec.finish();
        if samples.len() < MIN_SAMPLES {
            return say(app, "Dictate", "That was too short. Hold the key while you speak.".into(), 4_000);
        }
        say(app, "Typing", "Turning your speech into text...".into(), 0);
        let wav = match tempfile::Builder::new().prefix("byte-dictate").suffix(".wav").tempfile() {
            Ok(f) => f.into_temp_path(),
            Err(e) => return say(app, "Dictate", e.to_string(), 6_000),
        };
        if let Err(e) = std::fs::write(&wav, crate::helpers::mic::wav16(&samples)) {
            return say(app, "Dictate", e.to_string(), 6_000);
        }
        let heard = crate::voice::transcribe(Some(app), &state.paths.models, &model, &language, &wav).await;
        // The audio is not kept.
        drop(wav);
        drop(samples);
        let text = match heard {
            Ok(t) => t,
            Err(e) => return say(app, "Dictate", e.to_string(), 6_000),
        };
        // The window may have changed while BYTE was listening: check again before typing.
        let front = crate::screen::capture::front_window();
        let focused_password = tokio::task::spawn_blocking(win::password_focused).await.unwrap_or(false);
        if let Err(why) = gate(front.as_ref(), &excludes, false, crate::screen::read::shell_state(), focused_password) {
            return say(app, "Dictate", why, 6_000);
        }
        popup::hide(app);
        let result = tokio::task::spawn_blocking(move || {
            if !win::wait_modifiers_up() {
                return Err("Let go of Ctrl, Alt and Shift, then try again.".to_string());
            }
            type_into(&mut win::SendInputTyper, &text)
        })
        .await
        .unwrap_or_else(|_| Err("BYTE couldn't type that.".into()));
        match result {
            Ok(n) => say(app, "Typed", format!("{n} word{}", if n == 1 { "" } else { "s" }), 1_800),
            Err(e) => say(app, "Dictate", e, 5_000),
        }
    }

    #[cfg(not(windows))]
    async fn run(_app: &AppHandle) {}

    #[cfg(windows)]
    pub(super) mod win {
        use super::super::units;
        use super::super::Typer;
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
        use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};

        pub fn key_down(vk: u16) -> bool {
            (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16) & 0x8000 != 0
        }

        pub fn wait_modifiers_up() -> bool {
            for _ in 0..100 {
                if ![VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|k| key_down(k.0)) {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            false
        }

        /// The control that has the keyboard is a password box (UI Automation says so). Unknown counts as not a password box.
        pub fn password_focused() -> bool {
            if crate::control::com::ensure().is_err() {
                return false;
            }
            let Ok(auto) = (unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }) else { return false };
            let Ok(el) = (unsafe { auto.GetFocusedElement() }) else { return false };
            let flagged = unsafe { el.CurrentIsPassword() }.map(|b| b.as_bool()).unwrap_or(false);
            let name = unsafe { el.CurrentName() }.map(|b| b.to_string()).unwrap_or_default();
            flagged || crate::control::uia::sensitive_name(&name)
        }

        pub struct SendInputTyper;

        fn unit(u: u16, up: bool) -> INPUT {
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: u, dwFlags: if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE }, time: 0, dwExtraInfo: 0 } },
            }
        }

        impl Typer for SendInputTyper {
            fn type_text(&mut self, text: &str) -> Result<(), String> {
                // In small batches, so a long text does not flood the target program.
                for chunk in units(text).chunks(40) {
                    let inputs: Vec<INPUT> = chunk.iter().flat_map(|&u| [unit(u, false), unit(u, true)]).collect();
                    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
                    if sent as usize != inputs.len() {
                        return Err("BYTE couldn't type into that app. If it runs as administrator, Windows blocks this.".into());
                    }
                }
                Ok(())
            }
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            /// Real Win32 calls that need no one at the keyboard.
            #[test]
            fn key_state_and_the_focus_check_run_without_crashing() {
                eprintln!("ctrl down now: {}", key_down(VK_CONTROL.0));
                eprintln!("password focused now: {}", password_focused());
            }
        }
    }
}

#[cfg(desktop)]
pub fn start(app: &tauri::AppHandle) {
    flow::start(app);
}
#[cfg(not(desktop))]
pub fn start(_app: &tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        typed: Vec<String>,
        fail: bool,
    }
    impl Typer for Fake {
        fn type_text(&mut self, text: &str) -> Result<(), String> {
            if self.fail {
                return Err("blocked".into());
            }
            self.typed.push(text.into());
            Ok(())
        }
    }

    fn win(p: &str, t: &str) -> WindowInfo {
        WindowInfo { program: p.into(), title: t.into() }
    }

    #[test]
    fn the_key_to_watch_is_found_for_letters_digits_f_keys_and_space() {
        assert_eq!(vk_of("Control+Alt+KeyD"), Some(0x44));
        assert_eq!(vk_of("Alt+Super+KeyZ"), Some(0x5A));
        assert_eq!(vk_of("Control+Digit5"), Some(0x35));
        assert_eq!(vk_of("Control+Alt+F8"), Some(0x77));
        assert_eq!(vk_of("Control+Alt+Space"), Some(0x20));
        assert_eq!(vk_of("Control+Alt+F13"), None);
        assert_eq!(vk_of("Control+Alt+ArrowUp"), None);
        assert_eq!(vk_of(""), None);
    }

    #[test]
    fn the_default_keys_are_usable_and_others_are_not_when_unwatchable() {
        assert!(valid_keys(&default_keys()));
        assert!(!valid_keys("Control+Alt+ArrowUp"));
        assert!(!valid_keys("KeyD"));
    }

    #[test]
    fn dictated_text_is_one_line_without_control_characters() {
        assert_eq!(sanitize("  hello\nthere\r\n\tfriend  "), "hello there friend");
        assert_eq!(sanitize("line one\u{7}\u{1b}[31m two"), "line one [31m two");
        assert_eq!(sanitize("[BLANK_AUDIO]"), "");
        assert_eq!(sanitize("(music)"), "");
        assert_eq!(sanitize("(a long and real sentence in brackets that is a thought)").is_empty(), false);
        assert_eq!(sanitize(&"word ".repeat(1000)).chars().count(), MAX_CHARS);
        assert_eq!(sanitize("café 你好"), "café 你好");
    }

    #[test]
    fn nothing_is_typed_where_it_must_not_be() {
        let d = crate::screen::policy::default_excludes();
        assert!(gate(Some(&win("notepad", "x")), &d, false, Some(1), false).is_ok());
        assert!(gate(Some(&win("notepad", "x")), &d, false, Some(1), true).unwrap_err().contains("password"));
        assert!(gate(Some(&win("1Password", "Vault")), &d, false, None, false).is_err());
        assert!(gate(Some(&win("notepad", "x")), &d, true, None, false).unwrap_err().contains("private chat"));
        assert!(gate(Some(&win("game", "x")), &d, false, Some(3), false).unwrap_err().contains("full-screen"));
        assert!(gate(None, &d, false, None, false).is_err());
    }

    #[test]
    fn typing_goes_through_the_typer_once_and_counts_words() {
        let mut t = Fake::default();
        assert_eq!(type_into(&mut t, "  open the\nwindow "), Ok(3));
        assert_eq!(t.typed, vec!["open the window".to_string()]);
        assert!(type_into(&mut t, "[BLANK_AUDIO]").is_err());
        assert_eq!(t.typed.len(), 1);
        let mut f = Fake { fail: true, ..Default::default() };
        assert_eq!(type_into(&mut f, "hi"), Err("blocked".into()));
    }

    #[test]
    fn unicode_text_becomes_utf16_units_including_pairs() {
        assert_eq!(units("hi"), vec![104, 105]);
        assert_eq!(units("😀").len(), 2);
    }
}
