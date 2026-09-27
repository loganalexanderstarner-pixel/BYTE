use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

/// All on-disk locations BYTE uses. Everything lives under
/// `~/Library/Application Support/com.loganstarner.byte/` on macOS.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data: PathBuf,
    pub models: PathBuf,
    pub logs: PathBuf,
    pub settings_file: PathBuf,
}

impl Paths {
    pub fn resolve(app: &AppHandle) -> AppResult<Self> {
        let data = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::msg(format!("cannot locate app data folder: {e}")))?;
        Self::at(data)
    }

    pub fn at(data: PathBuf) -> AppResult<Self> {
        let paths = Paths {
            models: data.join("models"),
            logs: data.join("logs"),
            settings_file: data.join("settings.json"),
            data,
        };
        for dir in [&paths.data, &paths.models, &paths.logs] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(paths)
    }
}
