//! Streams chat completions from the local engine to the UI.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use tauri::ipc::Channel;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::engine::Endpoint;
use crate::error::{AppError, AppResult};
use crate::router::TurnPlan;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Files attached to this message (read by `files::ingest` when attached);
    /// `with_files` turns them into text (and photos) for the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<crate::files::Ingested>,
    /// Photos for a model that can see (data URLs), filled in by `with_files`.
    #[serde(skip)]
    pub images: Vec<String>,
}

impl ChatMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        ChatMessage { role: role.into(), content: content.into(), ..Default::default() }
    }
}

/// Where attached files start in a message's content (see `with_files`).
const FILES_MARK: &str = "\n\n<file name=";

/// A message's own words, without the attached files' text.
pub fn question_text(content: &str) -> &str {
    content.split(FILES_MARK).next().unwrap_or(content)
}

/// Rough cost of one photo in the context (vision encoders use a few hundred tokens).
const IMAGE_TOKENS: usize = 700;

/// Puts attached files into the messages the model sees. The latest message's
/// files get most of the room (about 45% of the context), earlier ones a
/// little, each reduced to the passages that match what was asked. Photos go
/// to models that can see (`vision`), at most 4, only from the latest message;
/// otherwise the model is told a photo was attached that it can't view.
pub fn with_files(history: &[ChatMessage], context: u32, vision: bool) -> Vec<ChatMessage> {
    let last_user = history.iter().rposition(|m| m.role == "user");
    history
        .iter()
        .enumerate()
        .map(|(i, m)| {
            if m.files.is_empty() {
                return m.clone();
            }
            let latest = Some(i) == last_user;
            let share = if latest { 0.45 } else { 0.08 };
            let budget = (context as f64 * 3.5 * share) as usize;
            let mut content = m.content.clone();
            content.push_str(&crate::files::for_model(&m.files, &m.content, budget));
            let mut images = Vec::new();
            for f in m.files.iter().filter(|f| f.kind == crate::files::FileKind::Image) {
                match &f.image {
                    Some(url) if vision && latest && images.len() < 4 => images.push(url.clone()),
                    _ if vision => content.push_str(&format!("\n\n[Photo \"{}\" was attached earlier.]", f.name)),
                    // BYTE's photo helper described it (and its text) in the file block above.
                    _ if f.text.starts_with(crate::looker::DESCRIBED) => content.push_str(&format!(
                        "\n\n[The user attached a photo, \"{}\". You can't see images, so BYTE's photo helper described it for you (above). Answer from that description; if the question needs a detail it doesn't mention, say so.]",
                        f.name
                    )),
                    // Its words were read (text recognition) and are in the file block above.
                    _ if !f.text.trim().is_empty() => content.push_str(&format!(
                        "\n\n[The user attached a photo, \"{}\". You can't see images, but the text in it was read for you (above). Use it, and if the question is about what the photo shows beyond its text, say a model marked \"Sees images\" can look at it.]",
                        f.name
                    )),
                    _ => content.push_str(&format!(
                        "\n\n[The user attached a photo, \"{}\", but the current model can't see images. Say so, and suggest a model marked \"Sees images\".]",
                        f.name
                    )),
                }
            }
            ChatMessage { role: m.role.clone(), content, files: Vec::new(), images }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum ChatEvent {
    Started { thinking: bool, model: String },
    Reasoning { delta: String },
    Content { delta: String },
    /// The model asked to use a tool (e.g. a web search).
    ToolCall { id: String, name: String, args: serde_json::Value },
    /// A tool finished; `summary` is a short human description.
    ToolResult { id: String, ok: bool, summary: String },
    /// The numbered sources gathered so far, for citations.
    Sources { sources: Vec<crate::tools::Source> },
    Stats(Stats),
    Done { finish_reason: String },
    /// Live progress from the BYTE cloud ("searching: …").
    Phase { text: String },
    /// Ids on the BYTE cloud for this turn, so the chat can continue there.
    Remote { conversation_id: String, message_id: Option<String>, user_message_id: Option<String> },
    /// Something the user should know about this answer (e.g. answered on this Mac because the cloud was down).
    Notice { text: String },
    /// A compare & decide score table (decide.rs), shown above the answer.
    Decision(crate::decide::Decision),
    /// Places found nearby (find_places), shown as cards.
    Places(crate::tools::PlacesFound),
    /// A trip plan (trip.rs), shown as an itinerary card.
    Trip(crate::trip::TripPlan),
    /// A recipe card (kitchen.rs).
    Recipe(crate::kitchen::Recipe),
    /// Dishes to choose from ("what can I make with…").
    RecipeIdeas(crate::kitchen::RecipeIdeas),
    /// A meal plan with a grocery list.
    MealPlan(crate::kitchen::MealPlan),
    /// A YouTube video's summary card (youtube.rs).
    Video(crate::youtube::VideoCard),
    /// The web agent wants to submit, commit or download: the user approves or denies.
    Approval(crate::web_agent::ApprovalAsk),
    /// The approval card was answered (or timed out: `ok` false).
    ApprovalDone { id: String, ok: bool },
    /// A file the web agent saved (a download, or a page as PDF/picture).
    Saved(crate::web_agent::SavedFile),
    /// The web agent's browser opened (true) or closed (false).
    Browsing { active: bool },
    /// What reviewers say: ratings, pros and cons (reviews.rs).
    Reviews(crate::reviews::Reviews),
    /// Prices found on store pages (prices.rs).
    Prices(crate::prices::Prices),
    /// Spoiler-free game hints, hidden until tapped (games.rs).
    Hints(crate::games::Hints),
    /// Claims in the answer its sources don't clearly back (selfcheck.rs).
    SelfCheck(crate::selfcheck::SelfCheck),
    /// Flashcards to flip, save as a deck and study (study.rs).
    Flashcards(crate::study::Flashcards),
    /// A multiple-choice quiz that scores itself (study.rs).
    Quiz(crate::study::Quiz),
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tokens_per_second: f64,
    pub prompt_ms: f64,
    pub total_ms: f64,
    pub thinking_ms: f64,
    /// Speed boost (speculative decoding): tokens the helper model guessed,
    /// and how many of them the main model kept.
    pub draft_tokens: u64,
    pub draft_accepted: u64,
}

/// One non-streamed reply constrained to a JSON schema (llama-server
/// `response_format`), thinking off. Returns the raw text; callers parse it
/// leniently because small models still cut replies short.
pub async fn complete_json(http: &reqwest::Client, ep: &Endpoint, system: &str, user: &str, schema: serde_json::Value, max_tokens: u32) -> AppResult<String> {
    if let Some(cloud) = &ep.cloud {
        return cloud.complete(system, user, &schema).await;
    }
    // Models that always reason (DeepSeek-R1 and the like) ignore `enable_thinking: false`;
    // the JSON only starts after the reasoning, so they need room for both.
    let key = model_key(ep);
    let mut thinks = quirks().lock().await.reasons.contains(&key);
    for _ in 0..2 {
        let body = serde_json::json!({
            "messages": [ { "role": "system", "content": system }, { "role": "user", "content": user } ],
            "max_tokens": if thinks { with_room(ep, max_tokens, system.len() + user.len() + schema.to_string().len()) } else { max_tokens },
            "temperature": 0.5,
            "stream": false,
            "response_format": { "type": "json_schema", "json_schema": { "name": "reply", "schema": schema } },
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let mut body = body;
        if thinks {
            cap_reasoning(&mut body);
        }
        let r = http.post(format!("{}/v1/chat/completions", ep.base_url)).bearer_auth(&ep.api_key).timeout(std::time::Duration::from_secs(600)).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(AppError::msg(format!("the engine returned {}", r.status())));
        }
        let v: serde_json::Value = r.json().await?;
        let msg = &v["choices"][0]["message"];
        let content = msg["content"].as_str().unwrap_or("").to_string();
        let reasoned = msg["reasoning_content"].as_str().is_some_and(|r| !r.trim().is_empty());
        // Reasoned first and ran out (nothing, or JSON cut off): learn it, and once more with room.
        let cut = v["choices"][0]["finish_reason"] == "length";
        if (content.trim().is_empty() || cut) && reasoned && !thinks {
            log::info!("{} reasons before JSON; giving it room", ep.model);
            quirks().lock().await.reasons.insert(key.clone());
            thinks = true;
            continue;
        }
        return Ok(content);
    }
    Ok(String::new())
}

/// Extra tokens for a model that reasons when asked not to (DeepSeek-R1 and the like).
const REASONING_ROOM: u32 = 3072;
/// How long such a model may reason before it has to answer.
const REASONING_CAP: u32 = 1536;

/// `max_tokens` plus room to reason first, within what the context has left
/// after a prompt of `prompt_chars` (never less than `max_tokens`).
fn with_room(ep: &Endpoint, max_tokens: u32, prompt_chars: usize) -> u32 {
    let left = (ep.context as usize).saturating_sub(prompt_chars / 3 + 64) as u32;
    (max_tokens + REASONING_ROOM).min(left).max(max_tokens)
}

/// Asks the engine to end the reasoning after `REASONING_CAP` tokens (unless the plan set its own budget).
fn cap_reasoning(body: &mut serde_json::Value) {
    if body.get("reasoning_budget_tokens").is_none() {
        body["reasoning_budget_tokens"] = REASONING_CAP.into();
        body["reasoning_budget_message"] = "\n\nI've thought enough; answering now.".into();
    }
}

/// A chat request adjusted for a model that reasons when asked not to.
fn for_reasoner(ep: &Endpoint, body: &serde_json::Value) -> serde_json::Value {
    let mut b = body.clone();
    if b.get("reasoning_budget_tokens").is_none() {
        let asked = b["max_tokens"].as_u64().unwrap_or(1024) as u32;
        let prompt_chars = b["messages"].to_string().len();
        b["max_tokens"] = with_room(ep, asked, prompt_chars).into();
        cap_reasoning(&mut b);
    }
    b
}

/// What BYTE learned about the running model's quirks (per engine and model).
#[derive(Default)]
struct Quirks {
    /// Reasons before answering even when asked not to.
    reasons: std::collections::HashSet<String>,
    /// Its chat template can't take tool calls/results (Gemma 3), or its tool calls can't be parsed.
    plain: std::collections::HashSet<String>,
}

fn quirks() -> &'static Mutex<Quirks> {
    static Q: std::sync::OnceLock<Mutex<Quirks>> = std::sync::OnceLock::new();
    Q.get_or_init(Default::default)
}

fn model_key(ep: &Endpoint) -> String {
    format!("{}|{}", ep.base_url, ep.model)
}

/// An engine error that means "this model can't do tool calling this way".
fn needs_plain(msg: &str) -> bool {
    ["Jinja Exception", "raise_exception", "Unable to generate parser", "roles must alternate", "does not match the expected", "Failed to parse tool call", "Failed to parse input"]
        .iter()
        .any(|k| msg.contains(k))
}

/// The same request without tool calling: tool calls and results become plain text in the
/// conversation (so templates that only know user/assistant turns accept it), and turns by
/// the same speaker are merged.
pub fn plain_body(body: &serde_json::Value) -> serde_json::Value {
    let mut out = body.clone();
    if let Some(o) = out.as_object_mut() {
        o.remove("tools");
        o.remove("tool_choice");
        o.remove("parallel_tool_calls");
    }
    let mut names: HashMap<String, String> = HashMap::new();
    let mut msgs: Vec<serde_json::Value> = Vec::new();
    for m in body["messages"].as_array().into_iter().flatten() {
        let role = m["role"].as_str().unwrap_or("user");
        let (role, content) = match role {
            "tool" => {
                let name = m["tool_call_id"].as_str().and_then(|id| names.get(id)).cloned().unwrap_or_else(|| "a tool".into());
                ("user", serde_json::Value::String(format!("[Result of {name}]\n{}", m["content"].as_str().unwrap_or(""))))
            }
            "assistant" => {
                for c in m["tool_calls"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(n)) = (c["id"].as_str(), c["function"]["name"].as_str()) {
                        names.insert(id.to_string(), n.to_string());
                    }
                }
                let text = m["content"].as_str().unwrap_or("").trim().to_string();
                if text.is_empty() {
                    continue;
                }
                ("assistant", serde_json::Value::String(text))
            }
            r => (r, m["content"].clone()),
        };
        let text_of = |c: &serde_json::Value| c.as_str().map(str::to_string);
        if let Some(last) = msgs.last_mut().filter(|l| l["role"] == role) {
            match (text_of(&last["content"]), text_of(&content)) {
                (Some(a), Some(b)) => last["content"] = format!("{a}\n\n{b}").into(),
                (_, b) => {
                    // Photos (content parts): keep the parts, add the text.
                    let mut parts = last["content"].as_array().cloned().unwrap_or_else(|| vec![serde_json::json!({ "type": "text", "text": last["content"].as_str().unwrap_or("") })]);
                    match b {
                        Some(t) => parts.push(serde_json::json!({ "type": "text", "text": t })),
                        None => parts.extend(content.as_array().cloned().unwrap_or_default()),
                    }
                    last["content"] = parts.into();
                }
            }
            continue;
        }
        msgs.push(serde_json::json!({ "role": role, "content": content }));
    }
    out["messages"] = msgs.into();
    out
}

/// Registry of running generations so the UI can stop them.
#[derive(Default, Clone)]
pub struct Generations {
    running: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl Generations {
    pub async fn register(&self, id: &str) -> CancellationToken {
        let t = CancellationToken::new();
        self.running.lock().await.insert(id.to_string(), t.clone());
        t
    }
    pub async fn finish(&self, id: &str) {
        self.running.lock().await.remove(id);
    }
    pub async fn cancel(&self, id: &str) -> bool {
        match self.running.lock().await.get(id) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }
}

/// HTTP client for the local engine. llama-server closes connections after a
/// streamed response, so pooled keep-alive connections go stale and the next
/// request fails with `IncompleteMessage`. Never reuse connections.
pub fn local_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("local http client")
}

/// System prompt + history as engine messages.
pub fn base_messages(system: &str, history: &[ChatMessage]) -> Vec<serde_json::Value> {
    let mut messages = vec![serde_json::json!({ "role": "system", "content": system })];
    messages.extend(history.iter().map(|m| {
        if m.images.is_empty() {
            serde_json::json!({ "role": m.role, "content": m.content })
        } else {
            // OpenAI-style parts: text, then the photos (llama-server with --mmproj).
            let mut parts = vec![serde_json::json!({ "type": "text", "text": m.content })];
            parts.extend(m.images.iter().map(|url| serde_json::json!({ "type": "image_url", "image_url": { "url": url } })));
            serde_json::json!({ "role": m.role, "content": parts })
        }
    }));
    messages
}

#[cfg(test)]
pub fn request_body(system: &str, history: &[ChatMessage], plan: TurnPlan) -> serde_json::Value {
    build_body(base_messages(system, history), plan, None)
}

/// Full request body; `tools` enables tool calling for this round.
pub fn build_body(messages: Vec<serde_json::Value>, plan: TurnPlan, tools: Option<Vec<serde_json::Value>>) -> serde_json::Value {
    // The running model's recommended sampling (see modelcfg.rs).
    let sp = plan.profile.sampling(plan.thinking);
    // f32 → tidy f64 (0.7, not 0.699999988).
    let r = |x: f32| (x as f64 * 1000.0).round() / 1000.0;
    let mut body = serde_json::json!({
        "messages": messages,
        "stream": true,
        "max_tokens": plan.max_tokens,
        "temperature": r(sp.temperature),
        "top_p": r(sp.top_p),
        "top_k": sp.top_k,
        "min_p": r(sp.min_p),
        "repeat_penalty": r(sp.repeat_penalty),
        "cache_prompt": true,
        "timings_per_token": false,
        "chat_template_kwargs": plan.profile.template_kwargs(plan.thinking, plan.mode),
    });
    if plan.thinking && plan.thinking_budget > 0 {
        body["reasoning_budget_tokens"] = plan.thinking_budget.into();
        body["reasoning_budget_message"] = "\n\nI've thought enough; answering now.".into();
    }
    if let Some(tools) = tools.filter(|t| !t.is_empty()) {
        body["tools"] = tools.into();
        body["tool_choice"] = "auto".into();
        body["parallel_tool_calls"] = true.into();
    }
    body
}

/// Incremental parser for `text/event-stream` bodies. Buffers raw bytes so a
/// multi-byte character split across network chunks is never corrupted.
#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    /// Feeds raw bytes and returns complete `data:` payloads.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(data) = line.strip_prefix("data:") {
                out.push(data.trim_start().to_string());
            }
        }
        out
    }
}

/// Rough token estimate (Qwen averages ~3.5 characters per token in English).
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count() * 2 / 7 + 4
}

/// Drops the oldest turns until the conversation fits the context window,
/// always keeping the latest user message.
pub fn fit_history(history: &[ChatMessage], system: &str, context: u32, reserve: u32) -> Vec<ChatMessage> {
    let budget = (context as usize).saturating_sub(reserve as usize + estimate_tokens(system));
    let mut kept: Vec<ChatMessage> = Vec::new();
    let mut used = 0usize;
    for m in history.iter().rev() {
        let t = estimate_tokens(&m.content) + m.images.len() * IMAGE_TOKENS;
        if used + t > budget && !kept.is_empty() {
            break;
        }
        used += t;
        kept.push(m.clone());
    }
    kept.reverse();
    // A conversation must not start with an assistant turn.
    while kept.len() > 1 && kept[0].role == "assistant" {
        kept.remove(0);
    }
    kept
}

#[derive(Debug, PartialEq)]
pub enum Delta {
    Reasoning(String),
    Content(String),
    Finish(String),
    Timings(Stats),
    /// A fragment of a streamed tool call; fragments share `index`.
    ToolCall { index: usize, id: Option<String>, name: Option<String>, args: String },
    Error(String),
    End,
}

/// Interprets one SSE payload from llama-server's OpenAI-compatible endpoint.
pub fn parse_payload(data: &str) -> Vec<Delta> {
    if data == "[DONE]" {
        return vec![Delta::End];
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
        return vec![];
    };
    if let Some(err) = v.get("error") {
        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("engine error");
        return vec![Delta::Error(msg.to_string())];
    }
    let mut out = Vec::new();
    if let Some(choice) = v.get("choices").and_then(|c| c.get(0)) {
        if let Some(delta) = choice.get("delta") {
            if let Some(r) = delta.get("reasoning_content").and_then(|x| x.as_str()) {
                if !r.is_empty() {
                    out.push(Delta::Reasoning(r.to_string()));
                }
            }
            if let Some(c) = delta.get("content").and_then(|x| x.as_str()) {
                if !c.is_empty() {
                    out.push(Delta::Content(c.to_string()));
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(|x| x.as_array()) {
                for (i, call) in calls.iter().enumerate() {
                    let f = call.get("function");
                    out.push(Delta::ToolCall {
                        index: call.get("index").and_then(|x| x.as_u64()).map(|x| x as usize).unwrap_or(i),
                        id: call.get("id").and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string),
                        name: f.and_then(|f| f.get("name")).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string),
                        args: f.and_then(|f| f.get("arguments")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    });
                }
            }
        }
        if let Some(f) = choice.get("finish_reason").and_then(|x| x.as_str()) {
            out.push(Delta::Finish(f.to_string()));
        }
    }
    if let Some(t) = v.get("timings") {
        let f = |k: &str| t.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
        out.push(Delta::Timings(Stats {
            prompt_tokens: f("prompt_n") as u64,
            completion_tokens: f("predicted_n") as u64,
            tokens_per_second: f("predicted_per_second"),
            prompt_ms: f("prompt_ms"),
            total_ms: f("prompt_ms") + f("predicted_ms"),
            thinking_ms: 0.0,
            draft_tokens: f("draft_n") as u64,
            draft_accepted: f("draft_n_accepted") as u64,
        }));
    }
    out
}

/// A tool call assembled from streamed fragments.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolCallReq {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Everything one engine request produced.
#[derive(Debug, Default)]
pub struct Round {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCallReq>,
    pub finish: String,
    pub stats: Option<Stats>,
    /// Time from the request to the first answer token, if thinking happened first.
    pub thinking_ms: f64,
}

/// Sends one request and streams reasoning/content deltas to `on_event`.
/// Tool calls and stats are returned, not emitted, so the caller decides.
/// A model whose template or tool calls the engine can't handle gets the
/// same request again without tool calling (`plain_body`), and from then on.
pub async fn stream_round(
    http: &reqwest::Client,
    ep: &Endpoint,
    body: &serde_json::Value,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(ChatEvent) -> AppResult<()> + Send),
) -> AppResult<Round> {
    let key = model_key(ep);
    // Thinking wasn't planned for this request (off, or a model with no thinking switch).
    let kw = &body["chat_template_kwargs"];
    let asked_not_to_think = kw["enable_thinking"] != true && kw["reasoning_effort"].is_null() && body.get("reasoning_budget_tokens").is_none();
    let reasoner = asked_not_to_think && quirks().lock().await.reasons.contains(&key);
    let body = &if reasoner { for_reasoner(ep, body) } else { body.clone() };
    let round = if !quirks().lock().await.plain.contains(&key) {
        match stream_round_once(http, ep, body, cancel, on_event).await {
            Err(AppError::Msg(m)) if needs_plain(&m) => {
                log::warn!("{}: no tool calling for this model ({m})", ep.model);
                quirks().lock().await.plain.insert(key.clone());
                stream_round_once(http, ep, &plain_body(body), cancel, on_event).await
            }
            other => other,
        }
    } else {
        stream_round_once(http, ep, &plain_body(body), cancel, on_event).await
    }?;
    // It reasoned though asked not to, and ran out before answering: learn it, and once more with room.
    if asked_not_to_think && !reasoner && !round.reasoning.trim().is_empty() {
        quirks().lock().await.reasons.insert(key.clone());
        if round.content.trim().is_empty() && round.tool_calls.is_empty() && round.finish == "length" {
            log::info!("{} reasons before answering; retrying with room", ep.model);
            let again = for_reasoner(ep, body);
            let again = if quirks().lock().await.plain.contains(&key) { plain_body(&again) } else { again };
            return stream_round_once(http, ep, &again, cancel, on_event).await;
        }
    }
    Ok(round)
}

async fn stream_round_once(
    http: &reqwest::Client,
    ep: &Endpoint,
    body: &serde_json::Value,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(ChatEvent) -> AppResult<()> + Send),
) -> AppResult<Round> {
    let started = Instant::now();
    let url = format!("{}/v1/chat/completions", ep.base_url);
    let mut attempt = 0;
    let resp = loop {
        attempt += 1;
        let sent = tokio::select! {
            r = http.post(&url).bearer_auth(&ep.api_key).json(body).send() => r,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        match sent {
            Ok(r) => break r,
            // Nothing was generated yet, so one retry is safe.
            Err(e) if attempt < 2 && (e.is_request() || e.is_connect()) => {
                log::warn!("engine request failed before streaming, retrying: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
            Err(e) => return Err(e.into()),
        }
    };
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or(text);
        return Err(AppError::msg(format!("engine returned {status}: {msg}")));
    }

    let mut round = Round { finish: "stop".into(), ..Default::default() };
    let mut calls: Vec<ToolCallReq> = Vec::new();
    let mut parser = SseParser::default();
    let mut bytes = resp.bytes_stream();
    let mut first_content: Option<Instant> = None;
    loop {
        let chunk = tokio::select! {
            c = bytes.next() => c,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        let Some(chunk) = chunk else { break };
        for payload in parser.push(&chunk?) {
            for d in parse_payload(&payload) {
                match d {
                    Delta::Reasoning(t) => {
                        round.reasoning.push_str(&t);
                        on_event(ChatEvent::Reasoning { delta: t })?
                    }
                    Delta::Content(t) => {
                        first_content.get_or_insert_with(Instant::now);
                        round.content.push_str(&t);
                        on_event(ChatEvent::Content { delta: t })?
                    }
                    Delta::ToolCall { index, id, name, args } => {
                        if calls.len() <= index {
                            calls.resize(index + 1, ToolCallReq::default());
                        }
                        let c = &mut calls[index];
                        if let Some(id) = id {
                            c.id = id;
                        }
                        if let Some(n) = name {
                            c.name.push_str(&n);
                        }
                        c.arguments.push_str(&args);
                    }
                    Delta::Finish(f) => round.finish = f,
                    Delta::Timings(s) => round.stats = Some(s),
                    // The answer was already streaming when the engine failed to read a tool call: keep it.
                    Delta::Error(m) if needs_plain(&m) && !round.content.trim().is_empty() => {
                        log::warn!("engine error after the answer started: {m}");
                    }
                    Delta::Error(m) => return Err(AppError::msg(m)),
                    Delta::End => {}
                }
            }
        }
    }
    if !round.reasoning.is_empty() {
        round.thinking_ms = first_content.map(|fc| (fc - started).as_secs_f64() * 1000.0).unwrap_or_else(|| started.elapsed().as_secs_f64() * 1000.0);
    }
    round.tool_calls = calls.into_iter().filter(|c| !c.name.is_empty()).collect();
    Ok(round)
}

/// Single request, no tools (used by the end-to-end test).
#[cfg(test)]
pub async fn stream(
    http: &reqwest::Client,
    ep: &Endpoint,
    body: serde_json::Value,
    plan: TurnPlan,
    cancel: CancellationToken,
    events: &Channel<ChatEvent>,
) -> AppResult<()> {
    let mut send = |e: ChatEvent| events.send(e).map_err(|e| AppError::msg(format!("UI channel closed: {e}")));
    send(ChatEvent::Started { thinking: plan.thinking, model: ep.model.clone() })?;
    let round = stream_round(http, ep, &body, &cancel, &mut send).await?;
    if let Some(mut s) = round.stats {
        s.thinking_ms = round.thinking_ms;
        send(ChatEvent::Stats(s))?;
    }
    send(ChatEvent::Done { finish_reason: round.finish })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attached(name: &str, kind: crate::files::FileKind, text: &str, image: Option<&str>) -> crate::files::Ingested {
        crate::files::Ingested { name: name.into(), kind, pages: None, text: text.into(), truncated: false, image: image.map(Into::into), ocr: false }
    }

    #[test]
    fn reasoners_get_room_within_the_context() {
        let ep = Endpoint { base_url: "x".into(), api_key: String::new(), model: "m".into(), context: 4096, vision: false, cloud: None };
        assert_eq!(with_room(&ep, 500, 300), 500 + REASONING_ROOM);
        // A long prompt (~3 characters a token) leaves less room, but never less than asked.
        assert_eq!(with_room(&ep, 500, 9000), 4096 - 3000 - 64);
        assert_eq!(with_room(&ep, 500, 20000), 500);
        let b = for_reasoner(&ep, &serde_json::json!({ "max_tokens": 300, "messages": [] }));
        assert_eq!(b["reasoning_budget_tokens"], REASONING_CAP);
        assert!(b["max_tokens"].as_u64().unwrap() > 300);
        // A plan with its own budget is left alone.
        let planned = serde_json::json!({ "max_tokens": 300, "reasoning_budget_tokens": 512, "messages": [] });
        assert_eq!(for_reasoner(&ep, &planned), planned);
    }

    #[test]
    fn plain_body_turns_tool_calls_into_text() {
        let body = serde_json::json!({
            "tools": [{"type": "function"}], "tool_choice": "auto", "parallel_tool_calls": true, "stream": true,
            "messages": [
                {"role": "system", "content": "You are BYTE."},
                {"role": "user", "content": "What is 12*3?"},
                {"role": "assistant", "content": "", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "calculate", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "c1", "content": "12*3 = 36"},
            ]
        });
        let p = plain_body(&body);
        assert!(p.get("tools").is_none() && p.get("tool_choice").is_none() && p.get("parallel_tool_calls").is_none());
        assert_eq!(p["stream"], true);
        let m = p["messages"].as_array().unwrap();
        assert_eq!(m.len(), 2, "{m:?}");
        assert_eq!(m[1]["role"], "user");
        assert_eq!(m[1]["content"], "What is 12*3?\n\n[Result of calculate]\n12*3 = 36");
        assert!(needs_plain("Jinja Exception: Conversation roles must alternate user/assistant"));
        assert!(needs_plain("The model produced output that does not match the expected peg-native format"));
        assert!(!needs_plain("the context is full"));
    }

    #[test]
    fn attached_files_reach_the_model_and_the_question_stays_clean() {
        use crate::files::FileKind;
        let mut first = ChatMessage::new("user", "Summarise the report");
        first.files = vec![attached("report.pdf", FileKind::Pdf, "[Page 1]\nRevenue grew 12% in 2025.", None)];
        let answer = ChatMessage::new("assistant", "It grew 12%.");
        let mut second = ChatMessage::new("user", "What is in this photo?");
        second.files = vec![attached("cat.png", FileKind::Image, "", Some("data:image/png;base64,AAAA"))];
        let history = vec![first, answer, second];

        // A model that can see gets the photo; the PDF text stays with its message.
        let seen = with_files(&history, 16384, true);
        assert!(seen[0].content.contains("<file name=\"report.pdf\"") && seen[0].content.contains("Revenue grew 12%"));
        assert_eq!(question_text(&seen[0].content), "Summarise the report");
        assert_eq!(seen[2].images, vec!["data:image/png;base64,AAAA".to_string()]);
        assert!(seen.iter().all(|m| m.files.is_empty()));
        let body = base_messages("sys", &seen);
        assert_eq!(body[3]["content"][0]["type"], "text");
        assert_eq!(body[3]["content"][1]["image_url"]["url"], "data:image/png;base64,AAAA");
        assert!(body[1]["content"].is_string());

        // A text-only model is told it can't see the photo.
        let blind = with_files(&history, 16384, false);
        assert!(blind[2].images.is_empty());
        assert!(blind[2].content.contains("can't see images"));
        assert!(base_messages("sys", &blind)[3]["content"].is_string());
    }

    #[test]
    fn attached_files_survive_the_wire() {
        let json = serde_json::json!({ "role": "user", "content": "hi", "files": [{ "name": "a.txt", "kind": "text", "text": "hello" }] });
        let m: ChatMessage = serde_json::from_value(json).unwrap();
        assert_eq!((m.files.len(), m.files[0].kind), (1, crate::files::FileKind::Text));
        // Messages without files serialize as before.
        assert_eq!(serde_json::to_value(ChatMessage::new("user", "x")).unwrap(), serde_json::json!({ "role": "user", "content": "x" }));
    }

    #[test]
    fn sse_parser_handles_split_chunks() {
        let mut p = SseParser::default();
        assert!(p.push(b"data: {\"a\":").is_empty());
        let out = p.push(b"1}\n\ndata: [DONE]\n\n");
        assert_eq!(out, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn sse_parser_ignores_comments_and_crlf() {
        let mut p = SseParser::default();
        let out = p.push(b": keep-alive\r\ndata: x\r\n\r\n");
        assert_eq!(out, vec!["x".to_string()]);
    }

    #[test]
    fn parses_reasoning_content_and_finish() {
        let d = parse_payload(r#"{"choices":[{"delta":{"reasoning_content":"hmm"},"finish_reason":null}]}"#);
        assert_eq!(d, vec![Delta::Reasoning("hmm".into())]);
        let d = parse_payload(r#"{"choices":[{"delta":{"content":"Hi"},"finish_reason":"stop"}]}"#);
        assert_eq!(d, vec![Delta::Content("Hi".into()), Delta::Finish("stop".into())]);
        assert_eq!(parse_payload("[DONE]"), vec![Delta::End]);
    }

    #[test]
    fn parses_timings() {
        let d = parse_payload(r#"{"choices":[{"delta":{},"finish_reason":"stop"}],"timings":{"prompt_n":12,"prompt_ms":40.0,"predicted_n":100,"predicted_ms":5000.0,"predicted_per_second":20.0}}"#);
        let Delta::Timings(s) = &d[1] else { panic!("{d:?}") };
        assert_eq!(s.completion_tokens, 100);
        assert_eq!(s.tokens_per_second, 20.0);
        assert_eq!(s.total_ms, 5040.0);
    }

    #[test]
    fn sse_parser_keeps_split_utf8_intact() {
        let mut p = SseParser::default();
        let bytes = "data: café ✓\n".as_bytes();
        let (a, b) = bytes.split_at(10); // splits inside the 'é' or '✓' sequence
        assert!(p.push(a).is_empty());
        assert_eq!(p.push(b), vec!["café ✓".to_string()]);
    }

    #[test]
    fn fit_history_drops_oldest_and_keeps_latest() {
        let msg = |r: &str, n: usize| ChatMessage::new(r, "x".repeat(n));
        let h = vec![msg("user", 7000), msg("assistant", 7000), msg("user", 700), msg("assistant", 700), msg("user", 35)];
        let kept = fit_history(&h, "sys", 2048, 1024);
        assert_eq!(kept.last().unwrap().content.len(), 35);
        assert!(kept.len() < h.len());
        assert_eq!(kept[0].role, "user");
        // A single huge message is still sent (the engine reports overflow).
        let kept = fit_history(&[msg("user", 100_000)], "sys", 2048, 1024);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn parses_streamed_tool_call_fragments() {
        let d = parse_payload(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"web_search","arguments":"{\"qu"}}]}}]}"#);
        assert_eq!(d, vec![Delta::ToolCall { index: 0, id: Some("call_1".into()), name: Some("web_search".into()), args: "{\"qu".into() }]);
        let d = parse_payload(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ery\":\"x\"}"}}]},"finish_reason":"tool_calls"}]}"#);
        assert_eq!(d[0], Delta::ToolCall { index: 0, id: None, name: None, args: "ery\":\"x\"}".into() });
        assert_eq!(d[1], Delta::Finish("tool_calls".into()));
    }

    #[test]
    fn parses_errors() {
        assert_eq!(parse_payload(r#"{"error":{"message":"context too long"}}"#), vec![Delta::Error("context too long".into())]);
    }

    #[test]
    fn body_toggles_thinking_and_budget() {
        let hist = vec![ChatMessage::new("user", "hi")];
        let b = request_body("sys", &hist, TurnPlan { thinking: true, thinking_budget: 1024, max_tokens: 2000, mode: crate::settings::Mode::Auto, profile: Default::default() });
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], true);
        assert_eq!(b["reasoning_budget_tokens"], 1024);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "hi");
        let b = request_body("sys", &hist, TurnPlan { thinking: false, thinking_budget: 0, max_tokens: 500, mode: crate::settings::Mode::Auto, profile: Default::default() });
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], false);
        assert!(b.get("reasoning_budget_tokens").is_none());
        assert_eq!(b["temperature"], 0.7);
    }
}


/// Helpers shared by the ignored end-to-end tests.
#[cfg(test)]
pub mod e2e_support {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use tauri::ipc::InvokeResponseBody;

    pub struct Server(std::process::Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }

    pub fn collecting_channel() -> (Channel<ChatEvent>, Arc<StdMutex<Vec<serde_json::Value>>>) {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let sink = seen.clone();
        let ch = Channel::new(move |body: InvokeResponseBody| {
            if let InvokeResponseBody::Json(s) = body {
                sink.lock().unwrap().push(serde_json::from_str(&s).unwrap());
            }
            Ok(())
        });
        (ch, seen)
    }

    /// Starts llama-server from BYTE_TEST_LLAMA_SERVER with BYTE_TEST_MODEL,
    /// using BYTE's real arguments. Returns None (skip) if they aren't set.
    pub async fn start_server() -> Option<(Server, Endpoint)> {
        let model = std::env::var("BYTE_TEST_MODEL").ok()?;
        start_server_with(&model, &[], None).await
    }

    /// Like `start_server`, with a specific model file, tuned options and extra arguments.
    pub async fn start_server_with(model: &str, extra: &[String], opts: Option<&crate::engine::LaunchOpts>) -> Option<(Server, Endpoint)> {
        let Ok(bin) = std::env::var("BYTE_TEST_LLAMA_SERVER") else {
            eprintln!("skipping: set BYTE_TEST_LLAMA_SERVER and BYTE_TEST_MODEL");
            return None;
        };
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let key = "test-key";
        let mut args = crate::engine::server_args(std::path::Path::new(model), port, key, "test", 4096);
        if let Some(o) = opts {
            crate::engine::apply_opts(&mut args, o);
        }
        args.extend_from_slice(extra);
        let child = std::process::Command::new(&bin)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn llama-server");
        let server = Server(child);
        let http = local_client();
        let base = format!("http://127.0.0.1:{port}");
        // Loading can be slow on a busy CI machine; allow up to 3 minutes.
        for _ in 0..900 {
            if let Ok(r) = http.get(format!("{base}/health")).send().await {
                if r.status().is_success() {
                    return Some((server, Endpoint { base_url: base, api_key: key.into(), model: "test".into(), context: 4096, vision: false, cloud: None }));
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        panic!("server never became healthy (flags rejected?)");
    }
}

/// End-to-end check against a real `llama-server`. Run with:
/// `BYTE_TEST_LLAMA_SERVER=/path/llama-server BYTE_TEST_MODEL=/path/model.gguf cargo test e2e -- --ignored --nocapture`
#[cfg(test)]
mod e2e {
    use super::e2e_support::collecting_channel;
    use super::*;
    use crate::router::plan_turn;
    use crate::settings::{Mode, ThinkingPref};

    fn text_of(events: &[serde_json::Value], kind: &str) -> String {
        events.iter().filter(|e| e["kind"] == kind).filter_map(|e| e["delta"].as_str()).collect()
    }

    #[tokio::test]
    #[ignore]
    async fn e2e_streams_from_real_llama_server() {
        let Some((_server, ep)) = e2e_support::start_server().await else { return };
        let http = local_client();
        let base = ep.base_url.clone();
        // Requests without the API key must be refused.
        let unauth = http.post(format!("{base}/v1/chat/completions")).json(&serde_json::json!({"messages":[]})).send().await.unwrap();
        assert_eq!(unauth.status(), 401);

        let sys = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, false, None);
        let hist = vec![ChatMessage::new("user", "What is 12 + 30? Reply with just the number.")];

        // Thinking on, with a budget: reasoning arrives separately from the answer.
        let mut plan = plan_turn(Mode::Auto, ThinkingPref::On, &hist[0].content);
        plan.thinking_budget = 256;
        plan.max_tokens = 600;
        let (ch, seen) = collecting_channel();
        stream(&http, &ep, request_body(&sys, &hist, plan), plan, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        let reasoning = text_of(&ev, "reasoning");
        let content = text_of(&ev, "content");
        eprintln!("THINKING ON  reasoning={} chars content={content:?}", reasoning.len());
        assert!(!reasoning.is_empty(), "expected reasoning_content deltas");
        assert!(!content.contains("<think>"), "thinking leaked into content: {content}");
        assert!(content.contains("42"), "{content}");
        let stats = ev.iter().find(|e| e["kind"] == "stats").expect("stats event");
        assert!(stats["completionTokens"].as_u64().unwrap() > 0);
        assert!(ev.last().unwrap()["kind"] == "done");

        // Thinking off: no reasoning at all.
        let plan = plan_turn(Mode::Fast, ThinkingPref::Off, &hist[0].content);
        let (ch, seen) = collecting_channel();
        stream(&http, &ep, request_body(&sys, &hist, plan), plan, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        let content = text_of(&ev, "content");
        eprintln!("THINKING OFF content={content:?} stats={:?}", ev.iter().find(|e| e["kind"] == "stats"));
        assert!(text_of(&ev, "reasoning").trim().is_empty());
        assert!(content.contains("42"), "{content}");

        // Cancellation stops promptly.
        let long = vec![ChatMessage::new("user", "Write a 2000-word essay about the ocean.")];
        let plan = plan_turn(Mode::Extended, ThinkingPref::Off, &long[0].content);
        let (ch, seen) = collecting_channel();
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            c2.cancel();
        });
        let started = std::time::Instant::now();
        let r = stream(&http, &ep, request_body(&sys, &long, plan), plan, cancel, &ch).await;
        assert!(matches!(r, Err(AppError::Cancelled)), "{r:?}");
        assert!(started.elapsed().as_secs() < 5);
        assert!(!text_of(&seen.lock().unwrap(), "content").is_empty());
    }
}

#[cfg(test)]
mod wire_format {
    use super::*;

    /// The UI depends on these exact JSON shapes.
    #[test]
    fn chat_events_serialize_as_camel_case() {
        let done = serde_json::to_value(ChatEvent::Done { finish_reason: "stop".into() }).unwrap();
        assert_eq!(done, serde_json::json!({ "kind": "done", "finishReason": "stop" }));
        let stats = serde_json::to_value(ChatEvent::Stats(Stats { tokens_per_second: 1.5, ..Default::default() })).unwrap();
        assert_eq!(stats["kind"], "stats");
        assert_eq!(stats["tokensPerSecond"], 1.5);
        let d = crate::models::DownloadEvent::Progress { id: "m".into(), bytes: 1, total: 2, bytes_per_sec: 3.0 };
        assert_eq!(serde_json::to_value(d).unwrap()["bytesPerSec"], 3.0);
        let s = crate::engine::EngineStatus::Ready { model: "m".into(), context: 4096, boosted: false, vision: false };
        assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!({ "state": "ready", "model": "m", "context": 4096, "boosted": false, "vision": false }));
        assert_eq!(serde_json::to_value(crate::engine::EngineStatus::NoModel).unwrap(), serde_json::json!({ "state": "noModel" }));
        // Web agent events (lib/types.ts ApprovalAsk / SavedFile): no field may clash with the "kind" tag.
        let ask = crate::web_agent::ApprovalAsk { id: "a1".into(), action: "submit".into(), title: "Submit?".into(), site: "example.com".into(), url: "https://example.com/".into(), target: "Send".into(), fields: vec![] };
        let v = serde_json::to_value(ChatEvent::Approval(ask)).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "approval", "id": "a1", "action": "submit", "title": "Submit?", "site": "example.com", "url": "https://example.com/", "target": "Send", "fields": [] }));
        let f = crate::web_agent::SavedFile { path: "/d/a.pdf".into(), name: "a.pdf".into(), format: "pdf".into(), bytes: 3, url: "https://x.com".into() };
        let v = serde_json::to_string(&ChatEvent::Saved(f)).unwrap();
        assert_eq!(v.matches("\"kind\"").count(), 1, "{v}");
        assert!(v.contains("\"format\":\"pdf\""));
        assert_eq!(serde_json::to_value(ChatEvent::ApprovalDone { id: "a1".into(), ok: true }).unwrap(), serde_json::json!({ "kind": "approvalDone", "id": "a1", "ok": true }));
        assert_eq!(serde_json::to_value(ChatEvent::Browsing { active: true }).unwrap(), serde_json::json!({ "kind": "browsing", "active": true }));
    }
}
