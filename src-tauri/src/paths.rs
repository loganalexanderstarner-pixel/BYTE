use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

/// All on-disk locations BYTE uses. Everything lives under
/// `~/Library/Application Support/com.loganstarner.byte/` on macOS.
///
/// `root` holds what every profile shares (models, the model catalog, engine
/// PID files, logs); `data` is the active profile's folder (database, key,
/// settings, action log).
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub data: PathBuf,
    pub models: PathBuf,
    pub logs: PathBuf,
    pub settings_file: PathBuf,
}

/// Where BYTE keeps files the user may want to see (notes, saved automation output). On a computer that is the
/// Documents folder; on a phone, Android doesn't let an app write into the shared Documents folder without broad
/// storage access, so the app's own folder is used (still private to BYTE, and backed up with its data).
pub fn user_documents_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = if cfg!(mobile) { app.path().app_data_dir().map(|d| d.join("Documents")) } else { app.path().document_dir() };
    dir.map_err(|e| AppError::msg(format!("No Documents folder: {e}")))
}

impl Paths {
    pub fn resolve(app: &AppHandle) -> AppResult<Self> {
        let data = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::msg(format!("cannot locate app data folder: {e}")))?;
        Self::at(data)
    }

    pub fn at(root: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&root)?;
        let profile = crate::profiles::Profiles::load(&root).active;
        let data = crate::profiles::Profiles::dir(&root, &profile);
        let paths = Paths {
            models: root.join("models"),
            logs: root.join("logs"),
            settings_file: data.join("settings.json"),
            data,
            root,
        };
        for dir in [&paths.data, &paths.models, &paths.logs] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(paths)
    }
}
