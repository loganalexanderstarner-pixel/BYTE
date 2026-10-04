//! Tauri commands for cloud mode, and the cloud side of `chat_send`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::State;

use super::{follow, parse_me, posted_ids, CloudClient, CloudError, CloudMe, STREAMING_ACTIONS};
use crate::chat::ChatEvent;
use crate::commands::ChatRequest;
use crate::error::{AppError, AppResult};
use crate::settings::Mode;
use crate::state::AppState;

/// The cloud part of a chat request.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTurn {
    /// The chat's conversation on the cloud; `None` starts one.
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// One of the account's mode ids (`me.modes`).
    pub mode: String,
    /// Newest message of this chat already on the cloud (to follow new rows after it).
    #[serde(default)]
    pub last_remote_id: Option<String>,
    /// Photos/files uploaded to the cloud for this message.
    #[serde(default)]
    pub attachment_ids: Vec<Value>,
    /// Edited message: fork the cloud conversation after this message first.
    #[serde(default)]
    pub branch_from: Option<String>,
    /// Don't answer on this Mac when the cloud is down (Both: this Mac is already answering beside it).
    #[serde(default)]
    pub no_fallback: bool,
}

/// Closest mode on this Mac when the cloud is down.
pub fn local_mode(cloud_mode: &str) -> Mode {
    match cloud_mode {
        "fast" => Mode::Fast,
        "extended" => Mode::Deep,
        "extended_plus" => Mode::Extended,
        _ => Mode::Auto,
    }
}

fn title_from(text: &str) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > 60 {
        format!("{}…", t.chars().take(59).collect::<String>())
    } else {
        t
    }
}

/// Sends the newest user message to the cloud and streams the answer.
/// `Unreachable` is only returned before the cloud accepted the message, so
/// the caller can safely answer locally instead.
pub async fn send(state: &AppState, request: &ChatRequest, turn: &CloudTurn, on_event: &Channel<ChatEvent>) -> Result<(), CloudError> {
    let client = state.cloud_client().await.map_err(CloudError::Other)?;
    let content = request.messages.iter().rev().find(|m| m.role == "user").map(|m| m.content.clone()).unwrap_or_default();
    let mut cid = match &turn.conversation_id {
        Some(c) if !c.is_empty() => c.clone(),
        _ => client.create_conversation(&title_from(&content)).await?,
    };
    if let Some(from) = turn.branch_from.as_deref().filter(|f| !f.is_empty()) {
        let v = client.post(&format!("/api/conversations/{cid}/branch"), &json!({ "message_id": from })).await?;
        cid = v.get("conversation_id").and_then(super::text).or_else(|| super::id_of(&v)).unwrap_or(cid);
    }
    let _ = on_event.send(ChatEvent::Remote { conversation_id: cid.clone(), message_id: None, user_message_id: None });
    let body = json!({ "content": content, "attachment_ids": turn.attachment_ids, "mode": turn.mode });
    let posted = client.post(&format!("/api/conversations/{cid}/messages"), &body).await?;
    let (user_id, assistant_id) = posted_ids(&posted);
    let _ = on_event.send(ChatEvent::Remote { conversation_id: cid.clone(), message_id: assistant_id.clone(), user_message_id: user_id.clone() });
    let _ = on_event.send(ChatEvent::Started { thinking: false, model: "BYTE Cloud".into() });
    // From here the cloud has the message: errors are reported, never answered twice.
    stream(state, &client, &cid, request.request_id.as_str(), user_id.or_else(|| turn.last_remote_id.clone()), assistant_id, on_event)
        .await
        .map_err(|e| match e {
            CloudError::Unreachable(m) => CloudError::Other(AppError::msg(format!("Lost the connection to the BYTE cloud ({m}). The answer may still finish there; reopen the chat to see it."))),
            other => other,
        })
}

async fn stream(
    state: &AppState,
    client: &CloudClient,
    cid: &str,
    request_id: &str,
    since: Option<String>,
    assistant: Option<String>,
    on_event: &Channel<ChatEvent>,
) -> Result<(), CloudError> {
    let cancel = state.generations.register(request_id).await;
    let end = follow(client, cid, since, assistant, &cancel, on_event).await;
    state.generations.finish(request_id).await;
    let end = end?;
    let _ = on_event.send(ChatEvent::Remote { conversation_id: cid.into(), message_id: end.assistant_id, user_message_id: None });
    let _ = on_event.send(ChatEvent::Done { finish_reason: end.finish });
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStatus {
    pub connected: bool,
    pub base_url: String,
    pub account: Option<CloudMe>,
}

async fn status(state: &AppState) -> CloudStatus {
    let s = state.settings.lock().await;
    CloudStatus {
        connected: s.cloud_connected,
        base_url: s.cloud_base_url.clone().unwrap_or_else(|| super::DEFAULT_BASE.into()),
        account: s.cloud_account.as_ref().map(parse_me),
    }
}

async fn save_settings(state: &AppState, f: impl FnOnce(&mut crate::settings::Settings)) -> AppResult<()> {
    let mut s = state.settings.lock().await;
    let mut next = s.clone();
    f(&mut next);
    next.save(&state.paths.settings_file)?;
    *s = next;
    Ok(())
}

#[tauri::command]
pub async fn cloud_status(state: State<'_, AppState>) -> AppResult<CloudStatus> {
    Ok(status(&state).await)
}

/// Checks a pasted key with `GET /api/auth/me` and, only if the cloud accepts
/// it, saves it in the Keychain.
#[tauri::command]
pub async fn cloud_connect(state: State<'_, AppState>, key: String, base_url: Option<String>) -> AppResult<CloudStatus> {
    connect(&state, &key, base_url).await?;
    Ok(status(&state).await)
}

pub async fn connect(state: &AppState, key: &str, base_url: Option<String>) -> AppResult<Value> {
    let key = key.trim();
    if !key.starts_with("byte_") || key.len() < 20 || key.contains(char::is_whitespace) {
        return Err(AppError::msg("That doesn't look like a BYTE key (it starts with byte_). Create one at byteai.bytebylogan.xyz → Settings → API keys."));
    }
    let base = base_url.filter(|b| !b.trim().is_empty()).map(|b| b.trim().trim_end_matches('/').to_string());
    if let Some(b) = &base {
        if !b.starts_with("https://") && !b.starts_with("http://127.0.0.1") && !b.starts_with("http://localhost") {
            return Err(AppError::msg("The cloud address must start with https://"));
        }
    }
    let client = CloudClient::new(base.as_deref().unwrap_or(super::DEFAULT_BASE), key);
    let raw = match client.get("/api/auth/me").await {
        Ok(v) => v,
        Err(CloudError::Unauthorized) => return Err(AppError::msg("The BYTE cloud didn't accept that key. Check it was copied completely, or create a new one.")),
        Err(e) => return Err(e.into()),
    };
    state.secrets.set(&state.cloud_account(), key)?;
    *state.cloud_key.lock().await = Some(key.to_string());
    let me = parse_me(&raw);
    save_settings(state, |s| {
        s.cloud_connected = true;
        s.cloud_base_url = base;
        s.cloud_account = Some(raw.clone());
        if !s.cloud_mode.as_ref().is_some_and(|m| me.modes.iter().any(|x| &x.id == m)) {
            s.cloud_mode = me.modes.iter().find(|m| m.id == "auto").or(me.modes.first()).map(|m| m.id.clone());
        }
    })
    .await?;
    Ok(raw)
}

/// Removes the key from the Keychain and stops using the cloud.
#[tauri::command]
pub async fn cloud_disconnect(state: State<'_, AppState>) -> AppResult<CloudStatus> {
    state.secrets.delete(&state.cloud_account())?;
    *state.cloud_key.lock().await = None;
    save_settings(&state, |s| {
        s.cloud_connected = false;
        s.cloud_account = None;
        s.use_cloud = false;
        s.workspace = "local".into();
    })
    .await?;
    Ok(status(&state).await)
}

/// Re-reads the account (tier, modes, budgets). A revoked key disconnects.
#[tauri::command]
pub async fn cloud_refresh(state: State<'_, AppState>) -> AppResult<CloudStatus> {
    let client = state.cloud_client().await?;
    match client.get("/api/auth/me").await {
        Ok(raw) => {
            let me = parse_me(&raw);
            save_settings(&state, |s| {
                s.cloud_account = Some(raw);
                if !s.cloud_mode.as_ref().is_some_and(|m| me.modes.iter().any(|x| &x.id == m)) {
                    s.cloud_mode = me.modes.first().map(|m| m.id.clone());
                }
            })
            .await?;
        }
        Err(CloudError::Unauthorized) => {
            state.secrets.delete(&state.cloud_account())?;
            *state.cloud_key.lock().await = None;
            save_settings(&state, |s| {
                s.cloud_connected = false;
                s.use_cloud = false;
                s.workspace = "local".into();
            })
            .await?;
            return Err(CloudError::Unauthorized.into());
        }
        Err(e) => return Err(e.into()),
    }
    Ok(status(&state).await)
}

/// Runs an action on a cloud message (regenerate, deepen, justify, stop,
/// answer-now, feedback). Actions that write a new answer stream it on
/// `on_event` like a normal turn.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRequest {
    pub request_id: String,
    pub conversation_id: String,
    pub message_id: String,
    pub action: String,
    /// Message after which the new answer appears.
    #[serde(default)]
    pub since: Option<String>,
    /// Feedback value ("up" / "down").
    #[serde(default)]
    pub value: Option<String>,
}

#[tauri::command]
pub async fn cloud_action(state: State<'_, AppState>, request: ActionRequest, on_event: Channel<ChatEvent>) -> AppResult<()> {
    if !super::ACTIONS.contains(&request.action.as_str()) {
        return Err(AppError::msg("unknown action"));
    }
    let client = state.cloud_client().await?;
    let id = &request.message_id;
    let body = match request.action.as_str() {
        "feedback" => json!({ "value": request.value.clone().unwrap_or_default(), "rating": if request.value.as_deref() == Some("down") { -1 } else { 1 } }),
        _ => json!({}),
    };
    let posted = client.post(&format!("/api/messages/{id}/{}", request.action), &body).await?;
    if STREAMING_ACTIONS.contains(&request.action.as_str()) {
        let (_, new_assistant) = posted_ids(&posted);
        let assistant = new_assistant.filter(|n| n != id);
        let _ = on_event.send(ChatEvent::Started { thinking: false, model: "BYTE Cloud".into() });
        stream(&state, &client, &request.conversation_id, &request.request_id, request.since.clone(), assistant, &on_event).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn cloud_delete_message(state: State<'_, AppState>, message_id: String) -> AppResult<()> {
    state.cloud_client().await?.delete(&format!("/api/messages/{message_id}")).await?;
    Ok(())
}

/// The account's conversations on the cloud (for importing into the sidebar).
#[tauri::command]
pub async fn cloud_conversations(state: State<'_, AppState>) -> AppResult<Value> {
    Ok(state.cloud_client().await?.get("/api/conversations").await?)
}

/// Copies a cloud conversation into this Mac's chat list (or refreshes the
/// copy) and returns the local chat id.
#[tauri::command]
pub async fn cloud_import(state: State<'_, AppState>, conversation_id: String) -> AppResult<String> {
    let v = state.cloud_client().await?.get(&format!("/api/conversations/{conversation_id}")).await?;
    let existing = state.db.list()?.into_iter().find(|c| c.cloud_id.as_deref() == Some(conversation_id.as_str())).map(|c| c.id);
    let chat = to_local_chat(&v, &conversation_id, existing);
    state.db.save(&chat)?;
    Ok(chat["id"].as_str().unwrap_or_default().to_string())
}

fn time_ms(v: &Value) -> Option<i64> {
    let s = v.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.timestamp_millis())
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").map(|d| d.and_utc().timestamp_millis()))
        .ok()
}

/// A cloud conversation in the app's chat shape.
pub fn to_local_chat(v: &Value, cid: &str, local_id: Option<String>) -> Value {
    let now = chrono::Utc::now().timestamp_millis();
    let rows = v.get("messages").and_then(Value::as_array).cloned().unwrap_or_default();
    let messages: Vec<Value> = rows
        .iter()
        .filter(|m| matches!(m.get("role").and_then(Value::as_str), Some("user" | "assistant")))
        .map(|m| {
            let rid = super::id_of(m).unwrap_or_default();
            let mut out = json!({
                "id": format!("cloud-{rid}"),
                "remoteId": rid,
                "role": m["role"],
                "content": m.get("content").and_then(Value::as_str).unwrap_or(""),
                "createdAt": m.get("created_at").and_then(time_ms).unwrap_or(now),
                "cloud": true,
            });
            let src = super::sources_of(m);
            if !src.is_empty() {
                out["sources"] = serde_json::to_value(src).unwrap_or(Value::Null);
            }
            out
        })
        .collect();
    json!({
        "id": local_id.unwrap_or_else(|| format!("cloud-{cid}")),
        "title": v.get("title").and_then(super::text).unwrap_or_else(|| "Cloud chat".into()),
        "createdAt": v.get("created_at").and_then(time_ms).unwrap_or(now),
        "updatedAt": v.get("updated_at").and_then(time_ms).unwrap_or(now),
        "cloudId": cid,
        "messages": messages,
    })
}

// ---------- general account calls (documents, library, memories, …) ----------

/// Only the cloud's own API paths, so the UI can't be pointed anywhere else.
pub(crate) fn api_path(path: &str) -> AppResult<&str> {
    let ok = path.starts_with("/api/")
        && !path.contains("..")
        && !path.contains("://")
        && !path.chars().any(|c| c.is_whitespace() || c.is_control() || c == '#' || c == '\\');
    ok.then_some(path).ok_or_else(|| AppError::msg("not a BYTE cloud API path"))
}

#[tauri::command]
pub async fn cloud_get(state: State<'_, AppState>, path: String) -> AppResult<Value> {
    Ok(state.cloud_client().await?.get(api_path(&path)?).await?)
}

#[tauri::command]
pub async fn cloud_post(state: State<'_, AppState>, path: String, body: Option<Value>) -> AppResult<Value> {
    Ok(state.cloud_client().await?.post(api_path(&path)?, &body.unwrap_or_else(|| json!({}))).await?)
}

#[tauri::command]
pub async fn cloud_delete(state: State<'_, AppState>, path: String) -> AppResult<Value> {
    Ok(state.cloud_client().await?.delete(api_path(&path)?).await?)
}

/// An image from the cloud (attachment, template thumbnail, document page) as
/// a `data:` URL the page can show.
#[tauri::command]
pub async fn cloud_image(state: State<'_, AppState>, path: String) -> AppResult<String> {
    let (bytes, mime) = state.cloud_client().await?.bytes(api_path(&path)?).await?;
    use base64::Engine as _;
    let mime = if mime.starts_with("image/") { mime } else { "image/png".into() };
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Saves a cloud file (a finished document, an export) where the user chose.
#[tauri::command]
pub async fn cloud_download(state: State<'_, AppState>, path: String, dest: String) -> AppResult<u64> {
    let (bytes, _) = state.cloud_client().await?.bytes(api_path(&path)?).await?;
    let dest = std::path::PathBuf::from(dest);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&dest, &bytes)?;
    Ok(bytes.len() as u64)
}

/// Largest file sent to the cloud in one upload.
const MAX_UPLOAD: u64 = 50 * 1024 * 1024;

/// Uploads a file the user picked (multipart field `file`) to `path`.
#[tauri::command]
pub async fn cloud_upload(state: State<'_, AppState>, path: String, file: String) -> AppResult<Value> {
    let client = state.cloud_client().await?;
    Ok(client.upload(api_path(&path)?, std::path::Path::new(&file), MAX_UPLOAD).await?)
}

/// Uploads a photo or file for a chat. Starts the cloud conversation if the
/// chat doesn't have one yet (attachments belong to a conversation).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Uploaded {
    pub conversation_id: String,
    pub attachment: Value,
}

#[tauri::command]
pub async fn cloud_attach(state: State<'_, AppState>, conversation_id: Option<String>, title: String, file: String) -> AppResult<Uploaded> {
    let client = state.cloud_client().await?;
    let cid = match conversation_id.filter(|c| !c.is_empty()) {
        Some(c) => c,
        None => client.create_conversation(&title_from(&title)).await?,
    };
    let attachment = client.upload(&format!("/api/conversations/{cid}/attachments"), std::path::Path::new(&file), MAX_UPLOAD).await?;
    Ok(Uploaded { conversation_id: cid, attachment })
}
