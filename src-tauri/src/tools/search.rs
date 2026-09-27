//! Keyless web search. Tries DuckDuckGo (HTML, then Lite), then Bing, and
//! spaces requests out so search engines don't throttle BYTE.

use std::time::{Duration, Instant};

use base64::Engine as _;
use scraper::{Html, Selector};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Browser-like identity; the endpoints serve bots a challenge page.
pub const BROWSER_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

/// Minimum gap between two outgoing searches.
const MIN_GAP: Duration = Duration::from_millis(1200);
static LAST_SEARCH: Mutex<Option<Instant>> = Mutex::const_new(None);

async fn pace() {
    let mut last = LAST_SEARCH.lock().await;
    if let Some(t) = *last {
        let since = t.elapsed();
        if since < MIN_GAP {
            tokio::time::sleep(MIN_GAP - since).await;
        }
    }
    *last = Some(Instant::now());
}

/// Runs a web search, falling back across engines until one returns results.
pub async fn search(client: &reqwest::Client, query: &str, max: usize) -> AppResult<Vec<SearchResult>> {
    let query = query.trim();
    if query.is_empty() {
        return Err(AppError::msg("empty search query"));
    }
    let mut errors = Vec::new();
    for engine in [Engine::DdgHtml, Engine::DdgLite, Engine::Bing] {
        pace().await;
        match engine.run(client, query).await {
            Ok(results) if !results.is_empty() => return Ok(dedupe(results, max)),
            Ok(_) => errors.push(format!("{}: no results", engine.name())),
            Err(e) => errors.push(format!("{}: {e}", engine.name())),
        }
    }
    log::warn!("all search engines failed for {query:?}: {errors:?}");
    Err(AppError::msg("web search is unavailable right now (the search engines didn't respond); try again in a minute"))
}

#[derive(Clone, Copy)]
enum Engine {
    DdgHtml,
    DdgLite,
    Bing,
}

impl Engine {
    fn name(self) -> &'static str {
        match self {
            Engine::DdgHtml => "duckduckgo",
            Engine::DdgLite => "duckduckgo-lite",
            Engine::Bing => "bing",
        }
    }

    async fn run(self, client: &reqwest::Client, query: &str) -> AppResult<Vec<SearchResult>> {
        let req = match self {
            Engine::DdgHtml => client.post("https://html.duckduckgo.com/html/").form(&[("q", query), ("b", "")]),
            Engine::DdgLite => client.post("https://lite.duckduckgo.com/lite/").form(&[("q", query)]),
            Engine::Bing => client.get("https://www.bing.com/search").query(&[("q", query), ("setlang", "en")]),
        };
        let resp = req
            .header(reqwest::header::USER_AGENT, BROWSER_UA)
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .timeout(Duration::from_secs(12))
            .send()
            .await?;
        // DuckDuckGo answers 202 with a bot-check page when it is throttling.
        if resp.status() != reqwest::StatusCode::OK {
            return Err(AppError::msg(format!("status {}", resp.status())));
        }
        let html = resp.text().await?;
        Ok(match self {
            Engine::DdgHtml => parse_ddg_html(&html),
            Engine::DdgLite => parse_ddg_lite(&html),
            Engine::Bing => parse_bing(&html),
        })
    }
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

fn text_of(el: scraper::ElementRef<'_>) -> String {
    el.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse_ddg_html(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let (result, link, snippet) = (sel("div.result"), sel("a.result__a"), sel(".result__snippet"));
    doc.select(&result)
        .filter(|r| !r.value().classes().any(|c| c == "result--ad"))
        .filter_map(|r| {
            let a = r.select(&link).next()?;
            let url = clean_ddg_url(a.value().attr("href")?)?;
            Some(SearchResult {
                title: text_of(a),
                url,
                snippet: r.select(&snippet).next().map(text_of).unwrap_or_default(),
            })
        })
        .collect()
}

pub fn parse_ddg_lite(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let (link, snippet) = (sel("a.result-link"), sel("td.result-snippet"));
    let snippets: Vec<String> = doc.select(&snippet).map(text_of).collect();
    doc.select(&link)
        .enumerate()
        .filter_map(|(i, a)| {
            Some(SearchResult {
                title: text_of(a),
                url: clean_ddg_url(a.value().attr("href")?)?,
                snippet: snippets.get(i).cloned().unwrap_or_default(),
            })
        })
        .collect()
}

pub fn parse_bing(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let (item, link, caption) = (sel("li.b_algo"), sel("h2 a"), sel(".b_caption p, p.b_lineclamp2, p.b_lineclamp3, p.b_lineclamp4"));
    doc.select(&item)
        .filter_map(|r| {
            let a = r.select(&link).next()?;
            let url = clean_bing_url(a.value().attr("href")?)?;
            Some(SearchResult {
                title: text_of(a),
                url,
                snippet: r.select(&caption).next().map(text_of).unwrap_or_default(),
            })
        })
        .collect()
}

/// DuckDuckGo links are either direct or `//duckduckgo.com/l/?uddg=<encoded>`;
/// ad links go through `y.js` and are dropped.
fn clean_ddg_url(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") { format!("https:{href}") } else { href.to_string() };
    let parsed = url::Url::parse(&absolute).ok()?;
    if parsed.domain().is_some_and(|d| d.ends_with("duckduckgo.com")) {
        if parsed.path().contains("y.js") {
            return None;
        }
        let target = parsed.query_pairs().find(|(k, _)| k == "uddg")?.1.into_owned();
        return normalize(&target);
    }
    normalize(&absolute)
}

/// Bing wraps results as `bing.com/ck/a?...&u=a1<base64url(target)>`.
fn clean_bing_url(href: &str) -> Option<String> {
    let parsed = url::Url::parse(href).ok()?;
    if parsed.domain().is_some_and(|d| d.ends_with("bing.com")) {
        let u = parsed.query_pairs().find(|(k, _)| k == "u")?.1.into_owned();
        let encoded = u.strip_prefix("a1")?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.trim_end_matches('='))
            .ok()?;
        return normalize(&String::from_utf8(bytes).ok()?);
    }
    normalize(href)
}

fn normalize(u: &str) -> Option<String> {
    let parsed = url::Url::parse(u).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.to_string())
}

fn dedupe(results: Vec<SearchResult>, max: usize) -> Vec<SearchResult> {
    let mut seen = std::collections::HashSet::new();
    results
        .into_iter()
        .filter(|r| !r.title.is_empty() && seen.insert(r.url.trim_end_matches('/').to_lowercase()))
        .take(max)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn parses_real_duckduckgo_html_page() {
        let r = parse_ddg_html(&fixture("ddg_html.html"));
        assert!(r.len() >= 8, "{}", r.len());
        assert_eq!(r[0].url, "https://v2.tauri.app/release/");
        assert!(!r[0].title.is_empty());
        assert!(r.iter().filter(|x| !x.snippet.is_empty()).count() >= 5);
        assert!(r.iter().all(|x| x.url.starts_with("http")));
    }

    #[test]
    fn parses_real_bing_page_and_decodes_redirects() {
        let r = parse_bing(&fixture("bing.html"));
        assert!(r.len() >= 5, "{}", r.len());
        assert!(r.iter().all(|x| !x.url.contains("bing.com/ck")), "{:?}", r[0]);
        assert!(r.iter().any(|x| x.url.starts_with("https://tauri.app")), "{r:?}");
    }

    #[test]
    fn parses_duckduckgo_lite_markup() {
        let html = r#"<table>
          <tr><td>1.</td><td><a rel="nofollow" href="https://www.rust-lang.org/" class='result-link'>Rust Programming Language</a></td></tr>
          <tr><td></td><td class='result-snippet'>A language empowering everyone.</td></tr>
          <tr><td>2.</td><td><a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust-lang.org%2Fbook%2F&amp;rut=x" class='result-link'>The Book</a></td></tr>
          <tr><td></td><td class='result-snippet'>Learn Rust.</td></tr>
        </table>"#;
        let r = parse_ddg_lite(html);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].url, "https://doc.rust-lang.org/book/");
        assert_eq!(r[1].snippet, "Learn Rust.");
    }

    #[test]
    fn cleans_redirects_and_drops_ads() {
        assert_eq!(
            clean_ddg_url("//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1&rut=abc").as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(clean_ddg_url("https://duckduckgo.com/y.js?ad_domain=x"), None);
        assert_eq!(clean_ddg_url("javascript:alert(1)"), None);
        // base64url of "https://tauri.app/"
        assert_eq!(clean_bing_url("https://www.bing.com/ck/a?!&u=a1aHR0cHM6Ly90YXVyaS5hcHAv&ntb=1").as_deref(), Some("https://tauri.app/"));
    }

    #[test]
    fn dedupes_and_limits() {
        let r = |u: &str| SearchResult { title: "t".into(), url: u.into(), snippet: String::new() };
        let out = dedupe(vec![r("https://a.com/"), r("https://A.com"), r("https://b.com"), r("https://c.com")], 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].url, "https://b.com");
    }
}
