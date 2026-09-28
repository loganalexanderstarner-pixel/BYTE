//! Web search. With a BYTE cloud key, the cloud's `/api/search` (SearXNG on
//! the cluster: Google, Bing, DuckDuckGo and Brave merged) answers first.
//! Without one, or when it can't help, the keyless chain runs: DuckDuckGo
//! (HTML, then Lite), then Bing. Either way BYTE keeps
//! only results that are actually about the query (Bing serves unrelated
//! pages to clients it thinks are bots), adds matching Wikipedia articles,
//! and spaces requests out so search engines don't throttle BYTE.

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

/// After DuckDuckGo throttles BYTE (a 202 bot check), leave it alone this long.
const DDG_COOLDOWN: Duration = Duration::from_secs(120);
static DDG_BLOCKED_AT: Mutex<Option<Instant>> = Mutex::const_new(None);

/// After the cloud's search says 429 (60 searches per 5 minutes) or fails,
/// BYTE leaves it alone for a while instead of retrying.
static CLOUD_RESTING_UNTIL: Mutex<Option<Instant>> = Mutex::const_new(None);

/// Search results and where they came from ("your BYTE cloud", "duckduckgo", …).
#[derive(Debug, Clone, PartialEq)]
pub struct Searched {
    pub results: Vec<SearchResult>,
    pub source: String,
}

/// Asks the cloud's search. `Ok(None)` means "use the keyless chain".
async fn cloud_search(cloud: &crate::cloud::CloudClient, query: &str, max: usize) -> Option<Vec<SearchResult>> {
    if CLOUD_RESTING_UNTIL.lock().await.is_some_and(|t| Instant::now() < t) {
        return None;
    }
    let rest = |secs: u64| async move {
        *CLOUD_RESTING_UNTIL.lock().await = Some(Instant::now() + Duration::from_secs(secs));
    };
    match cloud.search(query, max.max(8)).await {
        Ok((engine, results)) => {
            let found = results.len();
            let relevant = on_topic(results.clone(), query);
            // SearXNG merges several engines, so its results are trusted a little
            // more: one topic word is enough if the stricter check leaves nothing.
            let relevant = if relevant.is_empty() && engine == "searxng" { keep_matching(results, query, 1) } else { relevant };
            // "ddgs" is the same DuckDuckGo BYTE scrapes itself: same suspicion.
            let good = !relevant.is_empty() && (engine == "searxng" || relevant.len() * 2 >= found.min(6));
            log::info!("cloud search ({engine}): {} of {found} results on topic for {query:?}", relevant.len());
            good.then_some(relevant)
        }
        Err(crate::cloud::CloudError::Limited(m)) => {
            log::warn!("cloud search rate-limited, resting 5 minutes: {m}");
            rest(300).await;
            None
        }
        Err(crate::cloud::CloudError::Unauthorized) => {
            rest(600).await;
            None
        }
        Err(e) => {
            log::warn!("cloud search failed: {}", AppError::from(e));
            rest(60).await;
            None
        }
    }
}

/// Runs a web search, falling back across engines until one returns results
/// that are about the query. Matching Wikipedia articles are mixed in (they're
/// reliable for people, companies, places and things).
pub async fn search(client: &reqwest::Client, cloud: Option<&crate::cloud::CloudClient>, query: &str, max: usize) -> AppResult<Searched> {
    let query = query.trim();
    if query.is_empty() {
        return Err(AppError::msg("empty search query"));
    }
    let wiki = wikipedia(client, query);
    let web = async {
        if let Some(c) = cloud {
            if let Some(results) = cloud_search(c, query, max).await {
                return Ok((results, "your BYTE cloud".to_string()));
            }
        }
        let mut errors = Vec::new();
        for engine in [Engine::DdgHtml, Engine::DdgLite, Engine::Bing] {
            if engine.is_ddg() && DDG_BLOCKED_AT.lock().await.is_some_and(|t| t.elapsed() < DDG_COOLDOWN) {
                errors.push(format!("{}: resting after a bot check", engine.name()));
                continue;
            }
            pace().await;
            match engine.run(client, query).await {
                Ok(results) => {
                    let found = results.len();
                    let relevant = on_topic(results, query);
                    // Mostly unrelated results mean the engine is serving junk: try the next one.
                    if !relevant.is_empty() && relevant.len() * 2 >= found.min(6) {
                        return Ok((relevant, engine.name().to_string()));
                    }
                    errors.push(format!("{}: {} of {found} results on topic", engine.name(), relevant.len()));
                }
                Err(e) => {
                    if engine.is_ddg() && e.to_string().contains("202") {
                        *DDG_BLOCKED_AT.lock().await = Some(Instant::now());
                    }
                    errors.push(format!("{}: {e}", engine.name()));
                }
            }
        }
        Err(errors)
    };
    let (web, wiki) = tokio::join!(web, wiki);
    // Wikipedia's own search already matched these; one topic word in the title or snippet is enough.
    let wiki = keep_matching(wiki.unwrap_or_default(), query, 1);
    match web {
        Ok((results, source)) => Ok(Searched { results: dedupe(blend(results, wiki), max), source }),
        Err(errors) => {
            log::warn!("web search engines failed for {query:?}: {errors:?}");
            if wiki.is_empty() {
                Err(AppError::msg("web search is unavailable right now (the search engines didn't respond); try again in a minute"))
            } else {
                Ok(Searched { results: dedupe(wiki, max), source: "wikipedia".into() })
            }
        }
    }
}

/// Web results with the best Wikipedia article second (after the top web
/// result), so the pages BYTE reads include it.
fn blend(mut web: Vec<SearchResult>, wiki: Vec<SearchResult>) -> Vec<SearchResult> {
    if let Some(w) = wiki.into_iter().next() {
        let already = web.iter().any(|r| r.url.contains("wikipedia.org/wiki/"));
        if !already {
            web.insert(1.min(web.len()), w);
        }
    }
    web
}

/// Words that say nothing about the topic.
const QUERY_NOISE: &[&str] = &[
    "the", "and", "for", "are", "was", "were", "what", "whats", "when", "where", "which", "who", "whom", "why", "how",
    "does", "did", "can", "could", "should", "would", "will", "much", "many", "with", "this", "that", "these",
    "those", "there", "from", "about", "into", "than", "then", "have", "has", "had", "you", "your", "get", "got",
    "is", "a", "an", "of", "to", "in", "on", "at", "by", "or", "it", "its", "my", "me", "i", "do", "be", "any",
    "best", "good", "worth", "buy", "buying", "now", "current", "currently", "latest", "new", "today", "tell", "know",
    "fix", "error", "make", "use", "using", "way", "ways", "some",
];

fn topic_words(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !QUERY_NOISE.contains(w) && (w.len() > 1 || w.chars().all(|c| c.is_ascii_digit())))
        .map(str::to_string)
        .collect()
}

/// Keeps results that mention enough of the topic words (two, or a third of a
/// long query) in their title, address or snippet.
pub fn on_topic(results: Vec<SearchResult>, query: &str) -> Vec<SearchResult> {
    let n = topic_words(query).len();
    keep_matching(results, query, if n <= 1 { 1 } else { 2.max(n.div_ceil(3)) })
}

fn keep_matching(results: Vec<SearchResult>, query: &str, need: usize) -> Vec<SearchResult> {
    let words = topic_words(query);
    if words.is_empty() {
        return results;
    }
    results
        .into_iter()
        .filter(|r| {
            let hay = format!("{} {} {}", r.title, r.url, r.snippet).to_lowercase();
            words.iter().filter(|w| hay.contains(w.as_str())).count() >= need
        })
        .collect()
}

/// Wikipedia's search API: reliable and keyless. Returns article links.
async fn wikipedia(client: &reqwest::Client, query: &str) -> AppResult<Vec<SearchResult>> {
    let words = topic_words(query).join(" ");
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let v: serde_json::Value = client
        .get("https://en.wikipedia.org/w/api.php")
        .query(&[("action", "query"), ("list", "search"), ("srsearch", words.as_str()), ("format", "json"), ("srlimit", "3"), ("utf8", "1")])
        .header(reqwest::header::USER_AGENT, "BYTE/1.0 (desktop assistant; https://github.com/loganalexanderstarner-pixel/BYTE)")
        .timeout(Duration::from_secs(8))
        .send()
        .await?
        .json()
        .await?;
    Ok(parse_wikipedia(&v))
}

pub fn parse_wikipedia(v: &serde_json::Value) -> Vec<SearchResult> {
    v["query"]["search"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let title = r["title"].as_str()?;
                    let snippet = Html::parse_fragment(r["snippet"].as_str().unwrap_or("")).root_element().text().collect::<String>();
                    Some(SearchResult {
                        title: format!("{title} - Wikipedia"),
                        url: format!("https://en.wikipedia.org/wiki/{}", title.replace(' ', "_")),
                        snippet: snippet.split_whitespace().collect::<Vec<_>>().join(" "),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy)]
enum Engine {
    DdgHtml,
    DdgLite,
    Bing,
}

impl Engine {
    fn is_ddg(self) -> bool {
        matches!(self, Engine::DdgHtml | Engine::DdgLite)
    }

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

    fn r(title: &str, url: &str) -> SearchResult {
        SearchResult { title: title.into(), url: url.into(), snippet: String::new() }
    }

    #[test]
    fn junk_results_are_dropped() {
        // What Bing served for "How much does a Tesla Model 3 cost".
        let junk = vec![
            r("Create a Google Account for Gmail", "https://support.google.com/mail/answer/56256"),
            r("Definition of MUCH", "https://www.merriam-webster.com/dictionary/much"),
            r("Can I upgrade to Windows 11? | Microsoft Support", "https://support.microsoft.com/windows-11"),
        ];
        assert!(on_topic(junk, "How much does a Tesla Model 3 cost").is_empty());
        let good = vec![r("Tesla Model 3 price and specs", "https://www.edmunds.com/tesla/model-3/"), r("Model 3 | Tesla", "https://www.tesla.com/model3")];
        assert_eq!(on_topic(good, "How much does a Tesla Model 3 cost").len(), 2);
        // "What is a CEO?" isn't about OpenAI's CEO.
        let ceo = vec![r("What is a CEO? Roles and Responsibilities", "https://www.investopedia.com/terms/c/ceo.asp"), r("OpenAI - Wikipedia", "https://en.wikipedia.org/wiki/OpenAI")];
        let kept = on_topic(ceo, "Who is the CEO of OpenAI");
        assert!(kept.is_empty(), "each page has only one of the two topic words: {kept:?}");
        let both = vec![r("Sam Altman returns as CEO of OpenAI", "https://www.theverge.com/openai-ceo")];
        assert_eq!(on_topic(both, "Who is the CEO of OpenAI").len(), 1);
        assert!(on_topic(vec![r("OpenAI", "https://openai.com")], "").len() == 1);
    }

    #[test]
    fn wikipedia_articles_join_the_results_second() {
        let v = serde_json::json!({ "query": { "search": [
            { "title": "OpenAI", "snippet": "<span class=\"searchmatch\">OpenAI</span> is an American AI company" },
            { "title": "Sam Altman", "snippet": "CEO of OpenAI" }
        ]}});
        let wiki = parse_wikipedia(&v);
        assert_eq!(wiki[0].url, "https://en.wikipedia.org/wiki/OpenAI");
        assert_eq!(wiki[0].snippet, "OpenAI is an American AI company");
        assert_eq!(wiki[1].url, "https://en.wikipedia.org/wiki/Sam_Altman");
        let web = vec![r("OpenAI leadership", "https://openai.com/about"), r("News", "https://news.example/openai")];
        let mixed = blend(web, wiki);
        assert_eq!(mixed[1].url, "https://en.wikipedia.org/wiki/OpenAI");
        assert_eq!(mixed.len(), 3);
    }

    #[tokio::test]
    async fn the_cloud_answers_first_and_junk_falls_back() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let reply = |engine: &str, title: &str, href: &str| {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "engine": engine, "results": [{ "title": title, "body": "", "href": href }] }))
        };
        Mock::given(method("GET")).and(path("/api/search")).and(query_param("q", "steelers schedule")).respond_with(reply("searxng", "Steelers 2026 Schedule", "https://www.steelers.com/schedule/")).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/search")).and(query_param("q", "tesla model price")).respond_with(reply("ddgs", "Definition of MUCH", "https://www.merriam-webster.com/dictionary/much")).mount(&server).await;
        let cloud = crate::cloud::CloudClient::new(&server.uri(), "byte_test_key");
        let got = cloud_search(&cloud, "steelers schedule", 5).await.expect("searxng results are used");
        assert_eq!(got[0].url, "https://www.steelers.com/schedule/");
        // DuckDuckGo junk from the cloud's fallback: BYTE tries its own chain instead.
        assert!(cloud_search(&cloud, "tesla model price", 5).await.is_none());
    }

    #[test]
    fn dedupes_and_limits() {
        let r = |u: &str| SearchResult { title: "t".into(), url: u.into(), snippet: String::new() };
        let out = dedupe(vec![r("https://a.com/"), r("https://A.com"), r("https://b.com"), r("https://c.com")], 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].url, "https://b.com");
    }
}
