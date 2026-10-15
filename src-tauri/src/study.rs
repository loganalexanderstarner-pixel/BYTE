//! Study tools: flashcards with spaced repetition (SM-2), quizzes, and tutor
//! mode. "Make flashcards about the French Revolution", "quiz me on chapter 3"
//! (with a file attached), "flashcards from this" (the answer above). Cards are
//! saved into decks (DB tables `decks`, `cards`) and studied in the Study panel;
//! decks export to Anki.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::{self, ChatEvent};
use crate::error::{AppError, AppResult};
use crate::research::{self, Ctx, Emit, Gathered};
use crate::tools::SourceBook;

// ---------- spaced repetition (SM-2) ----------

/// A card's schedule.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    /// How easy the card is (starts at 2.5, never below 1.3).
    pub ease: f64,
    /// Days until the next review.
    pub interval: u32,
    /// Correct reviews in a row.
    pub reps: u32,
    pub lapses: u32,
    /// The day it's due (days since 1 Jan year 1, local time).
    pub due: i64,
}

impl Schedule {
    #[cfg(test)]
    pub fn new(today: i64) -> Schedule {
        Schedule { ease: 2.5, interval: 0, reps: 0, lapses: 0, due: today }
    }
}

/// SM-2: grade 0–5 (the Study panel sends Again 1, Hard 3, Good 4, Easy 5).
pub fn review(s: Schedule, grade: u8, today: i64) -> Schedule {
    let q = grade.min(5) as f64;
    let mut n = s;
    if grade < 3 {
        n.reps = 0;
        n.interval = 1;
        n.lapses += 1;
    } else {
        n.reps += 1;
        n.interval = match n.reps {
            1 => 1,
            2 => 6,
            _ => ((s.interval.max(1) as f64) * s.ease).round() as u32,
        };
        // Easy gets a little extra; hard a little less (common SM-2 variant).
        if grade == 5 && n.reps > 1 {
            n.interval = ((n.interval as f64) * 1.3).round() as u32;
        } else if grade == 3 && n.reps > 2 {
            n.interval = ((s.interval.max(1) as f64) * 1.2).round().max(1.0) as u32;
        }
    }
    n.ease = (s.ease + 0.1 - (5.0 - q) * (0.08 + (5.0 - q) * 0.02)).max(1.3);
    n.interval = n.interval.clamp(1, 3650);
    n.due = today + n.interval as i64;
    n
}

/// Today as a day number (local calendar).
pub fn today() -> i64 {
    use chrono::Datelike;
    chrono::Local::now().date_naive().num_days_from_ce() as i64
}

// ---------- cards, quizzes ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub front: String,
    pub back: String,
}

/// Flashcards made in chat (not saved yet).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flashcards {
    pub title: String,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuizQuestion {
    pub question: String,
    pub choices: Vec<String>,
    /// Index of the right choice.
    pub answer: usize,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quiz {
    pub title: String,
    pub questions: Vec<QuizQuestion>,
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum StudyAsk {
    Cards { topic: String, count: usize },
    Quiz { topic: String, count: usize },
}

const CARD_CUES: &[&str] = &["flashcards", "flash cards", "flashcard", "anki cards", "study cards", "make cards", "memory cards"];
const QUIZ_CUES: &[&str] = &["quiz me", "make a quiz", "make me a quiz", "give me a quiz", "create a quiz", "test me on", "test my knowledge", "practice test", "practice exam", "practice questions", "exam questions", "multiple choice questions", "multiple-choice questions", "a quiz on", "a quiz about"];

fn count_in(q: &str) -> Option<usize> {
    q.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<usize>().ok()).find(|n| (3..=50).contains(n))
}

/// The topic after "on / about / for / from / of", or what's left of the message.
fn topic_of(question: &str, cues: &[&str]) -> String {
    let lower = question.to_lowercase();
    for marker in [" on ", " about ", " for ", " from ", " of ", " covering "] {
        if let Some(i) = lower.find(marker) {
            // Only markers after the cue ("quiz me on X", "flashcards about X").
            if cues.iter().any(|c| lower[..i + 1].contains(c)) || lower[..i].contains("quiz") || lower[..i].contains("card") || lower[..i].split_whitespace().count() <= 4 {
                let t = question[i + marker.len()..].trim().trim_end_matches(['?', '.', '!']).trim();
                let t = t.trim_start_matches("the topic of ").trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
    }
    crate::reviews::subject(question, cues)
}

pub fn study_ask(question: &str) -> Option<StudyAsk> {
    let q = question.to_lowercase();
    // "What are flashcards?", "how does a quiz app work": questions about, not requests for.
    if q.starts_with("what is ") || q.starts_with("what are ") || q.starts_with("how does ") || q.contains("app") && q.contains("build") {
        return None;
    }
    let quiz_verb = q.contains("quiz") && ["give me", "make", "create", "write", "generate", "build me", "can i get"].iter().any(|v| q.contains(v));
    if quiz_verb || QUIZ_CUES.iter().any(|c| q.contains(c)) {
        return Some(StudyAsk::Quiz { topic: topic_of(question, QUIZ_CUES), count: count_in(&q).unwrap_or(8).clamp(3, 20) });
    }
    if CARD_CUES.iter().any(|c| q.contains(c)) {
        return Some(StudyAsk::Cards { topic: topic_of(question, CARD_CUES), count: count_in(&q).unwrap_or(12).clamp(3, 40) });
    }
    None
}

pub fn applies(enabled: bool, question: &str) -> bool {
    enabled && study_ask(question).is_some()
}

/// "flashcards from this", "quiz me on the above": the material is the answer before.
fn about_the_chat(topic: &str) -> bool {
    let t = topic.to_lowercase();
    t.is_empty() || ["this", "that", "the above", "above", "what you just said", "your answer", "our conversation", "this chat", "it"].iter().any(|w| t == *w || t.starts_with(&format!("{w} ")))
}

fn clean(v: &Value, max: usize) -> String {
    let s = v.as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(max).collect()
}

fn cards_schema(n: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "cards": { "type": "array", "minItems": 3, "maxItems": n, "items": { "type": "object", "properties": { "front": { "type": "string" }, "back": { "type": "string" } }, "required": ["front", "back"] } }
        },
        "required": ["title", "cards"]
    })
}

fn quiz_schema(n: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "questions": { "type": "array", "minItems": 2, "maxItems": n, "items": { "type": "object", "properties": {
                "question": { "type": "string" },
                "choices": { "type": "array", "minItems": 2, "maxItems": 5, "items": { "type": "string" } },
                "answer": { "type": "string" },
                "explanation": { "type": "string" }
            }, "required": ["question", "choices", "answer"] } }
        },
        "required": ["title", "questions"]
    })
}

/// The reply as JSON; when a small model ran out of tokens mid-list, the complete
/// objects of `list` that it did finish (so 5 good cards aren't lost to a cut-off 6th).
fn reply_json(reply: &str, list: &str) -> Value {
    let v = research::lenient_json(reply);
    if !v.is_null() {
        return v;
    }
    let Some(start) = reply.find(&format!("\"{list}\"")).and_then(|i| reply[i..].find('[').map(|j| i + j + 1)) else { return Value::Null };
    let (mut items, mut depth, mut from, mut in_str, mut esc) = (Vec::new(), 0usize, 0usize, false, false);
    for (i, ch) in reply[start..].char_indices() {
        if in_str {
            match ch {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => {
                if depth == 0 {
                    from = start + i;
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if let Ok(o) = serde_json::from_str::<Value>(&reply[from..=start + i]) {
                        items.push(o);
                    }
                }
            }
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    let title = reply.find("\"title\"").and_then(|i| {
        let rest = &reply[i + 7..];
        let a = rest.find('"')? + 1;
        let b = rest[a..].find('"')?;
        Some(rest[a..a + b].to_string())
    });
    json!({ "title": title.unwrap_or_default(), list: items })
}

pub fn parse_cards(reply: &str, max: usize) -> Option<Flashcards> {
    let v = reply_json(reply, "cards");
    let mut seen = std::collections::HashSet::new();
    let cards: Vec<Card> = v["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let front = clean(&c["front"], 300);
            let back = clean(&c["back"], 600);
            (!front.is_empty() && !back.is_empty() && front != back && seen.insert(front.to_lowercase())).then_some(Card { front, back })
        })
        .take(max)
        .collect();
    (cards.len() >= 2).then(|| Flashcards { title: clean(&v["title"], 80).trim().to_string(), cards })
}

pub fn parse_quiz(reply: &str, max: usize) -> Option<Quiz> {
    let v = reply_json(reply, "questions");
    let mut seen = std::collections::HashSet::new();
    let questions: Vec<QuizQuestion> = v["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| {
            let question = clean(&q["question"], 400);
            let mut choices: Vec<String> = Vec::new();
            for c in q["choices"].as_array().into_iter().flatten() {
                let c = clean(c, 200);
                if !c.is_empty() && !choices.iter().any(|x| x.eq_ignore_ascii_case(&c)) {
                    choices.push(c);
                }
            }
            choices.truncate(5);
            // The answer as an index (0- or 1-based), or as the choice's text.
            let answer = match &q["answer"] {
                Value::Number(n) => n.as_u64().map(|n| n as usize),
                Value::String(s) => {
                    let norm = |x: &str| x.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
                    let a = norm(s);
                    choices
                        .iter()
                        .position(|c| norm(c) == a)
                        // "A", "B)", "(c)": a letter.
                        .or_else(|| {
                            let letter = s.trim().trim_matches(['(', ')', '.']).chars().next()?.to_ascii_uppercase();
                            ('A'..='E').position(|l| l == letter).filter(|_| s.trim().trim_matches(['(', ')', '.']).len() == 1)
                        })
                        // "The answer is Jupiter" / "Jupiter, the gas giant": one choice inside the other.
                        .or_else(|| {
                            let hits: Vec<usize> = choices.iter().enumerate().filter(|(_, c)| !a.is_empty() && (a.contains(&norm(c)) || norm(c).contains(&a))).map(|(i, _)| i).collect();
                            (hits.len() == 1).then(|| hits[0])
                        })
                        .or_else(|| s.trim().parse::<usize>().ok())
                }
                _ => None,
            }?;
            let answer = if answer >= choices.len() && answer == choices.len() { answer - 1 } else { answer };
            (!question.is_empty() && choices.len() >= 2 && answer < choices.len() && seen.insert(question.to_lowercase())).then(|| QuizQuestion { question, choices, answer, explanation: clean(&q["explanation"], 500) })
        })
        .take(max)
        .collect();
    (questions.len() >= 2).then(|| Quiz { title: clean(&v["title"], 80), questions })
}

pub const CARDS_RULES: &str = "The user sees the flashcards above as a card set they can flip, save as a deck and study with \
spaced repetition. In one or two sentences, say what the set covers and suggest studying a few minutes a day. Don't list \
the cards again or list them.";

pub const QUIZ_RULES: &str = "The user sees the quiz above and answers it there; it scores itself and explains each answer. \
In one sentence, wish them luck and say they can save the ones they miss as flashcards. Don't repeat or list the questions, and don't give away any answers.";

/// BYTE's own short reply under a flashcard set or quiz (the model would list
/// the cards or give quiz answers away).
pub fn reply_for(notes: &str) -> Option<String> {
    let first = notes.lines().next()?;
    if let Some(rest) = first.strip_prefix("Flashcards shown: ") {
        return Some(format!(
            "Here are your flashcards: {}. Tap a card to flip it, or press **Study now**: BYTE brings each card back right before you'd forget it, so a few minutes a day is enough.",
            rest.trim_end_matches('.')
        ));
    }
    if first.starts_with("Quiz shown: ") {
        return Some("Here's your quiz. Answer every question, then press **Check my answers** to see your score and why each answer is right. You can save any you miss as flashcards. Good luck!".into());
    }
    None
}

/// "Just tell me the answer", "show me the full solution": the learner wants it now.
pub fn wants_solution(question: &str) -> bool {
    let q = question.to_lowercase();
    ["just tell me", "tell me the answer", "give me the answer", "show me the answer", "full solution", "show the solution", "show me the solution", "whole solution", "i give up", "skip the hints"]
        .iter()
        .any(|c| q.contains(c))
}

fn tutor_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "feedback": { "type": "string" },
            "step": { "type": "string" },
            "question": { "type": "string" }
        },
        "required": ["step", "question"]
    })
}

/// A tutor turn as text: feedback on the learner's last answer, the next small step, and a question.
/// Solves a one-variable linear equation in the text ("solve 2x + 6 = 14" →
/// ('x', 4.0)), so the tutor can check its hints don't give the answer away.
pub fn solve_linear(text: &str) -> Option<(char, f64)> {
    let (left, right) = text.split_once('=')?;
    if right.contains('=') {
        return None;
    }
    // A term: "+", "-", "6", "2x", "-3.5y", "x".
    fn is_term(t: &str) -> bool {
        let t = t.trim_matches(['?', '.', ',']);
        if t == "+" || t == "-" {
            return true;
        }
        let body = t.trim_start_matches(['+', '-']);
        let digits = body.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let var = &body[digits.len()..];
        !body.is_empty() && (digits.is_empty() || digits.parse::<f64>().is_ok()) && var.len() <= 1 && (!digits.is_empty() || var.len() == 1)
    }
    let lt: Vec<&str> = left.split_whitespace().rev().take_while(|t| is_term(t)).collect::<Vec<_>>().into_iter().rev().collect();
    let rt: Vec<&str> = right.split_whitespace().take_while(|t| is_term(t)).collect();
    // Sum of (coefficient of the variable, constant) on one side.
    let mut var: Option<char> = None;
    let mut side = |toks: &[&str]| -> Option<(f64, f64)> {
        let (mut a, mut b, mut sign) = (0.0, 0.0, 1.0);
        let mut any = false;
        for t in toks {
            let t = t.trim_matches(['?', '.', ',']);
            match t {
                "+" => sign = 1.0,
                "-" => sign = -1.0,
                _ => {
                    let neg = t.starts_with('-');
                    let body = t.trim_start_matches(['+', '-']);
                    let s = if neg { -sign } else { sign };
                    let digits = body.trim_end_matches(|c: char| c.is_ascii_alphabetic());
                    let v = &body[digits.len()..];
                    if let Some(ch) = v.chars().next() {
                        if var.is_some_and(|x| x != ch) {
                            return None;
                        }
                        var = Some(ch);
                        a += s * if digits.is_empty() { 1.0 } else { digits.parse::<f64>().ok()? };
                    } else {
                        b += s * digits.parse::<f64>().ok()?;
                    }
                    sign = 1.0;
                    any = true;
                }
            }
        }
        any.then_some((a, b))
    };
    let (a1, b1) = side(&lt)?;
    let (a2, b2) = side(&rt)?;
    let v = var?;
    if (a1 - a2).abs() < 1e-9 {
        return None;
    }
    Some((v, (b2 - b1) / (a1 - a2)))
}

fn number_text(x: f64) -> String {
    if (x - x.round()).abs() < 1e-9 {
        format!("{}", x.round() as i64)
    } else {
        format!("{}", (x * 1000.0).round() / 1000.0)
    }
}

/// A safe first question when the model's reply can't be used.
pub fn tutor_opener(question: &str) -> String {
    match solve_linear(question) {
        Some((v, _)) => format!("Let's work it out together, one step at a time.\n\n**Your turn:** What could you do to both sides so the {v} term is on its own?"),
        None => "Let's work it out together, one step at a time.\n\n**Your turn:** What do you already know about this, and what do you think the first step is?".into(),
    }
}

/// Whether `text` states a value for `var` ("x = 4", "x=-2"; not "2x = 8").
pub fn states_value(text: &str, var: char) -> bool {
    // Spaces go only around "=", so word boundaries ("so x", "2x") still count.
    let mut flat = text.to_lowercase().replace(['*', '$'], "");
    while flat.contains(" =") || flat.contains("= ") {
        flat = flat.replace(" =", "=").replace("= ", "=");
    }
    let t: Vec<char> = flat.chars().collect();
    t.windows(3).enumerate().any(|(i, w)| {
        w[0] == var && w[1] == '=' && (w[2].is_ascii_digit() || w[2] == '-') && (i == 0 || !t[i - 1].is_alphanumeric())
    })
}

pub fn parse_tutor(reply: &str, final_answers: &[String], first_turn: bool) -> Option<String> {
    let v = research::lenient_json(reply);
    let text = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    // Nothing to give feedback on before the learner has answered anything.
    let feedback = if first_turn { String::new() } else { text("feedback") };
    let (step, question) = (text("step"), text("question"));
    if step.is_empty() && question.is_empty() {
        return None;
    }
    // Small models sometimes slip the final answer into the step, or echo the
    // conversation as the question: then it isn't a hint.
    let squash = |s: &str| s.to_lowercase().replace([' ', '*', '$'], "");
    let both = format!("{step} {question}");
    if final_answers.iter().any(|a| !a.is_empty() && squash(&both).contains(&squash(a))) {
        return None;
    }
    let lq = question.to_lowercase();
    // It must actually ask the learner something.
    if lq.contains("learner:") || lq.contains("tutor:") || !question.contains('?') {
        return None;
    }
    let mut out = String::new();
    if !feedback.is_empty() {
        out.push_str(&feedback);
        out.push_str("\n\n");
    }
    if !step.is_empty() {
        out.push_str(&step);
        out.push_str("\n\n");
    }
    if !question.is_empty() {
        out.push_str(&format!("**Your turn:** {question}"));
    }
    Some(out.trim().to_string())
}

/// One tutor turn with a fixed shape (small models otherwise solve the whole problem).
/// `None`: fall back to a normal answer.
pub async fn tutor_reply(http: &reqwest::Client, ep: &crate::engine::Endpoint, system: &str, history: &[crate::chat::ChatMessage], calc: Option<&str>) -> AppResult<Option<String>> {
    let transcript: String = history
        .iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| format!("{}: {}\n", if m.role == "user" { "Learner" } else { "Tutor" }, chat::question_text(&m.content).chars().take(1500).collect::<String>()))
        .collect();
    let user = format!(
        "The conversation so far:\n{transcript}\n{}Reply as the tutor, in three parts. feedback: ONLY if the learner's last message is their \
attempt at an answer, say kindly whether it's right and why; if their last message is a question or a new problem, leave it empty. step: explain or hint at ONLY the next small step; never state the final \
answer or finish the problem. question: one short question that asks the learner to do that step themselves.",
        calc.map(|c| format!("(Calculator, for checking only, don't reveal it: {c})\n")).unwrap_or_default()
    );
    // The final answer (the calculator's result, or the solved equation) must not appear in a hint,
    // and with an equation, no value for its variable at all (right or wrong).
    let learner = history.iter().rev().find(|m| m.role == "user").map(|m| chat::question_text(&m.content).to_string()).unwrap_or_default();
    let problem = history.iter().filter(|m| m.role == "user").find_map(|m| solve_linear(chat::question_text(&m.content)));
    let mut finals: Vec<String> = calc.and_then(|c| c.rsplit('=').next()).map(|r| vec![r.trim().to_string()]).unwrap_or_default();
    if let Some((v, x)) = problem {
        finals.push(format!("{v}={}", number_text(x)));
        finals.push(format!("{v}is{}", number_text(x)));
    }
    let first_turn = !history.iter().any(|m| m.role == "assistant");
    // The learner's own message echoed back isn't a question for them.
    let echo = |t: &str| learner.len() > 12 && t.to_lowercase().contains(&learner.to_lowercase());
    for attempt in 0..2 {
        let ask = if attempt == 0 { user.clone() } else { format!("{user}\nImportant: your last reply gave the answer away. Give only a hint for the next step.") };
        let reply = chat::complete_json(http, ep, system, &ask, tutor_schema(), 500).await?;
        let solves = |t: &str| problem.is_some_and(|(v, _)| states_value(t, v));
        if let Some(t) = parse_tutor(&reply, &finals, first_turn).filter(|t| !echo(t) && !solves(t)) {
            return Ok(Some(t));
        }
    }
    // Both replies gave it away: a safe opening question instead.
    Ok(Some(tutor_opener(&learner)))
}

/// Rules for tutor mode (the composer's Tutor button).
pub const TUTOR_RULES: &str = "\n\n## Tutor mode (overrides the answer-format rules above)\nYou are a patient tutor. Teach; don't just hand over answers. \
Keep replies short and conversational: no headings, no TL;DR, no full worked solutions.\n\
- Start by finding out what the learner already knows, and match their level (simpler words and smaller steps for beginners).\n\
- Guide with one step or one question at a time (the Socratic way), then wait for their reply.\n\
- When they're right, say so briefly and move on; when they're wrong, point to the exact mistake kindly and give a hint, not the answer.\n\
- Give the full solution only if they ask for it or are stuck after two hints.\n\
- For maths, check every number with the calculator result when one is present, and show the working step by step.\n\
- End each reply with a short question or a small practice problem, then stop and wait: never answer your own question.";

/// Added to the learner's message in tutor mode (small models follow this far better than the system prompt).
pub const TUTOR_NUDGE: &str = "\n\n[Tutor mode: don't solve it for me. Help me take just the next step with a hint or a question, then stop and wait for my answer.]";

/// Makes flashcards or a quiz. `Ok(None)`: not a study request, or nothing usable came back.
pub async fn run(turn: &Turn<'_>, question: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(ask) = study_ask(question) else { return Ok(None) };
    let c = Ctx { turn, cancel, send };
    let (topic, count, quiz) = match &ask {
        StudyAsk::Cards { topic, count } => (topic.clone(), *count, false),
        StudyAsk::Quiz { topic, count } => (topic.clone(), *count, true),
    };

    // The material: attached files, the answer above, the web, or what the model knows.
    let last = turn.history.iter().rev().find(|m| m.role == "user").map(|m| m.content.as_str()).unwrap_or("");
    let attached = last.len() > question.len() + 200;
    let mut book = SourceBook::default();
    let material: String = if attached {
        last.chars().take(14_000).collect()
    } else if about_the_chat(&topic) {
        turn.history.iter().rev().find(|m| m.role == "assistant").map(|m| m.content.chars().take(12_000).collect()).unwrap_or_default()
    } else if turn.web {
        let mut g = Gathered::default();
        let lists = research::run_searches(&c, &mut g, &[topic.clone(), format!("{topic} key facts explained")], "s").await?;
        let candidates = research::interleave(&lists, &[], 8);
        research::read_pages(&c, &mut g, &candidates, 3, "s").await?;
        let budget = research::notes_budget(turn, used_tokens, 0.35);
        let (picks, _) = c.cancellable(research::rank_texts(turn, &g.texts, &topic, "", budget, 3)).await?;
        book = g.book;
        research::format_notes(&book, &picks)
    } else {
        String::new()
    };

    let name = if quiz { "make_quiz" } else { "make_flashcards" };
    let id = format!("byte_{name}");
    c.call(&id, name, json!({ "topic": topic, "count": count }))?;
    let source = if material.trim().is_empty() {
        format!("Topic: {topic}\n(Use your own knowledge; stick to well-established facts.)")
    } else {
        format!("Topic: {topic}\n\nMaterial to study (use only what's here):\n{material}")
    };
    let reply = if quiz {
        let user = format!(
            "{source}\n\nWrite a {count}-question multiple-choice quiz on this. Each question has 4 choices with exactly one correct \
answer (put the correct choice's exact text in \"answer\"), plausible wrong choices, and a one-sentence explanation of why the answer is right. \
Mix easy and harder questions; test understanding, not trivia. Give the quiz a short title."
        );
        let mut reply = String::new();
        for attempt in 0..2 {
            let ask = if attempt == 0 { user.clone() } else { format!("{user}\nEvery question must be different. Keep each one short.") };
            reply = c.cancellable(chat::complete_json(turn.http, turn.ep, "You write clear, fair quizzes. Reply only with JSON.", &ask, quiz_schema(count), (count * (160 + attempt * 100) + 300) as u32)).await?.unwrap_or_default();
            if parse_quiz(&reply, count).is_some() {
                break;
            }
        }
        let quiz_sys = "You write clear, fair quizzes. Reply only with JSON.";
        c.cancellable(crate::quality::improve(turn, quiz_sys, &user, quiz_schema(count), (count * 260 + 300) as u32, reply, |r| parse_quiz(r, count).map(|q| crate::quality::quiz(&q)))).await??
    } else {
        let user = format!(
            "{source}\n\nWrite {count} flashcards on this. Each has a short, specific question or term on the front and a clear, \
complete answer on the back (one idea per card, every front different, no yes/no questions). Plain text only: no Markdown, no LaTeX or $ signs (write CO2, x^2). Cover the most important ideas first. Give the set a short title."
        );
        // Small models sometimes run out of room or repeat a card; one more try with more room.
        let mut reply = String::new();
        for attempt in 0..2 {
            let ask = if attempt == 0 { user.clone() } else { format!("{user}\nEvery card must have a different front. Keep each back to one or two sentences.") };
            reply = c.cancellable(chat::complete_json(turn.http, turn.ep, "You write excellent study flashcards. Reply only with JSON.", &ask, cards_schema(count), (count * (90 + attempt * 80) + 300) as u32)).await?.unwrap_or_default();
            if parse_cards(&reply, count).is_some() {
                break;
            }
        }
        let cards_sys = "You write excellent study flashcards. Reply only with JSON.";
        c.cancellable(crate::quality::improve(turn, cards_sys, &user, cards_schema(count), (count * 170 + 300) as u32, reply, |r| parse_cards(r, count).map(|f| crate::quality::flashcards(&f, &topic, count)))).await??
    };
    let fallback_title = |t: &str| if t.is_empty() { topic.chars().take(60).collect::<String>() } else { t.to_string() };
    if quiz {
        let Some(mut q) = parse_quiz(&reply, count) else {
            c.result(&id, false, "The quiz couldn't be read")?;
            return Ok(None);
        };
        q.title = fallback_title(&q.title);
        c.result(&id, true, format!("{} questions", q.questions.len()))?;
        send(ChatEvent::Quiz(q.clone()))?;
        Ok(Some((book, format!("Quiz shown: {} ({} questions).\n\n{QUIZ_RULES}", q.title, q.questions.len()))))
    } else {
        let Some(mut f) = parse_cards(&reply, count) else {
            c.result(&id, false, "The cards couldn't be read")?;
            return Ok(None);
        };
        f.title = fallback_title(&f.title);
        c.result(&id, true, format!("{} cards", f.cards.len()))?;
        send(ChatEvent::Flashcards(f.clone()))?;
        Ok(Some((book, format!("Flashcards shown: {} ({} cards).\n\n{CARDS_RULES}", f.title, f.cards.len()))))
    }
}

// ---------- decks (DB tables `decks`, `cards`) ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckSummary {
    pub id: i64,
    pub name: String,
    pub cards: u32,
    /// Cards due today or earlier.
    pub due: u32,
    /// Cards never studied.
    pub new: u32,
    pub created: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudyCard {
    pub id: i64,
    pub deck_id: i64,
    pub front: String,
    pub back: String,
    #[serde(flatten)]
    pub schedule: Schedule,
}

/// Saves cards as a new deck (or adds them to the deck with that name).
pub fn deck_save(db: &crate::db::Db, name: &str, cards: &[Card]) -> AppResult<i64> {
    let name = name.trim();
    if name.is_empty() || cards.is_empty() {
        return Err(AppError::msg("a deck needs a name and at least one card"));
    }
    let conn = db.conn();
    let now = chrono::Utc::now().timestamp_millis();
    let id: i64 = match conn.query_row("SELECT id FROM decks WHERE name = ?1", [name], |r| r.get(0)) {
        Ok(id) => id,
        Err(_) => {
            conn.execute("INSERT INTO decks (name, created) VALUES (?1, ?2)", rusqlite::params![name, now])?;
            conn.last_insert_rowid()
        }
    };
    let today = today();
    for c in cards {
        let exists: bool = conn.query_row("SELECT 1 FROM cards WHERE deck_id = ?1 AND front = ?2", rusqlite::params![id, c.front], |_| Ok(true)).unwrap_or(false);
        if !exists {
            conn.execute(
                "INSERT INTO cards (deck_id, front, back, ease, interval, reps, lapses, due, created) VALUES (?1, ?2, ?3, 2.5, 0, 0, 0, ?4, ?5)",
                rusqlite::params![id, c.front, c.back, today, now],
            )?;
        }
    }
    Ok(id)
}

pub fn decks_list(db: &crate::db::Db) -> AppResult<Vec<DeckSummary>> {
    let today = today();
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT d.id, d.name, d.created,
                (SELECT COUNT(*) FROM cards c WHERE c.deck_id = d.id),
                (SELECT COUNT(*) FROM cards c WHERE c.deck_id = d.id AND c.due <= ?1 AND c.reps + c.lapses > 0),
                (SELECT COUNT(*) FROM cards c WHERE c.deck_id = d.id AND c.reps + c.lapses = 0)
         FROM decks d ORDER BY d.created DESC",
    )?;
    let rows = st.query_map([today], |r| Ok(DeckSummary { id: r.get(0)?, name: r.get(1)?, created: r.get(2)?, cards: r.get(3)?, due: r.get(4)?, new: r.get(5)? }))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn card_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<StudyCard> {
    Ok(StudyCard {
        id: r.get(0)?,
        deck_id: r.get(1)?,
        front: r.get(2)?,
        back: r.get(3)?,
        schedule: Schedule { ease: r.get(4)?, interval: r.get(5)?, reps: r.get(6)?, lapses: r.get(7)?, due: r.get(8)? },
    })
}

const CARD_COLS: &str = "id, deck_id, front, back, ease, interval, reps, lapses, due";

pub fn deck_cards(db: &crate::db::Db, deck_id: i64) -> AppResult<Vec<StudyCard>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {CARD_COLS} FROM cards WHERE deck_id = ?1 ORDER BY id"))?;
    let rows = st.query_map([deck_id], card_from)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Cards to study now: due reviews first, then up to `new_limit` new cards.
pub fn study_queue(db: &crate::db::Db, deck_id: i64, new_limit: u32) -> AppResult<Vec<StudyCard>> {
    let today = today();
    let conn = db.conn();
    let mut out = Vec::new();
    let mut st = conn.prepare(&format!("SELECT {CARD_COLS} FROM cards WHERE deck_id = ?1 AND due <= ?2 AND reps + lapses > 0 ORDER BY due, id"))?;
    for c in st.query_map(rusqlite::params![deck_id, today], card_from)? {
        out.push(c?);
    }
    let mut st = conn.prepare(&format!("SELECT {CARD_COLS} FROM cards WHERE deck_id = ?1 AND reps + lapses = 0 ORDER BY id LIMIT ?2"))?;
    for c in st.query_map(rusqlite::params![deck_id, new_limit], card_from)? {
        out.push(c?);
    }
    Ok(out)
}

pub fn card_review(db: &crate::db::Db, card_id: i64, grade: u8) -> AppResult<StudyCard> {
    let conn = db.conn();
    let card = conn.query_row(&format!("SELECT {CARD_COLS} FROM cards WHERE id = ?1"), [card_id], card_from).map_err(|_| AppError::msg("that card is gone"))?;
    let s = review(card.schedule, grade, today());
    conn.execute(
        "UPDATE cards SET ease = ?1, interval = ?2, reps = ?3, lapses = ?4, due = ?5 WHERE id = ?6",
        rusqlite::params![s.ease, s.interval, s.reps, s.lapses, s.due, card_id],
    )?;
    conn.execute("INSERT INTO card_reviews (card_id, at, grade) VALUES (?1, ?2, ?3)", rusqlite::params![card_id, chrono::Utc::now().timestamp_millis(), grade])?;
    Ok(StudyCard { schedule: s, ..card })
}

pub fn deck_delete(db: &crate::db::Db, deck_id: i64) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM card_reviews WHERE card_id IN (SELECT id FROM cards WHERE deck_id = ?1)", [deck_id])?;
    conn.execute("DELETE FROM cards WHERE deck_id = ?1", [deck_id])?;
    conn.execute("DELETE FROM decks WHERE id = ?1", [deck_id])?;
    Ok(())
}

pub fn card_delete(db: &crate::db::Db, card_id: i64) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM card_reviews WHERE card_id = ?1", [card_id])?;
    conn.execute("DELETE FROM cards WHERE id = ?1", [card_id])?;
    Ok(())
}

/// A deck as an Anki import file (File → Import in Anki): tab-separated, with
/// the headers Anki 2.1.55+ reads (deck name, no HTML).
pub fn anki_text(name: &str, cards: &[StudyCard]) -> String {
    let field = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let mut out = format!("#separator:tab\n#html:false\n#notetype:Basic\n#deck:{}\n", field(name));
    for c in cards {
        out.push_str(&format!("{}\t{}\n", field(&c.front), field(&c.back)));
    }
    out
}

pub fn deck_export(db: &crate::db::Db, deck_id: i64) -> AppResult<String> {
    let name: String = db.conn().query_row("SELECT name FROM decks WHERE id = ?1", [deck_id], |r| r.get(0)).map_err(|_| AppError::msg("that deck is gone"))?;
    Ok(anki_text(&name, &deck_cards(db, deck_id)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sm2_schedules() {
        let t = 1000;
        let s = Schedule::new(t);
        let a = review(s, 4, t);
        assert_eq!((a.reps, a.interval, a.due), (1, 1, t + 1));
        let b = review(a, 4, t + 1);
        assert_eq!((b.reps, b.interval), (2, 6));
        let c = review(b, 4, t + 7);
        assert_eq!(c.interval, 15); // 6 × 2.5
        assert!((c.ease - 2.5).abs() < 1e-9);
        // Easy grows faster and raises the ease; hard is slower and lowers it.
        let easy = review(b, 5, t + 7);
        assert!(easy.interval > c.interval && easy.ease > 2.5);
        let hard = review(b, 3, t + 7);
        assert!(hard.interval < c.interval && hard.ease < 2.5);
        // Again: back to the start, counted as a lapse, due tomorrow.
        let again = review(c, 1, t + 22);
        assert_eq!((again.reps, again.interval, again.lapses, again.due), (0, 1, 1, t + 23));
        // The ease never drops below 1.3.
        let mut x = s;
        for _ in 0..20 {
            x = review(x, 0, t);
        }
        assert!((x.ease - 1.3).abs() < 1e-9);
    }

    #[test]
    fn study_requests_are_understood() {
        assert_eq!(study_ask("Make flashcards about the French Revolution"), Some(StudyAsk::Cards { topic: "the French Revolution".into(), count: 12 }));
        assert_eq!(study_ask("make 20 flashcards on photosynthesis"), Some(StudyAsk::Cards { topic: "photosynthesis".into(), count: 20 }));
        assert_eq!(study_ask("Quiz me on the periodic table"), Some(StudyAsk::Quiz { topic: "the periodic table".into(), count: 8 }));
        assert_eq!(study_ask("give me a 5 question quiz about World War 2"), Some(StudyAsk::Quiz { topic: "World War 2".into(), count: 5 }));
        assert!(matches!(study_ask("turn this into flashcards"), Some(StudyAsk::Cards { .. })));
        assert_eq!(study_ask("What are flashcards?"), None);
        assert_eq!(study_ask("What's the weather?"), None);
        assert!(about_the_chat("this"));
        assert!(about_the_chat(""));
        assert!(!about_the_chat("photosynthesis"));
    }

    #[test]
    fn cards_and_quizzes_are_cleaned() {
        let f = parse_cards(r#"{"title":"Cells","cards":[{"front":"Mitochondria","back":"Makes ATP"},{"front":"mitochondria","back":"dup"},{"front":"Nucleus","back":""},{"front":"Ribosome","back":"Makes proteins"}]}"#, 10).unwrap();
        assert_eq!(f.cards.len(), 2);
        assert!(parse_cards("{}", 5).is_none());
        let q = parse_quiz(
            r#"{"title":"Chem","questions":[
                {"question":"Symbol for gold?","choices":["Ag","Au","Gd","Go"],"answer":1,"explanation":"Latin aurum."},
                {"question":"H2O is?","choices":["Water","Salt"],"answer":"Water"},
                {"question":"Pick B","choices":["x","y","z"],"answer":"B"},
                {"question":"Bad","choices":["only one"],"answer":0},
                {"question":"Out of range","choices":["a","b"],"answer":7}
            ]}"#,
            10,
        )
        .unwrap();
        assert_eq!(q.questions.len(), 3);
        assert_eq!(q.questions[0].answer, 1);
        assert_eq!(q.questions[1].answer, 0);
        assert_eq!(q.questions[2].answer, 1);
        let q = parse_quiz(r#"{"questions":[{"question":"Largest planet?","choices":["Mars","Jupiter","Venus"],"answer":"Jupiter."},{"question":"Closest to the Sun?","choices":["Mercury","Earth"],"answer":"The answer is Mercury"}]}"#, 5).unwrap();
        assert_eq!((q.questions[0].answer, q.questions[1].answer), (1, 0));
        assert!(reply_for("Flashcards shown: Cells (6 cards).\n\nrules").unwrap().contains("Cells (6 cards)"));
        assert!(reply_for("Quiz shown: X (4 questions).").unwrap().contains("Check my answers"));
        assert!(reply_for("Something else").is_none());
    }

    #[test]
    fn anki_export_is_tab_separated() {
        let c = |f: &str, b: &str| StudyCard { id: 1, deck_id: 1, front: f.into(), back: b.into(), schedule: Schedule::new(0) };
        let t = anki_text("Bio\t101", &[c("Mito\tchondria", "Makes\nATP")]);
        assert_eq!(t, "#separator:tab\n#html:false\n#notetype:Basic\n#deck:Bio 101\nMito chondria\tMakes ATP\n");
    }

    /// Real engine: flashcards, a quiz, and a tutor reply that asks back.
    #[tokio::test]
    #[ignore]
    async fn e2e_study() {
        use crate::agent::{run as agent_run, Modules, Task, Turn};
        let Ok(model) = std::env::var("BYTE_TEST_MODEL") else { return };
        let Some((_server, mut ep)) = crate::chat::e2e_support::start_server_with(&model, &["-c".into(), "16384".into()], None).await else { return };
        ep.context = 16384;
        let dir = tempfile::tempdir().unwrap();
        let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = crate::tools::fetch::web_client();
        let modules = Modules { study: true, small_model: true, ..Default::default() };
        for (q, task, want) in [
            ("Make 6 flashcards about photosynthesis", None, "flashcards"),
            ("Quiz me on the solar system, 4 questions", None, "quiz"),
            ("How do I solve 2x + 6 = 14?", Some(Task::Tutor), "content"),
        ] {
            let history = vec![chat::ChatMessage::new("user", q)];
            let system = crate::prompt::system_prompt(chrono::Local::now(), crate::settings::Mode::Auto, false, None);
            let plan = crate::router::plan_turn(crate::settings::Mode::Auto, crate::settings::ThinkingPref::Off, q);
            let (ch, seen) = crate::chat::e2e_support::collecting_channel();
            let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: crate::settings::Mode::Auto, web: false, memory: false, log: &log, files: None, app: None, task, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false, modules };
            agent_run(turn, CancellationToken::new(), &ch).await.unwrap();
            let ev = seen.lock().unwrap().clone();
            let card = ev.iter().find(|e| e["kind"] == want).cloned();
            let answer: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
            eprintln!("--- {q}\ncard: {}\nanswer: {answer}", card.as_ref().map(|c| c.to_string().chars().take(700).collect::<String>()).unwrap_or_default());
            assert!(card.is_some(), "no {want} for {q}");
            if task == Some(Task::Tutor) {
                assert!(answer.contains('?'), "the tutor should ask something back: {answer}");
                assert!(!states_value(&answer, 'x'), "the tutor solved it instead of teaching: {answer}");
            }
        }
    }

    #[test]
    fn cut_off_replies_keep_their_finished_cards() {
        let cut = r#"{"title":"Cells","cards":[{"front":"What is a cell?","back":"The smallest unit of life."},{"front":"What holds DNA?","back":"The nucleus {in eukaryotes}."},{"front":"What makes ATP?","back":"The mito"#;
        let f = parse_cards(cut, 10).unwrap();
        assert_eq!(f.title, "Cells");
        assert_eq!(f.cards.len(), 2);
        assert_eq!(f.cards[1].back, "The nucleus {in eukaryotes}.");
        assert!(parse_cards(r#"{"title":"x","cards":[{"front":"a","#, 10).is_none());
        let dup = r#"{"title":"Q","questions":[{"question":"Closest planet?","choices":["Mercury","Venus"],"answer":"Mercury"},{"question":"Closest planet?","choices":["Mercury","Venus"],"answer":"Mercury"},{"question":"Largest planet?","choices":["Jupiter","Mars"],"answer":"Jupiter"}]}"#;
        assert_eq!(parse_quiz(dup, 10).unwrap().questions.len(), 2);
    }

    #[test]
    fn tutor_turns_are_shaped() {
        let t = parse_tutor(r#"{"feedback":"","step":"Start by getting 2x alone: what could you subtract from both sides?","question":"What do you get after subtracting 6?"}"#, &[], false).unwrap();
        assert!(t.ends_with("**Your turn:** What do you get after subtracting 6?"));
        assert!(parse_tutor(r#"{"step":"So x = 4.","question":"Right?"}"#, &["x = 4".into()], false).is_none());
        assert!(parse_tutor("{}", &[], false).is_none());
        let first = parse_tutor(r#"{"feedback":"That's right!","step":"Look at the +6.","question":"What undoes adding 6?"}"#, &[], true).unwrap();
        assert!(!first.contains("right!"));
        // The exact reply the 0.6B model gave on the Mac runner: rejected.
        let leaked = r#"{"step":"You can solve this equation by first subtracting 6 from both sides. This will give you 2x = 8. Then, divide both sides by 2 to get x = 4.","question":"Learner: How do I solve 2x + 6 = 14?"}"#;
        assert!(parse_tutor(leaked, &["x=4".into(), "xis4".into()], true).is_none());
        assert!(parse_tutor(r#"{"step":"Look at the +6 first.","question":"Learner: how do I solve it?"}"#, &[], true).is_none());
        assert!(parse_tutor(r#"{"step":"Subtract 6 first.","question":"Please simplify the equation step by step."}"#, &[], true).is_none());
        assert!(states_value("Divide by 2 to solve for x. x = 8.", 'x'));
        assert!(states_value("so **x=-3**", 'x'));
        assert!(!states_value("2x = 14 - 6", 'x'));
        assert!(!states_value("What is x?", 'x'));
        assert_eq!(solve_linear("How do I solve 2x + 6 = 14?"), Some(('x', 4.0)));
        assert_eq!(solve_linear("solve 3x - 2 = 10"), Some(('x', 4.0)));
        assert_eq!(solve_linear("5 + 2y = 11"), Some(('y', 3.0)));
        assert_eq!(solve_linear("4x = 2x + 10"), Some(('x', 5.0)));
        assert_eq!(solve_linear("what is 2 + 2 = ?"), None);
        assert_eq!(solve_linear("no equation here"), None);
        let o = tutor_opener("How do I solve 2x + 6 = 14?");
        assert!(o.contains("x term") && !o.contains('4'));
        assert!(wants_solution("ok just tell me the answer"));
        assert!(!wants_solution("how do I solve 2x + 6 = 14?"));
    }

    #[test]
    fn decks_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(dir.path()).unwrap();
        let id = deck_save(&db, "Cells", &[Card { front: "Mitochondria".into(), back: "Makes ATP".into() }, Card { front: "Ribosome".into(), back: "Makes proteins".into() }]).unwrap();
        // Saving again adds only new cards.
        assert_eq!(deck_save(&db, "Cells", &[Card { front: "Mitochondria".into(), back: "x".into() }, Card { front: "Nucleus".into(), back: "Holds DNA".into() }]).unwrap(), id);
        let decks = decks_list(&db).unwrap();
        assert_eq!((decks[0].cards, decks[0].new, decks[0].due), (3, 3, 0));
        let queue = study_queue(&db, id, 20).unwrap();
        assert_eq!(queue.len(), 3);
        let reviewed = card_review(&db, queue[0].id, 4).unwrap();
        assert_eq!(reviewed.schedule.reps, 1);
        assert_eq!(decks_list(&db).unwrap()[0].new, 2);
        assert!(deck_export(&db, id).unwrap().contains("Mitochondria\tMakes ATP"));
        deck_delete(&db, id).unwrap();
        assert!(decks_list(&db).unwrap().is_empty());
    }
}
