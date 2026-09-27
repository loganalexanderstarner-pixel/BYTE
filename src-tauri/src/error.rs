use serde::{Serialize, Serializer};

/// Error type returned by every Tauri command. Serializes as a plain message so
/// the UI can show it directly.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Msg(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Tauri(#[from] tauri::Error),
    #[error("cancelled")]
    Cancelled,
}

impl AppError {
    pub fn msg(m: impl Into<String>) -> Self {
        AppError::Msg(m.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
