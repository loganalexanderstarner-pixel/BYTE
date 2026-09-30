//! Dashboards (Phase 10): the command-deck home's live tiles, local usage
//! stats and the research library. Everything is read from this Mac's own
//! data (no model, no network except the calendar links BYTE already reads),
//! so the home screen stays instant.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate};
use once_cell::sync::Lazy;
use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use crate::state::AppState;

// ------------------------------------------------------------------- tiles

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Coming {
    pub name: String,
    pub when: String,
    pub kind: String,
    pub overdue: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// Open to-dos due today (or overdue) and how many are overdue.
    pub todos_due: usize,
    pub todos_overdue: usize,
    pub todos_open: usize,
    /// The first few due to-dos.
    pub todos: Vec<String>,
    /// Trackers in the next 14 days (and overdue maintenance), soonest first.
    pub coming: Vec<Coming>,
    /// The next scheduled automation: (name, ms).
    pub next_automation: Option<(String, i64)>,
    /// Watched pages that changed in the last 3 days: (name, what changed).
    pub changes: Vec<(String, String)>,
    /// New stories in followed feeds.
    pub unread: i64,
    /// Chats whose answers cite sources.
    pub research: usize,
}

fn end_of_today_ms() -> i64 {
    Local::now().date_naive().succ_opt().and_then(|d| d.and_hms_opt(0, 0, 0)).and_then(|t| t.and_local_timezone(Local).single()).map(|t| t.timestamp_millis()).unwrap_or(i64::MAX)
}

/// Everything the home tiles show, in one quick read.
pub fn summary(db: &Db, today: NaiveDate, now_ms: i64, end_of_today: i64) -> Summary {
    let mut s = Summary::default();
    if let Ok(tasks) = crate::tasks::list(db, false) {
        let open: Vec<_> = tasks.iter().filter(|t| t.done_at.is_none()).collect();
        s.todos_open = open.len();
        let mut due: Vec<_> = open.iter().filter(|t| t.due.is_some_and(|d| d < end_of_today)).collect();
        due.sort_by_key(|t| t.due);
        s.todos_due = due.len();
        s.todos_overdue = due.iter().filter(|t| t.due.is_some_and(|d| d < now_ms)).count();
        s.todos = due.iter().take(3).map(|t| t.title.clone()).collect();
    }
    if let Ok(all) = crate::trackers::list(db) {
        let mut soon: Vec<_> = all.iter().filter(|t| !t.done).filter_map(|t| t.next.map(|n| (n, t))).filter(|(n, _)| (*n - today).num_days() <= 14).collect();
        soon.sort_by_key(|(n, _)| *n);
        s.coming = soon
            .into_iter()
            .take(4)
            .map(|(n, t)| Coming { name: t.name.clone(), when: crate::trackers::in_days(n, today), kind: format!("{:?}", t.kind).to_lowercase(), overdue: n < today })
            .collect();
    }
    if let Ok(autos) = crate::automations::list(db) {
        s.next_automation = autos.iter().filter(|a| a.enabled).filter_map(|a| a.next_run.map(|n| (a.name.clone(), n))).min_by_key(|(_, n)| *n);
    }
    if let Ok(ws) = crate::watchers::list(db) {
        let since = now_ms - 3 * 86_400_000;
        s.changes = ws.iter().filter(|w| w.enabled && w.last_change.is_some_and(|c| c >= since) && !w.last_note.is_empty()).take(3).map(|w| (w.name.clone(), w.last_note.clone())).collect();
    }
    if let Ok(fs) = crate::feeds::list(db) {
        s.unread = fs.iter().map(|f| f.unseen).sum();
    }
    s.research = research_count(db);
    s
}

// ------------------------------------------------------------------- today

static TODAY: Lazy<Mutex<Option<(Instant, NaiveDate, Vec<(String, String)>)>>> = Lazy::new(|| Mutex::new(None));
const TODAY_KEEP: Duration = Duration::from_secs(5 * 60);

/// Today's events: the Mac Calendar (when Mac control is on) and calendar links.
async fn today_events(state: &AppState) -> Vec<(String, String)> {
    let today = Local::now().date_naive();
    if let Some((at, day, v)) = TODAY.lock().ok().and_then(|g| g.clone()) {
        if day == today && at.elapsed() < TODAY_KEEP {
            return v;
        }
    }
    let (mac, links) = {
        let s = state.settings.lock().await;
        (s.mac_control && cfg!(target_os = "macos"), s.connectors_enabled)
    };
    let mut parts = crate::briefing::Parts::default();
    if mac {
        use crate::macctl::{Action, MacRunner};
        if let Ok(n) = crate::macctl::read_notes(&MacRunner, &Action::EventsList { days: 1 }).await {
            parts.events = Some(Ok(crate::briefing::events_from(&n)));
        }
    }
    if links {
        if let Some(evs) = crate::connectors::events_today(&state.net, &crate::connectors::Secrets).await {
            crate::briefing::merge_events(&mut parts, evs);
        }
    }
    let v = match parts.events {
        Some(Ok(v)) => v,
        _ => vec![],
    };
    if let Ok(mut g) = TODAY.lock() {
        *g = Some((Instant::now(), today, v.clone()));
    }
    v
}

// ------------------------------------------------------------------- usage

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub chats: i64,
    pub chats_week: i64,
    pub questions_week: i64,
    pub questions_month: i64,
    /// Answers that cite sources (all time).
    pub cited_answers: i64,
    /// Average writing speed of answers this month (tokens per second).
    pub avg_speed: Option<f64>,
    /// Modes used this month, most used first: (mode, answers).
    pub modes: Vec<(String, i64)>,
    /// Questions per day for the last 14 days, oldest first.
    pub per_day: Vec<i64>,
}

pub fn usage(db: &Db, now_ms: i64) -> AppResult<Usage> {
    let conn = db.conn();
    let week = now_ms - 7 * 86_400_000;
    let month = now_ms - 30 * 86_400_000;
    let one = |sql: &str, p: i64| -> AppResult<i64> { Ok(conn.query_row(sql, [p], |r| r.get::<_, i64>(0))?) };
    let mut u = Usage {
        chats: one("SELECT COUNT(*) FROM conversations WHERE ?1 = ?1", 0)?,
        chats_week: one("SELECT COUNT(*) FROM conversations WHERE updated_at >= ?1", week)?,
        questions_week: one("SELECT COUNT(*) FROM messages WHERE role = 'user' AND created_at >= ?1", week)?,
        questions_month: one("SELECT COUNT(*) FROM messages WHERE role = 'user' AND created_at >= ?1", month)?,
        cited_answers: one("SELECT COUNT(*) FROM messages WHERE role = 'assistant' AND json_array_length(json_extract(data, '$.sources')) > 0 AND ?1 = ?1", 0)?,
        ..Default::default()
    };
    u.avg_speed = conn
        .query_row(
            "SELECT AVG(CAST(json_extract(data, '$.stats.tokensPerSecond') AS REAL)) FROM messages WHERE role = 'assistant' AND created_at >= ?1 AND json_extract(data, '$.stats.tokensPerSecond') > 0",
            [month],
            |r| r.get::<_, Option<f64>>(0),
        )?
        .map(|v| (v * 10.0).round() / 10.0);
    let mut st = conn.prepare(
        "SELECT json_extract(data, '$.mode') AS m, COUNT(*) FROM messages WHERE role = 'assistant' AND created_at >= ?1 AND m IS NOT NULL GROUP BY m ORDER BY COUNT(*) DESC, m LIMIT 5",
    )?;
    u.modes = st.query_map([month], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let day = 86_400_000;
    let start = now_ms - 13 * day;
    let start_day = start - start.rem_euclid(day);
    let mut per = vec![0i64; 14];
    let mut st = conn.prepare("SELECT created_at FROM messages WHERE role = 'user' AND created_at >= ?1")?;
    for t in st.query_map([start_day], |r| r.get::<_, i64>(0))?.flatten() {
        let i = ((t - start_day) / day) as usize;
        if i < 14 {
            per[i] += 1;
        }
    }
    u.per_day = per;
    Ok(u)
}

// --------------------------------------------------------- research library

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Researched {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
    /// Sources cited in the chat's answers.
    pub sources: i64,
    /// The first few source titles.
    pub examples: Vec<String>,
}

fn research_count(db: &Db) -> usize {
    db.conn()
        .query_row(
            "SELECT COUNT(DISTINCT conversation_id) FROM messages WHERE role = 'assistant' AND json_array_length(json_extract(data, '$.sources')) > 0",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
}

/// Chats with cited answers, newest first; `query` narrows them by title or text.
pub fn research(db: &Db, query: &str, limit: usize) -> AppResult<Vec<Researched>> {
    let conn = db.conn();
    let like = format!("%{}%", query.trim().replace('%', "").replace('_', ""));
    let mut st = conn.prepare(
        "SELECT c.id, c.title, c.updated_at, SUM(json_array_length(json_extract(m.data, '$.sources'))),
                (SELECT json_extract(m2.data, '$.sources') FROM messages m2 WHERE m2.conversation_id = c.id AND m2.role = 'assistant'
                   AND json_array_length(json_extract(m2.data, '$.sources')) > 0 ORDER BY m2.seq DESC LIMIT 1)
         FROM conversations c JOIN messages m ON m.conversation_id = c.id
         WHERE m.role = 'assistant' AND json_array_length(json_extract(m.data, '$.sources')) > 0
           AND (?1 = '%%' OR c.title LIKE ?1 OR EXISTS (SELECT 1 FROM messages m3 WHERE m3.conversation_id = c.id AND m3.content LIKE ?1))
         GROUP BY c.id ORDER BY c.updated_at DESC LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![like, limit as i64], |r| {
            let src: Option<String> = r.get(4)?;
            let examples = src
                .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
                .map(|v| v.iter().filter_map(|x| x["title"].as_str().map(str::to_string)).filter(|t| !t.is_empty()).take(3).collect())
                .unwrap_or_default();
            Ok(Researched { id: r.get(0)?, title: r.get(1)?, updated_at: r.get(2)?, sources: r.get::<_, Option<i64>>(3)?.unwrap_or(0), examples })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ----------------------------------------------------------------- commands

#[tauri::command]
pub fn dashboard_summary(state: State<'_, AppState>) -> Summary {
    summary(&state.db, Local::now().date_naive(), chrono::Utc::now().timestamp_millis(), end_of_today_ms())
}

#[tauri::command]
pub async fn dashboard_today(state: State<'_, AppState>) -> AppResult<Vec<(String, String)>> {
    Ok(today_events(&state).await)
}

#[tauri::command]
pub fn dashboard_usage(state: State<'_, AppState>) -> AppResult<Usage> {
    usage(&state.db, chrono::Utc::now().timestamp_millis())
}

#[tauri::command]
pub fn research_library(state: State<'_, AppState>, query: Option<String>) -> AppResult<Vec<Researched>> {
    research(&state.db, query.as_deref().unwrap_or(""), 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        (dir, db)
    }

    fn chat(id: &str, title: &str, at: i64, answer: serde_json::Value) -> serde_json::Value {
        json!({ "id": id, "title": title, "createdAt": at, "updatedAt": at, "messages": [
            { "id": format!("{id}-q"), "role": "user", "content": "question about rust", "status": "done", "createdAt": at },
            { "id": format!("{id}-a"), "role": "assistant", "status": "done", "createdAt": at, "content": "answer", "mode": answer["mode"], "sources": answer["sources"], "stats": answer["stats"] },
        ] })
    }

    #[test]
    fn usage_and_the_research_library_come_from_saved_chats() {
        let (_d, db) = db();
        let now = chrono::Utc::now().timestamp_millis();
        db.save(&chat("a", "Rust vs Go", now, json!({ "mode": "deep", "sources": [{ "n": 1, "title": "Rust book", "url": "https://x" }, { "n": 2, "title": "Go blog", "url": "https://y" }], "stats": { "tokensPerSecond": 20.0 } }))).unwrap();
        db.save(&chat("b", "Hello", now, json!({ "mode": "fast", "sources": [], "stats": { "tokensPerSecond": 30.0 } }))).unwrap();
        db.save(&chat("c", "Old research", now - 40 * 86_400_000, json!({ "mode": "deep", "sources": [{ "n": 1, "title": "Paper", "url": "https://z" }] }))).unwrap();
        let u = usage(&db, now).unwrap();
        assert_eq!((u.chats, u.cited_answers), (3, 2));
        assert_eq!(u.chats_week, 2);
        assert_eq!(u.avg_speed, Some(25.0));
        assert_eq!(u.per_day.len(), 14);
        let lib = research(&db, "", 10).unwrap();
        assert_eq!(lib.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["a", "c"]);
        assert_eq!((lib[0].sources, lib[0].examples.clone()), (2, vec!["Rust book".to_string(), "Go blog".to_string()]));
        assert_eq!(research(&db, "Old", 10).unwrap().len(), 1);
        assert_eq!(research(&db, "nothing like this", 10).unwrap().len(), 0);
        assert_eq!(research_count(&db), 2);
    }

    #[test]
    fn the_summary_reads_every_module() {
        let (_d, db) = db();
        let today = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
        let now = today.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp_millis();
        let end = today.succ_opt().unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis();
        let task = |title: &str, due: Option<i64>| crate::tasks::Task { id: 0, title: title.into(), notes: String::new(), due, remind_at: None, repeat: String::new(), done_at: None, created: 0 };
        crate::tasks::save(&db, &task("Pay rent", Some(now - 86_400_000))).unwrap();
        crate::tasks::save(&db, &task("Call the bank", Some(now + 3_600_000))).unwrap();
        crate::tasks::save(&db, &task("Someday", None)).unwrap();
        crate::trackers::save(&db, &crate::trackers::Tracker { kind: crate::trackers::Kind::Bill, name: "Netflix".into(), amount: Some(15.49), cycle: "monthly".into(), next: Some(today.succ_opt().unwrap()), ..Default::default() }, today).unwrap();
        let s = summary(&db, today, now, end);
        assert_eq!((s.todos_open, s.todos_due, s.todos_overdue), (3, 2, 1));
        assert_eq!(s.todos, vec!["Pay rent".to_string(), "Call the bank".to_string()]);
        assert_eq!(s.coming, vec![Coming { name: "Netflix".into(), when: "tomorrow".into(), kind: "bill".into(), overdue: false }]);
        assert_eq!((s.unread, s.research, s.next_automation), (0, 0, None));
    }
}
