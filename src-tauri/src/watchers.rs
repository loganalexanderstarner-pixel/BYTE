//! Page watchers (Phase 10): BYTE checks a page every few hours and sends a
//! notification when it changes, or when a product's price drops (below a
//! target, if one was given). Prices are read from the page's own product
//! data (prices.rs), never guessed.
//!
//! In chat: "watch https://example.com/jobs", "tell me when <link> changes",
//! "tell me when <link> drops below $200", "track the price of <link>", "what
//! am I watching?", "stop watching <name>". Adding one shows an approval card,
//! since it runs on its own. Checks happen while BYTE is open (scheduler.rs).

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools::SourceBook;

/// Pages a user can watch.
const MAX_WATCHERS: i64 = 50;
/// Checks per loop tick (the rest wait for the next tick).
const PER_TICK: usize = 3;
/// Text kept for comparing (characters).
const KEEP_TEXT: usize = 60_000;
/// Lines shorter than this are menus, dates and counters, not content.
const MIN_LINE: usize = 25;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WatchKind {
    /// Any change to the page's main text.
    Change,
    /// The product's price.
    Price,
}

impl WatchKind {
    fn as_str(self) -> &'static str {
        match self {
            WatchKind::Change => "change",
            WatchKind::Price => "price",
        }
    }
    fn from_str(s: &str) -> WatchKind {
        if s == "price" {
            WatchKind::Price
        } else {
            WatchKind::Change
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Watcher {
    #[serde(default)]
    pub id: i64,
    pub url: String,
    #[serde(default)]
    pub name: String,
    pub kind: WatchKind,
    /// Price watchers: notify at or below this.
    #[serde(default)]
    pub target: Option<f64>,
    #[serde(default = "six")]
    pub every_hours: i64,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub last_checked: Option<i64>,
    #[serde(default)]
    pub next_check: Option<i64>,
    #[serde(default)]
    pub last_price: Option<f64>,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub last_change: Option<i64>,
    /// What changed last time ("Dropped to $179 (was $199)").
    #[serde(default)]
    pub last_note: String,
    #[serde(default)]
    pub last_error: String,
}

fn six() -> i64 {
    6
}
fn yes() -> bool {
    true
}

const COLS: &str = "id, url, name, kind, target, every_hours, enabled, created, last_checked, next_check, last_price, currency, last_change, last_note, last_error";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Watcher> {
    Ok(Watcher {
        id: r.get(0)?,
        url: r.get(1)?,
        name: r.get(2)?,
        kind: WatchKind::from_str(&r.get::<_, String>(3)?),
        target: r.get(4)?,
        every_hours: r.get(5)?,
        enabled: r.get::<_, i64>(6)? != 0,
        created: r.get(7)?,
        last_checked: r.get(8)?,
        next_check: r.get(9)?,
        last_price: r.get(10)?,
        currency: r.get(11)?,
        last_change: r.get(12)?,
        last_note: r.get(13)?,
        last_error: r.get(14)?,
    })
}

pub fn list(db: &Db) -> AppResult<Vec<Watcher>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM watchers ORDER BY created DESC"))?;
    let v = st.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

pub fn get(db: &Db, id: i64) -> AppResult<Option<Watcher>> {
    Ok(db.conn().query_row(&format!("SELECT {COLS} FROM watchers WHERE id = ?1"), [id], row).optional()?)
}

fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_default()
}

/// Adds or updates a watcher (its page state is kept on update). Checks are due right away for a new one.
pub fn save(db: &Db, w: &Watcher, now: i64) -> AppResult<Watcher> {
    let url = url::Url::parse(w.url.trim()).map_err(|_| AppError::msg("That doesn't look like a web address."))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::msg("BYTE can only watch web pages (http or https)."));
    }
    let name: String = if w.name.trim().is_empty() { host_of(url.as_str()) } else { w.name.trim().chars().take(120).collect() };
    let every = w.every_hours.clamp(1, 24 * 7);
    let target = w.target.filter(|t| t.is_finite() && *t > 0.0).filter(|_| w.kind == WatchKind::Price);
    let id = {
        let conn = db.conn();
        if w.id > 0 {
            conn.execute(
                "UPDATE watchers SET url = ?1, name = ?2, kind = ?3, target = ?4, every_hours = ?5, enabled = ?6,
                 next_check = CASE WHEN ?6 = 0 THEN NULL ELSE COALESCE(next_check, ?7) END WHERE id = ?8",
                params![url.as_str(), name, w.kind.as_str(), target, every, w.enabled as i64, now, w.id],
            )?;
            w.id
        } else {
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM watchers", [], |r| r.get(0))?;
            if count >= MAX_WATCHERS {
                return Err(AppError::msg(format!("BYTE watches up to {MAX_WATCHERS} pages. Stop watching one first.")));
            }
            conn.execute(
                "INSERT INTO watchers (url, name, kind, target, every_hours, enabled, created, next_check) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![url.as_str(), name, w.kind.as_str(), target, every, w.enabled as i64, now],
            )?;
            conn.last_insert_rowid()
        }
    };
    get(db, id)?.ok_or_else(|| AppError::msg("The watcher wasn't saved."))
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM watch_events WHERE watcher_id = ?1", [id])?;
    conn.execute("DELETE FROM watchers WHERE id = ?1", [id])?;
    Ok(())
}

/// Watchers whose check is due (a few at a time).
pub fn due(db: &Db, now: i64) -> AppResult<Vec<Watcher>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM watchers WHERE enabled = 1 AND next_check IS NOT NULL AND next_check <= ?1 ORDER BY next_check LIMIT ?2"))?;
    let v = st.query_map(params![now, PER_TICK as i64], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchEvent {
    pub at: i64,
    pub note: String,
}

pub fn events(db: &Db, id: i64) -> AppResult<Vec<WatchEvent>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT at, note FROM watch_events WHERE watcher_id = ?1 ORDER BY at DESC, id DESC LIMIT 30")?;
    let v = st.query_map([id], |r| Ok(WatchEvent { at: r.get(0)?, note: r.get(1)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

// ----------------------------------------------------------- what changed

/// A page's content lines, for comparing (menus, counters and dates are too short to count).
pub fn content_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.chars().count() >= MIN_LINE && !out.contains(&l) {
            out.push(l);
        }
    }
    out
}

fn hash(lines: &[String]) -> String {
    let mut h = Sha256::new();
    for l in lines {
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    format!("{:x}", h.finalize())
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>().trim_end())
    }
}

/// What changed between two versions of a page, in a sentence (None: nothing that matters).
pub fn text_change(old: &[String], new: &[String]) -> Option<String> {
    let added: Vec<&String> = new.iter().filter(|l| !old.contains(l)).collect();
    let removed = old.iter().filter(|l| !new.contains(l)).count();
    match (added.first(), removed) {
        (None, 0) => None,
        (Some(first), _) => {
            let more = added.len() - 1 + removed;
            Some(format!("New: “{}”{}", clip(first, 140), if more > 0 { format!(" (+{more} more {})", if more == 1 { "change" } else { "changes" }) } else { String::new() }))
        }
        (None, n) => Some(format!("{n} {} removed from the page", if n == 1 { "part was" } else { "parts were" })),
    }
}

/// The product's price on a page, with its currency: the lowest of the page's
/// own schema.org offers, else its `itemprop="price"` markup, else the price
/// Amazon shows to pay.
pub fn page_price(html: &str) -> Option<(f64, String)> {
    let offers = crate::prices::offers_on_page(html);
    if let Some(first_title) = offers.first().map(|o| o.0.clone()) {
        if let Some(p) = offers.into_iter().filter(|o| o.0 == first_title && o.1 > 0.0).map(|o| (o.1, o.2)).min_by(|a, b| a.0.total_cmp(&b.0)) {
            return Some(p);
        }
    }
    price_in_markup(html)
}

/// "$1,299.99" → (1299.99, "USD"); "199,00 €" → (199.0, "EUR").
pub fn shown_price(s: &str) -> Option<(f64, String)> {
    let s = s.trim();
    let currency = if s.contains('$') && !s.contains("CA$") && !s.contains("A$") {
        "USD"
    } else if s.contains('€') {
        "EUR"
    } else if s.contains('£') {
        "GBP"
    } else if s.contains('¥') {
        "JPY"
    } else {
        ""
    };
    let digits: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
    // "199,00": a comma with two digits after it (and no dot) is the decimal point.
    let n = match (digits.rfind(','), digits.contains('.')) {
        (Some(i), false) if digits.len() - i == 3 => digits.replace(',', "."),
        _ => digits.replace(',', ""),
    };
    n.trim_matches('.').parse::<f64>().ok().filter(|v| *v > 0.0).map(|v| (v, currency.to_string()))
}

fn price_in_markup(html: &str) -> Option<(f64, String)> {
    let doc = scraper::Html::parse_document(html);
    let pick = |css: &str| scraper::Selector::parse(css).ok().and_then(|sel| doc.select(&sel).next().map(|e| (e.value().attr("content").map(str::to_string), e.text().collect::<String>())));
    // schema.org microdata.
    if let Some((content, text)) = pick("[itemprop=price]") {
        let raw = content.unwrap_or(text);
        if let Some((v, shown)) = shown_price(&raw) {
            let cur = pick("[itemprop=priceCurrency]").and_then(|(c, t)| c.or(Some(t))).map(|c| c.trim().to_uppercase()).filter(|c| c.len() == 3).unwrap_or(shown);
            return Some((v, cur));
        }
    }
    // Amazon: the price to pay (not list prices or other sellers).
    for css in [".priceToPay .a-offscreen", "#corePrice_feature_div .a-offscreen", "#corePriceDisplay_desktop_feature_div .a-offscreen", "#price_inside_buybox"] {
        if let Some((_, text)) = pick(css) {
            if let Some(p) = shown_price(&text) {
                return Some(p);
            }
        }
    }
    None
}

pub fn money(v: f64, currency: &str) -> String {
    let n = if v.fract().abs() < 0.005 { format!("{v:.0}") } else { format!("{v:.2}") };
    match currency {
        "USD" | "" => format!("${n}"),
        "EUR" => format!("€{n}"),
        "GBP" => format!("£{n}"),
        "JPY" => format!("¥{n}"),
        c => format!("{n} {c}"),
    }
}

/// What a new price means: a note for the history, and whether it's worth a notification.
pub fn price_change(old: Option<f64>, new: f64, target: Option<f64>, currency: &str) -> Option<(String, bool)> {
    let m = |v: f64| money(v, currency);
    let was = old.map(|o| format!(" (was {})", m(o))).unwrap_or_default();
    if let Some(t) = target {
        let crossed = new <= t && old.is_none_or(|o| o > t);
        if crossed && old.is_some() {
            return Some((format!("Now {} — at or below your {} target{was}", m(new), m(t)), true));
        }
    }
    let old = old?;
    if (new - old).abs() < 0.005 {
        return None;
    }
    if new < old {
        // With a target, drops that stay above it are noted but not announced.
        let alert = target.is_none_or(|t| new <= t);
        Some((format!("Dropped to {}{was}", m(new)), alert))
    } else {
        Some((format!("Went up to {}{was}", m(new)), false))
    }
}

/// The result of one check.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    /// A note for the history (a change was seen).
    pub note: Option<String>,
    /// Worth a notification.
    pub alert: bool,
}

fn record(db: &Db, w: &Watcher, now: i64, note: &str) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("INSERT INTO watch_events (watcher_id, at, note) VALUES (?1, ?2, ?3)", params![w.id, now, note])?;
    conn.execute("UPDATE watchers SET last_change = ?1, last_note = ?2 WHERE id = ?3", params![now, note, w.id])?;
    conn.execute("DELETE FROM watch_events WHERE watcher_id = ?1 AND id NOT IN (SELECT id FROM watch_events WHERE watcher_id = ?1 ORDER BY at DESC, id DESC LIMIT 30)", [w.id])?;
    Ok(())
}

/// Compares a freshly read page with what BYTE saw last time and stores it.
pub fn apply(db: &Db, w: &Watcher, now: i64, text: &str, html: &str) -> AppResult<Checked> {
    let next = now + w.every_hours.max(1) * 3_600_000;
    match w.kind {
        WatchKind::Change => {
            let lines = content_lines(text);
            if lines.is_empty() {
                db.conn().execute("UPDATE watchers SET last_checked = ?1, next_check = ?2, last_error = ?3 WHERE id = ?4", params![now, next, "BYTE couldn't read any text on the page", w.id])?;
                return Ok(Checked { note: None, alert: false });
            }
            let h = hash(&lines);
            let (old_hash, old_text): (String, String) = db.conn().query_row("SELECT last_hash, last_text FROM watchers WHERE id = ?1", [w.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let stored: String = lines.join("\n").chars().take(KEEP_TEXT).collect();
            db.conn().execute("UPDATE watchers SET last_checked = ?1, next_check = ?2, last_error = '', last_hash = ?3, last_text = ?4 WHERE id = ?5", params![now, next, h, stored, w.id])?;
            if old_hash.is_empty() || old_hash == h {
                return Ok(Checked { note: None, alert: false });
            }
            let old: Vec<String> = old_text.lines().map(str::to_string).collect();
            let note = text_change(&old, &lines);
            if let Some(n) = &note {
                record(db, w, now, n)?;
            }
            Ok(Checked { alert: note.is_some(), note })
        }
        WatchKind::Price => {
            let Some((price, currency)) = page_price(html) else {
                db.conn().execute("UPDATE watchers SET last_checked = ?1, next_check = ?2, last_error = ?3 WHERE id = ?4", params![now, next, "BYTE couldn't find a price on the page", w.id])?;
                return Ok(Checked { note: None, alert: false });
            };
            let currency = if currency.is_empty() { w.currency.clone() } else { currency };
            db.conn().execute("UPDATE watchers SET last_checked = ?1, next_check = ?2, last_error = '', last_price = ?3, currency = ?4 WHERE id = ?5", params![now, next, price, currency, w.id])?;
            let change = price_change(w.last_price, price, w.target, &currency);
            if let Some((n, _)) = &change {
                record(db, w, now, n)?;
            }
            Ok(Checked { alert: change.as_ref().is_some_and(|c| c.1), note: change.map(|c| c.0) })
        }
    }
}

/// Reads the page and compares it; also returns the page's title (network
/// errors are stored on the watcher).
pub async fn check(db: &Db, net: &reqwest::Client, w: &Watcher) -> AppResult<(Checked, String)> {
    let now = chrono::Utc::now().timestamp_millis();
    match crate::tools::fetch::fetch_html(net, &w.url).await {
        Ok((page, html)) => Ok((apply(db, w, now, &page.text, &html)?, page.title)),
        Err(e) => {
            let next = now + w.every_hours.max(1) * 3_600_000;
            db.conn().execute("UPDATE watchers SET last_checked = ?1, next_check = ?2, last_error = ?3 WHERE id = ?4", params![now, next, e.to_string(), w.id])?;
            Err(e)
        }
    }
}

/// The loop's part (scheduler.rs): due checks, with a notification for each alert.
pub async fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    let on = {
        let s = state.settings.lock().await;
        s.watch_enabled && s.web_search && !crate::offline::is_offline()
    };
    if !on {
        return;
    }
    let now = chrono::Utc::now().timestamp_millis();
    let due = match due(&state.db, now) {
        Ok(d) => d,
        Err(e) => return log::warn!("watchers: {e}"),
    };
    for w in due {
        match check(&state.db, &state.net, &w).await {
            Ok((Checked { note: Some(note), alert: true }, _)) => {
                crate::scheduler::notify(app, &format!("{} changed", w.name), &note);
                let _ = tauri::Emitter::emit(app, "watchers://changed", w.id);
            }
            Ok(_) => {}
            Err(e) => log::info!("watcher {}: {e}", w.name),
        }
    }
}

// ------------------------------------------------------------------ in chat

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Watch { url: String, kind: WatchKind, target: Option<f64>, every_hours: i64 },
    List,
    Stop(String),
}

/// A price in words: "$199", "199.99", "1,299", "€50".
fn amount(s: &str) -> Option<f64> {
    let t: String = s.trim_start_matches(['$', '€', '£', '¥']).chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').filter(|c| *c != ',').collect();
    let t = t.trim_end_matches('.');
    t.parse::<f64>().ok().filter(|v| *v > 0.0)
}

/// "below $200", "under 150", "less than €80", "drops to 99".
fn target_in(l: &str) -> Option<f64> {
    for w in ["below ", "under ", "less than ", "drops to ", "falls to ", "gets to ", "hits ", "at or below "] {
        if let Some(i) = l.find(w) {
            if let Some(v) = l[i + w.len()..].split_whitespace().next().and_then(amount) {
                return Some(v);
            }
        }
    }
    None
}

fn every_in(l: &str) -> i64 {
    if l.contains("every hour") || l.contains("hourly") {
        1
    } else if l.contains("every day") || l.contains("daily") || l.contains("once a day") {
        24
    } else {
        6
    }
}

pub fn ask(q: &str) -> Option<Ask> {
    let l = q.trim().to_lowercase().replace('’', "'");
    let l = l.trim_end_matches(['?', '.', '!']).trim().to_string();
    if ["what am i watching", "what pages am i watching", "show my watchers", "list my watchers", "my price alerts", "what pages are you watching", "which pages am i watching"].iter().any(|w| l.contains(w)) {
        return Some(Ask::List);
    }
    for p in ["stop watching ", "stop tracking ", "unwatch ", "remove the price alert for ", "cancel the price alert for "] {
        if let Some(rest) = l.strip_prefix(p) {
            let what = rest.trim();
            if !what.is_empty() {
                return Some(Ask::Stop(what.to_string()));
            }
        }
    }
    let url = crate::feeds::url_in(&l)?;
    let pricey = ["price", "drops", "cheaper", "on sale", "goes on sale", " below ", " under ", "less than", "$", "€", "£"].iter().any(|w| l.contains(w));
    let starts = ["watch ", "keep an eye on ", "monitor ", "track ", "tell me when ", "tell me if ", "let me know when ", "let me know if ", "alert me when ", "alert me if ", "notify me when ", "notify me if ", "ping me when ", "set a price alert", "price alert for "];
    if !starts.iter().any(|s| l.starts_with(s)) {
        return None;
    }
    let watchy = pricey || l.starts_with("watch ") || l.starts_with("keep an eye on ") || l.starts_with("monitor ") || l.contains("change") || l.contains("updated") || l.contains("update");
    if !watchy {
        return None;
    }
    let kind = if pricey { WatchKind::Price } else { WatchKind::Change };
    Some(Ask::Watch { url, kind, target: if pricey { target_in(&l) } else { None }, every_hours: every_in(&l) })
}

pub fn applies(q: &str) -> bool {
    ask(q).is_some()
}

pub fn find<'a>(watchers: &'a [Watcher], what: &str) -> Option<&'a Watcher> {
    let w = what.trim_start_matches("the ").to_lowercase();
    watchers
        .iter()
        .find(|x| x.name.to_lowercase() == w || x.url.to_lowercase() == w)
        .or_else(|| watchers.iter().find(|x| w.len() >= 3 && (x.name.to_lowercase().contains(&w) || x.url.to_lowercase().contains(&w))))
}

fn every_text(h: i64) -> String {
    match h {
        1 => "every hour".into(),
        24 => "once a day".into(),
        h => format!("every {h} hours"),
    }
}

pub fn describe(w: &Watcher) -> String {
    match w.kind {
        WatchKind::Change => format!("Changes to the page, checked {}", every_text(w.every_hours)),
        WatchKind::Price => match w.target {
            Some(t) => format!("Price at or below {}, checked {}", money(t, &w.currency), every_text(w.every_hours)),
            None => format!("Price drops, checked {}", every_text(w.every_hours)),
        },
    }
}

fn list_notes(ws: &[Watcher]) -> String {
    if ws.is_empty() {
        return "BYTE isn't watching any pages. The user can say \"tell me when <link> drops below $200\" or \"watch <link>\".".into();
    }
    let lines: Vec<String> = ws
        .iter()
        .map(|w| {
            let price = w.last_price.map(|p| format!(", now {}", money(p, &w.currency))).unwrap_or_default();
            let last = if w.last_note.is_empty() { String::new() } else { format!(", last change: {}", w.last_note) };
            format!("- {} ({}): {}{price}{last}{}", w.name, w.url, describe(w).to_lowercase(), if w.enabled { "" } else { " [paused]" })
        })
        .collect();
    format!("Pages BYTE watches ({}):\n{}", ws.len(), lines.join("\n"))
}

pub async fn run(turn: &Turn<'_>, db: &Db, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(a) = ask(question) else { return Ok(None) };
    let none = SourceBook::default;
    let id = format!("byte_watch_{}", uuid::Uuid::new_v4().simple());
    match a {
        Ask::List => {
            let ws = list(db)?;
            send(ChatEvent::ToolCall { id: id.clone(), name: "watch_list".into(), args: json!({}) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} pages", ws.len()) })?;
            Ok(Some((none(), format!("{}\n\nList them briefly.", list_notes(&ws)))))
        }
        Ask::Stop(what) => {
            let ws = list(db)?;
            let Some(w) = find(&ws, &what).cloned() else {
                return Ok(Some((none(), format!("No watched page matches \"{what}\". {}", list_notes(&ws)))));
            };
            delete(db, w.id)?;
            send(ChatEvent::ToolCall { id: id.clone(), name: "watch_stop".into(), args: json!({ "name": w.name }) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: "Stopped".into() })?;
            Ok(Some((none(), format!("Done: BYTE stopped watching \"{}\". Confirm in one short sentence.", w.name))))
        }
        Ask::Watch { url, kind, target, every_hours } => {
            if !turn.web {
                return Ok(Some((none(), "Watching a page needs web access, which is off in Settings. Say so in one sentence.".into())));
            }
            let draft = Watcher { id: 0, url: url.clone(), name: host_of(&url), kind, target, every_hours, enabled: true, created: 0, last_checked: None, next_check: None, last_price: None, currency: String::new(), last_change: None, last_note: String::new(), last_error: String::new() };
            send(ChatEvent::ToolCall { id: id.clone(), name: "watch_add".into(), args: json!({ "url": url }) })?;
            let fields = vec![
                ("Page".to_string(), url.clone()),
                ("Watch for".to_string(), describe(&draft)),
                ("Note".to_string(), "BYTE checks while it's open and sends a notification when something changes. Stop it any time in the ✅ panel.".to_string()),
            ];
            if !crate::macctl::ask_ok("Watch this page?", "BYTE", fields, cancel, send).await? {
                send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
                return Ok(Some((none(), "The user chose not to watch the page. Nothing was set up. Say so in one sentence.".into())));
            }
            let now = chrono::Utc::now().timestamp_millis();
            let mut w = save(db, &draft, now)?;
            // The first look sets what later checks compare with.
            let first = check(db, turn.net, &w).await;
            if let Ok((_, t)) = &first {
                if !t.trim().is_empty() {
                    w.name = t.trim().chars().take(80).collect();
                    w = save(db, &w, now)?;
                }
            }
            let w = get(db, w.id)?.unwrap_or(w);
            let (ok, detail) = match (&first, w.kind, w.last_price) {
                (Err(e), _, _) => (false, format!("Couldn't read it yet ({e}); BYTE will try again")),
                (Ok(_), WatchKind::Price, Some(p)) => (true, format!("Now {}", money(p, &w.currency))),
                (Ok(_), WatchKind::Price, None) => (false, "BYTE couldn't find a price on that page".to_string()),
                (Ok(_), WatchKind::Change, _) => (true, describe(&w)),
            };
            send(ChatEvent::ToolResult { id, ok, summary: detail.clone() })?;
            turn.log.record("watch_add", &json!({ "url": w.url }), ok, &detail);
            send(ChatEvent::MacDone(crate::macctl::MacDone { app: "BYTE".into(), title: format!("Watching \"{}\"", w.name), detail: detail.clone(), ok, undo: None }))?;
            let below = match (w.last_price, w.target) {
                (Some(p), Some(t)) if p <= t => " The price is already at or below the target.",
                _ => "",
            };
            Ok(Some((none(), format!("Done: BYTE now watches \"{}\" ({}): {}. {detail}.{below} It checks while BYTE is open and sends a notification. Confirm in one or two short sentences.", w.name, w.url, describe(&w).to_lowercase()))))
        }
    }
}

// ----------------------------------------------------------------- commands

#[tauri::command]
pub fn watchers_list(state: State<'_, AppState>) -> AppResult<Vec<Watcher>> {
    list(&state.db)
}

#[tauri::command]
pub fn watcher_save(state: State<'_, AppState>, watcher: Watcher) -> AppResult<Watcher> {
    save(&state.db, &watcher, chrono::Utc::now().timestamp_millis())
}

#[tauri::command]
pub fn watcher_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete(&state.db, id)
}

#[tauri::command]
pub fn watcher_events(state: State<'_, AppState>, id: i64) -> AppResult<Vec<WatchEvent>> {
    events(&state.db, id)
}

/// "Check now": the updated watcher.
#[tauri::command]
pub async fn watcher_check(state: State<'_, AppState>, id: i64) -> AppResult<Watcher> {
    let w = get(&state.db, id)?.ok_or_else(|| AppError::msg("That watcher is gone."))?;
    let _ = check(&state.db, &state.net, &w).await;
    get(&state.db, id)?.ok_or_else(|| AppError::msg("That watcher is gone."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_understood() {
        assert_eq!(ask("tell me when https://shop.example.com/tv drops below $1,299.99"), Some(Ask::Watch { url: "https://shop.example.com/tv".into(), kind: WatchKind::Price, target: Some(1299.99), every_hours: 6 }));
        assert_eq!(ask("track the price of example.com/p/42"), Some(Ask::Watch { url: "https://example.com/p/42".into(), kind: WatchKind::Price, target: None, every_hours: 6 }));
        assert_eq!(ask("watch example.org/jobs every hour"), Some(Ask::Watch { url: "https://example.org/jobs".into(), kind: WatchKind::Change, target: None, every_hours: 1 }));
        assert_eq!(ask("let me know when example.org/news changes"), Some(Ask::Watch { url: "https://example.org/news".into(), kind: WatchKind::Change, target: None, every_hours: 6 }));
        assert_eq!(ask("alert me if example.com/x goes under 80 daily"), Some(Ask::Watch { url: "https://example.com/x".into(), kind: WatchKind::Price, target: Some(80.0), every_hours: 24 }));
        assert_eq!(ask("what am I watching?"), Some(Ask::List));
        assert_eq!(ask("stop watching the TV"), Some(Ask::Stop("the tv".into())));
        for q in ["watch a movie tonight", "tell me when the store opens", "track my package", "keep an eye on the kids", "let me know when you're done", "tell me when example.com is open"] {
            assert_eq!(ask(q), None, "{q}");
        }
    }

    #[test]
    fn page_changes_are_described() {
        let old = content_lines("Menu\nJobs at Example\nSenior engineer, remote, full time\nDesigner, New York office, hybrid\n2 hours ago");
        assert_eq!(old.len(), 2, "short lines don't count: {old:?}");
        let same = content_lines("Menu\nJobs at Example\nSenior engineer, remote, full time\nDesigner, New York office, hybrid\n5 hours ago");
        assert_eq!(hash(&old), hash(&same), "counters and dates don't make a change");
        let new = content_lines("Senior engineer, remote, full time\nData scientist, Berlin office, on site\nDesigner, New York office, hybrid");
        assert_eq!(text_change(&old, &new).unwrap(), "New: “Data scientist, Berlin office, on site”");
        let fewer = content_lines("Senior engineer, remote, full time");
        assert_eq!(text_change(&old, &fewer).unwrap(), "1 part was removed from the page");
        let both = content_lines("Senior engineer, remote, full time\nData scientist, Berlin office, on site");
        assert_eq!(text_change(&old, &both).unwrap(), "New: “Data scientist, Berlin office, on site” (+1 more change)");
        assert_eq!(text_change(&old, &old), None);
    }

    #[test]
    fn price_moves_are_judged() {
        assert_eq!(price_change(None, 199.0, None, "USD"), None, "the first price is only the baseline");
        assert_eq!(price_change(Some(199.0), 179.0, None, "USD"), Some(("Dropped to $179 (was $199)".into(), true)));
        assert_eq!(price_change(Some(179.0), 189.5, None, "USD"), Some(("Went up to $189.50 (was $179)".into(), false)));
        assert_eq!(price_change(Some(179.0), 179.0, None, "USD"), None);
        // With a target: drops above it are noted quietly, crossing it is announced once.
        assert_eq!(price_change(Some(250.0), 230.0, Some(200.0), "EUR"), Some(("Dropped to €230 (was €250)".into(), false)));
        assert_eq!(price_change(Some(230.0), 199.0, Some(200.0), "EUR"), Some(("Now €199 — at or below your €200 target (was €230)".into(), true)));
        assert_eq!(price_change(Some(199.0), 189.0, Some(200.0), "EUR"), Some(("Dropped to €189 (was €199)".into(), true)));
        assert_eq!(price_change(None, 150.0, Some(200.0), "GBP"), None, "already below when added: said in chat, not notified");
        assert_eq!(money(12.5, "CAD"), "12.50 CAD");
    }

    #[test]
    fn prices_come_from_the_page() {
        let html = r#"<html><head><script type="application/ld+json">
            {"@type":"Product","name":"TV 55","offers":[{"@type":"Offer","price":"549.99","priceCurrency":"usd"},{"@type":"Offer","price":"499.00","priceCurrency":"USD"}]}
        </script></head><body></body></html>"#;
        assert_eq!(page_price(html), Some((499.0, "USD".into())));
        assert_eq!(page_price("<html><body>$20 off!</body></html>"), None, "a number in the text isn't a price");
        // Microdata.
        let micro = r#"<div itemscope itemtype="https://schema.org/Product"><span itemprop="name">Lamp</span>
            <span itemprop="priceCurrency" content="EUR"></span><span itemprop="price" content="49.90">49,90 €</span></div>"#;
        assert_eq!(page_price(micro), Some((49.9, "EUR".into())));
        // Amazon's layout (a representative snippet; the live page can't be read from CI).
        let amazon = r#"<div id="corePriceDisplay_desktop_feature_div"><span class="a-price a-text-price"><span class="a-offscreen">$249.00</span></span>
            <span class="a-price priceToPay"><span class="a-offscreen">$189.99</span><span aria-hidden="true">$189<sup>99</sup></span></span></div>"#;
        assert_eq!(page_price(amazon), Some((189.99, "USD".into())), "the price to pay, not the list price");
        assert_eq!(shown_price("$1,299.99"), Some((1299.99, "USD".into())));
        assert_eq!(shown_price("199,00 €"), Some((199.0, "EUR".into())));
        assert_eq!(shown_price("£1,050"), Some((1050.0, "GBP".into())));
        assert_eq!(shown_price("free"), None);
    }

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        (dir, db)
    }

    fn watcher(kind: WatchKind, target: Option<f64>) -> Watcher {
        Watcher { id: 0, url: "https://www.example.com/p".into(), name: String::new(), kind, target, every_hours: 6, enabled: true, created: 0, last_checked: None, next_check: None, last_price: None, currency: String::new(), last_change: None, last_note: String::new(), last_error: String::new() }
    }

    fn product(price: &str) -> String {
        format!(r#"<script type="application/ld+json">{{"@type":"Product","name":"P","offers":{{"price":"{price}","priceCurrency":"USD"}}}}</script>"#)
    }

    #[test]
    fn watchers_are_checked_on_time_and_remember_what_they_saw() {
        let (_d, db) = db();
        let w = save(&db, &watcher(WatchKind::Price, Some(100.0)), 1_000).unwrap();
        assert_eq!(w.name, "example.com");
        assert_eq!(due(&db, 1_000).unwrap().len(), 1, "a new watcher is checked right away");
        let r = apply(&db, &w, 1_000, "", &product("120")).unwrap();
        assert_eq!(r, Checked { note: None, alert: false });
        let w = get(&db, w.id).unwrap().unwrap();
        assert_eq!((w.last_price, w.currency.as_str(), w.next_check), (Some(120.0), "USD", Some(1_000 + 6 * 3_600_000)));
        assert!(due(&db, 2_000).unwrap().is_empty());
        let r = apply(&db, &w, 3_000, "", &product("95")).unwrap();
        assert!(r.alert);
        assert_eq!(events(&db, w.id).unwrap()[0].note, "Now $95 — at or below your $100 target (was $120)");
        let w = get(&db, w.id).unwrap().unwrap();
        assert_eq!(w.last_note, "Now $95 — at or below your $100 target (was $120)");
        let r = apply(&db, &w, 4_000, "", "<html></html>").unwrap();
        assert!(!r.alert);
        assert_eq!(get(&db, w.id).unwrap().unwrap().last_error, "BYTE couldn't find a price on the page");

        let c = save(&db, &watcher(WatchKind::Change, Some(5.0)), 1_000).unwrap();
        assert_eq!(c.target, None, "only price watchers have targets");
        let page1 = "Senior engineer, remote, full time\nDesigner, New York office, hybrid";
        assert!(!apply(&db, &c, 1_000, page1, "").unwrap().alert, "the first look is the baseline");
        let c = get(&db, c.id).unwrap().unwrap();
        assert!(!apply(&db, &c, 2_000, page1, "").unwrap().alert);
        let r = apply(&db, &c, 3_000, &format!("{page1}\nData scientist, Berlin office, on site"), "").unwrap();
        assert_eq!(r.note.as_deref(), Some("New: “Data scientist, Berlin office, on site”"));

        // Paused: never due.
        let paused = save(&db, &Watcher { enabled: false, ..get(&db, c.id).unwrap().unwrap() }, 5_000).unwrap();
        assert_eq!(paused.next_check, None);
        assert!(due(&db, i64::MAX).unwrap().iter().all(|x| x.id != c.id));
        assert!(save(&db, &Watcher { url: "file:///etc/passwd".into(), ..watcher(WatchKind::Change, None) }, 1).is_err());
        assert!(save(&db, &Watcher { url: "not a url".into(), ..watcher(WatchKind::Change, None) }, 1).is_err());
        assert_eq!(find(&list(&db).unwrap(), "example").map(|x| x.id).is_some(), true);
        delete(&db, c.id).unwrap();
        assert!(events(&db, c.id).unwrap().is_empty());
    }

    /// Real pages: `cargo test e2e_real_watch -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn e2e_real_watch() {
        let net = crate::tools::fetch::web_client();
        for url in ["https://www.bestbuy.com/site/apple-airpods-pro-2-white/6447382.p", "https://www.amazon.com/dp/B0D1XD1ZV3", "https://www.rei.com/product/148560/rei-co-op-flash-22-pack", "https://www.ikea.com/us/en/p/kallax-shelf-unit-white-80275887/"] {
            match crate::tools::fetch::fetch_html(&net, url).await {
                Ok((page, html)) => println!("{url}: \"{}\" price {:?}, {} content lines", page.title, page_price(&html), content_lines(&page.text).len()),
                Err(e) => println!("{url}: ERROR {e}"),
            }
        }
        for url in ["https://www.rust-lang.org/", "https://en.wikipedia.org/wiki/Special:Random"] {
            let (page, _) = crate::tools::fetch::fetch_html(&net, url).await.unwrap();
            println!("{url}: {} content lines; first: {:?}", content_lines(&page.text).len(), content_lines(&page.text).first());
        }
    }
}
