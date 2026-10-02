//! BYTE's to-do list (Phase 10): tasks with due dates and reminders, kept in
//! BYTE's own encrypted database and shown in the ✅ panel.
//!
//! In chat: "add buy milk to my to-do list", "what's on my to-do list?", "mark buy
//! milk as done", and "every weekday at 8am, summarize AI news" (a scheduled
//! question, after the user's OK). "Remind me to …" stays with Apple Reminders
//! when Mac control is on (macctl.rs); otherwise it becomes a BYTE task whose
//! reminder is a notification.

use chrono::{DateTime, Datelike, NaiveDateTime, NaiveTime, TimeZone};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::scheduler::{self, Kind, Schedule, Spec};
use crate::state::AppState;
use crate::tools::SourceBook;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    #[serde(default)]
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// Due time (ms since 1970), if any.
    #[serde(default)]
    pub due: Option<i64>,
    /// When to send a notification (ms), if any.
    #[serde(default)]
    pub remind_at: Option<i64>,
    /// "", "daily", "weekdays", "weekly" or "monthly".
    #[serde(default)]
    pub repeat: String,
    #[serde(default)]
    pub done_at: Option<i64>,
    #[serde(default)]
    pub created: i64,
}

const REPEATS: &[&str] = &["", "daily", "weekdays", "weekly", "monthly"];

fn row_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task { id: r.get(0)?, title: r.get(1)?, notes: r.get(2)?, due: r.get(3)?, remind_at: r.get(4)?, repeat: r.get(5)?, done_at: r.get(6)?, created: r.get(7)? })
}

const COLS: &str = "id, title, notes, due, remind_at, repeat, done_at, created";

/// Open tasks (overdue and soonest first, then undated), then the latest done ones.
pub fn list(db: &Db, include_done: bool) -> AppResult<Vec<Task>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM tasks WHERE done_at IS NULL ORDER BY due IS NULL, due, created"))?;
    let mut v = st.query_map([], row_task)?.collect::<rusqlite::Result<Vec<_>>>()?;
    if include_done {
        let mut st = conn.prepare(&format!("SELECT {COLS} FROM tasks WHERE done_at IS NOT NULL ORDER BY done_at DESC LIMIT 30"))?;
        v.extend(st.query_map([], row_task)?.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    Ok(v)
}

pub fn get(db: &Db, id: i64) -> AppResult<Option<Task>> {
    Ok(db.conn().query_row(&format!("SELECT {COLS} FROM tasks WHERE id = ?1"), [id], row_task).optional()?)
}

pub fn save(db: &Db, t: &Task) -> AppResult<Task> {
    let title: String = t.title.trim().chars().take(300).collect();
    if title.is_empty() {
        return Err(AppError::msg("Give the task a name."));
    }
    let repeat = if REPEATS.contains(&t.repeat.as_str()) { t.repeat.clone() } else { String::new() };
    let notes: String = t.notes.trim().chars().take(4000).collect();
    let id = {
        let conn = db.conn();
        if t.id > 0 {
            conn.execute(
                "UPDATE tasks SET title = ?1, notes = ?2, due = ?3, remind_at = ?4, repeat = ?5, done_at = ?6 WHERE id = ?7",
                params![title, notes, t.due, t.remind_at, repeat, t.done_at, t.id],
            )?;
            t.id
        } else {
            let created = if t.created > 0 { t.created } else { chrono::Utc::now().timestamp_millis() };
            conn.execute("INSERT INTO tasks (title, notes, due, remind_at, repeat, done_at, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![title, notes, t.due, t.remind_at, repeat, t.done_at, created])?;
            conn.last_insert_rowid()
        }
    };
    get(db, id)?.ok_or_else(|| AppError::msg("The task wasn't saved."))
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    db.conn().execute("DELETE FROM tasks WHERE id = ?1", [id])?;
    Ok(())
}

/// The same moment in the next period (a repeating task moves on instead of finishing).
pub fn next_repeat<Tz: TimeZone>(repeat: &str, at_ms: i64, tz: &Tz) -> Option<i64> {
    let at = tz.timestamp_millis_opt(at_ms).single()?;
    let time = at.naive_local().time();
    let local = at.naive_local();
    let spec = match repeat {
        "daily" => Spec::Daily(time),
        "weekdays" => Spec::Weekdays(time),
        "weekly" => Spec::Weekly(local.weekday(), time),
        "monthly" => Spec::Monthly(local.day(), time),
        _ => return None,
    };
    scheduler::next_after(&spec, &at).map(|t| t.timestamp_millis())
}

/// Marks a task done (or undone). A repeating task moves to its next time instead.
pub fn set_done<Tz: TimeZone>(db: &Db, id: i64, done: bool, now: &DateTime<Tz>) -> AppResult<Task> {
    let mut t = get(db, id)?.ok_or_else(|| AppError::msg("That task is gone."))?;
    if done && !t.repeat.is_empty() && t.due.is_some() {
        let tz = now.timezone();
        let base = t.due.unwrap_or(now.timestamp_millis());
        // Past occurrences don't pile up: the next one after now.
        let mut next = next_repeat(&t.repeat, base, &tz);
        while let Some(n) = next.filter(|n| *n <= now.timestamp_millis()) {
            next = next_repeat(&t.repeat, n, &tz);
        }
        let shift = next.map(|n| n - base).unwrap_or(0);
        t.due = next;
        t.remind_at = t.remind_at.map(|r| r + shift);
    } else {
        t.done_at = done.then(|| now.timestamp_millis());
    }
    save(db, &t)
}

/// Reminders whose time has come: each fires once (a repeating one moves on).
pub fn take_due_reminders<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> AppResult<Vec<Task>> {
    let now_ms = now.timestamp_millis();
    let due: Vec<Task> = {
        let conn = db.conn();
        let mut st = conn.prepare(&format!("SELECT {COLS} FROM tasks WHERE done_at IS NULL AND remind_at IS NOT NULL AND remind_at <= ?1"))?;
        let v = st.query_map([now_ms], row_task)?.collect::<rusqlite::Result<Vec<_>>>()?;
        v
    };
    let tz = now.timezone();
    for t in &due {
        let mut next = if t.repeat.is_empty() { None } else { t.remind_at.and_then(|r| next_repeat(&t.repeat, r, &tz)) };
        while let Some(n) = next.filter(|n| *n <= now_ms) {
            next = next_repeat(&t.repeat, n, &tz);
        }
        db.conn().execute("UPDATE tasks SET remind_at = ?1 WHERE id = ?2", params![next, t.id])?;
    }
    Ok(due)
}

// ------------------------------------------------------------------ in chat

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Add { title: String, due: Option<NaiveDateTime> },
    List,
    Done { what: String },
    /// A question to ask on a schedule ("every weekday at 8am, …").
    Schedule { spec: Spec, prompt: String, briefing: bool },
}

fn clean(q: &str) -> String {
    let l = q.trim().replace(['’', '‘'], "'");
    let l = l.trim_end_matches(['?', '.', '!']).trim().to_string();
    let mut s = l.as_str();
    for p in ["please ", "Please ", "hey byte, ", "can you ", "could you ", "Can you ", "Could you "] {
        s = s.strip_prefix(p).unwrap_or(s);
    }
    s.to_string()
}

const LIST_WORDS: &[&str] = &["to-do list", "todo list", "to do list", "task list", "my tasks", "my to-dos", "my todos", "my to-do's"];

/// Strips "by Friday"/"tomorrow at 5pm" off a title; returns the due time found.
fn due_in(text: &str, now: NaiveDateTime) -> (String, Option<NaiveDateTime>) {
    let l = text.to_lowercase();
    let cut = [" by ", " due ", " tomorrow", " today", " tonight", " on monday", " on tuesday", " on wednesday", " on thursday", " on friday", " on saturday", " on sunday", " at "]
        .iter()
        .filter_map(|w| l.find(w))
        .min();
    let Some(i) = cut else { return (text.trim().to_string(), None) };
    let when = &l[i..];
    let time = scheduler::time_in(when).unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default());
    let today = now.date();
    let day = if when.contains("tomorrow") {
        today.succ_opt()
    } else if when.contains("today") || when.contains("tonight") {
        Some(today)
    } else {
        let names = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
        names.iter().position(|n| when.contains(n)).map(|want| {
            let have = today.weekday().num_days_from_monday() as i64;
            let ahead = (want as i64 - have).rem_euclid(7);
            today + chrono::Duration::days(if ahead == 0 { 7 } else { ahead })
        })
    };
    let time = if when.contains("tonight") && scheduler::time_in(when).is_none() { NaiveTime::from_hms_opt(19, 0, 0).unwrap_or(time) } else { time };
    let due = match day {
        Some(d) => Some(d.and_time(time)),
        // "at 5pm" alone: today, or tomorrow if that's past.
        None if scheduler::time_in(when).is_some() => {
            let t = today.and_time(time);
            Some(if t > now { t } else { t + chrono::Duration::days(1) })
        }
        None => None,
    };
    if due.is_none() {
        return (text.trim().to_string(), None);
    }
    (text[..i].trim().to_string(), due)
}

fn strip_quotes(s: &str) -> String {
    s.trim().trim_matches(|c: char| c == '"' || c == '\'' || c == '“' || c == '”').trim().to_string()
}

pub fn ask(q: &str, now: NaiveDateTime) -> Option<Ask> {
    let text = clean(q);
    let l = text.to_lowercase();
    if l.starts_with("how ") || l.starts_with("what is a ") || l.starts_with("why ") {
        return None;
    }
    // Scheduled questions: "every weekday at 8am, summarize AI news".
    if l.starts_with("every ") || l.starts_with("each ") || l.starts_with("daily ") || l.starts_with("on mondays") || l.starts_with("on fridays") || l.starts_with("brief me every") || l.starts_with("give me a briefing every") || l.starts_with("send me a briefing every") {
        if let Some(spec) = scheduler::spec_in(&l) {
            let briefing = l.contains("briefing") || l.starts_with("brief me");
            // What to do: the part after the schedule words (after the first comma, else after the time).
            let prompt = match text.split_once(',') {
                Some((_, rest)) if !rest.trim().is_empty() => rest.trim().to_string(),
                _ => {
                    let words: Vec<&str> = text.split_whitespace().collect();
                    let is_time = |i: usize| {
                        let w = words[i].to_lowercase();
                        let digit = w.starts_with(|c: char| c.is_ascii_digit());
                        (digit && (w.ends_with("am") || w.ends_with("pm") || w.contains(':'))) || (digit && i > 0 && words[i - 1].eq_ignore_ascii_case("at")) || ["morning", "evening", "night", "afternoon", "noon"].contains(&w.as_str())
                    };
                    let skip = (0..words.len()).find(|&i| is_time(i)).map(|p| p + 1).unwrap_or(0);
                    words[skip..].join(" ").trim_start_matches(|c: char| c == ',' || c.is_whitespace()).to_string()
                }
            };
            let prompt = prompt.trim_start_matches("please ").to_string();
            if briefing || prompt.split_whitespace().count() >= 2 {
                return Some(Ask::Schedule { spec, prompt: if briefing { String::new() } else { prompt }, briefing });
            }
        }
        return None;
    }
    let mentions_list = LIST_WORDS.iter().any(|w| l.contains(w));
    if !mentions_list && !l.starts_with("add a task") && !l.starts_with("new task") && !l.starts_with("mark ") {
        return None;
    }
    // Adding.
    for p in ["add a task to ", "add a task: ", "add task ", "new task: ", "new task ", "add a to-do: ", "add ", "put "] {
        if l.starts_with(p) {
            let rest = &text[p.len()..];
            let rl = rest.to_lowercase();
            let end = [" to my", " on my", " in my"].iter().filter_map(|w| rl.find(w)).min().unwrap_or(rest.len());
            let (title, due) = due_in(&rest[..end], now);
            let title = strip_quotes(&title);
            // "add to my to-do list" with nothing to add isn't an add.
            if title.is_empty() || title.eq_ignore_ascii_case("something") {
                return None;
            }
            return Some(Ask::Add { title, due });
        }
    }
    // Finishing.
    let finished = mentions_list || has_any(&l, &[" done", " complete", " finished"]);
    if let Some(rest) = l.strip_prefix("mark ").or_else(|| l.strip_prefix("check off ")).or_else(|| l.strip_prefix("cross off ")).or_else(|| l.strip_prefix("tick off ")).filter(|_| finished) {
        let end = [" as done", " done", " as complete", " complete", " off", " from my", " on my"].iter().filter_map(|w| rest.find(w)).min().unwrap_or(rest.len());
        let what = strip_quotes(&rest[..end]);
        if !what.is_empty() {
            return Some(Ask::Done { what });
        }
    }
    // Listing.
    if has_any(&l, &["what's on", "what is on", "show me", "show my", "read my", "list my", "what do i have", "what's left", "check my"]) || l == "my to-do list" || l == "to-do list" {
        return Some(Ask::List);
    }
    None
}

fn has_any(l: &str, words: &[&str]) -> bool {
    words.iter().any(|w| l.contains(w))
}

pub fn applies(q: &str) -> bool {
    ask(q, chrono::Local::now().naive_local()).is_some()
}

/// "Remind me to …" when Mac control can't use Apple Reminders.
pub fn reminder_ask(q: &str, now: NaiveDateTime) -> Option<Ask> {
    let text = clean(q);
    let l = text.to_lowercase();
    let rest = l.strip_prefix("remind me to ").or_else(|| l.strip_prefix("remind me "))?;
    let offset = text.len() - rest.len();
    let rest = &text[offset..];
    // "remind me at 5pm to stretch": the time comes first.
    let (title, due) = match rest.split_once(" to ") {
        Some((w, t)) if scheduler::time_in(w).is_some() || w.to_lowercase().contains("tomorrow") => (t.to_string(), due_in(&format!("x {w}"), now).1),
        _ => due_in(rest, now),
    };
    let title = strip_quotes(&title);
    (!title.is_empty() && due.is_some()).then_some(Ask::Add { title, due })
}

/// Which open task a phrase means (exact, then contained).
pub fn find<'a>(tasks: &'a [Task], what: &str) -> Option<&'a Task> {
    let w = what.to_lowercase();
    let open: Vec<&Task> = tasks.iter().filter(|t| t.done_at.is_none()).collect();
    open.iter().find(|t| t.title.to_lowercase() == w).or_else(|| open.iter().find(|t| t.title.to_lowercase().contains(&w))).or_else(|| open.iter().find(|t| w.contains(&t.title.to_lowercase()) && t.title.len() >= 3)).copied()
}

fn when_text(ms: i64) -> String {
    chrono::Local.timestamp_millis_opt(ms).single().map(|t| t.format("%a, %b %-d at %-I:%M %p").to_string()).unwrap_or_default()
}

fn local_ms(dt: NaiveDateTime) -> Option<i64> {
    chrono::Local.from_local_datetime(&dt).earliest().map(|t| t.timestamp_millis())
}

/// A list the model can read.
pub fn list_notes(tasks: &[Task], now_ms: i64) -> String {
    let open: Vec<&Task> = tasks.iter().filter(|t| t.done_at.is_none()).collect();
    if open.is_empty() {
        return "The user's BYTE to-do list is empty.".into();
    }
    let lines: Vec<String> = open
        .iter()
        .map(|t| {
            let due = t.due.map(|d| format!(" (due {}{})", when_text(d), if d < now_ms { ", overdue" } else { "" })).unwrap_or_default();
            format!("- {}{due}{}", t.title, if t.repeat.is_empty() { String::new() } else { format!(", repeats {}", t.repeat) })
        })
        .collect();
    format!("The user's BYTE to-do list ({} open):\n{}", open.len(), lines.join("\n"))
}

pub async fn run(turn: &Turn<'_>, db: &Db, question: &str, mac_reminders: bool, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let now = chrono::Local::now();
    let a = match ask(question, now.naive_local()) {
        Some(a) => a,
        None if !mac_reminders => match reminder_ask(question, now.naive_local()) {
            Some(a) => a,
            None => return Ok(None),
        },
        None => return Ok(None),
    };
    let none = SourceBook::default;
    let id = format!("byte_tasks_{}", uuid::Uuid::new_v4().simple());
    match a {
        Ask::Add { title, due } => {
            let due_ms = due.and_then(local_ms);
            let t = save(db, &Task { id: 0, title: title.clone(), notes: String::new(), due: due_ms, remind_at: due_ms, repeat: String::new(), done_at: None, created: 0 })?;
            let detail = t.due.map(|d| format!("Due {} · you'll get a notification", when_text(d))).unwrap_or_else(|| "On your to-do list".into());
            send(ChatEvent::ToolCall { id: id.clone(), name: "task_add".into(), args: json!({ "title": t.title }) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
            turn.log.record("task_add", &json!({ "title": t.title }), true, &detail);
            send(done_card(&format!("Added \"{}\" to your to-do list", t.title), &detail))?;
            Ok(Some((none(), format!("Done: \"{}\" is on the user's BYTE to-do list{}. It's in the ✅ panel. Confirm in one short sentence.", t.title, t.due.map(|d| format!(", due {}", when_text(d))).unwrap_or_default()))))
        }
        Ask::List => {
            let tasks = list(db, false)?;
            send(ChatEvent::ToolCall { id: id.clone(), name: "task_list".into(), args: json!({}) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} open tasks", tasks.len()) })?;
            Ok(Some((none(), format!("{}\n\nList them clearly (overdue first); keep it short.", list_notes(&tasks, now.timestamp_millis())))))
        }
        Ask::Done { what } => {
            let tasks = list(db, false)?;
            let Some(t) = find(&tasks, &what).cloned() else {
                return Ok(Some((none(), format!("No open task on the user's BYTE to-do list matches \"{what}\". {}", list_notes(&tasks, now.timestamp_millis())))));
            };
            let after = set_done(db, t.id, true, &now)?;
            let detail = if after.done_at.is_some() { "Checked off".to_string() } else { format!("Next time: {}", after.due.map(when_text).unwrap_or_default()) };
            send(ChatEvent::ToolCall { id: id.clone(), name: "task_done".into(), args: json!({ "title": t.title }) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
            send(done_card(&format!("Checked off \"{}\"", t.title), &detail))?;
            Ok(Some((none(), format!("Done: \"{}\" is checked off ({detail}). Confirm in one short sentence.", t.title))))
        }
        Ask::Schedule { spec, prompt, briefing } => {
            let (kind, name) = if briefing { (Kind::Briefing, "Morning briefing".to_string()) } else { (Kind::Prompt, short_name(&prompt)) };
            let what = format!("{}: {}", spec.describe(), if briefing { "your daily briefing".to_string() } else { format!("\"{prompt}\"") });
            send(ChatEvent::ToolCall { id: id.clone(), name: "schedule_add".into(), args: json!({ "what": what }) })?;
            let fields = vec![
                ("When".to_string(), spec.describe()),
                (if briefing { "What" } else { "BYTE will ask" }.to_string(), if briefing { "Your daily briefing: calendar, reminders, to-dos, weather and news".to_string() } else { prompt.clone() }),
                ("Note".to_string(), "It runs while BYTE is open (a missed run happens when you next open it). Each answer is saved as a chat and you get a notification. Change or stop it in the ✅ panel.".to_string()),
            ];
            let ok = crate::macctl::ask_ok("Add this schedule?", "BYTE", fields, cancel, send).await?;
            if !ok {
                send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
                return Ok(Some((none(), "The user chose not to add the schedule. Nothing was set up. Say so in one sentence.".into())));
            }
            let s = scheduler::save(db, &Schedule { id: 0, kind, name: name.clone(), spec: spec.text(), prompt: prompt.clone(), enabled: true, last_run: None, next_run: None, when: String::new(), last_chat: None, last_ok: None }, &now)?;
            let next = s.next_run.map(when_text).unwrap_or_default();
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("First run {next}") })?;
            send(done_card(&format!("Scheduled \"{name}\""), &format!("{} · first run {next}", spec.describe())))?;
            Ok(Some((none(), format!("Done: BYTE will run \"{name}\" {} (first run {next}) while BYTE is open, save each answer as a chat and send a notification. It can be changed in the ✅ panel. Confirm in one or two short sentences.", spec.describe().to_lowercase()))))
        }
    }
}

/// A short name for a scheduled question ("Summarize AI news").
fn short_name(prompt: &str) -> String {
    let words: Vec<&str> = prompt.split_whitespace().take(6).collect();
    let mut s = words.join(" ");
    if prompt.split_whitespace().count() > 6 {
        s.push('…');
    }
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The small "done" card (the same one Mac actions use).
fn done_card(title: &str, detail: &str) -> ChatEvent {
    ChatEvent::MacDone(crate::macctl::MacDone { app: "BYTE".into(), title: title.into(), detail: detail.into(), ok: true, undo: None })
}

// ----------------------------------------------------------------- commands

#[tauri::command]
pub fn tasks_list(state: State<'_, AppState>, include_done: Option<bool>) -> AppResult<Vec<Task>> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    list(&state.db, include_done.unwrap_or(true))
}

#[tauri::command]
pub fn task_save(state: State<'_, AppState>, task: Task) -> AppResult<Task> {
    save(&state.db, &task)
}

#[tauri::command]
pub fn task_done(state: State<'_, AppState>, id: i64, done: bool) -> AppResult<Task> {
    set_done(&state.db, id, done, &chrono::Local::now())
}

#[tauri::command]
pub fn task_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete(&state.db, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};

    fn now() -> NaiveDateTime {
        // Wednesday.
        NaiveDate::from_ymd_opt(2026, 9, 30).unwrap().and_hms_opt(10, 0, 0).unwrap()
    }
    fn day(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, if d < 20 { 10 } else { 9 }, d).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn requests_are_understood() {
        assert_eq!(ask("add buy milk to my to-do list", now()), Some(Ask::Add { title: "buy milk".into(), due: None }));
        assert_eq!(ask("Add “call the bank” to my todo list", now()), Some(Ask::Add { title: "call the bank".into(), due: None }));
        assert_eq!(ask("add renew passport by Friday to my to-do list", now()), Some(Ask::Add { title: "renew passport".into(), due: Some(day(2, 9, 0)) }));
        assert_eq!(ask("put pay rent tomorrow at 5pm on my to-do list", now()), Some(Ask::Add { title: "pay rent".into(), due: Some(day(1, 17, 0)) }));
        assert_eq!(ask("add a task: email Jen", now()), Some(Ask::Add { title: "email Jen".into(), due: None }));
        assert_eq!(ask("what's on my to-do list?", now()), Some(Ask::List));
        assert_eq!(ask("show my tasks", now()), Some(Ask::List));
        assert_eq!(ask("mark buy milk as done", now()), Some(Ask::Done { what: "buy milk".into() }));
        assert_eq!(ask("cross off pay rent from my to-do list", now()), Some(Ask::Done { what: "pay rent".into() }));
        assert_eq!(
            ask("every weekday at 8am, summarize the latest AI news", now()),
            Some(Ask::Schedule { spec: Spec::Weekdays(NaiveTime::from_hms_opt(8, 0, 0).unwrap()), prompt: "summarize the latest AI news".into(), briefing: false })
        );
        assert_eq!(
            ask("every monday at 9am plan my week from my calendar", now()),
            Some(Ask::Schedule { spec: Spec::Weekly(chrono::Weekday::Mon, NaiveTime::from_hms_opt(9, 0, 0).unwrap()), prompt: "plan my week from my calendar".into(), briefing: false })
        );
        assert!(matches!(ask("brief me every morning", now()), Some(Ask::Schedule { briefing: true, .. })));
        for q in ["how do I make a to-do list", "add 2 and 3", "every cloud has a silver lining", "what is a task list", "every day is a gift", "add salt to taste", "mark my words"] {
            assert_eq!(ask(q, now()), None, "{q}");
        }
        assert_eq!(reminder_ask("remind me to call Mom tomorrow at 3pm", now()), Some(Ask::Add { title: "call Mom".into(), due: Some(day(1, 15, 0)) }));
        assert_eq!(reminder_ask("remind me to call Mom", now()), None, "no time: nothing to remind at");
        assert_eq!(reminder_ask("remind me at 5pm to stretch", now()).map(|a| matches!(a, Ask::Add { .. })), Some(true));
    }

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        (dir, db)
    }

    fn task(title: &str) -> Task {
        Task { id: 0, title: title.into(), notes: String::new(), due: None, remind_at: None, repeat: String::new(), done_at: None, created: 0 }
    }

    #[test]
    fn tasks_are_kept_sorted_and_checked_off() {
        let (_d, db) = db();
        let now = Utc.with_ymd_and_hms(2026, 9, 30, 10, 0, 0).unwrap();
        let a = save(&db, &task("later")).unwrap();
        let b = save(&db, &Task { due: Some(now.timestamp_millis() + 3_600_000), ..task("soon") }).unwrap();
        let c = save(&db, &Task { due: Some(now.timestamp_millis() - 3_600_000), ..task("overdue") }).unwrap();
        let titles: Vec<String> = list(&db, true).unwrap().into_iter().map(|t| t.title).collect();
        assert_eq!(titles, vec!["overdue", "soon", "later"]);
        set_done(&db, b.id, true, &now).unwrap();
        let open: Vec<String> = list(&db, false).unwrap().into_iter().map(|t| t.title).collect();
        assert_eq!(open, vec!["overdue", "later"]);
        assert_eq!(list(&db, true).unwrap().last().unwrap().title, "soon", "done ones come last");
        set_done(&db, b.id, false, &now).unwrap();
        assert!(get(&db, b.id).unwrap().unwrap().done_at.is_none());
        assert!(save(&db, &task("  ")).is_err());
        assert_eq!(save(&db, &Task { repeat: "hourly".into(), ..task("x") }).unwrap().repeat, "", "unknown repeats are dropped");
        delete(&db, a.id).unwrap();
        delete(&db, c.id).unwrap();
        assert_eq!(find(&list(&db, false).unwrap(), "SOON").unwrap().id, b.id);
        assert!(find(&list(&db, false).unwrap(), "nothing like it").is_none());
    }

    #[test]
    fn reminders_fire_once_and_repeating_ones_move_on() {
        let (_d, db) = db();
        let now = Utc.with_ymd_and_hms(2026, 9, 30, 10, 0, 0).unwrap();
        let t0 = now.timestamp_millis() - 60_000;
        let one = save(&db, &Task { due: Some(t0), remind_at: Some(t0), ..task("once") }).unwrap();
        let rep = save(&db, &Task { due: Some(t0), remind_at: Some(t0), repeat: "daily".into(), ..task("pills") }).unwrap();
        let future = save(&db, &Task { remind_at: Some(now.timestamp_millis() + 60_000), ..task("later") }).unwrap();
        let fired: Vec<String> = take_due_reminders(&db, &now).unwrap().into_iter().map(|t| t.title).collect();
        assert_eq!(fired, vec!["once", "pills"]);
        assert!(take_due_reminders(&db, &now).unwrap().is_empty(), "once only");
        assert_eq!(get(&db, one.id).unwrap().unwrap().remind_at, None);
        assert_eq!(get(&db, rep.id).unwrap().unwrap().remind_at, Some(t0 + 86_400_000));
        assert!(get(&db, future.id).unwrap().unwrap().remind_at.is_some());
        // Checking off a repeating task moves it to the next day instead.
        let after = set_done(&db, rep.id, true, &now).unwrap();
        assert!(after.done_at.is_none());
        assert_eq!(after.due, Some(t0 + 86_400_000));
    }

    #[test]
    fn weekday_and_monthly_repeats() {
        let fri = Utc.with_ymd_and_hms(2026, 10, 2, 9, 0, 0).unwrap().timestamp_millis();
        assert_eq!(next_repeat("weekdays", fri, &Utc), Some(Utc.with_ymd_and_hms(2026, 10, 5, 9, 0, 0).unwrap().timestamp_millis()));
        let jan31 = Utc.with_ymd_and_hms(2027, 1, 31, 9, 0, 0).unwrap().timestamp_millis();
        assert_eq!(next_repeat("monthly", jan31, &Utc), Some(Utc.with_ymd_and_hms(2027, 2, 28, 9, 0, 0).unwrap().timestamp_millis()));
        assert_eq!(next_repeat("", fri, &Utc), None);
    }

    #[test]
    fn the_list_reads_well_for_the_model() {
        let now = 1_000_000_000_000;
        let tasks = vec![Task { due: Some(now - 1), ..task("pay rent") }, task("buy milk"), Task { done_at: Some(now), ..task("done one") }];
        let notes = list_notes(&tasks, now);
        assert!(notes.contains("(2 open)") && notes.contains("overdue") && !notes.contains("done one"));
        assert_eq!(list_notes(&[], now), "The user's BYTE to-do list is empty.");
        assert_eq!(short_name("summarize the latest news about AI chips and models"), "Summarize the latest news about AI…");
    }
}
