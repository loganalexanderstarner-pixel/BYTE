//! Custom assistants: BYTE set up for one job ("Email helper", "Study coach", your
//! own…). Each has a name, an emoji, instructions that go into every turn of its
//! chats, a few starter prompts and a default mode. A chat started with one keeps
//! it (`conversations.assistant_id`).

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Assistant {
    pub id: String,
    pub name: String,
    pub emoji: String,
    pub instructions: String,
    /// Up to 4 prompts shown on its empty chat.
    pub starters: Vec<String>,
    /// fast, auto, deep or extended ("" = the user's current mode).
    pub mode: String,
    pub created: i64,
}

fn preset(id: &str, name: &str, emoji: &str, mode: &str, instructions: &str, starters: &[&str]) -> Assistant {
    Assistant { id: id.into(), name: name.into(), emoji: emoji.into(), mode: mode.into(), instructions: instructions.into(), starters: starters.iter().map(|s| s.to_string()).collect(), created: 0 }
}

/// Ready-made assistants to start from (copied when chosen, so they can be edited).
pub fn presets() -> Vec<Assistant> {
    vec![
        preset(
            "email",
            "Email helper",
            "✉️",
            "fast",
            "Help the user write and reply to emails. Ask who it's to and what they want to happen if that isn't clear. Write a \
subject line and the email, ready to send: short paragraphs, a clear ask, friendly but professional unless told otherwise. \
Offer one alternative only when the tone could go either way.",
            &["Reply to this email politely saying no:", "Write a follow-up after a job interview", "Ask my landlord to fix the heating"],
        ),
        preset(
            "coach",
            "Study coach",
            "🎓",
            "auto",
            "You're a patient study coach. Explain ideas simply first, then in more depth if asked, with a short example each time. \
Check understanding with one question at the end. Suggest flashcards or a quiz when the user is learning facts.",
            &["Explain photosynthesis simply", "Help me plan a study week for my finals", "Quiz me on the French Revolution"],
        ),
        preset(
            "code",
            "Coding buddy",
            "🧑‍💻",
            "auto",
            "Help with programming. Ask for the language and error message if missing. Give working code in fenced blocks with the \
language named, explain what changed and why in a few bullets, and mention edge cases and how to test it.",
            &["Why does this Python code throw a KeyError?", "Write a bash script to rename photos by date", "Explain what a REST API is"],
        ),
        preset(
            "fitness",
            "Fitness planner",
            "🏃",
            "auto",
            "Help the user plan exercise and healthy routines at their level. Ask about their goal, time per week and any injuries \
before making a plan. Plans are week by week with sets, reps or minutes. You're not a doctor: for pain, injuries or medical \
conditions, suggest seeing one.",
            &["Make me a 4-week beginner running plan", "A 20-minute home workout with no equipment", "How do I start strength training?"],
        ),
    ]
}

const MODES: &[&str] = &["", "fast", "auto", "deep", "extended"];

/// The assistant as the model is told about it (added to the system prompt).
pub fn prompt_section(a: &Assistant) -> String {
    let i = a.instructions.trim();
    if i.is_empty() {
        return format!("\n\nIn this chat you are the user's \"{}\" assistant (you're still BYTE).", a.name);
    }
    format!("\n\nIn this chat you are the user's \"{}\" assistant (you're still BYTE). Follow these instructions from the user:\n{i}", a.name)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Assistant> {
    let starters: String = r.get(4)?;
    Ok(Assistant { id: r.get(0)?, name: r.get(1)?, emoji: r.get(2)?, instructions: r.get(3)?, starters: serde_json::from_str(&starters).unwrap_or_default(), mode: r.get(5)?, created: r.get(6)? })
}

pub fn list(db: &crate::db::Db) -> AppResult<Vec<Assistant>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, name, emoji, instructions, starters, mode, created FROM assistants ORDER BY created")?;
    let rows = st.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(db: &crate::db::Db, id: &str) -> AppResult<Option<Assistant>> {
    use rusqlite::OptionalExtension;
    Ok(db.conn().query_row("SELECT id, name, emoji, instructions, starters, mode, created FROM assistants WHERE id = ?1", [id], row).optional()?)
}

/// Adds (empty id) or updates an assistant; returns its id.
pub fn save(db: &crate::db::Db, a: &Assistant) -> AppResult<String> {
    let name = a.name.trim();
    if name.is_empty() {
        return Err(AppError::msg("Give the assistant a name."));
    }
    let id = if a.id.trim().is_empty() { uuid::Uuid::new_v4().to_string() } else { a.id.clone() };
    let starters: Vec<String> = a.starters.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).take(4).collect();
    let mode = if MODES.contains(&a.mode.as_str()) { a.mode.clone() } else { String::new() };
    let emoji: String = a.emoji.trim().chars().take(8).collect();
    db.conn().execute(
        "INSERT INTO assistants (id, name, emoji, instructions, starters, mode, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, emoji = excluded.emoji, instructions = excluded.instructions,
             starters = excluded.starters, mode = excluded.mode",
        rusqlite::params![id, name, if emoji.is_empty() { "🤖".to_string() } else { emoji }, a.instructions.trim(), serde_json::to_string(&starters)?, mode, chrono::Utc::now().timestamp_millis()],
    )?;
    Ok(id)
}

/// Deletes an assistant; its chats stay (as ordinary chats).
pub fn delete(db: &crate::db::Db, id: &str) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM assistants WHERE id = ?1", [id])?;
    conn.execute("UPDATE conversations SET assistant_id = NULL WHERE assistant_id = ?1", [id])?;
    Ok(())
}

#[tauri::command]
pub fn assistants_list(state: State<'_, AppState>) -> AppResult<Vec<Assistant>> {
    list(&state.db)
}

#[tauri::command]
pub fn assistant_presets() -> Vec<Assistant> {
    presets()
}

#[tauri::command]
pub fn assistant_save(state: State<'_, AppState>, assistant: Assistant) -> AppResult<String> {
    save(&state.db, &assistant)
}

#[tauri::command]
pub fn assistant_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    delete(&state.db, &id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistants_are_saved_edited_and_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(dir.path()).unwrap();
        let mut a = presets().into_iter().find(|p| p.id == "email").unwrap();
        a.id = String::new();
        a.starters.push("  ".into());
        a.starters.extend(["a".into(), "b".into()]);
        a.mode = "turbo".into();
        let id = save(&db, &a).unwrap();
        let got = get(&db, &id).unwrap().unwrap();
        assert_eq!(got.name, "Email helper");
        assert_eq!(got.starters.len(), 4, "blank starters dropped, at most 4");
        assert_eq!(got.mode, "", "unknown modes are cleared");
        let mut edit = got.clone();
        edit.name = "Emails".into();
        assert_eq!(save(&db, &edit).unwrap(), id);
        assert_eq!(list(&db).unwrap().iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Emails"]);
        delete(&db, &id).unwrap();
        assert!(list(&db).unwrap().is_empty());
        assert!(save(&db, &Assistant::default()).is_err(), "a name is needed");
    }

    #[test]
    fn the_prompt_keeps_byte_and_adds_the_instructions() {
        let a = Assistant { name: "Study coach".into(), instructions: "Be patient.".into(), ..Default::default() };
        let s = prompt_section(&a);
        assert!(s.contains("\"Study coach\" assistant (you're still BYTE)") && s.ends_with("Be patient."));
    }

    #[test]
    fn presets_are_complete() {
        for p in presets() {
            assert!(!p.name.is_empty() && !p.instructions.is_empty() && !p.emoji.is_empty() && p.starters.len() >= 3, "{p:?}");
            assert!(MODES.contains(&p.mode.as_str()));
        }
    }
}
