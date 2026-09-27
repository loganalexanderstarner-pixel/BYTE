//! Thin `#[tauri::command]` wrappers. Logic lives in the modules they call.

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::chat::{self, ChatEvent, ChatMessage};
use crate::db::{ConversationMeta, Memory, MetaPatch, Project, SearchHit};
use crate::profiles::{Profile, Profiles};
use crate::summarize::ChatSummary;
use crate::engine::{EngineStatus, LoadedModel, DEFAULT_CONTEXT, EXTRA_CONTEXT};
use crate::error::{AppError, AppResult};
use crate::models::{self, ModelStatus};
use crate::settings::{Mode, Settings, ThinkingPref};
use crate::state::AppState;
use crate::system::{self, SystemInfo};
use crate::{agent, prompt, router};

#[tauri::command]
pub fn system_info(state: State<'_, AppState>) -> SystemInfo {
    system::system_info(&state.paths.data)
}

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
pub async fn settings_update(state: State<'_, AppState>, patch: serde_json::Value) -> AppResult<Settings> {
    let mut s = state.settings.lock().await;
    let next = s.merged(patch)?;
    next.save(&state.paths.settings_file)?;
    *s = next.clone();
    Ok(next)
}

#[tauri::command]
pub async fn models_list(state: State<'_, AppState>) -> AppResult<Vec<ModelStatus>> {
    let (ctx, pref) = {
        let s = state.settings.lock().await;
        (s.context_size.unwrap_or(DEFAULT_CONTEXT), s.speed_pref)
    };
    let active = state.downloads.active_ids().await;
    let info = system::system_info(&state.paths.data).with_pref(pref);
    let catalog = state.catalog.get();
    let loaded_bytes = loaded_bytes(&state).await;
    Ok(models::list(&catalog, &models::ListContext { models_dir: &state.paths.models, info: &info, ctx, downloading: &active, loaded_bytes }))
}

/// The best chat model + version for this Mac, as a key like "qwen3.5-9b:Q6_K".
#[tauri::command]
pub async fn model_recommend(state: State<'_, AppState>) -> AppResult<Option<String>> {
    let (ctx, pref) = {
        let s = state.settings.lock().await;
        (s.context_size.unwrap_or(DEFAULT_CONTEXT), s.speed_pref)
    };
    let info = system::system_info(&state.paths.data).with_pref(pref);
    let catalog = state.catalog.get();
    Ok(models::recommend(&catalog, &info, ctx).map(|(m, v)| models::key(m, v)))
}

/// Fetches a newer catalog if one is published. Returns true if it changed.
#[tauri::command]
pub async fn catalog_refresh(state: State<'_, AppState>) -> AppResult<bool> {
    let url = state.settings.lock().await.catalog_url.clone().unwrap_or_else(|| models::DEFAULT_CATALOG_URL.to_string());
    // A private repository (or no internet) makes the online list unreachable;
    // the list built into the app keeps working, so say that plainly.
    state.catalog.refresh(&state.net, &url).await.map_err(|e| {
        log::info!("catalog refresh from {url} failed: {e}");
        AppError::msg(format!(
            "Couldn't reach the online model list, so BYTE is using the list built into this version ({} models). New BYTE versions include the newest models.",
            state.catalog.get().models.len()
        ))
    })
}

#[tauri::command]
pub async fn model_download(app: AppHandle, state: State<'_, AppState>, key: String) -> AppResult<()> {
    let catalog = state.catalog.get();
    let (model, variant) = catalog.resolve(&key)?;
    let key = models::key(model, variant);
    state
        .downloads
        .start(app, state.net.clone(), state.paths.models.clone(), model.repo.clone(), variant.clone(), key)
        .await
}

#[tauri::command]
pub async fn model_pause(state: State<'_, AppState>, key: String) -> AppResult<()> {
    state.downloads.pause(&key).await;
    Ok(())
}

#[tauri::command]
pub async fn model_delete(app: AppHandle, state: State<'_, AppState>, key: String) -> AppResult<()> {
    state.downloads.pause(&key).await;
    let catalog = state.catalog.get();
    let (model, variant) = catalog.resolve(&key)?;
    let key = models::key(model, variant);
    let active = state.settings.lock().await.active_model.clone();
    let active_key = active.as_deref().and_then(|a| catalog.resolve(a).ok()).map(|(m, v)| models::key(m, v));
    state.extras.remove(&app, &key).await;
    remember_alongside(&state, |list| list.retain(|k| k != &key)).await?;
    if active_key.as_deref() == Some(key.as_str()) {
        state.engine.stop().await;
        let _ = tauri::Emitter::emit(&app, crate::engine::STATUS_EVENT, EngineStatus::NoModel);
    }
    models::delete(&state.paths.models, variant)
}

/// Makes `key` the active chat model and (re)starts the engine with it.
#[tauri::command]
pub async fn model_activate(app: AppHandle, state: State<'_, AppState>, key: String) -> AppResult<()> {
    let catalog = state.catalog.get();
    let (model, variant) = catalog.resolve(&key)?;
    if model.role != models::Role::Chat {
        return Err(AppError::msg(format!("{} is a helper model and can't be used for chat", model.name)));
    }
    let key = models::key(model, variant);
    let ctx = {
        let mut s = state.settings.lock().await;
        let next = s.merged(serde_json::json!({ "activeModel": key }))?;
        next.save(&state.paths.settings_file)?;
        *s = next;
        s.context_size
    };
    // A model that was loaded alongside becomes the main one: don't run it twice.
    state.extras.remove(&app, &key).await;
    remember_alongside(&state, |list| list.retain(|k| k != &key)).await?;
    let reserved = state.extras.reserved().await;
    let draft = boost_draft(&state, &catalog, &key).await;
    state.engine.start(&app, state.paths.models.clone(), &catalog, &key, ctx, reserved, draft).await
}

/// The downloaded helper model for Speed boost, if it's on and one exists.
pub async fn boost_draft(state: &AppState, catalog: &models::Catalog, key: &str) -> Option<std::path::PathBuf> {
    if !state.settings.lock().await.speed_boost {
        return None;
    }
    let (model, _) = catalog.resolve(key).ok()?;
    let d = models::drafter_for(catalog, model)?;
    let (v, installed) = models::drafter_variant(d, &state.paths.models);
    installed.then(|| models::entry_path(&state.paths.models, v))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoostInfo {
    pub enabled: bool,
    /// Whether the main model has a helper at all.
    pub available: bool,
    /// Key of the helper version ("id:quant") and whether it's downloaded.
    pub helper_key: Option<String>,
    pub helper_name: Option<String>,
    pub helper_bytes: u64,
    pub installed: bool,
}

#[tauri::command]
pub async fn speed_boost_info(state: State<'_, AppState>) -> AppResult<BoostInfo> {
    let (enabled, active) = {
        let s = state.settings.lock().await;
        (s.speed_boost, s.active_model.clone())
    };
    let catalog = state.catalog.get();
    let helper = active
        .as_deref()
        .and_then(|k| catalog.resolve(k).ok())
        .and_then(|(m, _)| models::drafter_for(&catalog, m))
        .map(|d| (d, models::drafter_variant(d, &state.paths.models)));
    Ok(match helper {
        Some((d, (v, installed))) => BoostInfo {
            enabled,
            available: true,
            helper_key: Some(models::key(d, v)),
            helper_name: Some(d.name.clone()),
            helper_bytes: v.size_bytes,
            installed,
        },
        None => BoostInfo { enabled, available: false, helper_key: None, helper_name: None, helper_bytes: 0, installed: false },
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedTest {
    pub with_boost: Option<f64>,
    pub without_boost: f64,
    pub boost_kept: bool,
}

/// Measures tokens/sec with and without Speed boost on this Mac, keeps the
/// faster setting and leaves the engine running with it. Takes about a minute.
#[tauri::command]
pub async fn engine_speed_test(app: AppHandle, state: State<'_, AppState>) -> AppResult<SpeedTest> {
    let (model, ctx) = {
        let s = state.settings.lock().await;
        (s.active_model.clone(), s.context_size)
    };
    let model = model.ok_or_else(|| AppError::msg("choose a model first"))?;
    let catalog = state.catalog.get();
    let reserved = state.extras.reserved().await;
    let draft = catalog
        .resolve(&model)
        .ok()
        .and_then(|(m, _)| models::drafter_for(&catalog, m))
        .map(|d| models::drafter_variant(d, &state.paths.models))
        .filter(|(_, installed)| *installed)
        .map(|(v, _)| models::entry_path(&state.paths.models, v));
    let endpoint = || async { state.engine.endpoint().await.ok_or_else(|| AppError::msg("the engine isn't running")) };

    state.engine.start(&app, state.paths.models.clone(), &catalog, &model, ctx, reserved, None).await?;
    let without = crate::speed::measure(&state.local_http, &endpoint().await?).await?;
    let mut with = None;
    if let Some(d) = draft {
        state.engine.start(&app, state.paths.models.clone(), &catalog, &model, ctx, reserved, Some(d.clone())).await?;
        if matches!(state.engine.status().await, EngineStatus::Ready { boosted: true, .. }) {
            with = Some(crate::speed::measure(&state.local_http, &endpoint().await?).await?);
        }
        let keep = crate::speed::boost_wins(with, without);
        if !keep {
            state.engine.start(&app, state.paths.models.clone(), &catalog, &model, ctx, reserved, None).await?;
        }
        let mut s = state.settings.lock().await;
        let next = s.merged(serde_json::json!({ "speedBoost": keep }))?;
        next.save(&state.paths.settings_file)?;
        *s = next;
    }
    Ok(SpeedTest { with_boost: with, without_boost: without, boost_kept: crate::speed::boost_wins(with, without) })
}

/// Memory used by every running engine (main + extras).
async fn loaded_bytes(state: &AppState) -> u64 {
    state.engine.loaded().await.map(|l| l.needed_bytes).unwrap_or(0) + state.extras.reserved().await
}

/// Every model in memory: the main one first, then those loaded alongside.
#[tauri::command]
pub async fn models_loaded(state: State<'_, AppState>) -> AppResult<Vec<LoadedModel>> {
    let mut out = Vec::new();
    if let Some(m) = state.engine.describe().await {
        out.push(m);
    }
    for e in state.extras.all().await {
        if let Some(m) = e.describe().await {
            out.push(m);
        }
    }
    Ok(out)
}

/// Loads `key` alongside the main model so both can answer.
#[tauri::command]
pub async fn model_load(app: AppHandle, state: State<'_, AppState>, key: String) -> AppResult<()> {
    let key = load_extra(&app, &state, &key).await?;
    remember_alongside(&state, |list| {
        if !list.contains(&key) {
            list.push(key.clone());
        }
    })
    .await
}

/// Starts an extra engine for `key`; returns the canonical key.
pub async fn load_extra(app: &AppHandle, state: &AppState, key: &str) -> AppResult<String> {
    let catalog = state.catalog.get();
    let (model, variant) = catalog.resolve(key)?;
    if model.role != models::Role::Chat {
        return Err(AppError::msg(format!("{} is a helper model and can't be used for chat", model.name)));
    }
    let key = models::key(model, variant);
    if state.engine.loaded().await.is_some_and(|l| l.key == key) {
        return Err(AppError::msg(format!("{} is already the main model.", model.name)));
    }
    if state.extras.get(&key).await.is_some() {
        return Ok(key);
    }
    let reserved = loaded_bytes(state).await;
    let engine = state.extras.add(&key).await?;
    let result = engine.start(app, state.paths.models.clone(), &catalog, &key, Some(EXTRA_CONTEXT), reserved, None).await;
    if let Err(e) = result {
        state.extras.remove(app, &key).await;
        return Err(e);
    }
    Ok(key)
}

/// Updates the list of models to reload alongside the main one at launch.
async fn remember_alongside(state: &AppState, f: impl FnOnce(&mut Vec<String>)) -> AppResult<()> {
    let mut s = state.settings.lock().await;
    let mut next = s.clone();
    f(&mut next.loaded_alongside);
    next.save(&state.paths.settings_file)?;
    *s = next;
    Ok(())
}

/// Stops a model that was loaded alongside the main one.
#[tauri::command]
pub async fn model_unload(app: AppHandle, state: State<'_, AppState>, key: String) -> AppResult<bool> {
    remember_alongside(&state, |list| list.retain(|k| k != &key)).await?;
    Ok(state.extras.remove(&app, &key).await)
}

#[tauri::command]
pub async fn engine_status(state: State<'_, AppState>) -> AppResult<EngineStatus> {
    Ok(state.engine.status().await)
}

#[tauri::command]
pub async fn engine_restart(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let (model, ctx) = {
        let s = state.settings.lock().await;
        (s.active_model.clone(), s.context_size)
    };
    let model = model.ok_or_else(|| AppError::msg("choose a model first"))?;
    let catalog = state.catalog.get();
    let reserved = state.extras.reserved().await;
    let draft = boost_draft(&state, &catalog, &model).await;
    state.engine.start(&app, state.paths.models.clone(), &catalog, &model, ctx, reserved, draft).await
}

#[tauri::command]
pub async fn engine_log(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    Ok(state.engine.log_tail().await)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub request_id: String,
    pub messages: Vec<ChatMessage>,
    pub mode: Mode,
    pub thinking: ThinkingPref,
    /// Answer with this loaded model instead of the main one ("id:quant").
    #[serde(default)]
    pub model: Option<String>,
    /// Private chat: saved memories aren't used and nothing new is suggested.
    #[serde(default)]
    pub private: bool,
    /// The chat's project, whose instructions apply.
    #[serde(default)]
    pub project_id: Option<String>,
}

#[tauri::command]
pub async fn chat_send(state: State<'_, AppState>, request: ChatRequest, on_event: Channel<ChatEvent>) -> AppResult<()> {
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
    let last_user = request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");
    let plan = router::plan_turn(request.mode, request.thinking, last_user);
    let (web, user_name, memory, about_me) = {
        let s = state.settings.lock().await;
        (s.web_search, s.user_name.clone(), s.memory_enabled && !request.private, s.about_me.clone())
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
    let history = chat::fit_history(&request.messages, &system, ep.context, reserve.min(ep.context / 2));

    let cancel = state.generations.register(&request.request_id).await;
    let turn = agent::Turn {
        http: &state.local_http,
        net: &state.net,
        ep: &ep,
        system: &system,
        history: &history,
        plan,
        mode: request.mode,
        web,
        memory,
        log: &state.actions,
    };
    let result = agent::run(turn, cancel, &on_event).await;
    state.generations.finish(&request.request_id).await;
    match result {
        Err(AppError::Cancelled) => {
            let _ = on_event.send(ChatEvent::Done { finish_reason: "cancelled".into() });
            Ok(())
        }
        other => other,
    }
}

#[tauri::command]
pub async fn chat_cancel(state: State<'_, AppState>, request_id: String) -> AppResult<bool> {
    Ok(state.generations.cancel(&request_id).await)
}

// ---------- chats & memory (Phase 3) ----------

#[tauri::command]
pub fn chats_list(state: State<'_, AppState>) -> AppResult<Vec<ConversationMeta>> {
    state.db.list()
}

#[tauri::command]
pub fn chat_load(state: State<'_, AppState>, id: String) -> AppResult<Option<serde_json::Value>> {
    state.db.load(&id)
}

#[tauri::command]
pub fn chat_save(state: State<'_, AppState>, conversation: serde_json::Value) -> AppResult<()> {
    state.db.save(&conversation)
}

#[tauri::command]
pub fn chat_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.delete(&id)
}

#[tauri::command]
pub fn chat_update(state: State<'_, AppState>, id: String, patch: MetaPatch) -> AppResult<()> {
    state.db.update_meta(&id, &patch)
}

#[tauri::command]
pub fn chats_search(state: State<'_, AppState>, query: String) -> AppResult<Vec<SearchHit>> {
    state.db.search(&query, 30)
}

/// One-time move of chats kept in the old in-browser storage into the database.
/// Chats that already exist are left alone. Returns how many were imported.
#[tauri::command]
pub fn chats_import(state: State<'_, AppState>, conversations: Vec<serde_json::Value>) -> AppResult<usize> {
    let existing: std::collections::HashSet<String> = state.db.list()?.into_iter().map(|c| c.id).collect();
    let mut n = 0;
    for c in conversations {
        let id = c.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let has_messages = c.get("messages").and_then(|m| m.as_array()).is_some_and(|m| !m.is_empty());
        if !id.is_empty() && has_messages && !existing.contains(id) {
            state.db.save(&c)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Exports every chat into a new folder inside `dir`; returns that folder.
#[tauri::command]
pub fn chats_export(state: State<'_, AppState>, dir: String) -> AppResult<String> {
    let chats = state.db.export_all()?;
    let out = crate::export::export_chats(std::path::Path::new(&dir), &chats)?;
    Ok(out.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn memories_list(state: State<'_, AppState>) -> AppResult<Vec<Memory>> {
    state.db.memories()
}

#[tauri::command]
pub fn memory_add(state: State<'_, AppState>, text: String, source: Option<String>) -> AppResult<Memory> {
    state.db.add_memory(&text, source.as_deref().unwrap_or("user"))
}

#[tauri::command]
pub fn memory_update(state: State<'_, AppState>, id: String, text: String) -> AppResult<()> {
    state.db.update_memory(&id, &text)
}

#[tauri::command]
pub fn memory_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.delete_memory(&id)
}

/// Erases every saved chat and memory.
#[tauri::command]
pub fn data_wipe(state: State<'_, AppState>) -> AppResult<()> {
    state.db.wipe()
}

/// Gives a saved chat a short title, one-line summary and tags, using the main
/// model. Does nothing if it already has a summary.
#[tauri::command]
pub async fn chat_autotitle(state: State<'_, AppState>, id: String) -> AppResult<Option<ChatSummary>> {
    let Some(conv) = state.db.load(&id)? else { return Ok(None) };
    if conv.get("summary").and_then(|s| s.as_str()).is_some_and(|s| !s.is_empty()) {
        return Ok(None);
    }
    let messages = conv.get("messages").and_then(|m| m.as_array()).cloned().unwrap_or_default();
    if !messages.iter().any(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant")) {
        return Ok(None);
    }
    let Some(ep) = state.engine.endpoint().await else { return Ok(None) };
    let transcript = crate::summarize::transcript(&messages, 3000);
    let mut s = crate::summarize::summarize(&state.local_http, &ep, &transcript).await?;
    s.title = state.db.set_summary(&id, Some(&s.title), &s.summary, &s.tags)?;
    Ok(Some(s))
}

#[tauri::command]
pub fn projects_list(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    state.db.projects()
}

#[tauri::command]
pub fn project_save(state: State<'_, AppState>, project: Project) -> AppResult<Project> {
    state.db.save_project(&project)
}

#[tauri::command]
pub fn project_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.delete_project(&id)
}

#[tauri::command]
pub fn profiles_list(state: State<'_, AppState>) -> Profiles {
    Profiles::load(&state.paths.root)
}

#[tauri::command]
pub fn profile_create(state: State<'_, AppState>, name: String) -> AppResult<Profile> {
    Profiles::load(&state.paths.root).create(&state.paths.root, &name)
}

#[tauri::command]
pub fn profile_rename(state: State<'_, AppState>, id: String, name: String) -> AppResult<()> {
    Profiles::load(&state.paths.root).rename(&state.paths.root, &id, &name)
}

#[tauri::command]
pub fn profile_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    Profiles::load(&state.paths.root).delete(&state.paths.root, &id)
}

/// Makes `id` the active profile and restarts BYTE into it.
#[tauri::command]
pub async fn profile_switch(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    Profiles::load(&state.paths.root).set_active(&state.paths.root, &id)?;
    state.engine.stop().await;
    for e in state.extras.all().await {
        e.stop().await;
    }
    app.restart();
}
