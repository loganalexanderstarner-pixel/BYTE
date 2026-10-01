//! Brainstorm boards (Phase 11): sticky notes on a canvas that BYTE can add to,
//! expand, group into themes, de-duplicate and turn into an outline. A board is
//! one JSON document in the database; the UI owns positions and editing.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoardInfo {
    pub id: i64,
    pub title: String,
    pub count: usize,
    pub updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: i64,
    pub title: String,
    /// `{stickies: [{id, text, x, y, color, group?}], groups: [{id, label, x, y, w, h}]}` (the UI's shape).
    pub data: Value,
    pub updated: i64,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn list(db: &Db) -> AppResult<Vec<BoardInfo>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, title, data, updated FROM boards ORDER BY updated DESC")?;
    let rows = st.query_map([], |r| {
        let data: String = r.get(2)?;
        let count = serde_json::from_str::<Value>(&data).ok().and_then(|v| v["stickies"].as_array().map(Vec::len)).unwrap_or(0);
        Ok(BoardInfo { id: r.get(0)?, title: r.get(1)?, count, updated: r.get(3)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(db: &Db, id: i64) -> AppResult<Option<Board>> {
    let conn = db.conn();
    let row = conn
        .query_row("SELECT id, title, data, updated FROM boards WHERE id = ?1", [id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))
        .optional()?;
    Ok(row.map(|(id, title, data, updated)| Board { id, title, data: serde_json::from_str(&data).unwrap_or(json!({})), updated }))
}

/// Saves a board (id 0 = new). Returns its id.
pub fn save(db: &Db, id: i64, title: &str, data: &Value) -> AppResult<i64> {
    let title: String = title.trim().chars().take(120).collect();
    let title = if title.is_empty() { "Untitled board".to_string() } else { title };
    let text = serde_json::to_string(data)?;
    if text.len() > 2_000_000 {
        return Err(AppError::msg("That board is too big to save."));
    }
    let conn = db.conn();
    if id > 0 {
        conn.execute("UPDATE boards SET title = ?1, data = ?2, updated = ?3 WHERE id = ?4", params![title, text, now(), id])?;
        Ok(id)
    } else {
        conn.execute("INSERT INTO boards (title, data, updated) VALUES (?1, ?2, ?3)", params![title, text, now()])?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    db.conn().execute("DELETE FROM boards WHERE id = ?1", [id])?;
    Ok(())
}

// ------------------------------------------------------------------ BYTE's help on a board

/// What BYTE can do to a board.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Assist {
    /// More ideas on the topic.
    Ideas,
    /// More detail for one sticky (`focus`).
    Expand,
    /// Themes, with which stickies go in each.
    Group,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AssistResult {
    /// New stickies' texts.
    pub ideas: Vec<String>,
    /// Groups: a label and the indexes (into the stickies sent) that belong to it.
    pub groups: Vec<(String, Vec<usize>)>,
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ideas from a model reply: short, non-empty, not repeating each other or what's on the board; at most `max`.
pub fn clean_ideas(v: &Value, existing: &[String], max: usize) -> Vec<String> {
    let mut seen: Vec<String> = existing.iter().map(|e| norm(e)).collect();
    let mut out = vec![];
    for i in v["ideas"].as_array().into_iter().flatten() {
        let t = i.as_str().or_else(|| i["text"].as_str()).unwrap_or("").trim().trim_start_matches(['-', '•', '*', ' ']).trim();
        let t: String = t.chars().take(140).collect();
        let n = norm(&t);
        if n.len() < 2 || seen.iter().any(|s| *s == n) {
            continue;
        }
        seen.push(n);
        out.push(t);
        if out.len() == max {
            break;
        }
    }
    out
}

/// Groups from a model reply: labelled, indexes in range, each sticky in at most one group, empty groups dropped.
pub fn clean_groups(v: &Value, count: usize) -> Vec<(String, Vec<usize>)> {
    let mut used = vec![false; count];
    let mut out = vec![];
    for g in v["groups"].as_array().into_iter().flatten().take(8) {
        let label: String = g["label"].as_str().unwrap_or("").trim().chars().take(40).collect();
        if label.is_empty() {
            continue;
        }
        let mut items = vec![];
        for i in g["items"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            // Models count from 1.
            let i = i as usize;
            let idx = if i >= 1 && i <= count { i - 1 } else { continue };
            if !used[idx] {
                used[idx] = true;
                items.push(idx);
            }
        }
        if !items.is_empty() {
            out.push((label, items));
        }
    }
    out
}

fn prompt(kind: Assist, topic: &str, stickies: &[String], focus: Option<&str>) -> (String, Value) {
    let listed = stickies.iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)).collect::<Vec<_>>().join("\n");
    let ideas_schema = json!({ "type": "object", "properties": { "ideas": { "type": "array", "items": { "type": "string" } } }, "required": ["ideas"] });
    match kind {
        Assist::Ideas => (
            format!("Brainstorm topic: {topic}\n\nAlready on the board:\n{listed}\n\nGive 5 new, different ideas (short phrases, at most 12 words each) that aren't already on the board."),
            ideas_schema,
        ),
        Assist::Expand => (
            format!("Brainstorm topic: {topic}\n\nTake this idea further: \"{}\"\n\nGive 4 follow-up ideas or concrete next steps for it (short phrases, at most 12 words each).", focus.unwrap_or("")),
            ideas_schema,
        ),
        Assist::Group => (
            format!("Brainstorm topic: {topic}\n\nIdeas:\n{listed}\n\nGroup these ideas into 2 to 6 themes. Give each theme a short label (1-4 words) and the numbers of the ideas in it. Every idea goes in exactly one theme."),
            json!({ "type": "object", "properties": { "groups": { "type": "array", "items": { "type": "object", "properties": { "label": { "type": "string" }, "items": { "type": "array", "items": { "type": "integer" } } }, "required": ["label", "items"] } } }, "required": ["groups"] }),
        ),
    }
}

pub async fn assist_with(http: &reqwest::Client, ep: &crate::engine::Endpoint, kind: Assist, topic: &str, stickies: &[String], focus: Option<&str>) -> AppResult<AssistResult> {
    let (user, schema) = prompt(kind, topic, stickies, focus);
    let raw = crate::chat::complete_json(http, ep, "You help people brainstorm. Reply with JSON only.", &user, schema, 500).await?;
    let v = crate::research::lenient_json(&raw);
    Ok(match kind {
        Assist::Ideas => AssistResult { ideas: clean_ideas(&v, stickies, 5), ..Default::default() },
        Assist::Expand => AssistResult { ideas: clean_ideas(&v, stickies, 4), ..Default::default() },
        Assist::Group => AssistResult { groups: clean_groups(&v, stickies.len()), ..Default::default() },
    })
}

// ------------------------------------------------------------------ commands

#[tauri::command]
pub async fn boards_list(state: State<'_, AppState>) -> AppResult<Vec<BoardInfo>> {
    list(&state.db)
}

#[tauri::command]
pub async fn board_get(state: State<'_, AppState>, id: i64) -> AppResult<Board> {
    get(&state.db, id)?.ok_or_else(|| AppError::msg("That board isn't there any more."))
}

#[tauri::command]
pub async fn board_save(state: State<'_, AppState>, id: i64, title: String, data: Value) -> AppResult<i64> {
    save(&state.db, id, &title, &data)
}

#[tauri::command]
pub async fn board_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete(&state.db, id)
}

#[tauri::command]
pub async fn board_assist(state: State<'_, AppState>, kind: Assist, topic: String, stickies: Vec<String>, focus: Option<String>) -> AppResult<AssistResult> {
    let ep = match state.engine.endpoint().await {
        Some(ep) => ep,
        None => crate::backend::cloud_cards(&state).await.ok_or_else(|| AppError::msg("BYTE's ideas need a model: load one in Settings → Models."))?,
    };
    let stickies: Vec<String> = stickies.into_iter().take(80).map(|s| s.chars().take(200).collect()).collect();
    assist_with(&state.local_http, &ep, kind, &topic, &stickies, focus.as_deref()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_lists_and_deletes_boards() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        let data = json!({ "stickies": [{ "id": "a", "text": "Tacos" }, { "id": "b", "text": "Coffee" }], "groups": [] });
        let id = save(&db, 0, "  Food truck  ", &data).unwrap();
        let b = get(&db, id).unwrap().unwrap();
        assert_eq!((b.title.as_str(), &b.data), ("Food truck", &data));
        assert_eq!(list(&db).unwrap()[0].count, 2);
        save(&db, id, "", &json!({ "stickies": [] })).unwrap();
        assert_eq!(get(&db, id).unwrap().unwrap().title, "Untitled board");
        delete(&db, id).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn ideas_are_new_short_and_unique() {
        let v = json!({ "ideas": ["- Late-night tacos", "Coffee!", "late night tacos", "", "Breakfast burritos", { "text": "Kids menu" }, "x".repeat(300)] });
        let ideas = clean_ideas(&v, &["coffee".into()], 5);
        assert_eq!(&ideas[..3], ["Late-night tacos", "Breakfast burritos", "Kids menu"]);
        assert_eq!(ideas[3].chars().count(), 140);
        assert!(clean_ideas(&json!({}), &[], 5).is_empty());
    }

    #[test]
    fn groups_keep_valid_items_once() {
        let v = json!({ "groups": [{ "label": "Food", "items": [1, 2, 9, 2] }, { "label": "Drinks", "items": [2, 3] }, { "label": "", "items": [4] }, { "label": "Empty", "items": [] }] });
        assert_eq!(clean_groups(&v, 4), vec![("Food".to_string(), vec![0, 1]), ("Drinks".to_string(), vec![2])]);
    }

    /// Real engine: 5 new ideas for a topic.
    #[tokio::test]
    #[ignore]
    async fn e2e_board_ideas() {
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let have = vec!["Tacos".to_string(), "Coffee".to_string()];
        let r = assist_with(&crate::chat::local_client(), &ep, Assist::Ideas, "Ideas for a food truck menu", &have, None).await.unwrap();
        println!("{:?}", r.ideas);
        assert!(r.ideas.len() >= 3, "{:?}", r.ideas);
        let r = assist_with(&crate::chat::local_client(), &ep, Assist::Group, "Food truck", &["Tacos".into(), "Burritos".into(), "Iced coffee".into(), "Lemonade".into()], None).await.unwrap();
        println!("{:?}", r.groups);
        assert!(!r.groups.is_empty());
    }
}
