//! Job search tracker: the jobs you're looking at or have applied to (DB table
//! `jobs`), each with a status, deadline and notes. A posting's link can fill the
//! job in: BYTE reads the page and pulls out the company, role, place, pay,
//! deadline and the main requirements.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const STATUSES: &[&str] = &["saved", "applied", "interview", "offer", "rejected"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Job {
    pub id: i64,
    pub company: String,
    pub role: String,
    pub location: String,
    pub pay: String,
    pub url: String,
    /// saved, applied, interview, offer, rejected
    pub status: String,
    /// YYYY-MM-DD, or empty.
    pub deadline: String,
    /// When they applied (YYYY-MM-DD), or empty.
    pub applied: String,
    pub summary: String,
    pub requirements: Vec<String>,
    pub notes: String,
    pub updated: i64,
}

fn clean(v: &Value, max: usize) -> String {
    v.as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// A date the model wrote, as YYYY-MM-DD (or empty if it isn't one).
fn date(v: &Value) -> String {
    let s = v.as_str().unwrap_or("").trim();
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.to_string()).unwrap_or_default()
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "company": { "type": "string" },
            "role": { "type": "string" },
            "location": { "type": "string" },
            "pay": { "type": "string" },
            "deadline": { "type": "string" },
            "summary": { "type": "string" },
            "requirements": { "type": "array", "maxItems": 8, "items": { "type": "string" } }
        },
        "required": ["company", "role", "location", "pay", "deadline", "summary", "requirements"]
    })
}

/// A job from the model's reading of a posting (None if it found no role).
pub fn parse_posting(reply: &str, url: &str) -> Option<Job> {
    let v = crate::research::lenient_json(reply);
    let role = clean(&v["role"], 120);
    if role.is_empty() {
        return None;
    }
    Some(Job {
        company: clean(&v["company"], 120),
        role,
        location: clean(&v["location"], 120),
        pay: clean(&v["pay"], 80),
        url: url.to_string(),
        status: "saved".into(),
        deadline: date(&v["deadline"]),
        summary: clean(&v["summary"], 600),
        requirements: v["requirements"].as_array().into_iter().flatten().map(|r| clean(r, 200)).filter(|r| !r.is_empty()).take(8).collect(),
        ..Default::default()
    })
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    let reqs: String = r.get(10)?;
    Ok(Job {
        id: r.get(0)?,
        company: r.get(1)?,
        role: r.get(2)?,
        location: r.get(3)?,
        pay: r.get(4)?,
        url: r.get(5)?,
        status: r.get(6)?,
        deadline: r.get(7)?,
        applied: r.get(8)?,
        summary: r.get(9)?,
        requirements: serde_json::from_str(&reqs).unwrap_or_default(),
        notes: r.get(11)?,
        updated: r.get(12)?,
    })
}

const COLUMNS: &str = "id, company, role, location, pay, url, status, deadline, applied, summary, requirements, notes, updated";

pub fn list(db: &crate::db::Db) -> AppResult<Vec<Job>> {
    let conn = db.conn();
    // Soonest deadline first, then the most recently changed.
    let mut st = conn.prepare(&format!("SELECT {COLUMNS} FROM jobs ORDER BY deadline = '', deadline, updated DESC"))?;
    let rows = st.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Adds (id 0) or updates a job; returns its id. Applying stamps today's date.
pub fn save(db: &crate::db::Db, job: &Job) -> AppResult<i64> {
    if job.role.trim().is_empty() && job.company.trim().is_empty() {
        return Err(AppError::msg("A job needs at least a company or a role."));
    }
    let status = if STATUSES.contains(&job.status.as_str()) { job.status.clone() } else { "saved".into() };
    let applied = if job.applied.is_empty() && status != "saved" { chrono::Local::now().date_naive().to_string() } else { job.applied.clone() };
    let now = chrono::Utc::now().timestamp_millis();
    let reqs = serde_json::to_string(&job.requirements)?;
    let conn = db.conn();
    let p = rusqlite::params![job.company.trim(), job.role.trim(), job.location, job.pay, job.url, status, job.deadline, applied, job.summary, reqs, job.notes, now];
    if job.id > 0 {
        conn.execute(
            "UPDATE jobs SET company=?1, role=?2, location=?3, pay=?4, url=?5, status=?6, deadline=?7, applied=?8, summary=?9, requirements=?10, notes=?11, updated=?12 WHERE id=?13",
            rusqlite::params![p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9], p[10], p[11], job.id],
        )?;
        Ok(job.id)
    } else {
        conn.execute("INSERT INTO jobs (company, role, location, pay, url, status, deadline, applied, summary, requirements, notes, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)", p)?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn delete(db: &crate::db::Db, id: i64) -> AppResult<()> {
    db.conn().execute("DELETE FROM jobs WHERE id = ?1", [id])?;
    Ok(())
}

/// What "Prepare for the interview" asks in the chat.
pub fn prep_prompt(job: &Job) -> String {
    let mut s = format!("Help me prepare for my interview for the {} role at {}.", job.role, if job.company.is_empty() { "this company" } else { &job.company });
    if !job.summary.is_empty() {
        s.push_str(&format!("\n\nThe job: {}", job.summary));
    }
    if !job.requirements.is_empty() {
        s.push_str(&format!("\nWhat they ask for: {}.", job.requirements.join("; ")));
    }
    s.push_str("\n\nWhat does the company do, what questions will they likely ask (with how I could answer), and what should I ask them?");
    s
}

#[tauri::command]
pub fn jobs_list(state: State<'_, AppState>) -> AppResult<Vec<Job>> {
    list(&state.db)
}

#[tauri::command]
pub fn job_save(state: State<'_, AppState>, job: Job) -> AppResult<i64> {
    save(&state.db, &job)
}

#[tauri::command]
pub fn job_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete(&state.db, id)
}

#[tauri::command]
pub fn job_prep_prompt(job: Job) -> String {
    prep_prompt(&job)
}

/// Reads a job posting and fills in a job (not saved yet: the panel shows it first).
#[tauri::command]
pub async fn job_from_url(state: State<'_, AppState>, url: String) -> AppResult<Job> {
    let page = crate::tools::fetch::fetch_page(&state.net, url.trim()).await?;
    let ep = match state.engine.endpoint().await {
        Some(ep) => ep,
        None => crate::backend::cloud_cards(&state).await.ok_or_else(|| AppError::msg("Load a model first (Settings → Models) to read the posting, or fill it in yourself."))?,
    };
    let mut job = read_posting(&state.local_http, &ep, &page.url, &page.text).await?;
    if job.company.is_empty() {
        job.company = url::Url::parse(&page.url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_default();
    }
    Ok(job)
}

/// The model's reading of a posting's text.
pub async fn read_posting(http: &reqwest::Client, ep: &crate::engine::Endpoint, url: &str, text: &str) -> AppResult<Job> {
    let today = chrono::Local::now().date_naive();
    let user = format!(
        "Today is {today}. This is a job posting from {}:\n\n{}\n\nFill in the job: company, role (the job title), location (city or Remote), pay (as written, or empty), \
deadline to apply as YYYY-MM-DD (empty if none is given), a two-sentence summary of the job, and up to 8 main requirements (short).",
        url,
        text.chars().take(12_000).collect::<String>()
    );
    let reply = crate::chat::complete_json(http, ep, "You read job postings carefully. Reply only with JSON.", &user, schema(), 900).await?;
    let mut job = parse_posting(&reply, url).ok_or_else(|| AppError::msg("That page doesn't look like a job posting. Fill the job in yourself."))?;
    // Small models miss deadlines now and then; the date is usually written plainly.
    if job.deadline.is_empty() {
        job.deadline = deadline_in(text).unwrap_or_default();
    }
    Ok(job)
}

/// A deadline written in a posting ("Apply by 2026-11-15", "Applications close
/// November 15, 2026", "Deadline: 15 Nov 2026"), as YYYY-MM-DD.
pub fn deadline_in(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    for cue in ["apply by", "deadline", "applications close", "closing date", "apply before", "closes on", "closes"] {
        let mut from = 0;
        while let Some(i) = lower[from..].find(cue).map(|i| i + from) {
            let after = &text[i + cue.len()..];
            let window: String = after.chars().take(40).collect();
            let window = window.trim_start_matches([':', ' ', '-', 'o', 'n']).trim();
            for fmt in ["%Y-%m-%d", "%B %d, %Y", "%b %d, %Y", "%d %B %Y", "%d %b %Y", "%m/%d/%Y"] {
                // Try each leading run of words (dates are 1 to 3 words).
                let words: Vec<&str> = window.split_whitespace().collect();
                for n in 1..=3.min(words.len()) {
                    let cand = words[..n].join(" ").trim_end_matches(['.', ',', ';', ')']).to_string();
                    if let Ok(d) = chrono::NaiveDate::parse_from_str(&cand, fmt) {
                        return Some(d.to_string());
                    }
                }
            }
            from = i + cue.len();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postings_are_read_forgivingly() {
        let j = parse_posting(
            r#"{"company":" Acme  Corp ","role":"Junior Data Analyst","location":"Pittsburgh, PA","pay":"$55,000–$65,000","deadline":"2026-10-15","summary":"Analyze sales data.","requirements":["SQL","Excel",""]}"#,
            "https://jobs.example.com/1",
        )
        .unwrap();
        assert_eq!((j.company.as_str(), j.role.as_str(), j.deadline.as_str()), ("Acme Corp", "Junior Data Analyst", "2026-10-15"));
        assert_eq!(j.requirements, vec!["SQL", "Excel"]);
        assert_eq!(j.status, "saved");
        assert_eq!(parse_posting(r#"{"company":"x","role":"a","deadline":"October 15"}"#, "u").unwrap().deadline, "", "not a date");
        assert!(parse_posting(r#"{"company":"Acme"}"#, "u").is_none(), "no role: not a posting");
    }

    #[test]
    fn jobs_are_saved_updated_and_sorted_by_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(dir.path()).unwrap();
        let a = save(&db, &Job { company: "Acme".into(), role: "Analyst".into(), deadline: "2026-11-01".into(), ..Default::default() }).unwrap();
        let b = save(&db, &Job { company: "Beta".into(), role: "Designer".into(), deadline: "2026-10-01".into(), requirements: vec!["Figma".into()], ..Default::default() }).unwrap();
        save(&db, &Job { company: "Gamma".into(), role: "Writer".into(), ..Default::default() }).unwrap();
        let l = list(&db).unwrap();
        assert_eq!(l.iter().map(|j| j.company.as_str()).collect::<Vec<_>>(), ["Beta", "Acme", "Gamma"], "soonest deadline first, none last");
        assert_eq!(l[0].requirements, vec!["Figma"]);
        // Moving to "applied" stamps the date.
        let mut j = l.into_iter().find(|j| j.id == a).unwrap();
        j.status = "applied".into();
        save(&db, &j).unwrap();
        let j = list(&db).unwrap().into_iter().find(|j| j.id == a).unwrap();
        assert_eq!(j.applied, chrono::Local::now().date_naive().to_string());
        delete(&db, b).unwrap();
        assert_eq!(list(&db).unwrap().len(), 2);
        assert!(save(&db, &Job::default()).is_err());
    }

    /// Real engine: a posting's text becomes a job.
    #[tokio::test]
    #[ignore]
    async fn e2e_read_posting() {
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let text = "Brightline Analytics is hiring a Junior Data Analyst in Pittsburgh, PA (hybrid). Salary: $58,000 - $66,000 per year. \
You will build weekly sales dashboards and clean data for the marketing team. Requirements: SQL; Excel (pivot tables); a bachelor's degree \
in statistics, economics or a related field; clear written communication. Apply by 2026-11-15.";
        let job = read_posting(&crate::chat::local_client(), &ep, "https://jobs.example.com/42", text).await.unwrap();
        eprintln!("{job:?}");
        assert!(job.role.to_lowercase().contains("analyst"), "{job:?}");
        assert!(job.company.to_lowercase().contains("brightline"), "{job:?}");
        assert_eq!(job.deadline, "2026-11-15");
        assert!(!job.requirements.is_empty());
    }

    #[test]
    fn deadlines_are_found_in_the_posting_text() {
        assert_eq!(deadline_in("Great team. Apply by 2026-11-15.").as_deref(), Some("2026-11-15"));
        assert_eq!(deadline_in("Applications close November 5, 2026 at noon").as_deref(), Some("2026-11-05"));
        assert_eq!(deadline_in("Deadline: 15 Nov 2026").as_deref(), Some("2026-11-15"));
        assert_eq!(deadline_in("Closing date: 12/01/2026").as_deref(), Some("2026-12-01"));
        assert_eq!(deadline_in("We move fast. Apply today!"), None);
    }

    #[test]
    fn interview_prep_asks_the_right_things() {
        let p = prep_prompt(&Job { company: "Acme".into(), role: "Analyst".into(), requirements: vec!["SQL".into()], ..Default::default() });
        assert!(p.starts_with("Help me prepare for my interview for the Analyst role at Acme."));
        assert!(p.contains("SQL") && p.contains("what should I ask them"));
    }
}
