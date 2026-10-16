//! Searchable memory of what was on screen: the last helper, and the most careful one.
//!
//! While it is on, every few minutes BYTE reads the text of the window in front (the screen track's capture and text reader: a
//! picture is made in memory, turned into words and dropped) and keeps only those words, with the time, the program and the window
//! title, in BYTE's encrypted database on this computer. "What was I reading about X?" searches those words. Never pictures; never
//! sent anywhere (it does not even use a model: results are found by text search and listed as they are); off by default.
//!
//! What it leaves out: windows on the never-look-at list (and a second list of its own), password managers, banks and private
//! browser windows (the screen track's rules), BYTE itself, private chats, kids mode, a locked BYTE, a full-screen game, and times
//! you are away. Card-like numbers and key-like strings are blanked in the text before it is kept. **Clear everything** deletes it
//! all at once, **Forget the last hour** deletes the recent part, and entries older than the chosen number of days are deleted
//! (0 days means keep them until you clear them).

use serde::Serialize;

use crate::db::Db;
use crate::error::AppResult;

/// Longest text kept per look.
pub const MAX_CHARS: usize = 4_000;
/// A look with fewer words than this is not worth keeping.
pub const MIN_WORDS: usize = 8;
pub const DEFAULT_EVERY: u32 = 3;
pub const DEFAULT_DAYS: u32 = 14;

pub fn default_every() -> u32 {
    DEFAULT_EVERY
}
pub fn default_days() -> u32 {
    DEFAULT_DAYS
}

pub fn supported() -> bool {
    cfg!(windows)
}

// ---------------------------------------------------------------- the text

/// Blanks what must not be kept: runs of digits that pass the card check, and any word that looks like a key or password.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let words: Vec<String> = line
            .split_whitespace()
            .map(|w| {
                let bare = w.trim_matches(|c: char| !c.is_alphanumeric());
                if super::forms::looks_like_card(bare) || crate::clipboard::looks_secret(bare) {
                    "[hidden]".to_string()
                } else {
                    w.to_string()
                }
            })
            .collect();
        // Card numbers written in groups ("4111 1111 1111 1111") are spread over several words.
        let joined = words.join(" ");
        out.push_str(&hide_grouped_cards(&joined));
        out.push('\n');
    }
    out.trim().chars().take(MAX_CHARS).collect()
}

fn hide_grouped_cards(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || ((chars[j] == ' ' || chars[j] == '-') && j + 1 < chars.len() && chars[j + 1].is_ascii_digit())) {
                j += 1;
            }
            let run: String = chars[i..j].iter().collect();
            if super::forms::looks_like_card(&run) {
                out.push_str("[hidden]");
            } else {
                out.push_str(&run);
            }
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Words of a text, lower case, for comparing two looks.
fn word_set(t: &str) -> std::collections::HashSet<String> {
    t.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).map(|w| w.to_lowercase()).collect()
}

/// Nearly the same screen as before (the same words, 90 % or more): not kept again.
pub fn similar(a: &str, b: &str) -> bool {
    let (x, y) = (word_set(a), word_set(b));
    if x.is_empty() || y.is_empty() {
        return x.is_empty() && y.is_empty();
    }
    let both = x.intersection(&y).count() as f32;
    let either = x.union(&y).count() as f32;
    both / either >= 0.9
}

pub fn word_count(t: &str) -> usize {
    t.split_whitespace().count()
}

// ---------------------------------------------------------------- the store

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS screenmem (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    program TEXT NOT NULL,
    title TEXT NOT NULL,
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS screenmem_at ON screenmem(at);
CREATE VIRTUAL TABLE IF NOT EXISTS screenmem_fts USING fts5(text, title, tokenize = 'porter unicode61');
";

fn ensure(db: &Db) -> AppResult<()> {
    db.conn().execute_batch(SCHEMA)?;
    Ok(())
}

/// Keeps one look. Returns false when it was skipped (too little text, or the same as the last look).
pub fn add(db: &Db, at: i64, program: &str, title: &str, text: &str) -> AppResult<bool> {
    ensure(db)?;
    let text = redact(text);
    if word_count(&text) < MIN_WORDS {
        return Ok(false);
    }
    let conn = db.conn();
    let last: Option<String> = conn.query_row("SELECT text FROM screenmem ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).ok();
    if last.as_deref().is_some_and(|l| similar(l, &text)) {
        return Ok(false);
    }
    let title: String = title.chars().take(200).collect();
    conn.execute("INSERT INTO screenmem (at, program, title, text) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![at, program, title, text])?;
    let id = conn.last_insert_rowid();
    conn.execute("INSERT INTO screenmem_fts (rowid, text, title) VALUES (?1, ?2, ?3)", rusqlite::params![id, text, title])?;
    Ok(true)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub id: i64,
    pub at: i64,
    pub program: String,
    pub title: String,
    pub snippet: String,
}

/// The looks that match every word of `query` (best first), with a short piece of the text around the match.
pub fn search(db: &Db, query: &str, limit: usize) -> AppResult<Vec<Hit>> {
    ensure(db)?;
    let words: Vec<String> = query.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 2).take(10).map(|w| format!("\"{}\"", w.to_lowercase())).collect();
    if words.is_empty() {
        return Ok(vec![]);
    }
    let q = words.join(" ");
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT m.id, m.at, m.program, m.title, snippet(screenmem_fts, 0, '', '', ' ... ', 14)
         FROM screenmem_fts JOIN screenmem m ON m.id = screenmem_fts.rowid
         WHERE screenmem_fts MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = st.query_map(rusqlite::params![q, limit as i64], |r| Ok(Hit { id: r.get(0)?, at: r.get(1)?, program: r.get(2)?, title: r.get(3)?, snippet: r.get(4)? }))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn count(db: &Db) -> AppResult<i64> {
    ensure(db)?;
    Ok(db.conn().query_row("SELECT count(*) FROM screenmem", [], |r| r.get(0))?)
}

/// Deletes the looks taken at or after `since` (ms); returns how many.
pub fn forget_since(db: &Db, since: i64) -> AppResult<usize> {
    ensure(db)?;
    let conn = db.conn();
    conn.execute("DELETE FROM screenmem_fts WHERE rowid IN (SELECT id FROM screenmem WHERE at >= ?1)", [since])?;
    Ok(conn.execute("DELETE FROM screenmem WHERE at >= ?1", [since])?)
}

/// Deletes the looks older than `before` (ms).
pub fn purge_before(db: &Db, before: i64) -> AppResult<usize> {
    ensure(db)?;
    let conn = db.conn();
    conn.execute("DELETE FROM screenmem_fts WHERE rowid IN (SELECT id FROM screenmem WHERE at < ?1)", [before])?;
    Ok(conn.execute("DELETE FROM screenmem WHERE at < ?1", [before])?)
}

/// Deletes everything and gives the space back.
pub fn clear(db: &Db) -> AppResult<()> {
    ensure(db)?;
    let conn = db.conn();
    conn.execute_batch("DELETE FROM screenmem; DELETE FROM screenmem_fts; INSERT INTO screenmem_fts(screenmem_fts) VALUES('optimize');")?;
    let _ = conn.execute_batch("VACUUM;");
    Ok(())
}

/// The cut-off time for `days` of keeping (None = keep until cleared).
pub fn keep_cutoff(now_ms: i64, days: u32) -> Option<i64> {
    (days != 0).then(|| now_ms - i64::from(days) * 86_400_000)
}

// ---------------------------------------------------------------- asking in words

/// "what was I looking at about the tax form", "where did I see the flight number", "find what I read about solar panels" -> the topic.
pub fn parse_ask(q: &str) -> Option<String> {
    let l = q.trim().trim_end_matches(['?', '.', '!']).to_lowercase();
    let l = l.trim_start_matches("hey byte").trim_start_matches([',', ' ']);
    for p in [
        "what was i looking at about ", "what was i looking at when ", "what was i reading about ", "what was i reading on my screen about ", "what did i see about ", "what did i see on my screen about ",
        "where did i see ", "where did i read ", "find on my screen ", "search my screen history for ", "search what i saw for ", "find what i read about ", "find what i saw about ",
    ] {
        if let Some(rest) = l.strip_prefix(p) {
            let t = rest.trim().trim_start_matches("the ").trim_start_matches("that ").trim();
            return (t.chars().count() >= 2 && t.split_whitespace().count() <= 12).then(|| t.to_string());
        }
    }
    None
}

/// The reply for a spoken or typed question: the matches, listed. BYTE's own text; no model is asked.
pub fn answer(hits: &[Hit], topic: &str, now_ms: i64) -> String {
    if hits.is_empty() {
        return format!("I found nothing on your screen history about \"{topic}\".");
    }
    let mut out = format!("Here is what was on your screen about \"{topic}\":\n");
    for h in hits.iter().take(5) {
        let ago = ago(now_ms - h.at);
        let title = if h.title.trim().is_empty() { h.program.clone() } else { h.title.trim().to_string() };
        out.push_str(&format!("\n- {ago} in {title} ({}): ...{}...", h.program, h.snippet.split_whitespace().collect::<Vec<_>>().join(" ")));
    }
    out
}

fn ago(ms: i64) -> String {
    let m = (ms / 60_000).max(0);
    match m {
        0 => "just now".into(),
        1..=59 => format!("{m} minute{} ago", if m == 1 { "" } else { "s" }),
        60..=1439 => format!("{} hour{} ago", m / 60, if m / 60 == 1 { "" } else { "s" }),
        _ => format!("{} day{} ago", m / 1440, if m / 1440 == 1 { "" } else { "s" }),
    }
}

/// The hook in `agent::specialist` (marked `track:helpers`): the answer is the list itself.
pub async fn try_route(turn: &crate::agent::Turn<'_>, q: &str, send: crate::agent::Emit<'_>) -> AppResult<Option<(crate::tools::SourceBook, String, &'static str)>> {
    use crate::chat::ChatEvent;
    let Some(app) = turn.app else { return Ok(None) };
    let Some(topic) = parse_ask(crate::router::without_wake_phrase(q)) else { return Ok(None) };
    let state = tauri::Manager::state::<crate::state::AppState>(app);
    if !state.settings.lock().await.helper_screenmem || state.lock.is_locked() || crate::kids::is_on() {
        return Ok(None);
    }
    let id = format!("byte_screenmem_{}", uuid::Uuid::new_v4().simple());
    send(ChatEvent::ToolCall { id: id.clone(), name: "search_screen_history".into(), args: serde_json::json!({ "what": topic, "app": "BYTE" }) })?;
    let hits = search(&state.db, &topic, 5)?;
    send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} match{}", hits.len(), if hits.len() == 1 { "" } else { "es" }) })?;
    Ok(Some((Default::default(), answer(&hits, &topic, chrono::Utc::now().timestamp_millis()), "house")))
}

// ---------------------------------------------------------------- the watcher

/// Is the person away (no keyboard or mouse for this many seconds)?
#[cfg(windows)]
fn idle_seconds() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dwTime)) / 1000
    } else {
        0
    }
}

#[cfg(not(windows))]
fn idle_seconds() -> u64 {
    0
}

/// Away this long and nothing is looked at.
pub const AWAY_SECONDS: u64 = 300;

#[cfg(desktop)]
mod watch {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::{AppHandle, Manager};

    static RUNNING: AtomicBool = AtomicBool::new(false);

    pub fn apply(app: &AppHandle, on: bool) {
        if !on {
            if RUNNING.swap(false, Ordering::SeqCst) {
                crate::helpers::set_screenmem_tooltip(app, false);
            }
            return;
        }
        if !supported() || RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        crate::helpers::set_screenmem_tooltip(app, true);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // Not at the very start: BYTE and the person settle first.
            tokio::time::sleep(std::time::Duration::from_secs(45)).await;
            while RUNNING.load(Ordering::SeqCst) {
                let every = look(&app).await;
                tokio::time::sleep(std::time::Duration::from_secs(u64::from(every.max(1)) * 60)).await;
            }
        });
    }

    /// One look (if everything allows it). Returns the minutes to the next.
    async fn look(app: &AppHandle) -> u32 {
        let state = app.state::<crate::state::AppState>();
        let (on, every, days, mut excludes, own) = {
            let s = state.settings.lock().await;
            (s.helper_screenmem && !s.kids_mode, s.helper_screenmem_every, s.helper_screenmem_days, s.screen_exclude_apps.clone(), s.helper_screenmem_exclude.clone())
        };
        if !on {
            RUNNING.store(false, Ordering::SeqCst);
            return every;
        }
        excludes.extend(own);
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(cut) = keep_cutoff(now, days) {
            let _ = purge_before(&state.db, cut);
        }
        if state.lock.is_locked() || crate::helpers::blocked_now().is_some() || idle_seconds() >= AWAY_SECONDS || crate::screen::read::game_in_front(crate::screen::read::shell_state()) {
            return every;
        }
        let shot = tokio::task::spawn_blocking(move || crate::screen::flow::grab_and_read(excludes)).await;
        if let Ok(Ok(shot)) = shot {
            // The picture is already gone (only its words are here).
            let _ = add(&state.db, now, &shot.window.program, &shot.window.title, &shot.text);
        }
        every
    }
}

#[cfg(desktop)]
pub fn apply(app: &tauri::AppHandle, on: bool) {
    watch::apply(app, on);
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn screenmem_search(state: tauri::State<'_, crate::state::AppState>, query: String) -> AppResult<Vec<Hit>> {
    crate::lock::ensure(&state)?;
    search(&state.db, &query, 20)
}

#[tauri::command]
pub fn screenmem_count(state: tauri::State<'_, crate::state::AppState>) -> AppResult<i64> {
    crate::lock::ensure(&state)?;
    count(&state.db)
}

/// Deletes everything that was kept.
#[tauri::command]
pub fn screenmem_clear(state: tauri::State<'_, crate::state::AppState>) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    clear(&state.db)
}

/// Deletes the last hour.
#[tauri::command]
pub fn screenmem_forget_hour(state: tauri::State<'_, crate::state::AppState>) -> AppResult<usize> {
    crate::lock::ensure(&state)?;
    forget_since(&state.db, chrono::Utc::now().timestamp_millis() - 3_600_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (tempfile::TempDir, Db) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open_with_key(&d.path().join("t.db"), "k").unwrap();
        (d, db)
    }

    const TEXT: &str = "Solar panel installation guide for a flat roof with south facing tilt and inverter sizing notes";

    #[test]
    fn card_numbers_and_key_like_words_are_blanked_before_keeping() {
        let r = redact("Pay with 4111 1111 1111 1111 now\nkey sk-abcdefghijklmnop here\nplain 2026 text");
        assert!(r.contains("[hidden]") && !r.contains("4111") && !r.contains("sk-abc"));
        assert!(r.contains("plain 2026 text"));
        assert!(!redact("order 4111111111111111 ok").contains("4111"));
        assert_eq!(redact("order number 12345 ok"), "order number 12345 ok");
        assert!(redact(&"word ".repeat(2000)).chars().count() <= MAX_CHARS);
    }

    #[test]
    fn nearly_identical_screens_are_not_kept_twice() {
        assert!(similar(TEXT, TEXT));
        assert!(similar(TEXT, &format!("{TEXT} 10:31")));
        assert!(!similar(TEXT, "Quarterly tax form instructions for filing estimated payments online this year"));
        assert!(similar("", "  "));
    }

    #[test]
    fn a_look_is_kept_found_and_the_snippet_shows_the_match() {
        let (_d, db) = db();
        assert!(add(&db, 1_000, "chrome", "Solar guide", TEXT).unwrap());
        assert!(!add(&db, 2_000, "chrome", "Solar guide", TEXT).unwrap(), "the same screen is not kept again");
        assert!(!add(&db, 3_000, "chrome", "tiny", "too few words").unwrap());
        assert!(add(&db, 4_000, "code", "Tax", "Quarterly tax form instructions for filing estimated payments online before the deadline").unwrap());
        assert_eq!(count(&db).unwrap(), 2);
        let hits = search(&db, "solar inverter", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].program.as_str(), hits[0].title.as_str()), ("chrome", "Solar guide"));
        assert!(hits[0].snippet.to_lowercase().contains("solar") || hits[0].snippet.to_lowercase().contains("inverter"));
        assert!(search(&db, "tax deadline", 10).unwrap().len() == 1);
        assert!(search(&db, "zebra", 10).unwrap().is_empty());
        assert!(search(&db, "", 10).unwrap().is_empty());
        // Odd characters in a query can't break the search.
        assert!(search(&db, "\"solar\" OR * ) (", 10).is_ok());
    }

    #[test]
    fn forgetting_and_clearing_remove_text_and_index() {
        let (_d, db) = db();
        add(&db, 1_000, "a", "A", TEXT).unwrap();
        add(&db, 9_000_000, "b", "B", "Quarterly tax form instructions for filing estimated payments online before the deadline").unwrap();
        assert_eq!(forget_since(&db, 5_000_000).unwrap(), 1);
        assert_eq!(count(&db).unwrap(), 1);
        assert!(search(&db, "tax", 5).unwrap().is_empty(), "the search index forgets it too");
        assert_eq!(purge_before(&db, 2_000).unwrap(), 1);
        assert_eq!(count(&db).unwrap(), 0);
        add(&db, 1, "a", "A", TEXT).unwrap();
        clear(&db).unwrap();
        assert_eq!(count(&db).unwrap(), 0);
        assert!(search(&db, "solar", 5).unwrap().is_empty());
    }

    #[test]
    fn keeping_has_an_unlimited_choice() {
        assert_eq!(keep_cutoff(1_000_000_000, 0), None);
        assert_eq!(keep_cutoff(1_000_000_000, 1), Some(1_000_000_000 - 86_400_000));
        assert_eq!(default_days(), 14);
    }

    #[test]
    fn questions_in_words_find_the_topic() {
        assert_eq!(parse_ask("What was I looking at about the solar panels?").as_deref(), Some("solar panels"));
        assert_eq!(parse_ask("where did I see the flight number").as_deref(), Some("flight number"));
        assert_eq!(parse_ask("Hey BYTE, find what I read about taxes").as_deref(), Some("taxes"));
        assert_eq!(parse_ask("what is the weather"), None);
        assert_eq!(parse_ask("where did I see "), None);
    }

    #[test]
    fn the_answer_lists_matches_and_says_when_there_are_none() {
        let hits = vec![Hit { id: 1, at: 0, program: "chrome".into(), title: "Solar guide".into(), snippet: "south  facing\ntilt".into() }];
        let a = answer(&hits, "solar", 3 * 3_600_000);
        assert!(a.contains("3 hours ago in Solar guide (chrome): ...south facing tilt..."));
        assert!(answer(&[], "zebra", 0).contains("found nothing"));
        assert_eq!(ago(30_000), "just now");
        assert_eq!(ago(61_000), "1 minute ago");
        assert_eq!(ago(2 * 86_400_000), "2 days ago");
    }
}
