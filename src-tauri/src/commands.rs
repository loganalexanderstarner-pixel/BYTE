//! Thin `#[tauri::command]` wrappers. Logic lives in the modules they call.

use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::chat::{self, ChatEvent, ChatMessage};
use crate::engine::{EngineStatus, DEFAULT_CONTEXT};
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
    let ctx = state.settings.lock().await.context_size.unwrap_or(DEFAULT_CONTEXT);
    let active = state.downloads.active_ids().await;
    Ok(models::list(&state.paths.models, ctx, &active))
}

#[tauri::command]
pub async fn model_download(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.downloads.start(app, state.net.clone(), state.paths.models.clone(), id).await
}

#[tauri::command]
pub async fn model_pause(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.downloads.pause(&id).await;
    Ok(())
}

#[tauri::command]
pub async fn model_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.downloads.pause(&id).await;
    let active = state.settings.lock().await.active_model.clone();
    if active.as_deref() == Some(id.as_str()) {
        state.engine.stop().await;
        let _ = tauri::Emitter::emit(&app, crate::engine::STATUS_EVENT, EngineStatus::NoModel);
    }
    models::delete(&state.paths.models, &id)
}

/// Makes `id` the active chat model and (re)starts the engine with it.
#[tauri::command]
pub async fn model_activate(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let entry = models::find(&id)?;
    if entry.role != models::Role::Chat {
        return Err(AppError::msg(format!("{} is a helper model and can't be used for chat", entry.name)));
    }
    let ctx = {
        let mut s = state.settings.lock().await;
        let next = s.merged(serde_json::json!({ "activeModel": id }))?;
        next.save(&state.paths.settings_file)?;
        *s = next;
        s.context_size
    };
    state.engine.start(&app, state.paths.models.clone(), &id, ctx).await
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
    state.engine.start(&app, state.paths.models.clone(), &model, ctx).await
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
}

#[tauri::command]
pub async fn chat_send(state: State<'_, AppState>, request: ChatRequest, on_event: Channel<ChatEvent>) -> AppResult<()> {
    let ep = state
        .engine
        .endpoint()
        .await
        .ok_or_else(|| AppError::msg("The AI engine isn't ready yet. It usually takes a few seconds after launch."))?;
    let last_user = request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");
    let plan = router::plan_turn(request.mode, request.thinking, last_user);
    let (web, user_name) = {
        let s = state.settings.lock().await;
        (s.web_search, s.user_name.clone())
    };
    let system = prompt::system_prompt(chrono::Local::now(), request.mode, web, user_name.as_deref());
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
