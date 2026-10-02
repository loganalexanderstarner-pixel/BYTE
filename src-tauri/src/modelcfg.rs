//! Per-model settings for the best answers: each family's recommended
//! sampling (from the publishers' model cards) and how it thinks.
//!
//! Using one family's sampling for another hurts quality: Gemma is tuned for
//! temperature 1.0, Mistral Small for 0.15, Qwen for 0.6–0.7 with top-k 20.

use crate::models::CatalogModel;
use crate::settings::Mode;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampling {
    pub temperature: f32,
    pub top_p: f32,
    /// 0 disables top-k.
    pub top_k: u32,
    pub min_p: f32,
    pub repeat_penalty: f32,
}

const fn s(temperature: f32, top_p: f32, top_k: u32, min_p: f32) -> Sampling {
    Sampling { temperature, top_p, top_k, min_p, repeat_penalty: 1.0 }
}

/// How a model's thinking is controlled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thinking {
    /// On/off per turn via `enable_thinking` (Qwen3/3.5, Granite, EXAONE 4, SmolLM3…).
    Toggle,
    /// Always reasons first (DeepSeek-R1 distills, QwQ, Phi-4 reasoning, *-Thinking).
    Always,
    /// Reasoning effort low/medium/high (gpt-oss).
    Effort,
    /// Doesn't think.
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelProfile {
    pub thinking: Thinking,
    /// Sampling for turns with thinking.
    pub think: Sampling,
    /// Sampling for direct answers.
    pub plain: Sampling,
}

impl Default for ModelProfile {
    /// Qwen3's recommendations (BYTE's default family).
    fn default() -> Self {
        ModelProfile { thinking: Thinking::Toggle, think: s(0.6, 0.95, 20, 0.0), plain: s(0.7, 0.8, 20, 0.0) }
    }
}

impl ModelProfile {
    pub fn sampling(&self, thinking: bool) -> Sampling {
        if thinking {
            self.think
        } else {
            self.plain
        }
    }

    /// Whether this turn thinks, given what BYTE's router wanted.
    pub fn thinks(&self, wanted: bool) -> bool {
        match self.thinking {
            Thinking::Toggle | Thinking::Effort => wanted,
            Thinking::Always => true,
            Thinking::Never => false,
        }
    }

    /// Extra template arguments for this turn.
    pub fn template_kwargs(&self, thinking: bool, mode: Mode) -> serde_json::Value {
        match self.thinking {
            Thinking::Toggle => serde_json::json!({ "enable_thinking": thinking }),
            Thinking::Effort => serde_json::json!({ "reasoning_effort": match (thinking, mode) {
                (false, _) | (_, Mode::Fast) => "low",
                (_, Mode::Auto) => "medium",
                _ => "high",
            } }),
            _ => serde_json::json!({}),
        }
    }
}

/// Models that always reason before answering.
fn always_thinks(id: &str) -> bool {
    ["deepseek-r1", "qwq", "phi-4-reasoning", "phi-4-mini-reasoning", "magistral", "exaone-deep", "-thinking", "-think", "r1-zero", "-reasoning", "z1-", "apriel", "prover"]
        .iter()
        .any(|k| id.contains(k))
}

/// The best settings for `model`, by family.
pub fn profile(model: &CatalogModel) -> ModelProfile {
    let id = model.id.as_str();
    let has = |p: &[&str]| p.iter().any(|x| id.contains(x));
    let thinking = if has(&["gpt-oss"]) {
        Thinking::Effort
    } else if always_thinks(id) {
        Thinking::Always
    } else if model.thinking {
        Thinking::Toggle
    } else {
        Thinking::Never
    };
    let (think, plain) = if has(&["gpt-oss"]) {
        (s(1.0, 1.0, 0, 0.0), s(1.0, 1.0, 0, 0.0))
    } else if has(&["deepseek-r1", "qwq", "r1-zero"]) {
        (s(0.6, 0.95, 40, 0.0), s(0.6, 0.95, 40, 0.0))
    } else if has(&["phi-4-reasoning", "phi-4-mini-reasoning"]) {
        (s(0.8, 0.95, 50, 0.0), s(0.8, 0.95, 50, 0.0))
    } else if has(&["magistral"]) {
        (s(0.7, 0.95, 0, 0.0), s(0.7, 0.95, 0, 0.0))
    } else if has(&["qwen", "ornith", "mimo"]) {
        (s(0.6, 0.95, 20, 0.0), s(0.7, 0.8, 20, 0.0))
    } else if has(&["gemma"]) {
        (s(1.0, 0.95, 64, 0.0), s(1.0, 0.95, 64, 0.0))
    } else if has(&["mistral-small", "devstral", "ministral", "mistral-medium", "mistral-large"]) {
        (s(0.15, 0.95, 0, 0.0), s(0.15, 0.95, 0, 0.0))
    } else if has(&["mistral-nemo", "mistral-7b"]) {
        (s(0.3, 0.95, 0, 0.0), s(0.3, 0.95, 0, 0.0))
    } else if has(&["llama", "hermes", "nemotron", "tulu"]) {
        (s(0.6, 0.95, 0, 0.0), s(0.6, 0.9, 0, 0.0))
    } else if has(&["lfm"]) {
        let l = Sampling { temperature: 0.3, top_p: 1.0, top_k: 0, min_p: 0.15, repeat_penalty: 1.05 };
        (l, l)
    } else if has(&["granite"]) {
        (s(0.6, 0.95, 0, 0.0), s(0.3, 0.9, 0, 0.0))
    } else if has(&["smollm"]) {
        (s(0.6, 0.95, 0, 0.0), s(0.6, 0.95, 0, 0.0))
    } else if has(&["phi-"]) {
        (s(0.7, 0.95, 0, 0.0), s(0.7, 0.95, 0, 0.0))
    } else {
        // A safe general default: slight randomness, min-p cuts unlikely tokens.
        (s(0.6, 0.95, 40, 0.05), s(0.7, 0.9, 40, 0.05))
    };
    ModelProfile { thinking, think, plain }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Catalog;

    fn p(id: &str) -> ModelProfile {
        let c = Catalog::embedded();
        profile(c.model(id).unwrap_or_else(|| panic!("{id} not in catalog")))
    }

    #[test]
    fn families_get_their_recommended_settings() {
        assert_eq!(p("qwen3.5-9b"), ModelProfile::default());
        assert_eq!(p("gemma-3-12b").plain.temperature, 1.0);
        assert_eq!(p("gemma-3-12b").plain.top_k, 64);
        assert_eq!(p("gemma-3-12b").thinking, Thinking::Never);
        assert!(p("mistral-small-3.2-24b").plain.temperature < 0.2);
        assert_eq!(p("gpt-oss-20b").thinking, Thinking::Effort);
        assert_eq!(p("deepseek-r1-distill-qwen-14b").thinking, Thinking::Always);
        assert_eq!(p("llama-3.1-8b").plain.top_p, 0.9);
        assert!(p("lfm2.5-1.2b").plain.min_p > 0.1);
    }

    #[test]
    fn thinking_control_matches_the_model() {
        let q = p("qwen3.5-9b");
        assert!(q.thinks(true) && !q.thinks(false));
        assert_eq!(q.template_kwargs(false, Mode::Auto), serde_json::json!({ "enable_thinking": false }));
        let r1 = p("deepseek-r1-distill-qwen-14b");
        assert!(r1.thinks(false));
        assert_eq!(r1.template_kwargs(true, Mode::Deep), serde_json::json!({}));
        let g = p("gemma-3-12b");
        assert!(!g.thinks(true));
        let oss = p("gpt-oss-20b");
        assert_eq!(oss.template_kwargs(true, Mode::Deep)["reasoning_effort"], "high");
        assert_eq!(oss.template_kwargs(false, Mode::Deep)["reasoning_effort"], "low");
        assert_eq!(oss.template_kwargs(true, Mode::Auto)["reasoning_effort"], "medium");
    }
}
