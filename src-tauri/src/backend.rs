//! One way to answer a chat turn, whichever model does it
//! (docs/DESIGN-AND-PLATFORMS.md Part 2). `chat_send` hands every turn to
//! [`answer`], which picks a backend and owns the fallback rules:
//!
//! - [`LocalLlama`]: the bundled llama-server on this machine (tools, memory, projects).
//! - [`Cloud`]: the BYTE cloud (`docs/CLOUD-MODE.md`).
//!
//! A new backend (another engine, or the Windows/Linux builds' engines later)
//! implements [`ModelBackend`]; platform-specific code stays inside it.

use tauri::ipc::Channel;

use crate::chat::{self, ChatEvent};
use crate::cloud::cmd::CloudTurn;
use crate::cloud::CloudError;
use crate::commands::ChatRequest;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::{agent, prompt, router};

/// What a backend can do, so callers decide without knowing which one it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Private chats may use it (nothing leaves the machine).
    pub private: bool,
    /// Runs BYTE's own tools (web search, calculator, memory) on this machine.
    pub local_tools: bool,
    /// Can be unreachable for reasons outside the app (network, a server at home).
    pub can_be_unreachable: bool,
}

/// Why a turn failed: `Unreachable` means another backend may answer instead.
#[derive(Debug)]
pub enum BackendError {
    Unreachable(String),
    Failed(AppError),
}

impl From<AppError> for BackendError {
    fn from(e: AppError) -> Self {
        BackendError::Failed(e)
    }
}

/// Answers one chat turn, streaming `ChatEvent`s to the UI.
pub trait ModelBackend {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    async fn answer(&self, state: &AppState, request: &ChatRequest, on_event: &Channel<ChatEvent>) -> Result<(), BackendError>;
}

/// The bundled llama-server (the main model or one loaded alongside it).
pub struct LocalLlama;

impl ModelBackend for LocalLlama {
    fn name(&self) -> &'static str {
        "this machine"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { private: true, local_tools: true, can_be_unreachable: false }
    }
    async fn answer(&self, state: &AppState, request: &ChatRequest, on_event: &Channel<ChatEvent>) -> Result<(), BackendError> {
        Ok(local_turn(state, request, on_event).await?)
    }
}

/// The BYTE cloud, for one turn.
pub struct Cloud<'a> {
    pub turn: &'a CloudTurn,
}

impl ModelBackend for Cloud<'_> {
    fn name(&self) -> &'static str {
        "BYTE cloud"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { private: false, local_tools: false, can_be_unreachable: true }
    }
    async fn answer(&self, state: &AppState, request: &ChatRequest, on_event: &Channel<ChatEvent>) -> Result<(), BackendError> {
        crate::cloud::cmd::send(state, request, self.turn, on_event).await.map_err(|e| match e {
            CloudError::Unreachable(why) => BackendError::Unreachable(why),
            other => BackendError::Failed(other.into()),
        })
    }
}

/// Answers a turn: on the cloud when the request says so, else on this machine.
pub async fn answer(state: &AppState, mut request: ChatRequest, on_event: &Channel<ChatEvent>) -> AppResult<()> {
    let Some(turn) = request.cloud.take() else {
        return finish(LocalLlama.answer(state, &request, on_event).await);
    };
    let fallback = (!turn.no_fallback).then(|| {
        let mut local = request.clone();
        local.mode = crate::cloud::cmd::local_mode(&turn.mode);
        local
    });
    // Cards (recipes, compare tables, trips, reviews…) are made on this Mac, with the
    // model loaded here or else by asking the cloud for each card's JSON; the cloud
    // then writes the answer from BYTE's notes.
    // Not in Both (this Mac already answers beside the cloud, cards and all).
    let prepared = if turn.no_fallback {
        None
    } else {
        match prepare_for_cloud(state, &request, on_event).await {
            Ok(p) => p,
            Err(AppError::Cancelled) => {
                let _ = on_event.send(ChatEvent::Done { finish_reason: "cancelled".into() });
                return Ok(());
            }
            Err(e) => {
                log::warn!("couldn't prepare cards for the cloud: {e}");
                None
            }
        }
    };
    if let Some(p) = &prepared {
        // Flashcards and quizzes need no written answer: BYTE's own short reply.
        if let Some(reply) = (p.kind == "study").then(|| crate::study::reply_for(&p.notes)).flatten() {
            let _ = on_event.send(ChatEvent::Content { delta: reply });
            let _ = on_event.send(ChatEvent::Done { finish_reason: "stop".into() });
            return Ok(());
        }
        if let Some(last) = request.messages.iter_mut().rev().find(|m| m.role == "user") {
            last.content = agent::cloud_message(chat::question_text(&last.content), p);
        }
    }
    let result = with_fallback(state, &Cloud { turn: &turn }, &request, &LocalLlama, fallback.as_ref(), on_event).await;
    // The card's sources are the ones the answer cites ([n]).
    if let Some(p) = prepared.filter(|p| !p.sources.is_empty()) {
        if result.is_ok() {
            let _ = on_event.send(ChatEvent::Sources { sources: p.sources });
        }
    }
    result
}

/// Runs `primary`; if it can't be reached before it accepted the turn,
/// `secondary` answers `fallback` instead (when given) and the UI is told so
/// quietly. Without a fallback the unreachable error is reported.
pub async fn with_fallback<P: ModelBackend, S: ModelBackend>(
    state: &AppState,
    primary: &P,
    request: &ChatRequest,
    secondary: &S,
    fallback: Option<&ChatRequest>,
    on_event: &Channel<ChatEvent>,
) -> AppResult<()> {
    if request.private && !primary.capabilities().private {
        return Err(AppError::msg("Private chats stay on this Mac. Switch to a model on this Mac, or turn off Private."));
    }
    match primary.answer(state, request, on_event).await {
        Err(BackendError::Unreachable(why)) => match fallback {
            Some(local) => {
                log::warn!("{} unreachable, answering on {}: {why}", primary.name(), secondary.name());
                let _ = on_event.send(ChatEvent::Notice { text: "The BYTE cloud couldn't be reached, so this answer was written on this Mac.".into() });
                finish(secondary.answer(state, local, on_event).await)
            }
            None => Err(AppError::msg(format!("Your BYTE cloud can't be reached right now ({why}). The answer from this Mac is beside this one."))),
        },
        other => finish(other),
    }
}

fn finish(r: Result<(), BackendError>) -> AppResult<()> {
    match r {
        Ok(()) => Ok(()),
        Err(BackendError::Failed(e)) => Err(e),
        Err(BackendError::Unreachable(why)) => Err(AppError::msg(format!("Couldn't reach the model ({why})."))),
    }
}

/// Instant answers (answer_cache.rs): a chat's first question that was asked
/// (almost exactly) in the last week gets that answer at once, marked as
/// reused. Any problem just means answering normally.
async fn reuse_earlier_answer(state: &AppState, request: &ChatRequest, ep: &crate::engine::Endpoint, on_event: &Channel<ChatEvent>) -> bool {
    if request.private || request.fresh || request.model.is_some() || !state.settings.lock().await.answer_cache {
        return false;
    }
    let Some(question) = crate::answer_cache::cacheable(&request.messages) else { return false };
    let catalog = state.catalog.get();
    let Some(app) = state.app.get() else { return false };
    if crate::answer_cache::count(&state.db) == 0 || !crate::embed::Embedder::installed(&catalog, &state.paths.models) {
        return false;
    }
    let Ok(v) = state.embedder.embed(app, &state.paths.models, &catalog, &[question.to_string()], crate::embed::Purpose::Query).await else { return false };
    let mode = serde_json::to_value(request.mode).ok().and_then(|m| m.as_str().map(String::from)).unwrap_or_default();
    let Ok(Some(hit)) = crate::answer_cache::find(&state.db, &v[0], &mode) else { return false };
    let when = chrono::DateTime::from_timestamp_millis(hit.created_at).map(|t| t.with_timezone(&chrono::Local).format("%b %-d").to_string()).unwrap_or_default();
    let sources: Vec<crate::tools::Source> = serde_json::from_str(&hit.sources).unwrap_or_default();
    let _ = on_event.send(ChatEvent::Started { thinking: false, model: ep.model.clone() });
    let _ = on_event.send(ChatEvent::Notice {
        text: format!("Instant answer: you asked \"{}\" on {when}. Use Regenerate for a fresh answer.", hit.question.chars().take(80).collect::<String>()),
    });
    if !sources.is_empty() {
        let _ = on_event.send(ChatEvent::Sources { sources });
    }
    let _ = on_event.send(ChatEvent::Content { delta: hit.answer });
    let _ = on_event.send(ChatEvent::Done { finish_reason: "cache".into() });
    true
}

async fn local_turn(state: &AppState, request: &ChatRequest, on_event: &Channel<ChatEvent>) -> AppResult<()> {
    if state.tuning.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::msg("BYTE is tuning itself for this Mac (about a minute). Try again when it's done."));
    }
    let main_key = state.engine.loaded().await.map(|l| l.key);
    let ep = match request.model.as_deref() {
        Some(k) if main_key.as_deref() != Some(k) => {
            let engine = state.extras.get(k).await.ok_or_else(|| AppError::msg(format!("{k} isn't loaded. Load it in Settings → Models.")))?;
            engine.endpoint().await.ok_or_else(|| AppError::msg("That model is still loading. Try again in a few seconds."))?
        }
        _ => state
            .engine
            .endpoint()
            .await
            .ok_or_else(|| AppError::msg("The AI engine isn't ready yet. It usually takes a few seconds after launch."))?,
    };
    if reuse_earlier_answer(state, request, &ep, on_event).await {
        return Ok(());
    }
    better_model_hint(state, &ep, on_event).await;
    // A model that can't see gets the photos described by the photo helper.
    let described = if ep.vision { None } else { describe_photos(state, request, on_event).await };
    let request = described.as_ref().unwrap_or(request);
    let setup = Setup::new(state, request, ep).await?;
    if setup.saving {
        let lowered = setup.mode != request.mode;
        let _ = on_event.send(ChatEvent::Notice {
            text: format!("Battery saver is on (below 20% and unplugged): shorter thinking{}. Plug in for full answers.", if lowered { ", and Auto instead of Deep or Extended" } else { "" }),
        });
    }
    let cancel = state.generations.register(&request.request_id).await;
    let result = agent::run(setup.turn(state, request), cancel, on_event).await;
    state.generations.finish(&request.request_id).await;
    match result {
        Err(AppError::Cancelled) => {
            let _ = on_event.send(ChatEvent::Done { finish_reason: "cancelled".into() });
            Ok(())
        }
        other => other,
    }
}

/// Everything a local turn needs, owned (settings read once), so the answer
/// loop and the cloud's card preparation build the same `agent::Turn`.
struct Setup {
    ep: crate::engine::Endpoint,
    system: String,
    history: Vec<chat::ChatMessage>,
    plan: router::TurnPlan,
    web: bool,
    memory: bool,
    home: Option<String>,
    depth: u8,
    web_always: bool,
    kitchen: bool,
    metric: bool,
    web_agent: bool,
    modules: agent::Modules,
    cloud: Option<crate::cloud::CloudClient>,
    kb: bool,
    /// The mode used (battery saver can lower it).
    mode: crate::settings::Mode,
    /// Battery saver is lightening this turn.
    saving: bool,
}

impl Setup {
    async fn new(state: &AppState, request: &ChatRequest, ep: crate::engine::Endpoint) -> AppResult<Setup> {
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| chat::question_text(&m.content))
            .unwrap_or("");
        let catalog = state.catalog.get();
        let profile = catalog.resolve(&ep.model).map(|(m, _)| crate::modelcfg::profile(m)).unwrap_or_default();
        // Unknown models (added by hand) count as small: checking a good card costs nothing.
        let small_model = ep.cloud.is_none() && catalog.resolve(&ep.model).ok().and_then(|(m, _)| m.params_b).is_none_or(|b| b <= crate::quality::SMALL_B);
        let (overrides, saver) = {
            let s = state.settings.lock().await;
            (s.model_overrides.get(&ep.model).cloned(), s.battery_saver)
        };
        // Battery saver: below 20% and unplugged, lighter answers.
        let saving = saver && ep.cloud.is_none() && crate::system::low_battery();
        let mode = if saving && matches!(request.mode, crate::settings::Mode::Deep | crate::settings::Mode::Extended) { crate::settings::Mode::Auto } else { request.mode };
        let mut plan = router::plan_turn(mode, request.thinking, last_user).for_model(profile);
        if saving && plan.thinking {
            plan.thinking_budget = if plan.thinking_budget < 0 { 512 } else { plan.thinking_budget.min(512) };
        }
        if let Some(o) = &overrides {
            o.apply(&mut plan);
        }
        let (web, user_name, memory, about_me, home, depth, web_always, kitchen, metric, web_agent, modules, kb_on, cloud_on) = {
            let s = state.settings.lock().await;
            (s.web_search, s.user_name.clone(), s.memory_enabled && !request.private, s.about_me.clone(), s.home_place.clone(), s.research_depth, s.web_mode == "always", s.kitchen_enabled, s.measure_units == "metric", s.web_agent_enabled, agent::Modules {
                reviews: s.reviews_enabled,
                prices: s.prices_enabled,
                game_hints: s.game_hints_enabled,
                self_check: s.self_check,
                best_of_three: s.best_of_three,
                study: s.study_enabled,
                small_model,
                translate: s.translate_enabled,
            }, s.kb_enabled, s.cloud_connected)
        };
        let mut system = prompt::system_prompt(chrono::Local::now(), request.mode, web, user_name.as_deref());
        if memory {
            let memories: Vec<String> = state.db.memories()?.into_iter().map(|m| m.text).collect();
            system.push_str(&prompt::memory_section(about_me.as_deref(), &memories, true));
        }
        if let Some(project) = request.project_id.as_deref().filter(|p| !p.is_empty()).map(|p| state.db.project(p)).transpose()?.flatten() {
            system.push_str(&prompt::project_section(&project.name, &project.instructions));
        }
        if let Some(extra) = overrides.as_ref().map(|o| o.system_extra.trim()).filter(|e| !e.is_empty()) {
            system.push_str(&format!("\n\nThe user's extra instructions for this model:\n{extra}"));
        }
        if let Some(a) = request.assistant_id.as_deref().filter(|a| !a.is_empty()).map(|a| crate::assistants::get(&state.db, a)).transpose()?.flatten() {
            system.push_str(&crate::assistants::prompt_section(&a));
        }
        let reserve = plan.max_tokens + plan.thinking_budget.max(0) as u32;
        let messages = chat::with_files(&request.messages, ep.context, ep.vision);
        let history = chat::fit_history(&messages, &system, ep.context, reserve.min(ep.context / 2));
        // Web search goes through the BYTE cloud when a key is saved (its SearXNG
        // beats scraping search engines from this Mac); private chats stay keyless.
        let cloud = if web && !request.private && cloud_on { state.cloud_client().await.ok() } else { None };
        // "My files": the knowledge base is searchable when it's on and has passages.
        let kb = kb_on && crate::kb::chunk_count(&state.db) > 0;
        Ok(Setup { ep, system, history, plan, web, memory, home, depth, web_always, kitchen, metric, web_agent, modules, cloud, kb, mode, saving })
    }

    fn turn<'a>(&'a self, state: &'a AppState, request: &ChatRequest) -> agent::Turn<'a> {
        agent::Turn {
            http: &state.local_http,
            net: &state.net,
            cloud: self.cloud.as_ref(),
            ep: &self.ep,
            system: &self.system,
            history: &self.history,
            plan: self.plan,
            mode: self.mode,
            web: self.web,
            memory: self.memory,
            log: &state.actions,
            files: state.app.get().filter(|_| self.kb),
            app: state.app.get(),
            task: request.task,
            home: self.home.as_deref(),
            depth: self.depth.min(2),
            web_always: self.web_always,
            kitchen: self.kitchen,
            metric: self.metric,
            agent: self.web_agent,
            modules: self.modules,
        }
    }
}

/// Cloud mode: when a model is loaded on this Mac, BYTE makes the cards here
/// (recipes, compare tables, trips, reviews…) and the cloud writes the answer
/// from the notes. `Ok(None)`: nothing to prepare (or no local model); ask the cloud as usual.
async fn prepare_for_cloud(state: &AppState, request: &ChatRequest, on_event: &Channel<ChatEvent>) -> AppResult<Option<agent::Prepared>> {
    if request.private || state.tuning.load(std::sync::atomic::Ordering::SeqCst) {
        return Ok(None);
    }
    let ep = match state.engine.endpoint().await {
        Some(ep) => ep,
        None => match cloud_cards(state).await {
            Some(ep) => ep,
            None => return Ok(None),
        },
    };
    let setup = Setup::new(state, request, ep).await?;
    let cancel = state.generations.register(&request.request_id).await;
    let r = agent::prepare(setup.turn(state, request), cancel, on_event).await;
    state.generations.finish(&request.request_id).await;
    r
}

/// Once per small model: "your Mac can run a clearly better model" (Settings → Models).
async fn better_model_hint(state: &AppState, ep: &crate::engine::Endpoint, on_event: &Channel<ChatEvent>) {
    let settings = state.settings.lock().await.clone();
    if settings.better_model_hint_for.iter().any(|k| k == &ep.model) {
        return;
    }
    let catalog = state.catalog.get();
    let ctx = settings.context_size.unwrap_or(crate::engine::DEFAULT_CONTEXT);
    let info = crate::models::calibrate(crate::system::system_info(&state.paths.data).with_settings(&settings), &catalog);
    let Some(better) = crate::models::better_model(&catalog, &info, ctx, &ep.model) else { return };
    let _ = on_event.send(ChatEvent::Notice {
        text: format!("Your Mac can run {}, which gives noticeably better answers and cards than the small model in use. You can download it in Settings → Models.", better.name),
    });
    let mut s = state.settings.lock().await;
    let mut next = s.clone();
    next.better_model_hint_for.push(ep.model.clone());
    if next.save(&state.paths.settings_file).is_ok() {
        *s = next;
    }
}

/// The request with its latest photos described in words by the photo helper
/// (`None`: nothing to describe, the helper is off or not downloaded).
async fn describe_photos(state: &AppState, request: &ChatRequest, on_event: &Channel<ChatEvent>) -> Option<ChatRequest> {
    use crate::files::FileKind;
    if !state.settings.lock().await.photo_helper {
        return None;
    }
    let i = request.messages.iter().rposition(|m| m.role == "user")?;
    let todo: Vec<usize> = request.messages[i]
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.kind == FileKind::Image && f.image.is_some() && !f.text.starts_with(crate::looker::DESCRIBED))
        .map(|(n, _)| n)
        .take(4)
        .collect();
    let app = state.app.get()?;
    let catalog = state.catalog.get();
    if todo.is_empty() || crate::looker::choose(&catalog, &state.paths.models).is_none() {
        return None;
    }
    let mut out = request.clone();
    let question = chat::question_text(&request.messages[i].content).to_string();
    for n in todo {
        let f = &mut out.messages[i].files[n];
        let id = format!("byte_look_{n}");
        let _ = on_event.send(ChatEvent::ToolCall { id: id.clone(), name: "look_at_photo".into(), args: serde_json::json!({ "name": f.name }) });
        match state.looker.describe(app, &state.paths.models, &catalog, f.image.as_deref().unwrap_or_default(), &question).await {
            Ok(d) => {
                f.text = format!("{}{d}{}{}", crate::looker::DESCRIBED, if f.text.trim().is_empty() { "" } else { "\n\nText read from the photo:\n" }, f.text);
                let _ = on_event.send(ChatEvent::ToolResult { id, ok: true, summary: format!("Described {}", f.name) });
            }
            Err(e) => {
                log::warn!("photo helper: {e}");
                let _ = on_event.send(ChatEvent::ToolResult { id, ok: false, summary: "The photo helper couldn't look at it".into() });
            }
        }
    }
    Some(out)
}

/// With no model on this Mac: an endpoint whose structured replies (the cards' JSON)
/// come from the BYTE cloud (`cloud::json`).
pub(crate) async fn cloud_cards(state: &AppState) -> Option<crate::engine::Endpoint> {
    let modes = {
        let s = state.settings.lock().await;
        if !s.cloud_connected {
            return None;
        }
        s.cloud_account.as_ref().map(crate::cloud::parse_me).map(|me| me.modes).unwrap_or_default()
    };
    let mode = crate::cloud::json::pick_mode(&modes).unwrap_or_else(|| "auto".into());
    let client = state.cloud_client().await.ok()?;
    Some(crate::engine::Endpoint {
        base_url: "byte-cloud".into(),
        api_key: String::new(),
        model: "BYTE Cloud".into(),
        context: 32_768,
        vision: false,
        cloud: Some(std::sync::Arc::new(crate::cloud::json::JsonHelper::new(client, mode))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::e2e_support::collecting_channel;
    use crate::settings::{Mode, ThinkingPref};
    use std::sync::Mutex;

    /// A backend that answers with a canned result and records what it was asked.
    struct Fake {
        result: fn() -> Result<(), BackendError>,
        private: bool,
        asked: Mutex<Vec<Mode>>,
    }
    impl Fake {
        fn new(result: fn() -> Result<(), BackendError>, private: bool) -> Self {
            Fake { result, private, asked: Mutex::new(vec![]) }
        }
    }
    impl ModelBackend for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities { private: self.private, local_tools: false, can_be_unreachable: !self.private }
        }
        async fn answer(&self, _: &AppState, request: &ChatRequest, _: &Channel<ChatEvent>) -> Result<(), BackendError> {
            self.asked.lock().unwrap().push(request.mode);
            (self.result)()
        }
    }

    fn request(private: bool) -> ChatRequest {
        ChatRequest {
            request_id: "r".into(),
            messages: vec![],
            mode: Mode::Deep,
            thinking: ThinkingPref::Auto,
            model: None,
            private,
            project_id: None,
            assistant_id: None,
            cloud: None,
            fresh: false,
            task: None,
        }
    }

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let s = AppState::new(crate::paths::Paths::at(dir.path().to_path_buf()).unwrap());
        (dir, s)
    }

    #[tokio::test]
    async fn an_unreachable_cloud_hands_the_turn_to_this_machine_quietly() {
        let (_d, state) = state();
        let (ch, seen) = collecting_channel();
        let cloud = Fake::new(|| Err(BackendError::Unreachable("offline".into())), false);
        let local = Fake::new(|| Ok(()), true);
        let mut fb = request(false);
        fb.mode = Mode::Fast;
        with_fallback(&state, &cloud, &request(false), &local, Some(&fb), &ch).await.unwrap();
        assert_eq!(*local.asked.lock().unwrap(), vec![Mode::Fast], "the fallback request (with its mapped mode) is answered");
        assert_eq!(seen.lock().unwrap()[0]["kind"], "notice");
    }

    #[tokio::test]
    async fn without_a_fallback_unreachable_is_reported() {
        let (_d, state) = state();
        let (ch, _) = collecting_channel();
        let cloud = Fake::new(|| Err(BackendError::Unreachable("offline".into())), false);
        let local = Fake::new(|| Ok(()), true);
        let err = with_fallback(&state, &cloud, &request(false), &local, None, &ch).await.unwrap_err();
        assert!(err.to_string().contains("can't be reached"));
        assert!(local.asked.lock().unwrap().is_empty(), "Both: this machine is already answering beside it");
    }

    #[tokio::test]
    async fn other_failures_are_not_answered_twice() {
        let (_d, state) = state();
        let (ch, _) = collecting_channel();
        let cloud = Fake::new(|| Err(BackendError::Failed(AppError::msg("allowance used up"))), false);
        let local = Fake::new(|| Ok(()), true);
        let fb = request(false);
        let err = with_fallback(&state, &cloud, &request(false), &local, Some(&fb), &ch).await.unwrap_err();
        assert!(err.to_string().contains("allowance"));
        assert!(local.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn private_chats_never_reach_a_backend_that_leaves_the_machine() {
        let (_d, state) = state();
        let (ch, _) = collecting_channel();
        let cloud = Fake::new(|| Ok(()), false);
        let local = Fake::new(|| Ok(()), true);
        assert!(with_fallback(&state, &cloud, &request(true), &local, None, &ch).await.is_err());
        assert!(cloud.asked.lock().unwrap().is_empty());
        assert!(!Cloud { turn: &serde_json::from_value(serde_json::json!({ "mode": "auto" })).unwrap() }.capabilities().private);
        assert!(LocalLlama.capabilities().private);
    }
}
