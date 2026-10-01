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
    let mut out: Vec<String> = vec![];
    let mut in_code = false;
    let mut in_table = false;
    for raw in markdown.lines() {
        let line = raw.trim();
        if line.starts_with("```") {
            if !in_code {
                out.push("There's code on screen.".into());
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if line.starts_with('|') {
            if !in_table {
                out.push("There's a table on screen.".into());
                in_table = true;
            }
            continue;
        }
        in_table = false;
        if line.is_empty() || line.starts_with("**Confidence:**") || line.chars().all(|c| matches!(c, '-' | '*' | '_' | ' ')) {
            continue;
        }
        let line = line.trim_start_matches('#').trim_start_matches('>').trim();
        let line = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).unwrap_or(line);
        let text = plain(line);
        if !text.is_empty() {
            out.push(text);
        }
    }
    let joined = out.iter().map(|l| if l.ends_with(['.', '!', '?', ':', ';']) { l.clone() } else { format!("{l}.") }).collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= MAX_CHARS {
        return joined;
    }
    // Cut at a sentence end before the limit.
    let cut: String = joined.chars().take(MAX_CHARS).collect();
    let end = cut.rfind(". ").map(|i| i + 1).unwrap_or(cut.len());
    format!("{} That's the start; the rest is on screen.", cut[..end].trim())
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
        return Err(AppError::msg("Reading answers aloud needs a Mac for now."));
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
    PLAYING.lock().map(|p| p.is_some()).unwrap_or(false)
}

#[tauri::command]
pub async fn speech_say(app: AppHandle, state: State<'_, AppState>, text: String) -> AppResult<()> {
    let (voice, speed) = {
        let s = state.settings.lock().await;
        (s.speech_voice.clone(), s.speech_speed.clone())
    };
    say(Some(app), &text, &voice, &speed).await
}

#[tauri::command]
pub fn speech_stop() {
    stop_playing();
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
            "TL;DR. The M5 is about 20% faster. See Apple's page for details. More at. There's code on screen. There's a table on screen."
        );
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
