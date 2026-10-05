//! Spoken replies (Phase 11): BYTE reads answers aloud with macOS's own voices
//! (`/usr/bin/say`: offline, no download). The text goes through a temp file
//! (`-f`), never as an argument; one reply plays at a time and Stop ends it.
//! `speech://done` tells the hands-free loop when BYTE has finished talking.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const DONE_EVENT: &str = "speech://done";
/// Longest stretch read aloud; the rest stays on screen.
const MAX_CHARS: usize = 1500;

/// The `say` process playing now (killed by Stop or by the next reply).
static PLAYING: Mutex<Option<u32>> = Mutex::new(None);

/// What of an answer is worth hearing: words, without markdown, code, tables, links or citation marks.
pub fn speakable(markdown: &str) -> String {
    // Paragraphs (separated by a blank line in the result, so the voice pauses there) of sentences.
    let mut paras: Vec<Vec<String>> = vec![vec![]];
    let mut in_code = false;
    let mut in_table = false;
    let mut item = 0usize;
    let new_para = |paras: &mut Vec<Vec<String>>| {
        if paras.last().is_some_and(|p| !p.is_empty()) {
            paras.push(vec![]);
        }
    };
    for raw in markdown.lines() {
        let line = raw.trim();
        if line.starts_with("```") {
            if !in_code {
                new_para(&mut paras);
                paras.last_mut().unwrap().push("There's code on screen.".into());
                new_para(&mut paras);
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if line.starts_with('|') {
            if !in_table {
                new_para(&mut paras);
                paras.last_mut().unwrap().push("There's a table on screen.".into());
                new_para(&mut paras);
                in_table = true;
            }
            continue;
        }
        in_table = false;
        if line.is_empty() {
            new_para(&mut paras);
            continue;
        }
        if line.starts_with("**Confidence:**") || line.chars().all(|c| matches!(c, '-' | '*' | '_' | ' ')) {
            continue;
        }
        let heading = line.starts_with('#');
        if heading {
            new_para(&mut paras);
        }
        let line = line.trim_start_matches('#').trim_start_matches('>').trim();
        let line = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).unwrap_or(line);
        // "1. Do this" → "First, do this."
        let numbered = line.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit()));
        let text = match numbered {
            Some((_, rest)) => {
                item += 1;
                let rest = plain(rest);
                let mut chars = rest.chars();
                let lowered = chars.next().map(|c| c.to_lowercase().collect::<String>() + chars.as_str()).unwrap_or_default();
                format!("{}, {lowered}", ordinal(item))
            }
            None => {
                item = 0;
                plain(line)
            }
        };
        if !text.is_empty() {
            let text = if text.ends_with(['.', '!', '?', ':', ';']) { text } else { format!("{text}.") };
            paras.last_mut().unwrap().push(text);
        }
        if heading {
            new_para(&mut paras);
        }
    }
    let joined = paras.iter().filter(|p| !p.is_empty()).map(|p| p.join(" ")).collect::<Vec<_>>().join("\n\n");
    if joined.chars().count() <= MAX_CHARS {
        return joined;
    }
    // Cut at a sentence end before the limit.
    let cut: String = joined.chars().take(MAX_CHARS).collect();
    let end = cut.rfind(". ").map(|i| i + 1).unwrap_or(cut.len());
    format!("{} That's the start; the rest is on screen.", cut[..end].trim())
}

/// "First", "Second"… for spoken steps; "Next" after the fifth.
fn ordinal(n: usize) -> &'static str {
    ["First", "Second", "Third", "Fourth", "Fifth"].get(n.wrapping_sub(1)).copied().unwrap_or("Next")
}

/// One line without markdown: link text only, no URLs, citation marks, emphasis or inline code ticks.
fn plain(line: &str) -> String {
    let mut s = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // [text](url) → text; [3] / [1, 2] citation marks → nothing.
        if c == '[' {
            if let Some(close) = chars[i..].iter().position(|&x| x == ']').map(|p| p + i) {
                let inner: String = chars[i + 1..close].iter().collect();
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close..].iter().position(|&x| x == ')').map(|p| p + close) {
                        s.push_str(&inner);
                        i = end + 1;
                        continue;
                    }
                }
                if inner.chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ' || c == '-') && !inner.is_empty() {
                    i = close + 1;
                    continue;
                }
            }
        }
        if matches!(c, '*' | '_' | '`' | '~') {
            i += 1;
            continue;
        }
        s.push(c);
        i += 1;
    }
    // Bare URLs aren't worth hearing.
    s.split_whitespace().filter(|w| !w.starts_with("http://") && !w.starts_with("https://")).collect::<Vec<_>>().join(" ").replace(" .", ".").replace(" ,", ",")
}

/// Words per minute for a speed setting.
pub fn rate(speed: &str) -> u32 {
    match speed {
        "slow" => 150,
        "fast" => 230,
        _ => 185,
    }
}

/// `say`'s arguments: a voice name only if it looks like one, the rate, and the text file.
pub fn say_args(voice: &str, speed: &str, file: &std::path::Path) -> Vec<String> {
    let mut a = vec![];
    let v = voice.trim();
    if !v.is_empty() && v.len() <= 60 && v.chars().all(|c| c.is_alphanumeric() || " ()-'".contains(c)) && !v.starts_with('-') {
        a.push("-v".into());
        a.push(v.to_string());
    }
    a.push("-r".into());
    a.push(rate(speed).to_string());
    a.push("-f".into());
    a.push(file.to_string_lossy().into_owned());
    a
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub name: String,
    /// "en_US"
    pub language: String,
    pub sample: String,
}

/// `say -v '?'` lines: `Samantha            en_US    # Hello! My name is Samantha.`
pub fn parse_voices(out: &str) -> Vec<Voice> {
    out.lines()
        .filter_map(|l| {
            let (head, sample) = l.split_once('#')?;
            let head = head.trim_end();
            let lang_start = head.rfind(char::is_whitespace)?;
            let language = head[lang_start..].trim().to_string();
            let name = head[..lang_start].trim().to_string();
            (!name.is_empty() && language.contains('_')).then(|| Voice { name, language, sample: sample.trim().to_string() })
        })
        .collect()
}

fn stop_playing() {
    if let Some(pid) = PLAYING.lock().ok().and_then(|mut p| p.take()) {
        // The `say` child this module started (its pid, as a number argument).
        let _ = std::process::Command::new("/bin/kill").arg(pid.to_string()).status();
    }
}

/// Reads `text` aloud (a new reply replaces the one playing). Emits `speech://done` when it ends.
pub async fn say(app: Option<AppHandle>, text: &str, voice: &str, speed: &str) -> AppResult<()> {
    if !cfg!(target_os = "macos") {
        return Err(AppError::msg("The system voice is a Mac feature. Download a BYTE voice in Settings \u{2192} Voice to have answers read aloud."));
    }
    let words = speakable(text);
    stop_playing();
    if words.trim().is_empty() {
        if let Some(app) = app {
            let _ = app.emit(DONE_EVENT, ());
        }
        return Ok(());
    }
    let file = tempfile::Builder::new().prefix("byte-say").suffix(".txt").tempfile()?;
    std::fs::write(file.path(), &words)?;
    let mut child = tokio::process::Command::new("/usr/bin/say").args(say_args(voice, speed, file.path())).kill_on_drop(true).spawn()?;
    let pid = child.id();
    if let Ok(mut p) = PLAYING.lock() {
        *p = pid;
    }
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        drop(file);
        if let Ok(mut p) = PLAYING.lock() {
            if *p == pid {
                *p = None;
            }
        }
        if let Some(app) = app {
            let _ = app.emit(DONE_EVENT, ());
        }
    });
    Ok(())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn speaking() -> bool {
    PLAYING.lock().map(|p| p.is_some()).unwrap_or(false) || crate::tts::speaking()
}

/// Reads `text` aloud: BYTE's own voices (tts.rs) when they're downloaded, else the Mac's.
#[tauri::command]
/// `voice`: try this voice ("package/speaker") instead of the chosen one (the voice browser's ▶).
pub async fn speech_say(app: AppHandle, state: State<'_, AppState>, text: String, voice: Option<String>) -> AppResult<()> {
    if let Some(v) = voice.filter(|v| !v.is_empty()) {
        let models = &state.paths.models;
        if !crate::tts::usable(models, &v) {
            return Err(AppError::msg("Download that voice first."));
        }
        let how = {
            let s = state.settings.lock().await;
            crate::tts::Delivery { voice: v, speed: s.speech_speed.clone(), style: s.speech_style.clone(), cloud: None }
        };
        stop_playing();
        return crate::tts::feed(&app, models, &how, &uuid::Uuid::new_v4().to_string(), &text, true);
    }
    speech_feed(app, state, uuid::Uuid::new_v4().to_string(), text, true, None).await
}

/// More of answer `id` (the whole text so far): with BYTE's voices, finished sentences start playing while
/// the answer is still being written. With the Mac's voice, only the finished answer is read.
#[tauri::command]
pub async fn speech_feed(app: AppHandle, state: State<'_, AppState>, id: String, text: String, done: bool, private: Option<bool>) -> AppResult<()> {
    let (mut how, mac_voice, wants_cloud) = {
        let s = state.settings.lock().await;
        let how = crate::tts::Delivery { voice: s.byte_voice.clone(), speed: s.speech_speed.clone(), style: s.speech_style.clone(), cloud: None };
        (how, s.speech_voice.clone(), s.voice_where == "cloud" && s.cloud_connected && !private.unwrap_or(false))
    };
    let models = &state.paths.models;
    // BYTE Cloud voices: only when chosen, connected, not a private chat, and the cloud offers voices.
    if wants_cloud && cfg!(any(target_os = "macos", windows)) {
        if let Ok(client) = state.cloud_client().await {
            if let Ok(list) = crate::cloud::voice::voices(&client).await {
                let chosen = state.settings.lock().await.cloud_voice.clone();
                if let Some(v) = list.iter().find(|v| v.id == chosen).or(list.first()) {
                    how.cloud = Some((client, v.id.clone()));
                }
            }
        }
    }
    if how.cloud.is_some() || crate::tts::usable(models, &how.voice) {
        stop_playing();
        return crate::tts::feed(&app, models, &how, &id, &text, done);
    }
    if done {
        crate::tts::stop();
        return say(Some(app), &text, &mac_voice, &how.speed).await;
    }
    Ok(())
}

#[tauri::command]
pub fn speech_stop() {
    stop_playing();
    crate::tts::stop();
}

#[tauri::command]
pub async fn speech_voices() -> AppResult<Vec<Voice>> {
    if !cfg!(target_os = "macos") {
        return Ok(vec![]);
    }
    let out = tokio::process::Command::new("/usr/bin/say").args(["-v", "?"]).output().await?;
    Ok(parse_voices(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_what_is_worth_hearing() {
        let md = "## TL;DR\nThe **M5** is about *20%* faster [1][2].\n\n- See [Apple's page](https://apple.com/m5) for details.\n- More at https://example.com/x\n\n```rust\nfn main() {}\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n**Confidence:** Likely\n";
        assert_eq!(
            speakable(md),
            "TL;DR.\n\nThe M5 is about 20% faster.\n\nSee Apple's page for details. More at.\n\nThere's code on screen.\n\nThere's a table on screen."
        );
    }

    #[test]
    fn numbered_steps_are_said_in_words() {
        let md = "To reset it:\n\n1. Hold the **power** button.\n2. Wait ten seconds\n3. Press it again.\n\nThat's it!";
        assert_eq!(speakable(md), "To reset it:\n\nFirst, hold the power button. Second, wait ten seconds. Third, press it again.\n\nThat's it!");
        // A version number isn't a step.
        assert_eq!(speakable("Use version 2.5 now"), "Use version 2.5 now.");
    }

    #[test]
    fn long_answers_are_cut_at_a_sentence() {
        let md = "This is one sentence about the topic. ".repeat(80);
        let s = speakable(&md);
        assert!(s.ends_with("That's the start; the rest is on screen."));
        assert!(s.chars().count() < 1600);
    }

    #[test]
    fn say_arguments_never_carry_the_text() {
        let a = say_args("Samantha", "fast", std::path::Path::new("/tmp/t.txt"));
        assert_eq!(a, ["-v", "Samantha", "-r", "230", "-f", "/tmp/t.txt"]);
        // Odd voice names are dropped (macOS picks its default voice).
        assert_eq!(say_args("-o /tmp/x", "", std::path::Path::new("f"))[0], "-r");
        assert_eq!(say_args("Eddy (English (US))", "slow", std::path::Path::new("f"))[1], "Eddy (English (US))");
    }

    #[test]
    fn parses_the_voice_list() {
        let out = "Albert              en_US    # Hello! My name is Albert.\nAmélie              fr_CA    # Bonjour! Je m’appelle Amélie.\nEddy (English (US)) en_US    # Hello! My name is Eddy.\ngarbage line\n";
        let v = parse_voices(out);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], Voice { name: "Albert".into(), language: "en_US".into(), sample: "Hello! My name is Albert.".into() });
        assert_eq!(v[2].name, "Eddy (English (US))");
    }

    /// Real `say` (macOS): writes speech to a file.
    #[tokio::test]
    #[ignore]
    async fn e2e_say_writes_audio() {
        if !cfg!(target_os = "macos") {
            eprintln!("skipped: needs macOS");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("hello.aiff");
        let st = std::process::Command::new("/usr/bin/say").args(["-o"]).arg(&out).arg("Hey BYTE, what's the weather?").status().unwrap();
        assert!(st.success());
        assert!(std::fs::metadata(&out).unwrap().len() > 10_000);
    }
}
