//! BYTE's local database: chats, full-text search and long-term memories.
//!
//! One SQLite file (`byte.db`) encrypted with SQLCipher. The key is 256 random
//! bits stored next to it in `db.key` (readable only by this user). That keeps
//! chats unreadable to Spotlight, backups and anyone copying the database
//! file; moving the key into the Keychain behind Touch ID is Phase 12 (it needs
//! the stable self-signed certificate so macOS doesn't re-ask after updates).
//!
//! Messages are stored as the UI's JSON (reasoning, sources, steps, stats…) in
//! `data`, plus the searchable text in an FTS5 index. Private chats never reach
//! this module.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};

const SCHEMA_VERSION: i32 = 1;

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::msg(format!("database error: {e}"))
    }
}

/// Sidebar entry: everything about a chat except its messages.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub pinned: bool,
    pub folder: Option<String>,
    pub message_count: i64,
}

/// Changes to a chat's sidebar properties (only the fields present change).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaPatch {
    pub title: Option<String>,
    pub pinned: Option<bool>,
    /// `Some("")` removes the chat from its folder.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub conversation_id: String,
    pub message_id: String,
    pub title: String,
    /// Matching text with the hits wrapped in «».
    pub snippet: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    pub text: String,
    /// "user" (typed in Settings) or "chat" (suggested by BYTE and confirmed).
    pub source: String,
    pub created_at: i64,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Opens (creating if needed) the encrypted database in `dir`. A database
    /// that can't be decrypted (its key file was lost) is moved aside rather
    /// than deleted, and a fresh one is started.
    pub fn open(dir: &Path) -> AppResult<Db> {
        let db_path = dir.join("byte.db");
        let key = load_or_create_key(&dir.join("db.key"), db_path.exists())?;
        match Self::open_with_key(&db_path, &key) {
            Ok(db) => Ok(db),
            Err(e) if db_path.exists() => {
                let aside = dir.join(format!("byte.db.unreadable-{}", chrono::Utc::now().timestamp()));
                log::error!("database can't be opened ({e}); moving it to {} and starting fresh", aside.display());
                std::fs::rename(&db_path, &aside)?;
                Self::open_with_key(&db_path, &key)
            }
            Err(e) => Err(e),
        }
    }

    pub fn open_with_key(path: &Path, key: &str) -> AppResult<Db> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "key", key)?;
        // Fails here (not later) if the key is wrong.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&conn)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---------- chats ----------

    /// All chats, pinned first, then most recently used.
    pub fn list(&self) -> AppResult<Vec<ConversationMeta>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT c.id, c.title, c.created_at, c.updated_at, c.pinned, c.folder,
                    (SELECT count(*) FROM messages m WHERE m.conversation_id = c.id)
             FROM conversations c ORDER BY c.pinned DESC, c.updated_at DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(ConversationMeta {
                id: r.get(0)?,
                title: r.get(1)?,
                created_at: r.get(2)?,
                updated_at: r.get(3)?,
                pinned: r.get::<_, i64>(4)? != 0,
                folder: r.get(5)?,
                message_count: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// A chat with its messages, in the UI's shape.
    pub fn load(&self, id: &str) -> AppResult<Option<Value>> {
        let conn = self.conn();
        let meta = conn
            .query_row(
                "SELECT title, created_at, updated_at, pinned, folder FROM conversations WHERE id = ?1",
                [id],
                |r| {
                    Ok(serde_json::json!({
                        "id": id,
                        "title": r.get::<_, String>(0)?,
                        "createdAt": r.get::<_, i64>(1)?,
                        "updatedAt": r.get::<_, i64>(2)?,
                        "pinned": r.get::<_, i64>(3)? != 0,
                        "folder": r.get::<_, Option<String>>(4)?,
                    }))
                },
            )
            .optional()?;
        let Some(mut meta) = meta else { return Ok(None) };
        let mut st = conn.prepare("SELECT data FROM messages WHERE conversation_id = ?1 ORDER BY seq")?;
        let messages: Vec<Value> = st
            .query_map([id], |r| r.get::<_, String>(0))?
            .filter_map(|d| d.ok().and_then(|d| serde_json::from_str(&d).ok()))
            .collect();
        meta["messages"] = Value::Array(messages);
        Ok(Some(meta))
    }

    /// Saves a whole chat (the UI's JSON): its properties, every message, and
    /// the search index. Messages no longer in the chat are removed.
    pub fn save(&self, conv: &Value) -> AppResult<()> {
        let id = str_field(conv, "id")?;
        let title = conv.get("title").and_then(Value::as_str).unwrap_or("New chat");
        let now = chrono::Utc::now().timestamp_millis();
        let created = conv.get("createdAt").and_then(Value::as_i64).unwrap_or(now);
        let updated = conv.get("updatedAt").and_then(Value::as_i64).unwrap_or(now);
        let messages = conv.get("messages").and_then(Value::as_array).cloned().unwrap_or_default();

        let mut conn = self.conn();
        let tx = conn.transaction()?;
        // Pinned/folder are changed through `update_meta`, so keep them on upsert.
        tx.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, updated_at = excluded.updated_at",
            params![id, title, created, updated],
        )?;
        tx.execute("DELETE FROM messages WHERE conversation_id = ?1", [id])?;
        tx.execute("DELETE FROM messages_fts WHERE conversation_id = ?1", [id])?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO messages (id, conversation_id, seq, role, content, data, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            let mut fts = tx.prepare("INSERT INTO messages_fts (content, title, conversation_id, message_id) VALUES (?1, ?2, ?3, ?4)")?;
            for (seq, m) in messages.iter().enumerate() {
                let mid = str_field(m, "id")?;
                let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
                let content = m.get("content").and_then(Value::as_str).unwrap_or("");
                let at = m.get("createdAt").and_then(Value::as_i64).unwrap_or(now);
                ins.execute(params![mid, id, seq as i64, role, content, m.to_string(), at])?;
                fts.execute(params![content, title, id, mid])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM messages_fts WHERE conversation_id = ?1", [id])?;
        tx.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_meta(&self, id: &str, patch: &MetaPatch) -> AppResult<()> {
        let conn = self.conn();
        if let Some(t) = patch.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            conn.execute("UPDATE conversations SET title = ?2 WHERE id = ?1", params![id, t])?;
            conn.execute("UPDATE messages_fts SET title = ?2 WHERE conversation_id = ?1", params![id, t])?;
        }
        if let Some(p) = patch.pinned {
            conn.execute("UPDATE conversations SET pinned = ?2 WHERE id = ?1", params![id, p as i64])?;
        }
        if let Some(f) = &patch.folder {
            let f = f.trim();
            let f = (!f.is_empty()).then_some(f);
            conn.execute("UPDATE conversations SET folder = ?2 WHERE id = ?1", params![id, f])?;
        }
        Ok(())
    }

    /// Full-text search over every message (and chat titles), best matches first.
    pub fn search(&self, query: &str, limit: usize) -> AppResult<Vec<SearchHit>> {
        let Some(q) = fts_query(query) else { return Ok(Vec::new()) };
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT f.conversation_id, f.message_id, c.title,
                    snippet(messages_fts, 0, '«', '»', '…', 14), c.updated_at
             FROM messages_fts f JOIN conversations c ON c.id = f.conversation_id
             WHERE messages_fts MATCH ?1 ORDER BY bm25(messages_fts, 1.0, 3.0) LIMIT ?2",
        )?;
        let rows = st.query_map(params![q, (limit * 4) as i64], |r| {
            Ok(SearchHit {
                conversation_id: r.get(0)?,
                message_id: r.get(1)?,
                title: r.get(2)?,
                snippet: r.get(3)?,
                updated_at: r.get(4)?,
            })
        })?;
        // One hit per chat: its best-matching message.
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for hit in rows.flatten() {
            if seen.insert(hit.conversation_id.clone()) {
                out.push(hit);
                if out.len() == limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Every chat with its messages (for export).
    pub fn export_all(&self) -> AppResult<Vec<Value>> {
        let ids: Vec<String> = self.list()?.into_iter().map(|c| c.id).collect();
        let mut out = Vec::new();
        for id in ids {
            if let Some(c) = self.load(&id)? {
                out.push(c);
            }
        }
        Ok(out)
    }

    // ---------- memories ----------

    pub fn memories(&self) -> AppResult<Vec<Memory>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT id, text, source, created_at FROM memories ORDER BY created_at")?;
        let rows = st.query_map([], |r| Ok(Memory { id: r.get(0)?, text: r.get(1)?, source: r.get(2)?, created_at: r.get(3)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Adds a memory unless the same text is already saved.
    pub fn add_memory(&self, text: &str, source: &str) -> AppResult<Memory> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AppError::msg("A memory can't be empty."));
        }
        if text.chars().count() > 500 {
            return Err(AppError::msg("Keep memories short (under 500 characters)."));
        }
        if let Some(m) = self.memories()?.into_iter().find(|m| m.text.eq_ignore_ascii_case(text)) {
            return Ok(m);
        }
        let m = Memory {
            id: uuid::Uuid::new_v4().to_string(),
            text: text.to_string(),
            source: source.to_string(),
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        self.conn().execute(
            "INSERT INTO memories (id, text, source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![m.id, m.text, m.source, m.created_at],
        )?;
        Ok(m)
    }

    pub fn update_memory(&self, id: &str, text: &str) -> AppResult<()> {
        let text = text.trim();
        if text.is_empty() {
            return self.delete_memory(id);
        }
        self.conn().execute(
            "UPDATE memories SET text = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, text, chrono::Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn delete_memory(&self, id: &str) -> AppResult<()> {
        self.conn().execute("DELETE FROM memories WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Deletes every chat and memory (Settings → Memory → Erase everything).
    pub fn wipe(&self) -> AppResult<()> {
        self.conn().execute_batch("DELETE FROM messages_fts; DELETE FROM messages; DELETE FROM conversations; DELETE FROM memories;")?;
        Ok(())
    }
}

fn str_field<'a>(v: &'a Value, key: &str) -> AppResult<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| AppError::msg(format!("missing {key}")))
}

fn migrate(conn: &Connection) -> AppResult<()> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE conversations (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 pinned INTEGER NOT NULL DEFAULT 0,
                 folder TEXT,
                 summary TEXT
             );
             CREATE TABLE messages (
                 id TEXT PRIMARY KEY,
                 conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                 seq INTEGER NOT NULL,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 data TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX messages_by_conversation ON messages(conversation_id, seq);
             CREATE VIRTUAL TABLE messages_fts USING fts5(
                 content, title, conversation_id UNINDEXED, message_id UNINDEXED,
                 tokenize = 'porter unicode61'
             );
             CREATE TABLE memories (
                 id TEXT PRIMARY KEY,
                 text TEXT NOT NULL,
                 source TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    debug_assert_eq!(SCHEMA_VERSION, 1);
    Ok(())
}

/// Reads the database key, creating one for a new database.
fn load_or_create_key(path: &Path, db_exists: bool) -> AppResult<String> {
    if let Ok(k) = std::fs::read_to_string(path) {
        let k = k.trim().to_string();
        if k.len() == 64 && k.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(format!("x'{k}'"));
        }
    }
    if db_exists {
        log::warn!("database key missing or damaged; a new one will be created");
    }
    let hex = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    write_private(path, &hex)?;
    Ok(format!("x'{hex}'"))
}

/// Writes a file only this user can read.
fn write_private(path: &Path, contents: &str) -> AppResult<()> {
    let tmp: PathBuf = path.with_extension("tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Turns what the user typed into a safe FTS5 query: every word must match,
/// the last one as a prefix ("rust vers" finds "Rust version").
fn fts_query(input: &str) -> Option<String> {
    let words: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(12)
        .map(|w| format!("\"{}\"", w.to_lowercase()))
        .collect();
    let (last, rest) = words.split_last()?;
    let mut q = rest.join(" ");
    if !q.is_empty() {
        q.push(' ');
    }
    q.push_str(last);
    q.push('*');
    Some(q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat(id: &str, title: &str, texts: &[(&str, &str)]) -> Value {
        json!({
            "id": id,
            "title": title,
            "createdAt": 1000,
            "updatedAt": 2000,
            "messages": texts.iter().enumerate().map(|(i, (role, content))| json!({
                "id": format!("{id}-m{i}"), "role": role, "content": content, "status": "done", "createdAt": 1000 + i as i64,
                "sources": [{ "n": 1, "title": "t", "url": "https://example.com" }],
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn saves_loads_and_lists_chats() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        db.save(&chat("a", "Rust question", &[("user", "What's new in Rust?"), ("assistant", "Rust 1.98 added …")])).unwrap();
        let loaded = db.load("a").unwrap().unwrap();
        assert_eq!(loaded["title"], "Rust question");
        assert_eq!(loaded["messages"].as_array().unwrap().len(), 2);
        // Extra UI fields survive the round trip.
        assert_eq!(loaded["messages"][1]["sources"][0]["url"], "https://example.com");
        // Saving again with fewer messages removes the dropped ones.
        db.save(&chat("a", "Rust question", &[("user", "What's new in Rust?")])).unwrap();
        assert_eq!(db.load("a").unwrap().unwrap()["messages"].as_array().unwrap().len(), 1);
        let list = db.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].message_count, 1);
        assert!(db.load("missing").unwrap().is_none());
    }

    #[test]
    fn pins_folders_titles_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        db.save(&chat("a", "Older", &[("user", "one")])).unwrap();
        let mut b = chat("b", "Newer", &[("user", "two")]);
        b["updatedAt"] = json!(9000);
        db.save(&b).unwrap();
        assert_eq!(db.list().unwrap()[0].id, "b");
        db.update_meta("a", &MetaPatch { pinned: Some(true), folder: Some("Work".into()), title: Some("Renamed".into()) }).unwrap();
        let list = db.list().unwrap();
        assert_eq!((list[0].id.as_str(), list[0].pinned, list[0].folder.as_deref(), list[0].title.as_str()), ("a", true, Some("Work"), "Renamed"));
        // A later save from the UI keeps the pin and folder.
        db.save(&chat("a", "Renamed", &[("user", "one"), ("assistant", "ok")])).unwrap();
        assert!(db.list().unwrap()[0].pinned);
        db.update_meta("a", &MetaPatch { folder: Some(String::new()), ..Default::default() }).unwrap();
        assert_eq!(db.list().unwrap()[0].folder, None);
        db.delete("a").unwrap();
        assert_eq!(db.list().unwrap().len(), 1);
        assert!(db.search("one", 10).unwrap().is_empty());
    }

    #[test]
    fn full_text_search_finds_words_and_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        db.save(&chat("a", "Trip ideas", &[("user", "Plan a weekend in Lisbon"), ("assistant", "Day 1: Alfama and trams")])).unwrap();
        db.save(&chat("b", "Taxes", &[("user", "How do quarterly taxes work?")])).unwrap();
        let hits = db.search("lisb", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].conversation_id, "a");
        assert!(hits[0].snippet.contains("«Lisbon»"), "{}", hits[0].snippet);
        // Titles are searchable, several words must all match, and punctuation can't break the query.
        assert_eq!(db.search("trip", 10).unwrap()[0].conversation_id, "a");
        assert_eq!(db.search("quarterly taxes", 10).unwrap()[0].conversation_id, "b");
        assert!(db.search("quarterly lisbon", 10).unwrap().is_empty());
        assert!(db.search("\") OR * NEAR(", 10).is_ok());
        assert!(db.search("   ", 10).unwrap().is_empty());
        // Stemming: "working" finds "work".
        assert_eq!(db.search("working", 10).unwrap()[0].conversation_id, "b");
    }

    #[test]
    fn memories_add_dedupe_update_delete() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        let m = db.add_memory("Prefers metric units", "user").unwrap();
        assert_eq!(db.add_memory("prefers metric units", "chat").unwrap().id, m.id);
        assert!(db.add_memory("  ", "user").is_err());
        db.update_memory(&m.id, "Prefers metric units and 24-hour time").unwrap();
        assert_eq!(db.memories().unwrap()[0].text, "Prefers metric units and 24-hour time");
        db.update_memory(&m.id, "").unwrap();
        assert!(db.memories().unwrap().is_empty());
    }

    #[test]
    fn database_is_encrypted_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let db = Db::open(dir.path()).unwrap();
            db.save(&chat("a", "Secret plans", &[("user", "my secret word is pineapple")])).unwrap();
            db.add_memory("Lives in Denver", "user").unwrap();
        }
        let raw = std::fs::read(dir.path().join("byte.db")).unwrap();
        assert!(!raw.windows(9).any(|w| w == b"pineapple"), "plain text found in the database file");
        assert!(!raw.starts_with(b"SQLite format 3"));
        let key = std::fs::read_to_string(dir.path().join("db.key")).unwrap();
        assert_eq!(key.len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.path().join("db.key")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let db = Db::open(dir.path()).unwrap();
        assert_eq!(db.list().unwrap()[0].title, "Secret plans");
        assert_eq!(db.memories().unwrap()[0].text, "Lives in Denver");
    }

    #[test]
    fn lost_key_moves_the_old_database_aside() {
        let dir = tempfile::tempdir().unwrap();
        Db::open(dir.path()).unwrap().save(&chat("a", "x", &[("user", "hi")])).unwrap();
        std::fs::remove_file(dir.path().join("db.key")).unwrap();
        let db = Db::open(dir.path()).unwrap();
        assert!(db.list().unwrap().is_empty());
        let aside = std::fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with("byte.db.unreadable-"));
        assert!(aside);
    }

    #[test]
    fn fts_queries_are_quoted() {
        assert_eq!(fts_query("rust vers").as_deref(), Some("\"rust\" \"vers\"*"));
        assert_eq!(fts_query("a\"b OR c").as_deref(), Some("\"a\" \"b\" \"or\" \"c\"*"));
        assert_eq!(fts_query("!!!"), None);
    }
}
