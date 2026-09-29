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
    /// With web on: "auto" (search when a question needs it) or "always"
    /// (search for every real question).
    #[serde(default = "default_web_mode")]
    pub web_mode: String,
    /// What BYTE calls the user (asked during setup).
    pub user_name: Option<String>,
    /// The user's town, for "near me" questions ("Pittsburgh, PA"). Never guessed.
    #[serde(default)]
    pub home_place: Option<String>,
    /// How far research goes: 0 Normal, 1 More, 2 Max (more pages and searches).
    #[serde(default)]
    pub research_depth: u8,
    /// Where to fetch model catalog updates (default: the BYTE repository).
    pub catalog_url: Option<String>,
    /// Free-form "About me" the user writes; included in every conversation.
    pub about_me: Option<String>,
    /// Use saved memories in answers and let BYTE suggest new ones.
    pub memory_enabled: bool,
    /// Knowledge base module: index the chosen folders and let BYTE search
    /// them ("My files" in the chat box). Off = no indexing, no tool.
    pub kb_enabled: bool,
    /// Kitchen module: recipes, meal plans and the recipe box.
    #[serde(default = "yes")]
    pub kitchen_enabled: bool,
    /// Recipe measures: "us" (cups, spoons, °F; the default) or "metric" (g, mL, °C).
    #[serde(default = "default_units")]
    pub measure_units: String,
    /// Web agent module: BYTE may use a hidden browser for the user (open, click, fill forms with approval).
    #[serde(default = "yes")]
    pub web_agent_enabled: bool,
    /// Review summaries ("reviews of X", "is X worth it").
    #[serde(default = "yes")]
    pub reviews_enabled: bool,
    /// Price compare ("cheapest X", "where to buy X").
    #[serde(default = "yes")]
    pub prices_enabled: bool,
    /// Spoiler-free game hints ("stuck on … in <game>").
    #[serde(default = "yes")]
    pub game_hints_enabled: bool,
    /// Check cited answers against their sources (Deep, Extended, fact-check).
    #[serde(default = "yes")]
    pub self_check: bool,
    /// Three drafts and a majority vote for hard questions (Deep, Extended).
    #[serde(default = "yes")]
    pub best_of_three: bool,
    /// Study tools: flashcards, quizzes, tutor mode, the Study panel.
    #[serde(default = "yes")]
    pub study_enabled: bool,
    /// Photo helper: a small vision model describes photos for models that can't see them.
    #[serde(default = "yes")]
    pub photo_helper: bool,
    /// The writing studio (✍️): rewrite, expand, shorten, tone, grammar.
    #[serde(default = "yes")]
    pub writing_enabled: bool,
    /// The user's writing style for "Write like me" (learned from their samples; editable).
    #[serde(default)]
    pub writing_style: String,
    /// Advanced tuning per model key ("id:quant").
    #[serde(default)]
    pub model_overrides: std::collections::HashMap<String, ModelOverride>,
    /// Below 20% battery and unplugged: Deep/Extended answer like Auto, and thinking is short.
    #[serde(default = "yes")]
    pub battery_saver: bool,
    /// Mac control: BYTE uses Notes, Reminders, Calendar, Music and settings when asked (macOS).
    #[serde(default = "yes")]
    pub mac_control: bool,
    /// "Translate … into …" in chat: part by part, for long texts, files and pages.
    #[serde(default = "yes")]
    pub translate_enabled: bool,
    /// The job search tracker (💼).
    #[serde(default = "yes")]
    pub jobs_enabled: bool,
    /// Custom assistants (🤖).
    #[serde(default = "yes")]
    pub assistants_enabled: bool,
    /// Reuse the answer to a question asked (almost exactly) in the last week
    /// (needs the search-by-meaning model; see answer_cache.rs).
    pub answer_cache: bool,
    /// Models loaded alongside the main one; reloaded at launch.
    pub loaded_alongside: Vec<String>,
    /// Models BYTE already said "a better model fits your Mac" about (said once per model).
    #[serde(default)]
    pub better_model_hint_for: Vec<String>,
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
            web_mode: default_web_mode(),
            user_name: None,
            home_place: None,
            research_depth: 0,
            catalog_url: None,
            about_me: None,
            memory_enabled: true,
            kb_enabled: true,
            kitchen_enabled: true,
            measure_units: default_units(),
            web_agent_enabled: true,
            reviews_enabled: true,
            prices_enabled: true,
            game_hints_enabled: true,
            self_check: true,
            best_of_three: true,
            study_enabled: true,
            photo_helper: true,
            writing_enabled: true,
            writing_style: String::new(),
            model_overrides: Default::default(),
            battery_saver: true,
            translate_enabled: true,
            mac_control: true,
            jobs_enabled: true,
            assistants_enabled: true,
            answer_cache: true,
            loaded_alongside: Vec::new(),
            better_model_hint_for: Vec::new(),
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


fn yes() -> bool {
    true
}

fn default_units() -> String {
    "us".into()
}

fn default_web_mode() -> String {
    "auto".into()
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

/// Advanced tuning for one model (None/empty: the model's recommended value).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelOverride {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    /// Tokens the model may think (-1 = no limit).
    pub thinking_budget: Option<i32>,
    /// Extra instructions added to every chat with this model.
    pub system_extra: String,
}

impl ModelOverride {
    /// Applies the sampling and thinking overrides to a turn's plan (values clamped to sane ranges).
    pub fn apply(&self, plan: &mut crate::router::TurnPlan) {
        for s in [&mut plan.profile.think, &mut plan.profile.plain] {
            if let Some(t) = self.temperature {
                s.temperature = t.clamp(0.0, 2.0);
            }
            if let Some(p) = self.top_p {
                s.top_p = p.clamp(0.05, 1.0);
            }
        }
        if let (true, Some(b)) = (plan.thinking, self.thinking_budget) {
            plan.thinking_budget = if b < 0 { -1 } else { b.clamp(64, 32_768) };
        }
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;

    #[test]
    fn overrides_change_sampling_and_thinking() {
        let mut plan = crate::router::plan_turn(Mode::Deep, ThinkingPref::Auto, "Explain why the sky is blue");
        assert!(plan.thinking);
        let o = ModelOverride { temperature: Some(3.0), top_p: Some(0.5), thinking_budget: Some(10), system_extra: String::new() };
        o.apply(&mut plan);
        assert_eq!((plan.profile.think.temperature, plan.profile.plain.top_p), (2.0, 0.5), "clamped");
        assert_eq!(plan.thinking_budget, 64, "at least 64 tokens");
        let mut unlimited = crate::router::plan_turn(Mode::Auto, ThinkingPref::On, "x");
        ModelOverride { thinking_budget: Some(-1), ..Default::default() }.apply(&mut unlimited);
        assert_eq!(unlimited.thinking_budget, -1);
        let mut off = crate::router::plan_turn(Mode::Fast, ThinkingPref::Off, "x");
        let before = off;
        ModelOverride { thinking_budget: Some(900), ..Default::default() }.apply(&mut off);
        assert_eq!(off, before, "no thinking, nothing to budget");
    }

    #[test]
    fn overrides_are_saved_in_camel_case() {
        let mut s = Settings::default();
        s.model_overrides.insert("qwen3-8b:Q4_K_M".into(), ModelOverride { temperature: Some(0.3), system_extra: "Be brief.".into(), ..Default::default() });
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["modelOverrides"]["qwen3-8b:Q4_K_M"]["temperature"], serde_json::json!(0.3f32));
        assert_eq!(v["batterySaver"], true);
        let back: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(back.model_overrides["qwen3-8b:Q4_K_M"].system_extra, "Be brief.");
    }
}
