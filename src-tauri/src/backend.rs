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
    with_fallback(state, &Cloud { turn: &turn }, &request, &LocalLlama, fallback.as_ref(), on_event).await
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
    let last_user = request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| chat::question_text(&m.content))
        .unwrap_or("");
    let catalog = state.catalog.get();
    let profile = catalog.resolve(&ep.model).map(|(m, _)| crate::modelcfg::profile(m)).unwrap_or_default();
    let plan = router::plan_turn(request.mode, request.thinking, last_user).for_model(profile);
    let (web, user_name, memory, about_me, home, depth, web_always, kitchen, metric, web_agent, modules) = {
        let s = state.settings.lock().await;
        (s.web_search, s.user_name.clone(), s.memory_enabled && !request.private, s.about_me.clone(), s.home_place.clone(), s.research_depth, s.web_mode == "always", s.kitchen_enabled, s.measure_units == "metric", s.web_agent_enabled, agent::Modules {
            reviews: s.reviews_enabled,
            prices: s.prices_enabled,
            game_hints: s.game_hints_enabled,
            self_check: s.self_check,
            best_of_three: s.best_of_three,
            study: s.study_enabled,
        })
    };
    let mut system = prompt::system_prompt(chrono::Local::now(), request.mode, web, user_name.as_deref());
    if memory {
        let memories: Vec<String> = state.db.memories()?.into_iter().map(|m| m.text).collect();
        system.push_str(&prompt::memory_section(about_me.as_deref(), &memories, true));
    }
    if let Some(project) = request.project_id.as_deref().filter(|p| !p.is_empty()).map(|p| state.db.project(p)).transpose()?.flatten() {
        system.push_str(&prompt::project_section(&project.name, &project.instructions));
    }
    let reserve = plan.max_tokens + plan.thinking_budget.max(0) as u32;
    let messages = chat::with_files(&request.messages, ep.context, ep.vision);
    let history = chat::fit_history(&messages, &system, ep.context, reserve.min(ep.context / 2));

    // Web search goes through the BYTE cloud when a key is saved (its SearXNG
    // beats scraping search engines from this Mac); private chats stay keyless.
    let cloud = if web && !request.private && state.settings.lock().await.cloud_connected { state.cloud_client().await.ok() } else { None };
    // "My files": the knowledge base is searchable when it's on and has passages.
    let kb_on = state.settings.lock().await.kb_enabled;
    let files = state.app.get().filter(|_| kb_on && crate::kb::chunk_count(&state.db) > 0);
    let cancel = state.generations.register(&request.request_id).await;
    let turn = agent::Turn {
        http: &state.local_http,
        net: &state.net,
        cloud: cloud.as_ref(),
        ep: &ep,
        system: &system,
        history: &history,
        plan,
        mode: request.mode,
        web,
        memory,
        log: &state.actions,
        files,
        app: state.app.get(),
        task: request.task,
        home: home.as_deref(),
        depth: depth.min(2),
        web_always,
        kitchen,
        metric,
        agent: web_agent,
        modules,
    };
    let result = agent::run(turn, cancel, on_event).await;
    state.generations.finish(&request.request_id).await;
    match result {
        Err(AppError::Cancelled) => {
            let _ = on_event.send(ChatEvent::Done { finish_reason: "cancelled".into() });
            Ok(())
        }
        other => other,
    }
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
