//! Knowledge base: folders the user chooses are read (with `files::ingest`),
//! split into passages and indexed two ways, by words (SQLite FTS5) and by
//! meaning (embeddings, `embed.rs`). Search merges both rankings with
//! reciprocal-rank fusion, so exact names and loosely worded questions both
//! find the right passage. Everything stays in the encrypted database.
//!
//! Folders are re-scanned at launch, every 15 minutes and on request; only
//! files whose size or modification time changed are read again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::Db;
use crate::embed::{Embedder, Purpose};
use crate::error::{AppError, AppResult};
use crate::files::{self, FileKind};
use crate::state::AppState;

pub const PROGRESS_EVENT: &str = "kb://progress";
/// Passage size: about 600–700 tokens.
const CHUNK_CHARS: usize = 2_400;
/// Text repeated from the end of one passage at the start of the next.
const OVERLAP_CHARS: usize = 350;
/// Files per folder (a whole home folder by mistake shouldn't run for hours).
const MAX_FILES: usize = 20_000;
/// Folders skipped while walking (build output, dependencies, app data).
const SKIP_DIRS: &[&str] = &["node_modules", "target", "build", "dist", "Library", "__pycache__", "venv", ".git"];
/// Passages embedded per round.
const EMBED_BATCH: usize = 64;
/// Candidates from each ranking before they're merged.
const CANDIDATES: usize = 40;
pub const RESCAN_EVERY: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: i64,
    pub path: String,
    pub added_at: i64,
    pub last_scan: Option<i64>,
    pub error: Option<String>,
    pub files: i64,
    pub chunks: i64,
    /// Passages that have an embedding (searchable by meaning).
    pub embedded: i64,
    /// Bytes of text stored.
    pub bytes: i64,
}

/// A passage found by `search`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub chunk_id: i64,
    pub path: String,
    pub name: String,
    pub page: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub source_id: i64,
    /// "reading", "embedding" or "done".
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub file: String,
}

// ---------- pure helpers ----------

/// Splits extracted text into overlapping passages, keeping the page (or
/// slide/sheet) each starts on from `files`' `[Page N]` markers.
pub fn chunk(text: &str) -> Vec<(Option<u32>, String)> {
    fn emit(cur: &str, page: Option<u32>, out: &mut Vec<(Option<u32>, String)>) {
        let t = cur.trim();
        if t.chars().any(char::is_alphanumeric) {
            out.push((page, t.to_string()));
        }
    }
    /// The last `OVERLAP_CHARS` of a passage, starting at a word.
    fn tail(cur: &str) -> String {
        let chars: Vec<char> = cur.chars().collect();
        let start = chars.len().saturating_sub(OVERLAP_CHARS);
        let s: String = chars[start..].iter().collect();
        match s.find(char::is_whitespace) {
            Some(i) if start > 0 => s[i..].trim_start().to_string(),
            _ => s,
        }
    }
    let mut out = Vec::new();
    let mut page: Option<u32> = None;
    let mut cur = String::new();
    let mut cur_page = None;
    // `cur` holds text not yet in `out` (beyond the carried-over tail).
    let mut fresh = false;
    for para in text.split("\n\n").flat_map(split_long) {
        let p = para.trim();
        let body = match marker(p) {
            // A new page starts a new passage (no overlap across pages).
            Some(n) => {
                if fresh {
                    emit(&cur, cur_page, &mut out);
                }
                cur.clear();
                fresh = false;
                page = Some(n);
                p.split_once('\n').map(|(_, r)| r.trim()).unwrap_or("")
            }
            None => p,
        };
        if body.is_empty() {
            continue;
        }
        if fresh && cur.chars().count() + body.chars().count() > CHUNK_CHARS {
            emit(&cur, cur_page, &mut out);
            cur = tail(&cur);
            fresh = false;
        }
        if !fresh {
            cur_page = page;
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(body);
        fresh = true;
    }
    if fresh {
        emit(&cur, cur_page, &mut out);
    }
    out
}

/// `[Page 3]`, `[Slide 2]`, `[Sheet 1]` at the start of a paragraph.
fn marker(p: &str) -> Option<u32> {
    let first = p.lines().next()?;
    let inner = first.strip_prefix('[')?.strip_suffix(']')?;
    let (word, n) = inner.split_once(' ')?;
    matches!(word, "Page" | "Slide" | "Sheet").then(|| n.parse().ok()).flatten()
}

/// Paragraphs longer than a passage are cut at sentence ends (or anywhere).
fn split_long(p: &str) -> Vec<String> {
    if p.chars().count() <= CHUNK_CHARS {
        return vec![p.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for sentence in p.split_inclusive(['.', '!', '?', '\n']) {
        if cur.chars().count() + sentence.chars().count() > CHUNK_CHARS && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if sentence.chars().count() > CHUNK_CHARS {
            let chars: Vec<char> = sentence.chars().collect();
            for piece in chars.chunks(CHUNK_CHARS) {
                out.push(piece.iter().collect());
            }
        } else {
            cur.push_str(sentence);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Reciprocal-rank fusion: items high in either list rise to the top.
pub fn rrf(lists: &[Vec<i64>], k: f32) -> Vec<i64> {
    let mut score: HashMap<i64, f32> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *score.entry(*id).or_default() += 1.0 / (k + rank as f32 + 1.0);
        }
    }
    let mut ids: Vec<(i64, f32)> = score.into_iter().collect();
    ids.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    ids.into_iter().map(|(id, _)| id).collect()
}

/// A safe FTS5 query: the question's words (2+ letters), any of them.
pub fn fts_query(q: &str) -> Option<String> {
    let words: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .take(24)
        .map(|w| format!("\"{}\"", w.to_lowercase()))
        .collect();
    (!words.is_empty()).then(|| words.join(" OR "))
}

// ---------- database ----------

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn add_source(db: &Db, path: &Path) -> AppResult<i64> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !path.is_dir() {
        return Err(AppError::msg("choose a folder"));
    }
    let p = path.to_string_lossy().to_string();
    let conn = db.conn();
    conn.execute("INSERT OR IGNORE INTO kb_sources (path, added_at) VALUES (?1, ?2)", params![p, now()])?;
    Ok(conn.query_row("SELECT id FROM kb_sources WHERE path = ?1", [p], |r| r.get(0))?)
}

pub fn remove_source(db: &Db, id: i64) -> AppResult<()> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM kb_fts WHERE rowid IN (SELECT c.id FROM kb_chunks c JOIN kb_files f ON f.id = c.file_id WHERE f.source_id = ?1)", [id])?;
    tx.execute("DELETE FROM kb_chunks WHERE file_id IN (SELECT id FROM kb_files WHERE source_id = ?1)", [id])?;
    tx.execute("DELETE FROM kb_files WHERE source_id = ?1", [id])?;
    tx.execute("DELETE FROM kb_sources WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

pub fn sources(db: &Db) -> AppResult<Vec<Source>> {
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT s.id, s.path, s.added_at, s.last_scan, s.error,
                (SELECT COUNT(*) FROM kb_files f WHERE f.source_id = s.id),
                (SELECT COUNT(*) FROM kb_chunks c JOIN kb_files f ON f.id = c.file_id WHERE f.source_id = s.id),
                (SELECT COUNT(*) FROM kb_chunks c JOIN kb_files f ON f.id = c.file_id WHERE f.source_id = s.id AND c.embedding IS NOT NULL),
                (SELECT COALESCE(SUM(LENGTH(c.text)), 0) FROM kb_chunks c JOIN kb_files f ON f.id = c.file_id WHERE f.source_id = s.id)
         FROM kb_sources s ORDER BY s.added_at",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Source {
            id: r.get(0)?,
            path: r.get(1)?,
            added_at: r.get(2)?,
            last_scan: r.get(3)?,
            error: r.get(4)?,
            files: r.get(5)?,
            chunks: r.get(6)?,
            embedded: r.get(7)?,
            bytes: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Passages in the knowledge base (0 = nothing to search).
pub fn chunk_count(db: &Db) -> i64 {
    db.conn().query_row("SELECT COUNT(*) FROM kb_chunks", [], |r| r.get(0)).unwrap_or(0)
}

fn known_files(db: &Db, source_id: i64) -> AppResult<HashMap<String, (i64, i64, i64)>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT path, id, mtime, size FROM kb_files WHERE source_id = ?1")?;
    let rows = st.query_map([source_id], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn delete_file(db: &Db, file_id: i64) -> AppResult<()> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM kb_fts WHERE rowid IN (SELECT id FROM kb_chunks WHERE file_id = ?1)", [file_id])?;
    tx.execute("DELETE FROM kb_chunks WHERE file_id = ?1", [file_id])?;
    tx.execute("DELETE FROM kb_files WHERE id = ?1", [file_id])?;
    tx.commit()?;
    Ok(())
}

/// Stores a file's passages (replacing what was there). A file that couldn't
/// be read is kept with its error so it isn't retried until it changes.
#[allow(clippy::too_many_arguments)]
fn store_file(db: &Db, source_id: i64, path: &str, mtime: i64, size: i64, kind: &str, pages: Option<u32>, chunks: &[(Option<u32>, String)], error: Option<&str>) -> AppResult<()> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    let old: Option<i64> = tx.query_row("SELECT id FROM kb_files WHERE path = ?1", [path], |r| r.get(0)).optional()?;
    if let Some(id) = old {
        tx.execute("DELETE FROM kb_fts WHERE rowid IN (SELECT id FROM kb_chunks WHERE file_id = ?1)", [id])?;
        tx.execute("DELETE FROM kb_chunks WHERE file_id = ?1", [id])?;
        tx.execute("DELETE FROM kb_files WHERE id = ?1", [id])?;
    }
    tx.execute(
        "INSERT INTO kb_files (source_id, path, mtime, size, kind, pages, error) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![source_id, path, mtime, size, kind, pages, error],
    )?;
    let file_id = tx.last_insert_rowid();
    for (i, (page, text)) in chunks.iter().enumerate() {
        tx.execute("INSERT INTO kb_chunks (file_id, ord, page, text) VALUES (?1, ?2, ?3, ?4)", params![file_id, i as i64, page, text])?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO kb_fts (rowid, text) VALUES (?1, ?2)", params![id, text])?;
    }
    tx.commit()?;
    Ok(())
}

fn unembedded(db: &Db, limit: usize) -> AppResult<Vec<(i64, String)>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, text FROM kb_chunks WHERE embedding IS NULL ORDER BY id LIMIT ?1")?;
    let rows = st.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn set_embeddings(db: &Db, rows: &[(i64, Vec<f32>)]) -> AppResult<()> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    for (id, v) in rows {
        tx.execute("UPDATE kb_chunks SET embedding = ?2 WHERE id = ?1", params![id, crate::embed::to_bytes(v)])?;
    }
    tx.commit()?;
    Ok(())
}

fn set_scanned(db: &Db, source_id: i64, error: Option<&str>) -> AppResult<()> {
    db.conn().execute("UPDATE kb_sources SET last_scan = ?2, error = ?3 WHERE id = ?1", params![source_id, now(), error])?;
    Ok(())
}

/// Passage ids matching the words, best first (BM25).
pub fn fts_search(db: &Db, query: &str, limit: usize) -> AppResult<Vec<i64>> {
    let Some(q) = fts_query(query) else { return Ok(Vec::new()) };
    let conn = db.conn();
    let mut st = conn.prepare("SELECT rowid FROM kb_fts WHERE kb_fts MATCH ?1 ORDER BY bm25(kb_fts) LIMIT ?2")?;
    let rows = st.query_map(params![q, limit as i64], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Passage ids closest in meaning to `query_vec`, best first.
pub fn vector_search(db: &Db, query_vec: &[f32], limit: usize) -> AppResult<Vec<i64>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, embedding FROM kb_chunks WHERE embedding IS NOT NULL")?;
    let mut rows = st.query([])?;
    let mut best: Vec<(f32, i64)> = Vec::with_capacity(limit + 1);
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let blob: Vec<u8> = r.get(1)?;
        let score = crate::embed::cosine(query_vec, &crate::embed::from_bytes(&blob));
        if best.len() < limit || score > best.last().map(|b| b.0).unwrap_or(f32::MIN) {
            let at = best.partition_point(|b| b.0 >= score);
            best.insert(at, (score, id));
            best.truncate(limit);
        }
    }
    Ok(best.into_iter().map(|(_, id)| id).collect())
}

fn embedded_count(db: &Db) -> i64 {
    db.conn().query_row("SELECT COUNT(*) FROM kb_chunks WHERE embedding IS NOT NULL", [], |r| r.get(0)).unwrap_or(0)
}

pub fn hits(db: &Db, ids: &[i64]) -> AppResult<Vec<Hit>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT c.id, f.path, c.page, c.text FROM kb_chunks c JOIN kb_files f ON f.id = c.file_id WHERE c.id = ?1")?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(h) = st
            .query_row([id], |r| {
                let path: String = r.get(1)?;
                let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
                Ok(Hit { chunk_id: r.get(0)?, path, name, page: r.get(2)?, text: r.get(3)? })
            })
            .optional()?
        {
            out.push(h);
        }
    }
    Ok(out)
}

// ---------- walking folders ----------

/// Files BYTE can read under `dir` (hidden files and build folders skipped).
pub fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let p = e.path();
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) && !name.ends_with(".app") {
                    stack.push(p);
                }
            } else if ft.is_file() && readable(&p) {
                out.push(p);
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

fn readable(p: &Path) -> bool {
    let size_ok = std::fs::metadata(p).map(|m| m.len() > 0 && m.len() <= files::MAX_FILE_BYTES).unwrap_or(false);
    match files::kind_of(p) {
        // Photos add text only through text recognition (macOS Vision, Windows OCR).
        Some(FileKind::Image) => cfg!(any(target_os = "macos", windows)) && size_ok,
        Some(_) => size_ok,
        None => false,
    }
}

fn stamp(p: &Path) -> (i64, i64) {
    let m = std::fs::metadata(p).ok();
    let mtime = m.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
    (mtime, m.map(|m| m.len() as i64).unwrap_or(0))
}

fn kind_name(k: FileKind) -> &'static str {
    match k {
        FileKind::Pdf => "pdf",
        FileKind::Word => "word",
        FileKind::Slides => "slides",
        FileKind::Sheet => "sheet",
        FileKind::Text => "text",
        FileKind::Web => "web",
        FileKind::Image => "image",
        FileKind::Audio => "audio",
    }
}

// ---------- indexing ----------

/// One index run at a time.
static INDEXING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Re-reads changed files in one folder (or all of them), then embeds new passages.
pub async fn index(app: &AppHandle, only: Option<i64>) -> AppResult<()> {
    let _guard = INDEXING.lock().await;
    let state = app.state::<AppState>();
    let list = sources(&state.db)?;
    for s in list.into_iter().filter(|s| only.is_none_or(|id| id == s.id)) {
        let result = index_source(app, &state.db, &s).await;
        let _ = set_scanned(&state.db, s.id, result.as_ref().err().map(|e| e.to_string()).as_deref());
    }
    embed_pending(app).await;
    let _ = app.emit(PROGRESS_EVENT, Progress { source_id: only.unwrap_or(0), phase: "done".into(), done: 0, total: 0, file: String::new() });
    Ok(())
}

async fn index_source(app: &AppHandle, db: &Db, s: &Source) -> AppResult<()> {
    let root = PathBuf::from(&s.path);
    if !root.is_dir() {
        return Err(AppError::msg("folder not found (moved or deleted?)"));
    }
    let found = tauri::async_runtime::spawn_blocking(move || walk(&root)).await.map_err(|e| AppError::msg(e.to_string()))?;
    let mut known = known_files(db, s.id)?;
    let changed: Vec<(PathBuf, i64, i64)> = found
        .iter()
        .filter_map(|p| {
            let key = p.to_string_lossy().to_string();
            let (mtime, size) = stamp(p);
            let same = known.remove(&key).is_some_and(|(_, m, z)| m == mtime && z == size);
            (!same).then(|| (p.clone(), mtime, size))
        })
        .collect();
    // Whatever is left in `known` was deleted or moved away.
    for (_, (id, _, _)) in known {
        delete_file(db, id)?;
    }
    let total = changed.len();
    for (i, (path, mtime, size)) in changed.into_iter().enumerate() {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let _ = app.emit(PROGRESS_EVENT, Progress { source_id: s.id, phase: "reading".into(), done: i, total, file: name });
        let p = path.clone();
        let read = tauri::async_runtime::spawn_blocking(move || files::ingest(&p)).await.map_err(|e| AppError::msg(e.to_string()))?;
        let key = path.to_string_lossy().to_string();
        let kind = files::kind_of(&path).map(kind_name).unwrap_or("text");
        match read {
            Ok(f) => store_file(db, s.id, &key, mtime, size, kind, f.pages, &chunk(&f.text), None)?,
            Err(e) => store_file(db, s.id, &key, mtime, size, kind, None, &[], Some(&e.to_string()))?,
        }
    }
    Ok(())
}

/// Embeds passages that don't have a vector yet, if the embedding model is
/// downloaded. Word search works without it.
async fn embed_pending(app: &AppHandle) {
    let state = app.state::<AppState>();
    let catalog = state.catalog.get();
    if !Embedder::installed(&catalog, &state.paths.models) {
        return;
    }
    let total = state.db.conn().query_row("SELECT COUNT(*) FROM kb_chunks WHERE embedding IS NULL", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
    let mut done = 0;
    loop {
        let batch = match unembedded(&state.db, EMBED_BATCH) {
            Ok(b) if !b.is_empty() => b,
            _ => return,
        };
        let _ = app.emit(PROGRESS_EVENT, Progress { source_id: 0, phase: "embedding".into(), done, total, file: String::new() });
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        match state.embedder.embed(app, &state.paths.models, &catalog, &texts, Purpose::Document).await {
            Ok(vecs) => {
                let rows: Vec<(i64, Vec<f32>)> = batch.iter().map(|(id, _)| *id).zip(vecs).collect();
                if set_embeddings(&state.db, &rows).is_err() {
                    return;
                }
                done += rows.len();
            }
            Err(e) => {
                log::warn!("knowledge base: embedding stopped: {e}");
                return;
            }
        }
    }
}

/// The best passages for `query`: word matches and meaning matches merged.
pub async fn search(app: &AppHandle, query: &str, limit: usize) -> AppResult<Vec<Hit>> {
    let state = app.state::<AppState>();
    let words = fts_search(&state.db, query, CANDIDATES)?;
    let mut lists = vec![words];
    if embedded_count(&state.db) > 0 {
        let catalog = state.catalog.get();
        match state.embedder.embed(app, &state.paths.models, &catalog, &[query.to_string()], Purpose::Query).await {
            Ok(v) if !v.is_empty() => lists.push(vector_search(&state.db, &v[0], CANDIDATES)?),
            Ok(_) => {}
            Err(e) => log::info!("knowledge base: searching by words only ({e})"),
        }
    }
    let ids: Vec<i64> = rrf(&lists, 60.0).into_iter().take(limit).collect();
    hits(&state.db, &ids)
}

/// Re-scans every folder at launch (after a short pause) and then regularly.
pub fn schedule(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            let enabled = app.state::<AppState>().settings.lock().await.kb_enabled;
            if enabled && !sources(&app.state::<AppState>().db).map(|s| s.is_empty()).unwrap_or(true) {
                if let Err(e) = index(&app, None).await {
                    log::warn!("knowledge base scan failed: {e}");
                }
            }
            tokio::time::sleep(RESCAN_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        (dir, db)
    }

    #[test]
    fn passages_keep_their_page_and_overlap() {
        let text = format!(
            "[Page 1]\nIntro line.\n\n[Page 2]\n{}\n\n{}",
            "The tenant pays for damage from unreported leaks. ".repeat(40),
            "Rent is due on the first of each month. ".repeat(40)
        );
        let parts = chunk(&text);
        assert_eq!(parts[0], (Some(1), "Intro line.".to_string()));
        assert!(parts.len() >= 3, "{}", parts.len());
        assert!(parts[1..].iter().all(|(p, _)| *p == Some(2)));
        assert!(parts.iter().all(|(_, t)| t.chars().count() <= CHUNK_CHARS + OVERLAP_CHARS + 10));
        // The next passage starts with the end of the previous one.
        let tail: String = parts[1].1.chars().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect();
        assert!(parts[2].1.contains(tail.trim()), "no overlap");
        assert!(chunk("").is_empty() && chunk("[Page 1]\n\n[Page 2]").is_empty());
    }

    #[test]
    fn rank_fusion_rewards_agreement() {
        let merged = rrf(&[vec![1, 2, 3], vec![3, 4, 1]], 60.0);
        assert_eq!(&merged[..2], &[1, 3]);
        assert_eq!(merged.len(), 4);
        assert_eq!(fts_query("What's my lease's pet-policy?").unwrap(), "\"what\" OR \"my\" OR \"lease\" OR \"pet\" OR \"policy\"");
        assert!(fts_query("? !").is_none());
    }

    #[test]
    fn folders_index_search_and_forget() {
        let (_d, db) = test_db();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("lease.md"), "# Lease\n\nPets are allowed with a $300 deposit.").unwrap();
        std::fs::create_dir(folder.path().join("node_modules")).unwrap();
        std::fs::write(folder.path().join("node_modules/x.js"), "ignored").unwrap();
        std::fs::write(folder.path().join(".secret.txt"), "hidden").unwrap();
        let files = walk(folder.path());
        assert_eq!(files.len(), 1);

        let id = add_source(&db, folder.path()).unwrap();
        assert_eq!(add_source(&db, folder.path()).unwrap(), id, "same folder twice");
        let path = files[0].to_string_lossy().to_string();
        let f = files::ingest(&files[0]).unwrap();
        store_file(&db, id, &path, 1, 2, "text", None, &chunk(&f.text), None).unwrap();
        assert_eq!(chunk_count(&db), 1);

        let found = fts_search(&db, "are pets allowed?", 10).unwrap();
        let h = hits(&db, &found).unwrap();
        assert_eq!(h[0].name, "lease.md");
        assert!(h[0].text.contains("$300 deposit"));

        // Vectors: nearest first.
        set_embeddings(&db, &[(h[0].chunk_id, vec![1.0, 0.0])]).unwrap();
        assert_eq!(vector_search(&db, &[1.0, 0.0], 5).unwrap(), vec![h[0].chunk_id]);

        // Re-storing replaces; removing the folder removes everything.
        store_file(&db, id, &path, 3, 4, "text", None, &[(None, "Updated text about parking".into())], None).unwrap();
        assert!(fts_search(&db, "pets", 10).unwrap().is_empty());
        assert_eq!(fts_search(&db, "parking", 10).unwrap().len(), 1);
        let s = &sources(&db).unwrap()[0];
        assert_eq!((s.files, s.chunks, s.embedded), (1, 1, 0));
        remove_source(&db, id).unwrap();
        assert!(sources(&db).unwrap().is_empty());
        assert_eq!(chunk_count(&db), 0);
        assert!(fts_search(&db, "parking", 10).unwrap().is_empty());
    }
}
