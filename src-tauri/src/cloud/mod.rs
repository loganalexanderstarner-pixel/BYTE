//! Cloud mode: the owner's BYTE cluster as a remote backend, reached over the
//! public internet (contract: docs/CLOUD-MODE.md).
//!
//! - The API key authenticates as its owner and lives only in the macOS
//!   Keychain (`keychain.rs`). It's checked with `GET /api/auth/me` before
//!   it's saved.
//! - Modes come from the account (`me.modes`), never a hard-coded list.
//! - Answers stream as `delta` events (appended text only); `phase` and
//!   `sources` arrive while the answer is written.
//! - A slow first token is queueing, not an error: nothing is retried into the
//!   queue. An unreachable cluster makes BYTE answer locally instead.

pub mod cmd;
pub mod keychain;
pub mod sse;

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::chat::{ChatEvent, Stats};
use crate::error::AppError;

pub const DEFAULT_BASE: &str = "https://byteai.bytebylogan.xyz";

/// Why a cloud call failed, so callers can fall back when the cluster is down.
#[derive(Debug)]
pub enum CloudError {
    /// Can't reach the cluster (offline, DNS, refused, gateway errors).
    Unreachable(String),
    /// The key was rejected (401/403).
    Unauthorized,
    /// 429: a daily allowance or a rate limit is spent (the message says which).
    Limited(String),
    Other(AppError),
}

impl From<CloudError> for AppError {
    fn from(e: CloudError) -> Self {
        match e {
            CloudError::Unreachable(m) => AppError::msg(format!("The BYTE cloud can't be reached right now ({m}).")),
            CloudError::Unauthorized => {
                AppError::msg("The BYTE cloud didn't accept your key. It may have been revoked; paste a new one in Settings → Cloud.")
            }
            CloudError::Limited(m) => AppError::msg(m),
            CloudError::Other(e) => e,
        }
    }
}

type CloudResult<T> = Result<T, CloudError>;

/// A 429 means one of the account's daily allowances is spent: not a bug, so say so plainly.
fn limit_error(detail: Option<String>) -> CloudError {
    let why = detail.filter(|d| !d.trim().is_empty()).map(|d| format!(" ({})", d.chars().take(200).collect::<String>())).unwrap_or_default();
    CloudError::Limited(format!(
        "Today's allowance for this on your BYTE cloud is used up{why}. It resets tomorrow; until then try a lighter mode, or answer on this Mac."
    ))
}

fn net_error(e: reqwest::Error) -> CloudError {
    if e.is_connect() || e.is_timeout() || e.is_request() {
        CloudError::Unreachable(e.to_string())
    } else {
        CloudError::Other(e.into())
    }
}

#[derive(Clone)]
pub struct CloudClient {
    http: reqwest::Client,
    base: String,
    key: String,
}

/// One mode offered to this account, in the order the server lists them.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudMode {
    pub id: String,
    pub label: String,
}

/// The account behind a key (`GET /api/auth/me`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudMe {
    pub name: Option<String>,
    pub email: Option<String>,
    pub tier: Option<String>,
    pub modes: Vec<CloudMode>,
    /// Limits and what's left of them, shown as the server sends them.
    pub budgets: Value,
}

fn mode_label(id: &str) -> String {
    match id {
        "fast" => "Fast".into(),
        "auto" => "Auto".into(),
        "extended" => "Extended".into(),
        "extended_plus" => "Extended+".into(),
        other => {
            let s = other.replace('_', " ");
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

pub fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(o) => o.get("name").or_else(|| o.get("id")).and_then(text),
        _ => None,
    }
}

/// Reads the account without assuming more than the contract promises.
pub fn parse_me(v: &Value) -> CloudMe {
    let user = v.get("user").unwrap_or(v);
    let pick = |keys: &[&str]| keys.iter().find_map(|k| v.get(*k).or_else(|| user.get(*k)).and_then(text));
    let modes = v
        .get("modes")
        .or_else(|| user.get("modes"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let id = match m {
                        Value::String(s) => s.clone(),
                        Value::Object(o) => o.get("id").or_else(|| o.get("mode")).or_else(|| o.get("name")).and_then(text)?,
                        _ => return None,
                    };
                    let label = m.get("label").and_then(text).unwrap_or_else(|| mode_label(&id));
                    Some(CloudMode { id, label })
                })
                .collect()
        })
        .unwrap_or_default();
    CloudMe {
        name: pick(&["name", "display_name", "username"]),
        email: pick(&["email"]),
        tier: pick(&["tier", "plan"]),
        modes,
        budgets: ["budgets", "budget", "quota", "limits"].iter().find_map(|k| v.get(*k).cloned()).unwrap_or(Value::Null),
    }
}

/// Plain id of an object the server returned (`{"id": 12}` or `"12"`).
pub fn id_of(v: &Value) -> Option<String> {
    v.get("id").and_then(text).or_else(|| text(v))
}

impl CloudClient {
    pub fn new(base: &str, key: &str) -> Self {
        Self::with_http(http_client(), base, key)
    }

    /// Uses a shared HTTP client, so its open connection to the cloud (and
    /// the TLS handshake) is reused across messages instead of redone each time.
    pub fn with_http(http: reqwest::Client, base: &str, key: &str) -> Self {
        CloudClient { http, base: base.trim_end_matches('/').to_string(), key: key.trim().to_string() }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, self.url(path)).bearer_auth(&self.key)
    }

    async fn send(&self, rb: reqwest::RequestBuilder, timeout: Duration) -> CloudResult<Value> {
        let r = rb.timeout(timeout).send().await.map_err(net_error)?;
        let status = r.status();
        if status == 401 || status == 403 {
            return Err(CloudError::Unauthorized);
        }
        if matches!(status.as_u16(), 502..=504) {
            return Err(CloudError::Unreachable(format!("HTTP {status}")));
        }
        let body = r.text().await.map_err(net_error)?;
        if status == 429 {
            return Err(limit_error(serde_json::from_str::<Value>(&body).ok().and_then(|v| v.get("detail").and_then(text))));
        }
        if !status.is_success() {
            let detail = serde_json::from_str::<Value>(&body).ok().and_then(|v| v.get("detail").and_then(text)).unwrap_or(body);
            return Err(CloudError::Other(AppError::msg(format!("The BYTE cloud said: {} ({status})", detail.chars().take(300).collect::<String>()))));
        }
        Ok(if body.trim().is_empty() { Value::Null } else { serde_json::from_str(&body).unwrap_or(Value::String(body)) })
    }

    /// Web search through the cloud (`GET /api/search`, SearXNG on the cluster):
    /// `{engine: "searxng"|"ddgs"|"none", results: [{title, body, href}]}`.
    pub async fn search(&self, query: &str, count: usize) -> CloudResult<(String, Vec<crate::tools::search::SearchResult>)> {
        let rb = self.req(reqwest::Method::GET, "/api/search").query(&[("q", query), ("count", &count.to_string())]);
        let v = self.send(rb, Duration::from_secs(20)).await?;
        let engine = v.get("engine").and_then(Value::as_str).unwrap_or("none").to_string();
        let results = v
            .get("results")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|r| {
                        let url = r.get("href").or_else(|| r.get("url")).and_then(Value::as_str)?;
                        let parsed = url::Url::parse(url).ok()?;
                        matches!(parsed.scheme(), "http" | "https").then(|| crate::tools::search::SearchResult {
                            title: text(r.get("title").unwrap_or(&Value::Null)).unwrap_or_default(),
                            url: parsed.to_string(),
                            snippet: r.get("body").or_else(|| r.get("content")).and_then(text).unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok((engine, results))
    }

    pub async fn get(&self, path: &str) -> CloudResult<Value> {
        self.send(self.req(reqwest::Method::GET, path), Duration::from_secs(60)).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> CloudResult<Value> {
        // Posting may wait in the cluster's queue; that's normal, not an error.
        self.send(self.req(reqwest::Method::POST, path).json(body), Duration::from_secs(600)).await
    }

    pub async fn delete(&self, path: &str) -> CloudResult<Value> {
        self.send(self.req(reqwest::Method::DELETE, path), Duration::from_secs(60)).await
    }

    /// Raw bytes and content type (images, documents, exports).
    pub async fn bytes(&self, path: &str) -> CloudResult<(Vec<u8>, String)> {
        let r = self.req(reqwest::Method::GET, path).timeout(Duration::from_secs(300)).send().await.map_err(net_error)?;
        let status = r.status();
        if status == 401 || status == 403 {
            return Err(CloudError::Unauthorized);
        }
        if !status.is_success() {
            return Err(if matches!(status.as_u16(), 502..=504) {
                CloudError::Unreachable(format!("HTTP {status}"))
            } else {
                CloudError::Other(AppError::msg(format!("The BYTE cloud couldn't send that file ({status}).")))
            });
        }
        let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
        Ok((r.bytes().await.map_err(net_error)?.to_vec(), mime))
    }

    /// Sends a local file as multipart form field `file`.
    pub async fn upload(&self, path: &str, file: &std::path::Path, max: u64) -> CloudResult<Value> {
        let meta = std::fs::metadata(file).map_err(|e| CloudError::Other(e.into()))?;
        if meta.len() > max {
            return Err(CloudError::Other(AppError::msg(format!("That file is too big to send (limit {} MB).", max / (1024 * 1024)))));
        }
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let bytes = tokio::fs::read(file).await.map_err(|e| CloudError::Other(e.into()))?;
        let part = reqwest::multipart::Part::bytes(bytes).file_name(name.clone()).mime_str(mime_for(&name)).map_err(|e| CloudError::Other(e.into()))?;
        let form = reqwest::multipart::Form::new().part("file", part);
        self.send(self.req(reqwest::Method::POST, path).multipart(form), Duration::from_secs(600)).await
    }

    pub async fn create_conversation(&self, title: &str) -> CloudResult<String> {
        let v = self.post("/api/conversations", &json!({ "title": title })).await?;
        id_of(&v).ok_or_else(|| CloudError::Other(AppError::msg("the BYTE cloud didn't return a conversation id")))
    }

    /// Opens the event stream of a conversation, after message `since`.
    async fn open_stream(&self, cid: &str, since: Option<&str>) -> CloudResult<reqwest::Response> {
        let mut path = format!("/api/conversations/{cid}/stream");
        if let Some(s) = since {
            path.push_str(&format!("?since={s}"));
        }
        let r = self
            .req(reqwest::Method::GET, &path)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(net_error)?;
        match r.status().as_u16() {
            401 | 403 => Err(CloudError::Unauthorized),
            429 => Err(limit_error(None)),
            502..=504 => Err(CloudError::Unreachable(format!("HTTP {}", r.status()))),
            s if !(200..300).contains(&s) => Err(CloudError::Other(AppError::msg(format!("the BYTE cloud stream failed (HTTP {s})")))),
            _ => Ok(r),
        }
    }
}

/// Content type from a file name (for uploads).
pub fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "txt" | "md" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// Finds the user's and the assistant's message ids in whatever the post
/// returned (a row, a list of rows, or `{user_message, assistant_message}`).
pub fn posted_ids(v: &Value) -> (Option<String>, Option<String>) {
    let mut rows: Vec<&Value> = Vec::new();
    match v {
        Value::Array(a) => rows.extend(a.iter()),
        Value::Object(o) => {
            rows.push(v);
            rows.extend(o.values().filter(|x| x.is_object()));
            if let Some(Value::Array(a)) = o.get("messages") {
                rows.extend(a.iter());
            }
        }
        _ => {}
    }
    let find = |role: &str| rows.iter().find(|r| r.get("role").and_then(Value::as_str) == Some(role)).and_then(|r| id_of(r));
    let user = find("user").or_else(|| v.get("user_message_id").and_then(text)).or_else(|| (v.get("role").is_none()).then(|| id_of(v)).flatten());
    let assistant = find("assistant").or_else(|| v.get("assistant_message_id").and_then(text));
    (user, assistant)
}

pub fn sources_of(v: &Value) -> Vec<crate::tools::Source> {
    v.get("sources")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(i, s)| {
                    let url = s.get("url").or_else(|| s.get("link")).and_then(text).or_else(|| s.as_str().map(String::from)).unwrap_or_default();
                    crate::tools::Source {
                        n: s.get("n").and_then(Value::as_u64).map(|n| n as u32).unwrap_or(i as u32 + 1),
                        title: s.get("title").and_then(text).unwrap_or_else(|| url.clone()),
                        snippet: s.get("snippet").or_else(|| s.get("excerpt")).and_then(text).unwrap_or_default(),
                        read: true,
                        url,
                        meta: None,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The HTTP client for the cloud. One is kept for the whole app (AppState).
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(30))
        .user_agent(concat!("BYTE-mac/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

/// Wait between reconnect attempts that got nothing (grows with each try).
const BACKOFF_MS: u64 = if cfg!(test) { 5 } else { 500 };

/// How one streamed answer ended.
#[derive(Debug, PartialEq)]
pub struct TurnEnd {
    pub assistant_id: Option<String>,
    pub text: String,
    pub finish: String,
}

/// Follows one answer on the conversation's event stream and forwards it as
/// `ChatEvent`s. `since` is the message after which new rows appear (the
/// user's message); `assistant` is the answer's id if the post returned it.
/// Reconnects (with `since`) if the stream closes before the answer is done.
pub async fn follow(
    client: &CloudClient,
    cid: &str,
    since: Option<String>,
    mut assistant: Option<String>,
    cancel: &CancellationToken,
    on_event: &Channel<ChatEvent>,
) -> CloudResult<TurnEnd> {
    let started = Instant::now();
    let mut first_token: Option<Instant> = None;
    let mut text_out = String::new();
    // Connection attempts in a row that brought no events. A stream that
    // closes after delivering text (the server's `bye`, a redeploy) is
    // reopened at once; only empty attempts wait, and only they count.
    let mut empty = 0u64;
    let mut connected_once = false;
    let emit = |e: ChatEvent| {
        let _ = on_event.send(e);
    };
    loop {
        let resp = tokio::select! {
            _ = cancel.cancelled() => return stop(client, assistant, text_out).await,
            r = client.open_stream(cid, since.as_deref()) => r,
        };
        let resp = match resp {
            Ok(r) => r,
            // The stream came back once and now won't reopen: keep trying below, then re-read.
            Err(CloudError::Unreachable(m)) if connected_once => {
                empty += 1;
                if empty > 5 {
                    return recover(client, cid, since.as_deref(), assistant, text_out, on_event).await.ok_or(CloudError::Unreachable(m));
                }
                tokio::time::sleep(Duration::from_millis(BACKOFF_MS * empty)).await;
                continue;
            }
            Err(e) => return Err(e),
        };
        connected_once = true;
        let mut got_events = false;
        let mut body = resp.bytes_stream();
        let mut parser = sse::Parser::default();
        let mut finished: Option<String> = None;
        'read: loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return stop(client, assistant, text_out).await,
                c = body.next() => c,
            };
            let Some(chunk) = chunk else { break 'read };
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => break 'read, // dropped: reconnect below
            };
            for ev in parser.push(&String::from_utf8_lossy(&chunk)) {
                if ev.event != "bye" {
                    got_events = true;
                }
                let data: Value = serde_json::from_str(&ev.data).unwrap_or(Value::Null);
                let row_id = id_of(&data);
                let mine = |a: &Option<String>| a.is_none() || a == &row_id;
                match ev.event.as_str() {
                    "message" => {
                        if data.get("role").and_then(Value::as_str) != Some("assistant") || !mine(&assistant) {
                            continue;
                        }
                        assistant = row_id.clone();
                        // A full row (first sight, or after a reconnect): emit what we haven't shown.
                        let content = data.get("content").and_then(Value::as_str).unwrap_or("");
                        if content.len() > text_out.len() && content.starts_with(&text_out) {
                            let rest = &content[text_out.len()..];
                            first_token.get_or_insert_with(Instant::now);
                            emit(ChatEvent::Content { delta: rest.to_string() });
                            text_out.push_str(rest);
                        }
                        if let Some(s) = data.get("status").and_then(Value::as_str).filter(|s| matches!(*s, "done" | "error")) {
                            finished = Some(s.into());
                        }
                        let src = sources_of(&data);
                        if !src.is_empty() {
                            emit(ChatEvent::Sources { sources: src });
                        }
                    }
                    "delta" => {
                        if !mine(&assistant) {
                            continue;
                        }
                        assistant = row_id.clone().or(assistant);
                        if let Some(add) = data.get("append").and_then(Value::as_str).filter(|a| !a.is_empty()) {
                            first_token.get_or_insert_with(Instant::now);
                            emit(ChatEvent::Content { delta: add.to_string() });
                            text_out.push_str(add);
                        }
                        if let Some(s) = data.get("status").and_then(Value::as_str).filter(|s| matches!(*s, "done" | "error")) {
                            finished = Some(s.into());
                        }
                    }
                    "status" => {
                        if mine(&assistant) {
                            if let Some(s) = data.get("status").and_then(Value::as_str).filter(|s| matches!(*s, "done" | "error")) {
                                finished = Some(s.into());
                            }
                        }
                    }
                    "phase" => {
                        if let Some(p) = data.get("phase").and_then(text) {
                            emit(ChatEvent::Phase { text: p });
                        }
                    }
                    "sources" => {
                        if mine(&assistant) {
                            emit(ChatEvent::Sources { sources: sources_of(&data) });
                        }
                    }
                    "done" => finished = finished.or(Some("done".into())),
                    "bye" => break 'read,
                    _ => {}
                }
            }
            if finished.is_some() {
                break 'read;
            }
        }
        if let Some(f) = finished {
            let secs = first_token.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
            let tokens = (text_out.chars().count() as f64 / 4.0).round() as u64;
            emit(ChatEvent::Stats(Stats {
                completion_tokens: tokens,
                tokens_per_second: if secs > 0.5 { tokens as f64 / secs } else { 0.0 },
                total_ms: started.elapsed().as_secs_f64() * 1000.0,
                ..Default::default()
            }));
            if f == "error" {
                return Err(CloudError::Other(AppError::msg("The BYTE cloud couldn't finish this answer. Try again.")));
            }
            return Ok(TurnEnd { assistant_id: assistant, text: text_out, finish: "stop".into() });
        }
        if got_events {
            empty = 0;
            continue; // the answer is still coming: reopen right away
        }
        empty += 1;
        if empty > 5 {
            return recover(client, cid, since.as_deref(), assistant, text_out, on_event)
                .await
                .ok_or_else(|| CloudError::Unreachable("the answer stream kept dropping".into()));
        }
        tokio::time::sleep(Duration::from_millis(BACKOFF_MS * empty)).await;
    }
}

/// The answer row in a conversation: by id when known, else the first
/// assistant message after `since` (the user's message).
pub fn saved_answer<'a>(conv: &'a Value, since: Option<&str>, assistant: Option<&str>) -> Option<&'a Value> {
    let rows = conv.get("messages").or_else(|| conv.get("items")).unwrap_or(conv).as_array()?;
    if let Some(id) = assistant {
        if let Some(r) = rows.iter().find(|r| id_of(r).as_deref() == Some(id)) {
            return Some(r);
        }
    }
    let start = since.and_then(|s| rows.iter().position(|r| id_of(r).as_deref() == Some(s))).map(|i| i + 1).unwrap_or(0);
    rows[start..].iter().find(|r| r.get("role").and_then(Value::as_str) == Some("assistant"))
}

/// When the stream won't come back, the answer is usually already saved on
/// the cloud: re-read the conversation and finish from the saved row.
async fn recover(
    client: &CloudClient,
    cid: &str,
    since: Option<&str>,
    assistant: Option<String>,
    mut text_out: String,
    on_event: &Channel<ChatEvent>,
) -> Option<TurnEnd> {
    let conv = client.get(&format!("/api/conversations/{cid}")).await.ok()?;
    let row = saved_answer(&conv, since, assistant.as_deref())?;
    let content = row.get("content").and_then(Value::as_str).unwrap_or("");
    if content.is_empty() && text_out.is_empty() {
        return None;
    }
    if content.len() > text_out.len() && content.starts_with(&text_out) {
        let _ = on_event.send(ChatEvent::Content { delta: content[text_out.len()..].to_string() });
        text_out = content.to_string();
    }
    let src = sources_of(row);
    if !src.is_empty() {
        let _ = on_event.send(ChatEvent::Sources { sources: src });
    }
    Some(TurnEnd { assistant_id: id_of(row).or(assistant), text: text_out, finish: "stop".into() })
}

async fn stop(client: &CloudClient, assistant: Option<String>, text: String) -> CloudResult<TurnEnd> {
    if let Some(id) = &assistant {
        let _ = client.post(&format!("/api/messages/{id}/stop"), &json!({})).await;
    }
    Ok(TurnEnd { assistant_id: assistant, text, finish: "cancelled".into() })
}

/// Message actions that start a new streamed answer.
/// (`answer-now` and `stop` act on the answer already streaming.)
pub const STREAMING_ACTIONS: &[&str] = &["regenerate", "deepen", "justify"];
/// All message actions the app offers.
pub const ACTIONS: &[&str] = &["regenerate", "deepen", "justify", "stop", "answer-now", "feedback"];

#[cfg(test)]
mod tests;
