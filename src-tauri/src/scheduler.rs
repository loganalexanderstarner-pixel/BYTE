//! Things that happen on their own (Phase 10): task reminders, the daily briefing
//! and scheduled questions ("every weekday at 8am, summarize news about AI").
//!
//! A loop checks every 30 seconds while BYTE is open. Reminders become Mac
//! notifications; a schedule runs like a chat turn with nobody watching and its
//! answer is saved as a new chat, then a notification says it's ready. A run
//! missed while BYTE was closed happens once at the next launch (never several
//! times over).
//!
//! When a schedule repeats is a small text spec, easy to read and store:
//! "daily 07:30", "weekdays 07:30", "weekly mon 07:30", "monthly 1 07:30",
//! "once 2026-10-01T08:00".

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use chrono::{DateTime, Datelike, Days, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// How often the loop looks for due things.
const TICK: Duration = Duration::from_secs(30);

// --------------------------------------------------------------------- spec

/// When something repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec {
    Daily(NaiveTime),
    Weekdays(NaiveTime),
    Weekly(Weekday, NaiveTime),
    /// Day of the month (a short month uses its last day).
    Monthly(u32, NaiveTime),
    Once(NaiveDateTime),
}

const DAYS: [(&str, Weekday); 7] = [("mon", Weekday::Mon), ("tue", Weekday::Tue), ("wed", Weekday::Wed), ("thu", Weekday::Thu), ("fri", Weekday::Fri), ("sat", Weekday::Sat), ("sun", Weekday::Sun)];

fn day_name(d: Weekday) -> &'static str {
    DAYS.iter().find(|(_, w)| *w == d).map(|(n, _)| *n).unwrap_or("mon")
}

impl Spec {
    /// The stored form ("weekdays 07:30").
    pub fn text(&self) -> String {
        let t = |t: &NaiveTime| t.format("%H:%M").to_string();
        match self {
            Spec::Daily(x) => format!("daily {}", t(x)),
            Spec::Weekdays(x) => format!("weekdays {}", t(x)),
            Spec::Weekly(d, x) => format!("weekly {} {}", day_name(*d), t(x)),
            Spec::Monthly(n, x) => format!("monthly {n} {}", t(x)),
            Spec::Once(dt) => format!("once {}", dt.format("%Y-%m-%dT%H:%M")),
        }
    }

    /// For people ("Every weekday at 7:30 AM").
    pub fn describe(&self) -> String {
        let t = |t: &NaiveTime| t.format("%-I:%M %p").to_string();
        match self {
            Spec::Daily(x) => format!("Every day at {}", t(x)),
            Spec::Weekdays(x) => format!("Every weekday at {}", t(x)),
            Spec::Weekly(d, x) => format!("Every {} at {}", full_day(*d), t(x)),
            Spec::Monthly(n, x) => format!("Monthly on the {} at {}", ordinal(*n), t(x)),
            Spec::Once(dt) => format!("Once, {}", dt.format("%a, %b %-d at %-I:%M %p")),
        }
    }
}

fn full_day(d: Weekday) -> &'static str {
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][d.num_days_from_monday() as usize]
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Reads a stored spec.
pub fn parse_spec(s: &str) -> Option<Spec> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let time = |t: &str| NaiveTime::parse_from_str(t, "%H:%M").ok();
    match parts.as_slice() {
        ["daily", t] => Some(Spec::Daily(time(t)?)),
        ["weekdays", t] => Some(Spec::Weekdays(time(t)?)),
        ["weekly", d, t] => Some(Spec::Weekly(DAYS.iter().find(|(n, _)| n == d)?.1, time(t)?)),
        ["monthly", n, t] => {
            let n: u32 = n.parse().ok().filter(|n| (1..=31).contains(n))?;
            Some(Spec::Monthly(n, time(t)?))
        }
        ["once", dt] => NaiveDateTime::parse_from_str(dt, "%Y-%m-%dT%H:%M").ok().map(Spec::Once),
        _ => None,
    }
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1).and_then(|d| d.pred_opt()).map(|d| d.day()).unwrap_or(28)
}

/// A wall-clock time in `tz`. In a spring-forward gap it's the first time after
/// the gap; in a fall-back hour, the first of the two.
fn resolve<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> Option<DateTime<Tz>> {
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(t) => Some(t),
        LocalResult::Ambiguous(a, _) => Some(a),
        LocalResult::None => tz.from_local_datetime(&(naive + chrono::Duration::hours(1))).earliest(),
    }
}

/// The next time `spec` happens strictly after `now` (None: a one-off that's past).
pub fn next_after<Tz: TimeZone>(spec: &Spec, now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
    let tz = now.timezone();
    if let Spec::Once(dt) = spec {
        return resolve(&tz, *dt).filter(|t| t > now);
    }
    let today = now.naive_local().date();
    for d in 0..=62u64 {
        let date = today.checked_add_days(Days::new(d))?;
        let at = match spec {
            Spec::Daily(t) => Some(*t),
            Spec::Weekdays(t) => (date.weekday().num_days_from_monday() < 5).then_some(*t),
            Spec::Weekly(w, t) => (date.weekday() == *w).then_some(*t),
            Spec::Monthly(n, t) => (date.day() == (*n).min(days_in_month(date.year(), date.month()))).then_some(*t),
            Spec::Once(_) => None,
        };
        if let Some(t) = at.and_then(|t| resolve(&tz, date.and_time(t))) {
            if t > *now {
                return Some(t);
            }
        }
    }
    None
}

/// A time of day in a message: "8am", "8:30 pm", "noon", "20:00", "7".
pub fn time_in(text: &str) -> Option<NaiveTime> {
    let l = text.to_lowercase().replace('.', "");
    if l.contains("noon") || l.contains("midday") {
        return NaiveTime::from_hms_opt(12, 0, 0);
    }
    if l.contains("midnight") {
        return NaiveTime::from_hms_opt(0, 0, 0);
    }
    let words: Vec<&str> = l.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let w = w.trim_matches(|c: char| c == ',' || c == ';');
        let (num, mut ampm) = match w.find(|c: char| c.is_ascii_alphabetic()) {
            Some(p) => (&w[..p], &w[p..]),
            None => (w, ""),
        };
        if num.is_empty() || !num.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        if ampm.is_empty() {
            ampm = words.get(i + 1).map(|n| n.trim_matches(',')).filter(|n| *n == "am" || *n == "pm").unwrap_or("");
        }
        let prev = if i > 0 { words[i - 1] } else { "" };
        // "at 7" counts; a bare number elsewhere ("5 things") doesn't.
        if ampm.is_empty() && !num.contains(':') && prev != "at" {
            continue;
        }
        let (h, m) = match num.split_once(':') {
            Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
            None => (num.parse::<u32>().ok()?, 0),
        };
        let h = match ampm {
            "am" | "a" => if h == 12 { 0 } else { h },
            "pm" | "p" => if h == 12 { 12 } else { h + 12 },
            "" => h,
            _ => continue,
        };
        if let Some(t) = NaiveTime::from_hms_opt(h, m, 0) {
            return Some(t);
        }
    }
    None
}

/// "every weekday at 8am", "each monday at 9", "every morning", "daily at 6pm",
/// "every month on the 1st at 9am" → a spec. Mornings are 7:30 unless a time is said.
pub fn spec_in(text: &str) -> Option<Spec> {
    let l = text.to_lowercase();
    let time = time_in(&l).or_else(|| {
        if l.contains("morning") {
            NaiveTime::from_hms_opt(7, 30, 0)
        } else if l.contains("evening") || l.contains("night") {
            NaiveTime::from_hms_opt(18, 0, 0)
        } else if l.contains("afternoon") {
            NaiveTime::from_hms_opt(13, 0, 0)
        } else {
            None
        }
    })?;
    if l.contains("weekday") || l.contains("work day") || l.contains("workday") {
        return Some(Spec::Weekdays(time));
    }
    for (name, w) in [("monday", Weekday::Mon), ("tuesday", Weekday::Tue), ("wednesday", Weekday::Wed), ("thursday", Weekday::Thu), ("friday", Weekday::Fri), ("saturday", Weekday::Sat), ("sunday", Weekday::Sun)] {
        if l.contains(&format!("every {name}")) || l.contains(&format!("each {name}")) || l.contains(&format!("on {name}s")) || l.contains(&format!("{name}s at")) {
            return Some(Spec::Weekly(w, time));
        }
    }
    if l.contains("every month") || l.contains("monthly") || l.contains("each month") {
        // "on the 15th", "the 1st": a number followed by st/nd/rd/th.
        let day = l
            .split(|c: char| c.is_whitespace() || c == ',')
            .find_map(|w| {
                let digits: String = w.chars().take_while(|c| c.is_ascii_digit()).collect();
                let rest = &w[digits.len()..];
                (!digits.is_empty() && ["st", "nd", "rd", "th"].contains(&rest)).then(|| digits.parse::<u32>().ok()).flatten()
            })
            .filter(|n| (1..=31).contains(n))
            .unwrap_or(1);
        return Some(Spec::Monthly(day, time));
    }
    if ["every day", "everyday", "daily", "each day", "every morning", "each morning", "every evening", "every night", "every afternoon"].iter().any(|w| l.contains(w)) {
        return Some(Spec::Daily(time));
    }
    None
}

// ---------------------------------------------------------------- schedules

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// The daily briefing (briefing.rs).
    Briefing,
    /// A question asked on a schedule.
    Prompt,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Briefing => "briefing",
            Kind::Prompt => "prompt",
        }
    }
    fn from_str(s: &str) -> Kind {
        if s == "briefing" {
            Kind::Briefing
        } else {
            Kind::Prompt
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    #[serde(default)]
    pub id: i64,
    pub kind: Kind,
    pub name: String,
    pub spec: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub last_run: Option<i64>,
    #[serde(default)]
    pub next_run: Option<i64>,
    /// "Every weekday at 7:30 AM" (filled when listed).
    #[serde(default)]
    pub when: String,
    /// The chat the last run made (filled when listed).
    #[serde(default)]
    pub last_chat: Option<String>,
    #[serde(default)]
    pub last_ok: Option<bool>,
}

fn yes() -> bool {
    true
}

fn row_schedule(r: &rusqlite::Row<'_>) -> rusqlite::Result<Schedule> {
    let spec: String = r.get(3)?;
    Ok(Schedule {
        id: r.get(0)?,
        kind: Kind::from_str(&r.get::<_, String>(1)?),
        name: r.get(2)?,
        when: parse_spec(&spec).map(|s| s.describe()).unwrap_or_default(),
        spec,
        prompt: r.get(4)?,
        enabled: r.get::<_, i64>(5)? != 0,
        last_run: r.get(6)?,
        next_run: r.get(7)?,
        last_chat: r.get(8)?,
        last_ok: r.get::<_, Option<i64>>(9)?.map(|v| v != 0),
    })
}

const SCHEDULE_COLS: &str = "s.id, s.kind, s.name, s.spec, s.prompt, s.enabled, s.last_run, s.next_run,
    (SELECT conversation_id FROM runs WHERE schedule_id = s.id ORDER BY started DESC LIMIT 1),
    (SELECT ok FROM runs WHERE schedule_id = s.id AND finished IS NOT NULL ORDER BY started DESC LIMIT 1)";

fn ms<Tz: TimeZone>(t: &DateTime<Tz>) -> i64 {
    t.timestamp_millis()
}

pub fn list(db: &Db) -> AppResult<Vec<Schedule>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {SCHEDULE_COLS} FROM schedules s ORDER BY s.next_run IS NULL, s.next_run, s.id"))?;
    let v = st.query_map([], row_schedule)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

pub fn get(db: &Db, id: i64) -> AppResult<Option<Schedule>> {
    let conn = db.conn();
    Ok(conn.query_row(&format!("SELECT {SCHEDULE_COLS} FROM schedules s WHERE s.id = ?1"), [id], row_schedule).optional()?)
}

/// Adds or updates a schedule; works out its next run from `now`.
pub fn save<Tz: TimeZone>(db: &Db, s: &Schedule, now: &DateTime<Tz>) -> AppResult<Schedule> {
    let spec = parse_spec(&s.spec).ok_or_else(|| AppError::msg("BYTE didn't understand when that should run."))?;
    let name: String = s.name.trim().chars().take(120).collect();
    if name.is_empty() {
        return Err(AppError::msg("Give it a name."));
    }
    if s.kind == Kind::Prompt && s.prompt.trim().is_empty() {
        return Err(AppError::msg("Say what BYTE should do each time."));
    }
    let next = if s.enabled { next_after(&spec, now).map(|t| ms(&t)) } else { None };
    let prompt: String = s.prompt.trim().chars().take(4000).collect();
    let id = {
        let conn = db.conn();
        if s.id > 0 {
            conn.execute(
                "UPDATE schedules SET kind = ?1, name = ?2, spec = ?3, prompt = ?4, enabled = ?5, next_run = ?6 WHERE id = ?7",
                params![s.kind.as_str(), name, spec.text(), prompt, s.enabled as i64, next, s.id],
            )?;
            s.id
        } else {
            conn.execute(
                "INSERT INTO schedules (kind, name, spec, prompt, enabled, next_run) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![s.kind.as_str(), name, spec.text(), prompt, s.enabled as i64, next],
            )?;
            conn.last_insert_rowid()
        }
    };
    get(db, id)?.ok_or_else(|| AppError::msg("The schedule wasn't saved."))
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    db.conn().execute("DELETE FROM schedules WHERE id = ?1", [id])?;
    Ok(())
}

/// Schedules whose time has come; each moves on to its next time (a missed
/// run is done once, not once per missed time). One-offs switch off.
pub fn take_due<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> AppResult<Vec<Schedule>> {
    let due: Vec<Schedule> = list(db)?.into_iter().filter(|s| s.enabled && s.next_run.is_some_and(|n| n <= ms(now))).collect();
    let conn = db.conn();
    for s in &due {
        let next = parse_spec(&s.spec).and_then(|sp| next_after(&sp, now)).map(|t| ms(&t));
        conn.execute("UPDATE schedules SET last_run = ?1, next_run = ?2, enabled = CASE WHEN ?2 IS NULL THEN 0 ELSE enabled END WHERE id = ?3", params![ms(now), next, s.id])?;
    }
    Ok(due)
}

/// A new run's row; returns its id.
fn run_started(db: &Db, schedule: Option<i64>, now: i64) -> AppResult<i64> {
    let conn = db.conn();
    conn.execute("INSERT INTO runs (schedule_id, started) VALUES (?1, ?2)", params![schedule, now])?;
    Ok(conn.last_insert_rowid())
}

fn run_finished(db: &Db, run: i64, ok: bool, summary: &str, chat: Option<&str>) -> AppResult<()> {
    db.conn().execute(
        "UPDATE runs SET finished = ?1, ok = ?2, summary = ?3, conversation_id = ?4 WHERE id = ?5",
        params![chrono::Utc::now().timestamp_millis(), ok as i64, summary, chat, run],
    )?;
    // Keep the last 50 runs per schedule.
    db.conn().execute(
        "DELETE FROM runs WHERE id IN (SELECT id FROM runs r WHERE r.schedule_id = (SELECT schedule_id FROM runs WHERE id = ?1)
         AND r.id NOT IN (SELECT id FROM runs WHERE schedule_id = r.schedule_id ORDER BY started DESC LIMIT 50))",
        [run],
    )?;
    Ok(())
}

// ------------------------------------------------------------ running them

/// What an unattended turn wrote.
#[derive(Debug, Default)]
pub struct Answer {
    pub content: String,
    pub sources: serde_json::Value,
    pub error: Option<String>,
}

/// Collects a turn's events (the same ones the chat window gets).
fn collector() -> (Channel<crate::chat::ChatEvent>, Arc<StdMutex<Vec<serde_json::Value>>>) {
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let sink = seen.clone();
    let ch = Channel::new(move |body: InvokeResponseBody| {
        if let InvokeResponseBody::Json(s) = body {
            if let Ok(v) = serde_json::from_str(&s) {
                sink.lock().unwrap_or_else(|p| p.into_inner()).push(v);
            }
        }
        Ok(())
    });
    (ch, seen)
}

/// The written answer from collected events.
pub fn answer_from(events: &[serde_json::Value]) -> Answer {
    let mut a = Answer { sources: serde_json::Value::Array(vec![]), ..Default::default() };
    for e in events {
        match e["kind"].as_str() {
            Some("content") => a.content.push_str(e["delta"].as_str().unwrap_or("")),
            Some("sources") => a.sources = e["sources"].clone(),
            Some("done") if e["finishReason"] == "error" => a.error = Some("the answer stopped early".into()),
            _ => {}
        }
    }
    a
}

/// Asks a question with nobody watching, through the normal chat pipeline.
pub async fn ask_unattended(state: &AppState, question: &str) -> Answer {
    if state.engine.endpoint().await.is_none() {
        return Answer { error: Some("no model was loaded".into()), ..Default::default() };
    }
    let request = crate::commands::ChatRequest {
        request_id: format!("sched-{}", uuid::Uuid::new_v4().simple()),
        messages: vec![crate::chat::ChatMessage::new("user", question)],
        mode: crate::settings::Mode::Auto,
        thinking: crate::settings::ThinkingPref::Auto,
        model: None,
        private: false,
        project_id: None,
        assistant_id: None,
        cloud: None,
        fresh: true,
        task: None,
    };
    let (ch, seen) = collector();
    let result = crate::backend::answer(state, request, &ch).await;
    let events = seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let mut a = answer_from(&events);
    if let Err(e) = result {
        a.error = Some(e.to_string());
    }
    if a.error.is_none() && a.content.trim().is_empty() {
        a.error = Some("the model wrote nothing".into());
    }
    a
}

/// Saves a finished run as a chat the user can open; returns its id.
pub fn save_chat(db: &Db, title: &str, question: &str, answer: &Answer) -> AppResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let conv = serde_json::json!({
        "id": id,
        "title": title,
        "createdAt": now,
        "updatedAt": now,
        "messages": [
            { "id": uuid::Uuid::new_v4().to_string(), "role": "user", "content": question, "status": "done" },
            { "id": uuid::Uuid::new_v4().to_string(), "role": "assistant", "content": answer.content, "status": "done", "sources": answer.sources, "mode": "auto" },
        ],
    });
    db.save(&conv)?;
    db.conn().execute("UPDATE conversations SET title_locked = 1 WHERE id = ?1", [&id])?;
    Ok(id)
}

/// Shows a Mac notification (quietly does nothing if they're off).
pub fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}

/// Runs one schedule now: its answer becomes a chat, then a notification.
pub async fn run_schedule(app: &AppHandle, s: &Schedule) -> AppResult<Option<String>> {
    let state = app.state::<AppState>();
    let started = chrono::Utc::now().timestamp_millis();
    let run = run_started(&state.db, (s.id > 0).then_some(s.id), started)?;
    let day = chrono::Local::now().format("%a, %b %-d").to_string();
    let (title, question, answer) = match s.kind {
        Kind::Briefing => {
            let a = crate::briefing::write(&state).await;
            (format!("{} — {day}", s.name), "Daily briefing".to_string(), a)
        }
        Kind::Prompt => {
            let a = ask_unattended(&state, &s.prompt).await;
            (format!("{} — {day}", s.name), s.prompt.clone(), a)
        }
    };
    if let Some(e) = &answer.error {
        run_finished(&state.db, run, false, e, None)?;
        notify(app, &format!("BYTE couldn't run \"{}\"", s.name), &format!("{} Open BYTE to try again.", capitalize(e)));
        return Ok(None);
    }
    let chat = save_chat(&state.db, &title, &question, &answer)?;
    let summary: String = answer.content.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).unwrap_or("").chars().take(160).collect();
    run_finished(&state.db, run, true, &summary, Some(&chat))?;
    notify(app, &format!("{} is ready", s.name), &strip_md(&summary));
    let _ = tauri::Emitter::emit(app, "schedules://ran", &chat);
    Ok(Some(chat))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn strip_md(s: &str) -> String {
    s.replace("**", "").replace('*', "").replace('`', "").trim_start_matches(['-', '>', ' ']).to_string()
}

/// Checks for due reminders and schedules once.
pub async fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = chrono::Local::now();
    match crate::tasks::take_due_reminders(&state.db, &now) {
        Ok(due) => {
            for t in due {
                notify(app, "Reminder", &t.title);
            }
        }
        Err(e) => log::warn!("reminders: {e}"),
    }
    let due = match take_due(&state.db, &now) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("schedules: {e}");
            return;
        }
    };
    for s in due {
        if let Err(e) = run_schedule(app, &s).await {
            log::warn!("schedule {}: {e}", s.name);
        }
    }
    crate::watchers::tick(app).await;
}

/// Starts the loop (at launch).
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Let the engine start first, so a missed briefing has a model to write it.
        tokio::time::sleep(Duration::from_secs(45)).await;
        loop {
            tick(&app).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

// ----------------------------------------------------------------- commands

#[tauri::command]
pub fn schedules_list(state: State<'_, AppState>) -> AppResult<Vec<Schedule>> {
    list(&state.db)
}

#[tauri::command]
pub fn schedule_save(state: State<'_, AppState>, schedule: Schedule) -> AppResult<Schedule> {
    save(&state.db, &schedule, &chrono::Local::now())
}

#[tauri::command]
pub fn schedule_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete(&state.db, id)
}

/// "Run now": returns the new chat's id (None when it couldn't run; a notification says why).
#[tauri::command]
pub async fn schedule_run(app: AppHandle, id: i64) -> AppResult<Option<String>> {
    let s = {
        let state = app.state::<AppState>();
        get(&state.db, id)?.ok_or_else(|| AppError::msg("That schedule is gone."))?
    };
    run_schedule(&app, &s).await
}

/// Reads a schedule from words ("every weekday at 8am") for the panel's box.
#[tauri::command]
pub fn schedule_parse(text: String) -> Option<(String, String)> {
    spec_in(&text).map(|s| (s.text(), s.describe()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Timelike, Utc};

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }
    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn specs_round_trip_and_read_well() {
        for s in ["daily 07:30", "weekdays 08:00", "weekly fri 17:45", "monthly 31 09:00", "once 2026-10-01T08:00"] {
            assert_eq!(parse_spec(s).unwrap().text(), s);
        }
        for bad in ["", "daily", "daily 25:00", "weekly xyz 07:00", "monthly 0 07:00", "monthly 32 07:00", "hourly 07:00"] {
            assert!(parse_spec(bad).is_none(), "{bad}");
        }
        assert_eq!(Spec::Weekdays(t(7, 30)).describe(), "Every weekday at 7:30 AM");
        assert_eq!(Spec::Weekly(Weekday::Sun, t(18, 0)).describe(), "Every Sunday at 6:00 PM");
        assert_eq!(Spec::Monthly(2, t(9, 0)).describe(), "Monthly on the 2nd at 9:00 AM");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(23), "23rd");
    }

    #[test]
    fn next_times() {
        // Wed 2026-09-30 10:00 UTC.
        let now = at(2026, 9, 30, 10, 0);
        assert_eq!(next_after(&Spec::Daily(t(7, 30)), &now), Some(at(2026, 10, 1, 7, 30)));
        assert_eq!(next_after(&Spec::Daily(t(10, 0)), &now), Some(at(2026, 10, 1, 10, 0)), "strictly after now");
        assert_eq!(next_after(&Spec::Daily(t(10, 1)), &now), Some(at(2026, 9, 30, 10, 1)));
        // Friday → next is Monday.
        let fri = at(2026, 10, 2, 9, 0);
        assert_eq!(next_after(&Spec::Weekdays(t(8, 0)), &fri), Some(at(2026, 10, 5, 8, 0)));
        assert_eq!(next_after(&Spec::Weekly(Weekday::Wed, t(9, 0)), &now), Some(at(2026, 10, 7, 9, 0)));
        // The 31st in a 30-day month is its last day; February's too.
        assert_eq!(next_after(&Spec::Monthly(31, t(9, 0)), &now), Some(at(2026, 10, 31, 9, 0)));
        assert_eq!(next_after(&Spec::Monthly(31, t(9, 0)), &at(2026, 9, 1, 0, 0)), Some(at(2026, 9, 30, 9, 0)));
        assert_eq!(next_after(&Spec::Monthly(30, t(9, 0)), &at(2027, 2, 1, 0, 0)), Some(at(2027, 2, 28, 9, 0)));
        let once = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap().and_time(t(8, 0));
        assert_eq!(next_after(&Spec::Once(once), &now), Some(at(2026, 10, 1, 8, 0)));
        assert_eq!(next_after(&Spec::Once(once), &at(2026, 10, 2, 0, 0)), None);
    }

    #[test]
    fn wall_clock_times_hold_in_other_time_zones() {
        let est = FixedOffset::west_opt(4 * 3600).unwrap();
        let now = est.with_ymd_and_hms(2026, 9, 30, 23, 0, 0).unwrap();
        let next = next_after(&Spec::Daily(t(7, 30)), &now).unwrap();
        assert_eq!((next.naive_local().date().day(), next.hour(), next.minute()), (1, 7, 30));
    }

    #[test]
    fn times_and_repeats_in_words() {
        assert_eq!(time_in("at 8am"), Some(t(8, 0)));
        assert_eq!(time_in("at 8:30 pm"), Some(t(20, 30)));
        assert_eq!(time_in("at 12am"), Some(t(0, 0)));
        assert_eq!(time_in("at noon"), Some(t(12, 0)));
        assert_eq!(time_in("at 7"), Some(t(7, 0)));
        assert_eq!(time_in("at 18:15"), Some(t(18, 15)));
        assert_eq!(time_in("give me 5 things"), None);
        assert_eq!(time_in("at 7 p.m."), Some(t(19, 0)));
        assert_eq!(spec_in("every weekday at 8am summarize AI news"), Some(Spec::Weekdays(t(8, 0))));
        assert_eq!(spec_in("each monday at 9, plan my week"), Some(Spec::Weekly(Weekday::Mon, t(9, 0))));
        assert_eq!(spec_in("brief me every morning"), Some(Spec::Daily(t(7, 30))));
        assert_eq!(spec_in("daily at 6pm"), Some(Spec::Daily(t(18, 0))));
        assert_eq!(spec_in("every month on the 15th at 9am check my budget"), Some(Spec::Monthly(15, t(9, 0))));
        assert_eq!(spec_in("on fridays at 5pm"), Some(Spec::Weekly(Weekday::Fri, t(17, 0))));
        assert_eq!(spec_in("summarize AI news"), None);
        assert_eq!(spec_in("every day"), None, "no time, no guess");
    }

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        (dir, db)
    }

    #[test]
    fn schedules_are_saved_and_come_due_once() {
        let (_d, db) = db();
        let now = at(2026, 9, 30, 10, 0);
        let s = save(&db, &Schedule { id: 0, kind: Kind::Prompt, name: "AI news".into(), spec: "daily 07:30".into(), prompt: "Summarize AI news".into(), enabled: true, last_run: None, next_run: None, when: String::new(), last_chat: None, last_ok: None }, &now).unwrap();
        assert_eq!(s.next_run, Some(at(2026, 10, 1, 7, 30).timestamp_millis()));
        assert_eq!(s.when, "Every day at 7:30 AM");
        assert!(take_due(&db, &now).unwrap().is_empty());
        // Closed for three days: it runs once, then waits for tomorrow.
        let later = at(2026, 10, 4, 12, 0);
        assert_eq!(take_due(&db, &later).unwrap().len(), 1);
        assert!(take_due(&db, &later).unwrap().is_empty());
        let s = get(&db, s.id).unwrap().unwrap();
        assert_eq!((s.last_run, s.next_run), (Some(later.timestamp_millis()), Some(at(2026, 10, 5, 7, 30).timestamp_millis())));
        // A one-off switches itself off after running.
        let once = save(&db, &Schedule { spec: "once 2026-10-05T09:00".into(), id: 0, name: "Once".into(), ..s.clone() }, &later).unwrap();
        assert_eq!(take_due(&db, &at(2026, 10, 5, 9, 0)).unwrap().iter().filter(|x| x.id == once.id).count(), 1);
        assert!(!get(&db, once.id).unwrap().unwrap().enabled);
        // Switched off: no next run.
        let off = save(&db, &Schedule { enabled: false, ..s.clone() }, &later).unwrap();
        assert_eq!(off.next_run, None);
        assert!(save(&db, &Schedule { spec: "whenever".into(), ..s.clone() }, &later).is_err());
        assert!(save(&db, &Schedule { prompt: " ".into(), ..s.clone() }, &later).is_err());
        delete(&db, s.id).unwrap();
        assert!(get(&db, s.id).unwrap().is_none());
    }

    #[test]
    fn runs_are_recorded_and_the_last_chat_is_shown() {
        let (_d, db) = db();
        let now = at(2026, 9, 30, 10, 0);
        let s = save(&db, &Schedule { id: 0, kind: Kind::Briefing, name: "Morning briefing".into(), spec: "weekdays 07:30".into(), prompt: String::new(), enabled: true, last_run: None, next_run: None, when: String::new(), last_chat: None, last_ok: None }, &now).unwrap();
        let answer = Answer { content: "## TL;DR\nA quiet day.".into(), sources: serde_json::json!([]), error: None };
        let chat = save_chat(&db, "Morning briefing — Wed, Sep 30", "Daily briefing", &answer).unwrap();
        let run = run_started(&db, Some(s.id), 1).unwrap();
        run_finished(&db, run, true, "A quiet day.", Some(&chat)).unwrap();
        let listed = list(&db).unwrap();
        assert_eq!(listed[0].last_chat.as_deref(), Some(chat.as_str()));
        assert_eq!(listed[0].last_ok, Some(true));
        let loaded = db.load(&chat).unwrap().unwrap();
        assert_eq!(loaded["messages"][1]["content"], "## TL;DR\nA quiet day.");
    }

    #[test]
    fn answers_are_collected_from_chat_events() {
        let ev = vec![
            serde_json::json!({ "kind": "started", "thinking": false, "model": "m" }),
            serde_json::json!({ "kind": "content", "delta": "Hello " }),
            serde_json::json!({ "kind": "sources", "sources": [{ "n": 1 }] }),
            serde_json::json!({ "kind": "content", "delta": "there" }),
            serde_json::json!({ "kind": "done", "finishReason": "stop" }),
        ];
        let a = answer_from(&ev);
        assert_eq!(a.content, "Hello there");
        assert_eq!(a.sources[0]["n"], 1);
        assert!(a.error.is_none());
    }
}
