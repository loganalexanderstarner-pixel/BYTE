//! Commands for the Jobs settings. All of them are for grown-ups only (kids mode refuses).

use tauri::{AppHandle, Manager};

use super::model::{self, Job, RunRow, When};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub fn autojobs_list(state: tauri::State<'_, AppState>) -> AppResult<Vec<Job>> {
    crate::kids::grownups_only()?;
    model::list(&state.db)
}

#[tauri::command]
pub fn autojob_save(state: tauri::State<'_, AppState>, job: Job) -> AppResult<Job> {
    crate::kids::grownups_only()?;
    model::save(&state.db, job, &chrono::Local::now())
}

#[tauri::command]
pub fn autojob_delete(state: tauri::State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::kids::grownups_only()?;
    model::delete(&state.db, id)
}

#[tauri::command]
pub fn autojob_toggle(state: tauri::State<'_, AppState>, id: i64, on: bool) -> AppResult<Job> {
    crate::kids::grownups_only()?;
    model::set_enabled(&state.db, id, on, &chrono::Local::now())
}

/// Run now (or run again): returns at once; the result shows in the history and as a chat.
#[tauri::command]
pub fn autojob_run(app: AppHandle, id: i64) -> AppResult<()> {
    crate::kids::grownups_only()?;
    let state = app.state::<AppState>();
    let job = model::get(&state.db, id)?.ok_or_else(|| AppError::msg("That job no longer exists."))?;
    tauri::async_runtime::spawn(async move {
        if let Err(e) = super::run::run_job(&app, &job, "manual", None, true).await {
            log::warn!("job \"{}\": {e}", job.name);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn autojob_runs(state: tauri::State<'_, AppState>, job: Option<i64>, limit: usize) -> AppResult<Vec<RunRow>> {
    crate::kids::grownups_only()?;
    model::runs(&state.db, job, limit)
}

/// Makes the "read my digest aloud" job (off until switched on).
#[tauri::command]
pub fn autojob_digest_template(state: tauri::State<'_, AppState>, time: String, speak: bool) -> AppResult<Job> {
    crate::kids::grownups_only()?;
    let job = Job { name: "Morning digest".into(), template: "digest".into(), when: When::Daily { time }, speak, tools: vec![], max_runs_per_day: 2, ..Default::default() };
    model::save(&state.db, job, &chrono::Local::now())
}
