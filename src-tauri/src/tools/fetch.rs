//! Reads a web page: fetches it safely, extracts the main article text, and
//! keeps the passages most relevant to the user's question.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::tools::search::BROWSER_UA;

const MAX_BYTES: usize = 3 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);
const CACHE_TTL: Duration = Duration::from_secs(24 * 3600);
const CACHE_MAX: usize = 200;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub url: String,
    pub title: String,
    pub text: String,
}

/// 24-hour in-memory cache of extracted pages.
static CACHE: Mutex<Option<HashMap<String, (Instant, Page)>>> = Mutex::const_new(None);

pub async fn fetch_page(client: &reqwest::Client, raw_url: &str) -> AppResult<Page> {
    let url = check_url(raw_url).await?;
    if let Some(p) = cache_get(url.as_str()).await {
        return Ok(p);
    }
    let resp = client
        .get(url.clone())
        .header(reqwest::header::USER_AGENT, BROWSER_UA)
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.5")
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .timeout(TIMEOUT)
        .send()
        .await?;
    // Redirects may lead somewhere private; check the final address too.
    let final_url = check_url(resp.url().as_str()).await?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!("the page returned {}", resp.status())));
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ctype.contains("pdf") {
        return Err(AppError::msg("this link is a PDF; BYTE can't read PDFs from the web yet"));
    }
    if !(ctype.is_empty() || ctype.contains("html") || ctype.contains("text") || ctype.contains("xml")) {
        return Err(AppError::msg(format!("unsupported content type {ctype}")));
    }

    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BYTES {
            break;
        }
    }
    let html = String::from_utf8_lossy(&body).into_owned();
    let page = if ctype.contains("html") || ctype.is_empty() {
        extract(&html, final_url.as_str())
    } else {
        Page { url: final_url.to_string(), title: final_url.to_string(), text: html }
    };
    // Pages that are an app shell (their content arrives by JavaScript) leave
    // almost no text; treat them as unreadable so another result is read instead.
    if page.text.trim().len() < 300 {
        return Err(AppError::msg("couldn't find readable text on that page"));
    }
    cache_put(url.as_str(), page.clone()).await;
    Ok(page)
}

/// Sites whose pages can't be read without signing in or running their app
/// (BYTE would get a login wall or an empty shell).
const UNREADABLE_HOSTS: &[&str] = &[
    "whatsapp.com", "facebook.com", "instagram.com", "x.com", "twitter.com", "tiktok.com", "linkedin.com",
    "pinterest.com", "youtube.com", "youtu.be", "threads.net", "snapchat.com", "discord.com", "apps.apple.com",
    "play.google.com", "quora.com",
];

/// Whether a search result is worth reading (not a login wall, app, video or file).
pub fn worth_reading(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false; // e.g. passages from the user's files (file://)
    }
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let path = u.path().to_ascii_lowercase();
    !UNREADABLE_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
        && ![".pdf", ".zip", ".mp4", ".mp3", ".dmg", ".exe"].iter().any(|x| path.ends_with(x))
}

/// Extracts the main article with a Readability port, falling back to all
/// visible text when the page isn't article-shaped.
pub fn extract(html: &str, url: &str) -> Page {
    let article = dom_smoothie::Readability::new(html, Some(url), None).ok().and_then(|mut r| r.parse().ok());
    let (title, text) = match article {
        Some(a) if a.text_content.trim().len() > 200 => (a.title.clone(), a.text_content.to_string()),
        other => {
            let doc = scraper::Html::parse_document(html);
            let body = scraper::Selector::parse("body").expect("selector");
            let text = doc.select(&body).next().map(|b| b.text().collect::<Vec<_>>().join(" ")).unwrap_or_default();
            (other.map(|a| a.title).unwrap_or_default(), text)
        }
    };
    Page { url: url.to_string(), title: clean_ws(&title), text: tidy(&text) }
}

fn clean_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collapses whitespace but keeps paragraph breaks.
fn tidy(text: &str) -> String {
    text.split("\n")
        .map(clean_ws)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keeps the passages that best match `query`, in document order, up to
/// `budget` characters. Always includes the opening passage for context.
pub fn relevant_passages(text: &str, query: &str, budget: usize) -> String {
    if text.len() <= budget {
        return text.to_string();
    }
    let terms: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(w))
        .map(str::to_string)
        .collect();
    // Split into ~500-character passages on line boundaries.
    let mut passages: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if cur.len() + line.len() > 500 && !cur.is_empty() {
            passages.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.is_empty() {
        passages.push(cur);
    }
    let score = |p: &str| -> f64 {
        let lower = p.to_lowercase();
        let hits: usize = terms.iter().map(|t| lower.matches(t.as_str()).count().min(4)).sum();
        let distinct = terms.iter().filter(|t| lower.contains(t.as_str())).count();
        hits as f64 + 2.0 * distinct as f64 + if p.chars().any(|c| c.is_ascii_digit()) { 0.5 } else { 0.0 }
    };
    let mut ranked: Vec<(usize, f64)> = passages.iter().enumerate().map(|(i, p)| (i, score(p))).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut keep = vec![0usize];
    let mut used = passages[0].len();
    for (i, _) in ranked {
        if keep.contains(&i) {
            continue;
        }
        if used + passages[i].len() > budget {
            continue;
        }
        used += passages[i].len();
        keep.push(i);
    }
    keep.sort_unstable();
    let mut out = String::new();
    let mut prev: Option<usize> = None;
    for i in keep {
        if prev.is_some_and(|p| p + 1 != i) {
            out.push_str("\n…\n");
        } else if prev.is_some() {
            out.push('\n');
        }
        out.push_str(&passages[i]);
        prev = Some(i);
    }
    out
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "any", "can", "had", "her", "was", "one", "our", "out",
    "has", "have", "what", "when", "where", "which", "who", "why", "how", "with", "this", "that", "from", "they",
    "will", "would", "there", "their", "about", "into", "than", "then", "them", "these", "those", "does", "did",
];

/// Rejects anything that isn't a public http(s) address, so a web page or the
/// model can't make BYTE read local services (the engine, a router, etc.).
pub async fn check_url(raw: &str) -> AppResult<url::Url> {
    let url = url::Url::parse(raw.trim()).map_err(|_| AppError::msg(format!("not a valid web address: {raw}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::msg("only http and https links can be read"));
    }
    let host = url.host_str().ok_or_else(|| AppError::msg("the link has no host"))?.to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".localhost") || host.ends_with(".internal") {
        return Err(AppError::msg("links to this computer or the local network can't be read"));
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<IpAddr> = match host.trim_matches(['[', ']']).parse::<IpAddr>() {
        Ok(ip) => vec![ip],
        Err(_) => tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| AppError::msg(format!("couldn't find the website {host}")))?
            .map(|a| a.ip())
            .collect(),
    };
    if addrs.is_empty() || addrs.iter().any(|ip| !is_public(ip)) {
        return Err(AppError::msg("links to this computer or the local network can't be read"));
    }
    Ok(url)
}

pub fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])) // carrier-grade NAT
                || o[0] >= 224)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(&IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || (s[0] & 0xff00) == 0xff00) // multicast
        }
    }
}

async fn cache_get(url: &str) -> Option<Page> {
    let guard = CACHE.lock().await;
    let (at, page) = guard.as_ref()?.get(url)?;
    (at.elapsed() < CACHE_TTL).then(|| page.clone())
}

async fn cache_put(url: &str, page: Page) {
    let mut guard = CACHE.lock().await;
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= CACHE_MAX {
        if let Some(oldest) = map.iter().min_by_key(|(_, (t, _))| *t).map(|(k, _)| k.clone()) {
            map.remove(&oldest);
        }
    }
    map.insert(url.to_string(), (Instant::now(), page));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_results_that_cannot_be_read() {
        assert!(!worth_reading("https://web.whatsapp.com/"));
        assert!(!worth_reading("https://www.youtube.com/watch?v=x"));
        assert!(!worth_reading("https://example.com/report.pdf"));
        assert!(worth_reading("https://www.weather.gov/pbz/"));
        assert!(worth_reading("https://www.steelers.com/schedule/"));
        assert!(!worth_reading("not a url"));
    }

    #[test]
    fn extracts_article_from_real_page() {
        let html = std::fs::read_to_string(format!("{}/tests/fixtures/article.html", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let p = extract(&html, "https://v2.tauri.app/blog/tauri-20/");
        assert!(p.title.to_lowercase().contains("tauri"), "{}", p.title);
        assert!(p.text.len() > 2000, "{}", p.text.len());
        assert!(!p.text.contains("<div"), "html leaked into text");
    }

    #[test]
    fn passages_prefer_relevant_text_within_budget() {
        let mut text = String::from("Intro paragraph about the topic.\n");
        for i in 0..40 {
            text.push_str(&format!("Filler paragraph number {i} talking about unrelated gardening advice and weather.\n"));
        }
        text.push_str("The battery capacity of the M4 MacBook Air is 53.8 watt-hours.\n");
        for i in 0..40 {
            text.push_str(&format!("More filler {i} about cooking pasta and travel plans.\n"));
        }
        let out = relevant_passages(&text, "What is the M4 MacBook Air battery capacity?", 1200);
        assert!(out.len() <= 1300, "{}", out.len());
        assert!(out.starts_with("Intro paragraph"));
        assert!(out.contains("53.8 watt-hours"));
    }

    #[test]
    fn short_text_is_returned_whole() {
        assert_eq!(relevant_passages("short", "q", 1000), "short");
    }

    #[test]
    fn private_addresses_are_blocked() {
        for ip in ["127.0.0.1", "10.1.2.3", "192.168.1.1", "172.16.5.4", "169.254.1.1", "100.64.0.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:192.168.0.1"] {
            assert!(!is_public(&ip.parse().unwrap()), "{ip} should be blocked");
        }
        for ip in ["1.1.1.1", "142.250.72.14", "2606:4700:4700::1111"] {
            assert!(is_public(&ip.parse().unwrap()), "{ip} should be allowed");
        }
    }

    #[tokio::test]
    async fn check_url_rejects_local_and_odd_schemes() {
        for u in ["http://localhost:8080/", "http://127.0.0.1:1234/v1", "http://[::1]/", "file:///etc/passwd", "ftp://x.com", "http://printer.local/", "http://192.168.1.1/admin"] {
            assert!(check_url(u).await.is_err(), "{u} should be rejected");
        }
    }
}

/// DNS resolver that only returns public addresses. Used by every internet
/// client, so redirects and DNS rebinding can't reach local services either.
struct PublicOnlyResolver;

impl reqwest::dns::Resolve for PublicOnlyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs: Vec<std::net::SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.filter(|a| is_public(&a.ip())).collect();
            if addrs.is_empty() {
                return Err(format!("{host} resolves only to private addresses").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// HTTP client for the internet (search, pages, model downloads).
pub fn web_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("BYTE/", env!("CARGO_PKG_VERSION"), " (macOS; local AI assistant)"))
        .dns_resolver(std::sync::Arc::new(PublicOnlyResolver))
        .redirect(reqwest::redirect::Policy::limited(8))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("web client")
}
