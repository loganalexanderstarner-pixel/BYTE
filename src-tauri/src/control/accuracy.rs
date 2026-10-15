//! Commands for the accuracy settings: the PC-control test (run, stop, scores) and the per-program notes (list, add, delete, clear).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use super::planner::Asker;
use super::{brain, notes, selftest};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

static TESTING: AtomicBool = AtomicBool::new(false);
static STOP_TEST: AtomicBool = AtomicBool::new(false);

pub fn testing() -> bool {
    TESTING.load(Ordering::SeqCst)
}

/// Asks one named model (its own engine), whatever the settings say: the test measures that model.
struct KeyAsker<'a> {
    app: &'a AppHandle,
    key: String,
}

impl Asker for KeyAsker<'_> {
    fn ask<'a>(&'a self, messages: &'a [Value]) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let state = tauri::Manager::state::<AppState>(self.app);
            let catalog = state.catalog.get();
            let fmt = brain::PlanFormat::Schema { max_steps: super::actions::MAX_STEPS, reasons: false };
            super::brain_for(self.app).complete(self.app, &state.paths.models, &catalog, &self.key, false, messages, fmt).await.map_err(|e| e.to_string())
        })
    }
}

/// Runs the standard tasks for one downloaded model (a key "id:quant"), saves and returns its score. Nothing is clicked or typed.
#[tauri::command]
pub async fn control_selftest(app: AppHandle, state: State<'_, AppState>, model: String) -> AppResult<selftest::Score> {
    if super::is_active() {
        return Err(AppError::msg("BYTE is using the PC right now. Run the test when it is idle."));
    }
    let catalog = state.catalog.get();
    let (m, v) = catalog.resolve(model.trim()).map_err(|_| AppError::msg("That model is not in the catalog."))?;
    if !crate::models::is_installed(&state.paths.models, v) {
        return Err(AppError::msg(format!("Download {} first, then run the test.", m.name)));
    }
    if TESTING.swap(true, Ordering::SeqCst) {
        return Err(AppError::msg("A test is already running."));
    }
    STOP_TEST.store(false, Ordering::SeqCst);
    let (key, id) = (crate::models::key(m, v), m.id.clone());
    let suite = selftest::Suite::standard();
    let asker = KeyAsker { app: &app, key: key.clone() };
    let app2 = app.clone();
    let key2 = key.clone();
    let progress = move |done: usize, total: usize| {
        let _ = app2.emit(selftest::EVENT, json!({ "model": key2, "done": done, "total": total }));
    };
    let score = selftest::run_suite(&suite, &asker, &key, &progress, &|| STOP_TEST.load(Ordering::SeqCst)).await;
    TESTING.store(false, Ordering::SeqCst);
    let stopped = STOP_TEST.load(Ordering::SeqCst);
    if !stopped && score.total > 0 {
        selftest::save_score(&state.paths.data.join(selftest::FILE), &id, &score).map_err(AppError::msg)?;
    }
    super::activity(&app, "PC-control test", !stopped, &format!("{}: {}% of {} tasks", m.name, score.percent, score.total));
    let _ = app.emit(selftest::EVENT, json!({ "model": key, "done": score.total, "total": score.total, "finished": true }));
    Ok(score)
}

#[tauri::command]
pub fn control_selftest_stop() {
    STOP_TEST.store(true, Ordering::SeqCst);
}

/// The last score of every model tested here, by catalog id.
#[tauri::command]
pub fn control_scores(state: State<'_, AppState>) -> BTreeMap<String, selftest::Score> {
    selftest::load_scores(&state.paths.data.join(selftest::FILE))
}

// ------------------------------------------------------------------ notes

#[tauri::command]
pub fn control_notes_list(app: AppHandle) -> Vec<notes::Note> {
    notes::load(&super::notes_file(&app))
}

/// A note in the person's own words ("In Notepad the font is under the gear").
#[tauri::command]
pub fn control_note_add(app: AppHandle, program: String, goal: String, text: String) -> AppResult<Vec<notes::Note>> {
    let n = notes::from_words(&program, &goal, &text).ok_or_else(|| AppError::msg("A note needs a program and some words."))?;
    let path = super::notes_file(&app);
    let mut list = notes::load(&path);
    notes::upsert(&mut list, n);
    notes::save(&path, &list).map_err(AppError::msg)?;
    Ok(list)
}

#[tauri::command]
pub fn control_note_delete(app: AppHandle, id: String) -> AppResult<Vec<notes::Note>> {
    let path = super::notes_file(&app);
    let mut list = notes::load(&path);
    list.retain(|n| n.id != id);
    notes::save(&path, &list).map_err(AppError::msg)?;
    Ok(list)
}

#[tauri::command]
pub fn control_notes_clear(app: AppHandle) {
    let _ = std::fs::remove_file(super::notes_file(&app));
}
