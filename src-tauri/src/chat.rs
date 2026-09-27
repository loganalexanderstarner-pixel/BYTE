//! Streams chat completions from the local engine to the UI.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::engine::Endpoint;
use crate::error::{AppError, AppResult};
use crate::router::TurnPlan;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum ChatEvent {
    Started { thinking: bool, model: String },
    Reasoning { delta: String },
    Content { delta: String },
    Stats(Stats),
    Done { finish_reason: String },
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

pub fn request_body(system: &str, history: &[ChatMessage], plan: TurnPlan) -> serde_json::Value {
    let mut messages = vec![serde_json::json!({ "role": "system", "content": system })];
    messages.extend(history.iter().map(|m| serde_json::json!({ "role": m.role, "content": m.content })));
    // Qwen3's recommended sampling for thinking vs. non-thinking turns.
    let (temperature, top_p) = if plan.thinking { (0.6, 0.95) } else { (0.7, 0.8) };
    let mut body = serde_json::json!({
        "messages": messages,
        "stream": true,
        "max_tokens": plan.max_tokens,
        "temperature": temperature,
        "top_p": top_p,
        "top_k": 20,
        "min_p": 0.0,
        "cache_prompt": true,
        "timings_per_token": false,
        "chat_template_kwargs": { "enable_thinking": plan.thinking },
    });
    if plan.thinking && plan.thinking_budget > 0 {
        body["reasoning_budget_tokens"] = plan.thinking_budget.into();
        body["reasoning_budget_message"] = "\n\nI've thought enough; answering now.".into();
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
        let t = estimate_tokens(&m.content);
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
        }));
    }
    out
}

pub async fn stream(
    http: &reqwest::Client,
    ep: &Endpoint,
    body: serde_json::Value,
    plan: TurnPlan,
    cancel: CancellationToken,
    events: &Channel<ChatEvent>,
) -> AppResult<()> {
    let send = |e: ChatEvent| events.send(e).map_err(|e| AppError::msg(format!("UI channel closed: {e}")));
    send(ChatEvent::Started { thinking: plan.thinking, model: ep.model.clone() })?;

    let started = Instant::now();
    let url = format!("{}/v1/chat/completions", ep.base_url);
    let mut attempt = 0;
    let resp = loop {
        attempt += 1;
        let sent = tokio::select! {
            r = http.post(&url).bearer_auth(&ep.api_key).json(&body).send() => r,
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

    let mut parser = SseParser::default();
    let mut bytes = resp.bytes_stream();
    let mut first_content: Option<Instant> = None;
    let mut saw_reasoning = false;
    let mut finish = String::from("stop");
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
                        saw_reasoning = true;
                        send(ChatEvent::Reasoning { delta: t })?
                    }
                    Delta::Content(t) => {
                        first_content.get_or_insert_with(Instant::now);
                        send(ChatEvent::Content { delta: t })?
                    }
                    Delta::Finish(f) => finish = f,
                    Delta::Timings(mut s) => {
                        if saw_reasoning {
                            if let Some(fc) = first_content {
                                s.thinking_ms = (fc - started).as_secs_f64() * 1000.0;
                            }
                        }
                        send(ChatEvent::Stats(s))?
                    }
                    Delta::Error(m) => return Err(AppError::msg(m)),
                    Delta::End => {}
                }
            }
        }
    }
    send(ChatEvent::Done { finish_reason: finish })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let msg = |r: &str, n: usize| ChatMessage { role: r.into(), content: "x".repeat(n) };
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
    fn parses_errors() {
        assert_eq!(parse_payload(r#"{"error":{"message":"context too long"}}"#), vec![Delta::Error("context too long".into())]);
    }

    #[test]
    fn body_toggles_thinking_and_budget() {
        let hist = vec![ChatMessage { role: "user".into(), content: "hi".into() }];
        let b = request_body("sys", &hist, TurnPlan { thinking: true, thinking_budget: 1024, max_tokens: 2000 });
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], true);
        assert_eq!(b["reasoning_budget_tokens"], 1024);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "hi");
        let b = request_body("sys", &hist, TurnPlan { thinking: false, thinking_budget: 0, max_tokens: 500 });
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], false);
        assert!(b.get("reasoning_budget_tokens").is_none());
        assert_eq!(b["temperature"], 0.7);
    }
}

/// End-to-end check against a real `llama-server`. Run with:
/// `BYTE_TEST_LLAMA_SERVER=/path/llama-server BYTE_TEST_MODEL=/path/model.gguf cargo test e2e -- --ignored --nocapture`
#[cfg(test)]
mod e2e {
    use super::*;
    use crate::router::plan_turn;
    use crate::settings::{Mode, ThinkingPref};
    use std::sync::Mutex as StdMutex;
    use tauri::ipc::InvokeResponseBody;

    struct Server(std::process::Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }

    fn collecting_channel() -> (Channel<ChatEvent>, Arc<StdMutex<Vec<serde_json::Value>>>) {
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

    fn text_of(events: &[serde_json::Value], kind: &str) -> String {
        events.iter().filter(|e| e["kind"] == kind).filter_map(|e| e["delta"].as_str()).collect()
    }

    #[tokio::test]
    #[ignore]
    async fn e2e_streams_from_real_llama_server() {
        let (Ok(bin), Ok(model)) = (std::env::var("BYTE_TEST_LLAMA_SERVER"), std::env::var("BYTE_TEST_MODEL")) else {
            eprintln!("skipping: set BYTE_TEST_LLAMA_SERVER and BYTE_TEST_MODEL");
            return;
        };
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let key = "test-key";
        let args = crate::engine::server_args(std::path::Path::new(&model), port, key, "test", 4096);
        let child = std::process::Command::new(&bin)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn llama-server");
        let _guard = Server(child);
        let http = local_client();
        let base = format!("http://127.0.0.1:{port}");
        let mut healthy = false;
        for _ in 0..300 {
            if let Ok(r) = http.get(format!("{base}/health")).send().await {
                if r.status().is_success() {
                    healthy = true;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        assert!(healthy, "server never became healthy (flags rejected?)");

        // Requests without the API key must be refused.
        let unauth = http.post(format!("{base}/v1/chat/completions")).json(&serde_json::json!({"messages":[]})).send().await.unwrap();
        assert_eq!(unauth.status(), 401);

        let ep = Endpoint { base_url: base.clone(), api_key: key.into(), model: "test".into(), context: 4096 };
        let sys = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, false);
        let hist = vec![ChatMessage { role: "user".into(), content: "What is 12 + 30? Reply with just the number.".into() }];

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
        let long = vec![ChatMessage { role: "user".into(), content: "Write a 2000-word essay about the ocean.".into() }];
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
        let s = crate::engine::EngineStatus::Ready { model: "m".into(), context: 4096 };
        assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!({ "state": "ready", "model": "m", "context": 4096 }));
        assert_eq!(serde_json::to_value(crate::engine::EngineStatus::NoModel).unwrap(), serde_json::json!({ "state": "noModel" }));
    }
}
