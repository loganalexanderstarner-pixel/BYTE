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
    /// Pages BYTE reads itself after each search (the model then answers from them).
    pub auto_read: usize,
    /// Searches per answer; more are refused so small models don't loop.
    pub max_searches: usize,
}

pub fn limits(mode: Mode) -> Limits {
    match mode {
        Mode::Fast => Limits { tool_rounds: 1, max_results: 5, page_chars: 2500, auto_read: 1, max_searches: 1 },
        Mode::Auto => Limits { tool_rounds: 3, max_results: 6, page_chars: 3500, auto_read: 3, max_searches: 2 },
        Mode::Deep => Limits { tool_rounds: 6, max_results: 8, page_chars: 4500, auto_read: 4, max_searches: 4 },
        Mode::Extended => Limits { tool_rounds: 10, max_results: 8, page_chars: 5000, auto_read: 5, max_searches: 6 },
    }
}

/// Stop offering tools once the conversation uses this share of the context.
const TOOL_CONTEXT_SHARE: f64 = 0.70;

/// A job the user asked for explicitly (a button), beyond answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Task {
    /// Check the claims in the message (the Fact-check button).
    FactCheck,
    /// Use the browser for this message (the composer's Agent pill).
    Browse,
    /// Tutor mode: teach step by step instead of answering outright (the Tutor pill).
    Tutor,
}

pub struct Turn<'a> {
    pub http: &'a reqwest::Client,
    pub net: &'a reqwest::Client,
    /// The BYTE cloud for web search (None: keyless search only).
    pub cloud: Option<&'a crate::cloud::CloudClient>,
    pub ep: &'a Endpoint,
    pub system: &'a str,
    pub history: &'a [ChatMessage],
    pub plan: TurnPlan,
    pub mode: Mode,
    pub web: bool,
    /// Offer the `remember` tool (memory is on).
    pub memory: bool,
    pub log: &'a ActionLog,
    /// The app, when the knowledge base can be searched ("My files" on, folders indexed).
    pub files: Option<&'a tauri::AppHandle>,
    /// The app, for ranking research passages by meaning (None in tests).
    pub app: Option<&'a tauri::AppHandle>,
    /// A job the user asked for with a button (None: a normal answer).
    pub task: Option<Task>,
    /// The user's town for "near me" questions (Settings), if they gave one.
    pub home: Option<&'a str>,
    /// Research depth from Settings: 0 Normal, 1 More, 2 Max.
    pub depth: u8,
    /// Web "Always": search for every real question, not only ones that seem to need it.
    pub web_always: bool,
    /// The kitchen module is on (recipes, meal plans).
    pub kitchen: bool,
    /// Recipes in metric (g, mL, °C) instead of US cups and spoons (°F).
    pub metric: bool,
    /// The web agent module is on (BYTE may use a browser for the user).
    pub agent: bool,
    /// Which of the smaller research modules are on.
    pub modules: Modules,
}

/// Research modules the user can switch off (Settings → Features). Tests
/// leave them all off (`Modules::default()`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modules {
    pub reviews: bool,
    pub prices: bool,
    pub game_hints: bool,
    /// Check cited answers against their sources (Deep, Extended, fact-check).
    pub self_check: bool,
    /// Three drafts and a majority vote for hard questions (Deep, Extended).
    pub best_of_three: bool,
    /// Flashcards, quizzes and tutor mode.
    pub study: bool,
    /// The model is small (≤ 4B, or unknown): cards get checked and repaired (`quality`).
    pub small_model: bool,
    /// "Translate … into …": part by part, streamed (translate.rs).
    pub translate: bool,
    /// Mac control: notes, reminders, calendar, music, settings (macctl.rs).
    pub mac: bool,
    /// Mac upkeep: storage, health, uninstalling, login items (upkeep.rs).
    pub upkeep: bool,
    /// The to-do list, schedules and the daily briefing (tasks.rs, briefing.rs).
    pub tasks: bool,
    /// News feeds and page watchers.
    pub watch: bool,
    /// Automations and multi-step runs (automations.rs).
    pub automations: bool,
    /// Packages, bills, yearly dates, maintenance (trackers.rs).
    pub trackers: bool,
    /// Obsidian, Notion, calendar links (connectors/).
    pub connectors: bool,
}

/// BYTE searches before the model answers any question about the world
/// (`router::wants_web`), so the model can't answer from stale or invented memory.
fn must_search_first(turn: &Turn<'_>, question: &str) -> bool {
    turn.web && crate::router::wants_web_in(question, turn.web_always) && !crate::router::wants_files(question)
}

/// Sent when tools are withdrawn, so the model writes its answer instead of
/// trying to call a tool in plain text.
const ANSWER_NOW: &str = "Write your answer to my question now, using the search results and pages above. Cite them as [n]. If they don't answer it, say what you found and what's still unclear. Don't call any tools.";

pub(crate) type Emit<'a> = &'a (dyn Fn(ChatEvent) -> AppResult<()> + Sync);

/// Reads the best unread results in parallel (skipping sites that can't be
/// read), until `want` pages were read or the candidates run out, and adds
/// them to the conversation as read_page exchanges.
#[allow(clippy::too_many_arguments)]
async fn read_top(
    turn: &Turn<'_>,
    book: &mut SourceBook,
    messages: &mut Vec<Value>,
    question: &str,
    want: usize,
    page_chars: usize,
    // Sources from this index on came from the latest search; they're read first.
    from: usize,
    tag: &str,
    cancel: &CancellationToken,
    send: Emit<'_>,
) -> AppResult<usize> {
    if want == 0 {
        return Ok(0);
    }
    let (earlier, latest) = book.sources.split_at(from.min(book.sources.len()));
    let candidates: Vec<String> = latest
        .iter()
        .chain(earlier)
        .filter(|s| !s.read && tools::fetch::worth_reading(&s.url))
        .map(|s| s.url.clone())
        .take(want * 2)
        .collect();
    let mut calls = Vec::new();
    let mut results = Vec::new();
    let mut read = 0;
    // First the top `want`, then the spares for any that failed.
    let mut rest = candidates.as_slice();
    let mut batch_no = 0;
    while read < want && !rest.is_empty() {
        let take = (want - read).min(rest.len());
        let (batch, tail) = rest.split_at(take);
        rest = tail;
        let ids: Vec<String> = (0..batch.len()).map(|i| format!("byte_read_{tag}_{batch_no}_{i}")).collect();
        batch_no += 1;
        for (id, url) in ids.iter().zip(batch) {
            send(ChatEvent::ToolCall { id: id.clone(), name: tools::READ_PAGE.into(), args: json!({ "url": url }) })?;
        }
        let reads = futures_util::future::join_all(batch.iter().map(|u| tools::fetch::fetch_page(turn.net, u)));
        let pages = tokio::select! {
            p = reads => p,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        for ((id, url), page) in ids.iter().zip(batch).zip(pages) {
            let args = json!({ "url": url });
            let (ok, summary) = match page {
                Ok(p) => {
                    let n = book.mark_read(url, &p.title);
                    let text = tools::fetch::relevant_passages(&p.text, question, page_chars);
                    calls.push(json!({ "id": id, "type": "function", "function": { "name": tools::READ_PAGE, "arguments": args.to_string() } }));
                    results.push(json!({ "role": "tool", "tool_call_id": id, "content": format!("[{n}] {}\n{}\n\n{text}", p.title, p.url) }));
                    read += 1;
                    (true, p.title.chars().take(60).collect::<String>())
                }
                // Failed reads aren't shown to the model; a spare result is tried instead.
                Err(e) => (false, e.to_string()),
            };
            turn.log.record(tools::READ_PAGE, &args, ok, &summary);
            send(ChatEvent::ToolResult { id: id.clone(), ok, summary })?;
        }
    }
    if !calls.is_empty() {
        send(ChatEvent::Sources { sources: book.sources.clone() })?;
        messages.push(json!({ "role": "assistant", "content": "", "tool_calls": calls }));
        messages.extend(results);
    }
    Ok(read)
}

/// Removes tool-call markup a model writes as plain text (when tools aren't
/// offered, some models still try: `<tool_call><function=web_search>…`), so
/// it never reaches the answer. Works on streamed pieces.
#[derive(Default)]
pub struct ToolTextFilter {
    pending: String,
    inside: Option<&'static str>,
    /// Characters removed so far.
    pub dropped: usize,
}

const TOOL_TAGS: &[(&str, &str)] = &[("<tool_call>", "</tool_call>"), ("<function=", "</function>"), ("<tool_response>", "</tool_response>")];

impl ToolTextFilter {
    /// Feeds a streamed piece; returns the text that is safe to show.
    pub fn push(&mut self, piece: &str) -> String {
        self.pending.push_str(piece);
        let mut out = String::new();
        loop {
            if let Some(end) = self.inside {
                match self.pending.find(end) {
                    Some(i) => {
                        self.dropped += i + end.len();
                        self.pending.drain(..i + end.len());
                        self.inside = None;
                    }
                    None => {
                        // Keep only a possible partial end tag.
                        let keep = self.pending.len().min(end.len());
                        let cut = floor_char(&self.pending, self.pending.len() - keep);
                        self.dropped += cut;
                        self.pending.drain(..cut);
                        return out;
                    }
                }
                continue;
            }
            let Some(lt) = self.pending.find('<') else {
                out.push_str(&self.pending);
                self.pending.clear();
                return out;
            };
            out.push_str(&self.pending[..lt]);
            self.pending.drain(..lt);
            if let Some((open, close)) = TOOL_TAGS.iter().find(|(o, _)| self.pending.starts_with(o)) {
                self.dropped += open.len();
                self.pending.drain(..open.len());
                self.inside = Some(close);
                continue;
            }
            if TOOL_TAGS.iter().any(|(o, _)| o.starts_with(self.pending.as_str())) {
                return out; // might be the start of a tag: wait for more
            }
            out.push('<');
            self.pending.drain(..1);
        }
    }

    /// End of the stream: returns held-back text that turned out not to be a tag.
    pub fn finish(&mut self) -> String {
        if self.inside.take().is_some() {
            self.dropped += self.pending.len();
            self.pending.clear();
        }
        // Stray closing tags left behind.
        let rest = std::mem::take(&mut self.pending);
        TOOL_TAGS.iter().fold(rest, |t, (_, c)| t.replace(c, ""))
    }
}

fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The user message before the current one (for follow-up questions).
fn previous_question(history: &[ChatMessage]) -> Option<&str> {
    history.iter().rev().filter(|m| m.role == "user").nth(1).map(|m| m.content.as_str())
}

/// Two searches for the same thing (the model rephrasing instead of reading).
fn same_search(a: &str, b: &str) -> bool {
    let words = |s: &str| -> std::collections::BTreeSet<String> {
        s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(str::to_string).collect()
    };
    let (a, b) = (words(a), words(b));
    let common = a.intersection(&b).count();
    common * 3 >= a.len().max(b.len()) * 2
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
    // The question itself, without the text of attached files (they'd swamp web searches).
    let question = turn.history.iter().rev().find(|m| m.role == "user").map(|m| chat::question_text(&m.content).to_string()).unwrap_or_default();
    let ctx = ToolContext { net: turn.net, cloud: turn.cloud, question: &question, max_results: lim.max_results, page_chars: lim.page_chars, log: turn.log, files: turn.files, home: turn.home };
    // The web agent: BYTE uses a hidden browser for the user ("go to … and …").
    let mut session = match turn.app {
        Some(app) if turn.web && turn.agent && (turn.task == Some(Task::Browse) || crate::web_agent::wants_web_agent(&question)) => Some(open_session(app)?),
        _ => None,
    };
    let specs = match &session {
        Some(_) => {
            let mut v: Vec<Value> = tools::specs(true, false, false, false).into_iter().filter(|s| [tools::WEB_SEARCH, tools::CALCULATE].contains(&s["function"]["name"].as_str().unwrap_or(""))).collect();
            v.extend(crate::web_agent::specs());
            v
        }
        None => tools::specs(turn.web, turn.memory, turn.files.is_some(), crate::research::depth(turn.mode).is_some()),
    };
    let tool_rounds = if session.is_some() { crate::web_agent::MAX_STEPS } else { lim.tool_rounds };
    let mut messages = chat::base_messages(turn.system, turn.history);
    let extra_rules = match (&session, turn.task) {
        (Some(_), _) => Some(crate::web_agent::AGENT_RULES),
        (None, Some(Task::Tutor)) if turn.modules.study => Some(crate::study::TUTOR_RULES),
        _ => None,
    };
    if extra_rules == Some(crate::study::TUTOR_RULES) {
        if let Some(last) = messages.iter_mut().rev().find(|m| m["role"] == "user") {
            if let Some(text) = last["content"].as_str().map(str::to_string) {
                last["content"] = json!(format!("{text}{}", crate::study::TUTOR_NUDGE));
            }
        }
    }
    if let Some(rules) = extra_rules {
        if let Some(sys) = messages.first_mut() {
            let text = format!("{}{rules}", sys["content"].as_str().unwrap_or(""));
            sys["content"] = json!(text);
        }
    }
    let mut book = SourceBook::default();
    let mut totals = Stats::default();
    let mut rounds_with_stats = 0u32;
    let mut wrote_content = false;
    let mut finish: String;

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

    let mut searches: Vec<String> = Vec::new();
    let mut used_tools = messages.len() > chat::base_messages(turn.system, turn.history).len();

    // Weather: BYTE asks a forecast service instead of reading weather sites
    // (they're JavaScript apps with no readable text).
    let mut weather_done = session.is_some();
    if turn.web && session.is_none() {
        if let Some(place) = crate::router::weather_place(&question) {
            let call_id = "byte_weather_0".to_string();
            let args = json!({ "place": place });
            send(ChatEvent::ToolCall { id: call_id.clone(), name: tools::WEATHER.into(), args: args.clone() })?;
            let out = tokio::select! {
                o = tools::run(&ctx, &mut book, tools::WEATHER, &args) => o,
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
            };
            send(ChatEvent::ToolResult { id: call_id.clone(), ok: out.ok, summary: out.summary.clone() })?;
            if out.ok {
                send(ChatEvent::Sources { sources: book.sources.clone() })?;
                weather_done = true;
                used_tools = true;
            }
            push_tool_exchange(&mut messages, &call_id, tools::WEATHER, &args, out.content);
        }
    }

    // "Coffee near me", "pharmacies open now in Shadyside": BYTE looks the
    // places up on OpenStreetMap itself, like the weather.
    let mut places_done = false;
    if turn.web && !weather_done {
        if let Some((what, near)) = crate::router::places_request(&question, turn.home) {
            let call_id = "byte_places_0".to_string();
            let args = json!({ "what": what, "near": near });
            send(ChatEvent::ToolCall { id: call_id.clone(), name: tools::FIND_PLACES.into(), args: args.clone() })?;
            let out = tokio::select! {
                o = tools::run(&ctx, &mut book, tools::FIND_PLACES, &args) => o,
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
            };
            send(ChatEvent::ToolResult { id: call_id.clone(), ok: out.ok, summary: out.summary.clone() })?;
            if let Some(p) = out.places.clone() {
                send(ChatEvent::Places(p))?;
                send(ChatEvent::Sources { sources: book.sources.clone() })?;
                places_done = true;
            }
            push_tool_exchange(&mut messages, &call_id, tools::FIND_PLACES, &args, out.content);
            used_tools = true;
        }
    }
    let weather_done = weather_done || places_done;

    // Questions about the user's own files ("what does my lease say…"): BYTE
    // searches the knowledge base first, like the forced web search below.
    if session.is_none() && turn.files.is_some() && crate::router::wants_files(&question) {
        let call_id = "byte_files_0".to_string();
        let args = json!({ "query": search_query(&question, previous_question(turn.history)) });
        send(ChatEvent::ToolCall { id: call_id.clone(), name: tools::SEARCH_FILES.into(), args: args.clone() })?;
        let out = tokio::select! {
            o = tools::run(&ctx, &mut book, tools::SEARCH_FILES, &args) => o,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        send(ChatEvent::ToolResult { id: call_id.clone(), ok: out.ok, summary: out.summary.clone() })?;
        if !book.sources.is_empty() {
            send(ChatEvent::Sources { sources: book.sources.clone() })?;
        }
        push_tool_exchange(&mut messages, &call_id, tools::SEARCH_FILES, &args, out.content);
        used_tools = true;
    }

    // Deep and Extended: the research pipeline (research.rs) plans several
    // searches, reads many pages and hands the model ranked, numbered notes.
    // Fact-check first (asked for, or "is it true that…"), then compare &
    // decide ("X vs Y"), then research; each hands the model numbered notes.
    // Translate: BYTE translates part by part and streams it (translate.rs).
    if session.is_none() && turn.modules.translate && crate::translate::ask(&question).is_some() && crate::translate::run(&turn, &question, &cancel, &send).await?.is_some() {
        send(ChatEvent::Done { finish_reason: "stop".into() })?;
        return Ok(());
    }
    let mut prepared: Option<(SourceBook, String, &str)> = None;
    if let Some(s) = session.as_mut() {
        // The first step: open the site named in the message.
        send(ChatEvent::Browsing { active: true })?;
        if let Some(url) = crate::web_agent::url_in(&question) {
            let call_id = "byte_open_0".to_string();
            let args = json!({ "url": url.as_str() });
            send(ChatEvent::ToolCall { id: call_id.clone(), name: crate::web_agent::OPEN_URL.into(), args: args.clone() })?;
            let step = s.run(crate::web_agent::OPEN_URL, &args, &mut book, &cancel, &send).await?;
            send(ChatEvent::ToolResult { id: call_id.clone(), ok: step.ok, summary: step.summary.clone() })?;
            turn.log.record(crate::web_agent::OPEN_URL, &args, step.ok, &step.summary);
            if !book.sources.is_empty() {
                send(ChatEvent::Sources { sources: book.sources.clone() })?;
            }
            push_tool_exchange(&mut messages, &call_id, crate::web_agent::OPEN_URL, &args, step.content);
            used_tools = true;
        }
    } else {
        prepared = specialist(&turn, &question, estimate(&messages), weather_done, &cancel, &send).await?;
    }
    let prepared_name: Option<&str> = prepared.as_ref().map(|p| p.2);
    // Flashcards and quizzes get BYTE's own short reply (no spoilers, no repeated lists);
    // the daily briefing is put together by BYTE, so nothing in it can be invented.
    let mut canned: Option<String> = prepared.as_ref().and_then(|p| match p.2 {
        "study" => crate::study::reply_for(&p.1),
        "briefing" | "feeds_digest" | "trackers_list" => Some(p.1.clone()),
        _ => None,
    });
    if let Some((found, notes, name)) = prepared {
        book = found;
        if !book.sources.is_empty() {
            send(ChatEvent::Sources { sources: book.sources.clone() })?;
        }
        push_tool_exchange(&mut messages, &format!("byte_{name}_0"), name, &json!({ "question": question }), notes);
        searches.extend(std::iter::repeat_n(question.clone(), lim.max_searches));
        used_tools = true;
    } else if session.is_none() && !weather_done && !(turn.modules.best_of_three && crate::drafts::is_reasoning(&question)) && crate::research::applies(turn.mode, turn.web, turn.web_always, &question) {
        let query = search_query(&question, previous_question(turn.history));
        let (found, notes) = crate::research::run(&turn, &question, &query, estimate(&messages), &cancel, &send).await?;
        book = found;
        if !book.sources.is_empty() {
            send(ChatEvent::Sources { sources: book.sources.clone() })?;
        }
        push_tool_exchange(&mut messages, "byte_research_0", "research", &json!({ "question": question }), notes);
        // The planned searches count, so the model can't start over.
        searches.extend(std::iter::repeat_n(query, lim.max_searches));
        used_tools = true;
    }
    // Questions about the world: BYTE runs the first search itself and reads
    // the top pages instead of trusting the model to (small models often
    // answer from memory, or search again and again without reading).
    else if !weather_done && !(turn.modules.best_of_three && crate::drafts::applies(true, turn.mode, false, &question)) && must_search_first(&turn, &question) {
        let call_id = "byte_search_0".to_string();
        let query = search_query(&question, previous_question(turn.history));
        let args = json!({ "query": query });
        send(ChatEvent::ToolCall { id: call_id.clone(), name: tools::WEB_SEARCH.into(), args: args.clone() })?;
        let from = book.sources.len();
        let out = tokio::select! {
            o = tools::run(&ctx, &mut book, tools::WEB_SEARCH, &args) => o,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        send(ChatEvent::ToolResult { id: call_id.clone(), ok: out.ok, summary: out.summary.clone() })?;
        if !book.sources.is_empty() {
            send(ChatEvent::Sources { sources: book.sources.clone() })?;
        }
        push_tool_exchange(&mut messages, &call_id, tools::WEB_SEARCH, &args, out.content);
        searches.push(query);
        used_tools = true;
        let pages = page_budget(&turn, &messages, lim);
        read_top(&turn, &mut book, &mut messages, &question, pages.0, pages.1, from, "0", &cancel, &send).await?;
    }

    // Hard questions (maths, logic) in Deep/Extended: three drafts, and the
    // answer most of them reach. All different: one more pass weighs them.
    let mut chosen: Option<String> = canned.take();
    // Tutor mode: one step and a question back, not the whole solution.
    if chosen.is_none() && extra_rules == Some(crate::study::TUTOR_RULES) && !crate::study::wants_solution(&question) {
        let sys = messages.first().and_then(|m| m["content"].as_str()).unwrap_or(turn.system).to_string();
        let calc = messages.iter().find(|m| m["tool_call_id"] == "byte_calc_0").and_then(|m| m["content"].as_str()).map(str::to_string);
        send(ChatEvent::ToolCall { id: "byte_tutor".into(), name: "tutor_step".into(), args: json!({}) })?;
        let r = tokio::select! {
            r = crate::study::tutor_reply(turn.http, turn.ep, &sys, turn.history, calc.as_deref()) => r,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        let ok = matches!(r, Ok(Some(_)));
        send(ChatEvent::ToolResult { id: "byte_tutor".into(), ok, summary: if ok { "One step at a time".into() } else { "Answering normally".into() } })?;
        chosen = r.ok().flatten();
    }
    if chosen.is_none() && session.is_none() && crate::drafts::applies(turn.modules.best_of_three, turn.mode, used_tools, &question) {
        send(ChatEvent::ToolCall { id: "byte_drafts".into(), name: "write_drafts".into(), args: json!({ "drafts": crate::drafts::DRAFTS }) })?;
        let drafts = crate::drafts::write(turn.http, turn.ep, &messages, turn.plan, &cancel).await?;
        match crate::drafts::pick(&drafts) {
            Some((i, agree)) => {
                send(ChatEvent::ToolResult { id: "byte_drafts".into(), ok: true, summary: format!("{} drafts · {agree} agree", drafts.len()) })?;
                chosen = Some(drafts[i].clone());
            }
            None => {
                send(ChatEvent::ToolResult { id: "byte_drafts".into(), ok: true, summary: format!("{} drafts disagree · checking them", drafts.len()) })?;
                messages.push(json!({ "role": "assistant", "content": drafts.first().cloned().unwrap_or_default() }));
                messages.push(json!({ "role": "user", "content": crate::drafts::reconcile_prompt(&drafts) }));
            }
        }
    }

    let mut answer_text = String::new();
    let mut round = 0;
    let mut nudged = false;
    let mut retried = false;
    loop {
        if let Some(text) = chosen.take() {
            send(ChatEvent::Content { delta: text.clone() })?;
            answer_text.push_str(&text);
            finish = "stop".into();
            break;
        }
        if session.is_some() {
            crate::web_agent::compact(&mut messages);
        }
        let used = estimate(&messages);
        let steps_left = session.as_ref().map(|s| s.steps_left() > 0).unwrap_or(true);
        let allow_tools = !retried && steps_left && round < tool_rounds && (used as f64) < turn.ep.context as f64 * TOOL_CONTEXT_SHARE;
        if !allow_tools && used_tools && !nudged {
            messages.push(json!({ "role": "user", "content": ANSWER_NOW }));
            nudged = true;
        }
        let body = chat::build_body(messages.clone(), turn.plan, allow_tools.then(|| specs.clone()));

        // Separate text from different rounds with a blank line; drop tool-call markup.
        let mut first_in_round = true;
        let mut filter = ToolTextFilter::default();
        let mut shown = 0usize;
        let mut forward = |e: ChatEvent| -> AppResult<()> {
            let e = match e {
                ChatEvent::Content { delta } => {
                    let text = filter.push(&delta);
                    if text.is_empty() {
                        return Ok(());
                    }
                    ChatEvent::Content { delta: text }
                }
                other => other,
            };
            if let ChatEvent::Content { delta } = &e {
                if first_in_round && wrote_content {
                    send(ChatEvent::Content { delta: "\n\n".into() })?;
                }
                first_in_round = false;
                wrote_content = true;
                shown += delta.trim().len();
                answer_text.push_str(delta);
            }
            send(e)
        };
        let r = chat::stream_round(turn.http, turn.ep, &body, &cancel, &mut forward).await?;
        let tail = filter.finish();
        if !tail.trim().is_empty() {
            if first_in_round && wrote_content {
                send(ChatEvent::Content { delta: "\n\n".into() })?;
            }
            wrote_content = true;
            shown += tail.trim().len();
            answer_text.push_str(&tail);
            send(ChatEvent::Content { delta: tail })?;
        }
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
            // The final round wrote nothing readable (only tool-call markup): ask once more, plainly.
            if shown == 0 && used_tools && !retried && r.finish != "cancelled" {
                retried = true;
                messages.push(json!({ "role": "assistant", "content": "" }));
                messages.push(json!({ "role": "user", "content": ANSWER_NOW }));
                round += 1;
                continue;
            }
            break;
        }
        used_tools = true;

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
        let mut searched = false;
        let from = book.sources.len();
        for call in &calls {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let id = call["id"].as_str().unwrap_or_default().to_string();
            let name = call["function"]["name"].as_str().unwrap_or_default().to_string();
            let raw = call["function"]["arguments"].as_str().unwrap_or("{}");
            let args: Value = serde_json::from_str(raw).unwrap_or_else(|_| json!({ "raw": raw }));
            if name == tools::WEB_SEARCH {
                let q = args.get("query").and_then(Value::as_str).unwrap_or("").to_string();
                // Out of searches, or the same search again: answer from what's there.
                if searches.len() >= lim.max_searches || searches.iter().any(|p| same_search(p, &q)) {
                    let note = "Not searched again: the results and pages above are what's available. Answer from them now.";
                    messages.push(json!({ "role": "tool", "tool_call_id": id, "content": note }));
                    continue;
                }
                searches.push(q);
                searched = true;
            }
            send(ChatEvent::ToolCall { id: id.clone(), name: name.clone(), args: args.clone() })?;
            if let Some(s) = session.as_mut().filter(|_| crate::web_agent::TOOLS.contains(&name.as_str())) {
                let before = book.sources.len();
                let step = s.run(&name, &args, &mut book, &cancel, &send).await?;
                turn.log.record(&name, &args, step.ok, &step.summary);
                send(ChatEvent::ToolResult { id: id.clone(), ok: step.ok, summary: step.summary.clone() })?;
                if book.sources.len() != before {
                    send(ChatEvent::Sources { sources: book.sources.clone() })?;
                }
                messages.push(json!({ "role": "tool", "tool_call_id": id, "content": step.content }));
                continue;
            }
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
        // After a search the model asked for, BYTE reads the best new pages too.
        if searched {
            let pages = page_budget(&turn, &messages, lim);
            read_top(&turn, &mut book, &mut messages, &question, pages.0, pages.1, from, &round.to_string(), &cancel, &send).await?;
        }
        round += 1;
    }

    // Self-check: the cited claims against the passages they cite.
    let cited = matches!(turn.mode, Mode::Deep | Mode::Extended) || prepared_name == Some("fact_check");
    if turn.modules.self_check && cited && finish != "cancelled" && !book.sources.is_empty() {
        let contents: Vec<&str> = messages.iter().filter(|m| m["role"] == "tool").filter_map(|m| m["content"].as_str()).collect();
        let blocks = crate::selfcheck::source_blocks(&contents);
        if crate::selfcheck::cited_sentences(&answer_text).len() >= 2 {
            send(ChatEvent::ToolCall { id: "byte_selfcheck".into(), name: "check_answer".into(), args: json!({}) })?;
            let r = tokio::select! {
                r = crate::selfcheck::check(turn.http, turn.ep, &answer_text, &blocks) => r,
                _ = cancel.cancelled() => Err(AppError::Cancelled),
            };
            match r {
                Ok(Some(sc)) => {
                    let summary = if sc.issues.is_empty() { format!("All {} checked claims match their sources", sc.checked) } else { format!("{} of {} claims need a second look", sc.issues.len(), sc.checked) };
                    send(ChatEvent::ToolResult { id: "byte_selfcheck".into(), ok: true, summary })?;
                    send(ChatEvent::SelfCheck(sc))?;
                }
                Ok(None) => send(ChatEvent::ToolResult { id: "byte_selfcheck".into(), ok: true, summary: "Nothing to check".into() })?,
                Err(AppError::Cancelled) => return Err(AppError::Cancelled),
                Err(e) => send(ChatEvent::ToolResult { id: "byte_selfcheck".into(), ok: false, summary: e.to_string() })?,
            }
        }
    }
    if session.take().is_some() {
        // Dropping the session closes the browser (and its private data).
        send(ChatEvent::Browsing { active: false })?;
    }
    if rounds_with_stats > 0 {
        send(ChatEvent::Stats(totals))?;
    }
    send(ChatEvent::Done { finish_reason: finish })?;
    Ok(())
}

/// BYTE's card-making flows (YouTube, fact-check, study, kitchen, games,
/// reviews, prices, trips, compare), first match wins. Each shows its card and
/// returns numbered sources plus notes for the written answer.
async fn specialist(
    turn: &Turn<'_>,
    question: &str,
    used_tokens: usize,
    weather_done: bool,
    cancel: &CancellationToken,
    send: Emit<'_>,
) -> AppResult<Option<(SourceBook, String, &'static str)>> {
    let q = question;
    // A story, poem or song is writing, not something to do on the Mac (unless it also asks to save or send it).
    let writing_only = crate::router::creative_only(q);
    let mac_on = turn.modules.mac && !writing_only;
    let mac_reminders = cfg!(target_os = "macos") && mac_on;
    let tasks_db = turn.app.filter(|_| turn.modules.tasks).map(|a| &tauri::Manager::state::<crate::state::AppState>(a).inner().db);
    let watch_db = turn.app.filter(|_| turn.modules.watch).map(|a| &tauri::Manager::state::<crate::state::AppState>(a).inner().db);
    let trackers_db = turn.app.filter(|_| turn.modules.trackers).map(|a| &tauri::Manager::state::<crate::state::AppState>(a).inner().db);
    Ok(if turn.modules.automations && turn.app.is_some() && !writing_only && crate::automations::applies(q) {
        crate::automations::run(turn, q, cancel, send).await?.map(|(b, n)| (b, n, "automation"))
    } else if turn.modules.connectors && turn.app.is_some_and(|a| crate::connectors::applies(a, q)) {
        crate::connectors::run(turn, q, cancel, send).await?.map(|(b, n)| (b, n, "connectors"))
    } else if let Some(db) = trackers_db.filter(|_| crate::trackers::applies(q)) {
        crate::trackers::run(turn, db, q, send).await?
    } else if turn.modules.tasks && turn.app.is_some() && crate::briefing::applies(q) {
        crate::briefing::run(turn, cancel, send).await?.map(|(b, n)| (b, n, "briefing"))
    } else if let Some(db) = tasks_db.filter(|_| crate::tasks::applies(q) || (!mac_reminders && crate::tasks::reminder_ask(q, chrono::Local::now().naive_local()).is_some())) {
        crate::tasks::run(turn, db, q, mac_reminders, cancel, send).await?.map(|(b, n)| (b, n, "tasks"))
    } else if let Some(db) = watch_db.filter(|_| crate::feeds::applies(q)) {
        crate::feeds::run(turn, db, q, send).await?
    } else if let Some(db) = watch_db.filter(|_| crate::watchers::applies(q)) {
        crate::watchers::run(turn, db, q, cancel, send).await?.map(|(b, n)| (b, n, "watch"))
    } else if crate::macctl::applies(mac_on, q) {
        crate::macctl::run(turn, q, cancel, send).await?.map(|(b, n)| (b, n, "mac"))
    } else if crate::filectl::applies(mac_on, q) {
        crate::filectl::run(turn, q, cancel, send).await?.map(|(b, n)| (b, n, "mac"))
    } else if crate::upkeep::applies(mac_on && turn.modules.upkeep, q) {
        crate::upkeep::run(turn, q, cancel, send).await?.map(|(b, n)| (b, n, "mac"))
    } else if crate::terminal::applies(mac_on, q) {
        crate::terminal::run(turn, q, cancel, send).await?.map(|(b, n)| (b, n, "mac"))
    } else if crate::youtube::applies(turn.web, q, turn.history) {
        crate::youtube::run(turn, q, used_tokens, cancel, send).await?.map(|(b, n)| (b, n, "youtube"))
    } else if !weather_done && crate::factcheck::applies(turn.web, turn.task == Some(Task::FactCheck), q) {
        let (b, n) = crate::factcheck::run(turn, q, used_tokens, cancel, send).await?;
        Some((b, n, "fact_check"))
    } else if crate::study::applies(turn.modules.study, q) {
        crate::study::run(turn, q, used_tokens, cancel, send).await?.map(|(b, n)| (b, n, "study"))
    } else if crate::kitchen::applies(turn.kitchen, q) {
        crate::kitchen::run(turn, q, cancel, send).await?.map(|(b, n)| (b, n, "kitchen"))
    } else if crate::games::applies(turn.modules.game_hints, turn.web, q) {
        crate::games::run(turn, q, used_tokens, cancel, send).await?.map(|(b, n)| (b, n, "game_hints"))
    } else if crate::reviews::applies(turn.modules.reviews, turn.web, q) {
        crate::reviews::run(turn, q, used_tokens, cancel, send).await?.map(|(b, n)| (b, n, "reviews"))
    } else if crate::prices::applies(turn.modules.prices, turn.web, q) {
        crate::prices::run(turn, q, used_tokens, cancel, send).await?.map(|(b, n)| (b, n, "prices"))
    } else if crate::trip::applies(turn.web, q) {
        crate::trip::run(turn, q, used_tokens, cancel, send).await?.map(|(b, n)| (b, n, "plan_trip"))
    } else if !weather_done && crate::decide::applies(turn.web, q) {
        crate::decide::run(turn, q, used_tokens, cancel, send).await?.map(|(b, n)| (b, n, "compare"))
    } else {
        None
    })
}

/// What BYTE prepared on this Mac for a cloud answer: the card (already sent
/// to the UI), its sources, and notes for the cloud model to write from.
pub struct Prepared {
    pub sources: Vec<tools::Source>,
    pub notes: String,
    pub kind: &'static str,
}

/// Cloud mode with a model on this Mac: runs the card-making flows here (the
/// cloud has no way to make cards), so the cloud only writes the answer.
/// `None` when the question isn't one of them.
pub async fn prepare(turn: Turn<'_>, cancel: CancellationToken, events: &Channel<ChatEvent>) -> AppResult<Option<Prepared>> {
    let send = |e: ChatEvent| events.send(e).map_err(|e| AppError::msg(format!("UI channel closed: {e}")));
    let question = turn.history.iter().rev().find(|m| m.role == "user").map(|m| chat::question_text(&m.content).to_string()).unwrap_or_default();
    let used = estimate(&chat::base_messages(turn.system, turn.history));
    let found = specialist(&turn, &question, used, false, &cancel, &send).await?;
    Ok(found.map(|(book, notes, kind)| {
        if !book.sources.is_empty() {
            let _ = send(ChatEvent::Sources { sources: book.sources.clone() });
        }
        Prepared { sources: book.sources, notes, kind }
    }))
}

/// The message the cloud gets: the question plus what BYTE gathered here.
pub fn cloud_message(question: &str, p: &Prepared) -> String {
    format!(
        "{question}\n\n---\nBYTE already did the research for this on my Mac and showed me a card with the details, so write \
the answer from these notes (cite them as [n]; no need to search again):\n\n{}",
        p.notes.chars().take(24_000).collect::<String>()
    )
}

/// Opens the web agent's browser; files go to Downloads/BYTE.
fn open_session(app: &tauri::AppHandle) -> AppResult<crate::web_agent::Session> {
    use tauri::Manager;
    let browser = crate::web_agent::browser::TauriBrowser::start(app)?;
    let downloads = app.path().download_dir().or_else(|_| app.path().home_dir().map(|h| h.join("Downloads")));
    let dir = downloads.map(|d| d.join("BYTE")).map_err(|e| AppError::msg(format!("couldn't find the Downloads folder: {e}")))?;
    Ok(crate::web_agent::Session::new(Box::new(browser), dir))
}

/// How many pages to read and how much of each, so the pages fit in the
/// context with room left for the answer.
fn page_budget(turn: &Turn<'_>, messages: &[Value], lim: Limits) -> (usize, usize) {
    let room_tokens = (turn.ep.context as f64 * 0.6) as usize;
    let free_chars = room_tokens.saturating_sub(estimate(messages)) * 3;
    let pages = lim.auto_read.min(free_chars / 1500);
    if pages == 0 {
        return (0, 0);
    }
    (pages, lim.page_chars.min(free_chars / pages))
}

/// Openers that don't help a search engine.
const FILLERS: &[&str] = &[
    "search the web for", "search the web:", "search the web", "search online for", "search for", "search:",
    "look up", "look it up", "google", "hey byte", "hi byte", "byte,", "please", "can you tell me", "could you tell me",
    "can you find", "could you find", "can you", "could you", "tell me", "i want to know", "i'd like to know",
    "do you know", "i wonder",
];

/// Words that point back at an earlier message ("how much does *it* cost?").
const BACK_REFS: &[&str] = &["it", "its", "it's", "that", "this", "they", "them", "their", "he", "she", "his", "her", "there", "those", "these", "one"];

/// Turns a chat message into a search query: drops "search the web:"-style
/// openers and filler, spells out contractions, keeps it short, and for a
/// follow-up ("how much does it cost?") adds the earlier question's topic.
pub fn search_query(question: &str, previous: Option<&str>) -> String {
    let mut q = question.trim().replace(['\u{2019}', '\u{2018}'], "'");
    loop {
        let lower = q.to_lowercase();
        let opener = |f: &&&str| lower == **f || (lower.starts_with(**f) && lower[f.len()..].starts_with([' ', ':', ',']));
        let Some(f) = FILLERS.iter().find(opener) else {
            break;
        };
        q = q[f.len()..].trim_start_matches([':', ',', ' ']).to_string();
    }
    for (from, to) in [("what's", "what is"), ("who's", "who is"), ("where's", "where is"), ("how's", "how is"), ("when's", "when is"), ("What's", "What is"), ("Who's", "Who is"), ("Where's", "Where is"), ("How's", "How is"), ("When's", "When is")] {
        q = q.replace(from, to);
    }
    let q = q.trim().trim_end_matches(['?', '.', '!']).trim().to_string();
    let words: Vec<String> = q.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).map(str::to_string).collect();
    let lower = q.to_lowercase();
    let follow_up = words.len() <= 2
        || words.iter().any(|w| BACK_REFS.contains(&w.as_str()))
        || ["and ", "what about", "how about", "same for", "and what"].iter().any(|p| lower.starts_with(p));
    let q = match previous.filter(|_| follow_up) {
        Some(p) => {
            let topic: String = search_query(p, None).chars().take(90).collect();
            format!("{topic} {q}")
        }
        None => q,
    };
    q.chars().take(200).collect::<String>().trim().to_string()
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
        let history = vec![ChatMessage::new("user", "Use the calculator tool to compute 1234 * 5678, then tell me the result.")];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, false, None);
        let plan = crate::router::plan_turn(Mode::Auto, ThinkingPref::Off, &history[0].content);
        let (ch, seen) = collecting_channel();
        let turn = Turn { http: &http, cloud: None, net: &http, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: false, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false, modules: Default::default() };
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

    /// Real engine + a real browser window (Linux, under a display) + the
    /// internet: `xvfb-run cargo test e2e_web_agent -- --ignored`. The model
    /// opens a page and reads it; then a form submit must stop at the
    /// approval card (answered Deny here) and never be clicked.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn e2e_web_agent() {
        if std::env::var("DISPLAY").is_err() || std::env::var("BYTE_TEST_MODEL").is_err() {
            return;
        }
        let app = tauri::Builder::default().any_thread().build(tauri::generate_context!()).expect("app");
        let handle = app.handle().clone();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<Vec<Value>>>();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let all = rt.block_on(async {
                let (_server, ep) = start_server_with_ctx(16384).await.expect("engine");
                let dir = tempfile::tempdir().unwrap();
                let log = ActionLog::new(dir.path().join("a.jsonl"));
                let http = chat::local_client();
                let net = tools::fetch::web_client();
                let mut all = Vec::new();
                for q in [
                    "Go to https://example.com and tell me what the page says.",
                    "Go to https://httpbin.org/forms/post and order a large pizza for Ada (put Ada as the customer name), then submit the order.",
                ] {
                    let history = vec![ChatMessage::new("user", q)];
                    let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, true, None);
                    let plan = crate::router::plan_turn(Mode::Auto, ThinkingPref::Off, q);
                    let (ch, seen) = collecting_channel();
                    // Deny every approval card after a moment, like a user pressing Deny.
                    let watch = seen.clone();
                    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let stop2 = stop.clone();
                    let denier = tokio::spawn(async move {
                        let mut done = std::collections::HashSet::new();
                        let mut shown = 0;
                        while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
                            {
                                let ev = watch.lock().unwrap();
                                for e in ev.iter().skip(shown).filter(|e| e["kind"] != "content" && e["kind"] != "reasoning") {
                                    eprintln!("  event: {}", e.to_string().chars().take(300).collect::<String>());
                                }
                                shown = ev.len();
                            }
                            let ids: Vec<String> = watch.lock().unwrap().iter().filter(|e| e["kind"] == "approval").filter_map(|e| e["id"].as_str().map(str::to_string)).collect();
                            for id in ids {
                                if done.insert(id.clone()) {
                                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                    crate::web_agent::answer(&id, false);
                                }
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    });
                    let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: true, memory: false, log: &log, files: None, app: Some(&handle), task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: true, modules: Default::default() };
                    run(turn, CancellationToken::new(), &ch).await.unwrap();
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    let _ = denier.await;
                    all.push(seen.lock().unwrap().clone());
                }
                all
            });
            let _ = tx.send(all);
            handle.exit(0);
        });
        let mut app = app;
        let all = loop {
            #[allow(deprecated)]
            app.run_iteration(|_, _| {});
            if let Ok(all) = rx.try_recv() {
                break all;
            }
        };
        for (i, ev) in all.iter().enumerate() {
            let steps: Vec<String> = ev.iter().filter(|e| e["kind"] == "toolResult").map(|e| format!("{} {}", e["ok"], e["summary"])).collect();
            let answer: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
            eprintln!("--- turn {i}\nsteps: {steps:#?}\napproval cards: {}\nanswer: {answer}", ev.iter().filter(|e| e["kind"] == "approval").count());
            assert_eq!(ev.last().unwrap()["kind"], "done");
            assert!(ev.iter().any(|e| e["kind"] == "browsing" && e["active"] == true));
            assert!(ev.iter().any(|e| e["kind"] == "browsing" && e["active"] == false));
        }
        // Turn 1 read the page.
        assert!(all[0].iter().any(|e| e["kind"] == "toolResult" && e["summary"].as_str().unwrap_or("").contains("Example Domain")));
        // Turn 2: whatever the model did, nothing was submitted without approval.
        let submitted = all[1].iter().any(|e| e["kind"] == "toolResult" && e["ok"] == true && e["summary"].as_str().unwrap_or("").contains("Submit order"));
        assert!(!submitted, "submitted without approval");
    }

    #[test]
    fn the_cloud_gets_the_question_and_the_notes() {
        let p = Prepared { sources: vec![], notes: "[1] Banana bread (bbc.co.uk)\nIngredients: …".into(), kind: "kitchen" };
        let m = cloud_message("How do I make banana bread?", &p);
        assert!(m.starts_with("How do I make banana bread?\n\n---\n"));
        assert!(m.contains("[1] Banana bread"));
        assert!(m.contains("cite them as [n]"));
        // Long notes are cut so the message stays reasonable.
        let big = Prepared { sources: vec![], notes: "x".repeat(50_000), kind: "compare" };
        assert!(cloud_message("q", &big).len() < 25_000);
    }

    /// Real engine: Cloud mode's preparation makes the card on this Mac and
    /// hands back notes (no cloud needed to test this half).
    #[tokio::test]
    #[ignore]
    async fn e2e_prepare_for_cloud() {
        let Some((_server, mut ep)) = start_server_with_ctx(16384).await else { return };
        ep.context = 16384;
        let dir = tempfile::tempdir().unwrap();
        let log = ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let q = "What can I make with eggs, spinach and feta?";
        let history = vec![ChatMessage::new("user", q)];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, false, None);
        let plan = crate::router::plan_turn(Mode::Auto, ThinkingPref::Off, q);
        let (ch, seen) = collecting_channel();
        let turn = Turn { http: &http, cloud: None, net: &http, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: false, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: true, metric: false, agent: false, modules: Default::default() };
        let p = prepare(turn, CancellationToken::new(), &ch).await.unwrap().expect("prepared");
        assert_eq!(p.kind, "kitchen");
        assert!(!p.notes.is_empty());
        let ev = seen.lock().unwrap().clone();
        assert!(ev.iter().any(|e| e["kind"] == "recipeIdeas"), "{ev:?}");
        // Nothing was written: that's the cloud's job.
        assert!(!ev.iter().any(|e| e["kind"] == "content"));
        eprintln!("{}", cloud_message(q, &p));
        // A plain question prepares nothing.
        let history = vec![ChatMessage::new("user", "Tell me a joke")];
        let turn = Turn { http: &http, cloud: None, net: &http, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: false, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: true, metric: false, agent: false, modules: Default::default() };
        assert!(prepare(turn, CancellationToken::new(), &ch).await.unwrap().is_none());
    }

    /// Real engine + real internet (BYTE_TEST_WEB=1): reviews, prices and game
    /// hints each produce their card; a puzzle in Deep mode gets best of 3.
    #[tokio::test]
    #[ignore]
    async fn e2e_reviews_prices_hints_drafts() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let Some((_server, mut ep)) = start_server_with_ctx(16384).await else { return };
        ep.context = 16384;
        let dir = tempfile::tempdir().unwrap();
        let log = ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = tools::fetch::web_client();
        let modules = Modules { reviews: true, prices: true, game_hints: true, self_check: true, best_of_three: true, study: false, small_model: false, translate: false, mac: false, upkeep: false, tasks: false, watch: false, automations: false, trackers: false, connectors: false };
        let cases = [
            ("Reviews of the Sony WH-1000XM5", Mode::Auto, "reviews"),
            ("What's the cheapest place to buy a Steam Deck OLED?", Mode::Auto, "prices"),
            ("I'm stuck on the Water Temple in Ocarina of Time", Mode::Auto, "hints"),
            ("A bat and a ball cost $1.10 in total. The bat costs $1.00 more than the ball. How much does the ball cost?", Mode::Deep, "drafts"),
        ];
        for (q, mode, want) in cases {
            let history = vec![ChatMessage::new("user", q)];
            let system = crate::prompt::system_prompt(chrono::Local::now(), mode, true, None);
            let plan = crate::router::plan_turn(mode, ThinkingPref::Off, q);
            let (ch, seen) = collecting_channel();
            let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode, web: true, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false, modules };
            let t = std::time::Instant::now();
            run(turn, CancellationToken::new(), &ch).await.unwrap();
            let ev = seen.lock().unwrap().clone();
            let steps: Vec<String> = ev.iter().filter(|e| e["kind"] == "toolResult").map(|e| format!("{} {}", e["ok"], e["summary"])).collect();
            let answer: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
            let card = ev.iter().find(|e| e["kind"] == want).cloned();
            eprintln!("--- {q} ({:.0} s)\nsteps: {steps:#?}\ncard: {}\nanswer: {answer}", t.elapsed().as_secs_f64(), card.as_ref().map(|c| c.to_string().chars().take(900).collect::<String>()).unwrap_or_default());
            assert_eq!(ev.last().unwrap()["kind"], "done");
            match want {
                "drafts" => {
                    assert!(ev.iter().any(|e| e["kind"] == "toolResult" && e["summary"].as_str().unwrap_or("").contains("drafts")), "no drafts");
                    assert!(!answer.trim().is_empty());
                }
                // Prices depend on what stores publish today; the flow must run either way.
                "prices" => assert!(ev.iter().any(|e| e["kind"] == "toolCall" && e["name"] == "find_prices")),
                _ => assert!(card.is_some(), "no {want} card"),
            }
        }
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
        let history = vec![ChatMessage::new("user", "Search the web: what is the latest stable version of the Rust programming language?")];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, true, None);
        let plan = crate::router::plan_turn(Mode::Auto, ThinkingPref::Off, &history[0].content);
        let (ch, seen) = collecting_channel();
        let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: true, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false, modules: Default::default() };
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
            let history = vec![ChatMessage::new("user", q.clone())];
            let system = crate::prompt::system_prompt(chrono::Local::now(), mode, true, None);
            let plan = crate::router::plan_turn(mode, ThinkingPref::Auto, &q);
            let (ch, seen) = collecting_channel();
            let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode, web: true, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false, modules: Default::default() };
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
        crate::router::wants_web(q)
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
        assert_eq!(search_query("Search the web: what is the latest Rust version?", None), "what is the latest Rust version");
        assert_eq!(search_query("look up weather in Tokyo", None), "weather in Tokyo");
        assert_eq!(search_query("Latest iPhone price", None), "Latest iPhone price");
        assert!(search_query(&"x".repeat(500), None).len() <= 200);
        // "What's" matched WhatsApp on DuckDuckGo.
        assert_eq!(search_query("What\u{2019}s the weather in Pittsburgh this weekend?", None), "What is the weather in Pittsburgh this weekend");
        assert_eq!(search_query("Hey BYTE, can you tell me who is the CEO of OpenAI?", None), "who is the CEO of OpenAI");
        // Follow-ups carry the earlier topic.
        assert_eq!(search_query("How much does it cost?", Some("Is the Steam Deck OLED worth buying?")), "Is the Steam Deck OLED worth buying How much does it cost");
        assert_eq!(search_query("Who founded Anthropic?", Some("What is Claude?")), "Who founded Anthropic");
        assert_eq!(search_query("what about Nvidia?", Some("AMD stock price today")), "AMD stock price today what about Nvidia");
    }

    #[test]
    fn tool_call_markup_never_reaches_the_answer() {
        let mut f = ToolTextFilter::default();
        let mut out = String::new();
        for piece in ["Here is ", "the answer. <tool", "_call>\n<function=web_search>\n<parameter=query>\nx\n</parameter>\n</function>\n</tool", "_call> Done <b>ok</b>"] {
            out.push_str(&f.push(piece));
        }
        out.push_str(&f.finish());
        assert_eq!(out, "Here is the answer.  Done <b>ok</b>");
        assert!(f.dropped > 0);
        // An unfinished call at the end is dropped too; stray closers go.
        let mut f = ToolTextFilter::default();
        let mut out = f.push("<function=read_page>\n<parameter=url>https://x");
        out.push_str(&f.finish());
        assert_eq!(out, "");
        let mut f = ToolTextFilter::default();
        let mut out = f.push("a < b and 3<4 </parameter>");
        out.push_str(&f.finish());
        assert_eq!(out, "a < b and 3<4 </parameter>");
    }

    #[test]
    fn repeated_searches_are_recognised() {
        assert!(same_search("Steam Deck OLED price 2025 spec review", "Steam Deck OLED OLED 2024 specs price review"));
        assert!(same_search("Pittsburgh Steelers next game date", "Steelers next game date Pittsburgh"));
        assert!(!same_search("Steam Deck OLED review", "Nintendo Switch 2 price"));
    }

    #[test]
    fn deeper_modes_allow_more_tool_use() {
        let (f, a, d, e) = (limits(Mode::Fast), limits(Mode::Auto), limits(Mode::Deep), limits(Mode::Extended));
        assert!(f.tool_rounds < a.tool_rounds && a.tool_rounds < d.tool_rounds && d.tool_rounds < e.tool_rounds);
        assert!(f.page_chars < e.page_chars);
    }
}
