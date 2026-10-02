//! Backups, restore, automatic deletion of old chats, and erasing everything.
//!
//! A backup is one file: a zip of the active profile's data folder (the
//! SQLCipher database, its key, settings, the activity log…) and, by
//! default, the notes folder, sealed with a passphrase the user chooses
//! (PBKDF2-HMAC-SHA256, 600,000 rounds → ChaCha20-Poly1305, both from `ring`).
//! Without the passphrase the file is useless, so it can sit in iCloud Drive.
//! Models are never included (they're big and can be downloaded again).
//!
//! Restoring and erasing replace files the running app has open, so they're
//! staged and applied at the next launch (`apply_pending`, before the database
//! opens), and BYTE restarts. A restore keeps the replaced files in
//! `before-restore-<time>/` in the data folder.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

const MAGIC: &[u8; 8] = b"BYTEBK1\n";
const SALT_LEN: usize = 16;
const ROUNDS: u32 = if cfg!(test) { 1_000 } else { 600_000 };
const EXT: &str = "bytebackup";
/// Backups kept in the folder; older ones are removed after each new one.
const KEEP: usize = 5;
const RESTORE_DIR: &str = "restore-pending";
const ERASE_MARK: &str = "erase-pending";

// ------------------------------------------------------------------ sealing

fn derive(pass: &str, salt: &[u8]) -> LessSafeKey {
    let mut key = [0u8; 32];
    ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, std::num::NonZeroU32::new(ROUNDS).unwrap(), salt, pass.as_bytes(), &mut key);
    LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, &key).expect("32-byte key"))
}

/// `MAGIC | salt | nonce | ciphertext+tag`.
pub fn seal(plain: &[u8], pass: &str) -> AppResult<Vec<u8>> {
    if pass.chars().count() < 8 {
        return Err(AppError::msg("Use a passphrase of at least 8 characters."));
    }
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut salt).and_then(|_| rng.fill(&mut nonce)).map_err(|_| AppError::msg("No randomness available."))?;
    let mut buf = plain.to_vec();
    derive(pass, &salt)
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(MAGIC), &mut buf)
        .map_err(|_| AppError::msg("Couldn't encrypt the backup."))?;
    let mut out = Vec::with_capacity(MAGIC.len() + SALT_LEN + NONCE_LEN + buf.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&buf);
    Ok(out)
}

pub fn open(sealed: &[u8], pass: &str) -> AppResult<Vec<u8>> {
    let head = MAGIC.len() + SALT_LEN + NONCE_LEN;
    if sealed.len() < head || &sealed[..MAGIC.len()] != MAGIC {
        return Err(AppError::msg("That isn't a BYTE backup file."));
    }
    let salt = &sealed[MAGIC.len()..MAGIC.len() + SALT_LEN];
    let nonce: [u8; NONCE_LEN] = sealed[MAGIC.len() + SALT_LEN..head].try_into().expect("nonce length");
    let mut buf = sealed[head..].to_vec();
    let plain = derive(pass, salt)
        .open_in_place(Nonce::assume_unique_for_key(nonce), Aad::from(MAGIC), &mut buf)
        .map_err(|_| AppError::msg("Wrong passphrase (or the file is damaged)."))?;
    Ok(plain.to_vec())
}

// ------------------------------------------------------------------ archive

/// Files in the data folder that a backup leaves out.
fn skipped(name: &str) -> bool {
    name.starts_with("before-restore-") || name == RESTORE_DIR || name == ERASE_MARK || name.contains(".unreadable-") || name.ends_with("-wal") || name.ends_with("-shm") || name == ".DS_Store"
}

fn add_dir(zip: &mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>, dir: &Path, prefix: &str, top: bool) -> AppResult<()> {
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for entry in walkdir::WalkDir::new(dir).min_depth(1).sort_by_file_name().into_iter().filter_entry(|e| !(top && e.depth() == 1 && skipped(&e.file_name().to_string_lossy()))) {
        let entry = entry.map_err(|e| AppError::msg(e.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(dir).map_err(|e| AppError::msg(e.to_string()))?;
        let name = format!("{prefix}{}", rel.to_string_lossy().replace('\\', "/"));
        zip.start_file(name, opts).map_err(|e| AppError::msg(e.to_string()))?;
        zip.write_all(&std::fs::read(entry.path())?)?;
    }
    Ok(())
}

/// The plain (unsealed) archive. The database is checkpointed first so the
/// main file holds everything, and nothing writes to it while it's read.
pub fn archive(data: &Path, notes: Option<&Path>, db: &crate::db::Db) -> AppResult<Vec<u8>> {
    let conn = db.conn();
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    add_dir(&mut zip, data, "data/", true)?;
    drop(conn);
    if let Some(n) = notes.filter(|n| n.is_dir()) {
        add_dir(&mut zip, n, "notes/", false)?;
    }
    Ok(zip.finish().map_err(|e| AppError::msg(e.to_string()))?.into_inner())
}

/// A zip entry name that stays inside its folder ("data/x", "notes/a/b.md").
fn safe_rel(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() || name.contains('\\') {
        return None;
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::Normal(s) => out.push(s),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Unpacks a backup's data into `data/restore-pending/` (applied at the next
/// launch) and its notes into `notes_dir/Restored <date>/`. Returns how many
/// notes were restored.
pub fn stage_restore(plain: &[u8], data: &Path, notes_dir: Option<&Path>, date: &str) -> AppResult<usize> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(plain)).map_err(|_| AppError::msg("The backup's contents are damaged."))?;
    let names: Vec<String> = zip.file_names().map(str::to_string).collect();
    if !names.iter().any(|n| n == "data/byte.db") || !names.iter().any(|n| n == "data/db.key") {
        return Err(AppError::msg("That backup has no chats database in it."));
    }
    let pending = data.join(RESTORE_DIR);
    let _ = std::fs::remove_dir_all(&pending);
    let restored_notes = notes_dir.map(|n| n.join(format!("Restored {date}")));
    let mut notes = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| AppError::msg(e.to_string()))?;
        if !f.is_file() {
            continue;
        }
        let name = f.name().to_string();
        let target = if let Some(rest) = name.strip_prefix("data/") {
            safe_rel(rest).map(|r| pending.join(r))
        } else if let Some(rest) = name.strip_prefix("notes/") {
            restored_notes.as_ref().and_then(|d| safe_rel(rest).map(|r| d.join(r)))
        } else {
            None
        };
        let Some(target) = target else { continue };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)?;
        std::fs::write(&target, bytes)?;
        if name.starts_with("notes/") {
            notes += 1;
        }
    }
    // Written last: only a complete restore is applied.
    std::fs::write(pending.join(".complete"), b"")?;
    Ok(notes)
}

/// At launch, before the database opens: finish a staged restore or erase.
pub fn apply_pending(data: &Path) -> AppResult<()> {
    if data.join(ERASE_MARK).exists() {
        for entry in std::fs::read_dir(data)?.flatten() {
            let p = entry.path();
            let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            if let Err(e) = r {
                log::warn!("erase: couldn't remove {}: {e}", p.display());
            }
        }
        log::info!("erased BYTE's data in {}", data.display());
        return Ok(());
    }
    let pending = data.join(RESTORE_DIR);
    if !pending.join(".complete").exists() {
        let _ = std::fs::remove_dir_all(&pending);
        return Ok(());
    }
    let aside = data.join(format!("before-restore-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::create_dir_all(&aside)?;
    for entry in std::fs::read_dir(data)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == RESTORE_DIR || name.starts_with("before-restore-") {
            continue;
        }
        std::fs::rename(entry.path(), aside.join(&name))?;
    }
    for entry in std::fs::read_dir(&pending)?.flatten() {
        if entry.file_name() != ".complete" {
            std::fs::rename(entry.path(), data.join(entry.file_name()))?;
        }
    }
    std::fs::remove_dir_all(&pending)?;
    log::info!("restored a backup (the old files are in {})", aside.display());
    Ok(())
}

// ------------------------------------------------------------------ folder

/// iCloud Drive's BYTE Backups folder when iCloud Drive is on, else Documents/BYTE/Backups.
pub fn default_dir() -> PathBuf {
    let home = dirs_home();
    let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if icloud.is_dir() {
        icloud.join("BYTE Backups")
    } else {
        home.join("Documents/BYTE/Backups")
    }
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn in_icloud(dir: &Path) -> bool {
    dir.to_string_lossy().contains("com~apple~CloudDocs")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    /// Unix milliseconds.
    pub modified: i64,
}

pub fn list(dir: &Path) -> Vec<BackupFile> {
    let mut out: Vec<BackupFile> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == EXT))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let modified = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64;
            Some(BackupFile { path: e.path().to_string_lossy().into(), name: e.file_name().to_string_lossy().into(), size: m.len(), modified })
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.name.cmp(&a.name)));
    out
}

/// Writes a sealed backup into `dir` and removes all but the newest `KEEP`.
pub fn write(dir: &Path, sealed: &[u8], stamp: &str) -> AppResult<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("BYTE backup {stamp}.{EXT}"));
    let tmp = path.with_extension("partial");
    std::fs::write(&tmp, sealed)?;
    std::fs::rename(&tmp, &path)?;
    for old in list(dir).into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(old.path);
    }
    Ok(path)
}

// ------------------------------------------------------------------ auto-delete

/// Deletes unpinned chats not touched for `days` days. Returns how many.
pub fn auto_delete(db: &crate::db::Db, days: u32, now_ms: i64) -> AppResult<usize> {
    if days == 0 {
        return Ok(0);
    }
    let cutoff = now_ms - i64::from(days) * 86_400_000;
    let old: Vec<String> = db.list()?.into_iter().filter(|c| !c.pinned && c.updated_at < cutoff).map(|c| c.id).collect();
    for id in &old {
        db.delete(id)?;
    }
    Ok(old.len())
}

// ------------------------------------------------------------------ commands

fn passphrase_account(state: &AppState) -> String {
    format!("{}#backup", state.cloud_account())
}

fn backup_dir(state: &AppState, s: &crate::settings::Settings) -> PathBuf {
    let _ = state;
    s.backup_dir.as_deref().filter(|d| !d.trim().is_empty()).map(PathBuf::from).unwrap_or_else(default_dir)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub dir: String,
    pub icloud: bool,
    pub files: Vec<BackupFile>,
    pub remembered: bool,
}

#[tauri::command]
pub async fn backup_info(state: State<'_, AppState>) -> AppResult<BackupInfo> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    let s = state.settings.lock().await.clone();
    let dir = backup_dir(&state, &s);
    let remembered = state.secrets.get(&passphrase_account(&state)).ok().flatten().is_some();
    Ok(BackupInfo { icloud: in_icloud(&dir), files: list(&dir), dir: dir.to_string_lossy().into(), remembered })
}

/// Makes a backup now. Without a passphrase, the one remembered in the Keychain is used.
pub async fn run(state: &AppState, passphrase: Option<String>, remember: bool) -> AppResult<PathBuf> {
    let account = passphrase_account(state);
    let pass = match passphrase.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => state.secrets.get(&account)?.ok_or_else(|| AppError::msg("Type a passphrase for the backup."))?,
    };
    let s = state.settings.lock().await.clone();
    let notes = s.backup_include_notes.then(|| crate::notes::dir_for(&s));
    let plain = archive(&state.paths.data, notes.as_deref(), &state.db)?;
    let sealed = seal(&plain, &pass)?;
    let path = write(&backup_dir(state, &s), &sealed, &chrono::Local::now().format("%Y-%m-%d %H%M").to_string())?;
    if remember {
        state.secrets.set(&account, &pass)?;
    }
    let mut cur = state.settings.lock().await;
    cur.last_backup = Some(chrono::Utc::now().timestamp_millis());
    let _ = cur.save(&state.paths.settings_file);
    Ok(path)
}

#[tauri::command]
pub async fn backup_now(state: State<'_, AppState>, passphrase: Option<String>, remember: bool) -> AppResult<String> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    Ok(run(&state, passphrase, remember).await?.to_string_lossy().into())
}

#[tauri::command]
pub async fn backup_forget(state: State<'_, AppState>) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    state.secrets.delete(&passphrase_account(&state))
}

/// Restores a backup file and restarts BYTE into it.
#[tauri::command]
pub async fn backup_restore(app: AppHandle, state: State<'_, AppState>, path: String, passphrase: String) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    let sealed = std::fs::read(&path).map_err(|e| AppError::msg(format!("Couldn't read that file: {e}")))?;
    let plain = open(&sealed, &passphrase)?;
    let notes = crate::notes::dir_for(&state.settings.lock().await.clone());
    stage_restore(&plain, &state.paths.data, Some(&notes), &chrono::Local::now().format("%Y-%m-%d").to_string())?;
    restart(&app, &state).await
}

/// Erases chats, memories, settings and everything else in this profile's
/// data folder (models and your notes files stay), then restarts BYTE.
#[tauri::command]
pub async fn erase_everything(app: AppHandle, state: State<'_, AppState>, confirm: String) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    if confirm.trim() != "ERASE" {
        return Err(AppError::msg("Type ERASE to confirm."));
    }
    if crate::lock::AVAILABLE && state.settings.lock().await.lock_enabled {
        crate::lock::verify("erase everything in BYTE").await?;
    }
    let _ = state.secrets.delete(&passphrase_account(&state));
    std::fs::write(state.paths.data.join(ERASE_MARK), b"")?;
    restart(&app, &state).await
}

async fn restart(app: &AppHandle, state: &AppState) -> AppResult<()> {
    state.engine.stop().await;
    for e in state.extras.all().await {
        e.stop().await;
    }
    app.restart();
}

/// Daily, from the scheduler: delete old chats, and make the weekly backup when it's on.
pub async fn daily(state: &AppState) {
    let s = state.settings.lock().await.clone();
    if s.auto_delete_days > 0 && !state.lock.is_locked() {
        match auto_delete(&state.db, s.auto_delete_days, chrono::Utc::now().timestamp_millis()) {
            Ok(0) => {}
            Ok(n) => log::info!("deleted {n} chats older than {} days", s.auto_delete_days),
            Err(e) => log::warn!("auto-delete: {e}"),
        }
    }
    let week = 7 * 86_400_000;
    if s.backup_auto && s.last_backup.is_none_or(|t| chrono::Utc::now().timestamp_millis() - t > week) {
        if let Err(e) = run(state, None, false).await {
            log::warn!("weekly backup: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealing_round_trips_and_refuses_the_wrong_passphrase() {
        let sealed = seal(b"hello chats", "correct horse").unwrap();
        assert!(sealed.starts_with(MAGIC));
        assert!(!sealed.windows(5).any(|w| w == b"hello"), "nothing readable");
        assert_eq!(open(&sealed, "correct horse").unwrap(), b"hello chats");
        assert!(open(&sealed, "wrong horse!").unwrap_err().to_string().contains("Wrong passphrase"));
        assert!(open(b"not a backup", "correct horse").is_err());
        assert!(seal(b"x", "short").is_err());
        // Each backup gets its own salt and nonce.
        assert_ne!(seal(b"same", "correct horse").unwrap(), seal(b"same", "correct horse").unwrap());
    }

    #[test]
    fn backup_then_restore_at_the_next_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let db = crate::db::Db::open(&data).unwrap();
        db.save(&serde_json::json!({ "id": "c1", "title": "Before", "messages": [{ "id": "m1", "role": "user", "content": "hi" }] })).unwrap();
        std::fs::write(data.join("settings.json"), "{\"userName\":\"Sam\"}").unwrap();
        std::fs::create_dir_all(data.join("before-restore-old")).unwrap();
        let notes = tmp.path().join("Notes");
        std::fs::create_dir_all(notes.join("Inbox")).unwrap();
        std::fs::write(notes.join("Inbox/Idea.md"), "# Idea").unwrap();

        let plain = archive(&data, Some(&notes), &db).unwrap();
        let names: Vec<String> = zip::ZipArchive::new(std::io::Cursor::new(&plain)).unwrap().file_names().map(str::to_string).collect();
        assert!(names.contains(&"data/byte.db".into()) && names.contains(&"data/db.key".into()) && names.contains(&"notes/Inbox/Idea.md".into()));
        assert!(!names.iter().any(|n| n.contains("before-restore")), "{names:?}");
        let sealed = seal(&plain, "a good passphrase").unwrap();

        // Later: the chat changed; restore the backup.
        db.save(&serde_json::json!({ "id": "c2", "title": "After", "messages": [] })).unwrap();
        drop(db);
        let restored = stage_restore(&open(&sealed, "a good passphrase").unwrap(), &data, Some(&notes), "2026-10-02").unwrap();
        assert_eq!(restored, 1);
        assert!(notes.join("Restored 2026-10-02/Inbox/Idea.md").exists());
        apply_pending(&data).unwrap();
        let db = crate::db::Db::open(&data).unwrap();
        assert_eq!(db.list().unwrap().iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Before"]);
        assert!(std::fs::read_dir(&data).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("before-restore-2")));
        assert!(!data.join(RESTORE_DIR).exists());
    }

    #[test]
    fn an_unfinished_restore_is_ignored_and_bad_names_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(RESTORE_DIR)).unwrap();
        std::fs::write(tmp.path().join(RESTORE_DIR).join("byte.db"), "x").unwrap();
        std::fs::write(tmp.path().join("byte.db"), "current").unwrap();
        apply_pending(tmp.path()).unwrap();
        assert_eq!(std::fs::read_to_string(tmp.path().join("byte.db")).unwrap(), "current");
        assert!(!tmp.path().join(RESTORE_DIR).exists());
        for bad in ["../x", "/etc/passwd", "a/../../b", "a\\b", ""] {
            assert!(safe_rel(bad).is_none(), "{bad}");
        }
        assert_eq!(safe_rel("Inbox/a.md"), Some(PathBuf::from("Inbox/a.md")));
    }

    #[test]
    fn erase_empties_the_data_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("byte.db"), "x").unwrap();
        std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join(ERASE_MARK), "").unwrap();
        apply_pending(tmp.path()).unwrap();
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn keeps_the_newest_five() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..7 {
            write(tmp.path(), b"x", &format!("2026-10-0{i} 0000")).unwrap();
        }
        let files = list(tmp.path());
        assert_eq!(files.len(), KEEP);
        assert!(files.iter().all(|f| f.name.ends_with(".bytebackup")));
        assert!(!tmp.path().join("BYTE backup 2026-10-00 0000.bytebackup").exists());
    }

    #[test]
    fn auto_delete_keeps_pinned_and_recent_chats() {
        let tmp = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(tmp.path()).unwrap();
        for id in ["old", "pinned", "new"] {
            db.save(&serde_json::json!({ "id": id, "title": id, "messages": [] })).unwrap();
        }
        db.update_meta("pinned", &crate::db::MetaPatch { pinned: Some(true), folder: None, title: None, project_id: None }).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        assert_eq!(auto_delete(&db, 0, now + 100 * 86_400_000).unwrap(), 0, "off");
        assert_eq!(auto_delete(&db, 30, now).unwrap(), 0, "nothing is old yet");
        assert_eq!(auto_delete(&db, 30, now + 31 * 86_400_000).unwrap(), 2);
        assert_eq!(db.list().unwrap().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["pinned"]);
    }
}
