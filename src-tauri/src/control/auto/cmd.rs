//! Commands for Settings: saved rules.

use tauri::AppHandle;

use super::rules::{Rule, RuleStore};
use crate::error::{AppError, AppResult};

fn store(app: &AppHandle) -> RuleStore {
    RuleStore::new(super::rules_file(app))
}

/// This user's saved "don't ask again" rules.
#[tauri::command]
pub fn auto_rules_list(app: AppHandle) -> Vec<Rule> {
    store(&app).for_user(&super::user_id(&app))
}

#[tauri::command]
pub fn auto_rule_remove(app: AppHandle, id: String) -> AppResult<()> {
    store(&app).remove(&id).map_err(|e| AppError::msg(e.to_string()))
}

#[tauri::command]
pub fn auto_rules_clear(app: AppHandle) -> AppResult<()> {
    store(&app).clear(&super::user_id(&app)).map_err(|e| AppError::msg(e.to_string()))
}
