//! Automations (Phase 10): a trigger and a few steps BYTE does one after another.
//!
//! "Every weekday at 8am, find the latest AI news, then summarize it in five
//! bullets, then save it to a file" becomes a trigger (a scheduler spec, "when
//! BYTE opens", or only when run) and steps: ask BYTE something, write the
//! briefing, send a notification, add a to-do, save to a file, run one of the
//! user's Shortcuts. Each step's text is handed to the next (`{previous}`).
//!
//! Runs happen in the background: progress goes out as `automations://progress`
//! (the chat's run card and the ✅ panel follow it), the results are saved as
//! one chat, and a notification says it's done. A step that fails stops the
//! run; what was done so far is kept, and the run can go again from that step.
//!
//! Requests are read with rules, not a model (small models split steps badly),
//! and nothing is saved or started before the approval card is answered.

use chrono::{DateTime, TimeZone};
use futures_util::future::BoxFuture;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::scheduler::{self, parse_spec, spec_in};
use crate::state::AppState;
use crate::tools::SourceBook;

pub const PROGRESS_EVENT: &str = "automations://progress";
/// Most steps an automation can have.
const MAX_STEPS: usize = 8;
/// How much of a step's text is kept (and handed on).
const KEEP_CHARS: usize = 12_000;

// -------------------------------------------------------------------- steps

/// One thing an automation does.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Step {
    /// Ask BYTE (the normal chat pipeline: web, cards, tools).
    Ask { prompt: String },
    /// The daily briefing (briefing.rs).
    Briefing,
    /// A Mac notification.
    Notify { title: String, body: String },
    /// An item on BYTE's to-do list.
    AddTask { title: String },
    /// A Markdown file in ~/Documents/BYTE/Automations.
    SaveFile { name: String },
    /// One of the user's Shortcuts, given the text so far (macOS).
    Shortcut { name: String },
}

impl Step {
    /// What the step does, for cards and the panel.
    pub fn label(&self) -> String {
        match self {
            Step::Ask { prompt } => format!("Ask BYTE: {}", one_line(prompt, 90)),
            Step::Briefing => "Write your daily briefing".into(),
            Step::Notify { .. } => "Send you a notification".into(),
            Step::AddTask { title } if title.contains("{previous}") => "Add it to your to-do list".into(),
            Step::AddTask { title } => format!("Add \"{}\" to your to-do list", one_line(title, 60)),
            Step::SaveFile { name } => format!("Save it to Documents → BYTE → Automations as \"{}\"", file_stem(name)),
            Step::Shortcut { name } => format!("Run your shortcut \"{name}\""),
        }
    }

    fn check(&self) -> Result<(), String> {
        let empty = |s: &str| s.trim().is_empty();
        match self {
            Step::Ask { prompt } if empty(prompt) => Err("An \"Ask BYTE\" step needs a question.".into()),
            Step::AddTask { title } if empty(title) => Err("A to-do step needs a title.".into()),
            Step::Shortcut { name } if empty(name) => Err("A Shortcut step needs the shortcut's name.".into()),
            _ => Ok(()),
        }
    }
}

fn one_line(s: &str, max: usize) -> String {
    let line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max).collect::<String>().trim_end())
    } else {
        line
    }
}

fn keep(s: &str) -> String {
    s.chars().take(KEEP_CHARS).collect()
}

/// Fills `{previous}` in a step's text.
pub fn fill(template: &str, previous: &str) -> String {
    template.replace("{previous}", previous.trim())
}

/// A file name that stays inside the Automations folder: no path parts, no
/// hidden files, only ordinary characters.
pub fn file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_()&,'".contains(c) { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned: String = cleaned.trim_start_matches(['.', '-', ' ']).chars().take(60).collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() { "BYTE automation".into() } else { cleaned }
}

// ------------------------------------------------------------------ trigger

/// When an automation runs: "manual", "launch" or a scheduler spec ("weekdays 08:00").
pub fn describe_trigger(trigger: &str) -> String {
    match trigger {
        "manual" => "When you run it".into(),
        "launch" => "When BYTE opens".into(),
        spec => parse_spec(spec).map(|s| s.describe()).unwrap_or_else(|| "When you run it".into()),
    }
}

fn valid_trigger(trigger: &str) -> bool {
    matches!(trigger, "manual" | "launch") || parse_spec(trigger).is_some()
}

// --------------------------------------------------------------- automations

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    pub trigger: String,
    pub steps: Vec<Step>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub last_run: Option<i64>,
    #[serde(default)]
    pub next_run: Option<i64>,
    /// "Every weekday at 8:00 AM" (filled when listed).
    #[serde(default)]
    pub when: String,
    #[serde(default)]
    pub last_ok: Option<bool>,
    #[serde(default)]
    pub last_chat: Option<String>,
    /// A Shortcut can start it (a link key was made).
    #[serde(default)]
    pub linked: bool,
}

fn yes() -> bool {
    true
}

const COLS: &str = "a.id, a.name, a.trigger, a.steps, a.enabled, a.last_run, a.next_run, a.link_key,
    (SELECT ok FROM runs WHERE automation_id = a.id AND finished IS NOT NULL ORDER BY started DESC LIMIT 1),
    (SELECT conversation_id FROM runs WHERE automation_id = a.id AND conversation_id IS NOT NULL ORDER BY started DESC LIMIT 1)";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Automation> {
    let trigger: String = r.get(2)?;
    Ok(Automation {
        id: r.get(0)?,
        name: r.get(1)?,
        when: describe_trigger(&trigger),
        trigger,
        steps: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
        enabled: r.get::<_, i64>(4)? != 0,
        last_run: r.get(5)?,
        next_run: r.get(6)?,
        linked: !r.get::<_, String>(7)?.is_empty(),
        last_ok: r.get::<_, Option<i64>>(8)?.map(|v| v != 0),
        last_chat: r.get(9)?,
    })
}

pub fn list(db: &Db) -> AppResult<Vec<Automation>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM automations a ORDER BY a.id"))?;
    let v = st.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

pub fn get(db: &Db, id: i64) -> AppResult<Option<Automation>> {
    Ok(db.conn().query_row(&format!("SELECT {COLS} FROM automations a WHERE a.id = ?1"), [id], row).optional()?)
}

fn next_for<Tz: TimeZone>(trigger: &str, enabled: bool, now: &DateTime<Tz>) -> Option<i64> {
    if !enabled {
        return None;
    }
    parse_spec(trigger).and_then(|s| scheduler::next_after(&s, now)).map(|t| t.timestamp_millis())
}

/// Adds or updates an automation (checked first).
pub fn save<Tz: TimeZone>(db: &Db, a: &Automation, now: &DateTime<Tz>) -> AppResult<Automation> {
    let name: String = a.name.trim().chars().take(80).collect();
    if name.is_empty() {
        return Err(AppError::msg("Give the automation a name."));
    }
    if !valid_trigger(&a.trigger) {
        return Err(AppError::msg("BYTE didn't understand when that should run."));
    }
    if a.steps.is_empty() {
        return Err(AppError::msg("Add at least one step."));
    }
    if a.steps.len() > MAX_STEPS {
        return Err(AppError::msg(format!("An automation can have up to {MAX_STEPS} steps.")));
    }
    for s in &a.steps {
        s.check().map_err(AppError::msg)?;
    }
    let steps = serde_json::to_string(&a.steps)?;
    let next = next_for(&a.trigger, a.enabled, now);
    let id = {
        let conn = db.conn();
        if a.id > 0 {
            conn.execute(
                "UPDATE automations SET name = ?1, trigger = ?2, steps = ?3, enabled = ?4, next_run = ?5 WHERE id = ?6",
                params![name, a.trigger, steps, a.enabled as i64, next, a.id],
            )?;
            a.id
        } else {
            conn.execute(
                "INSERT INTO automations (name, trigger, steps, enabled, created, next_run) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![name, a.trigger, steps, a.enabled as i64, now.timestamp_millis(), next],
            )?;
            conn.last_insert_rowid()
        }
    };
    get(db, id)?.ok_or_else(|| AppError::msg("The automation wasn't saved."))
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    db.conn().execute("DELETE FROM automations WHERE id = ?1", [id])?;
    Ok(())
}

/// Scheduled automations whose time has come; each moves on to its next time.
pub fn take_due<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> AppResult<Vec<Automation>> {
    let due: Vec<Automation> = list(db)?.into_iter().filter(|a| a.enabled && a.next_run.is_some_and(|n| n <= now.timestamp_millis())).collect();
    let conn = db.conn();
    for a in &due {
        let next = next_for(&a.trigger, true, now);
        conn.execute(
            "UPDATE automations SET last_run = ?1, next_run = ?2, enabled = CASE WHEN ?2 IS NULL THEN 0 ELSE enabled END WHERE id = ?3",
            params![now.timestamp_millis(), next, a.id],
        )?;
    }
    Ok(due)
}

/// The key a Shortcut's link carries (made once); web pages can't guess it.
pub fn link_key(db: &Db, id: i64) -> AppResult<String> {
    let conn = db.conn();
    let key: Option<String> = conn.query_row("SELECT link_key FROM automations WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    let key = key.ok_or_else(|| AppError::msg("That automation is gone."))?;
    if !key.is_empty() {
        return Ok(key);
    }
    let key = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    conn.execute("UPDATE automations SET link_key = ?1 WHERE id = ?2", params![key, id])?;
    Ok(key)
}

/// The automation a link names, only when its key matches.
pub fn by_link(db: &Db, id: i64, key: &str) -> AppResult<Option<Automation>> {
    let stored: Option<String> = db.conn().query_row("SELECT link_key FROM automations WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    match stored {
        Some(k) if !k.is_empty() && same(&k, key) => get(db, id),
        _ => Ok(None),
    }
}

/// Compares without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// --------------------------------------------------------------------- runs

/// How a step went.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepRun {
    pub label: String,
    /// "waiting", "running", "done", "failed".
    pub status: String,
    /// One line: what happened.
    #[serde(default)]
    pub detail: String,
    /// The step's text (handed to the next step).
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub sources: Value,
}

impl StepRun {
    fn waiting(s: &Step) -> StepRun {
        StepRun { label: s.label(), status: "waiting".into(), detail: String::new(), output: String::new(), sources: Value::Null }
    }
}

/// A run's state, sent as `automations://progress` and kept in `runs.steps`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub run_id: i64,
    pub automation_id: i64,
    pub name: String,
    pub steps: Vec<StepRun>,
    pub finished: bool,
    pub ok: bool,
    #[serde(default)]
    pub chat: Option<String>,
}

/// What steps do in the world (the app, or a fake in tests).
pub trait Doer: Send + Sync {
    /// Asks BYTE; returns the answer and its sources.
    fn ask<'a>(&'a self, prompt: &'a str) -> BoxFuture<'a, Result<(String, Value), String>>;
    fn briefing(&self) -> BoxFuture<'_, Result<(String, Value), String>>;
    fn notify(&self, title: &str, body: &str);
    fn add_task(&self, title: &str) -> Result<String, String>;
    /// Saves the text; returns where.
    fn save_file(&self, name: &str, text: &str) -> Result<String, String>;
    /// Runs a Shortcut with the text as input; returns its output (may be empty).
    fn shortcut<'a>(&'a self, name: &'a str, input: &'a str) -> BoxFuture<'a, Result<String, String>>;
}

/// "summarize it", "translate that": the step works on the previous step's text.
fn refers_back(prompt: &str) -> bool {
    let l = format!(" {} ", prompt.to_lowercase().replace([',', '.', '!', '?', ':'], " "));
    [" it ", " that ", " this ", " them ", " those ", " these ", " the results ", " the result ", " the above ", " the answer ", " the news ", " the list "].iter().any(|w| l.contains(w))
}

/// The prompt an Ask step sends (the previous text added when it refers to it).
pub fn ask_prompt(prompt: &str, previous: &str, first: bool) -> String {
    if prompt.contains("{previous}") {
        return fill(prompt, previous);
    }
    if first || previous.trim().is_empty() || !refers_back(prompt) {
        return prompt.trim().to_string();
    }
    format!("{}\n\nHere is the text to use:\n\n{}", prompt.trim(), previous.trim())
}

/// A to-do title from the text so far: its first real line.
fn task_title(template: &str, previous: &str) -> String {
    let first = previous.lines().map(|l| l.trim().trim_start_matches(['#', '-', '*', '>', ' '])).find(|l| !l.is_empty()).unwrap_or("");
    one_line(&fill(template, &first.replace("**", "")), 120)
}

/// Does the steps from `from` on (earlier ones come from `runs`, as a resumed
/// run keeps them). Calls `on` after every change. Stops at the first failure.
pub async fn execute(doer: &dyn Doer, name: &str, steps: &[Step], mut runs: Vec<StepRun>, from: usize, cancel: &CancellationToken, on: &(dyn Fn(&[StepRun]) + Sync)) -> (Vec<StepRun>, bool) {
    runs.resize_with(steps.len(), || StepRun::waiting(&Step::Briefing));
    for (i, s) in steps.iter().enumerate().skip(from) {
        runs[i] = StepRun { status: "waiting".into(), ..StepRun::waiting(s) };
    }
    let mut previous = if from > 0 { runs.get(from - 1).map(|r| r.output.clone()).unwrap_or_default() } else { String::new() };
    for (i, step) in steps.iter().enumerate().skip(from) {
        if cancel.is_cancelled() {
            runs[i].status = "failed".into();
            runs[i].detail = "Stopped".into();
            on(&runs);
            return (runs, false);
        }
        runs[i].status = "running".into();
        on(&runs);
        let result: Result<(String, String, Value), String> = match step {
            Step::Ask { prompt } => doer.ask(&ask_prompt(prompt, &previous, i == 0)).await.map(|(t, src)| (t.clone(), format!("{} words", t.split_whitespace().count()), src)),
            Step::Briefing => doer.briefing().await.map(|(t, src)| (t, "Briefing written".into(), src)),
            Step::Notify { title, body } => {
                let title = if title.trim().is_empty() { name.to_string() } else { fill(title, &previous) };
                let body = if body.trim().is_empty() { one_line(&previous, 180) } else { one_line(&fill(body, &previous), 180) };
                let body = if body.is_empty() { format!("\"{name}\" ran.") } else { body.replace("**", "") };
                doer.notify(&title, &body);
                Ok((previous.clone(), "Notification sent".into(), Value::Null))
            }
            Step::AddTask { title } => {
                let t = task_title(title, &previous);
                if t.is_empty() {
                    Err("There was nothing to add to the to-do list.".into())
                } else {
                    doer.add_task(&t).map(|d| (previous.clone(), d, Value::Null))
                }
            }
            Step::SaveFile { name: file } => {
                if previous.trim().is_empty() {
                    Err("There was nothing to save yet.".into())
                } else {
                    doer.save_file(file, &previous).map(|path| (previous.clone(), format!("Saved to {path}"), Value::Null))
                }
            }
            Step::Shortcut { name: sc } => doer.shortcut(sc, &previous).await.map(|out| {
                let out = out.trim().to_string();
                let detail = if out.is_empty() { "Shortcut ran".to_string() } else { format!("Shortcut ran · {}", one_line(&out, 80)) };
                (if out.is_empty() { previous.clone() } else { out }, detail, Value::Null)
            }),
        };
        match result {
            Ok((output, detail, sources)) => {
                previous = keep(&output);
                runs[i] = StepRun { label: step.label(), status: "done".into(), detail, output: previous.clone(), sources };
                on(&runs);
            }
            Err(e) => {
                runs[i].status = "failed".into();
                runs[i].detail = e;
                on(&runs);
                return (runs, false);
            }
        }
    }
    (runs, true)
}

/// The run as one chat: each step's heading and text. Citations are kept for the
/// last answer only (each answer numbers its own sources).
pub fn compose(name: &str, runs: &[StepRun]) -> (String, Value) {
    let last_answer = runs.iter().rposition(|r| r.sources.as_array().is_some_and(|a| !a.is_empty()));
    let mut out = format!("**{name}**\n\n");
    for (i, r) in runs.iter().enumerate() {
        let mark = match r.status.as_str() {
            "done" => "✅",
            "failed" => "⚠️",
            _ => "⏸",
        };
        out.push_str(&format!("### {}. {} {}\n", i + 1, r.label, mark));
        let text = r.output.trim();
        let shows_text = r.status == "done" && !text.is_empty() && (i == 0 || runs[i - 1].output.trim() != text);
        if shows_text {
            let body = if Some(i) == last_answer { text.to_string() } else { without_citations(text) };
            out.push_str(&body);
            out.push_str("\n\n");
        } else if !r.detail.is_empty() {
            out.push_str(&format!("{}\n\n", r.detail));
        }
    }
    let sources = last_answer.map(|i| runs[i].sources.clone()).unwrap_or(json!([]));
    (out.trim_end().to_string(), sources)
}

fn without_citations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(p) = rest.find('[') {
        let after = &rest[p + 1..];
        let digits = after.chars().take_while(|c| c.is_ascii_digit() || *c == ',' || *c == ' ').count();
        if digits > 0 && after[digits..].starts_with(']') && after[..digits].chars().any(|c| c.is_ascii_digit()) {
            out.push_str(rest[..p].trim_end_matches(' '));
            rest = &after[digits + 1..];
        } else {
            out.push_str(&rest[..=p]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn run_started(db: &Db, automation: i64, steps: &[StepRun]) -> AppResult<i64> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO runs (automation_id, started, steps) VALUES (?1, ?2, ?3)",
        params![automation, chrono::Utc::now().timestamp_millis(), serde_json::to_string(steps)?],
    )?;
    Ok(conn.last_insert_rowid())
}

fn run_saved(db: &Db, run: i64, steps: &[StepRun]) -> AppResult<()> {
    db.conn().execute("UPDATE runs SET steps = ?1 WHERE id = ?2", params![serde_json::to_string(steps)?, run])?;
    Ok(())
}

fn run_finished(db: &Db, run: i64, automation: i64, ok: bool, summary: &str, chat: Option<&str>) -> AppResult<()> {
    let conn = db.conn();
    conn.execute(
        "UPDATE runs SET finished = ?1, ok = ?2, summary = ?3, conversation_id = ?4 WHERE id = ?5",
        params![chrono::Utc::now().timestamp_millis(), ok as i64, summary, chat, run],
    )?;
    conn.execute("UPDATE automations SET last_run = ?1 WHERE id = ?2", params![chrono::Utc::now().timestamp_millis(), automation])?;
    // Keep the last 30 runs per automation.
    conn.execute(
        "DELETE FROM runs WHERE automation_id = ?1 AND id NOT IN (SELECT id FROM runs WHERE automation_id = ?1 ORDER BY started DESC LIMIT 30)",
        [automation],
    )?;
    Ok(())
}

/// A run's saved state (the chat card reads it when a chat is opened again).
pub fn run_view(db: &Db, run: i64) -> AppResult<Option<RunView>> {
    let conn = db.conn();
    let r = conn
        .query_row(
            "SELECT r.automation_id, COALESCE(a.name, ''), r.steps, r.finished, r.ok, r.conversation_id FROM runs r LEFT JOIN automations a ON a.id = r.automation_id WHERE r.id = ?1",
            [run],
            |r| {
                Ok(RunView {
                    run_id: run,
                    automation_id: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    name: r.get(1)?,
                    steps: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default(),
                    finished: r.get::<_, Option<i64>>(3)?.is_some(),
                    ok: r.get::<_, i64>(4)? != 0,
                    chat: r.get(5)?,
                })
            },
        )
        .optional()?;
    Ok(r)
}

/// The app's way of doing steps.
struct AppDoer {
    app: AppHandle,
}

impl Doer for AppDoer {
    fn ask<'a>(&'a self, prompt: &'a str) -> BoxFuture<'a, Result<(String, Value), String>> {
        Box::pin(async move {
            let a = scheduler::ask_unattended(&self.app.state::<AppState>(), prompt).await;
            match a.error {
                Some(e) => Err(format!("BYTE couldn't answer: {e}")),
                None => Ok((a.content, a.sources)),
            }
        })
    }

    fn briefing(&self) -> BoxFuture<'_, Result<(String, Value), String>> {
        Box::pin(async move {
            let a = crate::briefing::write(&self.app.state::<AppState>()).await;
            match a.error {
                Some(e) => Err(e),
                None => Ok((a.content, a.sources)),
            }
        })
    }

    fn notify(&self, title: &str, body: &str) {
        scheduler::notify(&self.app, title, body);
    }

    fn add_task(&self, title: &str) -> Result<String, String> {
        let state = self.app.state::<AppState>();
        let t = crate::tasks::Task { id: 0, title: title.into(), notes: String::new(), due: None, remind_at: None, repeat: String::new(), done_at: None, created: 0 };
        crate::tasks::save(&state.db, &t).map(|t| format!("Added \"{}\"", t.title)).map_err(|e| e.to_string())
    }

    fn save_file(&self, name: &str, text: &str) -> Result<String, String> {
        let docs = crate::paths::user_documents_dir(&self.app).map_err(|e| e.to_string())?;
        let dir = docs.join("BYTE").join("Automations");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
        let path = dir.join(format!("{} {}.md", file_stem(name), chrono::Local::now().format("%Y-%m-%d %H%M")));
        std::fs::write(&path, text).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
        Ok(format!("Documents/BYTE/Automations/{}", path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()))
    }

    fn shortcut<'a>(&'a self, name: &'a str, input: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            if !cfg!(target_os = "macos") {
                return Err("Shortcuts are only on macOS.".into());
            }
            use crate::macctl::{Action, Command, MacRunner, Runner};
            let installed = MacRunner.run(&Action::ShortcutsList.command()).await.map_err(|e| e.text("Shortcuts"))?;
            let Some(real) = installed.lines().map(str::trim).find(|n| n.eq_ignore_ascii_case(name.trim())).map(str::to_string) else {
                return Err(format!("There's no shortcut called \"{name}\" in the Shortcuts app."));
            };
            let dir = std::env::temp_dir().join(format!("byte-shortcut-{}", uuid::Uuid::new_v4().simple()));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let (inp, out) = (dir.join("input.txt"), dir.join("output.txt"));
            std::fs::write(&inp, input).map_err(|e| e.to_string())?;
            let args = vec!["run".into(), real.clone(), "-i".into(), inp.display().to_string(), "-o".into(), out.display().to_string()];
            let result = MacRunner.run(&Command::Exec { program: "shortcuts", args }).await;
            let text = std::fs::read_to_string(&out).unwrap_or_default();
            let _ = std::fs::remove_dir_all(&dir);
            result.map(|_| text).map_err(|e| e.text("Shortcuts"))
        })
    }
}

/// Starts a run in the background; returns its id at once.
pub fn start(app: &AppHandle, a: Automation, from: usize) -> AppResult<i64> {
    let state = app.state::<AppState>();
    // Resuming keeps the steps that were done.
    let prior: Vec<StepRun> = if from > 0 {
        let conn = state.db.conn();
        let last: Option<String> = conn
            .query_row("SELECT steps FROM runs WHERE automation_id = ?1 ORDER BY started DESC LIMIT 1", [a.id], |r| r.get(0))
            .optional()?;
        last.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    } else {
        vec![]
    };
    let from = if prior.len() == a.steps.len() && prior.iter().take(from).all(|r| r.status == "done") { from } else { 0 };
    let mut initial: Vec<StepRun> = a.steps.iter().map(StepRun::waiting).collect();
    for (i, r) in prior.into_iter().take(from).enumerate() {
        initial[i] = r;
    }
    let run = run_started(&state.db, a.id, &initial)?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let emit = |runs: &[StepRun], finished: bool, ok: bool, chat: Option<String>| {
            let view = RunView { run_id: run, automation_id: a.id, name: a.name.clone(), steps: runs.iter().map(|r| StepRun { output: String::new(), sources: Value::Null, ..r.clone() }).collect(), finished, ok, chat };
            let _ = tauri::Emitter::emit(&app, PROGRESS_EVENT, &view);
        };
        let state = app.state::<AppState>();
        let on = |runs: &[StepRun]| {
            let _ = run_saved(&state.db, run, runs);
            emit(runs, false, false, None);
        };
        let doer = AppDoer { app: app.clone() };
        let (runs, ok) = execute(&doer, &a.name, &a.steps, initial, from, &CancellationToken::new(), &on).await;
        let day = chrono::Local::now().format("%a, %b %-d").to_string();
        let (content, sources) = compose(&a.name, &runs);
        let steps_text: Vec<String> = a.steps.iter().enumerate().map(|(i, s)| format!("{}. {}", i + 1, s.label())).collect();
        let question = format!("Automation \"{}\":\n{}", a.name, steps_text.join("\n"));
        let answer = scheduler::Answer { content, sources, error: None };
        let chat = scheduler::save_chat(&state.db, &format!("{} — {day}", a.name), &question, &answer).ok();
        let failed = runs.iter().position(|r| r.status == "failed");
        let summary = match failed {
            Some(i) => format!("Stopped at step {}: {}", i + 1, runs[i].detail),
            None => format!("All {} steps done", runs.len()),
        };
        let _ = run_saved(&state.db, run, &runs);
        let _ = run_finished(&state.db, run, a.id, ok, &summary, chat.as_deref());
        if ok {
            // A Notify step already told the user.
            if !a.steps.iter().any(|s| matches!(s, Step::Notify { .. })) {
                scheduler::notify(&app, &format!("\"{}\" is done", a.name), "Open BYTE to see what it did.");
            }
        } else {
            scheduler::notify(&app, &format!("\"{}\" stopped", a.name), &summary);
        }
        emit(&runs, true, ok, chat.clone());
        let _ = tauri::Emitter::emit(&app, "schedules://ran", &chat);
    });
    Ok(run)
}

/// Runs due automations (from the scheduler's tick).
pub async fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    match take_due(&state.db, &chrono::Local::now()) {
        Ok(due) => {
            for a in due {
                if let Err(e) = start(app, a, 0) {
                    log::warn!("automation: {e}");
                }
            }
        }
        Err(e) => log::warn!("automations: {e}"),
    }
}

/// "When BYTE opens" automations (once per launch, after the engine had time to start).
pub fn run_launch(app: &AppHandle) {
    let state = app.state::<AppState>();
    for a in list(&state.db).unwrap_or_default().into_iter().filter(|a| a.enabled && a.trigger == "launch") {
        if let Err(e) = start(app, a, 0) {
            log::warn!("automation at launch: {e}");
        }
    }
}

// ------------------------------------------------------------- from words

/// An automation read from a message.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub name: String,
    pub trigger: String,
    pub steps: Vec<Step>,
}

const LAUNCH_WORDS: [&str; 7] = ["when byte opens", "when byte starts", "when byte launches", "when i open byte", "when i log in", "when my mac starts", "at login"];

/// Words that belong to a "when" phrase ("every weekday at 8am").
fn trigger_word(w: &str) -> bool {
    let w = w.trim_matches(|c: char| c == ',' || c == ':' || c == '.');
    if w.is_empty() || w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return true;
    }
    [
        "every", "each", "on", "at", "in", "the", "weekday", "weekdays", "workday", "workdays", "day", "daily", "morning", "mornings", "evening", "evenings", "night", "nights",
        "afternoon", "afternoons", "month", "monthly", "am", "pm", "noon", "midnight", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "mondays",
        "tuesdays", "wednesdays", "thursdays", "fridays", "saturdays", "sundays", "everyday", "a.m", "p.m", "of",
    ]
    .contains(&w)
}

/// Splits "when … , steps" into the trigger and the steps' text.
fn split_trigger(text: &str) -> (String, String) {
    let t = text.trim();
    let l = t.to_lowercase();
    for w in LAUNCH_WORDS {
        if let Some(p) = l.find(w) {
            let rest = format!("{} {}", &t[..p], &t[p + w.len()..]);
            return ("launch".into(), rest.trim().trim_start_matches([',', ':']).trim().to_string());
        }
    }
    let Some(spec) = spec_in(t) else {
        return ("manual".into(), t.to_string());
    };
    // "Every weekday at 8am, …": the part before the first comma is the "when".
    if let Some((head, rest)) = t.split_once(',') {
        if spec_in(head).is_some() {
            return (spec.text(), rest.trim().to_string());
        }
    }
    // Otherwise drop the "when" words from the front ("every morning check …").
    let words: Vec<&str> = t.split_whitespace().collect();
    let skip = words.iter().take_while(|w| trigger_word(&w.to_lowercase())).count();
    (spec.text(), words[skip..].join(" "))
}

/// Separators between steps.
const THEN: [&str; 8] = ["; ", ", and then ", " and then ", ", then ", " then ", ". then ", ". after that, ", ", after that "];

/// Words that start a step joined with "and" ("… and save it to a file").
const STEP_VERBS: [&str; 11] = ["save ", "send me ", "notify me", "add it ", "add that ", "add them ", "put it ", "run my ", "run the ", "text me", "let me know"];

fn split_steps(body: &str) -> Vec<String> {
    let mut parts = vec![body.to_string()];
    for sep in THEN {
        parts = parts
            .into_iter()
            .flat_map(|p| {
                let l = p.to_lowercase();
                let mut out = vec![];
                let mut start = 0;
                let mut from = 0;
                while let Some(i) = l[from..].find(sep) {
                    let at = from + i;
                    out.push(p[start..at].to_string());
                    start = at + sep.len();
                    from = start;
                }
                out.push(p[start..].to_string());
                out
            })
            .collect();
    }
    // "… and save it", ", and send me …": only before a step's own verb.
    let mut steps = vec![];
    for p in parts {
        let mut rest = p.clone();
        loop {
            let l = rest.to_lowercase();
            let cut = [", and ", " and "].iter().find_map(|and| {
                let mut from = 0;
                while let Some(i) = l[from..].find(and) {
                    let at = from + i;
                    let after = &l[at + and.len()..];
                    if STEP_VERBS.iter().any(|v| after.starts_with(v)) {
                        return Some((at, and.len()));
                    }
                    from = at + and.len();
                }
                None
            });
            match cut {
                Some((at, len)) => {
                    steps.push(rest[..at].to_string());
                    rest = rest[at + len..].to_string();
                }
                None => {
                    steps.push(rest);
                    break;
                }
            }
        }
    }
    steps
        .into_iter()
        .map(|s| {
            let s = s.trim().trim_matches(|c: char| c == ',' || c == '.' || c == ';').trim();
            let l = s.to_lowercase();
            let s = ["first, ", "first ", "then ", "and ", "finally, ", "finally ", "also "].iter().find(|w| l.starts_with(*w)).map(|w| &s[w.len()..]).unwrap_or(s);
            s.trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// The name after "called"/"named" ("save it to a file called AI news").
fn called(clause: &str) -> Option<String> {
    let l = clause.to_lowercase();
    ["called ", "named ", "titled "].iter().find_map(|w| {
        l.find(w).map(|p| clause[p + w.len()..].trim().trim_matches(|c: char| c == '"' || c == '\'' || c == '“' || c == '”' || c == '.').to_string()).filter(|n| !n.is_empty())
    })
}

/// One clause → one step.
fn step_of(clause: &str, name: &str) -> Step {
    let l = clause.to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| l.contains(w));
    if has(&["my briefing", "the briefing", "daily briefing", "morning briefing", "brief me"]) && l.split_whitespace().count() <= 8 {
        return Step::Briefing;
    }
    if (l.starts_with("save") || l.starts_with("put") || l.starts_with("write it") || l.starts_with("store"))
        && has(&[" file", " document", " note", " text file", " markdown", "documents"])
    {
        return Step::SaveFile { name: called(clause).unwrap_or_else(|| name.to_string()) };
    }
    if has(&["notify me", "send me a notification", "send a notification", "notification", "alert me", "let me know", "ping me"]) && l.split_whitespace().count() <= 12 {
        return Step::Notify { title: name.to_string(), body: "{previous}".into() };
    }
    if (l.starts_with("add ") || l.starts_with("put ")) && has(&["to-do", "todo", "to do list", "my tasks", "task list"]) {
        let what = l.trim_start_matches("add ").trim_start_matches("put ");
        let end = ["to my", "on my", "to the", "onto my"].iter().filter_map(|w| what.find(w)).min().unwrap_or(what.len());
        let item = clause[clause.len() - what.len()..][..end].trim();
        let title = if item.is_empty() || ["it", "that", "this", "them", "the result", "the results"].contains(&item.to_lowercase().as_str()) { "{previous}".to_string() } else { item.to_string() };
        return Step::AddTask { title };
    }
    if l.starts_with("run ") && l.contains("shortcut") {
        // "run my shortcut Log Water", "run the Log Water shortcut", "run shortcut X".
        let mut rest = clause[4..].trim();
        for w in ["my ", "the "] {
            if rest.to_lowercase().starts_with(w) {
                rest = rest[w.len()..].trim();
            }
        }
        let name = if rest.to_lowercase().starts_with("shortcut") {
            rest[8..].trim().trim_start_matches(['"', ':', ' ']).trim_end_matches('"')
        } else {
            rest.rsplit_once(|c: char| c.is_whitespace()).filter(|(_, last)| last.eq_ignore_ascii_case("shortcut")).map(|(n, _)| n).unwrap_or(rest)
        };
        let name = name.trim().trim_matches(['"', '“', '”']).to_string();
        return Step::Shortcut { name };
    }
    Step::Ask { prompt: clause.trim().to_string() }
}

/// A short name for the automation.
fn draft_name(steps: &[Step], trigger: &str) -> String {
    let first = steps.iter().find_map(|s| match s {
        Step::Ask { prompt } => Some(prompt.clone()),
        Step::Briefing => Some("Daily briefing".into()),
        _ => None,
    });
    let base = first.map(|p| {
        let words: Vec<&str> = p.split_whitespace().take(5).collect();
        let mut s = words.join(" ").trim_end_matches(['.', ',', '?']).to_string();
        if let Some(c) = s.get(0..1) {
            s = c.to_uppercase() + &s[1..];
        }
        s
    });
    match (base, trigger) {
        (Some(b), _) => b,
        (None, "launch") => "When BYTE opens".into(),
        _ => "My automation".into(),
    }
}

/// Reads an automation from a message: a "when" (optional) and two or more steps
/// joined by "then", ";" or "and save/send/add/run…".
pub fn plan(text: &str) -> Option<Draft> {
    let t = text.trim().trim_end_matches(['.', '!']);
    let l = t.to_lowercase();
    // Questions and reminders go elsewhere.
    if t.ends_with('?') || ["remind me", "what ", "how ", "why ", "who ", "when did", "can you explain", "is it ", "are there"].iter().any(|w| l.starts_with(w)) {
        return None;
    }
    let (trigger, body) = split_trigger(t);
    let has_sep = THEN.iter().any(|s| body.to_lowercase().contains(s))
        || [", and ", " and "].iter().any(|a| STEP_VERBS.iter().any(|v| body.to_lowercase().contains(&format!("{a}{v}"))));
    if !has_sep {
        return None;
    }
    let clauses = split_steps(&body);
    if clauses.len() < 2 || clauses.len() > MAX_STEPS {
        return None;
    }
    let provisional: Vec<Step> = clauses.iter().map(|c| step_of(c, "")).collect();
    let name = draft_name(&provisional, &trigger);
    let steps: Vec<Step> = clauses.iter().map(|c| step_of(c, &name)).collect();
    // Each "ask" part must be an instruction ("find …", "summarize …"), so
    // "write a story about a dragon and then a knight" stays one request.
    if clauses.iter().zip(&steps).any(|(c, s)| matches!(s, Step::Ask { .. }) && !starts_with_verb(c)) {
        return None;
    }
    // Something must be done first (a notification or file needs text).
    if !matches!(steps[0], Step::Ask { .. } | Step::Briefing | Step::Shortcut { .. }) {
        return None;
    }
    if steps.iter().any(|s| s.check().is_err()) {
        return None;
    }
    Some(Draft { name, trigger, steps })
}

/// Instructions a step can start with.
const VERBS: [&str; 58] = [
    "find", "search", "look", "research", "check", "get", "summarize", "summarise", "write", "draft", "make", "create", "list", "translate", "compare", "explain", "give",
    "tell", "show", "rewrite", "shorten", "turn", "pick", "plan", "suggest", "collect", "gather", "read", "review", "outline", "answer", "calculate", "convert", "fetch",
    "pull", "grab", "rank", "sort", "choose", "brainstorm", "come", "generate", "ask", "put", "format", "fix", "proofread", "simplify", "expand", "analyze", "analyse",
    "extract", "note", "keep", "highlight", "describe", "prepare", "catch",
];

fn starts_with_verb(clause: &str) -> bool {
    let l = clause.to_lowercase();
    let first = l.trim_start_matches("please ").split_whitespace().next().unwrap_or("");
    VERBS.contains(&first.trim_matches(|c: char| !c.is_alphabetic()))
}

/// A message that asks for an automation (or a multi-step run).
pub fn applies(q: &str) -> bool {
    plan(q).is_some()
}

// ---------------------------------------------------------------- in chat

/// The chat card that follows a run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunCard {
    pub run_id: i64,
    pub automation_id: i64,
    pub name: String,
    pub when: String,
    pub steps: Vec<StepRun>,
}

pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let (Some(app), Some(draft)) = (turn.app, plan(question)) else {
        return Ok(None);
    };
    let db = &app.state::<AppState>().inner().db;
    let id = format!("byte_auto_{}", uuid::Uuid::new_v4().simple());
    let when = describe_trigger(&draft.trigger);
    send(ChatEvent::ToolCall { id: id.clone(), name: "automation_plan".into(), args: json!({ "steps": draft.steps.len() }) })?;
    send(ChatEvent::ToolResult { id: id.clone(), ok: true, summary: format!("{} steps", draft.steps.len()) })?;
    let now_run = draft.trigger == "manual";
    let mut fields = vec![("When".to_string(), if now_run { "Now (and whenever you run it again from the ✅ panel)".to_string() } else { when.clone() })];
    for (i, s) in draft.steps.iter().enumerate() {
        fields.push((format!("Step {}", i + 1), s.label()));
    }
    let note = if now_run {
        "BYTE does the steps one after another in the background and shows each one here. The result is saved as a chat, and you get a notification. If a step fails, BYTE stops there and you can run it again from that step."
    } else if draft.trigger == "launch" {
        "It runs each time BYTE opens. Each run is saved as a chat, and you get a notification. Change or stop it in the ✅ panel."
    } else {
        "It runs while BYTE is open (a missed run happens when you next open it; Settings → General can open BYTE at login). Each run is saved as a chat, and you get a notification. Change or stop it in the ✅ panel."
    };
    fields.push(("Note".to_string(), note.to_string()));
    let title = if now_run { "Do these steps?" } else { "Add this automation?" };
    if !crate::macctl::ask_ok(title, "BYTE", fields, cancel, send).await? {
        send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
        return Ok(Some((SourceBook::default(), "The user chose not to run or save these steps. Nothing was done. Say so in one sentence.".into())));
    }
    let saved = save(db, &Automation { id: 0, name: draft.name.clone(), trigger: draft.trigger.clone(), steps: draft.steps.clone(), enabled: true, last_run: None, next_run: None, when: String::new(), last_ok: None, last_chat: None, linked: false }, &chrono::Local::now())?;
    if now_run {
        let run_id = start(app, saved.clone(), 0)?;
        send(ChatEvent::AutomationRun(RunCard { run_id, automation_id: saved.id, name: saved.name.clone(), when: saved.when.clone(), steps: saved.steps.iter().map(StepRun::waiting).collect() }))?;
        turn.log.record("automation_run", &json!({ "name": saved.name, "steps": saved.steps.len() }), true, "started");
        return Ok(Some((SourceBook::default(), format!(
            "BYTE started \"{}\": {} steps, done one after another in the background (the card above shows each step as it goes). The result will be saved as a chat and the user gets a notification. It's also saved in the ✅ panel to run again. Tell them this in one or two short sentences; don't do the steps yourself.",
            saved.name,
            saved.steps.len()
        ))));
    }
    let next = saved.next_run.map(|n| chrono::Local.timestamp_millis_opt(n).single().map(|t| t.format("%a %-I:%M %p").to_string()).unwrap_or_default());
    send(ChatEvent::MacDone(crate::macctl::MacDone {
        app: "BYTE".into(),
        title: format!("Added the automation \"{}\"", saved.name),
        detail: match &next {
            Some(n) => format!("{when} · first run {n}"),
            None => when.clone(),
        },
        ok: true,
        undo: None,
    }))?;
    turn.log.record("automation_add", &json!({ "name": saved.name, "when": when }), true, "saved");
    Ok(Some((SourceBook::default(), format!(
        "Done: the automation \"{}\" is saved: {} it does {} steps ({}). It's in the ✅ panel, where it can be run now, changed or turned off. Confirm in one or two short sentences.",
        saved.name,
        when.to_lowercase(),
        saved.steps.len(),
        saved.steps.iter().map(Step::label).collect::<Vec<_>>().join("; ")
    ))))
}

// ----------------------------------------------------------------- commands

#[tauri::command]
pub fn automations_list(state: State<'_, AppState>) -> AppResult<Vec<Automation>> {
    list(&state.db)
}

#[tauri::command]
pub fn automation_save(state: State<'_, AppState>, automation: Automation) -> AppResult<Automation> {
    save(&state.db, &automation, &chrono::Local::now())
}

#[tauri::command]
pub fn automation_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete(&state.db, id)
}

/// Runs it now (from step `from`, 0-based, to go again after a failure); returns the run's id.
#[tauri::command]
pub fn automation_run(app: AppHandle, id: i64, from: Option<usize>) -> AppResult<i64> {
    let a = get(&app.state::<AppState>().db, id)?.ok_or_else(|| AppError::msg("That automation is gone."))?;
    start(&app, a, from.unwrap_or(0))
}

#[tauri::command]
pub fn automation_run_status(state: State<'_, AppState>, run_id: i64) -> AppResult<Option<RunView>> {
    Ok(run_view(&state.db, run_id)?.map(|mut v| {
        for s in &mut v.steps {
            s.output.clear();
            s.sources = Value::Null;
        }
        v
    }))
}

/// Reads a "when" from words ("every weekday at 8am", "when BYTE opens") for the builder.
#[tauri::command]
pub fn automation_trigger_parse(text: String) -> Option<(String, String)> {
    let l = text.to_lowercase();
    if LAUNCH_WORDS.iter().any(|w| l.contains(w)) || l.contains("launch") || l.contains("opens") {
        return Some(("launch".into(), describe_trigger("launch")));
    }
    spec_in(&text).map(|s| (s.text(), s.describe()))
}

#[cfg(test)]
#[path = "automations_tests.rs"]
mod tests;
