//! Learning from what went wrong, only with the person's yes: when a run fails or is stopped, a small card offers to save a note
//! ("in Notepad, the font is under the gear") in the person's own words. After a stop, and only when "Learn from corrections" is
//! on, BYTE also notices (best effort) that the person moved on to another program and offers to remember which one. It never saves
//! anything by itself, and it keeps no record of what the person did.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const FAILED_EVENT: &str = "control://failed";
pub const LEARNED_EVENT: &str = "control://learned";

/// How long after a stop BYTE watches for the person to move to another program, and how often it looks.
const WATCH_SECS: u64 = 120;
const LOOK_EVERY_MS: u64 = 1500;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedRun {
    pub goal: String,
    pub stopped: bool,
    pub summary: String,
    /// The program that was in front when the run ended (lower-case), to pre-fill the note; empty when it is BYTE itself.
    pub app: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Switched {
    pub goal: String,
    /// The program the person went to.
    pub app: String,
    pub title: String,
}

/// Whether the person is now in another program than `from` (BYTE's own window does not count). `now` is `(program, title)`.
pub fn switched_to(from: &str, now: &(String, String)) -> Option<(String, String)> {
    let app = now.0.trim().to_lowercase();
    (!app.is_empty() && app != "byte" && app != from.trim().to_lowercase()).then(|| (app, now.1.clone()))
}

/// The program to pre-fill: not BYTE, not empty.
pub fn prefill(app: &str) -> String {
    let a = app.trim().to_lowercase();
    if a == "byte" {
        String::new()
    } else {
        a
    }
}

/// A run did not work: the card offering a note. Only when notes are on (the caller checks).
pub fn offer(app: &AppHandle, goal: &str, stopped: bool, summary: &str) {
    let front = super::real_screen().foreground().map(|(a, _)| a).unwrap_or_default();
    let _ = app.emit(FAILED_EVENT, FailedRun { goal: goal.to_string(), stopped, summary: summary.to_string(), app: prefill(&front) });
}

/// After a stop: for a couple of minutes, notice when the person goes to another program, and offer a note about it.
pub fn watch(app: AppHandle, goal: String) {
    tauri::async_runtime::spawn(async move {
        let start = tokio::task::spawn_blocking(|| super::real_screen().foreground().ok()).await.ok().flatten();
        let Some((from, _)) = start else { return };
        let mut waited = 0u64;
        while waited < WATCH_SECS * 1000 {
            tokio::time::sleep(Duration::from_millis(LOOK_EVERY_MS)).await;
            waited += LOOK_EVERY_MS;
            if super::is_active() {
                return; // another run began
            }
            let Some(now) = tokio::task::spawn_blocking(|| super::real_screen().foreground().ok()).await.ok().flatten() else { continue };
            if let Some((app_name, title)) = switched_to(&from, &now) {
                let _ = app.emit(LEARNED_EVENT, Switched { goal, app: app_name, title });
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn going_to_another_program_is_noticed_and_byte_itself_is_not() {
        assert_eq!(switched_to("notepad", &("chrome".into(), "Google".into())), Some(("chrome".into(), "Google".into())));
        assert_eq!(switched_to("notepad", &("Notepad".into(), "x".into())), None, "same program");
        assert_eq!(switched_to("notepad", &("byte".into(), "BYTE".into())), None, "BYTE's own window");
        assert_eq!(switched_to("notepad", &(String::new(), String::new())), None);
    }

    #[test]
    fn the_note_form_is_filled_with_the_program_but_never_with_byte() {
        assert_eq!(prefill(" Notepad "), "notepad");
        assert_eq!(prefill("byte"), "");
    }

    #[test]
    fn the_events_carry_only_the_goal_and_the_program() {
        let p = serde_json::to_value(Switched { goal: "change the font".into(), app: "notepad".into(), title: "Untitled".into() }).unwrap();
        assert_eq!(p["app"], "notepad");
        let f = serde_json::to_value(FailedRun { goal: "g".into(), stopped: true, summary: "s".into(), app: "a".into() }).unwrap();
        assert!(f.get("stopped").is_some() && f.get("steps").is_none());
    }
}
