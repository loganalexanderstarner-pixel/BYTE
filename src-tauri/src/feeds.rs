//! News feeds (Phase 10): the user follows sites' RSS/Atom feeds and BYTE
//! catches them up on what's new, as a digest it puts together itself: the
//! headlines and summaries come from the feeds, never from a model.
//!
//! In chat: "follow theverge.com", "add https://example.com/feed.xml to my
//! feeds", "what's new in my feeds?", "what feeds do I follow?", "unfollow The
//! Verge". A site's own address works: BYTE finds the feed its page links to.
//! A digest on a schedule is a scheduled question ("every morning at 7, what's
//! new in my feeds"), so it runs like any other (scheduler.rs).

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools::SourceBook;

/// Items kept per feed (older ones are dropped).
const KEEP: i64 = 300;
/// Headlines per feed in a digest.
const PER_FEED: usize = 5;
/// New items shown after following a feed (the rest count as already seen).
const FIRST_UNSEEN: usize = 5;
/// Feeds a user can follow.
const MAX_FEEDS: i64 = 100;

// ------------------------------------------------------------------ parsing

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub guid: String,
    pub title: String,
    pub link: String,
    /// ms since 1970, if the feed said.
    pub published: Option<i64>,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub site: String,
    pub items: Vec<Item>,
}

/// Plain text from a bit of HTML (summaries are often HTML, and some feeds
/// escape entities twice, so "&#8216;" arrives as text).
fn plain(s: &str) -> String {
    let text = if s.contains('<') || s.contains('&') {
        let frag = scraper::Html::parse_fragment(s);
        frag.root_element().text().collect::<Vec<_>>().join(" ")
    } else {
        s.to_string()
    };
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    let cut = cut.rsplit_once(' ').map(|(a, _)| a.to_string()).unwrap_or(cut);
    format!("{}…", cut.trim_end_matches(|c: char| c.is_ascii_punctuation()))
}

fn date_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    chrono::DateTime::parse_from_rfc2822(s)
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(s))
        .map(|d| d.timestamp_millis())
        .ok()
        .or_else(|| chrono::NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()?.and_hms_opt(0, 0, 0).map(|d| d.and_utc().timestamp_millis()))
}

/// Reads an RSS 2.0, RSS 1.0 (RDF) or Atom feed. None: it isn't one.
pub fn parse_feed(xml: &str) -> Option<Parsed> {
    use quick_xml::events::{BytesStart, Event};
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Parsed::default();
    let mut is_feed = false;
    let mut item: Option<Item> = None;
    // Local name of the element whose text is being read, and the text so far.
    let mut field: Option<String> = None;
    let mut text = String::new();
    let mut depth_in_item = 0usize;

    fn local(e: &BytesStart<'_>) -> String {
        String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase()
    }
    fn attr(e: &BytesStart<'_>, name: &[u8]) -> Option<String> {
        e.attributes().flatten().find(|a| a.key.local_name().as_ref() == name).and_then(|a| a.unescape_value().ok()).map(|v| v.into_owned())
    }
    // Atom <link href rel>: the page itself is rel="alternate" (or no rel).
    fn atom_link(e: &BytesStart<'_>) -> Option<String> {
        let rel = attr(e, b"rel").unwrap_or_default();
        (rel.is_empty() || rel == "alternate").then(|| attr(e, b"href")).flatten()
    }

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local(&e);
                match name.as_str() {
                    "rss" | "feed" | "rdf" | "channel" => is_feed = true,
                    "item" | "entry" => {
                        item = Some(Item::default());
                        depth_in_item = 0;
                        continue;
                    }
                    _ => {}
                }
                if item.is_some() {
                    depth_in_item += 1;
                }
                if name == "link" {
                    if let Some(href) = atom_link(&e) {
                        match item.as_mut() {
                            Some(it) if it.link.is_empty() => it.link = href,
                            None if out.site.is_empty() => out.site = href,
                            _ => {}
                        }
                    }
                }
                let wanted = match (&item, name.as_str()) {
                    (Some(_), "title" | "link" | "guid" | "id" | "pubdate" | "published" | "updated" | "date" | "description" | "summary" | "encoded" | "content") => depth_in_item == 1,
                    (None, "title" | "link") => true,
                    _ => false,
                };
                if wanted {
                    field = Some(name);
                    text.clear();
                }
            }
            Ok(Event::Empty(e)) => {
                if local(&e) == "link" {
                    if let Some(href) = atom_link(&e) {
                        match item.as_mut() {
                            Some(it) if it.link.is_empty() => it.link = href,
                            None if out.site.is_empty() => out.site = href,
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Text(t)) if field.is_some() => {
                if let Ok(s) = t.unescape() {
                    text.push_str(&s);
                }
            }
            Ok(Event::CData(t)) if field.is_some() => text.push_str(&String::from_utf8_lossy(&t.into_inner())),
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase();
                if name == "item" || name == "entry" {
                    if let Some(mut it) = item.take() {
                        if it.guid.is_empty() {
                            it.guid = if it.link.is_empty() { it.title.clone() } else { it.link.clone() };
                        }
                        if !it.title.is_empty() || !it.summary.is_empty() {
                            if it.title.is_empty() {
                                it.title = short(&it.summary, 90);
                            }
                            out.items.push(it);
                        }
                    }
                    continue;
                }
                if field.as_deref() == Some(name.as_str()) {
                    field = None;
                    let t = text.trim().to_string();
                    match item.as_mut() {
                        Some(it) => match name.as_str() {
                            "title" => it.title = plain(&t),
                            "link" if it.link.is_empty() && !t.is_empty() => it.link = t,
                            "guid" | "id" => it.guid = t,
                            "pubdate" | "published" | "date" => it.published = it.published.or_else(|| date_ms(&t)),
                            "updated" => it.published = it.published.or_else(|| date_ms(&t)),
                            "description" | "summary" if it.summary.is_empty() => it.summary = short(&plain(&t), 280),
                            "encoded" | "content" if it.summary.is_empty() => it.summary = short(&plain(&t), 280),
                            _ => {}
                        },
                        None => match name.as_str() {
                            "title" if out.title.is_empty() => out.title = plain(&t),
                            "link" if out.site.is_empty() && !t.is_empty() => out.site = t,
                            _ => {}
                        },
                    }
                }
                if item.is_some() {
                    depth_in_item = depth_in_item.saturating_sub(1);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                // A broken end still leaves what was read.
                break;
            }
            _ => {}
        }
    }
    (is_feed && (!out.title.is_empty() || !out.items.is_empty())).then_some(out)
}

/// Feeds a web page points to (`<link rel="alternate" type="application/rss+xml">`).
pub fn discover(html: &str, base: &str) -> Vec<String> {
    let doc = scraper::Html::parse_document(html);
    let Ok(sel) = scraper::Selector::parse("link[rel~=alternate][href]") else { return vec![] };
    let base = url::Url::parse(base).ok();
    let mut out = Vec::new();
    for el in doc.select(&sel) {
        let ty = el.value().attr("type").unwrap_or("").to_ascii_lowercase();
        if !(ty.contains("rss") || ty.contains("atom")) {
            continue;
        }
        let href = el.value().attr("href").unwrap_or("");
        let abs = match &base {
            Some(b) => b.join(href).map(|u| u.to_string()).unwrap_or_default(),
            None => href.to_string(),
        };
        // Comment feeds are rarely what people want.
        if !abs.is_empty() && !abs.contains("/comments/") && !out.contains(&abs) {
            out.push(abs);
        }
    }
    out
}

/// A web address in a message: "https://…", or a bare "theverge.com/tech".
pub fn url_in(text: &str) -> Option<String> {
    for w in text.split_whitespace() {
        let w = w.trim_matches(|c: char| matches!(c, '"' | '\'' | '<' | '>' | '(' | ')' | ',' | '“' | '”')).trim_end_matches(['.', '?', '!']);
        if w.starts_with("http://") || w.starts_with("https://") {
            return url::Url::parse(w).ok().map(|u| u.to_string());
        }
        let host = w.split('/').next().unwrap_or("");
        let labels: Vec<&str> = host.split('.').collect();
        let tld_ok = labels.last().is_some_and(|t| t.len() >= 2 && t.chars().all(|c| c.is_ascii_alphabetic()));
        if labels.len() >= 2 && tld_ok && labels.iter().all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) && !w.contains('@') {
            return url::Url::parse(&format!("https://{w}")).ok().map(|u| u.to_string());
        }
    }
    None
}

// ------------------------------------------------------------------ storing

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub site: String,
    pub added: i64,
    pub last_checked: Option<i64>,
    pub last_error: String,
    /// New items not yet in a digest.
    pub unseen: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub id: i64,
    pub feed_id: i64,
    pub title: String,
    pub link: String,
    pub published: Option<i64>,
    pub summary: String,
}

fn row_feed(r: &rusqlite::Row<'_>) -> rusqlite::Result<Feed> {
    Ok(Feed { id: r.get(0)?, url: r.get(1)?, title: r.get(2)?, site: r.get(3)?, added: r.get(4)?, last_checked: r.get(5)?, last_error: r.get(6)?, unseen: r.get(7)? })
}

const FEED_COLS: &str = "f.id, f.url, f.title, f.site, f.added, f.last_checked, f.last_error, (SELECT COUNT(*) FROM feed_items i WHERE i.feed_id = f.id AND i.seen = 0)";

pub fn list(db: &Db) -> AppResult<Vec<Feed>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {FEED_COLS} FROM feeds f ORDER BY f.title COLLATE NOCASE"))?;
    let v = st.query_map([], row_feed)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

pub fn get(db: &Db, id: i64) -> AppResult<Option<Feed>> {
    Ok(db.conn().query_row(&format!("SELECT {FEED_COLS} FROM feeds f WHERE f.id = ?1"), [id], row_feed).optional()?)
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM feed_items WHERE feed_id = ?1", [id])?;
    conn.execute("DELETE FROM feeds WHERE id = ?1", [id])?;
    Ok(())
}

/// Stores a feed's items; returns how many were new. With `first`, only the
/// newest few count as unseen (so the first digest isn't a flood).
pub fn store_items(db: &Db, feed_id: i64, items: &[Item], now: i64, first: bool) -> AppResult<usize> {
    let conn = db.conn();
    let mut newest: Vec<&Item> = items.iter().collect();
    newest.sort_by_key(|i| std::cmp::Reverse(i.published.unwrap_or(0)));
    let mut added = 0;
    for (rank, it) in newest.iter().enumerate() {
        let seen = first && rank >= FIRST_UNSEEN;
        let n = conn.execute(
            "INSERT OR IGNORE INTO feed_items (feed_id, guid, title, link, published, summary, seen, fetched) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![feed_id, it.guid.chars().take(500).collect::<String>(), it.title.chars().take(300).collect::<String>(), it.link, it.published, it.summary, seen as i64, now],
        )?;
        added += n;
    }
    conn.execute(
        "DELETE FROM feed_items WHERE feed_id = ?1 AND id NOT IN (SELECT id FROM feed_items WHERE feed_id = ?1 ORDER BY COALESCE(published, fetched) DESC, id DESC LIMIT ?2)",
        params![feed_id, KEEP],
    )?;
    Ok(added)
}

/// Adds a feed that was read successfully; following it again just refreshes it.
pub fn add(db: &Db, url: &str, parsed: &Parsed, now: i64) -> AppResult<(Feed, bool)> {
    let existing: Option<i64> = db.conn().query_row("SELECT id FROM feeds WHERE url = ?1", [url], |r| r.get(0)).optional()?;
    let (id, fresh) = match existing {
        Some(id) => (id, false),
        None => {
            let count: i64 = db.conn().query_row("SELECT COUNT(*) FROM feeds", [], |r| r.get(0))?;
            if count >= MAX_FEEDS {
                return Err(AppError::msg(format!("BYTE follows up to {MAX_FEEDS} feeds. Unfollow one first.")));
            }
            let title = if parsed.title.trim().is_empty() { url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| url.to_string()) } else { parsed.title.trim().chars().take(120).collect() };
            db.conn().execute("INSERT INTO feeds (url, title, site, added, last_checked) VALUES (?1, ?2, ?3, ?4, ?4)", params![url, title, parsed.site, now])?;
            (db.conn().last_insert_rowid(), true)
        }
    };
    store_items(db, id, &parsed.items, now, fresh)?;
    Ok((get(db, id)?.ok_or_else(|| AppError::msg("The feed wasn't saved."))?, fresh))
}

fn checked(db: &Db, id: i64, now: i64, error: &str) -> AppResult<()> {
    db.conn().execute("UPDATE feeds SET last_checked = ?1, last_error = ?2 WHERE id = ?3", params![now, error, id])?;
    Ok(())
}

/// Items not yet in a digest, newest first, grouped by feed.
pub fn unseen(db: &Db) -> AppResult<Vec<(Feed, Vec<FeedItem>)>> {
    let mut out = Vec::new();
    for f in list(db)?.into_iter().filter(|f| f.unseen > 0) {
        let items = {
            let conn = db.conn();
            let mut st = conn.prepare("SELECT id, feed_id, title, link, published, summary FROM feed_items WHERE feed_id = ?1 AND seen = 0 ORDER BY COALESCE(published, fetched) DESC, id DESC")?;
            let v = st
                .query_map([f.id], |r| Ok(FeedItem { id: r.get(0)?, feed_id: r.get(1)?, title: r.get(2)?, link: r.get(3)?, published: r.get(4)?, summary: r.get(5)? }))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        out.push((f, items));
    }
    // Busiest feeds first.
    out.sort_by_key(|(_, items)| std::cmp::Reverse(items.len()));
    Ok(out)
}

pub fn mark_seen(db: &Db, feed_ids: &[i64]) -> AppResult<()> {
    let conn = db.conn();
    for id in feed_ids {
        conn.execute("UPDATE feed_items SET seen = 1 WHERE feed_id = ?1", [id])?;
    }
    Ok(())
}

// ------------------------------------------------------------------ fetching

/// Reads a feed address, or finds the feed a site's page links to.
/// Returns the feed's own address and what it holds.
pub async fn resolve(net: &reqwest::Client, url: &str) -> AppResult<(String, Parsed)> {
    let (final_url, body, html) = crate::tools::fetch::fetch_raw(net, url).await?;
    if !html {
        if let Some(p) = parse_feed(&body) {
            return Ok((final_url, p));
        }
    }
    let mut candidates = discover(&body, &final_url);
    if candidates.is_empty() && html {
        // Common places for a site's feed.
        if let Ok(base) = url::Url::parse(&final_url) {
            candidates = ["/feed", "/rss", "/feed.xml", "/rss.xml", "/atom.xml", "/index.xml"].iter().filter_map(|p| base.join(p).ok().map(|u| u.to_string())).collect();
        }
    }
    for c in candidates.into_iter().take(6) {
        if let Ok((u, body, false)) = crate::tools::fetch::fetch_raw(net, &c).await {
            if let Some(p) = parse_feed(&body) {
                return Ok((u, p));
            }
        }
    }
    Err(AppError::msg("BYTE couldn't find a news feed (RSS or Atom) at that address."))
}

/// Checks every feed for new items; returns how many arrived and which feeds failed.
pub async fn refresh_all(db: &Db, net: &reqwest::Client) -> (usize, Vec<String>) {
    let feeds = list(db).unwrap_or_default();
    let mut new = 0;
    let mut failed = Vec::new();
    let now = chrono::Utc::now().timestamp_millis();
    for f in feeds {
        let got = crate::tools::fetch::fetch_raw(net, &f.url).await.and_then(|(_, body, _)| parse_feed(&body).ok_or_else(|| AppError::msg("it isn't a feed any more")));
        match got {
            Ok(p) => {
                new += store_items(db, f.id, &p.items, now, false).unwrap_or(0);
                let _ = checked(db, f.id, now, "");
            }
            Err(e) => {
                let _ = checked(db, f.id, now, &e.to_string());
                failed.push(f.title.clone());
            }
        }
    }
    (new, failed)
}

// ------------------------------------------------------------------ digest

fn when(ms: Option<i64>, now: i64) -> String {
    let Some(ms) = ms else { return String::new() };
    let mins = (now - ms) / 60_000;
    match mins {
        m if m < 0 => String::new(),
        m if m < 60 => format!("{}m ago", m.max(1)),
        m if m < 24 * 60 => format!("{}h ago", m / 60),
        m if m < 7 * 24 * 60 => format!("{}d ago", m / (24 * 60)),
        _ => String::new(),
    }
}

/// The digest, written by BYTE from the feeds (every headline has its source).
pub fn compose(groups: &[(Feed, Vec<FeedItem>)], book: &mut SourceBook, failed: &[String], now: i64) -> String {
    let total: usize = groups.iter().map(|(_, i)| i.len()).sum();
    let mut out = Vec::new();
    if total == 0 {
        out.push("> **TL;DR:** Nothing new in your feeds since the last digest.".to_string());
    } else {
        let names: Vec<&str> = groups.iter().take(3).map(|(f, _)| f.title.as_str()).collect();
        let more = if groups.len() > 3 { format!(" and {} more", groups.len() - 3) } else { String::new() };
        out.push(format!("> **TL;DR:** {total} new {} from {}{more}.", if total == 1 { "story" } else { "stories" }, names.join(", ")));
    }
    for (feed, items) in groups {
        out.push(String::new());
        out.push(format!("### {}", feed.title));
        for it in items.iter().take(PER_FEED) {
            let n = book.add(&it.title, &it.link, &it.summary);
            let age = when(it.published, now);
            let age = if age.is_empty() { String::new() } else { format!(" · {age}") };
            let gist = if it.summary.is_empty() || it.summary == it.title { String::new() } else { format!(" — {}", short(&it.summary, 160)) };
            out.push(format!("- **{}**{gist}{age} [{n}]", it.title));
        }
        if items.len() > PER_FEED {
            out.push(format!("- *…and {} more*", items.len() - PER_FEED));
        }
    }
    if !failed.is_empty() {
        out.push(String::new());
        out.push(format!("*Couldn't read: {}.*", failed.join(", ")));
    }
    out.join("\n")
}

// ------------------------------------------------------------------ in chat

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Follow(String),
    Unfollow(String),
    List,
    Digest,
}

pub fn ask(q: &str) -> Option<Ask> {
    let l = q.trim().to_lowercase().replace('’', "'");
    let l = l.trim_end_matches(['?', '.', '!']).trim();
    let feedish = l.contains("feed") || l.contains("rss");
    // Following: needs an address.
    if let Some(rest) = ["follow ", "subscribe to ", "add the feed ", "add feed ", "add the rss feed ", "add rss "].iter().find_map(|p| l.strip_prefix(p)) {
        if !rest.starts_with("up") {
            if let Some(u) = url_in(rest) {
                return Some(Ask::Follow(u));
            }
        }
    }
    if l.starts_with("add ") && (l.ends_with("to my feeds") || l.ends_with("to my news feeds") || l.ends_with("to my rss feeds")) {
        return url_in(l).map(Ask::Follow);
    }
    // Unfollowing.
    for p in ["unfollow ", "unsubscribe from ", "stop following "] {
        if let Some(rest) = l.strip_prefix(p) {
            let what = rest.trim_end_matches(" feed").trim_end_matches(" in my feeds").trim();
            if !what.is_empty() {
                return Some(Ask::Unfollow(what.to_string()));
            }
        }
    }
    if let Some(rest) = l.strip_prefix("remove ").filter(|r| r.ends_with(" from my feeds")) {
        return Some(Ask::Unfollow(rest.trim_end_matches(" from my feeds").trim().to_string()));
    }
    if !feedish {
        return None;
    }
    if ["what feeds", "which feeds", "list my feeds", "show my feeds", "feeds do i follow", "my feed list", "feeds am i following"].iter().any(|w| l.contains(w)) {
        return Some(Ask::List);
    }
    if ["what's new in my", "what is new in my", "anything new in my", "catch me up on my", "read my feeds", "my feeds digest", "feed digest", "news from my feeds", "check my feeds", "digest of my feeds", "go through my feeds", "summarize my feeds", "my rss"].iter().any(|w| l.contains(w)) {
        return Some(Ask::Digest);
    }
    None
}

pub fn applies(q: &str) -> bool {
    ask(q).is_some()
}

/// Which feed a phrase means (title, then address).
pub fn find<'a>(feeds: &'a [Feed], what: &str) -> Option<&'a Feed> {
    let w = what.trim_start_matches("the ").to_lowercase();
    feeds
        .iter()
        .find(|f| f.title.to_lowercase() == w)
        .or_else(|| feeds.iter().find(|f| f.title.to_lowercase().contains(&w) && w.len() >= 3))
        .or_else(|| feeds.iter().find(|f| f.url.to_lowercase().contains(&w) && w.len() >= 4))
}

fn feeds_notes(feeds: &[Feed]) -> String {
    if feeds.is_empty() {
        return "The user doesn't follow any feeds yet. They can say \"follow theverge.com\" or add one in the ✅ panel.".into();
    }
    let lines: Vec<String> = feeds.iter().map(|f| format!("- {} ({}){}", f.title, f.url, if f.unseen > 0 { format!(", {} new", f.unseen) } else { String::new() })).collect();
    format!("The feeds the user follows in BYTE ({}):\n{}", feeds.len(), lines.join("\n"))
}

/// The kind is "feeds_digest" when the reply is BYTE's own digest (sent as is).
pub async fn run(turn: &Turn<'_>, db: &Db, question: &str, send: Emit<'_>) -> AppResult<Option<(SourceBook, String, &'static str)>> {
    let Some(a) = ask(question) else { return Ok(None) };
    let none = SourceBook::default;
    let id = format!("byte_feeds_{}", uuid::Uuid::new_v4().simple());
    let now = chrono::Utc::now().timestamp_millis();
    match a {
        Ask::Follow(url) => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "feed_follow".into(), args: json!({ "url": url }) })?;
            if !turn.web {
                send(ChatEvent::ToolResult { id, ok: false, summary: "Web access is off".into() })?;
                return Ok(Some((none(), "Following a feed needs web access, which is off in Settings. Say so in one sentence.".into(), "feeds")));
            }
            match resolve(turn.net, &url).await {
                Ok((feed_url, parsed)) => {
                    let (f, fresh) = add(db, &feed_url, &parsed, now)?;
                    let detail = format!("{} · {} recent {}", f.url, parsed.items.len(), if parsed.items.len() == 1 { "story" } else { "stories" });
                    send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
                    turn.log.record("feed_follow", &json!({ "url": f.url }), true, &detail);
                    send(ChatEvent::MacDone(crate::macctl::MacDone { app: "BYTE".into(), title: format!("{} \"{}\"", if fresh { "Following" } else { "Already following" }, f.title), detail, ok: true, undo: None }))?;
                    let latest: Vec<String> = parsed.items.iter().take(3).map(|i| format!("- {}", i.title)).collect();
                    Ok(Some((none(), format!("Done: BYTE {} the feed \"{}\". Its latest headlines:\n{}\nConfirm in one or two short sentences and say \"what's new in my feeds?\" gives a digest.", if fresh { "now follows" } else { "already followed" }, f.title, latest.join("\n")), "feeds")))
                }
                Err(e) => {
                    send(ChatEvent::ToolResult { id, ok: false, summary: e.to_string() })?;
                    Ok(Some((none(), format!("Following {url} didn't work: {e} Explain briefly and suggest pasting the site's RSS or Atom feed address."), "feeds")))
                }
            }
        }
        Ask::Unfollow(what) => {
            let feeds = list(db)?;
            let Some(f) = find(&feeds, &what).cloned() else {
                return Ok(Some((none(), format!("No followed feed matches \"{what}\". {}", feeds_notes(&feeds)), "feeds")));
            };
            delete(db, f.id)?;
            send(ChatEvent::ToolCall { id: id.clone(), name: "feed_unfollow".into(), args: json!({ "title": f.title }) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: "Unfollowed".into() })?;
            Ok(Some((none(), format!("Done: BYTE no longer follows \"{}\". Confirm in one short sentence.", f.title), "feeds")))
        }
        Ask::List => {
            let feeds = list(db)?;
            send(ChatEvent::ToolCall { id: id.clone(), name: "feed_list".into(), args: json!({}) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} feeds", feeds.len()) })?;
            Ok(Some((none(), format!("{}\n\nList them briefly.", feeds_notes(&feeds)), "feeds")))
        }
        Ask::Digest => {
            let feeds = list(db)?;
            if feeds.is_empty() {
                return Ok(Some((none(), feeds_notes(&feeds), "feeds")));
            }
            send(ChatEvent::ToolCall { id: id.clone(), name: "feed_digest".into(), args: json!({ "what": format!("{} feeds", feeds.len()) }) })?;
            let failed = if turn.web { refresh_all(db, turn.net).await.1 } else { Vec::new() };
            let groups = unseen(db)?;
            let mut book = SourceBook::default();
            let md = compose(&groups, &mut book, &failed, now);
            mark_seen(db, &groups.iter().map(|(f, _)| f.id).collect::<Vec<_>>())?;
            let total: usize = groups.iter().map(|(_, i)| i.len()).sum();
            send(ChatEvent::ToolResult { id, ok: failed.len() < feeds.len(), summary: format!("{total} new") })?;
            Ok(Some((book, md, "feeds_digest")))
        }
    }
}

// ----------------------------------------------------------------- commands

#[tauri::command]
pub fn feeds_list(state: State<'_, AppState>) -> AppResult<Vec<Feed>> {
    list(&state.db)
}

#[tauri::command]
pub async fn feed_follow(state: State<'_, AppState>, url: String) -> AppResult<Feed> {
    let url = url_in(&url).ok_or_else(|| AppError::msg("That doesn't look like a web address."))?;
    let (feed_url, parsed) = resolve(&state.net, &url).await?;
    Ok(add(&state.db, &feed_url, &parsed, chrono::Utc::now().timestamp_millis())?.0)
}

#[tauri::command]
pub fn feed_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete(&state.db, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:dc="http://purl.org/dc/elements/1.1/">
<channel>
  <title>Example News</title>
  <link>https://example.com/</link>
  <description>All the news</description>
  <image><title>Logo title</title><link>https://example.com/logo</link></image>
  <item>
    <title>First &amp; best story</title>
    <link>https://example.com/a</link>
    <guid isPermaLink="false">a-1</guid>
    <pubDate>Wed, 30 Sep 2026 08:00:00 GMT</pubDate>
    <description><![CDATA[<p>The <b>first</b> story.</p>]]></description>
  </item>
  <item>
    <title>&amp;#8216;Second&amp;#8217; story</title>
    <link>https://example.com/b</link>
    <dc:date>2026-09-29T10:00:00Z</dc:date>
    <content:encoded><![CDATA[<div>Longer body</div>]]></content:encoded>
  </item>
</channel>
</rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Blog</title>
  <link href="https://blog.example.org/feed.xml" rel="self"/>
  <link href="https://blog.example.org/"/>
  <entry>
    <title type="html">Hello &lt;em&gt;world&lt;/em&gt;</title>
    <link rel="alternate" href="https://blog.example.org/hello"/>
    <id>tag:blog.example.org,2026:hello</id>
    <updated>2026-09-28T12:00:00+02:00</updated>
    <summary>Short summary</summary>
  </entry>
</feed>"#;

    #[test]
    fn rss_and_atom_are_read() {
        let p = parse_feed(RSS).unwrap();
        assert_eq!(p.title, "Example News");
        assert_eq!(p.site, "https://example.com/");
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[0].title, "First & best story");
        assert_eq!(p.items[0].guid, "a-1");
        assert_eq!(p.items[0].summary, "The first story.");
        assert_eq!(p.items[0].published, Some(chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z").unwrap().timestamp_millis()));
        assert_eq!(p.items[1].title, "‘Second’ story", "entities escaped twice");
        assert_eq!(p.items[1].guid, "https://example.com/b", "no guid: the link");
        assert_eq!(p.items[1].summary, "Longer body");
        assert!(p.items[1].published.is_some());

        let a = parse_feed(ATOM).unwrap();
        assert_eq!(a.title, "Atom Blog");
        assert_eq!(a.site, "https://blog.example.org/", "rel=self is the feed, not the site");
        assert_eq!(a.items[0].title, "Hello world");
        assert_eq!(a.items[0].link, "https://blog.example.org/hello");
        assert_eq!(a.items[0].guid, "tag:blog.example.org,2026:hello");
        assert_eq!(a.items[0].summary, "Short summary");

        assert!(parse_feed("<html><head><title>Not a feed</title></head></html>").is_none());
        assert!(parse_feed("plain text").is_none());
    }

    #[test]
    fn feeds_are_found_on_pages_and_addresses_in_words() {
        let html = r#"<html><head>
            <link rel="alternate" type="application/rss+xml" title="Main" href="/feed/">
            <link rel="alternate" type="application/atom+xml" href="https://x.org/atom.xml">
            <link rel="alternate" type="application/rss+xml" href="/comments/feed/">
            <link rel="alternate" hreflang="de" href="/de/">
        </head></html>"#;
        assert_eq!(discover(html, "https://x.org/blog/"), vec!["https://x.org/feed/", "https://x.org/atom.xml"]);
        assert_eq!(url_in("follow theverge.com please").as_deref(), Some("https://theverge.com/"));
        assert_eq!(url_in("add https://example.com/rss.xml, thanks").as_deref(), Some("https://example.com/rss.xml"));
        assert_eq!(url_in("follow my heart"), None);
        assert_eq!(url_in("email me at a@b.com"), None);
        assert_eq!(url_in("version 1.2 is out"), None);
    }

    #[test]
    fn requests_are_understood() {
        assert_eq!(ask("follow theverge.com"), Some(Ask::Follow("https://theverge.com/".into())));
        assert_eq!(ask("Subscribe to https://example.com/feed.xml"), Some(Ask::Follow("https://example.com/feed.xml".into())));
        assert_eq!(ask("add arstechnica.com to my feeds"), Some(Ask::Follow("https://arstechnica.com/".into())));
        assert_eq!(ask("unfollow The Verge"), Some(Ask::Unfollow("the verge".into())));
        assert_eq!(ask("remove ars technica from my feeds"), Some(Ask::Unfollow("ars technica".into())));
        assert_eq!(ask("what's new in my feeds?"), Some(Ask::Digest));
        assert_eq!(ask("catch me up on my RSS feeds"), Some(Ask::Digest));
        assert_eq!(ask("what feeds do I follow?"), Some(Ask::List));
        for q in ["follow up with Sam", "follow the recipe", "what is an RSS feed", "how do I feed my cat", "follow my heart"] {
            assert_eq!(ask(q), None, "{q}");
        }
    }

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        (dir, db)
    }

    fn item(n: i64) -> Item {
        Item { guid: format!("g{n}"), title: format!("Story {n}"), link: format!("https://e.com/{n}"), published: Some(n * 1000), summary: String::new() }
    }

    #[test]
    fn new_items_are_stored_once_and_digested_once() {
        let (_d, db) = db();
        let parsed = Parsed { title: "E".into(), site: "https://e.com".into(), items: (1..=8).map(item).collect() };
        let (f, fresh) = add(&db, "https://e.com/feed", &parsed, 10_000).unwrap();
        assert!(fresh);
        assert_eq!(f.unseen, 5, "only the newest few count as new at first");
        let (again, fresh) = add(&db, "https://e.com/feed", &parsed, 10_000).unwrap();
        assert!(!fresh);
        assert_eq!(again.id, f.id);
        assert_eq!(store_items(&db, f.id, &[item(8), item(9)], 11_000, false).unwrap(), 1, "only story 9 is new");
        let groups = unseen(&db).unwrap();
        assert_eq!(groups[0].1.len(), 6);
        assert_eq!(groups[0].1[0].title, "Story 9", "newest first");
        let mut book = SourceBook::default();
        let md = compose(&groups, &mut book, &[], 12_000);
        assert!(md.starts_with("> **TL;DR:** 6 new stories from E."), "{md}");
        assert!(md.contains("- **Story 9** · 1m ago [1]"), "{md}");
        assert!(md.contains("*…and 1 more*"), "{md}");
        assert_eq!(book.sources.len(), 5);
        mark_seen(&db, &[f.id]).unwrap();
        assert!(unseen(&db).unwrap().is_empty());
        let md = compose(&[], &mut SourceBook::default(), &["Broken".into()], 12_000);
        assert!(md.contains("Nothing new") && md.contains("Couldn't read: Broken."), "{md}");
        assert_eq!(find(&list(&db).unwrap(), "e.com/feed").map(|x| x.id), Some(f.id));
        delete(&db, f.id).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }

    /// Real feeds on the internet: `cargo test e2e_real_feeds -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn e2e_real_feeds() {
        let net = crate::tools::fetch::web_client();
        for url in ["https://hnrss.org/frontpage", "https://blog.rust-lang.org/", "https://www.theverge.com/", "https://feeds.bbci.co.uk/news/rss.xml", "https://github.blog/"] {
            match resolve(&net, url).await {
                Ok((u, p)) => {
                    println!("{url} -> {u}: \"{}\" ({}), {} items; first: {:?}", p.title, p.site, p.items.len(), p.items.first().map(|i| (&i.title, &i.link, i.published.is_some(), i.summary.chars().take(60).collect::<String>())));
                    assert!(!p.items.is_empty(), "{url}");
                    assert!(p.items.iter().all(|i| !i.title.is_empty() && !i.guid.is_empty()), "{url}");
                }
                Err(e) => println!("{url}: ERROR {e}"),
            }
        }
    }
}
