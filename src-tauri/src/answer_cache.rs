//! Instant answers: when a new chat asks (almost exactly) a question answered
//! on this Mac in the last week, BYTE shows that answer at once, marked as
//! reused, with "Ask again" for a fresh one. Questions are compared by meaning
//! (embeddings, `embed.rs`), so "whats a roth ira" matches "What is a Roth IRA?".
//!
//! Only standalone first questions are kept (a follow-up depends on its chat),
//! never private chats, questions with files, or anything time-sensitive.

use rusqlite::params;

use crate::chat::ChatMessage;
use crate::db::Db;
use crate::error::AppResult;

/// How alike two questions must be (cosine of their embeddings).
pub const THRESHOLD: f32 = 0.97;
/// Answers older than this are never reused.
pub const MAX_AGE_MS: i64 = 7 * 24 * 3600 * 1000;
/// Answers kept at most (oldest dropped first).
const MAX_ROWS: i64 = 500;

pub struct Cached {
    pub question: String,
    pub answer: String,
    pub sources: String,
    pub created_at: i64,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The question, when this request is one the cache may answer or learn from:
/// a chat's first message, no files, nothing that needs current information.
pub fn cacheable(messages: &[ChatMessage]) -> Option<&str> {
    let [only] = messages else { return None };
    let q = only.content.trim();
    let ok = only.role == "user" && only.files.is_empty() && (8..=600).contains(&q.chars().count()) && !crate::router::needs_fresh_info(q);
    ok.then_some(q)
}

pub fn count(db: &Db) -> i64 {
    db.conn().query_row("SELECT COUNT(*) FROM answer_cache", [], |r| r.get(0)).unwrap_or(0)
}

pub fn put(db: &Db, question: &str, mode: &str, vector: &[f32], answer: &str, sources: &str) -> AppResult<()> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO answer_cache (question, mode, embedding, answer, sources, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![question, mode, crate::embed::to_bytes(vector), answer, sources, now()],
    )?;
    conn.execute(
        "DELETE FROM answer_cache WHERE created_at < ?1 OR id NOT IN (SELECT id FROM answer_cache ORDER BY created_at DESC LIMIT ?2)",
        params![now() - MAX_AGE_MS, MAX_ROWS],
    )?;
    Ok(())
}

/// The newest recent answer to a question like `vector` in the same mode.
pub fn find(db: &Db, vector: &[f32], mode: &str) -> AppResult<Option<Cached>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT question, embedding, answer, sources, created_at FROM answer_cache WHERE mode = ?1 AND created_at >= ?2 ORDER BY created_at DESC")?;
    let mut rows = st.query(params![mode, now() - MAX_AGE_MS])?;
    let mut best: Option<(f32, Cached)> = None;
    while let Some(r) = rows.next()? {
        let blob: Vec<u8> = r.get(1)?;
        let score = crate::embed::cosine(vector, &crate::embed::from_bytes(&blob));
        if score >= THRESHOLD && best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, Cached { question: r.get(0)?, answer: r.get(2)?, sources: r.get(3)?, created_at: r.get(4)? }));
        }
    }
    Ok(best.map(|(_, c)| c))
}

pub fn clear(db: &Db) -> AppResult<()> {
    db.conn().execute("DELETE FROM answer_cache", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_standalone_timeless_first_questions_are_cached() {
        assert_eq!(cacheable(&[ChatMessage::new("user", "What is a Roth IRA?")]), Some("What is a Roth IRA?"));
        assert!(cacheable(&[ChatMessage::new("user", "hi")]).is_none());
        assert!(cacheable(&[ChatMessage::new("user", "What's the weather today in Pittsburgh?")]).is_none());
        let two = [ChatMessage::new("user", "What is a Roth IRA?"), ChatMessage::new("assistant", "…"), ChatMessage::new("user", "And a 401k?")];
        assert!(cacheable(&two).is_none());
    }

    #[test]
    fn similar_questions_in_the_same_mode_reuse_the_answer() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        put(&db, "What is a Roth IRA?", "auto", &[1.0, 0.0], "A retirement account…", "[]").unwrap();
        let close = crate::embed::normalize(vec![1.0, 0.1]);
        let far = crate::embed::normalize(vec![1.0, 1.0]);
        assert_eq!(find(&db, &close, "auto").unwrap().unwrap().answer, "A retirement account…");
        assert!(find(&db, &close, "deep").unwrap().is_none(), "other mode");
        assert!(find(&db, &far, "auto").unwrap().is_none(), "different question");
        clear(&db).unwrap();
        assert_eq!(count(&db), 0);
    }
}
