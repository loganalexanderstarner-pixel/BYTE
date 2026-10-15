//! "Problem report" (Settings → Backup and help): the last lines of BYTE's own log, the BYTE version, the system version and
//! the names of the hardware, with the person's name, home folder and anything that looks like a key or token taken out.
//! The person sees exactly the text first and can change it, and then saves it to a file. Nothing is sent anywhere: the
//! BYTE Cloud has no endpoint for reports, so saving a file (to attach to a message) is all it does.
//!
//! The scrubbing is in [`scrub`], a pure function with its tests. The patterns mirror `scripts/check-secrets.sh` (BYTE
//! keys, bearer tokens, private key blocks, GitHub and OpenAI-style keys, AWS keys) and add what a log can hold: values
//! of settings named like a secret, long opaque tokens, e-mail addresses and IP addresses.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Log lines kept.
pub const LOG_LINES: usize = 300;

/// Who and where to take out of the text.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    /// The system user name ("" when unknown).
    pub user: String,
    /// The home folder ("" when unknown).
    pub home: String,
    /// The computer's name ("" when unknown).
    pub host: String,
}

impl Identity {
    pub fn this_pc() -> Identity {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|h| h.to_string_lossy().to_string()).unwrap_or_default();
        let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
        let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
        Identity { user, home, host }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '/' | '\\' | '~' | '@' | '%')
}

const SECRET_NAMES: &[&str] = &["token", "secret", "password", "passphrase", "passwd", "apikey", "api_key", "api-key", "authorization", "credential", "bearer", "privatekey", "private_key", "cloudkey", "cloud_key"];

fn secret_name(w: &str) -> bool {
    let l = w.to_lowercase();
    let l = l.trim_matches(|c: char| !c.is_alphanumeric());
    l == "key" || l.ends_with("key") && l.len() > 3 && !l.ends_with("hotkey") && !l.ends_with("keys") || SECRET_NAMES.iter().any(|n| l.contains(n))
}

fn alnum_run(s: &str, from: usize, ok: impl Fn(char) -> bool) -> usize {
    s[from..].chars().take_while(|c| ok(*c)).count()
}

/// Does this word look like a credential by itself?
fn secret_word(w: &str) -> bool {
    let w = w.trim_matches(|c: char| matches!(c, '.' | ',' | ';' | '-' | '_'));
    let after = |p: &str| w.strip_prefix(p);
    if let Some(r) = after("byte_") {
        if r.len() >= 20 && r.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return true;
        }
    }
    if after("ghp_").is_some_and(|r| alnum_run(r, 0, |c| c.is_ascii_alphanumeric()) >= 30) || after("github_pat_").is_some_and(|r| r.len() >= 40) {
        return true;
    }
    if after("sk-").is_some_and(|r| r.len() >= 32 && r.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
        return true;
    }
    if after("AKIA").is_some_and(|r| r.len() >= 16 && r.chars().take(16).all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())) {
        return true;
    }
    // Long opaque strings (tokens, hashes): 32+ letters/digits/-_ with both a letter and a digit, no path or dot in them.
    w.len() >= 32 && w.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '=')) && w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| c.is_ascii_alphabetic())
}

fn ipv4(w: &str) -> bool {
    let parts: Vec<&str> = w.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n <= 255))
}

fn email(w: &str) -> bool {
    match w.split_once('@') {
        Some((a, b)) => !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.'),
        None => false,
    }
}

/// Replaces every place the home folder is written (either slash) and generic user-folder paths with `~`.
fn scrub_paths(line: &str, id: &Identity) -> String {
    let mut out = line.to_string();
    for h in [id.home.clone(), id.home.replace('\\', "/"), id.home.replace('/', "\\")] {
        if h.trim_matches(['/', '\\']).len() >= 3 {
            out = replace_ci(&out, &h, "~");
        }
    }
    // Other user folders written out (a log from a path of another account, a copied report).
    for marker in ["\\Users\\", "/Users/", "/home/", "\\home\\"] {
        let mut result = String::new();
        let mut rest = out.as_str();
        while let Some(i) = find_ci(rest, marker) {
            let after = i + marker.len();
            result.push_str(&rest[..i]);
            let name_len: usize = rest[after..].chars().take_while(|c| !matches!(c, '\\' | '/' | ' ' | '"' | '\'' | ':' | ',' | ';' | ')')).map(char::len_utf8).sum();
            // "C:" before "\Users\" goes too; the name is the user folder.
            while result.ends_with(|c: char| c.is_ascii_alphabetic() || c == ':') && result.len() >= 2 && result.ends_with(':') {
                result.pop();
                if result.ends_with(|c: char| c.is_ascii_alphabetic()) {
                    result.pop();
                }
                break;
            }
            result.push('~');
            rest = &rest[after + name_len..];
        }
        result.push_str(rest);
        out = result;
    }
    if id.user.chars().count() >= 3 {
        out = replace_word_ci(&out, &id.user, "<user>");
    }
    if id.host.chars().count() >= 3 {
        out = replace_word_ci(&out, &id.host, "<computer>");
    }
    out
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let (h, n) = (hay.to_lowercase(), needle.to_lowercase());
    // Lowercasing can change byte offsets for exotic characters; only trust the match when the lengths agree.
    if h.len() != hay.len() {
        return hay.find(needle);
    }
    h.find(&n)
}

fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
    let mut out = String::new();
    let mut rest = hay;
    while let Some(i) = find_ci(rest, needle) {
        out.push_str(&rest[..i]);
        out.push_str(with);
        rest = &rest[i + needle.len()..];
    }
    out.push_str(rest);
    out
}

fn replace_word_ci(hay: &str, word: &str, with: &str) -> String {
    let mut out = String::new();
    let mut rest = hay;
    let mut prev_alnum = false;
    while let Some(i) = find_ci(rest, word) {
        let before_ok = !(prev_alnum && i == 0) && !rest[..i].chars().next_back().is_some_and(char::is_alphanumeric);
        let end = i + word.len();
        let after_ok = !rest[end..].chars().next().is_some_and(char::is_alphanumeric);
        out.push_str(&rest[..i]);
        if before_ok && after_ok {
            out.push_str(with);
        } else {
            out.push_str(&rest[i..end]);
        }
        prev_alnum = rest[..end].chars().next_back().is_some_and(char::is_alphanumeric);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// A MAC address: six pairs of hex digits joined by ":" or "-".
fn mac(t: &str) -> bool {
    let sep = if t.contains(':') { ':' } else { '-' };
    let parts: Vec<&str> = t.split(sep).collect();
    parts.len() == 6 && parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// An IPv6 address (with or without a "%zone"): hex groups and colons, "::" or all eight groups.
fn ipv6(t: &str) -> bool {
    let t = t.split('%').next().unwrap_or(t);
    if !t.contains(':') || !t.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
        return false;
    }
    let groups: Vec<&str> = t.split(':').collect();
    let ok_groups = groups.iter().all(|g| g.len() <= 4);
    let compressed = t.contains("::");
    ok_groups && ((compressed && t.matches("::").count() == 1 && groups.len() <= 9) || (!compressed && groups.len() == 8))
}

/// Replaces MAC and IPv6 addresses. Words with other letters or symbols in them ("std::fs::write", times) are left alone.
fn scrub_net(line: &str) -> String {
    let mut out = String::new();
    let mut tok = String::new();
    let flush = |tok: &mut String, out: &mut String| {
        if !tok.is_empty() {
            if mac(tok) {
                out.push_str("[mac]");
            } else if ipv6(tok) {
                out.push_str("[ip]");
            } else {
                out.push_str(tok);
            }
            tok.clear();
        }
    };
    for c in line.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '%') {
            tok.push(c);
        } else {
            flush(&mut tok, &mut out);
            out.push(c);
        }
    }
    flush(&mut tok, &mut out);
    out
}

/// Scrubs one line (no private-key block handling: that is in [`scrub`]).
fn scrub_line(line: &str, id: &Identity) -> String {
    let line = scrub_net(&scrub_paths(line, id));
    let mut out = String::new();
    let mut chars = line.char_indices().peekable();
    // After a secret-looking name and its ":" or "=", the next word is the value.
    let mut redact_next = false;
    let mut bearer = false;
    while let Some(&(i, c)) = chars.peek() {
        if is_word_char(c) {
            let mut j = i;
            while let Some(&(k, ch)) = chars.peek() {
                if is_word_char(ch) {
                    j = k + ch.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let w = &line[i..j];
            if (redact_next || bearer) && w.eq_ignore_ascii_case("bearer") {
                // "Authorization: Bearer <token>": the token is the value.
                out.push_str(w);
                bearer = true;
                redact_next = false;
            } else if redact_next || bearer {
                out.push_str("[removed]");
                redact_next = false;
                bearer = false;
            } else if secret_word(w) {
                out.push_str("[removed]");
            } else if email(w) {
                out.push_str("[email]");
            } else if ipv4(w.trim_end_matches('.')) {
                out.push_str("[ip]");
            } else {
                out.push_str(w);
                if w.eq_ignore_ascii_case("bearer") {
                    bearer = true;
                } else if secret_name(w) {
                    // Look ahead: only a ":" or "=" (with quotes and spaces) makes the next word its value.
                    let tail = &line[j..];
                    let sep: String = tail.chars().take_while(|c| matches!(c, ' ' | ':' | '=' | '"' | '\'' | '\t')).collect();
                    if sep.contains(':') || sep.contains('=') {
                        redact_next = true;
                    }
                }
            }
        } else {
            out.push(c);
            chars.next();
        }
    }
    out
}

/// The text with names, folders and credentials taken out.
pub fn scrub(text: &str, id: &Identity) -> String {
    let mut out = Vec::new();
    let mut in_key = false;
    for line in text.lines() {
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
            in_key = true;
            out.push("[private key removed]".to_string());
            continue;
        }
        if in_key {
            if line.contains("-----END") {
                in_key = false;
            }
            continue;
        }
        out.push(scrub_line(line, id));
    }
    out.join("\n")
}

/// The last `n` lines.
pub fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// What the report holds, assembled from plain facts (already free of anything personal) and the log.
pub fn compose(facts: &[(&str, String)], log: &str, id: &Identity) -> String {
    let mut out = String::from("BYTE problem report\n===================\n");
    for (k, v) in facts {
        out.push_str(&format!("{k}: {v}\n"));
    }
    out.push_str(&format!("\nLast lines of BYTE's log (at most {LOG_LINES}):\n-----\n"));
    out.push_str(log);
    out.push('\n');
    scrub(&out, id)
}

fn newest_log(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

fn read_tail(path: &Path, n: usize) -> String {
    // Read only the end of a large file.
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(400_000);
    let _ = f.seek(SeekFrom::Start(start));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    tail(&String::from_utf8_lossy(&buf), n)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Exactly what would be saved.
    pub text: String,
    /// The parts, in words, for the heading of the preview.
    pub includes: Vec<String>,
}

#[tauri::command]
pub async fn problem_report_build(app: AppHandle, state: State<'_, AppState>) -> AppResult<Report> {
    let info = crate::system::system_info(&state.paths.data);
    let gpus: Vec<String> = info.gpus.iter().map(|g| format!("{} ({:.0} GB)", g.name, g.dedicated_bytes as f64 / 1e9)).collect();
    let active = state.settings.lock().await.active_model.clone().unwrap_or_else(|| "none".into());
    let facts = vec![
        ("BYTE version", env!("CARGO_PKG_VERSION").to_string()),
        ("System", format!("{} ({})", info.os_version, info.platform)),
        ("Processor", format!("{}, {} cores", info.chip, info.cpu_cores)),
        ("Memory", format!("{:.0} GB", info.total_ram_bytes as f64 / 1e9)),
        ("Graphics", if gpus.is_empty() { "none found".into() } else { gpus.join(", ") }),
        ("Free disk", format!("{:.0} GB", info.free_disk_bytes as f64 / 1e9)),
        ("Model in use", active),
    ];
    let mut dirs = vec![state.paths.logs.clone()];
    if let Ok(d) = app.path().app_log_dir() {
        dirs.push(d);
    }
    let log = newest_log(&dirs).map(|p| read_tail(&p, LOG_LINES)).unwrap_or_else(|| "(no log file found)".into());
    let text = compose(&facts, &log, &Identity::this_pc());
    Ok(Report { text, includes: vec!["BYTE version and system version".into(), "Names of your processor and graphics card, memory and free disk".into(), format!("The last {LOG_LINES} lines of BYTE's log")] })
}

/// Saves the (possibly edited) preview text to the chosen file, scrubbed once more.
#[tauri::command]
pub async fn problem_report_save(path: String, text: String) -> AppResult<()> {
    let clean = scrub(&text, &Identity::this_pc());
    std::fs::write(&path, clean).map_err(|e| AppError::msg(format!("Couldn't write the file: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> Identity {
        Identity { user: "Jordan".into(), home: "C:\\Users\\Jordan".into(), host: "DESKTOP-4F2K9Q".into() }
    }

    // Fake keys are assembled at run time so this file itself contains nothing that looks like a credential.
    fn fake(prefix: &str, n: usize) -> String {
        format!("{prefix}{}", "aB3".repeat(n).chars().take(n).collect::<String>())
    }

    #[test]
    fn the_user_name_and_home_folder_become_a_tilde() {
        let s = scrub("opened C:\\Users\\Jordan\\AppData\\Roaming\\byte\\x.db and c:/users/jordan/Documents", &id());
        assert_eq!(s, "opened ~\\AppData\\Roaming\\byte\\x.db and ~/Documents");
        assert_eq!(scrub("hello Jordan, hi jordan.", &id()), "hello <user>, hi <user>.");
        assert!(!scrub("JordanBlake is someone else", &id()).contains("<user>"), "whole words only");
    }

    #[test]
    fn other_user_folders_are_taken_out_too() {
        let s = scrub("/Users/sam/Library/x and /home/pat/.config and D:\\Users\\Kim\\f", &Identity::default());
        assert!(!s.contains("sam") && !s.contains("pat") && !s.contains("Kim"), "{s}");
        assert!(s.contains("~/Library/x") && s.contains("~/.config"), "{s}");
    }

    #[test]
    fn keys_in_every_shape_the_secret_check_knows_are_removed() {
        for key in [fake("byte_", 40), fake("ghp_", 36), fake("github_pat_", 50), fake("sk-", 40), format!("AKIA{}", "ABCDEFGHIJKLMNOP")] {
            let s = scrub(&format!("sent with {key} ok"), &id());
            assert!(!s.contains(&key), "{key} survived: {s}");
            assert!(s.contains("[removed]"));
        }
        let s = scrub("Authorization: Bearer abc123def456 and more", &id());
        assert!(!s.contains("abc123def456"), "{s}");
    }

    #[test]
    fn a_private_key_block_is_cut_out() {
        let text = format!("before\n-----BEGIN {} PRIVATE KEY-----\nAAAA\nBBBB\n-----END {} PRIVATE KEY-----\nafter", "RSA", "RSA");
        let s = scrub(&text, &id());
        assert_eq!(s, "before\n[private key removed]\nafter");
    }

    #[test]
    fn values_of_secret_named_settings_go_whatever_they_look_like() {
        for line in ["password=hunter2", "\"apiKey\": \"hunter2\"", "token: hunter2", "client_secret = hunter2", "cloudKey: hunter2"] {
            let s = scrub(line, &id());
            assert!(!s.contains("hunter2"), "{line} -> {s}");
        }
        assert_eq!(scrub("hotkey: Control+Alt+Space", &id()), "hotkey: Control+Alt+Space", "a shortcut is not a secret");
        assert_eq!(scrub("keys are fine", &id()), "keys are fine");
    }

    #[test]
    fn long_opaque_tokens_emails_and_addresses_are_removed_but_ordinary_text_is_not() {
        let tok = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        let (a, b) = (format!("10.{}.2.3", 1), format!("{}.168.0.9", 192));
        let s = scrub(&format!("hash {tok} mail me@example.com from {a} and {b}."), &id());
        assert!(!s.contains(tok) && !s.contains("me@example.com") && !s.contains(&a) && !s.contains(&b), "{s}");
        let plain = "INFO engine ready in 3.2s on port 8080 with model qwen3-14b-q4_k_m.gguf (v1.2.3)";
        assert_eq!(scrub(plain, &id()), plain, "version numbers and file names stay");
    }

    #[test]
    fn the_report_has_the_facts_the_last_lines_and_nothing_personal() {
        let log: String = (0..500).map(|i| format!("line {i} for Jordan\n")).collect();
        let text = compose(&[("BYTE version", "1.2.3".into()), ("Graphics", "Some Card (16 GB)".into())], &tail(&log, LOG_LINES), &id());
        assert!(text.contains("BYTE version: 1.2.3") && text.contains("Some Card (16 GB)"));
        assert!(text.contains("line 499") && text.contains("line 200") && !text.contains("line 199 "));
        assert!(!text.contains("Jordan"));
    }

    #[test]
    fn network_addresses_and_the_computer_name_are_removed() {
        let s = scrub("nic aa:bb:cc:dd:ee:ff and 00-1A-2B-3C-4D-5E on fe80::1ff:fe23:4567:890a%12, ::1, 2001:db8:85a3:0:0:8a2e:370:7334 host DESKTOP-4F2K9Q", &id());
        for gone in ["aa:bb:cc", "00-1A-2B", "fe80", "::1", "2001:db8", "DESKTOP-4F2K9Q"] {
            assert!(!s.contains(gone), "{gone} survived: {s}");
        }
        assert!(s.contains("[mac]") && s.contains("[ip]") && s.contains("<computer>"));
        for keep in ["at 12:30:45 in std::fs::write", "see crate::chat::ChatEvent", "mode a-b-c-d-e-f"] {
            assert_eq!(scrub(keep, &id()), keep);
        }
    }

    #[test]
    fn tail_takes_the_end() {
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        assert_eq!(tail("a", 5), "a");
        assert_eq!(tail("", 5), "");
    }
}
