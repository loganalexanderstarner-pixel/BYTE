//! Saved jobs: a name, plain-words instructions and WHEN they run (a schedule or a trigger), kept per profile in the
//! encrypted database. Every job is created switched off. Its run history is kept beside it.

use chrono::{DateTime, TimeZone};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::scheduler::{next_after, parse_spec};

/// Jobs a person can keep (generous; 0 is not offered here, the list is short in practice).
pub const MAX_JOBS: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum When {
    /// Every day at "HH:MM".
    Daily { time: String },
    /// Monday to Friday at "HH:MM".
    Weekdays { time: String },
    /// Every N hours (BYTE has to be open).
    EveryHours { hours: u32 },
    /// A download finished (a new, complete file in the Downloads folder).
    Download,
    /// A new file appears in this folder.
    Folder { path: String },
    /// A new message in an allowed channel (`channel` empty: any allowed channel of that connector).
    Message { connector: String, channel: String },
    /// BYTE starts.
    Start,
    /// A page watcher noticed a change (its id, from the watchers list).
    Watcher { id: i64 },
}

impl When {
    pub fn is_schedule(&self) -> bool {
        matches!(self, When::Daily { .. } | When::Weekdays { .. } | When::EveryHours { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            When::Daily { time } => parse_spec(&format!("daily {time}")).map(|s| s.describe()).unwrap_or_else(|| "Every day".into()),
            When::Weekdays { time } => parse_spec(&format!("weekdays {time}")).map(|s| s.describe()).unwrap_or_else(|| "Every weekday".into()),
            When::EveryHours { hours } => if *hours == 1 { "Every hour".to_string() } else { format!("Every {hours} hours") },
            When::Download => "When a download finishes".into(),
            When::Folder { path } => format!("When a new file appears in {path}"),
            When::Message { connector, channel } => format!("When a new message arrives in {}{}", if channel.is_empty() { "any allowed channel of " } else { "an allowed channel of " }, connector),
            When::Start => "When BYTE starts".into(),
            When::Watcher { id } => format!("When watched page {id} changes"),
        }
    }

    /// Errors in plain words, for the settings form.
    pub fn check(&self) -> Result<(), String> {
        match self {
            When::Daily { time } | When::Weekdays { time } => {
                chrono::NaiveTime::parse_from_str(time, "%H:%M").map(|_| ()).map_err(|_| "Pick a time like 07:30.".to_string())
            }
            When::EveryHours { hours } => (1..=168).contains(hours).then_some(()).ok_or_else(|| "Every how many hours? Pick 1 to 168.".to_string()),
            When::Folder { path } => (!path.trim().is_empty()).then_some(()).ok_or_else(|| "Pick a folder to watch.".to_string()),
            When::Message { connector, .. } => crate::messaging::known(connector).then_some(()).ok_or_else(|| "Pick a connector.".to_string()),
            _ => Ok(()),
        }
    }

    /// The next time a schedule is due after `now` (None for triggers).
    pub fn next<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
        match self {
            When::Daily { time } => next_after(&parse_spec(&format!("daily {time}"))?, now),
            When::Weekdays { time } => next_after(&parse_spec(&format!("weekdays {time}"))?, now),
            When::EveryHours { hours } => Some(now.clone() + chrono::Duration::hours(*hours as i64)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub connector: String,
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Job {
    pub id: i64,
    pub name: String,
    /// What to do, in plain words.
    pub instruction: String,
    pub when: When,
    /// Off until the person turns it on.
    pub enabled: bool,
    /// The tool groups the job may use (`toolbox::ALL_GROUPS`); empty: it only writes from what it is given.
    pub tools: Vec<String>,
    /// The job operates the PC (also set when the words ask for it). Runs only while PC control is on.
    pub needs_pc: bool,
    /// Say the result out loud when it finishes.
    pub speak: bool,
    /// Also post the result to an allowed channel.
    pub post_to: Option<Target>,
    /// "" (the instruction) or "digest" (the morning digest).
    pub template: String,
    /// Runs per day at most (0 = Unlimited).
    pub max_runs_per_day: u32,
    /// Finished runs kept (0 = Unlimited).
    pub keep_runs: u32,
    /// Digest: say it out loud / which connectors' channels it may summarise are set on the connectors.
    pub next_run: Option<i64>,
    pub last_run: Option<i64>,
    pub last_ok: Option<bool>,
    pub last_chat: Option<String>,
    /// Filled when listed.
    pub describe: String,
}

impl Default for Job {
    fn default() -> Self {
        Job { id: 0, name: String::new(), instruction: String::new(), when: When::Daily { time: "08:00".into() }, enabled: false, tools: vec!["web".into()], needs_pc: false, speak: false, post_to: None, template: String::new(), max_runs_per_day: 24, keep_runs: 50, next_run: None, last_run: None, last_ok: None, last_chat: None, describe: String::new() }
    }
}

impl Job {
    /// Cleans and checks a job from the form. Instructions that ask for PC actions mark the job as needing PC control.
    pub fn prepared(mut self) -> AppResult<Job> {
        self.name = self.name.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
        self.instruction = self.instruction.trim().chars().take(4000).collect();
        if self.name.is_empty() {
            return Err(AppError::msg("Give the job a name."));
        }
        if self.template.is_empty() && self.instruction.is_empty() {
            return Err(AppError::msg("Say in a few words what the job should do."));
        }
        if !["", "digest"].contains(&self.template.as_str()) {
            return Err(AppError::msg("Unknown job template."));
        }
        self.when.check().map_err(AppError::msg)?;
        self.tools.retain(|t| super::toolbox::ALL_GROUPS.contains(&t.as_str()));
        self.tools.dedup();
        if self.template.is_empty() && crate::agent::is_pc_action(&self.instruction) {
            self.needs_pc = true;
        }
        if self.needs_pc && !self.tools.iter().any(|t| t == "pc") {
            self.tools.push("pc".into());
        }
        if !self.needs_pc {
            self.tools.retain(|t| t != "pc");
        }
        if let Some(t) = &self.post_to {
            if !crate::messaging::known(&t.connector) || t.channel.is_empty() {
                return Err(AppError::msg("Pick where to post the result, or turn that off."));
            }
        }
        Ok(self)
    }
}

pub fn ensure(db: &Db) -> AppResult<()> {
    db.conn().execute_batch(
        "CREATE TABLE IF NOT EXISTS byte_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, body TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 0, next_run INTEGER, created INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS byte_job_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT, job_id INTEGER NOT NULL, started INTEGER NOT NULL, finished INTEGER,
            ok INTEGER, summary TEXT NOT NULL DEFAULT '', conversation_id TEXT, trigger TEXT NOT NULL DEFAULT '');
         CREATE INDEX IF NOT EXISTS byte_job_runs_job ON byte_job_runs (job_id, started);",
    )?;
    Ok(())
}

fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    let body: String = r.get(2)?;
    let mut j: Job = serde_json::from_str(&body).unwrap_or_default();
    j.id = r.get(0)?;
    j.name = r.get(1)?;
    j.enabled = r.get::<_, i64>(3)? != 0;
    j.next_run = r.get(4)?;
    j.last_run = r.get(5)?;
    j.last_ok = r.get::<_, Option<i64>>(6)?.map(|v| v != 0);
    j.last_chat = r.get(7)?;
    j.describe = j.when.describe();
    Ok(j)
}

const COLS: &str = "j.id, j.name, j.body, j.enabled, j.next_run,
    (SELECT started FROM byte_job_runs WHERE job_id = j.id ORDER BY started DESC LIMIT 1),
    (SELECT ok FROM byte_job_runs WHERE job_id = j.id AND finished IS NOT NULL ORDER BY started DESC LIMIT 1),
    (SELECT conversation_id FROM byte_job_runs WHERE job_id = j.id AND conversation_id IS NOT NULL ORDER BY started DESC LIMIT 1)";

pub fn list(db: &Db) -> AppResult<Vec<Job>> {
    ensure(db)?;
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM byte_jobs j ORDER BY j.name COLLATE NOCASE, j.id"))?;
    let v = st.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

pub fn get(db: &Db, id: i64) -> AppResult<Option<Job>> {
    ensure(db)?;
    Ok(db.conn().query_row(&format!("SELECT {COLS} FROM byte_jobs j WHERE j.id = ?1"), [id], from_row).optional()?)
}

/// Saves a job (new when `id` is 0). A schedule that is on gets its next time.
pub fn save<Tz: TimeZone>(db: &Db, job: Job, now: &DateTime<Tz>) -> AppResult<Job> {
    ensure(db)?;
    let mut j = job.prepared()?;
    if j.id == 0 && list(db)?.len() >= MAX_JOBS {
        return Err(AppError::msg("That's a lot of jobs. Remove one you no longer need first."));
    }
    let next = if j.enabled { j.when.next(now).map(|t| t.timestamp_millis()) } else { None };
    let mut stored = j.clone();
    (stored.id, stored.enabled, stored.next_run, stored.last_run, stored.last_ok, stored.last_chat, stored.describe) = (0, false, None, None, None, None, String::new());
    let body = serde_json::to_string(&stored)?;
    let conn = db.conn();
    if j.id == 0 {
        conn.execute("INSERT INTO byte_jobs (name, body, enabled, next_run, created) VALUES (?1, ?2, ?3, ?4, ?5)", params![j.name, body, j.enabled as i64, next, now.timestamp_millis()])?;
        j.id = conn.last_insert_rowid();
    } else {
        let n = conn.execute("UPDATE byte_jobs SET name = ?1, body = ?2, enabled = ?3, next_run = ?4 WHERE id = ?5", params![j.name, body, j.enabled as i64, next, j.id])?;
        if n == 0 {
            return Err(AppError::msg("That job no longer exists."));
        }
    }
    drop(conn);
    get(db, j.id)?.ok_or_else(|| AppError::msg("The job could not be saved."))
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    ensure(db)?;
    let conn = db.conn();
    conn.execute("DELETE FROM byte_job_runs WHERE job_id = ?1", [id])?;
    conn.execute("DELETE FROM byte_jobs WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_enabled<Tz: TimeZone>(db: &Db, id: i64, on: bool, now: &DateTime<Tz>) -> AppResult<Job> {
    let j = get(db, id)?.ok_or_else(|| AppError::msg("That job no longer exists."))?;
    save(db, Job { enabled: on, ..j }, now)
}

/// Enabled schedules that are due. Their next time is moved on straight away, so a slow run is not started twice, and a
/// run missed while BYTE was closed happens once.
pub fn take_due<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> AppResult<Vec<Job>> {
    let due: Vec<Job> = list(db)?.into_iter().filter(|j| j.enabled && j.next_run.is_some_and(|t| t <= now.timestamp_millis())).collect();
    for j in &due {
        let next = j.when.next(now).map(|t| t.timestamp_millis());
        db.conn().execute("UPDATE byte_jobs SET next_run = ?1 WHERE id = ?2", params![next, j.id])?;
    }
    Ok(due)
}

// ------------------------------------------------------------------ history

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRow {
    pub id: i64,
    pub job_id: i64,
    pub started: i64,
    pub finished: Option<i64>,
    pub ok: Option<bool>,
    /// What it did, or why it didn't run.
    pub summary: String,
    pub chat: Option<String>,
    /// "schedule", "manual", "download", "folder", "message", "start", "watcher".
    pub trigger: String,
}

pub fn run_started(db: &Db, job: i64, now: i64, trigger: &str) -> AppResult<i64> {
    ensure(db)?;
    let conn = db.conn();
    conn.execute("INSERT INTO byte_job_runs (job_id, started, trigger) VALUES (?1, ?2, ?3)", params![job, now, trigger])?;
    Ok(conn.last_insert_rowid())
}

pub fn run_finished(db: &Db, run: i64, now: i64, ok: bool, summary: &str, chat: Option<&str>) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("UPDATE byte_job_runs SET finished = ?1, ok = ?2, summary = ?3, conversation_id = ?4 WHERE id = ?5", params![now, ok as i64, summary.chars().take(300).collect::<String>(), chat, run])?;
    let job: Option<i64> = conn.query_row("SELECT job_id FROM byte_job_runs WHERE id = ?1", [run], |r| r.get(0)).optional()?;
    let keep: Option<String> = job.and_then(|j| conn.query_row("SELECT body FROM byte_jobs WHERE id = ?1", [j], |r| r.get(0)).optional().ok().flatten());
    let keep = keep.and_then(|b| serde_json::from_str::<Job>(&b).ok()).map(|j| j.keep_runs).unwrap_or(50);
    if let (Some(j), true) = (job, keep > 0) {
        conn.execute("DELETE FROM byte_job_runs WHERE job_id = ?1 AND id NOT IN (SELECT id FROM byte_job_runs WHERE job_id = ?1 ORDER BY started DESC, id DESC LIMIT ?2)", params![j, keep])?;
    }
    Ok(())
}

pub fn runs(db: &Db, job: Option<i64>, limit: usize) -> AppResult<Vec<RunRow>> {
    ensure(db)?;
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, job_id, started, finished, ok, summary, conversation_id, trigger FROM byte_job_runs WHERE (?1 IS NULL OR job_id = ?1) ORDER BY started DESC, id DESC LIMIT ?2")?;
    let v = st
        .query_map(params![job, if limit == 0 { -1 } else { limit as i64 }], |r| {
            Ok(RunRow { id: r.get(0)?, job_id: r.get(1)?, started: r.get(2)?, finished: r.get(3)?, ok: r.get::<_, Option<i64>>(4)?.map(|v| v != 0), summary: r.get(5)?, chat: r.get(6)?, trigger: r.get(7)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

/// Runs started in the last 24 hours (for the per-day limit).
pub fn runs_today(db: &Db, job: i64, now: i64) -> AppResult<u32> {
    ensure(db)?;
    Ok(db.conn().query_row("SELECT COUNT(*) FROM byte_job_runs WHERE job_id = ?1 AND started > ?2 AND summary NOT LIKE 'Skipped:%'", params![job, now - 86_400_000], |r| r.get::<_, i64>(0))? as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone as _};

    fn db() -> (tempfile::TempDir, Db) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(d.path()).unwrap();
        (d, db)
    }

    fn at(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 14, h, m, 0).unwrap()
    }

    fn job(name: &str) -> Job {
        Job { name: name.into(), instruction: "Summarise the news on Rust".into(), ..Default::default() }
    }

    #[test]
    fn a_new_job_is_off_and_has_no_next_time() {
        let (_d, db) = db();
        let j = save(&db, job("News"), &at(9, 0)).unwrap();
        assert!(!j.enabled && j.next_run.is_none() && j.id > 0);
        assert_eq!(j.describe, "Every day at 8:00 AM");
    }

    #[test]
    fn turning_one_on_sets_the_next_time_and_it_runs_once_when_due() {
        let (_d, db) = db();
        let j = save(&db, job("News"), &at(9, 0)).unwrap();
        let j = set_enabled(&db, j.id, true, &at(9, 0)).unwrap();
        assert_eq!(j.next_run, Some(Local.with_ymd_and_hms(2026, 10, 15, 8, 0, 0).unwrap().timestamp_millis()));
        assert!(take_due(&db, &at(10, 0)).unwrap().is_empty());
        let late = Local.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).unwrap();
        assert_eq!(take_due(&db, &late).unwrap().len(), 1, "a missed run happens once");
        assert!(take_due(&db, &late).unwrap().is_empty());
    }

    #[test]
    fn hours_triggers_and_checks() {
        let (_d, db) = db();
        let mut j = job("Every 3");
        j.when = When::EveryHours { hours: 3 };
        j.enabled = true;
        let j = save(&db, j, &at(9, 0)).unwrap();
        assert_eq!(j.next_run, Some(at(12, 0).timestamp_millis()));
        let mut t = job("Dl");
        t.when = When::Download;
        t.enabled = true;
        assert!(save(&db, t, &at(9, 0)).unwrap().next_run.is_none(), "triggers have no clock time");
        let mut bad = job("Bad");
        bad.when = When::EveryHours { hours: 0 };
        assert!(save(&db, bad, &at(9, 0)).is_err());
        assert!(save(&db, Job { name: "".into(), ..job("x") }, &at(9, 0)).is_err());
        assert!(save(&db, Job { instruction: "".into(), ..job("y") }, &at(9, 0)).is_err());
    }

    #[test]
    fn asking_for_the_pc_marks_the_job_and_pc_is_never_a_default_tool() {
        let p = Job { instruction: "open notepad".into(), ..job("Pc") }.prepared().unwrap();
        assert!(p.needs_pc && p.tools.contains(&"pc".to_string()));
        let q = Job { tools: vec!["web".into(), "pc".into(), "bogus".into()], ..job("Web") }.prepared().unwrap();
        assert!(!q.needs_pc && q.tools == vec!["web".to_string()]);
    }

    #[test]
    fn history_is_kept_with_a_limit_and_unlimited_is_a_choice() {
        let (_d, db) = db();
        let j = save(&db, Job { keep_runs: 2, ..job("H") }, &at(9, 0)).unwrap();
        for i in 0..5 {
            let r = run_started(&db, j.id, i, "manual").unwrap();
            run_finished(&db, r, i + 1, true, "done", None).unwrap();
        }
        assert_eq!(runs(&db, Some(j.id), 50).unwrap().len(), 2);
        let u = save(&db, Job { id: j.id, keep_runs: 0, ..job("H") }, &at(9, 0)).unwrap();
        for i in 10..15 {
            let r = run_started(&db, u.id, i, "manual").unwrap();
            run_finished(&db, r, i + 1, i % 2 == 0, "x", Some("chat")).unwrap();
        }
        assert_eq!(runs(&db, Some(u.id), 0).unwrap().len(), 7);
        assert_eq!(get(&db, u.id).unwrap().unwrap().last_chat.as_deref(), Some("chat"));
        delete(&db, u.id).unwrap();
        assert!(runs(&db, None, 0).unwrap().is_empty() && get(&db, u.id).unwrap().is_none());
    }
}
