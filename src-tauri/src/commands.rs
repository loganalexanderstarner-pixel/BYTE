//! Thin `#[tauri::command]` wrappers. Logic lives in the modules they call.

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::chat::{ChatEvent, ChatMessage};
use crate::db::{ConversationMeta, Memory, MetaPatch, Project, SearchHit};
use crate::profiles::{Profile, Profiles};
use crate::summarize::ChatSummary;
use crate::engine::{EngineStatus, LoadedModel, DEFAULT_CONTEXT, EXTRA_CONTEXT};
use crate::error::{AppError, AppResult};
use crate::models::{self, ModelStatus};
use crate::settings::{Mode, Settings, ThinkingPref};
use crate::state::AppState;
use crate::system::{self, SystemInfo};

// ---------- documents made on this Mac (docs.rs) ----------

/// Text of a source document the user picked (empty if none or unreadable).
async fn reference_text(path: Option<String>) -> AppResult<String> {
    let Some(p) = path.filter(|p| !p.is_empty()) else { return Ok(String::new()) };
    let f = tokio::task::spawn_blocking(move || crate::files::ingest(std::path::Path::new(&p))).await.map_err(|e| AppError::msg(e.to_string()))??;
    Ok(f.text)
}

async fn main_endpoint(state: &AppState) -> AppResult<crate::engine::Endpoint> {
    state.engine.endpoint().await.ok_or_else(|| AppError::msg("The model on this Mac isn't ready yet. Load one in Settings → Models, or use BYTE Cloud."))
}

#[tauri::command]
pub async fn doc_outline(state: State<'_, AppState>, kind: crate::docs::DocKind, prompt: String, reference_path: Option<String>) -> AppResult<crate::docs::Outline> {
    let ep = main_endpoint(&state).await?;
    let reference = reference_text(reference_path).await?;
    crate::docs::outline(&state.local_http, &ep, kind, prompt.trim(), &reference).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocWriteRequest {
    pub request_id: String,
    pub kind: crate::docs::DocKind,
    pub prompt: String,
    pub outline: crate::docs::Outline,
    /// Search the web first and cite what was read.
    #[serde(default)]
    pub research: bool,
    #[serde(default)]
    pub reference_path: Option<String>,
}

/// Writes an approved outline section by section (stop it with `chat_cancel`).
#[tauri::command]
pub async fn doc_write(state: State<'_, AppState>, request: DocWriteRequest, on_event: Channel<crate::docs::DocEvent>) -> AppResult<crate::docs::DocSpec> {
    let ep = main_endpoint(&state).await?;
    let reference = reference_text(request.reference_path.clone()).await?;
    let research = if request.research {
        let _ = on_event.send(crate::docs::DocEvent::Phase { text: "Searching the web and reading pages".into() });
        let cloud = if state.settings.lock().await.cloud_connected { state.cloud_client().await.ok() } else { None };
        let query = format!("{} {}", request.outline.title, request.prompt.chars().take(120).collect::<String>());
        crate::docs::research(&state.net, cloud.as_ref(), &query).await
    } else {
        crate::docs::Research::default()
    };
    let cancel = state.generations.register(&request.request_id).await;
    let result = crate::docs::write(&state.local_http, &ep, request.kind, &request.outline, &research, &reference, &cancel, &on_event).await;
    state.generations.finish(&request.request_id).await;
    result
}

/// Saves a finished file (made by the UI's renderers) where the user chose.
#[tauri::command]
pub async fn doc_save(path: String, data: String) -> AppResult<()> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|e| AppError::msg(format!("bad file data: {e}")))?;
    if let Some(dir) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, bytes)?;
    Ok(())
}

/// Saves a trip's calendar file (.ics) and opens it, so Calendar (or the
/// system's calendar app) offers to add the events. Only .ics files.
#[tauri::command]
pub async fn calendar_open(app: tauri::AppHandle, path: String, data: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    if !path.to_lowercase().ends_with(".ics") {
        return Err(AppError::msg("only calendar (.ics) files can be opened this way"));
    }
    doc_save(path.clone(), data).await?;
    app.opener().open_path(&path, None::<&str>).map_err(|e| AppError::msg(format!("couldn't open the calendar file: {e}")))
}

// ---------- web agent (web_agent/) ----------

/// Approve or deny the web agent's approval card. False when it's no longer waiting.
#[tauri::command]
pub fn agent_approve(id: String, ok: bool) -> bool {
    crate::web_agent::answer(&id, ok)
}

/// Shows or hides the web agent's browser (to watch, or to take over a login).
#[tauri::command]
pub fn agent_show(app: tauri::AppHandle, visible: bool) -> AppResult<bool> {
    crate::web_agent::browser::show(&app, visible)
}

/// Opens a file the web agent saved (documents and pictures only) or shows
/// it in Finder. Only files in Downloads/BYTE.
#[tauri::command]
pub fn agent_file(app: tauri::AppHandle, path: String, open: bool) -> AppResult<()> {
    use tauri::Manager;
    use tauri_plugin_opener::OpenerExt;
    let dir = app.path().download_dir().or_else(|_| app.path().home_dir().map(|h| h.join("Downloads"))).map_err(|e| AppError::msg(e.to_string()))?.join("BYTE");
    let file = std::path::Path::new(&path).canonicalize().map_err(|_| AppError::msg("that file isn't there any more"))?;
    let dir = dir.canonicalize().map_err(|_| AppError::msg("the Downloads/BYTE folder isn't there"))?;
    if !file.starts_with(&dir) {
        return Err(AppError::msg("only files BYTE saved in Downloads/BYTE"));
    }
    let viewable = file.extension().and_then(|e| e.to_str()).map(|e| crate::web_agent::VIEWABLE.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false);
    if open && viewable {
        app.opener().open_path(file.to_string_lossy(), None::<&str>).map_err(|e| AppError::msg(format!("couldn't open it: {e}")))
    } else {
        // Anything else (programs, installers, archives) is only shown, never run.
        app.opener().reveal_item_in_dir(&file).map_err(|e| AppError::msg(format!("couldn't show it: {e}")))
    }
}

// ---------- study decks (study.rs) ----------

#[tauri::command]
pub async fn decks_list(state: State<'_, AppState>) -> AppResult<Vec<crate::study::DeckSummary>> {
    crate::study::decks_list(&state.db)
}

#[tauri::command]
pub async fn deck_save(state: State<'_, AppState>, name: String, cards: Vec<crate::study::Card>) -> AppResult<i64> {
    crate::study::deck_save(&state.db, &name, &cards)
}

#[tauri::command]
pub async fn deck_cards(state: State<'_, AppState>, id: i64) -> AppResult<Vec<crate::study::StudyCard>> {
    crate::study::deck_cards(&state.db, id)
}

#[tauri::command]
pub async fn study_queue(state: State<'_, AppState>, id: i64) -> AppResult<Vec<crate::study::StudyCard>> {
    crate::study::study_queue(&state.db, id, 20)
}

#[tauri::command]
pub async fn card_review(state: State<'_, AppState>, id: i64, grade: u8) -> AppResult<crate::study::StudyCard> {
    crate::study::card_review(&state.db, id, grade)
}

#[tauri::command]
pub async fn deck_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::study::deck_delete(&state.db, id)
}

#[tauri::command]
pub async fn card_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::study::card_delete(&state.db, id)
}

/// A deck as an Anki import file (text); the UI saves it.
#[tauri::command]
pub async fn deck_export(state: State<'_, AppState>, id: i64) -> AppResult<String> {
    crate::study::deck_export(&state.db, id)
}

// ---------- recipe box (kitchen.rs) ----------

#[tauri::command]
pub async fn recipes_list(state: State<'_, AppState>, query: Option<String>) -> AppResult<Vec<crate::kitchen::SavedRecipe>> {
    crate::kitchen::recipes_list(&state.db, query.as_deref())
}

#[tauri::command]
pub async fn recipe_save(state: State<'_, AppState>, recipe: serde_json::Value) -> AppResult<i64> {
    crate::kitchen::recipe_save(&state.db, &recipe)
}

#[tauri::command]
pub async fn recipe_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::kitchen::recipe_delete(&state.db, id)
}

// ---------- knowledge base (kb.rs) ----------

/// The knowledge base at a glance: folders, and the search model's state.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KbStatus {
    pub sources: Vec<crate::kb::Source>,
    /// Download key and size of the embedding model ("search by meaning").
    pub embed_key: Option<String>,
    pub embed_bytes: u64,
    pub embed_installed: bool,
    pub embed_running: bool,
}

#[tauri::command]
pub async fn kb_status(state: State<'_, AppState>) -> AppResult<KbStatus> {
    let catalog = state.catalog.get();
    let embed_key = crate::embed::Embedder::model_key(&catalog);
    let embed_bytes = embed_key.as_deref().and_then(|k| catalog.resolve(k).ok()).map(|(_, v)| v.size_bytes).unwrap_or(0);
    Ok(KbStatus {
        sources: crate::kb::sources(&state.db)?,
        embed_installed: crate::embed::Embedder::installed(&catalog, &state.paths.models),
        embed_running: state.embedder.running().await,
        embed_key,
        embed_bytes,
    })
}

/// The photo helper (a small vision model for models that can't see photos).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookerStatus {
    /// The helper model in use, when one is downloaded ("id:quant").
    pub model: Option<String>,
    /// What to download for the default helper (model, then its image adapter).
    pub downloads: Vec<String>,
    pub download_bytes: u64,
}

#[tauri::command]
pub fn looker_status(state: State<'_, AppState>) -> AppResult<LookerStatus> {
    let catalog = state.catalog.get();
    Ok(LookerStatus {
        model: crate::looker::choose(&catalog, &state.paths.models),
        downloads: crate::looker::downloads(&catalog),
        download_bytes: crate::looker::download_bytes(&catalog),
    })
}

/// Adds a folder and starts reading it in the background.
#[tauri::command]
pub async fn kb_add(app: AppHandle, state: State<'_, AppState>, path: String) -> AppResult<i64> {
    let id = crate::kb::add_source(&state.db, std::path::Path::new(&path))?;
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::kb::index(&app, Some(id)).await {
            log::warn!("knowledge base: indexing {path} failed: {e}");
        }
    });
    Ok(id)
}

#[tauri::command]
pub async fn kb_remove(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::kb::remove_source(&state.db, id)
}

/// Re-reads changed files (one folder, or all), in the background.
#[tauri::command]
pub async fn kb_reindex(app: AppHandle, id: Option<i64>) -> AppResult<()> {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::kb::index(&app, id).await {
            log::warn!("knowledge base: re-index failed: {e}");
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn kb_search(app: AppHandle, query: String, limit: Option<usize>) -> AppResult<Vec<crate::kb::Hit>> {
    crate::kb::search(&app, &query, limit.unwrap_or(8).min(30)).await
}

/// Reads a file the user attached to a local chat (text for the model, or a photo).
#[tauri::command]
pub async fn file_ingest(path: String) -> AppResult<crate::files::Ingested> {
    tokio::task::spawn_blocking(move || crate::files::ingest(std::path::Path::new(&path))).await.map_err(|e| AppError::msg(e.to_string()))?
}

/// Apps using the most memory right now (for "quit these to make room").
#[tauri::command]
pub async fn memory_report() -> crate::memory::MemoryReport {
    tokio::task::spawn_blocking(crate::memory::report).await.unwrap_or(crate::memory::MemoryReport { total_bytes: 0, available_bytes: 0, apps: vec![] })
}

/// Quits one of the apps in the memory report (the user clicked Quit).
#[tauri::command]
pub async fn app_quit(name: String) -> AppResult<()> {
    tokio::task::spawn_blocking(move || crate::memory::quit_app(&name)).await.map_err(|e| AppError::msg(e.to_string()))?
}

#[tauri::command]
pub fn system_info(state: State<'_, AppState>) -> SystemInfo {
    system::system_info(&state.paths.data)
}

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
pub async fn settings_update(app: AppHandle, state: State<'_, AppState>, patch: serde_json::Value) -> AppResult<Settings> {
    let mut s = state.settings.lock().await;
    let next = s.merged(patch)?;
    crate::quick::check(&next)?;
    if next.open_at_login != s.open_at_login {
        crate::background::apply_login(&app, next.open_at_login).map_err(AppError::msg)?;
    }
    next.save(&state.paths.settings_file)?;
    *s = next.clone();
    crate::quick::apply_shortcuts(&app, &next);
    crate::quick::apply_tray(&app, next.menu_bar_icon);
    Ok(next)
}

#[tauri::command]
pub async fn models_list(state: State<'_, AppState>) -> AppResult<Vec<ModelStatus>> {
    let settings = state.settings.lock().await.clone();
    let ctx = settings.context_size.unwrap_or(DEFAULT_CONTEXT);
    let active = state.downloads.active_ids().await;
    let catalog = state.catalog.get();
    let info = models::calibrate(system::system_info(&state.paths.data).with_settings(&settings), &catalog);
    let loaded_bytes = loaded_bytes(&state).await;
    Ok(models::list(&catalog, &models::ListContext { models_dir: &state.paths.models, info: &info, ctx, downloading: &active, loaded_bytes }))
}

/// The best chat model + version for this Mac, as a key like "qwen3.5-9b:Q6_K".
#[tauri::command]
pub async fn model_recommend(state: State<'_, AppState>) -> AppResult<Option<String>> {
    let settings = state.settings.lock().await.clone();
    let ctx = settings.context_size.unwrap_or(DEFAULT_CONTEXT);
    let catalog = state.catalog.get();
    let info = models::calibrate(system::system_info(&state.paths.data).with_settings(&settings), &catalog);
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
    let (repo, variant, key) = catalog.download_target(&key)?;
    let dir = models::download_dir(&state.paths.models, &key);
    std::fs::create_dir_all(&dir)?;
    state.downloads.start(app, state.net.clone(), dir, repo, variant, key).await
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
    if key.ends_with(&format!(":{}", models::HEAD_QUANT)) || key.ends_with(&format!(":{}", models::VISION_QUANT)) {
        let (_, head, _) = catalog.download_target(&key)?;
        return models::delete(&models::download_dir(&state.paths.models, &key), &head);
    }
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
    let opts = crate::tune::launch_opts(&state, &catalog, &key).await;
    state.engine.start(&app, state.paths.models.clone(), &catalog, &key, ctx, reserved, opts).await?;
    auto_tune(&app);
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoostInfo {
    pub enabled: bool,
    /// Whether the main model has a helper at all.
    pub available: bool,
    /// Download key of the helper ("id:quant", or "id:speed-head") and whether it's downloaded.
    pub helper_key: Option<String>,
    pub helper_name: Option<String>,
    pub helper_bytes: u64,
    pub installed: bool,
    /// "draft" (separate small model), "mtp", "eagle3" or "dspark" (the model's own head).
    pub kind: Option<models::HelperKind>,
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
        .and_then(|(m, _)| models::helper_for(&catalog, m, &state.paths.models));
    Ok(match helper {
        Some(h) => BoostInfo {
            enabled,
            available: true,
            installed: h.installed(&state.paths.models),
            helper_bytes: h.variant.size_bytes,
            helper_key: Some(h.key),
            helper_name: Some(h.name),
            kind: Some(h.kind),
        },
        None => BoostInfo { enabled, available: false, helper_key: None, helper_name: None, helper_bytes: 0, installed: false, kind: None },
    })
}

#[tauri::command]
pub async fn gpu_share_info() -> AppResult<system::GpuShare> {
    Ok(system::gpu_share())
}

/// Raises or resets the GPU's share of memory (asks for the admin password).
/// The UI restarts the engine afterwards so the model can use it.
#[tauri::command]
pub async fn gpu_share_set(raise: bool) -> AppResult<system::GpuShare> {
    tauri::async_runtime::spawn_blocking(move || system::set_gpu_share(raise)).await.map_err(|e| AppError::msg(e.to_string()))?
}

/// Measures and keeps the fastest engine settings for the active model on
/// this Mac (1–2 minutes; progress on `engine://tune`).
#[tauri::command]
pub async fn engine_tune(app: AppHandle, state: State<'_, AppState>, thorough: Option<bool>) -> AppResult<crate::settings::Tuning> {
    crate::tune::run(&app, &state, None, thorough.unwrap_or(false)).await
}

/// Tunes every downloaded model that fits this Mac, then returns to the one in use.
#[tauri::command]
pub async fn engine_tune_all(app: AppHandle, state: State<'_, AppState>, thorough: Option<bool>) -> AppResult<usize> {
    crate::tune::run_all(&app, &state, thorough.unwrap_or(false)).await
}

/// Tunes the active model in the background if it hasn't been tuned on this Mac.
pub fn auto_tune(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let (enabled, key) = {
            let s = state.settings.lock().await;
            (s.auto_tune, s.active_model.clone())
        };
        let Some(key) = key.filter(|_| enabled) else { return };
        let catalog = state.catalog.get();
        let Ok((m, v)) = catalog.resolve(&key) else { return };
        if crate::tune::saved(&state, &models::key(m, v)).await.is_some() {
            return;
        }
        if let Err(e) = crate::tune::run(&app, &state, None, false).await {
            log::warn!("automatic tuning didn't finish: {e}");
        }
    });
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
    let result = engine.start(app, state.paths.models.clone(), &catalog, &key, Some(EXTRA_CONTEXT), reserved, Default::default()).await;
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
    let opts = crate::tune::launch_opts(&state, &catalog, &model).await;
    state.engine.start(&app, state.paths.models.clone(), &catalog, &model, ctx, reserved, opts).await
}

#[tauri::command]
pub async fn engine_log(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    Ok(state.engine.log_tail().await)
}

#[derive(Debug, Clone, Deserialize)]
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
    /// The custom assistant the chat was started with (its instructions apply).
    #[serde(default)]
    pub assistant_id: Option<String>,
    /// Answer on the BYTE cloud instead of this Mac.
    #[serde(default)]
    pub cloud: Option<crate::cloud::cmd::CloudTurn>,
    /// Don't reuse an earlier answer ("Ask again").
    #[serde(default)]
    pub fresh: bool,
    /// A job asked for with a button (e.g. Fact-check).
    #[serde(default)]
    pub task: Option<crate::agent::Task>,
}

/// Remembers a finished first answer for instant reuse (the UI calls this).
#[tauri::command]
pub async fn answer_cache_put(state: State<'_, AppState>, question: String, mode: Mode, answer: String, sources: serde_json::Value) -> AppResult<()> {
    let catalog = state.catalog.get();
    if !state.settings.lock().await.answer_cache
        || answer.trim().is_empty()
        || crate::answer_cache::cacheable(&[ChatMessage::new("user", question.clone())]).is_none()
        || !crate::embed::Embedder::installed(&catalog, &state.paths.models)
    {
        return Ok(());
    }
    let Some(app) = state.app.get() else { return Ok(()) };
    let v = state.embedder.embed(app, &state.paths.models, &catalog, &[question.clone()], crate::embed::Purpose::Query).await?;
    let mode = serde_json::to_value(mode)?.as_str().unwrap_or("auto").to_string();
    crate::answer_cache::put(&state.db, &question, &mode, &v[0], &answer, &sources.to_string())
}

#[tauri::command]
pub async fn answer_cache_clear(state: State<'_, AppState>) -> AppResult<()> {
    crate::answer_cache::clear(&state.db)
}

#[tauri::command]
pub async fn chat_send(state: State<'_, AppState>, request: ChatRequest, on_event: Channel<ChatEvent>) -> AppResult<()> {
    crate::backend::answer(&state, request, &on_event).await
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

/// Meters for the tuning panel.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStats {
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub engine_rss_bytes: Option<u64>,
    pub gpu_budget_bytes: u64,
    pub battery: Option<Battery>,
    pub battery_saving: bool,
    /// The loaded model's recommended sampling (for the tuning panel's sliders).
    pub recommended: Option<Recommended>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommended {
    pub temperature: f32,
    pub top_p: f32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

#[tauri::command]
pub async fn engine_live(state: State<'_, AppState>) -> AppResult<LiveStats> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_memory();
    let engine_rss_bytes = state.engine.pid().and_then(|pid| {
        let pid = Pid::from_u32(pid);
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        sys.process(pid).map(|p| p.memory())
    });
    let settings = state.settings.lock().await.clone();
    let info = system::system_info(&state.paths.data).with_settings(&settings);
    let battery = system::battery();
    let catalog = state.catalog.get();
    let recommended = state.engine.loaded().await.and_then(|l| catalog.resolve(&l.key).ok().map(|(m, _)| crate::modelcfg::profile(m))).map(|p| Recommended {
        temperature: (p.plain.temperature * 100.0).round() / 100.0,
        top_p: (p.plain.top_p * 100.0).round() / 100.0,
    });
    Ok(LiveStats {
        recommended,
        ram_used_bytes: sys.used_memory(),
        ram_total_bytes: sys.total_memory(),
        engine_rss_bytes,
        gpu_budget_bytes: info.gpu_budget_bytes,
        battery_saving: settings.battery_saver && battery.is_some_and(|(p, c)| p < 20 && !c),
        battery: battery.map(|(percent, charging)| Battery { percent, charging }),
    })
}
