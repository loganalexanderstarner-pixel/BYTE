//! Settings → Privacy: the Mac permissions BYTE may use (with live status where
//! macOS tells apps cheaply) and the activity log of what BYTE did
//! (`tools::ActionLog`, actions.jsonl).

use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    pub id: &'static str,
    pub name: &'static str,
    /// What it's for, in plain words.
    pub why: &'static str,
    /// "allowed", "denied", "not asked" or "unknown" (macOS doesn't say).
    pub status: &'static str,
    /// System Settings link to change it.
    pub url: String,
}

const PRIVACY: &str = "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension";

pub fn permissions() -> Vec<Permission> {
    let p = |id, name, why, status, anchor: &str| Permission { id, name, why, status, url: format!("{PRIVACY}?{anchor}") };
    vec![
        p("microphone", "Microphone", "Voice input, Talk mode and “Hey BYTE”. Audio stays on this Mac.", mac::microphone(), "Privacy_Microphone"),
        p("accessibility", "Accessibility", "Copying the text you selected for ⌥⌘B (smart reply, translate, explain).", mac::accessibility(), "Privacy_Accessibility"),
        p("automation", "Automation", "Mac control: Notes, Reminders, Calendar, Mail drafts, Music, Finder and System Events, each asked once.", "unknown", "Privacy_Automation"),
        p("calendars", "Calendars", "“What's on my calendar?” and the daily briefing.", "unknown", "Privacy_Calendars"),
        p("reminders", "Reminders", "“Remind me to…” with Apple Reminders.", "unknown", "Privacy_Reminders"),
        Permission {
            id: "notifications",
            name: "Notifications",
            why: "Reminders, finished automations, watcher alerts and clipped pages.",
            status: "unknown",
            url: "x-apple.systempreferences:com.apple.Notifications-Settings.extension".into(),
        },
    ]
}

#[cfg(target_os = "macos")]
mod mac {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    pub fn accessibility() -> &'static str {
        if unsafe { AXIsProcessTrusted() } != 0 {
            "allowed"
        } else {
            "not asked"
        }
    }

    pub fn microphone() -> &'static str {
        use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
        let Some(media) = (unsafe { AVMediaTypeAudio }) else { return "unknown" };
        match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) } {
            AVAuthorizationStatus::Authorized => "allowed",
            AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted => "denied",
            _ => "not asked",
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    pub fn accessibility() -> &'static str {
        "unknown"
    }
    pub fn microphone() -> &'static str {
        "unknown"
    }
}

/// One thing BYTE did, from the activity log.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub ts: String,
    pub tool: String,
    /// "web", "mac", "terminal", "files", "connectors", "automations", "tasks", "memory" or "other".
    pub kind: &'static str,
    pub ok: bool,
    pub summary: String,
    pub args: Value,
}

pub fn kind_of(tool: &str) -> &'static str {
    match tool {
        "mac_terminal" => "terminal",
        t if t.starts_with("mac_") => "mac",
        "search_my_files" => "files",
        "remember" => "memory",
        t if t.starts_with("notion_") || t.starts_with("obsidian_") || t.starts_with("calendar_link") => "connectors",
        t if t.starts_with("automation_") => "automations",
        t if t.starts_with("task_") || t.starts_with("tracker_") => "tasks",
        "web_search" | "read_page" | "academic_search" | "find_places" | "weather" | "feed_follow" | "watch_add" => "web",
        // The web agent's steps.
        "open_url" | "look_at_page" | "click" | "type_text" | "choose_option" | "scroll_page" | "go_back" | "submit_form" | "download_file" | "save_page" => "web",
        _ => "other",
    }
}

/// The log, newest first (the rotated file too), at most `limit` entries
/// matching `query` (tool, summary or arguments) and `kind`.
pub fn read(log: &Path, limit: usize, query: &str, kind: Option<&str>) -> Vec<Activity> {
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    for path in [log.to_path_buf(), log.with_extension("jsonl.1")] {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines().rev() {
            if out.len() >= limit {
                return out;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let tool = v["tool"].as_str().unwrap_or("").to_string();
            let a = Activity {
                ts: v["ts"].as_str().unwrap_or("").to_string(),
                kind: kind_of(&tool),
                tool,
                ok: v["ok"].as_bool().unwrap_or(true),
                summary: v["summary"].as_str().unwrap_or("").to_string(),
                args: v["args"].clone(),
            };
            if kind.is_some_and(|k| k != a.kind) {
                continue;
            }
            if !q.is_empty() && !format!("{} {} {}", a.tool, a.summary, a.args).to_lowercase().contains(&q) {
                continue;
            }
            out.push(a);
        }
    }
    out
}

pub const OFFLINE_EVENT: &str = "privacy://offline";

/// The menu-bar icon's Offline tick. Going online while BYTE is locked waits for unlocking.
pub async fn set_offline(app: &tauri::AppHandle, on: bool) {
    use tauri::{Emitter, Manager};
    let state = app.state::<AppState>();
    if !on && state.lock.is_locked() {
        crate::quick::sync_offline_item(true);
        return;
    }
    let mut s = state.settings.lock().await;
    s.offline = on;
    if let Err(e) = s.save(&state.paths.settings_file) {
        log::warn!("couldn't save the offline switch: {e}");
    }
    crate::offline::set(on);
    crate::quick::sync_offline_item(on);
    let _ = app.emit(OFFLINE_EVENT, on);
}

#[tauri::command]
pub fn privacy_permissions() -> Vec<Permission> {
    permissions()
}

#[tauri::command]
pub fn actions_list(state: State<'_, AppState>, limit: Option<usize>, query: Option<String>, kind: Option<String>) -> AppResult<Vec<Activity>> {
    crate::lock::ensure(&state)?;
    Ok(read(&state.paths.data.join("actions.jsonl"), limit.unwrap_or(300).min(5000), query.as_deref().unwrap_or(""), kind.as_deref()))
}

#[tauri::command]
pub fn actions_clear(state: State<'_, AppState>) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    let log = state.paths.data.join("actions.jsonl");
    for p in [log.clone(), log.with_extension("jsonl.1")] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_newest_first_across_the_rotated_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("actions.jsonl");
        let log = crate::tools::ActionLog::new(path.clone());
        log.record("web_search", &json!({"query": "old news"}), true, "5 results");
        std::fs::rename(&path, path.with_extension("jsonl.1")).unwrap();
        log.record("mac_reminder_add", &json!({"title": "Call Mom"}), true, "Added to Reminders");
        log.record("mac_terminal", &json!({"command": "df -h /"}), false, "Not run");
        std::fs::OpenOptions::new().append(true).open(&path).and_then(|mut f| std::io::Write::write_all(&mut f, b"not json\n")).unwrap();

        let all = read(&path, 10, "", None);
        assert_eq!(all.iter().map(|a| a.tool.as_str()).collect::<Vec<_>>(), vec!["mac_terminal", "mac_reminder_add", "web_search"]);
        assert_eq!(all.iter().map(|a| a.kind).collect::<Vec<_>>(), vec!["terminal", "mac", "web"]);
        assert!(!all[0].ok);
        assert_eq!(read(&path, 1, "", None).len(), 1);
        assert_eq!(read(&path, 10, "mom", None)[0].tool, "mac_reminder_add");
        assert_eq!(read(&path, 10, "", Some("web")).len(), 1);
        assert!(read(&dir.path().join("none.jsonl"), 10, "", None).is_empty());
    }

    #[test]
    fn every_permission_links_to_system_settings() {
        let ps = permissions();
        assert_eq!(ps.len(), 6);
        for p in &ps {
            assert!(p.url.starts_with("x-apple.systempreferences:com.apple."), "{}", p.url);
            assert!(!p.url.contains(char::is_whitespace));
            assert!(["allowed", "denied", "not asked", "unknown"].contains(&p.status));
        }
        assert!(ps.iter().any(|p| p.url.ends_with("Privacy_Microphone")));
    }

    #[test]
    fn kinds() {
        assert_eq!(kind_of("mac_trash"), "mac");
        assert_eq!(kind_of("notion_create"), "connectors");
        assert_eq!(kind_of("automation_run"), "automations");
        assert_eq!(kind_of("submit_form"), "web");
        assert_eq!(kind_of("calculate"), "other");
    }
}
