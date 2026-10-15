//! The account profile, app side (infra: byte-ai `profile.py`). The settings that follow a person (how BYTE talks, the swearing switch,
//! who may ask for things, their name and town, their voice choices, what Auto has learned) are kept on their BYTE Cloud account too, so
//! the same person on a PC, a Mac, the phone and the web app gets the same BYTE, and nothing is lost on a reinstall or a new device.
//!
//! Each of these settings has a time it was last changed *on a device* (`profile_clock.json`). A change is sent to the account with that
//! time; the account keeps the newer of two values per setting, so devices that change different things never overwrite each other and
//! a device that was off for a month cannot undo what a newer one set. What came back newer than this device's own value is applied here.
//! Settings tied to one device (the wake word switch, hotkeys, downloaded models and voices, which Mac voice) are not in the list.
//! Without a connected BYTE Cloud account none of this does anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// (key on the account, key in this app's settings). Everything else stays on the device.
pub const KEYS: &[(&str, &str)] = &[
    ("personality", "personality"),
    ("talk_mode", "talkMode"),
    ("allow_swearing", "allowSwearing"),
    ("voice_lock", "voiceLock"),
    ("hey_byte_engine", "heyByteEngine"),
    ("voice_where", "voiceWhere"),
    ("cloud_voice", "cloudVoice"),
    ("speech_style", "speechStyle"),
    ("speech_speed", "speechSpeed"),
    ("user_name", "userName"),
    ("home_place", "homePlace"),
];
/// What Auto has learned (style.rs): not a setting, a file, kept the same way. The newest side wins as a whole.
pub const LEARNED: &str = "style_learned";

const CLOCK_FILE: &str = "profile_clock.json";

/// When each synced key was last changed on this device (ms since 1970).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Clock(pub BTreeMap<String, i64>);

pub fn load_clock(path: &Path) -> Clock {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_clock(path: &Path, c: &Clock) {
    let tmp = path.with_extension("json.tmp");
    let ok = serde_json::to_string(c).map_err(std::io::Error::other).and_then(|s| std::fs::write(&tmp, s)).and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = ok {
        log::warn!("profile clock: could not save: {e}");
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Which synced keys differ between two versions of the settings.
pub fn changed_keys(old: &Value, new: &Value) -> Vec<&'static str> {
    KEYS.iter().filter(|(_, s)| old.get(*s) != new.get(*s)).map(|(k, _)| *k).collect()
}

/// What to send to the account: every synced key this device has ever changed, with its time.
pub fn outgoing(settings: &Value, clock: &Clock, learned: Option<&Value>) -> Value {
    let mut out = Map::new();
    for (key, setting) in KEYS {
        if let (Some(t), Some(v)) = (clock.0.get(*key), settings.get(*setting)) {
            if *t > 0 {
                out.insert((*key).to_string(), json!({ "v": v, "t": t }));
            }
        }
    }
    if let (Some(t), Some(v)) = (clock.0.get(LEARNED), learned) {
        if *t > 0 {
            out.insert(LEARNED.to_string(), json!({ "v": v, "t": t }));
        }
    }
    Value::Object(out)
}

/// What came back from the account that is newer than this device's own: a settings patch, the learned style if newer, and the new clock.
pub fn incoming(remote: &Value, settings: &Value, clock: &Clock) -> (Value, Option<Value>, Clock) {
    let mut patch = Map::new();
    let mut learned = None;
    let mut next = clock.clone();
    let Some(obj) = remote.as_object() else { return (Value::Object(patch), learned, next) };
    for (key, entry) in obj {
        let (Some(v), Some(t)) = (entry.get("v"), entry.get("t").and_then(Value::as_i64)) else { continue };
        if t <= clock.0.get(key).copied().unwrap_or(0) {
            continue;
        }
        if key == LEARNED {
            learned = Some(v.clone());
            next.0.insert(key.clone(), t);
        } else if let Some((_, setting)) = KEYS.iter().find(|(k, _)| k == key) {
            if settings.get(*setting) != Some(v) {
                patch.insert((*setting).to_string(), v.clone());
            }
            next.0.insert(key.clone(), t);
        }
    }
    (Value::Object(patch), learned, next)
}

fn clock_path(state: &AppState) -> PathBuf {
    state.paths.data.join(CLOCK_FILE)
}

fn learned_path(state: &AppState) -> PathBuf {
    state.paths.data.join("style.json")
}

/// Stamps keys as changed now, on this device. (Called when the person changes a setting, and when Auto learns.)
pub fn stamp(state: &AppState, keys: &[&str]) {
    if keys.is_empty() {
        return;
    }
    let path = clock_path(state);
    let mut c = load_clock(&path);
    let t = now_ms();
    for k in keys {
        c.0.insert((*k).to_string(), t);
    }
    save_clock(&path, &c);
}

static PENDING: AtomicBool = AtomicBool::new(false);

/// Sends changes soon (a few seconds later, once, however many settings changed in between).
pub fn push_soon(app: &tauri::AppHandle) {
    use tauri::Manager;
    if PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        PENDING.store(false, Ordering::SeqCst);
        let state = app.state::<AppState>();
        if let Err(e) = run(&state).await {
            log::info!("profile sync: {e}");
        }
    });
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub pushed: usize,
    pub pulled: usize,
}

/// One round: send this device's changes, bring back what is newer. Does nothing (and is not an error) without a BYTE Cloud account.
pub async fn run(state: &AppState) -> AppResult<Report> {
    let Ok(client) = state.cloud_client().await else { return Ok(Report::default()) };
    let kids = state.settings.lock().await.kids_mode;
    if kids {
        return Ok(Report::default());
    }
    let settings = serde_json::to_value(&*state.settings.lock().await)?;
    let cpath = clock_path(state);
    let clock = load_clock(&cpath);
    let learned = std::fs::read_to_string(learned_path(state)).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let changes = outgoing(&settings, &clock, learned.as_ref());
    let pushed = changes.as_object().map(Map::len).unwrap_or(0);
    let reply = client.post("/api/profile/merge", &json!({ "changes": changes })).await.map_err(AppError::from)?;
    let (patch, learned_in, next) = incoming(reply.get("profile").unwrap_or(&Value::Null), &settings, &clock);
    let mut pulled = 0;
    if patch.as_object().is_some_and(|p| !p.is_empty()) {
        pulled += patch.as_object().map(Map::len).unwrap_or(0);
        let mut s = state.settings.lock().await;
        // `merged` checks every value, so a bad one from another device is refused rather than stored.
        let applied = s.merged(patch)?;
        applied.save(&state.paths.settings_file)?;
        *s = applied;
    }
    if let Some(l) = learned_in {
        if serde_json::from_value::<crate::style::Learned>(l.clone()).is_ok() {
            std::fs::write(learned_path(state), l.to_string())?;
            pulled += 1;
        }
    }
    save_clock(&cpath, &next);
    Ok(Report { pushed, pulled })
}

/// Brings this device up to date with the account, and sends its own changes.
#[tauri::command]
pub async fn profile_sync(state: tauri::State<'_, AppState>) -> AppResult<Report> {
    run(&state).await
}

#[cfg(test)]
#[path = "profile_sync_tests.rs"]
mod tests;
