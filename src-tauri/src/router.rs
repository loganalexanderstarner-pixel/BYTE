//! Decides per message whether the model should think before answering and
//! how much it may write. Phase 8 swaps the heuristic for a small classifier.

use serde::Serialize;

use crate::settings::{Mode, ThinkingPref};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPlan {
    pub thinking: bool,
    /// Max tokens the model may spend thinking (-1 = unlimited).
    pub thinking_budget: i32,
    pub max_tokens: u32,
}

const REASONING_CUES: &[&str] = &[
    "why", "how does", "how do", "how would", "explain", "prove", "derive", "compare", "versus", " vs ",
    "calculate", "solve", "estimate", "plan", "step by step", "analy", "evaluate", "trade-off", "tradeoff",
    "pros and cons", "should i", "which is better", "design", "strategy", "debug", "optimi", "reason",
    "what if", "difference between", "implications",
];

/// Cheap signal that a message benefits from deliberate reasoning.
pub fn looks_complex(message: &str) -> bool {
    let m = message.to_lowercase();
    if m.chars().count() > 280 {
        return true;
    }
    if REASONING_CUES.iter().any(|c| m.contains(c)) {
        return true;
    }
    let math = m.chars().filter(|c| matches!(c, '=' | '+' | '*' | '^' | '/' | '%')).count();
    let digits = m.chars().filter(|c| c.is_ascii_digit()).count();
    math >= 2 && digits >= 2
}

pub fn plan_turn(mode: Mode, pref: ThinkingPref, message: &str) -> TurnPlan {
    let thinking = match pref {
        ThinkingPref::On => true,
        ThinkingPref::Off => false,
        ThinkingPref::Auto => match mode {
            Mode::Fast => false,
            Mode::Auto => looks_complex(message),
            Mode::Deep | Mode::Extended => true,
        },
    };
    let (budget, max_tokens) = match mode {
        Mode::Fast => (512, 1536),
        Mode::Auto => (2048, 4096),
        Mode::Deep => (6144, 8192),
        Mode::Extended => (-1, 12288),
    };
    TurnPlan { thinking, thinking_budget: if thinking { budget } else { 0 }, max_tokens }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_talk_skips_thinking_in_auto() {
        assert!(!plan_turn(Mode::Auto, ThinkingPref::Auto, "hey byte, what's up").thinking);
    }

    #[test]
    fn reasoning_questions_think_in_auto() {
        assert!(plan_turn(Mode::Auto, ThinkingPref::Auto, "Explain why the sky is blue").thinking);
        assert!(plan_turn(Mode::Auto, ThinkingPref::Auto, "what is 17*23 + 4^2 = ?").thinking);
    }

    #[test]
    fn explicit_preference_wins() {
        assert!(plan_turn(Mode::Fast, ThinkingPref::On, "hi").thinking);
        assert!(!plan_turn(Mode::Deep, ThinkingPref::Off, "prove it").thinking);
    }

    #[test]
    fn deep_modes_think_and_write_more() {
        let d = plan_turn(Mode::Deep, ThinkingPref::Auto, "hi");
        let f = plan_turn(Mode::Fast, ThinkingPref::Auto, "hi");
        assert!(d.thinking && d.max_tokens > f.max_tokens);
        assert_eq!(plan_turn(Mode::Extended, ThinkingPref::Auto, "x").thinking_budget, -1);
        assert_eq!(f.thinking_budget, 0);
    }
}
