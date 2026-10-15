//! The optional scheduled, spoken morning briefing (setting `scheduledBriefing`, off by default).
//!
//! Turning it on adds one daily schedule (kind: briefing, scheduler.rs) at `scheduledBriefingTime`; turning it off
//! removes that schedule. The briefing itself is built by briefing.rs, saved as a chat and announced like any schedule.
//! When it has run, the short summary line is also sent to the window as `house://speak-briefing`, which says it out loud
//! (not during a game or a call, and only on systems that can speak).

use crate::db::Db;
use crate::error::AppResult;
use crate::scheduler::{self, Kind, Schedule};
use crate::state::AppState;

/// The schedule's name (how it is found again).
pub const NAME: &str = "Spoken morning briefing";

/// "07:30" when the text is a time, else the default.
pub fn clean_time(t: &str) -> String {
    chrono::NaiveTime::parse_from_str(t.trim(), "%H:%M").map(|t| t.format("%H:%M").to_string()).unwrap_or_else(|_| "07:30".into())
}

/// What is said out loud from the briefing text: its TL;DR line without Markdown, or the first plain line.
pub fn spoken_line(content: &str) -> String {
    let line = content.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).unwrap_or("");
    let line = line.trim_start_matches(['>', '-', ' ']).replace("**", "").replace('*', "").replace('`', "");
    let line = line.strip_prefix("TL;DR:").unwrap_or(&line).trim().to_string();
    if line.is_empty() {
        "Your morning briefing is ready.".into()
    } else {
        format!("Good morning. {line}")
    }
}

/// Adds, moves or removes the daily schedule so it matches the setting.
pub fn sync(db: &Db, enabled: bool, time: &str) -> AppResult<()> {
    let existing = scheduler::list(db)?.into_iter().find(|s| s.name == NAME && s.kind == Kind::Briefing);
    match (enabled, existing) {
        (false, Some(s)) => scheduler::delete(db, s.id),
        (false, None) => Ok(()),
        (true, found) => {
            let spec = format!("daily {}", clean_time(time));
            if found.as_ref().is_some_and(|s| s.spec == spec && s.enabled) {
                return Ok(());
            }
            let s = Schedule { id: found.map(|s| s.id).unwrap_or(0), kind: Kind::Briefing, name: NAME.into(), spec, prompt: String::new(), enabled: true, last_run: None, next_run: None, when: String::new(), last_chat: None, last_ok: None };
            scheduler::save(db, &s, &chrono::Local::now()).map(|_| ())
        }
    }
}

/// Called after a schedule has run: speaks the briefing line when this was the spoken briefing.
pub fn after_run(app: &tauri::AppHandle, s: &Schedule, content: &str) {
    if s.name == NAME {
        let _ = tauri::Emitter::emit(app, "house://speak-briefing", spoken_line(content));
    }
}

#[tauri::command]
pub async fn house_briefing_sync(state: tauri::State<'_, AppState>, enabled: bool, time: String) -> AppResult<()> {
    crate::kids::grownups_only()?;
    sync(&state.db, enabled, &time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_cleaned() {
        assert_eq!(clean_time("7:05"), "07:05");
        assert_eq!(clean_time("nonsense"), "07:30");
    }

    #[test]
    fn the_spoken_line_is_plain() {
        assert_eq!(spoken_line("> **TL;DR:** 3 events, rain at noon.\n\n## Today"), "Good morning. 3 events, rain at noon.");
        assert_eq!(spoken_line(""), "Your morning briefing is ready.");
    }

    #[test]
    fn the_setting_adds_moves_and_removes_the_schedule() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        sync(&db, true, "06:45").unwrap();
        let all = scheduler::list(&db).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].spec, "daily 06:45");
        sync(&db, true, "06:45").unwrap();
        assert_eq!(scheduler::list(&db).unwrap().len(), 1, "not added twice");
        sync(&db, true, "08:00").unwrap();
        let all = scheduler::list(&db).unwrap();
        assert_eq!((all.len(), all[0].spec.as_str()), (1, "daily 08:00"));
        sync(&db, false, "08:00").unwrap();
        assert!(scheduler::list(&db).unwrap().is_empty());
    }
}
