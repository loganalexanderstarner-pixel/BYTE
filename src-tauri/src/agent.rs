//! The agent loop: lets the model call tools (web search, page reading,
//! calculator) over several rounds, then writes the final cited answer.
//!
//! Each round is one engine request. If the model asks for tools, BYTE runs
//! them, appends the results to the conversation and asks again. Limits come
//! from the mode, and tools are withdrawn once the context gets full so the
//! model always has room to answer.

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::chat::{self, ChatEvent, ChatMessage, Stats};
use crate::engine::Endpoint;
use crate::error::{AppError, AppResult};
use crate::router::TurnPlan;
use crate::settings::Mode;
use crate::tools::{self, ActionLog, SourceBook, ToolContext};

/// How much tool use each mode allows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Rounds in which the model may call tools before it must answer.
    pub tool_rounds: usize,
    pub max_results: usize,
    /// Characters of each read page given to the model.
    pub page_chars: usize,
    /// Top results BYTE reads automatically for time-sensitive questions.
    pub auto_read: usize,
}

pub fn limits(mode: Mode) -> Limits {
    match mode {
        Mode::Fast => Limits { tool_rounds: 1, max_results: 5, page_chars: 2500, auto_read: 0 },
        Mode::Auto => Limits { tool_rounds: 3, max_results: 6, page_chars: 4000, auto_read: 1 },
        Mode::Deep => Limits { tool_rounds: 6, max_results: 8, page_chars: 5000, auto_read: 2 },
        Mode::Extended => Limits { tool_rounds: 10, max_results: 8, page_chars: 5000, auto_read: 3 },
    }
}

/// Stop offering tools once the conversation uses this share of the context.
const TOOL_CONTEXT_SHARE: f64 = 0.70;

pub struct Turn<'a> {
    pub http: &'a reqwest::Client,
    pub net: &'a reqwest::Client,
    pub ep: &'a Endpoint,
    pub system: &'a str,
    pub history: &'a [ChatMessage],
    pub plan: TurnPlan,
    pub mode: Mode,
    pub web: bool,
    /// Offer the `remember` tool (memory is on).
    pub memory: bool,
    pub log: &'a ActionLog,
}

/// Web search is required on the first round when the question is clearly
/// time-sensitive, so the model can't answer from stale training data.
fn must_search_first(turn: &Turn<'_>, question: &str) -> bool {
    turn.web && crate::router::needs_fresh_info(question)
}

/// Records a tool call BYTE made on the model's behalf, plus its result, in
/// the conversation sent to the model.
fn push_tool_exchange(messages: &mut Vec<Value>, call_id: &str, name: &str, args: &Value, content: String) {
    messages.push(json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [{ "id": call_id, "type": "function", "function": { "name": name, "arguments": args.to_string() } }],
    }));
    messages.push(json!({ "role": "tool", "tool_call_id": call_id, "content": content }));
}

pub async fn run(turn: Turn<'_>, cancel: CancellationToken, events: &Channel<ChatEvent>) -> AppResult<()> {
    let send = |e: ChatEvent| events.send(e).map_err(|e| AppError::msg(format!("UI channel closed: {e}")));
    send(ChatEvent::Started { thinking: turn.plan.thinking, model: turn.ep.model.clone() })?;

    let lim = limits(turn.mode);
    let question = turn.history.iter().rev().find(|m| m.role == "user").map(|m| m.content.clone()).unwrap_or_default();
    let ctx = ToolContext { net: turn.net, question: &question, max_results: lim.max_results, page_chars: lim.page_chars, log: turn.log };
    let specs = tools::specs(turn.web, turn.memory);
    let mut messages = chat::base_messages(turn.system, turn.history);
    let mut book = SourceBook::default();
    let mut totals = Stats::default();
    let mut rounds_with_stats = 0u32;
    let mut wrote_content = false;
    let mut finish = String::from("stop");

    // Arithmetic: BYTE runs the calculator itself. Small models often skip the
    // tool and do the sum in their head (1234 * 5678 came out 7,112,932).
    if let Some(expr) = crate::router::math_expression(&question) {
        if tools::calc::calculate(&expr).is_ok() {
            let call_id = "byte_calc_0".to_string();
            let args = json!({ "expression": expr });
            send(ChatEvent::ToolCall { id: call_id.clone(), name: tools::CALCULATE.into(), args: args.clone() })?;
            let out = tools::run(&ctx, &mut book, tools::CALCULATE, &args).await;
            send(ChatEvent::ToolResult { id: call_id.clone(), ok: out.ok, summary: out.summary.clone() })?;
            push_tool_exchange(&mut messages, &call_id, tools::CALCULATE, &args, out.content);
        }
    }

    // Time-sensitive questions: BYTE runs the first search itself instead of
    // trusting the model to decide (small models often answer from memory).
    if must_search_first(&turn, &question) {
        let call_id = "byte_search_0".to_string();
        let args = json!({ "query": search_query(&question) });
        send(ChatEvent::ToolCall { id: call_id.clone(), name: tools::WEB_SEARCH.into(), args: args.clone() })?;
        let out = tokio::select! {
            o = tools::run(&ctx, &mut book, tools::WEB_SEARCH, &args) => o,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        send(ChatEvent::ToolResult { id: call_id.clone(), ok: out.ok, summary: out.summary.clone() })?;
        if !book.sources.is_empty() {
            send(ChatEvent::Sources { sources: book.sources.clone() })?;
        }
        push_tool_exchange(&mut messages, &call_id, tools::WEB_SEARCH, &args, out.content);

        // Snippets rarely hold the full answer: read the top results too, in parallel.
        let urls: Vec<String> = book.sources.iter().take(lim.auto_read).map(|s| s.url.clone()).collect();
        if !urls.is_empty() {
            let ids: Vec<String> = (0..urls.len()).map(|i| format!("byte_read_{i}")).collect();
            for (id, url) in ids.iter().zip(&urls) {
                send(ChatEvent::ToolCall { id: id.clone(), name: tools::READ_PAGE.into(), args: json!({ "url": url }) })?;
            }
            let reads = futures_util::future::join_all(urls.iter().map(|u| tools::fetch::fetch_page(turn.net, u)));
            let pages = tokio::select! {
                p = reads => p,
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
            };
            let mut calls = Vec::new();
            let mut results = Vec::new();
            for ((id, url), page) in ids.iter().zip(&urls).zip(pages) {
                let args = json!({ "url": url });
                let (ok, summary, content) = match page {
                    Ok(p) => {
                        let n = book.mark_read(&p.url, &p.title);
                        let text = tools::fetch::relevant_passages(&p.text, &question, lim.page_chars);
                        (true, p.title.chars().take(60).collect::<String>(), format!("[{n}] {}\n{}\n\n{text}", p.title, p.url))
                    }
                    Err(e) => (false, e.to_string(), format!("Couldn't read {url}: {e}")),
                };
                turn.log.record(tools::READ_PAGE, &args, ok, &summary);
                send(ChatEvent::ToolResult { id: id.clone(), ok, summary })?;
                calls.push(json!({ "id": id, "type": "function", "function": { "name": tools::READ_PAGE, "arguments": args.to_string() } }));
                results.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
            }
            send(ChatEvent::Sources { sources: book.sources.clone() })?;
            messages.push(json!({ "role": "assistant", "content": "", "tool_calls": calls }));
            messages.extend(results);
        }
    }

    for round in 0..=lim.tool_rounds {
        let used = estimate(&messages);
        let allow_tools = round < lim.tool_rounds && (used as f64) < turn.ep.context as f64 * TOOL_CONTEXT_SHARE;
        let body = chat::build_body(messages.clone(), turn.plan, allow_tools.then(|| specs.clone()));

        // Separate text from different rounds with a blank line.
        let mut first_in_round = true;
        let mut forward = |e: ChatEvent| -> AppResult<()> {
            if matches!(e, ChatEvent::Content { .. }) {
                if first_in_round && wrote_content {
                    send(ChatEvent::Content { delta: "\n\n".into() })?;
                }
                first_in_round = false;
                wrote_content = true;
            }
            send(e)
        };
        let r = chat::stream_round(turn.http, turn.ep, &body, &cancel, &mut forward).await?;
        if let Some(s) = &r.stats {
            totals.prompt_tokens += s.prompt_tokens;
            totals.completion_tokens += s.completion_tokens;
            totals.prompt_ms += s.prompt_ms;
            totals.total_ms += s.total_ms;
            totals.draft_tokens += s.draft_tokens;
            totals.draft_accepted += s.draft_accepted;
            totals.tokens_per_second = s.tokens_per_second;
            rounds_with_stats += 1;
        }
        totals.thinking_ms += r.thinking_ms;
        finish = r.finish.clone();

        if r.tool_calls.is_empty() || !allow_tools {
            break;
        }

        // Record the assistant's tool request, then each result.
        let calls: Vec<Value> = r
            .tool_calls
            .iter()
            .enumerate()
            .map(|(i, c)| {
                json!({
                    "id": if c.id.is_empty() { format!("call_{round}_{i}") } else { c.id.clone() },
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments },
                })
            })
            .collect();
        messages.push(json!({ "role": "assistant", "content": r.content, "tool_calls": calls }));
        for call in &calls {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let id = call["id"].as_str().unwrap_or_default().to_string();
            let name = call["function"]["name"].as_str().unwrap_or_default().to_string();
            let raw = call["function"]["arguments"].as_str().unwrap_or("{}");
            let args: Value = serde_json::from_str(raw).unwrap_or_else(|_| json!({ "raw": raw }));
            send(ChatEvent::ToolCall { id: id.clone(), name: name.clone(), args: args.clone() })?;
            let before = book.sources.len();
            let out = tokio::select! {
                o = tools::run(&ctx, &mut book, &name, &args) => o,
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
            };
            send(ChatEvent::ToolResult { id: id.clone(), ok: out.ok, summary: out.summary.clone() })?;
            if book.sources.len() != before || name == tools::READ_PAGE {
                send(ChatEvent::Sources { sources: book.sources.clone() })?;
            }
            messages.push(json!({ "role": "tool", "tool_call_id": id, "content": out.content }));
        }
    }

    if rounds_with_stats > 0 {
        send(ChatEvent::Stats(totals))?;
    }
    send(ChatEvent::Done { finish_reason: finish })?;
    Ok(())
}

/// Turns a chat message into a search query: drops "search the web:"-style
/// prefixes and keeps it short.
pub fn search_query(question: &str) -> String {
    let mut q = question.trim().to_string();
    let lower = q.to_lowercase();
    for prefix in ["search the web for", "search the web:", "search the web", "look up", "google", "search for", "search:"] {
        if lower.starts_with(prefix) {
            q = q[prefix.len()..].trim_start_matches([':', ',', ' ']).to_string();
            break;
        }
    }
    let q: String = q.chars().take(200).collect();
    q.trim().trim_end_matches(['?', '.', '!']).trim().to_string()
}

fn estimate(messages: &[Value]) -> usize {
    messages.iter().map(|m| chat::estimate_tokens(&m.to_string())).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::e2e_support::{collecting_channel, start_server, start_server_with};

    async fn start_server_with_ctx(ctx: u32) -> Option<(crate::chat::e2e_support::Server, crate::engine::Endpoint)> {
        let model = std::env::var("BYTE_TEST_MODEL").ok()?;
        start_server_with(&model, &["-c".into(), ctx.to_string()], None).await
    }
    use crate::settings::ThinkingPref;

    #[test]
    fn tool_exchange_is_a_call_then_its_result() {
        let mut messages = Vec::new();
        push_tool_exchange(&mut messages, "byte_calc_0", tools::CALCULATE, &json!({ "expression": "6 * 7" }), "6 * 7 = 42".into());
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "calculate");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "byte_calc_0");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "byte_calc_0");
        assert_eq!(messages[1]["content"], "6 * 7 = 42");
    }

    /// Real engine: the model must call the calculator and use its result.
    /// Run with BYTE_TEST_LLAMA_SERVER and BYTE_TEST_MODEL set (see chat.rs).
    #[tokio::test]
    #[ignore]
    async fn e2e_agent_uses_calculator_tool() {
        let Some((_server, ep)) = start_server().await else { return };
        let dir = tempfile::tempdir().unwrap();
        let log = ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let history = vec![ChatMessage { role: "user".into(), content: "Use the calculator tool to compute 1234 * 5678, then tell me the result.".into() }];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, false, None);
        let plan = crate::router::plan_turn(Mode::Auto, ThinkingPref::Off, &history[0].content);
        let (ch, seen) = collecting_channel();
        let turn = Turn { http: &http, net: &http, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: false, memory: false, log: &log };
        run(turn, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        let calls: Vec<_> = ev.iter().filter(|e| e["kind"] == "toolCall").collect();
        eprintln!("tool calls: {calls:?}");
        assert!(!calls.is_empty(), "model never called a tool: {ev:?}");
        assert_eq!(calls[0]["name"], "calculate");
        let result = ev.iter().find(|e| e["kind"] == "toolResult").unwrap();
        assert_eq!(result["ok"], true);
        let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
        eprintln!("answer: {content}");
        assert!(content.replace(',', "").contains("7006652"), "{content}");
        assert_eq!(ev.last().unwrap()["kind"], "done");
        assert!(std::fs::read_to_string(dir.path().join("a.jsonl")).unwrap().contains("calculate"));
    }

    /// Real engine + real internet (also needs BYTE_TEST_WEB=1).
    #[tokio::test]
    #[ignore]
    async fn e2e_agent_searches_web_and_cites() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let Some((_server, ep)) = start_server().await else { return };
        let dir = tempfile::tempdir().unwrap();
        let log = ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = tools::fetch::web_client();
        let history = vec![ChatMessage { role: "user".into(), content: "Search the web: what is the latest stable version of the Rust programming language?".into() }];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, true, None);
        let plan = crate::router::plan_turn(Mode::Auto, ThinkingPref::Off, &history[0].content);
        let (ch, seen) = collecting_channel();
        let turn = Turn { http: &http, net: &net, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: true, memory: false, log: &log };
        run(turn, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        let mut counts = std::collections::BTreeMap::new();
        for e in &ev {
            *counts.entry(e["kind"].as_str().unwrap_or("?").to_string()).or_insert(0) += 1;
        }
        eprintln!("event kinds: {counts:?}");
        for e in ev.iter().filter(|e| e["kind"] == "toolCall" || e["kind"] == "toolResult" || e["kind"] == "stats") {
            eprintln!("{e}");
        }
        let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
        eprintln!("answer: {content}");
        assert!(ev.iter().any(|e| e["kind"] == "toolCall" && e["name"] == "web_search"), "never searched");
        assert!(ev.iter().any(|e| e["kind"] == "sources"), "no sources emitted");
        assert!(!content.trim().is_empty());
    }

    /// Web-search quality check on everyday questions (real engine + real internet).
    /// Prints what BYTE searched, read and answered for each question. Needs
    /// BYTE_TEST_LLAMA_SERVER, BYTE_TEST_MODEL and BYTE_TEST_WEB=1; optional
    /// BYTE_TEST_QUESTIONS (one per line) replaces the built-in list.
    #[tokio::test]
    #[ignore]
    async fn e2e_web_quality_report() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let Some((_server, mut ep)) = start_server_with_ctx(16384).await else { return };
        ep.context = 16384;
        let dir = tempfile::tempdir().unwrap();
        let log = ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = tools::fetch::web_client();
        let questions: Vec<String> = std::env::var("BYTE_TEST_QUESTIONS")
            .map(|q| q.lines().map(str::to_string).filter(|l| !l.trim().is_empty()).collect())
            .unwrap_or_else(|_| WEB_QUESTIONS.iter().map(|s| s.to_string()).collect());
        let mode = match std::env::var("BYTE_TEST_MODE").as_deref() {
            Ok("deep") => Mode::Deep,
            Ok("fast") => Mode::Fast,
            _ => Mode::Auto,
        };
        for q in questions {
            let history = vec![ChatMessage { role: "user".into(), content: q.clone() }];
            let system = crate::prompt::system_prompt(chrono::Local::now(), mode, true, None);
            let plan = crate::router::plan_turn(mode, ThinkingPref::Auto, &q);
            let (ch, seen) = collecting_channel();
            let turn = Turn { http: &http, net: &net, ep: &ep, system: &system, history: &history, plan, mode, web: true, memory: false, log: &log };
            let t = std::time::Instant::now();
            let r = run(turn, CancellationToken::new(), &ch).await;
            let ev = seen.lock().unwrap().clone();
            eprintln!("\n=== {q}  ({:.0}s, forced search: {})", t.elapsed().as_secs_f64(), must_search_q(&q));
            for e in &ev {
                match e["kind"].as_str() {
                    Some("toolCall") => eprintln!("  call  {} {}", e["name"], e["args"]),
                    Some("toolResult") => eprintln!("  -> ok={} {}", e["ok"], e["summary"]),
                    _ => {}
                }
            }
            let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
            eprintln!("  answer: {}", content.chars().take(700).collect::<String>().replace('\n', " / "));
            if let Err(e) = r {
                eprintln!("  ERROR: {e}");
            }
        }
    }

    fn must_search_q(q: &str) -> bool {
        crate::router::needs_fresh_info(q)
    }

    /// Everyday questions that need the web to be answered well.
    const WEB_QUESTIONS: &[&str] = &[
        "Who won the most recent Super Bowl?",
        "What's the weather in Pittsburgh this weekend?",
        "Who is the CEO of OpenAI?",
        "How much does a Tesla Model 3 cost?",
        "How do I fix \"xcrun: error: invalid active developer path\" on my Mac?",
        "Is the Steam Deck OLED worth buying?",
        "What time do the Steelers play next?",
        "What is llama.cpp?",
    ];

    #[test]
    fn search_query_cleans_prefixes() {
        assert_eq!(search_query("Search the web: what is the latest Rust version?"), "what is the latest Rust version");
        assert_eq!(search_query("look up weather in Tokyo"), "weather in Tokyo");
        assert_eq!(search_query("Latest iPhone price"), "Latest iPhone price");
        assert!(search_query(&"x".repeat(500)).len() <= 200);
    }

    #[test]
    fn deeper_modes_allow_more_tool_use() {
        let (f, a, d, e) = (limits(Mode::Fast), limits(Mode::Auto), limits(Mode::Deep), limits(Mode::Extended));
        assert!(f.tool_rounds < a.tool_rounds && a.tool_rounds < d.tool_rounds && d.tool_rounds < e.tool_rounds);
        assert!(f.page_chars < e.page_chars);
    }
}
