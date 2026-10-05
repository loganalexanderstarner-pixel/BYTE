//! Updates on phones. Tauri's updater plugin is desktop-only, so Android gets its
//! own: read `android.json` from the latest release, download the APK, check its
//! SHA-256 and hand it to Android's installer (docs/ANDROID.md, milestone A5).
//! Until then these say so instead of failing quietly.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub version: String,
    pub notes: String,
}

#[tauri::command]
pub fn update_configured(_app: AppHandle) -> bool {
    false
}

#[tauri::command]
pub async fn update_check(_app: AppHandle) -> AppResult<Option<UpdateInfo>> {
    Ok(None)
}

#[tauri::command]
pub async fn update_install(_app: AppHandle, _state: State<'_, AppState>) -> AppResult<()> {
    Err(AppError::msg("Updates on Android arrive with the in-app updater; for now, install the new APK from the Releases page."))
}

pub async fn daily(_app: &AppHandle) {}
