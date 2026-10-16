//! Read aloud with follow-along highlighting: select text anywhere, press the shortcut, and BYTE reads it in a small window that
//! highlights the sentence being spoken, at the speed you chose (slow, normal or fast). Press the shortcut again, or Stop, to end it.
//! The voice is the one chosen for BYTE (downloaded voices, on this computer).

use std::sync::atomic::{AtomicBool, Ordering};

pub const SPEEDS: &[&str] = &["slow", "normal", "fast"];

pub fn default_speed() -> String {
    "normal".into()
}

pub fn default_keys() -> String {
    if cfg!(windows) { "Control+Alt+KeyL".into() } else { "Alt+Super+KeyL".into() }
}

pub fn supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

/// A chosen speed, or normal for anything else.
pub fn speed_or_normal(s: &str) -> &'static str {
    SPEEDS.iter().find(|x| x.eq_ignore_ascii_case(s.trim())).copied().unwrap_or("normal")
}

/// The sentences of `text` with their positions (in characters), in order. Positions are found one after the other, so repeated
/// sentences each get their own place.
pub fn sentence_spans(text: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut byte_cursor = 0usize;
    for s in crate::tts::sentences(text) {
        let Some(rel) = text[byte_cursor..].find(&s) else { continue };
        let start_b = byte_cursor + rel;
        let end_b = start_b + s.len();
        let start = text[..start_b].chars().count();
        let end = start + s.chars().count();
        out.push((s, start, end));
        byte_cursor = end_b;
    }
    out
}

static STOP: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

#[cfg(desktop)]
mod flow {
    use super::*;
    use crate::helpers::popup::{self, Kind, Payload};
    use tauri::{AppHandle, Manager};

    fn say(app: &AppHandle, body: String) {
        popup::show(app, Kind::Read, Payload { title: "Read aloud".into(), body, buttons: vec![popup::button("close", "Close")], auto_hide: 10_000, ..Default::default() }, None);
    }

    pub fn toggle(app: &AppHandle) {
        if ACTIVE.load(Ordering::SeqCst) {
            stop(app);
            return;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move { run(&app).await });
    }

    pub fn stop(app: &AppHandle) {
        STOP.store(true, Ordering::SeqCst);
        crate::tts::stop();
        if !ACTIVE.load(Ordering::SeqCst) {
            popup::hide(app);
        }
    }

    async fn run(app: &AppHandle) {
        let state = app.state::<crate::state::AppState>();
        let (on, excludes, speed, voice, style) = {
            let s = state.settings.lock().await;
            (s.helper_read, s.screen_exclude_apps.clone(), speed_or_normal(&s.helper_read_speed), s.byte_voice.clone(), s.speech_style.clone())
        };
        if !on {
            return;
        }
        if let Some(why) = crate::helpers::blocked_now() {
            return say(app, why.into());
        }
        if !crate::tts::usable(&state.paths.models, &voice) {
            return say(app, "Download one of BYTE's voices first: Settings > Voice and Hey BYTE.".into());
        }
        #[cfg(windows)]
        {
            let front = crate::screen::capture::front_window();
            if let Err(why) = crate::helpers::explain::gate(front.as_ref(), &excludes, false, crate::screen::read::shell_state()) {
                return say(app, why);
            }
        }
        #[cfg(not(windows))]
        let _ = &excludes;
        crate::clipboard::pause_history(true);
        #[cfg(windows)]
        let got = crate::selection::capture_windows().await;
        #[cfg(not(windows))]
        let got = crate::selection::capture(&crate::macctl::MacRunner).await;
        crate::clipboard::pause_history(false);
        let text = match got {
            Ok(c) => c.text,
            Err(e) => return say(app, e.to_string()),
        };
        let (text, cut) = crate::screen::read::prepare_text(&text, crate::screen::read::MOST_CHARS);
        let spans = sentence_spans(&text);
        if spans.is_empty() {
            return say(app, "There was nothing to read.".into());
        }
        ACTIVE.store(true, Ordering::SeqCst);
        STOP.store(false, Ordering::SeqCst);
        let how = crate::tts::Delivery { voice, speed: speed.to_string(), style, cloud: None };
        let title = format!("Reading aloud ({speed}){}", if cut { ", the first part" } else { "" });
        for (i, (sentence, start, end)) in spans.iter().enumerate() {
            if STOP.load(Ordering::SeqCst) {
                break;
            }
            popup::show(app, Kind::Read, Payload { title: title.clone(), body: text.clone(), mark: Some([*start, *end]), buttons: vec![popup::button("stop", "Stop")], ..Default::default() }, None);
            let id = format!("byte_read_{}_{i}", uuid::Uuid::new_v4().simple());
            if let Err(e) = crate::tts::feed(app, &state.paths.models, &how, &id, sentence, true) {
                say(app, e.to_string());
                break;
            }
            // Until this sentence has been spoken (the voice reports "speaking" from the moment a sentence is handed over).
            let began = std::time::Instant::now();
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                if STOP.load(Ordering::SeqCst) || (!crate::tts::speaking() && began.elapsed() > std::time::Duration::from_millis(300)) {
                    break;
                }
                if began.elapsed() > std::time::Duration::from_secs(120) {
                    break;
                }
            }
        }
        let stopped = STOP.load(Ordering::SeqCst);
        ACTIVE.store(false, Ordering::SeqCst);
        crate::tts::stop();
        if stopped {
            popup::hide(app);
        } else {
            popup::show(app, Kind::Read, Payload { title: "Finished reading".into(), body: text, buttons: vec![popup::button("close", "Close")], auto_hide: 3_000, ..Default::default() }, None);
        }
    }

    pub fn clicked(app: &AppHandle, id: &str) {
        if id == "stop" {
            stop(app);
        } else {
            popup::hide(app);
        }
    }
}

#[cfg(desktop)]
pub fn toggle(app: &tauri::AppHandle) {
    flow::toggle(app);
}
#[cfg(desktop)]
pub async fn clicked(app: &tauri::AppHandle, id: &str) {
    flow::clicked(app, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_get_their_own_positions_in_order() {
        let text = "First one. Second one! Third? First one.";
        let spans = sentence_spans(text);
        assert_eq!(spans.len(), 4);
        let chars: Vec<char> = text.chars().collect();
        for (s, a, b) in &spans {
            assert_eq!(chars[*a..*b].iter().collect::<String>(), *s);
        }
        // The repeated sentence is found at its second place, not its first.
        assert!(spans[3].1 > spans[2].1);
    }

    #[test]
    fn positions_count_characters_not_bytes() {
        let text = "Café été. Voilà ça!";
        let spans = sentence_spans(text);
        let chars: Vec<char> = text.chars().collect();
        assert_eq!(spans.len(), 2);
        assert_eq!(chars[spans[1].1..spans[1].2].iter().collect::<String>(), "Voilà ça!");
    }

    #[test]
    fn speeds_are_one_of_three() {
        assert_eq!(speed_or_normal("SLOW"), "slow");
        assert_eq!(speed_or_normal(" fast "), "fast");
        assert_eq!(speed_or_normal("warp"), "normal");
        assert_eq!(default_speed(), "normal");
    }

    #[test]
    fn nothing_to_read_gives_no_spans() {
        assert!(sentence_spans("   ").is_empty());
    }
}
