use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Fast,
    Auto,
    Deep,
    Extended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingPref {
    Auto,
    On,
    Off,
}

/// Persistent user settings. Unknown or missing fields fall back to defaults so
/// older settings files keep loading after upgrades.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub onboarding_complete: bool,
    pub active_model: Option<String>,
    /// Context window override in tokens. `None` lets the RAM planner decide.
    pub context_size: Option<u32>,
    pub default_mode: Mode,
    pub thinking: ThinkingPref,
    pub theme: String,
    pub accent: Option<String>,
    pub font_scale: f32,
    pub density: String,
    pub show_stats: bool,
    /// Let BYTE search and read the web when a question needs it.
    pub web_search: bool,
    /// What BYTE calls the user (asked during setup).
    pub user_name: Option<String>,
    /// Where to fetch model catalog updates (default: the BYTE repository).
    pub catalog_url: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            onboarding_complete: false,
            active_model: None,
            context_size: None,
            default_mode: Mode::Auto,
            thinking: ThinkingPref::Auto,
            theme: "neon-night".into(),
            accent: None,
            font_scale: 1.0,
            density: "comfortable".into(),
            show_stats: true,
            web_search: true,
            user_name: None,
            catalog_url: None,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("settings file unreadable, using defaults: {e}");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    /// Writes atomically (temp file + rename) so a crash never leaves a
    /// half-written settings file behind.
    pub fn save(&self, path: &Path) -> AppResult<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Applies a partial JSON update from the UI.
    pub fn merged(&self, patch: serde_json::Value) -> AppResult<Settings> {
        let mut current = serde_json::to_value(self)?;
        if let (Some(obj), serde_json::Value::Object(p)) = (current.as_object_mut(), patch) {
            for (k, v) in p {
                obj.insert(k, v);
            }
        }
        Ok(serde_json::from_value(current)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_unrelated_fields() {
        let s = Settings::default();
        let m = s
            .merged(serde_json::json!({ "theme": "paper", "defaultMode": "fast" }))
            .unwrap();
        assert_eq!(m.theme, "paper");
        assert_eq!(m.default_mode, Mode::Fast);
        assert_eq!(m.thinking, ThinkingPref::Auto);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings::default();
        s.active_model = Some("qwen3-14b".into());
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).active_model.as_deref(), Some("qwen3-14b"));
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(!Settings::load(&path).onboarding_complete);
    }
}
