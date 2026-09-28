//! Fact-check (Phase 6, v0.6.1). For "is it true that…", "fact-check this…"
//! or the Fact-check button on a message, BYTE:
//!
//! 1. picks out up to 5 checkable claims (a short JSON request),
//! 2. searches for each claim twice: once plainly and once for rebuttals
//!    ("… myth OR false OR debunked"), plus papers for research claims,
//! 3. reads a few pages per claim and keeps the best passages for that claim,
//! 4. asks the model for a verdict table (True … False, Unproven) that quotes
//!    the exact sentence settling each claim, and a confidence line.
//!
//! Shares its search, reading and ranking steps with `research.rs`.

use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat;
use crate::error::AppResult;
use crate::research::{self, Ctx, Emit, Gathered};
use crate::settings::Mode;
use crate::tools::SourceBook;

pub const FACT_RULES: &str = "Fact-check the claims above using only these notes. Start with a one-line \
**TL;DR:** blockquote giving the overall result. Then a table with the columns | Claim | Verdict | Evidence |, one \
row per claim. Verdict is exactly one of: True, Mostly true, Mixed, Mostly false, False, Unproven (Unproven when \
the notes don't settle it). In Evidence, quote the exact sentence from a source that settles the claim, in \
quotation marks, followed by its number like [3]; for a false claim, quote the source that contradicts it. After \
the table, add a short note per claim only where context matters (what's true about it, where sources disagree). \
Use only source numbers listed above and don't add a list of sources. End with one line exactly like \
`**Confidence:** Likely — <why, in one short sentence>` using Verified, Likely or Unsure.";

/// Whether this turn is a fact-check: asked for (`task`) or phrased as one.
pub fn applies(web: bool, forced: bool, question: &str) -> bool {
    web && (forced || crate::router::wants_fact_check(question))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Claim {
    pub claim: String,
    /// A search-engine query for it.
    #[serde(default)]
    pub search: String,
}

fn claims_schema(max: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "claims": { "type": "array", "minItems": 1, "maxItems": max, "items": {
                "type": "object",
                "properties": { "claim": { "type": "string" }, "search": { "type": "string" } },
                "required": ["claim", "search"]
            }}
        },
        "required": ["claims"]
    })
}

/// Claims from the model's reply: trimmed, non-trivial, no repeats, at most
/// `max`. Falls back to the question itself as the one claim.
pub fn parse_claims(reply: &str, question: &str, max: usize) -> Vec<Claim> {
    let v = research::lenient_json(reply);
    let mut out: Vec<Claim> = Vec::new();
    for c in v["claims"].as_array().into_iter().flatten() {
        let Ok(mut c) = serde_json::from_value::<Claim>(c.clone()) else { continue };
        c.claim = c.claim.trim().chars().take(300).collect();
        c.search = c.search.trim().trim_matches('"').chars().take(160).collect();
        if c.claim.split_whitespace().count() < 3 || out.iter().any(|o| research::similar(&o.claim, &c.claim)) {
            continue;
        }
        if c.search.split_whitespace().count() < 2 {
            c.search = crate::agent::search_query(&c.claim, None);
        }
        out.push(c);
        if out.len() >= max {
            break;
        }
    }
    if out.is_empty() {
        let q = strip_opener(question);
        out.push(Claim { search: crate::agent::search_query(&q, None), claim: q });
    }
    out
}

/// "Is it true that X?" → "X"; "Fact-check this: X" → "X".
pub fn strip_opener(question: &str) -> String {
    let q = question.trim();
    let lower = q.to_lowercase();
    for o in ["is it true that", "is it really true that", "fact-check this:", "fact check this:", "fact-check:", "fact check:", "fact-check", "fact check", "true or false:", "true or false", "is it a myth that", "debunk:"] {
        if lower.starts_with(o) {
            return q[o.len()..].trim_start_matches([':', ',', ' ']).trim_end_matches('?').trim().to_string();
        }
    }
    q.trim_end_matches('?').to_string()
}

/// A short, single-sentence question or statement.
pub fn is_single_claim(text: &str) -> bool {
    let body = strip_opener(text);
    body.len() <= 240 && body.trim_end_matches(['.', '!', '?']).matches(['.', '!', '?']).count() == 0
}

/// (claims at most, pages read per claim) by mode.
fn budget(mode: Mode) -> (usize, usize) {
    match mode {
        Mode::Fast => (2, 2),
        Mode::Auto => (3, 3),
        Mode::Deep => (5, 5),
        Mode::Extended => (5, 6),
    }
}

async fn extract(turn: &Turn<'_>, text: &str, max: usize) -> Vec<Claim> {
    // A short question ("is it true that X?") is one claim; asking a model to
    // split it only invites invented extra claims.
    if is_single_claim(text) {
        return parse_claims("", text, 1);
    }
    let user = format!(
        "Text to fact-check:\n{}\n\nList up to {max} separate factual claims in it that can be checked against sources \
(facts, numbers, events, cause and effect; not opinions). Write each as one plain sentence, and give a 3 to 8 word \
search-engine query for it. If the text is a single question like \"is it true that…\", its claim is that statement.",
        text.chars().take(6000).collect::<String>()
    );
    let reply = chat::complete_json(turn.http, turn.ep, "You find checkable claims. Reply only with JSON.", &user, claims_schema(max), 600).await.unwrap_or_default();
    parse_claims(&reply, text, max)
}

/// Runs the fact-check and returns the sources and the notes for the model.
pub async fn run(turn: &Turn<'_>, question: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<(SourceBook, String)> {
    let (max_claims, pages) = budget(turn.mode);
    let pages = research::scale_pages(pages, turn.depth);
    let c = Ctx { turn, cancel, send };
    let mut g = Gathered::default();

    c.call("byte_fclaims", "extract_claims", json!({}))?;
    let claims = c.cancellable(extract(turn, question, max_claims)).await?;
    c.result("byte_fclaims", true, format!("{} claim{} to check", claims.len(), if claims.len() == 1 { "" } else { "s" }))?;

    // Per claim: which gathered texts belong to it.
    let mut ranges = Vec::with_capacity(claims.len());
    for (i, cl) in claims.iter().enumerate() {
        let tag = format!("c{i}");
        let queries = vec![cl.search.clone(), format!("{} myth OR false OR debunked", cl.search)];
        let start = g.texts.len();
        let lists = research::run_searches(&c, &mut g, &queries, &tag).await?;
        if crate::router::wants_papers(&cl.claim) {
            research::find_papers(&c, &mut g, &cl.search, 4, &tag).await?;
        }
        let seen: Vec<String> = g.book.sources.iter().filter(|s| s.read).map(|s| s.url.clone()).collect();
        let candidates = research::interleave(&lists, &seen, pages * 2);
        research::read_pages(&c, &mut g, &candidates, pages, &tag).await?;
        ranges.push(start..g.texts.len());
    }

    c.call("byte_frank", "rank_passages", json!({}))?;
    let budget = research::notes_budget(turn, used_tokens, 0.5) / claims.len().max(1);
    let mut notes = String::new();
    let mut kept = 0;
    let mut by_meaning = false;
    for (i, (cl, range)) in claims.iter().zip(&ranges).enumerate() {
        // A claim whose own pages all failed uses everything read.
        let texts = if range.is_empty() { &g.texts[..] } else { &g.texts[range.clone()] };
        let (picks, meaning) = c.cancellable(research::rank_texts(turn, texts, &cl.claim, &cl.search, budget, 2)).await?;
        kept += picks.len();
        by_meaning |= meaning;
        notes.push_str(&format!("### Claim {}: {}\n\n{}\n", i + 1, cl.claim, research::format_notes(&g.book, &picks)));
    }
    c.result("byte_frank", kept > 0, research::rank_summary(kept, &notes, by_meaning))?;

    let content = if kept == 0 {
        "Nothing readable was found for these claims. Say they couldn't be checked, give what you know with that caveat, and suggest where to look.".to_string()
    } else {
        format!("Fact-check notes for: {}\n\n{notes}\n{FACT_RULES}", question.chars().take(500).collect::<String>())
    };
    Ok((g.book, content))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The verdicts the rules allow (the UI badges them: `markdown.ts` `markVerdicts`).
    const VERDICTS: &[&str] = &["True", "Mostly true", "Mixed", "Mostly false", "False", "Unproven"];

    #[test]
    fn claims_are_read_leniently() {
        let reply = r#"{"claims":[{"claim":"Humans only use 10% of their brains.","search":"10 percent brain myth"},{"claim":"Humans only use 10% of their brain.","search":"x"},{"claim":"ok","search":"a b"},{"claim":"Einstein failed math at school.","search":""}]}"#;
        let c = parse_claims(reply, "q", 5);
        assert_eq!(c.len(), 2, "{c:?}");
        assert_eq!(c[0].search, "10 percent brain myth");
        // A missing search is made from the claim.
        assert!(c[1].search.contains("Einstein"));
        assert_eq!(parse_claims(reply, "q", 1).len(), 1);
    }

    #[test]
    fn falls_back_to_the_question() {
        let c = parse_claims("sorry", "Is it true that goldfish have a 3-second memory?", 3);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].claim, "goldfish have a 3-second memory");
        assert_eq!(strip_opener("Fact-check this: The Great Wall is visible from space."), "The Great Wall is visible from space.");
    }

    #[test]
    fn short_questions_are_one_claim() {
        assert!(is_single_claim("Is it true that we only use 10% of our brains?"));
        assert!(!is_single_claim("Fact-check this: The Eiffel Tower is in Rome. It was built in 1889. It is 330 m tall."));
    }

    #[test]
    fn runs_only_with_the_web() {
        assert!(applies(true, false, "Is it true that we only use 10% of our brains?"));
        assert!(applies(true, true, "The moon landing was in 1969."));
        assert!(!applies(false, true, "The moon landing was in 1969."));
        assert!(!applies(true, false, "What's the weather tomorrow?"));
        assert!(budget(Mode::Deep).1 > budget(Mode::Auto).1);
    }

    /// Real engine + real internet. Needs BYTE_TEST_LLAMA_SERVER, BYTE_TEST_MODEL and BYTE_TEST_WEB=1.
    #[tokio::test]
    #[ignore]
    async fn e2e_fact_check() {
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
        let q = "Is it true that we only use 10% of our brains?";
        let history = vec![chat::ChatMessage::new("user", q)];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, true, None);
        let plan = crate::router::plan_turn(Mode::Auto, crate::settings::ThinkingPref::Off, q);
        let (ch, seen) = crate::chat::e2e_support::collecting_channel();
        let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: true, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0 };
        crate::agent::run(turn, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        for e in ev.iter().filter(|e| e["kind"] == "toolCall" || e["kind"] == "toolResult") {
            eprintln!("{e}");
        }
        let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
        eprintln!("{content}");
        assert!(ev.iter().any(|e| e["kind"] == "toolCall" && e["name"] == "extract_claims"));
        assert!(VERDICTS.iter().any(|v| content.contains(v)), "no verdict: {content}");
    }
}
