//! Live captions and "what did they just say" (Windows): BYTE listens to the sound the computer plays (WASAPI loopback), keeps the
//! last 30 seconds in memory only, writes captions into a small window while they are on, and can say what was just said. It
//! starts only when the person clicks "Start live captions" in the tray menu, shows a window while it listens, and nothing is
//! saved: the sound is memory, and the words are shown and dropped.

use std::collections::VecDeque;
use std::sync::Mutex;

/// How much sound "what did they just say" looks back over.
pub const LOOKBACK_SECONDS: usize = 30;
/// How often new sound is turned into words.
pub const EVERY_SECONDS: u64 = 5;
/// Caption lines kept in the window.
pub const LINES: usize = 4;

pub fn supported() -> bool {
    cfg!(windows)
}

/// Adds a heard piece to the lines, dropping the oldest. Empty pieces and noises like "[BLANK_AUDIO]" add nothing.
pub fn push_line(lines: &mut VecDeque<String>, heard: &str) {
    let t = super::dictate::sanitize(heard);
    if t.is_empty() || lines.back().is_some_and(|l| *l == t) {
        return;
    }
    lines.push_back(t);
    while lines.len() > LINES {
        lines.pop_front();
    }
}

pub fn render(lines: &VecDeque<String>) -> String {
    if lines.is_empty() { "Listening...".into() } else { lines.iter().cloned().collect::<Vec<_>>().join("\n") }
}

struct Live {
    rec: super::mic::Recording,
    lines: VecDeque<String>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

pub fn listening() -> bool {
    LIVE.lock().map(|l| l.is_some()).unwrap_or(false)
}

#[cfg(desktop)]
mod flow {
    use super::*;
    use crate::helpers::mic::{self, Recording, Source};
    use crate::helpers::popup::{self, Kind, Payload};
    use tauri::{AppHandle, Manager};

    fn show(app: &AppHandle, title: &str, body: String, stop: bool, hide: u32) {
        let buttons = if stop { vec![popup::button("stop", "Stop captions")] } else { vec![popup::button("close", "Close")] };
        popup::show(app, Kind::Captions, Payload { title: title.into(), body, buttons, auto_hide: hide, ..Default::default() }, None);
    }

    pub async fn start(app: &AppHandle) {
        let state = app.state::<crate::state::AppState>();
        let (on, model) = {
            let s = state.settings.lock().await;
            (s.helper_captions, s.voice_model.clone())
        };
        if !on || listening() {
            return;
        }
        if let Some(why) = crate::helpers::blocked_now() {
            return show(app, "Live captions", why.into(), false, 6_000);
        }
        if crate::voice::pick(&state.paths.models, &model).is_none() {
            return show(app, "Live captions", "Captions need a speech model first: Settings > Models > Voice.".into(), false, 8_000);
        }
        let Some(rec) = Recording::start(Source::Loopback, Some(LOOKBACK_SECONDS + 10)) else {
            return show(app, "Live captions", "BYTE couldn't listen to the sound this computer plays.".into(), false, 8_000);
        };
        let from = rec.total();
        if let Ok(mut l) = LIVE.lock() {
            *l = Some(Live { rec, lines: VecDeque::new() });
        }
        crate::helpers::set_captions_menu(app, true);
        show(app, "Live captions (listening to this computer's sound, kept in memory)", render(&VecDeque::new()), true, 0);
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut from = from;
            while listening() {
                tokio::time::sleep(std::time::Duration::from_secs(EVERY_SECONDS)).await;
                let Some((chunk, now)) = LIVE.lock().ok().and_then(|l| l.as_ref().map(|l| l.rec.since(from))) else { break };
                from = now;
                if mic::is_quiet(&chunk) {
                    continue;
                }
                if let Some(text) = transcribe(&app2, &chunk).await {
                    let body = LIVE.lock().ok().and_then(|mut l| {
                        l.as_mut().map(|l| {
                            push_line(&mut l.lines, &text);
                            render(&l.lines)
                        })
                    });
                    if let Some(b) = body {
                        show(&app2, "Live captions (listening to this computer's sound, kept in memory)", b, true, 0);
                    }
                }
            }
        });
    }

    async fn transcribe(app: &AppHandle, samples: &[i16]) -> Option<String> {
        let state = app.state::<crate::state::AppState>();
        let (model, language) = {
            let s = state.settings.lock().await;
            (s.voice_model.clone(), s.voice_language.clone())
        };
        let wav = tempfile::Builder::new().prefix("byte-captions").suffix(".wav").tempfile().ok()?.into_temp_path();
        std::fs::write(&wav, mic::wav16(samples)).ok()?;
        let out = crate::voice::transcribe(Some(app), &state.paths.models, &model, &language, &wav).await.ok();
        drop(wav);
        out
    }

    pub async fn stop(app: &AppHandle) {
        let live = LIVE.lock().ok().and_then(|mut l| l.take());
        if let Some(l) = live {
            let _ = l.rec.finish();
        }
        crate::helpers::set_captions_menu(app, false);
        popup::hide(app);
    }

    /// "What did they just say?": the last 30 seconds, turned into words once, from the memory buffer.
    pub async fn recap(app: &AppHandle) {
        let samples = LIVE.lock().ok().and_then(|l| l.as_ref().map(|l| l.rec.last(LOOKBACK_SECONDS)));
        let Some(samples) = samples else {
            return show(app, "What did they just say?", "Live captions are not on. Start them from the tray menu first; BYTE can only look back over sound it was listening to.".into(), false, 10_000);
        };
        if mic::is_quiet(&samples) {
            return show(app, "What did they just say?", "Nothing was playing in the last 30 seconds.".into(), false, 6_000);
        }
        show(app, "What did they just say?", "Listening back over the last 30 seconds...".into(), false, 0);
        let text = transcribe(app, &samples).await.map(|t| super::super::dictate::sanitize(&t)).filter(|t| !t.is_empty());
        let body = text.unwrap_or_else(|| "BYTE couldn't make out any words in the last 30 seconds.".into());
        show(app, "In the last 30 seconds", body, listening(), 0);
    }

    pub async fn clicked(app: &AppHandle, id: &str) {
        if id == "stop" {
            stop(app).await;
        } else if listening() {
            // Back to the captions.
            let body = LIVE.lock().ok().and_then(|l| l.as_ref().map(|l| render(&l.lines))).unwrap_or_default();
            show(app, "Live captions (listening to this computer's sound, kept in memory)", body, true, 0);
        } else {
            popup::hide(app);
        }
    }
}

#[cfg(desktop)]
pub fn start(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { flow::start(&app).await });
}
#[cfg(desktop)]
pub fn stop(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { flow::stop(&app).await });
}
#[cfg(desktop)]
pub fn recap(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { flow::recap(&app).await });
}
#[cfg(desktop)]
pub async fn clicked(app: &tauri::AppHandle, id: &str) {
    flow::clicked(app, id).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_lines_keep_the_newest_and_skip_noise_and_repeats() {
        let mut l = VecDeque::new();
        for t in ["one", "[BLANK_AUDIO]", "two", "two", "(music)", "three", "four", "five"] {
            push_line(&mut l, t);
        }
        assert_eq!(l.iter().cloned().collect::<Vec<_>>(), vec!["two", "three", "four", "five"]);
        assert_eq!(render(&l), "two\nthree\nfour\nfive");
        assert_eq!(render(&VecDeque::new()), "Listening...");
    }

    #[test]
    fn the_numbers_are_sensible() {
        assert_eq!(LOOKBACK_SECONDS, 30);
        assert!(EVERY_SECONDS >= 3 && LINES >= 2);
    }
}
