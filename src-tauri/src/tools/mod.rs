//! Tools the model can call, and the bookkeeping around them: the JSON
//! schemas sent to the engine, numbered sources for citations, and an
//! action log of every call.

pub mod calc;
pub mod fetch;
pub mod search;
pub mod weather;

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{json, Value};

/// A citable source. `n` is the number the model uses in `[n]`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub n: u32,
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// True once BYTE actually read the page (not just saw it in results).
    pub read: bool,
}

/// Sources gathered during one answer, numbered in order of discovery.
#[derive(Debug, Default, Clone)]
pub struct SourceBook {
    pub sources: Vec<Source>,
}

impl SourceBook {
    fn key(url: &str) -> String {
        url.trim_end_matches('/').to_lowercase()
    }

    /// Adds a source (or returns the existing one's number for the same URL).
    pub fn add(&mut self, title: &str, url: &str, snippet: &str) -> u32 {
        let k = Self::key(url);
        if let Some(s) = self.sources.iter().find(|s| Self::key(&s.url) == k) {
            return s.n;
        }
        let n = self.sources.len() as u32 + 1;
        self.sources.push(Source { n, title: title.to_string(), url: url.to_string(), snippet: snippet.to_string(), read: false });
        n
    }

    pub fn mark_read(&mut self, url: &str, title: &str) -> u32 {
        let n = self.add(title, url, "");
        if let Some(s) = self.sources.iter_mut().find(|s| s.n == n) {
            s.read = true;
            if s.title.is_empty() || s.title == s.url {
                s.title = title.to_string();
            }
        }
        n
    }
}

/// Per-answer limits and context for tool execution.
pub struct ToolContext<'a> {
    pub net: &'a reqwest::Client,
    /// The BYTE cloud, whose search answers first when a key is saved (not for private chats).
    pub cloud: Option<&'a crate::cloud::CloudClient>,
    /// The user's question, used to pick relevant passages from pages.
    pub question: &'a str,
    pub max_results: usize,
    /// Characters of page text returned to the model per page.
    pub page_chars: usize,
    pub log: &'a ActionLog,
    /// The app, when the user's knowledge base can be searched ("My files").
    pub files: Option<&'a tauri::AppHandle>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    pub ok: bool,
    /// Short human description for the UI, e.g. "Found 8 results".
    pub summary: String,
    /// What the model sees.
    #[serde(skip)]
    pub content: String,
}

pub const WEB_SEARCH: &str = "web_search";
pub const READ_PAGE: &str = "read_page";
pub const CALCULATE: &str = "calculate";
pub const WEATHER: &str = "weather";
/// Suggests saving a fact about the user; the UI asks before saving it.
pub const REMEMBER: &str = "remember";
/// Searches the folders the user added to the knowledge base.
pub const SEARCH_FILES: &str = "search_my_files";

/// Passages returned per knowledge base search.
const FILE_HITS: usize = 6;

/// `file://` link for a passage (with its page), used as its source URL.
pub fn file_url(path: &str, page: Option<u32>) -> String {
    let mut u = url::Url::from_file_path(path).map(|u| u.to_string()).unwrap_or_else(|_| format!("file://{path}"));
    if let Some(p) = page {
        u.push_str(&format!("#page={p}"));
    }
    u
}

/// OpenAI-style tool definitions for the engine.
pub fn specs(web: bool, memory: bool, files: bool) -> Vec<Value> {
    let mut v = Vec::new();
    if files {
        v.push(json!({
            "type": "function",
            "function": {
                "name": SEARCH_FILES,
                "description": "Search the user's own files (the folders they added to BYTE: documents, notes, PDFs). Use it whenever the question may be answered by their files. Returns numbered passages with file names and pages.",
                "parameters": {
                    "type": "object",
                    "properties": { "query": { "type": "string", "description": "What to look for, in a few words." } },
                    "required": ["query"]
                }
            }
        }));
    }
    if web {
        v.push(json!({
            "type": "function",
            "function": {
                "name": WEB_SEARCH,
                "description": "Search the web. Use for anything recent or time-sensitive (news, prices, releases, schedules, current events) or facts you are not sure about. Returns numbered results with titles, links and snippets.",
                "parameters": {
                    "type": "object",
                    "properties": { "query": { "type": "string", "description": "A concise search query, like you would type into a search engine." } },
                    "required": ["query"]
                }
            }
        }));
        v.push(json!({
            "type": "function",
            "function": {
                "name": WEATHER,
                "description": "Current weather and a 7-day forecast for a place. Use this for any weather question instead of searching.",
                "parameters": {
                    "type": "object",
                    "properties": { "place": { "type": "string", "description": "City, with state or country if it helps, e.g. 'Pittsburgh, PA' or 'Paris, France'." } },
                    "required": ["place"]
                }
            }
        }));
        v.push(json!({
            "type": "function",
            "function": {
                "name": READ_PAGE,
                "description": "Read a web page to get details the search snippets don't include. Pass a URL from the search results.",
                "parameters": {
                    "type": "object",
                    "properties": { "url": { "type": "string", "description": "The full http(s) URL to read." } },
                    "required": ["url"]
                }
            }
        }));
    }
    v.push(json!({
        "type": "function",
        "function": {
            "name": CALCULATE,
            "description": "Evaluate math exactly: arithmetic, percentages, powers, unit conversions (e.g. '5 km to miles', '15% of 80', '2^32'). Always use this instead of doing arithmetic yourself.",
            "parameters": {
                "type": "object",
                "properties": { "expression": { "type": "string" } },
                "required": ["expression"]
            }
        }
    }));
    if memory {
        v.push(json!({
            "type": "function",
            "function": {
                "name": REMEMBER,
                "description": "Suggest saving a lasting fact or preference the user shared about themselves, so you remember it in future chats. The user confirms first.",
                "parameters": {
                    "type": "object",
                    "properties": { "note": { "type": "string", "description": "A short third-person note, e.g. 'Works as a nurse' or 'Prefers short answers'." } },
                    "required": ["note"]
                }
            }
        }));
    }
    v
}

pub async fn run(ctx: &ToolContext<'_>, book: &mut SourceBook, name: &str, args: &Value) -> ToolOutput {
    let arg = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let out = match name {
        WEB_SEARCH => {
            let q = arg("query");
            match search::search(ctx.net, ctx.cloud, &q, ctx.max_results).await {
                Ok(search::Searched { results, source }) => {
                    let mut content = format!("Search results for \"{q}\":\n");
                    for r in &results {
                        let n = book.add(&r.title, &r.url, &r.snippet);
                        content.push_str(&format!("\n[{n}] {}\n{}\n{}\n", r.title, r.url, r.snippet));
                    }
                    content.push_str("\nAnswer only from these results and pages you read, citing them as [n]. If they don't clearly contain the answer, use read_page on the most relevant link, or say you couldn't confirm it.");
                    let via = if source == "your BYTE cloud" { " via your BYTE cloud" } else { "" };
                    ToolOutput { ok: true, summary: format!("{} results{via}", results.len()), content }
                }
                Err(e) => ToolOutput { ok: false, summary: e.to_string(), content: format!("Search failed: {e}") },
            }
        }
        READ_PAGE => {
            let url = arg("url");
            match fetch::fetch_page(ctx.net, &url).await {
                Ok(page) => {
                    let n = book.mark_read(&page.url, &page.title);
                    let text = fetch::relevant_passages(&page.text, ctx.question, ctx.page_chars);
                    let title: String = page.title.chars().take(60).collect();
                    ToolOutput {
                        ok: true,
                        summary: if title.is_empty() { host_of(&page.url) } else { title },
                        content: format!("[{n}] {}\n{}\n\n{text}", page.title, page.url),
                    }
                }
                Err(e) => ToolOutput { ok: false, summary: e.to_string(), content: format!("Couldn't read {url}: {e}") },
            }
        }
        WEATHER => match weather::forecast(ctx.net, &arg("place")).await {
            Ok((place, text)) => {
                let n = book.add(&format!("Weather forecast for {}", place.label()), &weather::source_url(&place), "");
                book.mark_read(&weather::source_url(&place), &format!("Weather forecast for {}", place.label()));
                ToolOutput { ok: true, summary: place.label(), content: format!("[{n}] {text}") }
            }
            Err(e) => ToolOutput { ok: false, summary: e.to_string(), content: format!("Couldn't get the weather: {e}") },
        },
        CALCULATE => match calc::calculate(&arg("expression")) {
            Ok(r) => ToolOutput { ok: true, summary: format!("= {r}"), content: r },
            Err(e) => ToolOutput { ok: false, summary: e.to_string(), content: format!("Error: {e}") },
        },
        SEARCH_FILES => {
            let q = arg("query");
            match ctx.files {
                None => ToolOutput { ok: false, summary: "My files is off".into(), content: "The user's files can't be searched right now.".into() },
                Some(app) => match crate::kb::search(app, if q.is_empty() { ctx.question } else { &q }, FILE_HITS).await {
                    Ok(hits) if hits.is_empty() => ToolOutput { ok: true, summary: "Nothing found in your files".into(), content: "No passages in the user's files match. Say so; don't guess what their files contain.".into() },
                    Ok(hits) => {
                        let mut content = format!("Passages from the user's files for \"{q}\":\n");
                        for h in &hits {
                            let title = match h.page {
                                Some(p) => format!("{} (p. {p})", h.name),
                                None => h.name.clone(),
                            };
                            let url = file_url(&h.path, h.page);
                            let snippet: String = h.text.chars().take(240).collect();
                            let n = book.add(&title, &url, &snippet);
                            book.mark_read(&url, &title);
                            content.push_str(&format!("\n[{n}] {title}\n{}\n", h.text));
                        }
                        content.push_str("\nAnswer from these passages, citing them as [n]. If they don't answer the question, say so.");
                        let files: std::collections::BTreeSet<&str> = hits.iter().map(|h| h.name.as_str()).collect();
                        ToolOutput { ok: true, summary: format!("{} passages from {} file{}", hits.len(), files.len(), if files.len() == 1 { "" } else { "s" }), content }
                    }
                    Err(e) => ToolOutput { ok: false, summary: e.to_string(), content: format!("Searching the user's files failed: {e}") },
                },
            }
        }
        REMEMBER => {
            let note = arg("note");
            if note.is_empty() {
                ToolOutput { ok: false, summary: "empty note".into(), content: "Error: the note is empty.".into() }
            } else {
                // Nothing is saved here: the UI shows the note with Save / Dismiss.
                ToolOutput { ok: true, summary: note.chars().take(200).collect(), content: "Suggested to the user; it's saved only if they confirm. Continue your answer normally without mentioning this.".into() }
            }
        }
        other => ToolOutput { ok: false, summary: format!("unknown tool {other}"), content: format!("Unknown tool {other}.") },
    };
    ctx.log.record(name, args, out.ok, &out.summary);
    out
}

pub fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
        .unwrap_or_else(|| url.to_string())
}

/// Append-only JSON-lines log of every tool call (read by the permissions
/// dashboard later). Rotated at 5 MB.
#[derive(Debug, Clone)]
pub struct ActionLog {
    path: PathBuf,
}

impl ActionLog {
    pub fn new(path: PathBuf) -> Self {
        ActionLog { path }
    }

    pub fn record(&self, tool: &str, args: &Value, ok: bool, summary: &str) {
        use std::io::Write;
        if std::fs::metadata(&self.path).map(|m| m.len() > 5_000_000).unwrap_or(false) {
            let _ = std::fs::rename(&self.path, self.path.with_extension("jsonl.1"));
        }
        let line = json!({ "ts": chrono::Utc::now().to_rfc3339(), "tool": tool, "args": args, "ok": ok, "summary": summary });
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_numbered_and_deduplicated() {
        let mut b = SourceBook::default();
        assert_eq!(b.add("A", "https://a.com/", "x"), 1);
        assert_eq!(b.add("B", "https://b.com", "y"), 2);
        assert_eq!(b.add("A again", "https://A.com", ""), 1);
        assert_eq!(b.mark_read("https://b.com/", "B page"), 2);
        assert!(b.sources[1].read);
        assert_eq!(b.mark_read("https://c.com", "C"), 3);
    }

    #[test]
    fn specs_respect_web_toggle() {
        let names = |v: Vec<Value>| v.iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(specs(true, false, false)), vec![WEB_SEARCH, WEATHER, READ_PAGE, CALCULATE]);
        assert_eq!(names(specs(false, false, false)), vec![CALCULATE]);
        assert_eq!(names(specs(false, true, false)), vec![CALCULATE, REMEMBER]);
        assert_eq!(names(specs(false, false, true)), vec![SEARCH_FILES, CALCULATE]);
        assert_eq!(file_url("/Users/me/My Lease.pdf", Some(3)), "file:///Users/me/My%20Lease.pdf#page=3");
    }

    #[tokio::test]
    async fn calculator_runs_through_registry_and_is_logged() {
        let dir = tempfile::tempdir().unwrap();
        let log = ActionLog::new(dir.path().join("actions.jsonl"));
        let net = reqwest::Client::new();
        let ctx = ToolContext { net: &net, cloud: None, question: "", max_results: 5, page_chars: 1000, log: &log, files: None };
        let out = run(&ctx, &mut SourceBook::default(), CALCULATE, &json!({"expression": "6*7"})).await;
        assert!(out.ok);
        assert_eq!(out.content, "42");
        let logged = std::fs::read_to_string(dir.path().join("actions.jsonl")).unwrap();
        assert!(logged.contains("\"tool\":\"calculate\""));
    }

    #[tokio::test]
    async fn read_page_refuses_local_urls() {
        let dir = tempfile::tempdir().unwrap();
        let log = ActionLog::new(dir.path().join("a.jsonl"));
        let net = reqwest::Client::new();
        let ctx = ToolContext { net: &net, cloud: None, question: "", max_results: 5, page_chars: 1000, log: &log, files: None };
        let out = run(&ctx, &mut SourceBook::default(), READ_PAGE, &json!({"url": "http://127.0.0.1:8080/"})).await;
        assert!(!out.ok);
    }

    /// Live internet check: `BYTE_TEST_WEB=1 cargo test live_web -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_web_search_and_read() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let net = fetch::web_client();
        let found = search::search(&net, None, "rust programming language", 5).await.expect("search");
        eprintln!("results from {}: {:#?}", found.source, found.results.iter().map(|r| (&r.title, &r.url)).collect::<Vec<_>>());
        assert!(!found.results.is_empty());
        let page = fetch::fetch_page(&net, "https://www.rust-lang.org/").await.expect("read");
        eprintln!("page: {} ({} chars)", page.title, page.text.len());
        assert!(page.text.to_lowercase().contains("rust"));
    }

    #[test]
    fn host_strips_www() {
        assert_eq!(host_of("https://www.example.com/a"), "example.com");
    }
}
