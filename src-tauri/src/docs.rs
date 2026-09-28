//! Documents made on this Mac (Phase 5, light; the BYTE cloud stays the main
//! document maker). The local model plans an outline the user approves, then
//! writes each section as structured JSON ("DocSpec"). The UI turns a DocSpec
//! into PDF, PowerPoint or Word (`src/lib/docs/`), so this module never deals
//! with file formats.
//!
//! JSON comes from llama-server's schema-constrained output (like
//! `summarize.rs`), and is still parsed leniently: bad blocks are dropped
//! instead of failing the whole document.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::Channel;

use crate::engine::Endpoint;
use crate::error::{AppError, AppResult};
use crate::tools::Source;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DocKind {
    Pdf,
    Pptx,
    Docx,
}

impl DocKind {
    fn describe(self) -> &'static str {
        match self {
            DocKind::Pdf => "a well-structured report (PDF)",
            DocKind::Docx => "a well-structured Word document",
            DocKind::Pptx => "a slide presentation",
        }
    }
    /// How much each section says.
    fn section_rules(self) -> &'static str {
        match self {
            DocKind::Pptx => "This is for slides: 1 short intro paragraph at most, then 3-6 bullets of under 12 words each. Add a table or chart only when numbers really help.",
            _ => "Write 2-4 substantial paragraphs, plus bullets, a numbered list, a table, a chart or a callout where they help the reader.",
        }
    }
    fn max_tokens(self) -> u32 {
        match self {
            DocKind::Pptx => 700,
            _ => 1400,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutlineSection {
    pub title: String,
    /// What the section covers (guides the writing; editable).
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Outline {
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    pub sections: Vec<OutlineSection>,
}

/// One piece of a section. Unknown or incomplete blocks are dropped by `clean_blocks`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Block {
    Paragraph { text: String },
    Bullets { items: Vec<String> },
    Numbered { items: Vec<String> },
    Table { columns: Vec<String>, rows: Vec<Vec<String>> },
    /// `chart` is "bar", "line" or "pie".
    Chart { chart: String, title: String, labels: Vec<String>, values: Vec<f64> },
    Callout { text: String },
    Quote { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub title: String,
    pub blocks: Vec<Block>,
}

/// A written document, ready for the UI's renderers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocSpec {
    pub kind: DocKind,
    pub title: String,
    pub subtitle: String,
    pub sections: Vec<Section>,
    /// Numbered sources the text cites as [n].
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum DocEvent {
    /// Something happening before writing ("Searching the web…").
    Phase { text: String },
    /// Writing section `index` (0-based) of `total`.
    Section { index: usize, total: usize, title: String },
}

// ---------- prompts and parsing ----------

fn outline_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "subtitle": { "type": "string" },
            "sections": { "type": "array", "minItems": 3, "maxItems": 12, "items": {
                "type": "object",
                "properties": { "title": { "type": "string" }, "notes": { "type": "string" } },
                "required": ["title", "notes"]
            }}
        },
        "required": ["title", "subtitle", "sections"]
    })
}

fn section_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "blocks": { "type": "array", "minItems": 1, "maxItems": 10, "items": {
                "type": "object",
                "properties": {
                    "type": { "enum": ["paragraph", "bullets", "numbered", "table", "chart", "callout", "quote"] },
                    "text": { "type": "string" },
                    "items": { "type": "array", "items": { "type": "string" } },
                    "columns": { "type": "array", "items": { "type": "string" } },
                    "rows": { "type": "array", "items": { "type": "array", "items": { "type": "string" } } },
                    "chart": { "enum": ["bar", "line", "pie"] },
                    "title": { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "values": { "type": "array", "items": { "type": "number" } }
                },
                "required": ["type"]
            }}
        },
        "required": ["blocks"]
    })
}

fn json_object(reply: &str) -> Option<Value> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    serde_json::from_str(reply.get(start..=end)?).ok()
}

fn tidy(s: &str, max: usize) -> String {
    s.trim().trim_matches('"').chars().take(max).collect::<String>().trim().to_string()
}

pub fn parse_outline(reply: &str) -> Option<Outline> {
    let v = json_object(reply)?;
    let sections: Vec<OutlineSection> = v["sections"]
        .as_array()?
        .iter()
        .filter_map(|s| {
            let title = tidy(s["title"].as_str().or_else(|| s.as_str())?, 90);
            (!title.is_empty()).then(|| OutlineSection { title, notes: tidy(s["notes"].as_str().unwrap_or(""), 300) })
        })
        .take(14)
        .collect();
    let title = tidy(v["title"].as_str().unwrap_or(""), 120);
    (!title.is_empty() && !sections.is_empty()).then(|| Outline { title, subtitle: tidy(v["subtitle"].as_str().unwrap_or(""), 160), sections })
}

fn strings(v: &Value, max_items: usize, max_len: usize) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| tidy(s, max_len)).or_else(|| x.as_f64().map(|n| n.to_string()))).filter(|s| !s.is_empty()).take(max_items).collect())
        .unwrap_or_default()
}

/// Turns the model's blocks into valid ones, dropping what can't be shown.
pub fn clean_blocks(v: &Value) -> Vec<Block> {
    let Some(arr) = v["blocks"].as_array() else { return Vec::new() };
    arr.iter()
        .filter_map(|b| {
            let text = tidy(b["text"].as_str().unwrap_or(""), 4000);
            let items = strings(&b["items"], 12, 400);
            Some(match b["type"].as_str()? {
                "paragraph" if !text.is_empty() => Block::Paragraph { text },
                "callout" if !text.is_empty() => Block::Callout { text },
                "quote" if !text.is_empty() => Block::Quote { text },
                "bullets" if !items.is_empty() => Block::Bullets { items },
                "numbered" if !items.is_empty() => Block::Numbered { items },
                "table" => {
                    let columns = strings(&b["columns"], 8, 60);
                    let rows: Vec<Vec<String>> = b["rows"]
                        .as_array()?
                        .iter()
                        .map(|r| {
                            let mut cells = strings(r, columns.len(), 200);
                            cells.resize(columns.len(), String::new());
                            cells
                        })
                        .filter(|r| r.iter().any(|c| !c.is_empty()))
                        .take(30)
                        .collect();
                    if columns.len() < 2 || rows.is_empty() {
                        return None;
                    }
                    Block::Table { columns, rows }
                }
                "chart" => {
                    let labels = strings(&b["labels"], 12, 40);
                    let values: Vec<f64> = b["values"].as_array()?.iter().filter_map(Value::as_f64).filter(|x| x.is_finite()).take(labels.len()).collect();
                    let chart = match b["chart"].as_str().unwrap_or("bar") {
                        c @ ("line" | "pie") => c.to_string(),
                        _ => "bar".to_string(),
                    };
                    if labels.len() < 2 || values.len() != labels.len() {
                        return None;
                    }
                    Block::Chart { chart, title: tidy(b["title"].as_str().unwrap_or(""), 90), labels, values }
                }
                _ => return None,
            })
        })
        .collect()
}

/// One non-streaming, schema-constrained request (thinking off).
async fn ask_json(http: &reqwest::Client, ep: &Endpoint, system: &str, user: &str, schema: Value, max_tokens: u32) -> AppResult<String> {
    let body = json!({
        "messages": [ { "role": "system", "content": system }, { "role": "user", "content": user } ],
        "max_tokens": max_tokens,
        "temperature": 0.5,
        "stream": false,
        "response_format": { "type": "json_schema", "json_schema": { "name": "doc", "schema": schema } },
        "chat_template_kwargs": { "enable_thinking": false },
    });
    let r = http.post(format!("{}/v1/chat/completions", ep.base_url)).bearer_auth(&ep.api_key).timeout(Duration::from_secs(600)).json(&body).send().await?;
    if !r.status().is_success() {
        return Err(AppError::msg(format!("the engine returned {}", r.status())));
    }
    let v: Value = r.json().await?;
    Ok(v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string())
}

const WRITER: &str = "You are BYTE, writing documents for the user. Write clear, accurate, well-organized content in plain English. \
Never invent statistics, quotes or sources; use numbers only from the research notes or well-known facts. \
When research notes are given, cite them as [n] right after the claim.";

/// Plans the document: title, subtitle and sections with a note on each.
pub async fn outline(http: &reqwest::Client, ep: &Endpoint, kind: DocKind, prompt: &str, material: &str) -> AppResult<Outline> {
    let user = format!(
        "Plan {} about this request:\n\n{prompt}\n\n{}Reply with JSON: a short title, a one-line subtitle, and {} sections, each with a title and a one-sentence note on what it covers.",
        kind.describe(),
        if material.is_empty() { String::new() } else { format!("Material to use:\n{}\n\n", material.chars().take(6000).collect::<String>()) },
        if kind == DocKind::Pptx { "6 to 10" } else { "4 to 8" },
    );
    // One retry: a reply cut off mid-JSON (or empty) is common with small models.
    for _ in 0..2 {
        let reply = ask_json(http, ep, WRITER, &user, outline_schema(), 1500).await?;
        if let Some(o) = parse_outline(&reply) {
            return Ok(o);
        }
    }
    Err(AppError::msg("The model's plan couldn't be read. Try again, or use a bigger model."))
}

/// Writes one section.
async fn section(http: &reqwest::Client, ep: &Endpoint, kind: DocKind, outline: &Outline, i: usize, notes: &str) -> AppResult<Vec<Block>> {
    let s = &outline.sections[i];
    let plan: Vec<String> = outline.sections.iter().enumerate().map(|(j, x)| format!("{}{}. {}", if j == i { "→ " } else { "  " }, j + 1, x.title)).collect();
    let user = format!(
        "Document: \"{}\" ({}).\nSections:\n{}\n\nWrite section {} \"{}\" only. It covers: {}\n{}\n{}\nReply with JSON blocks. Don't repeat the section title.",
        outline.title,
        kind.describe(),
        plan.join("\n"),
        i + 1,
        s.title,
        if s.notes.is_empty() { "what its title says" } else { &s.notes },
        kind.section_rules(),
        if notes.is_empty() { String::new() } else { format!("\nResearch notes (cite as [n]):\n{notes}\n") },
    );
    let reply = ask_json(http, ep, WRITER, &user, section_schema(), kind.max_tokens()).await?;
    let blocks = json_object(&reply).map(|v| clean_blocks(&v)).unwrap_or_default();
    if blocks.is_empty() {
        return Err(AppError::msg(format!("Section \"{}\" came back empty.", s.title)));
    }
    Ok(blocks)
}

/// Research gathered once per document: numbered sources and their text.
#[derive(Default)]
pub struct Research {
    pub sources: Vec<Source>,
    pages: Vec<(u32, String)>,
}

impl Research {
    /// The passages most relevant to one section, as "[n] text" notes.
    pub fn notes_for(&self, topic: &str, budget: usize) -> String {
        let per = (budget / self.pages.len().max(1)).max(400);
        self.pages
            .iter()
            .map(|(n, text)| (n, crate::tools::fetch::relevant_passages(text, topic, per)))
            .filter(|(_, t)| !t.trim().is_empty())
            .map(|(n, t)| format!("[{n}] {t}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Searches the web for the document's topic and reads the best pages.
pub async fn research(net: &reqwest::Client, cloud: Option<&crate::cloud::CloudClient>, query: &str) -> Research {
    let mut out = Research::default();
    let Ok(found) = crate::tools::search::search(net, cloud, query, 6).await else { return out };
    let reads = futures_util::future::join_all(
        found.results.iter().filter(|r| crate::tools::fetch::worth_reading(&r.url)).take(4).map(|r| crate::tools::fetch::fetch_page(net, &r.url)),
    )
    .await;
    for page in reads.into_iter().flatten() {
        let n = out.sources.len() as u32 + 1;
        out.sources.push(Source { n, title: page.title.clone(), url: page.url.clone(), snippet: String::new(), read: true });
        out.pages.push((n, page.text));
    }
    out
}

/// Writes every section of an approved outline.
#[allow(clippy::too_many_arguments)]
pub async fn write(
    http: &reqwest::Client,
    ep: &Endpoint,
    kind: DocKind,
    outline: &Outline,
    research: &Research,
    reference: &str,
    cancel: &tokio_util::sync::CancellationToken,
    events: &Channel<DocEvent>,
) -> AppResult<DocSpec> {
    let total = outline.sections.len();
    let mut sections = Vec::with_capacity(total);
    for (i, s) in outline.sections.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let _ = events.send(DocEvent::Section { index: i, total, title: s.title.clone() });
        let topic = format!("{} {}", s.title, s.notes);
        let mut notes = research.notes_for(&topic, 3500);
        if !reference.is_empty() {
            let from_file = crate::tools::fetch::relevant_passages(reference, &topic, 2500);
            notes = format!("From the user's file:\n{from_file}\n\n{notes}");
        }
        // One retry: small models sometimes return an empty or broken section.
        let blocks = match section(http, ep, kind, outline, i, &notes).await {
            Ok(b) => b,
            Err(_) => section(http, ep, kind, outline, i, &notes).await?,
        };
        sections.push(Section { title: s.title.clone(), blocks });
    }
    Ok(DocSpec { kind, title: outline.title.clone(), subtitle: outline.subtitle.clone(), sections, sources: research.sources.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlines_are_read_leniently() {
        let o = parse_outline("Here: {\"title\": \" Solar power \", \"subtitle\": \"For beginners\", \"sections\": [{\"title\": \"Why solar\", \"notes\": \"Costs\"}, {\"title\": \"\"}, \"How panels work\"]}").unwrap();
        assert_eq!(o.title, "Solar power");
        assert_eq!(o.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Why solar", "How panels work"]);
        assert!(parse_outline("{\"title\": \"x\", \"sections\": []}").is_none());
        assert!(parse_outline("no json").is_none());
    }

    #[test]
    fn bad_blocks_are_dropped_and_tables_charts_fixed() {
        let v = json!({ "blocks": [
            { "type": "paragraph", "text": "Solar panels turn light into electricity [1]." },
            { "type": "paragraph", "text": "" },
            { "type": "bullets", "items": ["Cheap", "Clean", ""] },
            { "type": "table", "columns": ["Year", "Cost"], "rows": [["2015", "$3.00"], ["2025"], ["", ""]] },
            { "type": "table", "columns": ["Only one"], "rows": [["x"]] },
            { "type": "chart", "chart": "donut", "title": "Cost per watt", "labels": ["2015", "2025"], "values": [3.0, 1.1] },
            { "type": "chart", "labels": ["a", "b"], "values": [1.0] },
            { "type": "video", "text": "?" }
        ]});
        let b = clean_blocks(&v);
        assert_eq!(b.len(), 4, "{b:?}");
        assert_eq!(b[1], Block::Bullets { items: vec!["Cheap".into(), "Clean".into()] });
        assert_eq!(b[2], Block::Table { columns: vec!["Year".into(), "Cost".into()], rows: vec![vec!["2015".into(), "$3.00".into()], vec!["2025".into(), String::new()]] });
        assert!(matches!(&b[3], Block::Chart { chart, .. } if chart == "bar"));
        // The shape the UI reads.
        assert_eq!(serde_json::to_value(&b[0]).unwrap(), json!({ "type": "paragraph", "text": "Solar panels turn light into electricity [1]." }));
    }

    /// Real engine (BYTE_TEST_MODEL): a small model plans and writes valid JSON.
    #[tokio::test]
    #[ignore]
    async fn e2e_plans_and_writes_a_section() {
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let http = crate::chat::local_client();
        let o = outline(&http, &ep, DocKind::Pdf, "A short guide to growing tomatoes on a balcony", "").await.unwrap();
        eprintln!("{o:#?}");
        assert!(o.sections.len() >= 3);
        let blocks = section(&http, &ep, DocKind::Pdf, &o, 0, "").await.unwrap();
        eprintln!("{blocks:#?}");
        assert!(!blocks.is_empty());
    }
}
