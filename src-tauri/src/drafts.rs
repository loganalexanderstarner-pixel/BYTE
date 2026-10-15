//! Best of 3 for hard questions (maths, logic, puzzles, "what does this code
//! print"): BYTE writes three drafts, compares their final answers, and gives
//! the one most drafts agree on. When all three disagree, one more pass sees
//! all three and works out which is right. Small models get these wrong often
//! enough that majority voting clearly helps (self-consistency).

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::chat::{self, ChatEvent};
use crate::error::AppResult;
use crate::settings::Mode;

pub const DRAFTS: usize = 3;

/// Questions with one right answer that take reasoning to reach.
pub fn is_reasoning(question: &str) -> bool {
    let q = question.to_lowercase();
    const ALWAYS: &[&str] = &["puzzle", "riddle", "brain teaser", "prove that", "logic problem", "what does this code print", "what is the output", "output of this code", "what will this print"];
    if ALWAYS.iter().any(|w| q.contains(w)) {
        return true;
    }
    // Word problems: numbers plus a question about a quantity.
    // At least two quantities ("$1.10 … $1.00", "3 cats … 3 mice"); one ("Switch 2") is just a name.
    let digit_groups = q.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|w| w.chars().any(|c| c.is_ascii_digit())).count();
    let number_words = q.split(|c: char| !c.is_alphabetic()).filter(|w| ["one", "two", "three", "four", "five", "six", "ten", "half", "twice", "double", "triple"].contains(w)).count();
    let has_number = digit_groups + number_words >= 2;
    const ASKS: &[&str] = &["how many", "how much", "how long", "how far", "how old", "what is the probability", "what's the probability", "probability that", "solve", "calculate", "work out", "what time will", "what percentage", "find the value", "find x", "remainder"];
    has_number && ASKS.iter().any(|w| q.contains(w))
}

/// Best of 3 runs for hard questions in Deep and Extended, when nothing was looked up.
pub fn applies(enabled: bool, mode: Mode, used_tools: bool, question: &str) -> bool {
    enabled && !used_tools && matches!(mode, Mode::Deep | Mode::Extended) && is_reasoning(question) && crate::router::math_expression(question).is_none()
}

/// The final answer a draft gives: a \boxed{} value, an "answer is/Answer:"
/// line, else the last number, else the last line; normalized for comparing.
pub fn final_answer(draft: &str) -> String {
    let t = draft.trim();
    if let Some(i) = t.rfind("\\boxed{") {
        let rest = &t[i + 7..];
        if let Some(j) = rest.find('}') {
            return normalize(&rest[..j]);
        }
    }
    // ASCII lowering keeps byte positions the same as in `t`.
    let lower = t.to_ascii_lowercase();
    for marker in ["final answer:", "final answer is", "the answer is", "answer:", "answer is"] {
        if let Some(i) = lower.rfind(marker) {
            let rest = t[i + marker.len()..].trim_start_matches([':', ' ', '*']);
            let line = rest.lines().next().unwrap_or("").trim();
            if !line.is_empty() {
                return normalize(first_value(line).unwrap_or(line));
            }
        }
    }
    let tail: String = t.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
    if let Some(n) = last_number(&tail) {
        return normalize(&n);
    }
    normalize(t.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(""))
}

/// "42 apples." → "42"; a plain word answer stays whole.
fn first_value(s: &str) -> Option<&str> {
    let start = s.find(|c: char| c.is_ascii_digit() || c == '-')?;
    let end = s[start..].find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '/' | '-'))).map(|e| start + e).unwrap_or(s.len());
    Some(s[start..end].trim_end_matches(['.', ',']))
}

fn last_number(s: &str) -> Option<String> {
    let mut found = None;
    let mut cur = String::new();
    for ch in s.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() || (!cur.is_empty() && matches!(ch, '.' | ',' | '/')) {
            cur.push(ch);
        } else {
            if cur.chars().any(|c| c.is_ascii_digit()) {
                found = Some(cur.trim_end_matches(['.', ',', '/']).to_string());
            }
            cur.clear();
        }
    }
    found
}

/// Case, spaces, commas, "$", "**" and trailing ".0" don't matter.
pub fn normalize(s: &str) -> String {
    let t: String = s.to_lowercase().chars().filter(|c| !matches!(c, ' ' | ',' | '$' | '*' | '`')).collect();
    let t = t.trim_end_matches('.').to_string();
    match t.parse::<f64>() {
        Ok(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{}", x as i64),
        Ok(x) => format!("{x}"),
        Err(_) => t,
    }
}

/// The draft to give: (index, how many agree), or None when they all differ.
pub fn pick(drafts: &[String]) -> Option<(usize, usize)> {
    let answers: Vec<String> = drafts.iter().map(|d| final_answer(d)).collect();
    let mut best: Option<(usize, usize)> = None;
    for (i, a) in answers.iter().enumerate() {
        if a.is_empty() {
            continue;
        }
        let agree = answers.iter().filter(|b| *b == a).count();
        // The shortest of the agreeing drafts (clearest), for the largest group.
        let better = match best {
            None => agree >= 2,
            Some((j, k)) => agree > k || (agree == k && answers[j] == *a && drafts[i].len() < drafts[j].len()),
        };
        if better && agree >= 2 {
            best = Some((i, agree));
        }
    }
    best
}

/// The message for the extra pass when all drafts differ.
pub fn reconcile_prompt(drafts: &[String]) -> String {
    let mut s = String::from("Three attempts at my question reached different answers:\n\n");
    for (i, d) in drafts.iter().enumerate() {
        s.push_str(&format!("Attempt {}:\n{}\n\n", i + 1, d.chars().take(4000).collect::<String>()));
    }
    s.push_str("Check each attempt carefully, find the mistakes, and give the correct answer with a clear explanation. End with \"Final answer: …\".");
    s
}

/// Writes the drafts (one at a time: there's one engine slot). Returns the
/// drafts' texts and the stats totals of the rounds.
pub async fn write(http: &reqwest::Client, ep: &crate::engine::Endpoint, messages: &[Value], plan: crate::router::TurnPlan, cancel: &CancellationToken) -> AppResult<Vec<String>> {
    let mut drafts = Vec::new();
    for k in 0..DRAFTS {
        let mut body = chat::build_body(messages.to_vec(), plan, None);
        body["seed"] = json!(1000 + k as u64);
        body["temperature"] = json!(0.7);
        let mut ignore = |_e: ChatEvent| -> AppResult<()> { Ok(()) };
        let r = chat::stream_round(http, ep, &body, cancel, &mut ignore).await?;
        if r.finish == "cancelled" {
            return Err(crate::error::AppError::Cancelled);
        }
        drafts.push(r.content);
    }
    Ok(drafts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_questions_are_recognized() {
        for yes in ["A bat and a ball cost $1.10 in total. The bat costs $1 more than the ball. How much does the ball cost?", "If 3 cats catch 3 mice in 3 minutes, how many cats catch 100 mice in 100 minutes?", "Here's a riddle: what has keys but can't open locks?", "What is the probability that two dice sum to 7?"] {
            assert!(is_reasoning(yes), "{yes}");
        }
        for no in ["How many people live in Tokyo?", "What's the weather tomorrow?", "Write a poem about rain", "how long is the Great Wall"] {
            assert!(!is_reasoning(no), "{no}");
        }
    }

    #[test]
    fn final_answers_are_extracted_and_normalized() {
        assert_eq!(final_answer("Let x be…\nSo the ball costs $0.05.\n\n**Final answer: $0.05**"), "0.05");
        assert_eq!(final_answer("… therefore \\boxed{42}."), "42");
        assert_eq!(final_answer("We need 3 cats. The answer is 3 cats."), "3");
        assert_eq!(final_answer("It takes 1,200 steps in total"), "1200");
        assert_eq!(final_answer("The answer is: a piano"), "apiano");
        assert_eq!(normalize("7.0"), "7");
        assert_eq!(normalize("1/6"), "1/6");
    }

    #[test]
    fn the_majority_wins() {
        let d = |s: &str| s.to_string();
        let drafts = vec![d("long working… Final answer: 5 cents"), d("Final answer: 10 cents"), d("Final answer: 5")];
        assert_eq!(pick(&drafts), Some((2, 2)));
        let all = vec![d("Final answer: 1"), d("Final answer: 1"), d("x Final answer: 1")];
        assert_eq!(pick(&all), Some((0, 3)));
        assert_eq!(pick(&[d("Final answer: 1"), d("Final answer: 2"), d("Final answer: 3")]), None);
        assert!(reconcile_prompt(&drafts).contains("Attempt 3:"));
    }
}
