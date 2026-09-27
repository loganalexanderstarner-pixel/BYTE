//! Decides per message whether the model should think before answering and
//! how much it may write. Accuracy first: in Auto mode BYTE thinks unless the
//! message is clearly simple (small talk, a rewrite, a sum the calculator
//! answers), and scales the thinking budget with how hard the question looks.

use serde::Serialize;

use crate::settings::{Mode, ThinkingPref};

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPlan {
    pub thinking: bool,
    /// Max tokens the model may spend thinking (-1 = unlimited).
    pub thinking_budget: i32,
    pub max_tokens: u32,
    pub mode: Mode,
    /// The running model's recommended sampling and thinking control.
    #[serde(skip)]
    pub profile: crate::modelcfg::ModelProfile,
}

impl TurnPlan {
    /// Adapts the plan to the running model: models that always think do,
    /// models that can't think don't, and sampling follows the model card.
    pub fn for_model(mut self, profile: crate::modelcfg::ModelProfile) -> Self {
        self.profile = profile;
        let thinking = profile.thinks(self.thinking);
        if thinking != self.thinking {
            self.thinking = thinking;
            self.thinking_budget = if thinking { mode_limits(self.mode).0 } else { 0 };
        }
        self
    }
}

/// (thinking budget, max tokens) per mode.
fn mode_limits(mode: Mode) -> (i32, u32) {
    match mode {
        Mode::Fast => (512, 1536),
        Mode::Auto => (2048, 4096),
        Mode::Deep => (6144, 8192),
        Mode::Extended => (-1, 12288),
    }
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

const FRESHNESS_CUES: &[&str] = &[
    "latest", "newest", "current", "currently", "right now", "today", "tonight", "yesterday", "tomorrow",
    "this week", "this month", "this year", "last week", "recent", "recently", "news", "update", "price",
    "cost of", "stock", "weather", "forecast", "score", "who won", "release", "released", "version",
    "announced", "election", "schedule", "open now", "near me", "search the web", "look up", "google",
];

/// True when a question depends on up-to-date information, so BYTE should
/// search before answering instead of relying on training data.
pub fn needs_fresh_info(message: &str) -> bool {
    let m = format!(" {} ", message.to_lowercase());
    if FRESHNESS_CUES.iter().any(|c| m.contains(c)) {
        return true;
    }
    // Mentions of recent years (training data may predate them).
    let year = chrono::Datelike::year(&chrono::Local::now());
    (year - 1..=year + 1).any(|y| m.contains(&y.to_string()))
}

/// How much deliberate reasoning a message needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effort {
    /// Greetings, thanks, rewrites, formatting, a sum the calculator answers.
    Trivial,
    /// A short, plain question: a little thinking catches slips.
    Light,
    /// Anything else.
    Normal,
    /// Reasoning cues, long messages, maths.
    Hard,
}

/// Whole messages (after greetings and "BYTE" are removed) that are small talk.
const SMALL_TALK: &[&str] = &[
    "", "how are you", "how are you doing", "what's up", "whats up", "sup", "who are you", "what's your name",
    "good morning", "good night", "good evening", "see you", "see you later", "got it", "thank you", "bye",
];
const GREETING_WORDS: &[&str] = &["hi", "hey", "hello", "yo", "byte"];
const ACK_WORDS: &[&str] = &["ok", "okay", "cool", "nice", "great", "thanks", "thx", "ty", "yes", "no", "sure", "lol", "haha", "perfect", "awesome"];

const TEXT_JOBS: &[&str] = &[
    "rewrite", "rephrase", "reword", "paraphrase", "proofread", "fix the spelling", "fix the grammar", "fix grammar",
    "fix spelling", "fix typos", "translate", "format this", "format as", "make it shorter", "make it longer",
    "shorten", "summarize this", "summarise this", "tl;dr", "turn this into", "convert this to", "capitalize",
];

/// Classifies a message (cheap heuristics; no model call).
pub fn effort(message: &str) -> Effort {
    let m = message.trim().to_lowercase();
    let clean: String = m.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '\'').collect();
    let words = clean.split_whitespace().count();
    let rest: Vec<&str> = clean.split_whitespace().filter(|w| !GREETING_WORDS.contains(w)).collect();
    if words <= 6 && (SMALL_TALK.contains(&rest.join(" ").as_str()) || rest.iter().all(|w| ACK_WORDS.contains(w))) {
        return Effort::Trivial;
    }
    if TEXT_JOBS.iter().any(|j| m.starts_with(j) || m.contains(&format!("\n{j}")) || (words <= 12 && m.contains(j))) {
        return Effort::Trivial;
    }
    // The calculator answers plain sums exactly; thinking adds nothing.
    if words <= 8 && math_expression(message).is_some() {
        return Effort::Trivial;
    }
    if looks_complex(message) {
        return Effort::Hard;
    }
    if words <= 12 {
        Effort::Light
    } else {
        Effort::Normal
    }
}

pub fn plan_turn(mode: Mode, pref: ThinkingPref, message: &str) -> TurnPlan {
    let (budget, max_tokens) = mode_limits(mode);
    let (thinking, budget) = match pref {
        ThinkingPref::On => (true, budget),
        ThinkingPref::Off => (false, 0),
        ThinkingPref::Auto => match (mode, effort(message)) {
            (Mode::Fast, _) => (false, 0),
            (Mode::Auto, Effort::Trivial) => (false, 0),
            (Mode::Auto, Effort::Light) => (true, 384),
            (Mode::Auto, Effort::Normal) => (true, 1024),
            (Mode::Auto, Effort::Hard) => (true, budget),
            (Mode::Deep | Mode::Extended, _) => (true, budget),
        },
    };
    TurnPlan { thinking, thinking_budget: if thinking { budget } else { 0 }, max_tokens, mode, profile: Default::default() }
}

/// An arithmetic question in the user's message ("what's 1234 * 5678?",
/// "15% of 80"), rewritten for the calculator. Small models often skip the
/// calculator and get the numbers wrong, so BYTE runs it itself.
///
/// Conservative on purpose: it needs two numbers and a real operator. Dates
/// (2026-09-27, 9/27), ranges (3-5), versions (1.2.3), phone numbers and
/// sizes like 1920x1080 don't count; `-` and `/` only count with spaces
/// around them unless another operator is present.
pub fn math_expression(message: &str) -> Option<String> {
    if message.contains("```") || message.len() > 2000 {
        return None;
    }
    if let Some(p) = percent_of(message) {
        return Some(p);
    }
    let chars: Vec<char> = message.chars().collect();
    let mut best: Option<String> = None;
    let mut i = 0;
    while i < chars.len() {
        let starts = chars[i].is_ascii_digit() || (chars[i] == '(' && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()));
        let after_word = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '.');
        if starts && !after_word {
            let end = scan_expression(&chars, i);
            if let Some(e) = normalise(&chars[i..end]) {
                if best.as_ref().is_none_or(|b| e.len() > b.len()) {
                    best = Some(e);
                }
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    best
}

/// End (exclusive) of the run of number/operator characters starting at `start`.
fn scan_expression(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        let prev = if i > 0 { chars[i - 1] } else { ' ' };
        let next = chars.get(i + 1).copied().unwrap_or(' ');
        let ok = c.is_ascii_digit()
            || c == ' '
            || "+-*/^×÷()%".contains(c)
            || (c == '.' && prev.is_ascii_digit() && next.is_ascii_digit())
            // Thousands separator: 1,234
            || (c == ',' && prev.is_ascii_digit() && chars.get(i + 1..i + 4).is_some_and(|d| d.iter().all(|c| c.is_ascii_digit()))
                && !chars.get(i + 4).is_some_and(|c| c.is_ascii_digit()))
            // "4 x 5" (spaced x only, so 1920x1080 isn't read as a sum)
            || ((c == 'x' || c == 'X') && prev == ' ' && next == ' ');
        if !ok {
            break;
        }
        i += 1;
    }
    i
}

fn normalise(span: &[char]) -> Option<String> {
    let raw: String = span.iter().collect();
    let raw = raw.trim_end_matches(|c: char| c == ' ' || "+-*/^×÷(x".contains(c)).trim();
    let mut numbers = 0;
    let mut strong = 0;
    let mut weak = 0;
    let mut depth = 0i32;
    let chars: Vec<char> = raw.chars().collect();
    let mut in_number = false;
    for (i, &c) in chars.iter().enumerate() {
        let is_num = c.is_ascii_digit() || ((c == '.' || c == ',') && in_number);
        if is_num && !in_number {
            numbers += 1;
        }
        in_number = is_num;
        let spaced = i > 0 && chars[i - 1] == ' ' && chars.get(i + 1) == Some(&' ');
        match c {
            '+' | '*' | '^' | '×' | '÷' | 'x' | 'X' => strong += 1,
            '-' | '/' if spaced => weak += 1,
            '-' | '/' => {}
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    // Versions like 1.2.3 aren't numbers.
    if raw.split(|c: char| !(c.is_ascii_digit() || c == '.')).any(|w| w.matches('.').count() > 1) {
        return None;
    }
    let unspaced_weak = raw.contains(|c| c == '-' || c == '/') && weak == 0;
    if numbers < 2 || depth != 0 || (strong == 0 && weak == 0) || (strong == 0 && unspaced_weak) {
        return None;
    }
    // Dates like 2026-09-27 with other operators around still aren't maths.
    if strong == 0 && weak == 0 {
        return None;
    }
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '×' | 'x' | 'X' => out.push('*'),
            '÷' => out.push('/'),
            ',' if i > 0 && chars[i - 1].is_ascii_digit() => {}
            _ => out.push(c),
        }
    }
    Some(out.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// "15% of 80", "15 percent of 80".
fn percent_of(message: &str) -> Option<String> {
    let words: Vec<String> = message
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| "?!,;:\"'()".contains(c)).trim_end_matches('.').to_lowercase())
        .collect();
    let num = |w: &str| {
        let w = w.replace(',', "");
        w.parse::<f64>().ok().map(|_| w)
    };
    for i in 0..words.len() {
        let (pct, next) = if let Some(n) = words[i].strip_suffix('%').and_then(num) {
            (n, i + 1)
        } else if words.get(i + 1).is_some_and(|w| w == "percent") {
            match num(&words[i]) {
                Some(n) => (n, i + 2),
                None => continue,
            }
        } else {
            continue;
        };
        if words.get(next).is_some_and(|w| w == "of") {
            if let Some(base) = words.get(next + 1).and_then(|w| num(w.trim_start_matches('$'))) {
                return Some(format!("{pct}% of {base}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_time_sensitive_questions() {
        assert!(needs_fresh_info("What is the latest stable version of Rust?"));
        assert!(needs_fresh_info("Biggest tech news this week"));
        let year = chrono::Datelike::year(&chrono::Local::now());
        assert!(needs_fresh_info(&format!("Best laptops of {year}")));
        assert!(!needs_fresh_info("Explain how photosynthesis works"));
        assert!(!needs_fresh_info("Write a poem about the sea"));
    }

    #[test]
    fn accuracy_first_effort_levels() {
        let e = |m: &str| effort(m);
        for m in ["hi", "Thanks!", "hey byte, what's up", "ok cool", "what's 1234 * 5678?", "Rewrite this so it sounds friendlier: see you at 5"] {
            assert_eq!(e(m), Effort::Trivial, "{m}");
        }
        assert_eq!(e("Translate to French: the meeting moved to Tuesday afternoon because of the storm"), Effort::Trivial);
        for m in ["What is the capital of Australia?", "Who wrote Dune?", "hi, what causes tides on earth"] {
            assert_eq!(e(m), Effort::Light, "{m}");
        }
        assert_eq!(e("Tell me about the history of the printing press in Europe and its effect on literacy"), Effort::Normal);
        assert_eq!(e("Should I use Postgres or SQLite for a small desktop app?"), Effort::Hard);
        // Short plain questions think a little; hard ones get the full budget.
        let light = plan_turn(Mode::Auto, ThinkingPref::Auto, "Who wrote Dune?");
        assert!(light.thinking && light.thinking_budget == 384);
        assert_eq!(plan_turn(Mode::Auto, ThinkingPref::Auto, "Explain why the sky is blue").thinking_budget, 2048);
        assert!(!plan_turn(Mode::Fast, ThinkingPref::Auto, "Explain why the sky is blue").thinking);
    }

    #[test]
    fn small_talk_skips_thinking_in_auto() {
        assert!(!plan_turn(Mode::Auto, ThinkingPref::Auto, "hey byte, what's up").thinking);
    }

    #[test]
    fn reasoning_questions_think_in_auto() {
        assert!(plan_turn(Mode::Auto, ThinkingPref::Auto, "Explain why the sky is blue").thinking);
        assert!(plan_turn(Mode::Auto, ThinkingPref::Auto, "If a train leaves at 3pm going 80 km/h, when does it cover 200 km?").thinking);
        // A plain sum goes to the calculator, which is exact; no thinking needed.
        assert!(!plan_turn(Mode::Auto, ThinkingPref::Auto, "what is 17*23 + 4^2 = ?").thinking);
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

    #[test]
    fn finds_arithmetic_to_calculate() {
        let m = |s: &str| math_expression(s);
        assert_eq!(m("Use the calculator tool to compute 1234 * 5678, then tell me the result.").as_deref(), Some("1234 * 5678"));
        assert_eq!(m("what's 12 x 30?").as_deref(), Some("12 * 30"));
        assert_eq!(m("how much is 1,250 × 4 + 17").as_deref(), Some("1250 * 4 + 17"));
        assert_eq!(m("(3 + 4) ^ 2 please").as_deref(), Some("(3 + 4) ^ 2"));
        assert_eq!(m("100 - 37").as_deref(), Some("100 - 37"));
        assert_eq!(m("144 / 12 = ?").as_deref(), Some("144 / 12"));
        assert_eq!(m("What is 15% of 80?").as_deref(), Some("15% of 80"));
        assert_eq!(m("tip: 18 percent of $64.50").as_deref(), Some("18% of 64.50"));
        assert_eq!(m("2.5*4").as_deref(), Some("2.5*4"));
    }

    #[test]
    fn ignores_things_that_are_not_sums() {
        for s in [
            "Meeting on 2026-09-27 at 3pm",
            "sleep 9/27 please",
            "it takes 3-5 days",
            "update to 1.2.3 then 1.2.4",
            "call 555-1234",
            "my screen is 1920x1080",
            "What's new in Qwen3.5?",
            "I have 3 apples",
            "C++ or Rust?",
            "```\nlet x = 1 + 2;\n```",
            "Explain GPT-4 vs Llama-3",
        ] {
            assert_eq!(math_expression(s), None, "{s}");
        }
    }
}
