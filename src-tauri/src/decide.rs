//! Compare & decide (Phase 6, v0.6.1). For "X vs Y", "which is better…",
//! "should I get X or Y", BYTE:
//!
//! 1. names the options (2–5) and the criteria that matter (3–7, weighted
//!    1–5 from what the user said) with a short JSON request,
//! 2. researches each option (a search per option plus a head-to-head one),
//!    reads pages and keeps the best passages,
//! 3. asks the model to score every option on every criterion (1–10, a
//!    one-line reason and the source numbers),
//! 4. sends the scores to the UI as a `Decision` (an interactive table with a
//!    weight slider per criterion), then the model writes a short, cited
//!    recommendation.
//!
//! If the options can't be read from the question, the turn falls back to
//! plain research.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::{self, ChatEvent};
use crate::error::AppResult;
use crate::research::{self, Ctx, Emit, Gathered};
use crate::settings::Mode;
use crate::tools::SourceBook;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Criterion {
    pub name: String,
    /// How much it matters, 1–5.
    pub weight: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    /// 1–10.
    pub score: u8,
    pub reason: String,
    /// Source numbers backing the score.
    pub sources: Vec<u32>,
}

/// The score table sent to the UI: `scores[option][criterion]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub options: Vec<String>,
    pub criteria: Vec<Criterion>,
    pub scores: Vec<Vec<Option<Cell>>>,
}

impl Decision {
    /// Weighted totals out of 10, one per option (unscored cells don't count).
    pub fn totals(&self) -> Vec<f64> {
        self.scores
            .iter()
            .map(|row| {
                let (sum, w) = row.iter().zip(&self.criteria).fold((0.0, 0.0), |(s, w), (cell, c)| match cell {
                    Some(cell) => (s + cell.score as f64 * c.weight as f64, w + c.weight as f64),
                    None => (s, w),
                });
                if w > 0.0 {
                    sum / w
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// The table as text for the model (it writes the recommendation from it).
    pub fn as_text(&self) -> String {
        let mut out = format!("| Criterion (weight) | {} |\n|---|{}\n", self.options.join(" | "), "---|".repeat(self.options.len()));
        for (j, c) in self.criteria.iter().enumerate() {
            let cells: Vec<String> = self
                .scores
                .iter()
                .map(|row| match &row[j] {
                    Some(cell) => {
                        let cites: String = cell.sources.iter().map(|n| format!("[{n}]")).collect();
                        let reason = if cell.reason.is_empty() { String::new() } else { format!(" {}", cell.reason) };
                        format!("{}/10{reason}{cites}", cell.score)
                    }
                    None => "not scored".into(),
                })
                .collect();
            out.push_str(&format!("| {} ({}) | {} |\n", c.name, c.weight, cells.join(" | ")));
        }
        let totals: Vec<String> = self.totals().iter().map(|t| format!("{t:.1}")).collect();
        out.push_str(&format!("| **Weighted total** | {} |\n", totals.join(" | ")));
        out
    }
}

pub const DECIDE_RULES: &str = "The user sees the score table above as an interactive table (they can change the \
weights), so don't repeat it as a table. Write a short recommendation from the notes and scores: start with a one-line \
**TL;DR:** blockquote naming the best pick for the user's situation, then a `##` section per option with its main \
strengths and weaknesses, citing sources like [3]. Then a short section on what would change the pick (e.g. \"if \
battery life matters most, choose …\"). Use only source numbers listed above and don't add a list of sources. End \
with one line exactly like `**Confidence:** Likely — <why, in one short sentence>` using Verified, Likely or Unsure.";

/// Whether this question gets the compare & decide flow.
pub fn applies(web: bool, question: &str) -> bool {
    web && crate::router::wants_compare(question)
}

/// Options named in the question: "X vs Y", "should I get X or Y",
/// "which is better, X or Y", "compare X, Y and Z". Empty when there aren't two.
pub fn options_from_question(question: &str) -> Vec<String> {
    let q = question.trim().trim_end_matches(['?', '!', '.']).replace('\u{2019}', "'");
    let lower = q.to_lowercase();
    // Cut a side at the words that start the user's situation ("for college").
    let tail_cut_list = |t: &str, commas: bool| -> String {
        let l = t.to_lowercase();
        let mut end = t.len();
        for w in [" for ", " to ", " in ", " if ", " when ", " as ", " with my ", " on a ", " under ", " at ", ", ", "? ", " - "] {
            if w == ", " && !commas {
                continue;
            }
            if let Some(i) = l.find(w) {
                end = end.min(i);
            }
        }
        t[..end].to_string()
    };
    let tail_cut = |t: &str| tail_cut_list(t, true);
    let head_cut = |t: &str| -> String {
        let l = t.to_lowercase();
        let mut start = 0;
        for w in [": ", ", ", "compare ", "between ", "get ", "buy ", "choose ", "pick ", "better ", "best "] {
            if let Some(i) = l.rfind(w) {
                start = start.max(i + w.len());
            }
        }
        t[start..].to_string()
    };
    let clean = |t: &str| -> String {
        let t = t.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '+' && c != ')');
        let l = t.to_lowercase();
        let t = ["a ", "an ", "the ", "my "].iter().find(|a| l.starts_with(**a)).map(|a| &t[a.len()..]).unwrap_or(t);
        t.trim().to_string()
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some((sep, i)) = [" vs. ", " vs ", " versus "].iter().filter_map(|s| lower.find(s).map(|i| (*s, i))).min_by_key(|(_, i)| *i) {
        let (left, right) = (&q[..i], &q[i + sep.len()..]);
        parts.push(head_cut(left));
        // "A vs B vs C"
        for r in right.split(" vs ").flat_map(|x| x.split(" vs. ")) {
            parts.push(tail_cut(r));
        }
    } else if lower.contains(" or ") {
        let start = ["should i get ", "should i buy ", "should i choose ", "should i pick ", "should i use ", "should i go with ", "which is better", "which one is better", "which should i", "better:", "better,"]
            .iter()
            .filter_map(|c| lower.find(c).map(|i| i + c.len()))
            .max();
        let Some(start) = start else { return Vec::new() };
        let mut body = q[start..].to_string();
        // "which is better for a beginner, Python or JavaScript" → after the comma.
        if let Some(i) = body.rfind([',', ':']) {
            body = body[i + 1..].to_string();
        }
        let items: Vec<&str> = body.split(" or ").flat_map(|x| x.split(", ")).collect();
        for (k, it) in items.iter().enumerate() {
            parts.push(if k + 1 == items.len() { tail_cut(it) } else { it.to_string() });
        }
    } else if let Some(i) = lower.find("compare ") {
        let body = tail_cut_list(&q[i + 8..].replace(" with ", " and "), false);
        parts.extend(body.split(" and ").flat_map(|x| x.split(", ")).map(str::to_string));
    }
    let mut out: Vec<String> = Vec::new();
    for p in parts.iter().map(|p| clean(p)) {
        let words = p.split_whitespace().count();
        if (1..=6).contains(&words) && !out.iter().any(|o| o.eq_ignore_ascii_case(&p)) {
            out.push(p);
        }
    }
    if out.len() < 2 {
        return Vec::new();
    }
    out.truncate(5);
    out
}

/// An option the model wrote that is really our prompt echoed back.
fn junk_option(o: &str) -> bool {
    let l = o.to_lowercase();
    l.chars().all(|c| !c.is_alphabetic()) || ["short name", "option", "2 to 5", "choice a", "choice b"].iter().any(|j| l == *j || l.starts_with(j))
}

fn setup_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "options": { "type": "array", "minItems": 2, "maxItems": 5, "items": { "type": "string" } },
            "criteria": { "type": "array", "minItems": 3, "maxItems": 7, "items": {
                "type": "object",
                "properties": { "name": { "type": "string" }, "weight": { "type": "integer", "minimum": 1, "maximum": 5 } },
                "required": ["name", "weight"]
            }}
        },
        "required": ["options", "criteria"]
    })
}

/// Options and criteria from the model's reply; None without 2 real options.
pub fn parse_setup(reply: &str) -> Option<(Vec<String>, Vec<Criterion>)> {
    let v = research::lenient_json(reply);
    let mut options: Vec<String> = Vec::new();
    for o in v["options"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let o: String = o.trim().trim_matches('"').chars().take(80).collect();
        if !o.is_empty() && !junk_option(&o) && !options.iter().any(|x| x.eq_ignore_ascii_case(&o)) {
            options.push(o);
        }
    }
    options.truncate(5);
    if options.len() < 2 {
        return None;
    }
    let mut criteria: Vec<Criterion> = Vec::new();
    for c in v["criteria"].as_array().into_iter().flatten() {
        let name: String = c["name"].as_str().unwrap_or("").trim().chars().take(60).collect();
        if name.is_empty() || criteria.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let weight = c["weight"].as_u64().unwrap_or(3).clamp(1, 5) as u8;
        criteria.push(Criterion { name, weight });
    }
    criteria.truncate(7);
    if criteria.is_empty() {
        criteria = ["Quality", "Price", "Ease of use"].iter().map(|n| Criterion { name: n.to_string(), weight: 3 }).collect();
    }
    Some((options, criteria))
}

fn scores_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "scores": { "type": "array", "items": {
                "type": "object",
                "properties": {
                    "option": { "type": "string" },
                    "criterion": { "type": "string" },
                    "score": { "type": "integer", "minimum": 1, "maximum": 10 },
                    "reason": { "type": "string" },
                    "sources": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["option", "criterion", "score", "reason", "sources"]
            }}
        },
        "required": ["scores"]
    })
}

/// Finds a name in `list`: exact (ignoring case), else one containing the other.
fn find(list: &[&str], name: &str) -> Option<usize> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    list.iter()
        .position(|x| x.to_lowercase() == n)
        .or_else(|| list.iter().position(|x| { let x = x.to_lowercase(); x.contains(&n) || n.contains(&x) }))
}

/// The score grid from the model's reply. Scores are clamped to 1–10, source
/// numbers not in `valid` are dropped, unknown options/criteria are ignored.
pub fn parse_scores(reply: &str, options: &[String], criteria: &[Criterion], valid: &[u32]) -> Vec<Vec<Option<Cell>>> {
    let v = research::lenient_json(reply);
    let opts: Vec<&str> = options.iter().map(String::as_str).collect();
    let crits: Vec<&str> = criteria.iter().map(|c| c.name.as_str()).collect();
    let mut grid: Vec<Vec<Option<Cell>>> = vec![vec![None; criteria.len()]; options.len()];
    for s in v["scores"].as_array().into_iter().flatten() {
        let (Some(i), Some(j)) = (find(&opts, s["option"].as_str().unwrap_or("")), find(&crits, s["criterion"].as_str().unwrap_or(""))) else { continue };
        let Some(score) = s["score"].as_f64() else { continue };
        let sources: Vec<u32> = s["sources"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n| n as u32).filter(|n| valid.contains(n)).collect();
        grid[i][j] = Some(Cell { score: score.round().clamp(1.0, 10.0) as u8, reason: s["reason"].as_str().unwrap_or("").trim().chars().take(160).collect(), sources });
    }
    grid
}

/// Pages read in total, by mode.
fn pages(mode: Mode) -> usize {
    match mode {
        Mode::Fast => 4,
        Mode::Auto => 6,
        Mode::Deep => 10,
        Mode::Extended => 16,
    }
}

/// Runs compare & decide. `Ok(None)`: the options couldn't be found (the
/// caller falls back to research). Otherwise the sources and model notes.
pub async fn run(turn: &Turn<'_>, question: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let c = Ctx { turn, cancel, send };
    let mut g = Gathered::default();

    c.call("byte_dsetup", "plan_comparison", json!({}))?;
    let named = options_from_question(question);
    let ask = if named.is_empty() {
        "Which specific products, services or choices should the user compare (at least two, each a short name like \"MacBook Air M4\")?".to_string()
    } else {
        format!("The options are: {}. Repeat them exactly as the options.", named.join("; "))
    };
    let user = format!(
        "Question:\n{question}\n\n{ask} Then list the 3 to 7 criteria that matter most for this user's choice, each \
weighted 1 (minor) to 5 (crucial) from what the user said about their needs."
    );
    let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, "You set up comparisons. Reply only with JSON.", &user, setup_schema(), 400)).await?.unwrap_or_default();
    let setup = parse_setup(&reply).map(|(o, cr)| (if named.is_empty() { o } else { named.clone() }, cr)).or_else(|| {
        // The model's reply was unusable but the question names the options.
        (!named.is_empty()).then(|| (named.clone(), parse_setup(r#"{"options":["a","b"]}"#).map(|x| x.1).unwrap_or_default()))
    });
    let Some((options, criteria)) = setup else {
        c.result("byte_dsetup", false, "Couldn't tell what to compare")?;
        return Ok(None);
    };
    c.result("byte_dsetup", true, format!("{} options, {} criteria", options.len(), criteria.len()))?;

    // A search per option (with the top criteria), plus a head-to-head one.
    let mut top: Vec<&Criterion> = criteria.iter().collect();
    top.sort_by_key(|c| std::cmp::Reverse(c.weight));
    let focus: Vec<&str> = top.iter().take(2).map(|c| c.name.as_str()).collect();
    let mut queries: Vec<String> = options.iter().map(|o| format!("{o} review {}", focus.join(" "))).collect();
    queries.push(options.join(" vs "));
    let lists = research::run_searches(&c, &mut g, &queries, "d").await?;
    let candidates = research::interleave(&lists, &[], research::scale_pages(pages(turn.mode), turn.depth) * 2);
    research::read_pages(&c, &mut g, &candidates, research::scale_pages(pages(turn.mode), turn.depth), "d").await?;

    c.call("byte_drank", "rank_passages", json!({}))?;
    let terms = format!("{} {}", options.join(" "), criteria.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(" "));
    let budget = research::notes_budget(turn, used_tokens, 0.4);
    let (picks, by_meaning) = c.cancellable(research::rank_texts(turn, &g.texts, question, &terms, budget, 3)).await?;
    let notes = research::format_notes(&g.book, &picks);
    c.result("byte_drank", !picks.is_empty(), research::rank_summary(picks.len(), &notes, by_meaning))?;

    c.call("byte_dscore", "score_options", json!({}))?;
    let user = format!(
        "Question:\n{question}\n\nOptions: {}\nCriteria: {}\n\nResearch notes:\n{}\n\nScore every option on every \
criterion from 1 (poor) to 10 (excellent) for this user, with a one-line reason and the numbers of the sources that \
support it (from the notes; an empty list if it's general knowledge). Be fair and use the notes.",
        options.join("; "),
        criteria.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join("; "),
        notes.chars().take(budget.min(12_000)).collect::<String>()
    );
    let cells = options.len() * criteria.len();
    let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, "You score options fairly. Reply only with JSON.", &user, scores_schema(), (cells * 70 + 200) as u32)).await?.unwrap_or_default();
    let valid: Vec<u32> = g.book.sources.iter().map(|s| s.n).collect();
    let decision = Decision { scores: parse_scores(&reply, &options, &criteria, &valid), options, criteria };
    let scored = decision.scores.iter().flatten().filter(|c| c.is_some()).count();
    c.result("byte_dscore", scored > 0, format!("{scored} of {cells} scores"))?;
    if scored > 0 {
        send(ChatEvent::Decision(decision.clone()))?;
    }

    let table = if scored > 0 { format!("Scores:\n{}\n", decision.as_text()) } else { String::new() };
    let content = format!("Comparison notes for: {question}\n\n{notes}\n{table}\n{DECIDE_RULES}");
    Ok(Some((g.book, content)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crit(n: &str, w: u8) -> Criterion {
        Criterion { name: n.into(), weight: w }
    }

    #[test]
    fn setup_is_read_leniently() {
        let (o, c) = parse_setup(r#"{"options":["MacBook Air M4","Dell XPS 13","macbook air m4"],"criteria":[{"name":"Battery life","weight":7},{"name":"Price","weight":0},{"name":"price","weight":3},{"name":"Build"}]}"#).unwrap();
        assert_eq!(o, vec!["MacBook Air M4", "Dell XPS 13"]);
        assert_eq!(c, vec![crit("Battery life", 5), crit("Price", 1), crit("Build", 3)]);
        assert!(parse_setup(r#"{"options":["Only one"],"criteria":[]}"#).is_none());
        assert!(parse_setup("no json").is_none());
        // Criteria missing: sensible defaults.
        assert_eq!(parse_setup(r#"{"options":["A","B"]}"#).unwrap().1.len(), 3);
    }

    #[test]
    fn options_come_from_the_question() {
        assert_eq!(options_from_question("MacBook Air M4 vs Dell XPS 13 for a college student?"), vec!["MacBook Air M4", "Dell XPS 13"]);
        assert_eq!(options_from_question("Should I get an iPhone 17 or a Pixel 10?"), vec!["iPhone 17", "Pixel 10"]);
        assert_eq!(options_from_question("Which is better for a beginner, Python or JavaScript?"), vec!["Python", "JavaScript"]);
        assert_eq!(options_from_question("compare Netflix, Hulu and Disney+ for families"), vec!["Netflix", "Hulu", "Disney+"]);
        assert_eq!(options_from_question("Rust vs Go vs Zig"), vec!["Rust", "Go", "Zig"]);
        assert!(options_from_question("What's the best laptop for college?").is_empty());
        // The model echoing the prompt isn't an option.
        assert!(parse_setup(r#"{"options":["2 to 5","short names"],"criteria":[]}"#).is_none());
    }

    #[test]
    fn scores_are_matched_clamped_and_checked() {
        let options = vec!["MacBook Air M4".to_string(), "Dell XPS 13".to_string()];
        let criteria = vec![crit("Battery life", 5), crit("Price", 2)];
        let reply = r#"{"scores":[
            {"option":"MacBook Air M4","criterion":"Battery life","score":9,"reason":"18 hours","sources":[1,9]},
            {"option":"Dell XPS","criterion":"battery","score":14,"reason":"12 hours","sources":[2]},
            {"option":"Dell XPS 13","criterion":"Price","score":0,"reason":"cheaper","sources":[]},
            {"option":"Framework","criterion":"Price","score":5,"reason":"x","sources":[]}
        ]}"#;
        let g = parse_scores(reply, &options, &criteria, &[1, 2, 3]);
        assert_eq!(g[0][0].as_ref().unwrap().sources, vec![1]);
        assert_eq!(g[1][0].as_ref().unwrap().score, 10);
        assert_eq!(g[1][1].as_ref().unwrap().score, 1);
        assert!(g[0][1].is_none());
    }

    #[test]
    fn totals_are_weighted() {
        let cell = |s| Some(Cell { score: s, reason: String::new(), sources: vec![] });
        let d = Decision { options: vec!["A".into(), "B".into()], criteria: vec![crit("x", 3), crit("y", 1)], scores: vec![vec![cell(8), cell(4)], vec![cell(6), None]] };
        let t = d.totals();
        assert!((t[0] - 7.0).abs() < 1e-9);
        assert!((t[1] - 6.0).abs() < 1e-9);
        let text = d.as_text();
        assert!(text.contains("| x (3) | 8/10 | 6/10 |"), "{text}");
        assert!(text.contains("| **Weighted total** | 7.0 | 6.0 |"));
        // The UI gets camelCase JSON.
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["scores"][1][1], Value::Null);
        assert_eq!(v["criteria"][0]["weight"], 3);
    }

    /// Real engine + real internet. Needs BYTE_TEST_LLAMA_SERVER, BYTE_TEST_MODEL and BYTE_TEST_WEB=1.
    #[tokio::test]
    #[ignore]
    async fn e2e_compare() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let Ok(model) = std::env::var("BYTE_TEST_MODEL") else { return };
        let Some((_server, mut ep)) = crate::chat::e2e_support::start_server_with(&model, &["-c".into(), "16384".into()], None).await else { return };
        ep.context = 16384;
        let dir = tempfile::tempdir().unwrap();
        let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = crate::tools::fetch::web_client();
        let q = "MacBook Air M4 vs Dell XPS 13 for a college student?";
        let history = vec![chat::ChatMessage::new("user", q)];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, true, None);
        let plan = crate::router::plan_turn(Mode::Auto, crate::settings::ThinkingPref::Off, q);
        let (ch, seen) = crate::chat::e2e_support::collecting_channel();
        let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: true, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false };
        crate::agent::run(turn, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        for e in ev.iter().filter(|e| e["kind"] == "toolCall" || e["kind"] == "toolResult" || e["kind"] == "decision") {
            eprintln!("{e}");
        }
        let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
        eprintln!("{content}");
        let d = ev.iter().find(|e| e["kind"] == "decision").expect("no decision table");
        assert!(d["options"].as_array().unwrap().len() >= 2);
        assert!(!content.trim().is_empty());
    }
}
