use std::collections::HashMap;
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

/// What BYTE favours when it recommends a model version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedPref {
    /// Faster answers; accepts a smaller or more compressed model.
    Speed,
    #[default]
    Balanced,
    /// The smartest model that fits, even if it's slower.
    Quality,
}

/// Engine settings measured to be fastest for one model on this Mac.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Tuning {
    /// Use the Speed boost helper.
    pub boost: bool,
    /// Keep the conversation memory (KV cache) at full precision instead of 8-bit.
    pub kv_f16: bool,
    /// Tokens processed per GPU batch while reading the prompt.
    pub ubatch: u32,
    /// Measured speeds with these settings.
    pub tokens_per_sec: f64,
    pub prompt_per_sec: f64,
    /// Chip it was measured on (re-tune on another Mac).
    pub chip: String,
    pub tested_at: i64,
    pub flash_attn: bool,
    pub draft_n_max: u32,
    pub draft_p_min: f32,
    /// Whether the thorough tune (more settings, ~5 minutes) was run.
    pub thorough: bool,
    /// Which kind of helper the look-ahead was tuned for.
    pub helper_kind: crate::models::HelperKind,
    /// Repeated-text guessing (llama.cpp ngram-mod) won.
    pub ngram: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            boost: false,
            kv_f16: false,
            ubatch: 512,
            tokens_per_sec: 0.0,
            prompt_per_sec: 0.0,
            chip: String::new(),
            tested_at: 0,
            flash_attn: true,
            draft_n_max: 16,
            draft_p_min: 0.75,
            thorough: false,
            helper_kind: crate::models::HelperKind::Draft,
            ngram: false,
        }
    }
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
    /// Free-form "About me" the user writes; included in every conversation.
    pub about_me: Option<String>,
    /// Use saved memories in answers and let BYTE suggest new ones.
    pub memory_enabled: bool,
    /// Knowledge base module: index the chosen folders and let BYTE search
    /// them ("My files" in the chat box). Off = no indexing, no tool.
    pub kb_enabled: bool,
    /// Models loaded alongside the main one; reloaded at launch.
    pub loaded_alongside: Vec<String>,
    /// Speculative decoding with a small same-family helper model.
    pub speed_boost: bool,
    pub speed_pref: SpeedPref,
    /// Measure and apply the fastest engine settings the first time a model loads.
    pub auto_tune: bool,
    /// Measured best settings per model key ("id:quant").
    pub tuning: HashMap<String, Tuning>,
    /// A BYTE cloud key is saved in the Keychain (the key itself is never here).
    pub cloud_connected: bool,
    /// Cloud address; `None` = the default in `cloud::DEFAULT_BASE`.
    pub cloud_base_url: Option<String>,
    /// The account as `GET /api/auth/me` last described it (tier, modes, budgets).
    pub cloud_account: Option<serde_json::Value>,
    /// Answer with the BYTE cloud instead of the model on this Mac.
    pub use_cloud: bool,
    /// Cloud mode id last chosen (one of the account's modes).
    pub cloud_mode: Option<String>,
    /// Which workspace the sidebar shows: "local" (this Mac), "cloud" or "both".
    pub workspace: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            onboarding_complete: false,
            active_model: None,
            context_size: None,
            default_mode: Mode::Auto,
            thinking: ThinkingPref::Auto,
            theme: "midnight".into(),
            accent: None,
            font_scale: 1.0,
            density: "comfortable".into(),
            show_stats: true,
            web_search: true,
            user_name: None,
            catalog_url: None,
            about_me: None,
            memory_enabled: true,
            kb_enabled: true,
            loaded_alongside: Vec::new(),
            speed_boost: true,
            speed_pref: SpeedPref::Balanced,
            auto_tune: true,
            tuning: HashMap::new(),
            cloud_connected: false,
            cloud_base_url: None,
            cloud_account: None,
            use_cloud: false,
            cloud_mode: None,
            workspace: "local".into(),
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
        let mut next: Settings = serde_json::from_value(current)?;
        if !matches!(next.workspace.as_str(), "local" | "cloud" | "both") {
            next.workspace = "local".into();
        }
        Ok(next)
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
