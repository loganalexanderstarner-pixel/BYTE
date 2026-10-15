//! A portable backup of settings and chats to a file the person picks (a settings card, on every system).
//!
//! The file is sealed with a passphrase (the same ChaCha20-Poly1305 sealing as `backup.rs`) and holds the chats in plain
//! JSON (so they do not depend on this PC's database key) and the settings. It never holds the BYTE Cloud key (that lives
//! in the system's secret store, not in settings), the account details, the kids PIN, folders of this PC or anything whose
//! name says token, secret or password. Restoring writes the file next to the data folder and BYTE finishes it at the next
//! launch (the chats are put back by id, settings are merged over the current ones, and what this PC keeps for itself
//! stays as it is).

use std::path::Path;

use serde_json::{json, Map, Value};
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const FORMAT: &str = "byte-portable";
pub const VERSION: u32 = 1;
const STAGED: &str = "portable-restore.json";

/// Settings that belong to one PC or one account and are neither saved nor restored.
const KEEP_LOCAL: &[&str] = &[
    "cloudAccount", "cloudBaseUrl", "cloudConnected", "useCloud", "cloudMode", "kidsPin", "kidsMode", "notesDir", "backupDir", "lastBackup", "tuning", "activeModel", "loadedAlongside", "contextSize",
    "catalogUrl", "lockEnabled", "onboardingComplete", "betterModelHintFor", "homePlace",
];

/// Words that mean a setting holds a secret, wherever it appears in the name.
const SECRET_WORDS: &[&str] = &["token", "secret", "password", "passphrase", "credential", "apikey", "api_key", "cloudkey", "privatekey"];

fn looks_secret(key: &str) -> bool {
    let k = key.to_lowercase();
    SECRET_WORDS.iter().any(|w| k.contains(w))
}

/// The settings that may leave this PC.
pub fn portable_settings(settings: &Value) -> Value {
    let mut out = Map::new();
    if let Some(obj) = settings.as_object() {
        for (k, v) in obj {
            if KEEP_LOCAL.contains(&k.as_str()) || looks_secret(k) {
                continue;
            }
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

/// `current` with the restored portable settings laid over it; what stays local is kept from `current`.
pub fn merge_settings(current: &Value, restored: &Value) -> Value {
    let mut out = current.as_object().cloned().unwrap_or_default();
    for (k, v) in portable_settings(restored).as_object().into_iter().flatten() {
        out.insert(k.clone(), v.clone());
    }
    Value::Object(out)
}

pub fn build_payload(settings: &Value, chats: Vec<Value>, now_ms: i64) -> Value {
    json!({ "format": FORMAT, "version": VERSION, "createdAt": now_ms, "settings": portable_settings(settings), "chats": chats })
}

pub fn pack(payload: &Value, passphrase: &str) -> AppResult<Vec<u8>> {
    crate::backup::seal(&serde_json::to_vec(payload)?, passphrase)
}

pub fn unpack(sealed: &[u8], passphrase: &str) -> AppResult<Value> {
    let plain = crate::backup::open(sealed, passphrase)?;
    let v: Value = serde_json::from_slice(&plain).map_err(|_| AppError::msg("That file is a full BYTE backup, not a settings and chats backup. Use “Restore” in the Privacy backup section for it."))?;
    if v["format"] != FORMAT {
        return Err(AppError::msg("That isn't a BYTE settings and chats backup."));
    }
    if v["version"].as_u64().is_none_or(|n| n > u64::from(VERSION)) {
        return Err(AppError::msg("That backup was made by a newer BYTE. Update BYTE first."));
    }
    Ok(v)
}

/// Largest file or payload accepted (a lot of chats is a few tens of MB).
pub const MAX_BYTES: usize = 200_000_000;
const MAX_CHATS: usize = 20_000;
const MAX_MESSAGES: usize = 50_000;
const REPORT: &str = "portable-restore-report.json";

/// A chat that is safe to hand to the database: a short plain id, a bounded list of messages that each have an id.
pub fn valid_chat(c: &Value) -> bool {
    let id_ok = |v: &Value| v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')));
    if !id_ok(&c["id"]) {
        return false;
    }
    match c["messages"].as_array() {
        Some(m) => m.len() <= MAX_MESSAGES && m.iter().all(|m| id_ok(&m["id"]) && m["content"].as_str().is_none_or(|t| t.len() <= 5_000_000)),
        None => c.get("messages").is_none_or(Value::is_null),
    }
}

/// Writes the opened backup where the next launch picks it up. Returns (chats, settings) counts.
pub fn stage(data: &Path, payload: &Value) -> AppResult<(usize, usize)> {
    let chats = payload["chats"].as_array().map_or(0, Vec::len);
    let settings = payload["settings"].as_object().map_or(0, Map::len);
    let bytes = serde_json::to_vec(payload)?;
    if bytes.len() > MAX_BYTES || chats > MAX_CHATS {
        return Err(AppError::msg("That backup is too large to restore."));
    }
    std::fs::write(data.join(STAGED), bytes)?;
    Ok((chats, settings))
}

/// The settings after the restore, or why they cannot be built (nothing has been changed yet).
pub fn merged_settings(current: &Value, payload: &Value) -> Result<crate::settings::Settings, String> {
    let merged = merge_settings(current, &payload["settings"]);
    serde_json::from_value(merged).map_err(|e| format!("the saved settings could not be applied: {e}"))
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub ok: bool,
    pub chats: usize,
    pub skipped: usize,
    pub settings: usize,
    pub error: Option<String>,
    /// Where the copy of what was replaced is.
    pub kept_in: Option<String>,
}

fn restore(state: &AppState, payload: &Value) -> Result<RestoreReport, String> {
    let current = serde_json::to_value(&*state.settings.blocking_lock()).map_err(|e| e.to_string())?;
    // Build the settings first: if they cannot be applied, nothing is touched.
    let next = merged_settings(&current, payload)?;
    // A copy of what is about to be replaced, in the folder the "Free up space" card lists.
    let aside = state.paths.data.join(format!("before-restore-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::create_dir_all(&aside).map_err(|e| format!("could not keep a copy of your current data: {e}"))?;
    let chats_now = state.db.export_all().map_err(|e| format!("could not keep a copy of your current chats: {e}"))?;
    std::fs::write(aside.join("chats.json"), serde_json::to_vec(&chats_now).map_err(|e| e.to_string())?).map_err(|e| format!("could not keep a copy of your current chats: {e}"))?;
    std::fs::write(aside.join("settings.json"), serde_json::to_vec_pretty(&current).map_err(|e| e.to_string())?).map_err(|e| format!("could not keep a copy of your current settings: {e}"))?;
    next.save(&state.paths.settings_file).map_err(|e| format!("could not save the restored settings: {e}"))?;
    *state.settings.blocking_lock() = next;
    let (mut chats, mut skipped) = (0, 0);
    for c in payload["chats"].as_array().into_iter().flatten() {
        if valid_chat(c) && state.db.save(c).is_ok() {
            chats += 1;
        } else {
            skipped += 1;
        }
    }
    Ok(RestoreReport { ok: true, chats, skipped, settings: payload["settings"].as_object().map_or(0, Map::len), error: None, kept_in: Some(aside.to_string_lossy().into()) })
}

/// At launch (database open, settings loaded): puts a staged restore in place and writes a report the Settings card shows.
/// The staged file (plain text) is deleted before anything else, so it never lingers whatever happens.
pub fn apply_staged(state: &AppState) -> Option<RestoreReport> {
    let path = state.paths.data.join(STAGED);
    let size = std::fs::metadata(&path).ok()?.len() as usize;
    let bytes = if size <= MAX_BYTES { std::fs::read(&path).ok() } else { None };
    let _ = std::fs::remove_file(&path);
    let report = match bytes.and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(|v| v["format"] == FORMAT) {
        Some(payload) if payload["chats"].as_array().is_none_or(|c| c.len() <= MAX_CHATS) => restore(state, &payload).unwrap_or_else(|e| RestoreReport { ok: false, error: Some(e), ..Default::default() }),
        _ => RestoreReport { ok: false, error: Some("the staged backup was missing, too large or damaged; nothing was changed".into()), ..Default::default() },
    };
    if let Some(e) = &report.error {
        log::error!("restore failed: {e}");
    } else {
        log::info!("restored a settings and chats backup: {} chats, {} skipped", report.chats, report.skipped);
    }
    if let Ok(text) = serde_json::to_vec(&report) {
        let _ = std::fs::write(state.paths.data.join(REPORT), text);
    }
    Some(report)
}

/// What the last restore did (once): the Settings card shows it after the restart.
#[tauri::command]
pub fn house_restore_report(state: State<'_, AppState>) -> Option<RestoreReport> {
    let p = state.paths.data.join(REPORT);
    let r = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let _ = std::fs::remove_file(p);
    r
}

#[tauri::command]
pub async fn house_backup_save(state: State<'_, AppState>, path: String, passphrase: String) -> AppResult<usize> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    let settings = serde_json::to_value(&*state.settings.lock().await)?;
    let chats = state.db.export_all()?;
    let count = chats.len();
    let sealed = pack(&build_payload(&settings, chats, chrono::Utc::now().timestamp_millis()), &passphrase)?;
    let target = std::path::PathBuf::from(&path);
    let tmp = target.with_extension("partial");
    std::fs::write(&tmp, sealed).map_err(|e| AppError::msg(format!("Couldn't write the file: {e}")))?;
    std::fs::rename(&tmp, &target)?;
    Ok(count)
}

/// Opens the chosen file with the passphrase, stages it and restarts BYTE to finish.
#[tauri::command]
pub async fn house_backup_restore(app: AppHandle, state: State<'_, AppState>, path: String, passphrase: String) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    let sealed = std::fs::read(&path).map_err(|e| AppError::msg(format!("Couldn't read that file: {e}")))?;
    let payload = unpack(&sealed, &passphrase)?;
    stage(&state.paths.data, &payload)?;
    state.engine.stop().await;
    for e in state.extras.all().await {
        e.stop().await;
    }
    // --- track:found begin ---
    crate::found::crash::clean_exit();
    // --- track:found end ---
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Value {
        json!({
            "theme": "paper", "readAloud": true, "quickKeys": "Control+Alt+Space",
            "cloudAccount": { "email": "someone@example.com", "tier": "pro" }, "cloudBaseUrl": "https://example.invalid", "cloudConnected": true,
            "kidsPin": "1234", "notesDir": "C:\\Users\\me\\Notes", "activeModel": "m:Q4", "tuning": { "m": {} },
            "someApiToken": "abc", "clientSecret": "s", "wifiPassword": "p"
        })
    }

    #[test]
    fn no_secret_and_nothing_of_this_pc_leaves() {
        let out = portable_settings(&settings());
        let o = out.as_object().unwrap();
        assert_eq!(o.len(), 3, "only theme, readAloud and quickKeys: {out}");
        let text = out.to_string();
        for bad in ["someone@example.com", "1234", "abc", "Users", "invalid"] {
            assert!(!text.contains(bad), "{bad} leaked: {text}");
        }
    }

    #[test]
    fn a_round_trip_keeps_chats_and_settings_and_needs_the_passphrase() {
        let chats = vec![json!({ "id": "c1", "title": "Trip", "messages": [{ "id": "m1", "role": "user", "content": "hello" }] })];
        let payload = build_payload(&settings(), chats.clone(), 5);
        let sealed = pack(&payload, "correct horse").unwrap();
        assert!(!sealed.windows(5).any(|w| w == b"Trip"), "sealed: nothing readable");
        let back = unpack(&sealed, "correct horse").unwrap();
        assert_eq!(back["chats"], json!(chats));
        assert_eq!(back["settings"]["theme"], "paper");
        assert!(unpack(&sealed, "wrong horse!!").is_err());
        assert!(pack(&payload, "short").is_err());
    }

    #[test]
    fn other_files_are_refused_kindly() {
        let other = crate::backup::seal(b"PK\x03\x04 not json", "correct horse").unwrap();
        assert!(unpack(&other, "correct horse").unwrap_err().to_string().contains("full BYTE backup"));
        let wrong = crate::backup::seal(br#"{"format":"x","version":1}"#, "correct horse").unwrap();
        assert!(unpack(&wrong, "correct horse").is_err());
        let newer = crate::backup::seal(br#"{"format":"byte-portable","version":99}"#, "correct horse").unwrap();
        assert!(unpack(&newer, "correct horse").unwrap_err().to_string().contains("newer"));
    }

    #[test]
    fn restoring_lays_settings_over_the_current_ones_and_keeps_what_is_local() {
        let current = json!({ "theme": "midnight", "cloudConnected": true, "cloudAccount": { "tier": "pro" }, "notesDir": "D:\\Notes", "kidsPin": "9999" });
        let restored = json!({ "theme": "paper", "cloudConnected": false, "notesDir": "C:\\old", "kidsPin": "0000", "readAloud": true });
        let m = merge_settings(&current, &restored);
        assert_eq!(m["theme"], "paper");
        assert_eq!(m["readAloud"], true);
        assert_eq!(m["cloudConnected"], true);
        assert_eq!(m["notesDir"], "D:\\Notes");
        assert_eq!(m["kidsPin"], "9999");
        assert_eq!(m["cloudAccount"]["tier"], "pro");
    }

    #[test]
    fn only_plain_chats_with_ids_are_accepted() {
        assert!(valid_chat(&json!({ "id": "c-1", "messages": [{ "id": "m1", "content": "hi" }] })));
        assert!(valid_chat(&json!({ "id": "c1" })));
        for bad in [json!({}), json!({ "id": "" }), json!({ "id": "../x" }), json!({ "id": "a".repeat(101) }), json!({ "id": "c", "messages": [{ "content": "no id" }] }), json!({ "id": "c", "messages": "x" })] {
            assert!(!valid_chat(&bad), "{bad}");
        }
    }

    #[test]
    fn settings_that_cannot_be_applied_abort_before_anything_changes() {
        let current = serde_json::to_value(crate::settings::Settings::default()).unwrap();
        assert!(merged_settings(&current, &json!({ "settings": { "theme": "paper" } })).is_ok());
        let err = merged_settings(&current, &json!({ "settings": { "fontScale": "huge" } })).unwrap_err();
        assert!(err.contains("could not be applied"), "{err}");
    }

    #[test]
    fn a_huge_payload_is_not_staged() {
        let dir = tempfile::tempdir().unwrap();
        let many: Vec<Value> = (0..20_001).map(|i| json!({ "id": format!("c{i}") })).collect();
        assert!(stage(dir.path(), &build_payload(&json!({}), many, 1)).is_err());
        assert!(!dir.path().join(STAGED).exists());
    }

    #[test]
    fn staging_writes_one_file_and_counts() {
        let dir = tempfile::tempdir().unwrap();
        let payload = build_payload(&settings(), vec![json!({"id":"a"}), json!({"id":"b"})], 1);
        assert_eq!(stage(dir.path(), &payload).unwrap(), (2, 3));
        assert!(dir.path().join(STAGED).exists());
    }
}
