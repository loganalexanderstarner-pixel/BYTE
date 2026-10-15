//! One-click updates: BYTE checks the latest release's `latest.json`, downloads
//! the new app, verifies its signature against the public key built into this
//! app (the owner signs releases with the private key, a GitHub secret), installs
//! it and restarts. Builds made before a key was set up say updates aren't set up.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const AVAILABLE_EVENT: &str = "update://available";
pub const PROGRESS_EVENT: &str = "update://progress";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub version: String,
    /// The release notes.
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub got: u64,
    pub total: Option<u64>,
}

/// The public key in the updater's config (empty until the owner sets one up).
fn pubkey(updater: Option<&serde_json::Value>) -> &str {
    updater.and_then(|u| u.get("pubkey")).and_then(serde_json::Value::as_str).unwrap_or("").trim()
}

fn configured(app: &AppHandle) -> bool {
    !pubkey(app.config().plugins.0.get("updater")).is_empty()
}

fn ready(app: &AppHandle) -> AppResult<()> {
    if !configured(app) {
        return Err(AppError::msg("Automatic updates aren't set up in this build. Download new versions from the Releases page."));
    }
    crate::offline::guard()?;
    crate::kids::grownups_only()
}

async fn find(app: &AppHandle) -> AppResult<Option<tauri_plugin_updater::Update>> {
    let updater = app.updater().map_err(|e| AppError::msg(format!("The updater couldn't start: {e}")))?;
    updater.check().await.map_err(|e| AppError::msg(format!("Couldn't check for updates: {e}")))
}

fn info(u: &tauri_plugin_updater::Update) -> UpdateInfo {
    UpdateInfo { current: u.current_version.clone(), version: u.version.clone(), notes: u.body.clone().unwrap_or_default() }
}

/// Why BYTE can't replace itself where it's running, if it can't: macOS runs a downloaded, unsigned app from a
/// hidden read-only copy ("App Translocation") until its quarantine mark is removed, and a disk image is
/// read-only too.
fn unwritable_place(exe: &str) -> Option<&'static str> {
    if exe.contains("/AppTranslocation/") {
        Some(MOVE_HELP)
    } else if exe.starts_with("/Volumes/") {
        Some("BYTE is running from the disk image (.dmg), which can't be changed. Quit BYTE, drag it into Applications, eject the disk image, then open BYTE from Applications and install the update.")
    } else {
        None
    }
}

const MOVE_HELP: &str = "macOS is running BYTE from a hidden read-only copy, because it was downloaded and isn't signed by Apple. \
To fix it once: quit BYTE, make sure it's in Applications, then run this in Terminal: \
xattr -dr com.apple.quarantine /Applications/BYTE.app — and open BYTE again. (Installing with Homebrew avoids this.)";

/// Whether this build can update itself (it was built with the owner's public key).
#[tauri::command]
pub fn update_configured(app: AppHandle) -> bool {
    configured(&app)
}

/// A newer version, or nothing when this one is the latest.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> AppResult<Option<UpdateInfo>> {
    ready(&app)?;
    Ok(find(&app).await?.as_ref().map(info))
}

/// Downloads, checks the signature, installs and restarts.
#[tauri::command]
pub async fn update_install(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    ready(&app)?;
    if let Some(help) = std::env::current_exe().ok().and_then(|p| unwritable_place(&p.to_string_lossy())) {
        return Err(AppError::msg(help));
    }
    let update = find(&app).await?.ok_or_else(|| AppError::msg("BYTE is already up to date."))?;
    let mut got = 0u64;
    let progress = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                got += chunk as u64;
                let _ = progress.emit(PROGRESS_EVENT, Progress { got, total });
            },
            || {},
        )
        .await
        .map_err(|e| {
            let e = e.to_string();
            // Missed by the path check (another read-only place): the same plain help.
            if e.contains("Read-only file system") || e.contains("os error 30") {
                AppError::msg(MOVE_HELP)
            } else {
                AppError::msg(format!("The update couldn't be installed: {e}"))
            }
        })?;
    state.engine.stop().await;
    for e in state.extras.all().await {
        e.stop().await;
    }
    app.restart();
}

/// Daily, from the scheduler: tells the UI when a newer version is out (only
/// when updates are set up, the check is on, and BYTE is online).
pub async fn daily(app: &AppHandle) {
    let on = app.state::<AppState>().settings.lock().await.update_check;
    if !on || ready(app).is_err() {
        return;
    }
    match find(app).await {
        Ok(Some(u)) => {
            let _ = app.emit(AVAILABLE_EVENT, info(&u));
        }
        Ok(None) => {}
        Err(e) => log::info!("update check: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_read_only_places() {
        assert!(unwritable_place("/private/var/folders/x/T/AppTranslocation/ABC/d/BYTE.app/Contents/MacOS/byte").is_some());
        assert!(unwritable_place("/Volumes/BYTE/BYTE.app/Contents/MacOS/byte").is_some());
        assert!(unwritable_place("/Applications/BYTE.app/Contents/MacOS/byte").is_none());
    }

    #[test]
    fn needs_a_public_key() {
        assert_eq!(pubkey(Some(&serde_json::json!({ "pubkey": "  " }))), "");
        assert_eq!(pubkey(None), "");
        assert_eq!(pubkey(Some(&serde_json::json!({ "pubkey": "dW50cnVzdGVk" }))), "dW50cnVzdGVk");
    }

    #[test]
    fn the_app_config_points_at_the_latest_release() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let endpoints = conf["plugins"]["updater"]["endpoints"].as_array().unwrap();
        assert_eq!(endpoints[0], "https://github.com/loganalexanderstarner-pixel/BYTE/releases/latest/download/latest.json");
        // The owner's minisign public key (key id 360A24738B9BC87B); updates signed with anything else are refused.
        let key = conf["plugins"]["updater"]["pubkey"].as_str().unwrap();
        use base64::Engine as _;
        let text = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(key).unwrap()).unwrap();
        assert!(text.starts_with("untrusted comment: minisign public key: 360A24738B9BC87B"), "{text}");
    }
}
