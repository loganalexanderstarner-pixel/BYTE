//! Deep and Extended research (Phase 6). Instead of one search and a few
//! pages, BYTE:
//!
//! 1. plans 3–6 searches that cover the question from different angles
//!    (a short JSON request to the loaded model),
//! 2. runs them all, plus a paper search when published research helps,
//! 3. reads 12 (Deep) or 24 (Extended) pages, 6 at a time, alternating
//!    between the searches so no single one dominates,
//! 4. keeps the most relevant passages: by meaning when the search-by-meaning
//!    model is downloaded (the knowledge base's), else by matching words; at
//!    most 3 per source, within about half the context,
//! 5. Extended only: asks the model what's still missing and runs those
//!    searches too,
//! 6. hands the numbered notes to the model with instructions for a
//!    sectioned, cited report that ends with a confidence line.
//!
//! Every step shows in the answer's activity list. A failed step never fails
//! the answer: BYTE continues with what it has.

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::{self, ChatEvent};
use crate::error::{AppError, AppResult};
use crate::settings::Mode;
use crate::tools::{self, academic, search::SearchResult, SourceBook};

/// How far research goes in each mode (None: no research pipeline).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Depth {
    /// Searches planned up front (the question itself counts as one).
    pub searches: usize,
    /// Pages read in total.
    pub pages: usize,
    /// Papers asked for when the question calls for research.
    pub papers: usize,
    /// Rounds of "what's missing?" follow-up searches.
    pub gap_rounds: usize,
}

pub fn depth(mode: Mode) -> Option<Depth> {
    depth_at(mode, 0)
}

/// Research depth for a mode at the Settings level (0 Normal, 1 More, 2 Max).
pub fn depth_at(mode: Mode, level: u8) -> Option<Depth> {
    let l = level.min(2) as usize;
    match mode {
        Mode::Deep => Some(Depth { searches: 4 + l, pages: [12, 20, 32][l], papers: [6, 8, 10][l], gap_rounds: 0 }),
        Mode::Extended => Some(Depth { searches: 6 + l, pages: [24, 32, 48][l], papers: [8, 10, 10][l], gap_rounds: 1 + usize::from(l == 2) }),
        Mode::Fast | Mode::Auto => None,
    }
}

/// Pages read at once, by depth level.
pub fn batch(level: u8) -> usize {
    [6, 8, 10][level.min(2) as usize]
}

/// Scales another pipeline's page count (fact-check, compare, trip) by the depth level.
pub fn scale_pages(pages: usize, level: u8) -> usize {
    match level {
        0 => pages,
        1 => pages * 3 / 2,
        _ => pages * 2,
    }
}
/// Most passages kept from one source, so one long page can't fill the notes.
const PER_SOURCE: usize = 3;
/// Passage size when splitting pages.
const PASSAGE_CHARS: usize = 700;
/// Share of the context the notes may use.
const NOTES_SHARE: f64 = 0.5;

/// Instructions that follow the notes.
pub const REPORT_RULES: &str = "Write your answer from these research notes. \
Structure it as a report: start with a one-line **TL;DR:** blockquote, then `##` sections that cover the \
question's angles, with bullets or a table where they help. Cite every fact with its source number, like [3] \
or [2][5], right after the claim; use only numbers listed above. Where sources disagree, say so and cite both. \
Papers are known only by their abstracts: say what kind of study it was when that's clear. Don't add a list of sources or links at the end; BYTE shows them. Don't pad: if the \
notes don't cover something, say it's unclear. End with one line exactly like \
`**Confidence:** Verified — <why, in one short sentence>` using Verified (several independent sources agree), Likely (one good \
source, or most agree) or Unsure (thin or conflicting sources), and a short reason.";

pub(crate) type Emit<'a> = &'a (dyn Fn(ChatEvent) -> AppResult<()> + Sync);

/// Whether this question gets the research pipeline in `mode`.
pub fn applies(mode: Mode, web: bool, always: bool, question: &str) -> bool {
    web && depth(mode).is_some()
        && !crate::router::wants_files(question)
        && (crate::router::wants_web_in(question, always) || crate::router::wants_papers(question))
}

// ---------- planning ----------

fn plan_schema(max: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "searches": { "type": "array", "minItems": 2, "maxItems": max, "items": { "type": "string" } },
            "papers": { "type": "boolean" }
        },
        "required": ["searches", "papers"]
    })
}

/// JSON from a model reply that may wrap it in prose or a code fence;
/// `Null` when there's none.
pub(crate) fn lenient_json(reply: &str) -> Value {
    serde_json::from_str(reply.trim()).unwrap_or_else(|_| match (reply.find('{'), reply.rfind('}')) {
        (Some(a), Some(b)) if b > a => serde_json::from_str(&reply[a..=b]).unwrap_or(Value::Null),
        _ => Value::Null,
    })
}

/// Searches from the model's reply (lenient: bad JSON gives nothing),
/// cleaned, without near-duplicates of `first`, at most `max` in total.
pub fn parse_plan(reply: &str, first: &str, max: usize) -> (Vec<String>, bool) {
    let v = lenient_json(reply);
    let mut out = vec![first.to_string()];
    for q in v["searches"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let q: String = q.trim().trim_matches('"').chars().take(160).collect();
        if q.split_whitespace().count() < 2 || out.iter().any(|p| similar(p, &q)) {
            continue;
        }
        out.push(q);
        if out.len() >= max {
            break;
        }
    }
    (out, v["papers"].as_bool().unwrap_or(false))
}

/// Two searches that would return the same results.
pub(crate) fn similar(a: &str, b: &str) -> bool {
    let words = |s: &str| -> std::collections::BTreeSet<String> {
        s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(str::to_string).collect()
    };
    let (a, b) = (words(a), words(b));
    let common = a.intersection(&b).count();
    common * 4 > a.len().max(b.len()) * 3
}

async fn plan(turn: &Turn<'_>, question: &str, first: &str, max: usize) -> (Vec<String>, bool) {
    let today = chrono::Local::now().format("%B %-d, %Y");
    let user = format!(
        "Today is {today}. Question:\n{question}\n\nPlan web research for a thorough, accurate answer. Write {max} short \
search-engine queries (3 to 8 words each) that together cover it from different angles: the main facts, recent \
developments, numbers or comparisons, expert views and criticism. Don't repeat the question in other words. \
Set papers to true if published scientific or medical research would help."
    );
    let system = "You plan web research. Reply only with JSON.";
    match chat::complete_json(turn.http, turn.ep, system, &user, plan_schema(max), 400).await {
        Ok(reply) => parse_plan(&reply, first, max),
        Err(e) => {
            log::warn!("research plan failed: {e}");
            (vec![first.to_string()], false)
        }
    }
}

fn gap_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "missing": { "type": "array", "maxItems": 3, "items": { "type": "string" } } },
        "required": ["missing"]
    })
}

/// Follow-up searches for what the notes don't cover yet (Extended).
async fn gaps(turn: &Turn<'_>, question: &str, notes: &str, done: &[String]) -> Vec<String> {
    let user = format!(
        "Question:\n{question}\n\nSearches done: {}\n\nNotes so far (shortened):\n{}\n\nWhat important part of the question \
do these notes not answer yet? Reply with up to 3 new search queries (3 to 8 words each) that would fill those gaps, \
or an empty list if the notes already cover it.",
        done.join("; "),
        notes.chars().take(5000).collect::<String>()
    );
    let reply = chat::complete_json(turn.http, turn.ep, "You review research notes. Reply only with JSON.", &user, gap_schema(), 300).await.unwrap_or_default();
    let v = lenient_json(&reply);
    v["missing"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|q| q.trim().chars().take(160).collect::<String>())
        .filter(|q| q.split_whitespace().count() >= 2 && !done.iter().any(|d| similar(d, q)))
        .take(3)
        .collect()
}

// ---------- reading ----------

/// Search results, taken in turns from each search (first result of every
/// search, then the second, …), readable and not seen before.
pub fn interleave(lists: &[Vec<SearchResult>], skip: &[String], want: usize) -> Vec<SearchResult> {
    let key = |u: &str| u.trim_end_matches('/').to_lowercase();
    let mut seen: Vec<String> = skip.iter().map(|u| key(u)).collect();
    let mut out = Vec::new();
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    'outer: for i in 0..longest {
        for l in lists {
            let Some(r) = l.get(i) else { continue };
            let k = key(&r.url);
            if seen.contains(&k) || !tools::fetch::worth_reading(&r.url) {
                continue;
            }
            seen.push(k);
            out.push(r.clone());
            if out.len() >= want {
                break 'outer;
            }
        }
    }
    out
}

/// Everything gathered so far: numbered sources and the text read from each.
#[derive(Default)]
pub struct Gathered {
    pub book: SourceBook,
    /// (source number, text): pages read and paper abstracts.
    pub texts: Vec<(u32, String)>,
    pub searches: Vec<String>,
}

/// What the research steps share: the turn, cancellation and the event sink.
pub(crate) struct Ctx<'a, 'b> {
    pub turn: &'a Turn<'b>,
    pub cancel: &'a CancellationToken,
    pub send: Emit<'a>,
}

impl Ctx<'_, '_> {
    pub fn call(&self, id: &str, name: &str, args: Value) -> AppResult<()> {
        (self.send)(ChatEvent::ToolCall { id: id.into(), name: name.into(), args })
    }

    pub fn result(&self, id: &str, ok: bool, summary: impl Into<String>) -> AppResult<()> {
        (self.send)(ChatEvent::ToolResult { id: id.into(), ok, summary: summary.into() })
    }

    pub async fn cancellable<T>(&self, f: impl std::future::Future<Output = T>) -> AppResult<T> {
        tokio::select! {
            v = f => Ok(v),
            _ = self.cancel.cancelled() => Err(AppError::Cancelled),
        }
    }
}

/// Runs searches (in parallel; the search module spaces them out) and
/// returns each one's results, adding them to the sources.
pub(crate) async fn run_searches(c: &Ctx<'_, '_>, g: &mut Gathered, queries: &[String], tag: &str) -> AppResult<Vec<Vec<SearchResult>>> {
    let ids: Vec<String> = (0..queries.len()).map(|i| format!("byte_rsearch_{tag}_{i}")).collect();
    for (id, q) in ids.iter().zip(queries) {
        c.call(id, tools::WEB_SEARCH, json!({ "query": q }))?;
    }
    let turn = c.turn;
    let found = c
        .cancellable(futures_util::future::join_all(queries.iter().map(|q| tools::search::search(turn.net, turn.cloud, q, 8))))
        .await?;
    let mut lists = Vec::new();
    for ((id, q), r) in ids.iter().zip(queries).zip(found) {
        let args = json!({ "query": q });
        match r {
            Ok(s) => {
                for x in &s.results {
                    g.book.add(&x.title, &x.url, &x.snippet);
                }
                let summary = format!("{} results", s.results.len());
                turn.log.record(tools::WEB_SEARCH, &args, true, &summary);
                c.result(id, true, summary)?;
                lists.push(s.results);
            }
            Err(e) => {
                turn.log.record(tools::WEB_SEARCH, &args, false, &e.to_string());
                c.result(id, false, e.to_string())?;
            }
        }
        g.searches.push(q.clone());
    }
    Ok(lists)
}

/// Reads up to `want` of `candidates`, `BATCH` at a time, trying the next
/// candidate for each page that can't be read.
pub(crate) async fn read_pages(c: &Ctx<'_, '_>, g: &mut Gathered, candidates: &[SearchResult], want: usize, tag: &str) -> AppResult<usize> {
    let mut read = 0;
    let mut rest = candidates;
    let mut batch_no = 0;
    while read < want && !rest.is_empty() {
        let take = (want - read).min(batch(c.turn.depth)).min(rest.len());
        let (batch, tail) = rest.split_at(take);
        rest = tail;
        let ids: Vec<String> = (0..batch.len()).map(|i| format!("byte_rread_{tag}_{batch_no}_{i}")).collect();
        batch_no += 1;
        for (id, r) in ids.iter().zip(batch) {
            c.call(id, tools::READ_PAGE, json!({ "url": r.url }))?;
        }
        let net = c.turn.net;
        let pages = c.cancellable(futures_util::future::join_all(batch.iter().map(|r| tools::fetch::fetch_page(net, &r.url)))).await?;
        for ((id, r), page) in ids.iter().zip(batch).zip(pages) {
            let args = json!({ "url": r.url });
            let (ok, summary) = match page {
                Ok(p) if p.text.trim().len() >= 200 => {
                    let n = g.book.mark_read(&r.url, if p.title.is_empty() { &r.title } else { &p.title });
                    g.texts.push((n, p.text));
                    read += 1;
                    (true, p.title.chars().take(60).collect::<String>())
                }
                Ok(_) => (false, "almost no text".to_string()),
                Err(e) => (false, e.to_string()),
            };
            c.turn.log.record(tools::READ_PAGE, &args, ok, &summary);
            c.result(id, ok, summary)?;
        }
    }
    if read > 0 {
        (c.send)(ChatEvent::Sources { sources: g.book.sources.clone() })?;
    }
    Ok(read)
}

/// Like `read_pages`, but keeps each page's HTML too (for its JSON-LD data):
/// returns (source number, page URL, HTML) for every page read.
pub(crate) async fn read_html_pages(c: &Ctx<'_, '_>, g: &mut Gathered, candidates: &[SearchResult], want: usize, tag: &str) -> AppResult<Vec<(u32, String, String)>> {
    let mut out = Vec::new();
    let mut rest = candidates;
    let mut batch_no = 0;
    while out.len() < want && !rest.is_empty() {
        let take = (want - out.len()).min(batch(c.turn.depth)).min(rest.len());
        let (batch, tail) = rest.split_at(take);
        rest = tail;
        let ids: Vec<String> = (0..batch.len()).map(|i| format!("byte_rhtml_{tag}_{batch_no}_{i}")).collect();
        batch_no += 1;
        for (id, r) in ids.iter().zip(batch) {
            c.call(id, tools::READ_PAGE, json!({ "url": r.url }))?;
        }
        let net = c.turn.net;
        let pages = c.cancellable(futures_util::future::join_all(batch.iter().map(|r| tools::fetch::fetch_html(net, &r.url)))).await?;
        for ((id, r), page) in ids.iter().zip(batch).zip(pages) {
            let args = json!({ "url": r.url });
            let (ok, summary) = match page {
                Ok((p, html)) if p.text.trim().len() >= 200 || !html.is_empty() => {
                    let n = g.book.mark_read(&r.url, if p.title.is_empty() { &r.title } else { &p.title });
                    g.texts.push((n, p.text));
                    out.push((n, p.url.clone(), html));
                    (true, p.title.chars().take(60).collect::<String>())
                }
                Ok(_) => (false, "almost no text".to_string()),
                Err(e) => (false, e.to_string()),
            };
            c.turn.log.record(tools::READ_PAGE, &args, ok, &summary);
            c.result(id, ok, summary)?;
        }
    }
    if !out.is_empty() {
        (c.send)(ChatEvent::Sources { sources: g.book.sources.clone() })?;
    }
    Ok(out)
}

pub(crate) async fn find_papers(c: &Ctx<'_, '_>, g: &mut Gathered, query: &str, max: usize, tag: &str) -> AppResult<()> {
    let id = &format!("byte_rpapers_{tag}");
    let args = json!({ "query": query });
    c.call(id, tools::ACADEMIC_SEARCH, args.clone())?;
    match c.cancellable(academic::search(c.turn.net, query, max)).await? {
        Ok(papers) => {
            for p in &papers {
                let n = g.book.add_paper(p);
                if !p.abstract_text.is_empty() {
                    let head = describe_paper(p);
                    g.texts.push((n, format!("{head}\n{}", p.abstract_text)));
                }
            }
            let summary = if papers.is_empty() { "No papers found".to_string() } else { format!("{} papers", papers.len()) };
            c.turn.log.record(tools::ACADEMIC_SEARCH, &args, true, &summary);
            c.result(id, true, summary)?;
            if !papers.is_empty() {
                (c.send)(ChatEvent::Sources { sources: g.book.sources.clone() })?;
            }
        }
        Err(e) => {
            c.turn.log.record(tools::ACADEMIC_SEARCH, &args, false, &e.to_string());
            c.result(id, false, e.to_string())?;
        }
    }
    Ok(())
}

fn describe_paper(p: &academic::Paper) -> String {
    let who = match p.authors.len() {
        0 => String::new(),
        1 => format!(" by {}", p.authors[0]),
        _ => format!(" by {} et al.", p.authors[0]),
    };
    let when = p.year.map(|y| format!(", {y}")).unwrap_or_default();
    let venue = if p.venue.is_empty() { String::new() } else { format!(" ({})", p.venue) };
    format!("Paper{who}{when}{venue}. Abstract:")
}

// ---------- ranking ----------

/// Splits a page into passages of about `PASSAGE_CHARS`, on line breaks.
pub fn passages(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if cur.len() + line.len() > PASSAGE_CHARS && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        // A single very long line is cut on sentence ends.
        if line.len() > PASSAGE_CHARS * 2 {
            for sentence in line.split_inclusive(". ") {
                if cur.len() + sentence.len() > PASSAGE_CHARS && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur.push_str(sentence);
            }
            continue;
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

const STOP: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "any", "can", "was", "our", "has", "have", "what", "when",
    "where", "which", "who", "why", "how", "with", "this", "that", "from", "they", "will", "would", "there", "their",
    "about", "into", "than", "then", "them", "these", "those", "does", "did", "is", "it", "of", "to", "in", "on", "a",
];

fn terms(text: &str) -> Vec<String> {
    let mut t: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !STOP.contains(w))
        .map(str::to_string)
        .collect();
    t.sort();
    t.dedup();
    t
}

/// Word-match score of a passage against the question's terms (and, with
/// less weight, the planned searches' terms).
pub fn lexical_score(passage: &str, main: &[String], extra: &[String]) -> f64 {
    let lower = passage.to_lowercase();
    let count = |ts: &[String]| -> (f64, f64) {
        let hits: usize = ts.iter().map(|t| lower.matches(t.as_str()).count().min(3)).sum();
        let distinct = ts.iter().filter(|t| lower.contains(t.as_str())).count();
        (hits as f64, distinct as f64)
    };
    let (h1, d1) = count(main);
    let (h2, d2) = count(extra);
    let digits = if passage.chars().any(|c| c.is_ascii_digit()) { 0.5 } else { 0.0 };
    // Short fragments (menus, captions) rarely carry an answer.
    let short = if passage.len() < 150 { 0.5 } else { 1.0 };
    (h1 + 3.0 * d1 + 0.3 * h2 + d2 + digits) * short
}

/// A passage picked for the notes.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub n: u32,
    pub order: usize,
    pub text: String,
}

/// Picks the best passages within `budget` characters, at most
/// `PER_SOURCE` per source. `ranked` is best first.
#[cfg(test)]
pub fn pick(ranked: &[(u32, usize, String)], budget: usize) -> Vec<Pick> {
    pick_n(ranked, budget, PER_SOURCE)
}

/// `pick` with a different cap per source.
pub fn pick_n(ranked: &[(u32, usize, String)], budget: usize, per_source: usize) -> Vec<Pick> {
    let mut per: std::collections::HashMap<u32, usize> = Default::default();
    let mut used = 0;
    let mut out = Vec::new();
    for (n, order, text) in ranked {
        let c = per.entry(*n).or_default();
        if *c >= per_source || used + text.len() > budget {
            continue;
        }
        *c += 1;
        used += text.len();
        out.push(Pick { n: *n, order: *order, text: text.clone() });
    }
    out
}

/// The notes as the model sees them: grouped by source (in source order),
/// passages in page order.
pub fn format_notes(book: &SourceBook, picks: &[Pick]) -> String {
    let mut ns: Vec<u32> = picks.iter().map(|p| p.n).collect();
    ns.sort_unstable();
    ns.dedup();
    let mut out = String::new();
    for n in ns {
        let Some(s) = book.sources.iter().find(|s| s.n == n) else { continue };
        let mut mine: Vec<&Pick> = picks.iter().filter(|p| p.n == n).collect();
        mine.sort_by_key(|p| p.order);
        out.push_str(&format!("[{n}] {}\n{}\n", s.title, s.url));
        for (i, p) in mine.iter().enumerate() {
            if i > 0 {
                out.push_str("…\n");
            }
            out.push_str(p.text.trim());
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// Ranks all passages and returns the notes and how many passages were used.
async fn rank(turn: &Turn<'_>, g: &Gathered, question: &str, budget: usize) -> (String, usize, bool) {
    let (picks, by_meaning) = rank_texts(turn, &g.texts, question, &g.searches.join(" "), budget, PER_SOURCE).await;
    (format_notes(&g.book, &picks), picks.len(), by_meaning)
}

/// Picks the best passages of `texts` for `question` (word matches, plus
/// meaning when the embedding model is there), within `budget` characters
/// and `per_source` passages per source. Also says whether meaning was used.
pub(crate) async fn rank_texts(turn: &Turn<'_>, texts: &[(u32, String)], question: &str, extra_terms: &str, budget: usize, per_source: usize) -> (Vec<Pick>, bool) {
    let main = terms(question);
    let extra = terms(extra_terms);
    let mut all: Vec<(u32, usize, String, f64)> = Vec::new();
    for (n, text) in texts {
        for (i, p) in passages(text).into_iter().enumerate() {
            let s = lexical_score(&p, &main, &extra);
            all.push((*n, i, p, s));
        }
    }
    all.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    // By meaning, when the embedding model is downloaded: the top word
    // matches are re-ranked by reciprocal-rank fusion of both orders.
    let mut by_meaning = false;
    if let Some(app) = turn.app {
        all.truncate(160);
        if let Some(order) = meaning_order(app, question, &all).await {
            let mut fused: Vec<(f64, usize)> = (0..all.len()).map(|i| (1.0 / (60.0 + i as f64), i)).collect();
            for (rank, &i) in order.iter().enumerate() {
                fused[i].0 += 1.0 / (60.0 + rank as f64);
            }
            fused.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            all = fused.into_iter().map(|(_, i)| all[i].clone()).collect();
            by_meaning = true;
        }
    }
    let ranked: Vec<(u32, usize, String)> = all.into_iter().map(|(n, i, p, _)| (n, i, p)).collect();
    (pick_n(&ranked, budget, per_source), by_meaning)
}

/// Characters of notes that fit: `share` of the context minus what the
/// conversation already uses.
pub(crate) fn notes_budget(turn: &Turn<'_>, used_tokens: usize, share: f64) -> usize {
    ((turn.ep.context as f64 * share) as usize).saturating_sub(used_tokens).max(1000) * 3
}

/// Passage indexes ordered by similarity of meaning to the question, or
/// None when the embedding model isn't downloaded (or fails).
async fn meaning_order(app: &tauri::AppHandle, question: &str, all: &[(u32, usize, String, f64)]) -> Option<Vec<usize>> {
    use tauri::Manager;
    let state = app.try_state::<crate::state::AppState>()?;
    let catalog = state.catalog.get();
    if !crate::embed::Embedder::installed(&catalog, &state.paths.models) {
        return None;
    }
    let q = state.embedder.embed(app, &state.paths.models, &catalog, &[question.to_string()], crate::embed::Purpose::Query).await.ok()?;
    let texts: Vec<String> = all.iter().map(|(_, _, p, _)| p.chars().take(1500).collect()).collect();
    let v = state.embedder.embed(app, &state.paths.models, &catalog, &texts, crate::embed::Purpose::Document).await.ok()?;
    let mut order: Vec<(f32, usize)> = v.iter().enumerate().map(|(i, e)| (crate::embed::cosine(&q[0], e), i)).collect();
    order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Some(order.into_iter().map(|(_, i)| i).collect())
}

// ---------- the pipeline ----------

/// Runs the research and returns the gathered sources and the notes for the
/// model (with `REPORT_RULES`). `used_tokens` is what the conversation
/// already takes, so the notes leave room for the answer.
pub async fn run(turn: &Turn<'_>, question: &str, first_query: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<(SourceBook, String)> {
    let d = depth_at(turn.mode, turn.depth).unwrap_or(Depth { searches: 3, pages: 8, papers: 5, gap_rounds: 0 });
    let c = Ctx { turn, cancel, send };
    let mut g = Gathered::default();

    // 1. Plan, while the user's own search already runs (it's always the first one).
    c.call("byte_rplan", "plan_research", json!({ "question": question.chars().take(200).collect::<String>() }))?;
    let first = vec![first_query.to_string()];
    let mut first_g = Gathered::default();
    let (planned, first_lists) = tokio::join!(c.cancellable(plan(turn, question, first_query, d.searches)), run_searches(&c, &mut first_g, &first, "0"));
    let (queries, model_wants_papers) = planned?;
    let mut lists = first_lists?;
    for s in &first_g.book.sources {
        g.book.add(&s.title, &s.url, &s.snippet);
    }
    g.searches.extend(first_g.searches);
    let papers = model_wants_papers || crate::router::wants_papers(question);
    c.result("byte_rplan", true, format!("{} searches{}", queries.len(), if papers { " + papers" } else { "" }))?;

    // 2. The other searches (and papers).
    lists.extend(run_searches(&c, &mut g, &queries[1..], "1").await?);
    if papers {
        find_papers(&c, &mut g, first_query, d.papers, "0").await?;
    }
    if !g.book.sources.is_empty() {
        send(ChatEvent::Sources { sources: g.book.sources.clone() })?;
    }

    // 3. Read.
    let candidates = interleave(&lists, &[], d.pages * 2);
    read_pages(&c, &mut g, &candidates, d.pages, "0").await?;

    // Budget for the notes: half the context minus what's used, in characters.
    let budget = notes_budget(turn, used_tokens, NOTES_SHARE);

    // 4. Rank.
    c.call("byte_rrank_0", "rank_passages", json!({}))?;
    let (mut notes, mut kept, mut by_meaning) = c.cancellable(rank(turn, &g, question, budget)).await?;
    c.result("byte_rrank_0", kept > 0, rank_summary(kept, &notes, by_meaning))?;

    // 5. Extended: fill the gaps.
    for round in 0..d.gap_rounds {
        let id = format!("byte_rgaps_{round}");
        c.call(&id, "find_gaps", json!({}))?;
        let more = c.cancellable(gaps(turn, question, &notes, &g.searches)).await?;
        c.result(&id, true, if more.is_empty() { "Nothing important missing".to_string() } else { format!("{} more searches", more.len()) })?;
        if more.is_empty() {
            break;
        }
        let tag = format!("g{round}");
        let lists = run_searches(&c, &mut g, &more, &tag).await?;
        let seen: Vec<String> = g.book.sources.iter().filter(|s| s.read).map(|s| s.url.clone()).collect();
        let candidates = interleave(&lists, &seen, d.pages / 2);
        read_pages(&c, &mut g, &candidates, d.pages / 3, &tag).await?;
        let rid = format!("byte_rrank_{}", round + 1);
        c.call(&rid, "rank_passages", json!({}))?;
        (notes, kept, by_meaning) = c.cancellable(rank(turn, &g, question, budget)).await?;
        c.result(&rid, kept > 0, rank_summary(kept, &notes, by_meaning))?;
    }

    let content = if notes.trim().is_empty() {
        "Research found nothing readable for this question. Say so, answer only what you're sure of, and suggest what the user could check.".to_string()
    } else {
        format!("Research notes for: {question}\n\n{notes}\n{REPORT_RULES}")
    };
    Ok((g.book, content))
}

pub(crate) fn rank_summary(kept: usize, notes: &str, by_meaning: bool) -> String {
    let sources = notes.lines().filter(|l| l.starts_with('[')).count();
    format!("{kept} passages from {sources} sources{}", if by_meaning { ", by meaning" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(url: &str) -> SearchResult {
        SearchResult { title: url.into(), url: url.into(), snippet: String::new() }
    }

    #[test]
    fn only_deep_modes_research() {
        assert!(depth(Mode::Auto).is_none() && depth(Mode::Fast).is_none());
        let (d, e) = (depth(Mode::Deep).unwrap(), depth(Mode::Extended).unwrap());
        assert!(e.pages > d.pages && e.searches > d.searches && e.gap_rounds > d.gap_rounds);
        assert!(applies(Mode::Deep, true, false, "What does research say about intermittent fasting?"));
        assert!(!applies(Mode::Deep, false, false, "What does research say about intermittent fasting?"));
        assert!(!applies(Mode::Auto, true, false, "What does research say about intermittent fasting?"));
        assert!(!applies(Mode::Deep, true, false, "thanks!"));
        // Always: research even what doesn't look like it needs the web; still not small talk.
        assert!(!applies(Mode::Deep, true, false, "I'm thinking about switching to a standing desk at work"));
        assert!(applies(Mode::Deep, true, true, "I'm thinking about switching to a standing desk at work"));
        assert!(!applies(Mode::Deep, true, true, "thanks!"));
        assert!(!applies(Mode::Deep, true, false, "What does my lease say about pets?"));
    }

    #[test]
    fn depth_levels_read_more() {
        assert_eq!(depth_at(Mode::Deep, 0), depth(Mode::Deep));
        let (n, m, x) = (depth_at(Mode::Deep, 0).unwrap(), depth_at(Mode::Deep, 1).unwrap(), depth_at(Mode::Deep, 9).unwrap());
        assert!(n.pages < m.pages && m.pages < x.pages && n.searches < x.searches);
        assert_eq!(x.pages, 32);
        assert_eq!(depth_at(Mode::Extended, 2).unwrap().pages, 48);
        assert!(depth_at(Mode::Auto, 2).is_none());
        assert_eq!((batch(0), batch(1), batch(2), batch(7)), (6, 8, 10, 10));
        assert_eq!((scale_pages(4, 0), scale_pages(4, 1), scale_pages(4, 2)), (4, 6, 8));
    }

    #[test]
    fn plan_is_read_leniently() {
        let (q, p) = parse_plan(r#"{"searches": ["fasting weight loss trials", "fasting weight loss clinical trials", "intermittent fasting muscle loss", "x"], "papers": true}"#, "intermittent fasting weight loss", 4);
        // Near-duplicates and one-word queries are dropped; the user's own search stays first.
        assert_eq!(q, vec!["intermittent fasting weight loss", "fasting weight loss trials", "intermittent fasting muscle loss"]);
        assert!(p);
        let (q, p) = parse_plan("Sure! ```json\n{\"searches\":[\"a b c\",\"d e f\",\"g h i\"],\"papers\":false}```", "first query", 2);
        assert_eq!(q, vec!["first query", "a b c"]);
        assert!(!p);
        assert_eq!(parse_plan("not json", "first query", 4), (vec!["first query".to_string()], false));
    }

    #[test]
    fn reading_alternates_between_searches() {
        let a = vec![r("https://a.com/1"), r("https://a.com/2"), r("https://a.com/3")];
        let b = vec![r("https://b.com/1"), r("https://a.com/1/"), r("https://www.youtube.com/watch?v=x")];
        let got: Vec<String> = interleave(&[a.clone(), b], &[], 10).into_iter().map(|r| r.url).collect();
        assert_eq!(got, vec!["https://a.com/1", "https://b.com/1", "https://a.com/2", "https://a.com/3"]);
        let got = interleave(&[a], &["https://a.com/1".into()], 1);
        assert_eq!(got[0].url, "https://a.com/2");
    }

    #[test]
    fn pages_split_into_passages() {
        let text = format!("{}\n{}\n\n{}", "a".repeat(400), "b".repeat(400), format!("{}. ", "c".repeat(300)).repeat(6));
        let p = passages(&text);
        assert!(p.len() >= 3, "{}", p.len());
        assert!(p.iter().all(|x| x.len() <= PASSAGE_CHARS * 2), "{:?}", p.iter().map(|x| x.len()).collect::<Vec<_>>());
        assert!(passages("\n \n").is_empty());
    }

    #[test]
    fn relevant_passages_score_higher() {
        let main = terms("How much weight do people lose with intermittent fasting?");
        let extra = terms("fasting trials 2024");
        let good = "In a 12-week trial, people on intermittent fasting lost 4.5 kg of weight on average, similar to calorie counting.";
        let bad = "Subscribe to our newsletter for the latest recipes and meal ideas delivered to your inbox every week.";
        assert!(lexical_score(good, &main, &extra) > lexical_score(bad, &main, &extra) * 3.0);
    }

    #[test]
    fn picks_respect_budget_and_per_source_cap() {
        let ranked: Vec<(u32, usize, String)> = (0..6).map(|i| (1, i, format!("one {i}"))).chain((0..2).map(|i| (2, i, format!("two {i}")))).collect();
        let p = pick(&ranked, 10_000);
        assert_eq!(p.iter().filter(|x| x.n == 1).count(), PER_SOURCE);
        assert_eq!(p.iter().filter(|x| x.n == 2).count(), 2);
        assert_eq!(pick(&ranked, 12).len(), 2);
        let mut book = SourceBook::default();
        book.add("One", "https://one.com", "");
        book.add("Two", "https://two.com", "");
        let notes = format_notes(&book, &[Pick { n: 2, order: 0, text: "b".into() }, Pick { n: 1, order: 3, text: "late".into() }, Pick { n: 1, order: 1, text: "early".into() }]);
        assert_eq!(notes, "[1] One\nhttps://one.com\nearly\n…\nlate\n\n[2] Two\nhttps://two.com\nb\n\n");
    }

    /// Real engine + real internet: a Deep research answer with sources and a
    /// confidence line. Needs BYTE_TEST_LLAMA_SERVER, BYTE_TEST_MODEL and BYTE_TEST_WEB=1.
    #[tokio::test]
    #[ignore]
    async fn e2e_deep_research() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let Ok(model) = std::env::var("BYTE_TEST_MODEL") else { return };
        let Some((_server, mut ep)) = crate::chat::e2e_support::start_server_with(&model, &["-c".into(), "16384".into()], None).await else { return };
        ep.context = 16384;
        let dir = tempfile::tempdir().unwrap();
        let log = tools::ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = tools::fetch::web_client();
        let q = std::env::var("BYTE_TEST_QUESTION").unwrap_or_else(|_| "What does research say about intermittent fasting for weight loss?".into());
        let history = vec![chat::ChatMessage::new("user", q.clone())];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Deep, true, None);
        let plan = crate::router::plan_turn(Mode::Deep, crate::settings::ThinkingPref::Off, &q);
        let (ch, seen) = crate::chat::e2e_support::collecting_channel();
        let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: Mode::Deep, web: true, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false, modules: Default::default() };
        let t = std::time::Instant::now();
        crate::agent::run(turn, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        for e in ev.iter().filter(|e| e["kind"] == "toolCall" || e["kind"] == "toolResult") {
            eprintln!("{e}");
        }
        let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
        eprintln!("({:.0}s)\n{content}", t.elapsed().as_secs_f64());
        assert!(ev.iter().any(|e| e["kind"] == "toolCall" && e["name"] == "plan_research"));
        assert!(ev.iter().filter(|e| e["kind"] == "toolResult" && e["ok"] == true).count() >= 3);
        assert!(content.contains('['), "no citations: {content}");
    }
}
