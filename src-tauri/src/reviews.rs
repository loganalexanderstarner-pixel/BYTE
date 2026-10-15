//! Review summarizer: "reviews of the Sony WH-1000XM6", "is the Kindle
//! Colorsoft worth it", "Framework 13 pros and cons". BYTE reads review
//! sites and owner discussions, collects the star ratings pages publish
//! (schema.org AggregateRating), and has the model sort what reviewers say
//! into pros and cons with how many sources mention each.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::{self, ChatEvent};
use crate::error::AppResult;
use crate::research::{self, Ctx, Emit, Gathered};
use crate::tools::{self, SourceBook};

/// A star rating a site publishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rating {
    pub site: String,
    pub value: f32,
    /// The top of the scale (usually 5 or 10).
    pub best: f32,
    /// How many ratings (None: one editor's review).
    pub count: Option<u32>,
    pub n: u32,
}

/// A pro or con and the sources that say it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub text: String,
    pub sources: Vec<u32>,
}

/// The reviews card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reviews {
    pub product: String,
    pub verdict: String,
    pub ratings: Vec<Rating>,
    pub pros: Vec<Point>,
    pub cons: Vec<Point>,
    pub best_for: Vec<String>,
    pub skip_if: Vec<String>,
    /// Pages read.
    pub read: u32,
}

const CUES: &[&str] = &[
    "what do people say about", "what do people think of", "what do owners say about", "reviews of", "review of", "reviews for", "reviews on",
    "pros and cons of", "pros and cons", "worth buying", "worth the money", "worth it", "any good", "complaints about", "problems with",
    "reviews", "review",
];

/// "Review my essay", "code review": asks BYTE to review something, not for reviews.
const NOT_REVIEWS: &[&str] = &["review my", "review this", "review the following", "review it", "review our", "peer review", "code review", "performance review", "review your", "literature review", "review these",
    // "Is it worth it to learn Rust": about doing something, not a product.
    "worth it to ", "worth it for me to", "worth learning", "worth doing", "worth going"];

/// Words around the product name that aren't part of it.
const FILLER: &[&str] = &[
    "what's", "whats", "what", "is", "are", "the", "a", "an", "place", "to", "buy", "get", "on", "of", "for", "right", "now", "online", "today",
    "me", "i", "should", "any", "there", "good", "do", "does", "people", "say", "about", "it", "its", "it's", "where", "can", "how", "much",
    "find", "tell", "show", "summarize", "summarise", "please", "give", "and", "latest", "honest", "real", "user", "owner", "customer", "at",
];

/// The thing a question is about, with the cue words and filler removed
/// ("What's the cheapest place to buy AirPods Pro 3?" → "AirPods Pro 3").
pub fn subject(question: &str, cues: &[&str]) -> String {
    let q = question.trim().trim_end_matches(['?', '.', '!']);
    let lower = q.to_ascii_lowercase();
    // Cut out every cue (longest first), keeping the pieces between them.
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let mut sorted: Vec<&&str> = cues.iter().collect();
    sorted.sort_by_key(|c| std::cmp::Reverse(c.len()));
    for c in sorted {
        let mut from = 0;
        while let Some(i) = lower[from..].find(*c).map(|i| i + from) {
            let end = i + c.len();
            let whole = (i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric()) && (end == lower.len() || !lower.as_bytes()[end].is_ascii_alphanumeric());
            if whole && !cuts.iter().any(|(a, b)| i < *b && end > *a) {
                cuts.push((i, end));
            }
            from = end;
        }
    }
    cuts.sort();
    let mut pieces = Vec::new();
    let mut at = 0;
    for (a, b) in cuts {
        pieces.push(&q[at..a]);
        at = b;
    }
    pieces.push(&q[at..]);
    let clean = |p: &str| -> String {
        let mut words: Vec<&str> = p.split_whitespace().collect();
        while words.first().is_some_and(|w| FILLER.contains(&w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').as_ref())) {
            words.remove(0);
        }
        // Trailing filler and "in the US"-style endings.
        loop {
            let n = words.len();
            if n >= 3 && words[n - 3].eq_ignore_ascii_case("in") && words[n - 2].eq_ignore_ascii_case("the") {
                words.truncate(n - 3);
                continue;
            }
            if words.last().is_some_and(|w| FILLER.contains(&w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).as_ref())) {
                words.pop();
                continue;
            }
            break;
        }
        words.join(" ").trim_matches([',', ':', ';', '"', '\'']).to_string()
    };
    pieces.into_iter().map(clean).max_by_key(|s| s.len()).unwrap_or_default()
}

/// Questions asking what reviewers and owners think of a product.
pub fn wants_reviews(question: &str) -> bool {
    let q = question.to_lowercase();
    if NOT_REVIEWS.iter().any(|n| q.contains(n)) || !CUES.iter().any(|c| q.contains(c)) {
        return false;
    }
    // "X vs Y reviews" is a comparison.
    if crate::decide::options_from_question(question).len() >= 2 {
        return false;
    }
    let s = subject(question, CUES);
    !s.is_empty() && s.split_whitespace().count() <= 8
}

pub fn applies(enabled: bool, web: bool, question: &str) -> bool {
    enabled && web && wants_reviews(question)
}

fn num(v: &Value) -> Option<f32> {
    match v {
        Value::Number(n) => n.as_f64().map(|x| x as f32),
        Value::String(s) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    }
}

/// The star rating a page publishes: (value, best, count).
pub fn rating_on_page(html: &str) -> Option<(f32, f32, Option<u32>)> {
    let lds = tools::fetch::json_ld(html);
    let agg = lds.iter().find_map(|v| {
        let r = if tools::fetch::ld_is(v, "AggregateRating") { v } else { &v["aggregateRating"] };
        let value = num(&r["ratingValue"])?;
        let best = num(&r["bestRating"]).unwrap_or(5.0);
        let count = num(&r["ratingCount"]).or_else(|| num(&r["reviewCount"])).map(|c| c as u32);
        Some((value, best, count))
    });
    // An editor's review: one rating.
    let single = || {
        lds.iter().filter(|v| tools::fetch::ld_is(v, "Review")).find_map(|v| {
            let r = &v["reviewRating"];
            Some((num(&r["ratingValue"])?, num(&r["bestRating"]).unwrap_or(5.0), None))
        })
    };
    agg.or_else(single).filter(|(v, b, _)| *b > 0.0 && *v > 0.0 && v <= b)
}

/// Review texts a page publishes as JSON-LD (shoppers' reviews on store pages).
pub fn review_texts(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for v in tools::fetch::json_ld(html) {
        let reviews = match &v["review"] {
            Value::Array(a) => a.clone(),
            o @ Value::Object(_) => vec![o.clone()],
            _ if tools::fetch::ld_is(&v, "Review") => vec![v.clone()],
            _ => vec![],
        };
        for r in reviews {
            if let Some(t) = r["reviewBody"].as_str().map(str::trim).filter(|t| t.len() > 20) {
                let stars = num(&r["reviewRating"]["ratingValue"]).map(|s| format!("({s}★) ")).unwrap_or_default();
                out.push(format!("{stars}{}", t.chars().take(600).collect::<String>()));
            }
        }
    }
    out.truncate(20);
    out
}

fn schema() -> Value {
    let points = json!({ "type": "array", "maxItems": 6, "items": { "type": "object", "properties": { "point": { "type": "string" }, "sources": { "type": "array", "items": { "type": "integer" } } }, "required": ["point", "sources"] } });
    json!({
        "type": "object",
        "properties": {
            "verdict": { "type": "string" },
            "pros": points,
            "cons": points,
            "best_for": { "type": "array", "maxItems": 3, "items": { "type": "string" } },
            "skip_if": { "type": "array", "maxItems": 3, "items": { "type": "string" } }
        },
        "required": ["verdict", "pros", "cons"]
    })
}

/// The model's summary, cleaned: known sources only, duplicates dropped,
/// the points most sources agree on first.
pub fn parse_reviews(reply: &str, valid: &[u32]) -> Option<(String, Vec<Point>, Vec<Point>, Vec<String>, Vec<String>)> {
    let v = research::lenient_json(reply);
    let verdict = v["verdict"].as_str().unwrap_or("").trim().to_string();
    let points = |key: &str| -> Vec<Point> {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<Point> = v[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let text = p["point"].as_str().or(p.as_str())?.trim().trim_end_matches('.').to_string();
                if text.len() < 3 || !seen.insert(text.to_lowercase()) {
                    return None;
                }
                let mut sources: Vec<u32> = p["sources"].as_array().into_iter().flatten().filter_map(|n| n.as_u64()).map(|n| n as u32).filter(|n| valid.contains(n)).collect();
                sources.sort_unstable();
                sources.dedup();
                Some(Point { text, sources })
            })
            .take(6)
            .collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.sources.len()));
        out
    };
    let list = |key: &str| -> Vec<String> { v[key].as_array().into_iter().flatten().filter_map(|s| s.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).take(3).collect() };
    let (pros, cons) = (points("pros"), points("cons"));
    if verdict.is_empty() && pros.is_empty() && cons.is_empty() {
        return None;
    }
    Some((verdict, pros, cons, list("best_for"), list("skip_if")))
}

pub const REVIEW_RULES: &str = "The user sees a reviews card above with the ratings, pros and cons. In your answer: \
give the overall verdict in a sentence or two, the biggest strength and the most common complaint, who it suits and \
who should skip it, citing sources as [n]. Say when reviewers disagree, and when complaints come from a few owners \
rather than most. Don't repeat every point from the card. Keep it short.";

pub async fn run(turn: &Turn<'_>, question: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let product = subject(question, CUES);
    if product.is_empty() {
        return Ok(None);
    }
    let c = Ctx { turn, cancel, send };
    let mut g = Gathered::default();
    let queries = vec![format!("{product} review"), format!("{product} reddit"), format!("{product} problems complaints"), format!("{product} long term review")];
    let lists = research::run_searches(&c, &mut g, &queries, "v").await?;
    let want = research::scale_pages(if matches!(turn.mode, crate::settings::Mode::Deep | crate::settings::Mode::Extended) { 12 } else { 7 }, turn.depth);
    let candidates = research::interleave(&lists, &[], want * 2);
    let pages = research::read_html_pages(&c, &mut g, &candidates, want, "v").await?;

    let mut ratings = Vec::new();
    for (n, url, html) in &pages {
        if let Some((value, best, count)) = rating_on_page(html) {
            ratings.push(Rating { site: tools::host_of(url), value, best, count, n: *n });
        }
        let extra = review_texts(html);
        if !extra.is_empty() {
            g.texts.push((*n, format!("Shopper reviews:\n{}", extra.join("\n"))));
        }
    }

    c.call("byte_vrank", "rank_passages", json!({}))?;
    let budget = research::notes_budget(turn, used_tokens, 0.4);
    let (picks, by_meaning) = c.cancellable(research::rank_texts(turn, &g.texts, question, &format!("{product} pros cons problems battery quality price"), budget, 3)).await?;
    let notes = research::format_notes(&g.book, &picks);
    c.result("byte_vrank", !picks.is_empty(), research::rank_summary(picks.len(), &notes, by_meaning))?;

    c.call("byte_vsum", "summarize_reviews", json!({ "product": product }))?;
    let user = format!(
        "Product: {product}\n\nWhat reviewers and owners wrote (numbered sources):\n{}\n\nSummarize the reviews: a one-sentence \
verdict; up to 6 pros and up to 6 cons, each a short phrase with the numbers of every source that says it; who it's best \
for; who should skip it. Only use what the sources say.",
        notes.chars().take(budget.min(12_000)).collect::<String>()
    );
    let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, "You summarize product reviews fairly. Reply only with JSON.", &user, schema(), 900)).await?.unwrap_or_default();
    let valid: Vec<u32> = g.book.sources.iter().map(|s| s.n).collect();
    let parsed = parse_reviews(&reply, &valid);
    c.result("byte_vsum", parsed.is_some(), if parsed.is_some() { format!("{} pages, {} ratings", pages.len(), ratings.len()) } else { "Couldn't summarize".into() })?;
    let mut card_text = String::new();
    if let Some((verdict, pros, cons, best_for, skip_if)) = parsed {
        let card = Reviews { product: product.clone(), verdict, ratings, pros, cons, best_for, skip_if, read: pages.len() as u32 };
        card_text = card_summary(&card);
        send(ChatEvent::Reviews(card))?;
    }
    Ok(Some((g.book, format!("Reviews of: {product}\n\n{card_text}\nNotes:\n{notes}\n\n{REVIEW_RULES}"))))
}

/// The card as text for the model.
fn card_summary(r: &Reviews) -> String {
    let cite = |s: &[u32]| s.iter().map(|n| format!("[{n}]")).collect::<String>();
    let mut out = format!("Verdict: {}\n", r.verdict);
    for x in &r.ratings {
        out.push_str(&format!("Rating on {} [{}]: {}/{}{}\n", x.site, x.n, x.value, x.best, x.count.map(|c| format!(" from {c} ratings")).unwrap_or_default()));
    }
    for p in &r.pros {
        out.push_str(&format!("Pro: {} {}\n", p.text, cite(&p.sources)));
    }
    for p in &r.cons {
        out.push_str(&format!("Con: {} {}\n", p.text, cite(&p.sources)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subject_is_found() {
        assert_eq!(subject("Reviews of the Sony WH-1000XM6?", CUES), "Sony WH-1000XM6");
        assert_eq!(subject("Is the Kindle Colorsoft worth it", CUES), "Kindle Colorsoft");
        assert_eq!(subject("Framework Laptop 13 pros and cons", CUES), "Framework Laptop 13");
        assert_eq!(subject("what do people say about the Rivian R2", CUES), "Rivian R2");
        assert_eq!(subject("Is the Roborock S8 any good?", CUES), "Roborock S8");
    }

    #[test]
    fn which_questions_want_reviews() {
        for yes in ["Reviews of the Sony WH-1000XM6", "Is the Kindle Colorsoft worth it?", "Framework Laptop 13 pros and cons", "any complaints about the Toyota RAV4 2025?"] {
            assert!(wants_reviews(yes), "{yes}");
        }
        for no in ["Review my essay please", "can you do a code review of this function", "write a literature review on sleep", "MacBook Air vs Dell XPS reviews", "reviews", "is it worth it to learn Rust or Go"] {
            assert!(!wants_reviews(no), "{no}");
        }
    }

    #[test]
    fn ratings_and_review_texts_come_from_pages() {
        let html = include_str!("../tests/fixtures/product_offer.html");
        assert_eq!(rating_on_page(html), Some((4.7, 5.0, Some(2143))));
        let texts = review_texts(html);
        assert_eq!(texts.len(), 2);
        assert!(texts[1].starts_with("(2★) Case scratches"));
        let editor = r#"<script type="application/ld+json">{"@type":"Review","reviewRating":{"@type":"Rating","ratingValue":8,"bestRating":10},"reviewBody":"A great pair of headphones with excellent noise cancelling."}</script>"#;
        assert_eq!(rating_on_page(editor), Some((8.0, 10.0, None)));
        assert_eq!(rating_on_page(r#"<script type="application/ld+json">{"@type":"Product","aggregateRating":{"ratingValue":"9","bestRating":"5"}}</script>"#), None);
    }

    #[test]
    fn summaries_are_cleaned() {
        let reply = r#"{"verdict":"Excellent noise cancelling, pricey.","pros":[{"point":"Great ANC","sources":[1,2,9]},{"point":"great anc","sources":[3]},{"point":"Comfortable","sources":[1,2,3]}],"cons":[{"point":"Expensive.","sources":[2]}],"best_for":["Commuters"],"skip_if":[]}"#;
        let (verdict, pros, cons, best, skip) = parse_reviews(reply, &[1, 2, 3]).unwrap();
        assert_eq!(verdict, "Excellent noise cancelling, pricey.");
        assert_eq!(pros, vec![Point { text: "Comfortable".into(), sources: vec![1, 2, 3] }, Point { text: "Great ANC".into(), sources: vec![1, 2] }]);
        assert_eq!(cons[0].text, "Expensive");
        assert_eq!(best, vec!["Commuters"]);
        assert!(skip.is_empty());
        assert!(parse_reviews("nonsense", &[1]).is_none());
    }
}
