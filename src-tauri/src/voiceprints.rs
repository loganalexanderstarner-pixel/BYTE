//! "Teach BYTE your voice" (docs/HEY-BYTE-ASSISTANT.md): a few people can each teach BYTE a name and a few spoken sentences, like an
//! Alexa's voice profiles. Then each spoken turn ("Hey BYTE", Talk mode) is checked against them in parallel with being turned
//! into text, so BYTE knows who is talking, uses their name, and (if the person wants a lock) only does things for a voice it knows.
//!
//! The recognizing happens on the cluster (byte-ai's `/api/voiceprints`, the same speaker model the app uses to label speakers in
//! recordings), so it needs a connected BYTE Cloud account; the recordings are sent for that one check and never kept, only a
//! 192-number voiceprint per person is, and it can be listed and deleted here. Without a cloud account nothing here does anything.
//!
//! The lock (Settings, "Who can ask for things"): `off` (nobody is turned away), `actions` (a voice BYTE doesn't know can ask questions
//! but can't make BYTE do things on the PC: timers, files, volume, music, uninstalling...), `all` (only known voices at all).
//! "Known" for a lock is stricter than for a greeting: a similarity of 0.70, not 0.50, because a similar voice (a sibling) can reach 0.6.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::cloud::CloudClient;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// The similarity a lock needs; just recognizing someone (the greeting, their name) needs what the cluster calls a match (0.50).
pub const STRICT: f32 = 0.70;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: i64,
    pub name: String,
    pub samples: i64,
}

/// Who spoke, as worked out for one spoken turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Speaker {
    /// The check ran (false: no cloud, a problem, or nobody has taught BYTE a voice).
    pub checked: bool,
    /// The person's name when BYTE is reasonably sure.
    pub name: Option<String>,
    /// The closest taught voice and how close (0 to 1), sure or not.
    pub best: Option<String>,
    pub score: f32,
    /// How many voices BYTE has been taught.
    pub taught: u32,
}

impl Speaker {
    /// Sure enough for a lock.
    pub fn sure(&self) -> bool {
        self.checked && self.name.is_some() && self.score >= STRICT
    }
}

pub fn parse_voices(v: &Value) -> Vec<Voice> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| Some(Voice { id: x.get("id")?.as_i64()?, name: x.get("name")?.as_str()?.to_string(), samples: x.get("samples").and_then(Value::as_i64).unwrap_or(1) }))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_identity(v: &Value) -> Speaker {
    Speaker {
        checked: true,
        name: v.get("name").and_then(Value::as_str).map(str::to_string),
        best: v.get("best").and_then(Value::as_str).map(str::to_string),
        score: v.get("score").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        taught: v.get("taught").and_then(Value::as_u64).unwrap_or(0) as u32,
    }
}

/// Why a spoken request is turned away, if it is. `is_action`: it asks BYTE to do something on this PC.
pub fn refusal(lock: &str, spoken: bool, speaker: Option<&Speaker>, is_action: bool) -> Option<String> {
    if !spoken || !matches!(lock, "actions" | "all") {
        return None;
    }
    // A lock with nobody taught locks nobody out.
    if speaker.is_some_and(|s| s.checked && s.taught == 0) {
        return None;
    }
    if speaker.is_some_and(Speaker::sure) {
        return None;
    }
    let checked = speaker.is_some_and(|s| s.checked);
    match (lock, is_action) {
        ("all", _) => Some(if checked { "I don't recognize that voice, so I'll leave it. You can teach me a voice in Settings.".into() } else { "I couldn't check whose voice that was just now, so I'll leave it.".into() }),
        ("actions", true) => Some(if checked { "I don't recognize your voice, so I won't do that one. I can still answer questions.".into() } else { "I couldn't check whose voice that was just now, so I won't do that one. I can still answer questions.".into() }),
        _ => None,
    }
}

pub fn b64(wav: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(wav)
}

pub async fn list(client: &CloudClient) -> crate::cloud::CloudResult<Vec<Voice>> {
    Ok(parse_voices(&client.get("/api/voiceprints").await?))
}

pub async fn teach(client: &CloudClient, name: &str, wavs_base64: &[String]) -> crate::cloud::CloudResult<Value> {
    client.post("/api/voiceprints", &json!({ "name": name, "audio": wavs_base64 })).await
}

pub async fn forget(client: &CloudClient, id: i64) -> crate::cloud::CloudResult<()> {
    client.delete(&format!("/api/voiceprints/{id}")).await.map(|_| ())
}

pub async fn identify(client: &CloudClient, wav_base64: &str) -> crate::cloud::CloudResult<Speaker> {
    Ok(parse_identity(&client.post("/api/voiceprints/identify", &json!({ "audio": wav_base64 })).await?))
}

// ------------------------------------------------------------------ commands

fn nope() -> AppError {
    AppError::msg("Teaching BYTE your voice needs BYTE Cloud. Connect it in Settings → Cloud first.")
}

#[tauri::command]
pub async fn voiceprints_list(state: State<'_, AppState>) -> AppResult<Vec<Voice>> {
    let Ok(client) = state.cloud_client().await else { return Ok(vec![]) };
    Ok(list(&client).await.unwrap_or_default())
}

/// Teaches BYTE a voice: a name and a few recordings (WAV, base64). Adding to a name already taught sharpens it.
#[tauri::command]
pub async fn voiceprint_teach(state: State<'_, AppState>, name: String, wavs_base64: Vec<String>) -> AppResult<Value> {
    let client = state.cloud_client().await.map_err(|_| nope())?;
    teach(&client, name.trim(), &wavs_base64).await.map_err(AppError::from)
}

#[tauri::command]
pub async fn voiceprint_forget(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let client = state.cloud_client().await.map_err(|_| nope())?;
    forget(&client, id).await.map_err(AppError::from)
}

/// Whose voice is in this recording (WAV, base64)? Never an error the person sees: a failed check is "not checked".
#[tauri::command]
pub async fn voice_identify(state: State<'_, AppState>, wav_base64: String) -> AppResult<Speaker> {
    let Ok(client) = state.cloud_client().await else { return Ok(Speaker::default()) };
    Ok(identify(&client, &wav_base64).await.unwrap_or_default())
}

#[cfg(test)]
#[path = "voiceprints_tests.rs"]
mod tests;
