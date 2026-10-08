//! Connectors (Phase 10) that need no app registration with another company:
//! an Obsidian vault (a local folder), Notion (the user's own integration
//! secret) and calendar links (secret iCal addresses). Each is off until set
//! up in Settings → Connectors. Secrets (the Notion secret, calendar
//! addresses, which let anyone read the calendar) live only in the macOS
//! Keychain; the vault folder and the Notion parent page are plain settings.
//!
//! Google, Dropbox, OneDrive and Spotify need BYTE registered with them first
//! (owner decision 2026-09-30: later).

pub mod ics;
pub mod notion;
pub mod obsidian;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::cloud::keychain::SecretStore;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools::SourceBook;

// ------------------------------------------------------------------ secrets

#[cfg_attr(any(not(target_os = "macos"), test), allow(dead_code))]
const SERVICE: &str = "com.loganstarner.byte.connectors";
const NOTION: &str = "notion";
const CALENDARS: &str = "calendars";

/// The Keychain, under BYTE's connectors entry. Reads are remembered for the
/// session (chat routing asks on many turns, and each Keychain read is a call
/// into the Security framework); saving or removing updates the memory too.
/// Test builds never touch the real Keychain (a CI Mac has no one to answer
/// its prompts).
pub struct Secrets;

static REMEMBERED: std::sync::Mutex<Option<std::collections::HashMap<String, Option<String>>>> = std::sync::Mutex::new(None);

fn remembered(account: &str) -> Option<Option<String>> {
    REMEMBERED.lock().ok()?.as_ref()?.get(account).cloned()
}

fn remember(account: &str, value: Option<String>) {
    if let Ok(mut m) = REMEMBERED.lock() {
        m.get_or_insert_with(Default::default).insert(account.to_string(), value);
    }
}

impl SecretStore for Secrets {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        if let Some(v) = remembered(account) {
            return Ok(v);
        }
        let v = keychain::get(account)?;
        remember(account, v.clone());
        Ok(v)
    }
    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        keychain::set(account, secret)?;
        remember(account, Some(secret.to_string()));
        Ok(())
    }
    fn delete(&self, account: &str) -> AppResult<()> {
        keychain::delete(account)?;
        remember(account, None);
        Ok(())
    }
}

/// What the system calls its secret store, for messages.
#[cfg(all(target_os = "macos", not(test)))]
const STORE: &str = "Keychain";
#[cfg(all(windows, not(test)))]
const STORE: &str = "Credential Manager";
#[cfg(all(target_os = "linux", not(test)))]
const STORE: &str = "system keyring";

#[cfg(all(any(target_os = "macos", windows, target_os = "linux"), not(test)))]
mod keychain {
    use super::STORE;
    use super::SERVICE;
    use crate::error::{AppError, AppResult};

    pub fn get(account: &str) -> AppResult<Option<String>> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.get_password()) {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::msg(format!("Couldn't read the {STORE}: {}", crate::cloud::keychain::why(&e)))),
        }
    }
    pub fn set(account: &str, secret: &str) -> AppResult<()> {
        keyring::Entry::new(SERVICE, account).and_then(|e| e.set_password(secret)).map_err(|e| AppError::msg(format!("Couldn't save to the {STORE}: {}", crate::cloud::keychain::why(&e))))
    }
    pub fn delete(account: &str) -> AppResult<()> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::msg(format!("Couldn't remove it from the {STORE}: {}", crate::cloud::keychain::why(&e)))),
        }
    }
}

#[cfg(any(not(any(target_os = "macos", windows, target_os = "linux")), test))]
mod keychain {
    use crate::error::{AppError, AppResult};

    pub fn get(_account: &str) -> AppResult<Option<String>> {
        Ok(None)
    }
    pub fn set(_account: &str, _secret: &str) -> AppResult<()> {
        Err(AppError::msg("BYTE keeps connector secrets in the system's secret store (the macOS Keychain, Windows Credential Manager or the Linux desktop's keyring), which this system doesn't have yet."))
    }
    pub fn delete(_account: &str) -> AppResult<()> {
        Ok(())
    }
}

/// A calendar link (the address is the secret part).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Calendar {
    pub name: String,
    pub url: String,
}

pub fn calendars(store: &dyn SecretStore) -> Vec<Calendar> {
    store.get(CALENDARS).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_calendars(store: &dyn SecretStore, list: &[Calendar]) -> AppResult<()> {
    if list.is_empty() {
        return store.delete(CALENDARS);
    }
    store.set(CALENDARS, &serde_json::to_string(list)?)
}

/// Today's events from every calendar link (errors skipped, logged).
pub async fn events_today(net: &reqwest::Client, store: &dyn SecretStore) -> Option<Vec<(String, String)>> {
    let cals = calendars(store);
    if cals.is_empty() {
        return None;
    }
    let mut all = vec![];
    for c in cals {
        match ics::fetch(net, &c.url).await {
            Ok(events) => all.extend(ics::on_day(&events, ics::today())),
            Err(e) => log::warn!("calendar {}: {e}", c.name),
        }
    }
    Some(all)
}

// ------------------------------------------------------------------ status

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Secrets can be kept (macOS Keychain).
    pub keychain: bool,
    pub vault: Option<String>,
    pub vault_notes: usize,
    pub notion: bool,
    pub notion_parent: Option<String>,
    /// Calendar names and hosts (never the full address).
    pub calendars: Vec<(String, String)>,
}

fn host(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default()
}

// ------------------------------------------------------------------ in chat

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    VaultSearch(String),
    VaultWrite { title: String, body: String },
    NotionSearch(String),
    NotionWrite { title: String, body: String },
    Calendar { days: u32, from_tomorrow: bool },
}

/// "Title — body", "Title: body", or just a body (its first words make the title).
fn title_body(rest: &str) -> (String, String) {
    let rest = rest.trim().trim_start_matches([':', '-', '—', ' ']);
    for sep in [" — ", " - ", "\n"] {
        if let Some((t, b)) = rest.split_once(sep) {
            if !t.trim().is_empty() && t.split_whitespace().count() <= 12 {
                return (t.trim().to_string(), b.trim().to_string());
            }
        }
    }
    let title: String = rest.split_whitespace().take(8).collect::<Vec<_>>().join(" ");
    (title, rest.to_string())
}

pub fn ask(q: &str, has_vault: bool, has_notion: bool, has_calendars: bool) -> Option<Ask> {
    let t = q.trim().trim_end_matches(['?', '!']).trim();
    let l = t.to_lowercase();
    if has_vault {
        for p in ["add a note to obsidian", "add a note in obsidian", "save to obsidian", "write a note in obsidian", "create an obsidian note", "new obsidian note", "save this to obsidian", "save that to obsidian", "save it to obsidian", "put this in obsidian", "put that in obsidian"] {
            if let Some(rest) = l.strip_prefix(p) {
                // Nothing after it: the last answer is saved.
                let (title, body) = if rest.trim().is_empty() { (String::new(), String::new()) } else { title_body(&t[t.len() - rest.len()..]) };
                return Some(Ask::VaultWrite { title, body });
            }
        }
        if l.contains("obsidian") || l.contains("my vault") {
            let query = l.replace("obsidian", " ").replace("my vault", " ");
            if query.split_whitespace().count() >= 1 {
                return Some(Ask::VaultSearch(query.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
        }
    }
    if has_notion {
        for p in ["add a page to notion", "add a page in notion", "create a notion page", "new notion page", "save to notion", "save this to notion", "save that to notion", "save it to notion", "put this in notion", "put that in notion", "add to notion"] {
            if let Some(rest) = l.strip_prefix(p) {
                let (title, body) = if rest.trim().is_empty() { (String::new(), String::new()) } else { title_body(&t[t.len() - rest.len()..]) };
                return Some(Ask::NotionWrite { title, body });
            }
        }
        if l.contains("notion") && !l.starts_with("what is notion") && !l.contains("notion of") {
            let query = l.replace("in notion", " ").replace("notion", " ").replace("search", " ").replace("my ", " ").replace(" for ", " ");
            let query = query.split_whitespace().filter(|w| !["what", "does", "say", "about", "find", "look", "up", "pages", "page", "on", "the"].contains(w)).collect::<Vec<_>>().join(" ");
            if !query.is_empty() {
                return Some(Ask::NotionSearch(query));
            }
        }
    }
    if has_calendars {
        let cal = ["my calendar", "my schedule", "what do i have today", "what do i have tomorrow", "what's on today", "what's on tomorrow", "any meetings", "my meetings", "my events"].iter().any(|w| l.contains(w));
        if cal && !l.contains("add ") && !l.contains("schedule a") && !l.contains("create ") {
            let week = l.contains("week") || l.contains("next few days");
            let tomorrow = l.contains("tomorrow");
            return Some(Ask::Calendar { days: if week { 7 } else { 1 }, from_tomorrow: tomorrow && !week });
        }
    }
    None
}

/// The last thing BYTE wrote in this chat (for "save that to …").
fn last_answer(turn: &Turn<'_>) -> Option<String> {
    turn.history.iter().rev().find(|m| m.role == "assistant" && !m.content.trim().is_empty()).map(|m| m.content.clone())
}

fn done_card(title: &str, detail: &str, undo: Option<String>) -> ChatEvent {
    ChatEvent::MacDone(crate::macctl::MacDone { app: "BYTE".into(), title: title.into(), detail: detail.into(), ok: true, undo })
}

pub fn applies(app: &AppHandle, q: &str) -> bool {
    let state = app.state::<AppState>();
    let (vault, parent) = match state.settings.try_lock() {
        Ok(s) => (s.obsidian_vault.is_some(), s.notion_parent.is_some()),
        Err(_) => (false, false),
    };
    let l = q.to_lowercase();
    // Only read the Keychain when the message could be for Notion or a calendar.
    let notion = (l.contains("notion")) && (parent || Secrets.get(NOTION).ok().flatten().is_some());
    let calendars = (l.contains("calendar") || l.contains("schedule") || l.contains("today") || l.contains("tomorrow") || l.contains("meeting") || l.contains("events")) && !calendars(&Secrets).is_empty();
    ask(q, vault, notion, calendars && !mac_calendar(&state)).is_some()
}

/// The Mac's own Calendar answers calendar questions when Mac control is on.
fn mac_calendar(state: &AppState) -> bool {
    cfg!(target_os = "macos") && state.settings.try_lock().map(|s| s.mac_control).unwrap_or(false)
}

pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(app) = turn.app else { return Ok(None) };
    let state = app.state::<AppState>();
    let (vault, parent) = {
        let s = state.settings.lock().await;
        (s.obsidian_vault.clone(), s.notion_parent.clone())
    };
    let token = Secrets.get(NOTION).ok().flatten();
    let has_cals = !calendars(&Secrets).is_empty() && !mac_calendar(&state);
    let Some(a) = ask(question, vault.is_some(), token.is_some(), has_cals) else { return Ok(None) };
    let id = format!("byte_conn_{}", uuid::Uuid::new_v4().simple());
    let mut book = SourceBook::default();
    match a {
        Ask::VaultSearch(query) => {
            let dir = std::path::PathBuf::from(vault.unwrap_or_default());
            send(ChatEvent::ToolCall { id: id.clone(), name: "obsidian_search".into(), args: json!({ "query": query }) })?;
            let q = query.clone();
            let hits = tokio::task::spawn_blocking(move || obsidian::search(&dir, &q, 6)).await.unwrap_or_default();
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} notes", hits.len()) })?;
            if hits.is_empty() {
                return Ok(Some((book, format!("Nothing in the user's Obsidian vault mentions \"{query}\". Say so, and suggest other words to try."))));
            }
            let mut notes = String::from("From the user's Obsidian vault (cite as [n]):\n");
            for h in &hits {
                let n = book.add(&h.title, &obsidian::open_link(&h.path), &h.snippet);
                notes.push_str(&format!("[{n}] {} — {}\n", h.title, h.snippet));
            }
            send(ChatEvent::Sources { sources: book.sources.clone() })?;
            notes.push_str("\nAnswer from these notes, citing [n]. If they don't answer it, say so.");
            Ok(Some((book, notes)))
        }
        Ask::VaultWrite { title, body } => {
            let (title, body) = if body.is_empty() {
                let Some(ans) = last_answer(turn) else { return Ok(Some((book, "There's no answer in this chat to save yet. Ask what they'd like the note to say.".into()))) };
                (if title.is_empty() { ans.lines().map(|l| l.trim_start_matches(['#', '*', ' ', '>'])).find(|l| !l.trim().is_empty()).unwrap_or("Note from BYTE").chars().take(60).collect() } else { title }, ans)
            } else {
                (title, body)
            };
            let vault = std::path::PathBuf::from(vault.unwrap_or_default());
            let fields = vec![("Vault".to_string(), vault.display().to_string()), ("New note".to_string(), format!("BYTE/{}.md", obsidian::stem(&title))), ("Text".to_string(), body.chars().take(400).collect())];
            if !crate::macctl::ask_ok("Add this note to Obsidian?", "Obsidian", fields, cancel, send).await? {
                return Ok(Some((book, "The user chose not to add the note. Nothing was saved. Say so in one sentence.".into())));
            }
            let path = obsidian::write(&vault, &title, &body)?;
            let undo = crate::macctl::keep_undo(crate::macctl::Undo::Created(vec![path.clone()]));
            turn.log.record("obsidian_write", &json!({ "note": path.display().to_string() }), true, "saved");
            send(done_card(&format!("Added “{title}” to Obsidian"), &format!("In your vault's BYTE folder · {}", path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()), Some(undo)))?;
            Ok(Some((book, format!("Done: the note \"{title}\" is saved in the BYTE folder of the user's Obsidian vault. Confirm in one sentence."))))
        }
        Ask::NotionSearch(query) => {
            let token = token.unwrap_or_default();
            let n = notion::Notion { http: &state.net, base: notion::API, token: &token };
            send(ChatEvent::ToolCall { id: id.clone(), name: "notion_search".into(), args: json!({ "query": query }) })?;
            let pages = match n.search(&query, 5).await {
                Ok(p) => p,
                Err(e) => {
                    send(ChatEvent::ToolResult { id, ok: false, summary: e.to_string() })?;
                    return Ok(Some((book, format!("Searching Notion failed: {e}. Tell the user plainly."))));
                }
            };
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} pages", pages.len()) })?;
            if pages.is_empty() {
                return Ok(Some((book, format!("No Notion pages shared with BYTE match \"{query}\". Mention that BYTE only sees pages shared with its integration (••• → Connections in Notion)."))));
            }
            let mut notes = String::from("From the user's Notion (cite as [n]):\n");
            for p in pages.iter().take(3) {
                let text = n.text(&p.id, 3000).await.unwrap_or_default();
                let k = book.add(&p.title, &p.url, &text.chars().take(300).collect::<String>());
                notes.push_str(&format!("\n[{k}] {}\n{}\n", p.title, text));
            }
            for p in pages.iter().skip(3) {
                let k = book.add(&p.title, &p.url, "");
                notes.push_str(&format!("[{k}] {} (not read)\n", p.title));
            }
            send(ChatEvent::Sources { sources: book.sources.clone() })?;
            notes.push_str("\nAnswer from these pages, citing [n]. If they don't answer it, say so.");
            Ok(Some((book, notes)))
        }
        Ask::NotionWrite { title, body } => {
            let Some(parent) = parent else {
                return Ok(Some((book, "BYTE needs a Notion page to add pages under: Settings → Connectors → Notion → \"Add new pages under\". Tell the user.".into())));
            };
            let (title, body) = if body.is_empty() {
                let Some(ans) = last_answer(turn) else { return Ok(Some((book, "There's no answer in this chat to save yet. Ask what the page should say.".into()))) };
                (if title.is_empty() { ans.lines().map(|l| l.trim_start_matches(['#', '*', ' ', '>'])).find(|l| !l.trim().is_empty()).unwrap_or("From BYTE").chars().take(60).collect() } else { title }, ans)
            } else {
                (title, body)
            };
            let fields = vec![("Title".to_string(), title.clone()), ("Text".to_string(), body.chars().take(400).collect())];
            if !crate::macctl::ask_ok("Add this page to Notion?", "Notion", fields, cancel, send).await? {
                return Ok(Some((book, "The user chose not to add the page. Nothing was saved. Say so in one sentence.".into())));
            }
            let token = token.unwrap_or_default();
            let n = notion::Notion { http: &state.net, base: notion::API, token: &token };
            let page = n.create(&parent, &title, &body).await?;
            turn.log.record("notion_create", &json!({ "title": title }), true, &page.url);
            send(done_card(&format!("Added “{title}” to Notion"), &page.url, None))?;
            Ok(Some((book, format!("Done: the page \"{title}\" is in Notion: {}. Confirm in one sentence with the link.", page.url))))
        }
        Ask::Calendar { days, from_tomorrow } => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "calendar_links".into(), args: json!({ "days": days }) })?;
            let mut all = vec![];
            for c in calendars(&Secrets) {
                match ics::fetch(&state.net, &c.url).await {
                    Ok(e) => all.extend(e),
                    Err(e) => log::warn!("calendar {}: {e}", c.name),
                }
            }
            let start = if from_tomorrow { ics::today().succ_opt().unwrap_or(ics::today()) } else { ics::today() };
            let days_list = ics::upcoming(&all, start, days);
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} days with events", days_list.len()) })?;
            let mut notes = String::from("The user's calendar (from their calendar links; read-only):\n");
            if days_list.is_empty() {
                notes.push_str("Nothing scheduled.\n");
            }
            for (d, evs) in days_list {
                notes.push_str(&format!("{}:\n", d.format("%A, %B %-d")));
                for (t, s) in evs {
                    notes.push_str(&format!("- {t} — {s}\n"));
                }
            }
            notes.push_str("\nList these clearly by day, keeping the exact times. Don't add events.");
            Ok(Some((book, notes)))
        }
    }
}

// ----------------------------------------------------------------- commands

fn status_of(vault: Option<String>, parent: Option<String>) -> Status {
    let notes = vault.as_deref().map(|v| obsidian::notes(std::path::Path::new(v)).len()).unwrap_or(0);
    Status {
        keychain: cfg!(any(target_os = "macos", windows)),
        vault,
        vault_notes: notes,
        notion: Secrets.get(NOTION).ok().flatten().is_some(),
        notion_parent: parent,
        calendars: calendars(&Secrets).into_iter().map(|c| (c.name.clone(), host(&c.url))).collect(),
    }
}

#[tauri::command]
pub async fn connectors_status(state: State<'_, AppState>) -> AppResult<Status> {
    let (vault, parent) = {
        let s = state.settings.lock().await;
        (s.obsidian_vault.clone(), s.notion_parent.clone())
    };
    Ok(status_of(vault, parent))
}

async fn set_setting(state: &AppState, f: impl FnOnce(&mut crate::settings::Settings)) -> AppResult<()> {
    let mut s = state.settings.lock().await;
    f(&mut s);
    s.save(&state.paths.settings_file)
}

#[tauri::command]
pub async fn obsidian_set(state: State<'_, AppState>, path: Option<String>) -> AppResult<Status> {
    let path = match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => Some(obsidian::check(&p)?.display().to_string()),
        None => None,
    };
    set_setting(&state, |s| s.obsidian_vault = path).await?;
    connectors_status(state).await
}

/// Checks the secret with Notion, then keeps it in the Keychain.
#[tauri::command]
pub async fn notion_connect(state: State<'_, AppState>, secret: String, parent: Option<String>) -> AppResult<Status> {
    let secret = secret.trim().to_string();
    if secret.len() < 20 || secret.contains(char::is_whitespace) {
        return Err(AppError::msg("That doesn't look like a Notion secret (it starts with ntn_ or secret_)."));
    }
    let n = notion::Notion { http: &state.net, base: notion::API, token: &secret };
    n.me().await?;
    let parent = match parent.filter(|p| !p.trim().is_empty()) {
        Some(p) => Some(notion::page_id(&p).ok_or_else(|| AppError::msg("Paste the link of the Notion page new pages should go under."))?),
        None => None,
    };
    Secrets.set(NOTION, &secret)?;
    set_setting(&state, |s| s.notion_parent = parent).await?;
    connectors_status(state).await
}

#[tauri::command]
pub async fn notion_disconnect(state: State<'_, AppState>) -> AppResult<Status> {
    Secrets.delete(NOTION)?;
    set_setting(&state, |s| s.notion_parent = None).await?;
    connectors_status(state).await
}

/// Adds a calendar link after reading it once (so a wrong address is caught now).
#[tauri::command]
pub async fn calendar_link_add(state: State<'_, AppState>, name: String, url: String) -> AppResult<(Status, usize)> {
    let url = ics::normalize(&url)?;
    let events = ics::fetch(&state.net, &url).await?;
    let mut list = calendars(&Secrets);
    list.retain(|c| c.url != url);
    let name = if name.trim().is_empty() { host(&url) } else { name.trim().chars().take(60).collect() };
    list.push(Calendar { name, url });
    save_calendars(&Secrets, &list)?;
    Ok((connectors_status(state).await?, events.len()))
}

#[tauri::command]
pub async fn calendar_link_remove(state: State<'_, AppState>, index: usize) -> AppResult<Status> {
    let mut list = calendars(&Secrets);
    if index < list.len() {
        list.remove(index);
    }
    save_calendars(&Secrets, &list)?;
    connectors_status(state).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::keychain::MemoryStore;

    #[test]
    fn requests_go_to_the_right_connector() {
        let a = |q: &str| ask(q, true, true, true);
        assert_eq!(a("what do my obsidian notes say about sourdough?"), Some(Ask::VaultSearch("what do my notes say about sourdough".into())));
        assert_eq!(a("search my vault for tax receipts"), Some(Ask::VaultSearch("search for tax receipts".into())));
        assert_eq!(a("add a note to Obsidian: Groceries — milk, eggs"), Some(Ask::VaultWrite { title: "Groceries".into(), body: "milk, eggs".into() }));
        assert_eq!(a("save that to obsidian"), Some(Ask::VaultWrite { title: String::new(), body: String::new() }));
        assert_eq!(a("search Notion for the Lisbon trip"), Some(Ask::NotionSearch("lisbon trip".into())));
        assert_eq!(a("add a page to Notion: Packing list — passport, charger"), Some(Ask::NotionWrite { title: "Packing list".into(), body: "passport, charger".into() }));
        assert_eq!(a("what's on my calendar tomorrow?"), Some(Ask::Calendar { days: 1, from_tomorrow: true }));
        assert_eq!(a("what's my schedule this week"), Some(Ask::Calendar { days: 7, from_tomorrow: false }));
        assert_eq!(a("add lunch to my calendar"), None, "adding events isn't read-only");
        assert_eq!(ask("what does my obsidian vault say about x", false, false, false), None, "not set up");
        assert_eq!(a("what is notion?"), None);
        assert_eq!(a("the notion of time"), None);
    }

    #[test]
    fn calendar_addresses_stay_in_the_secret_store() {
        let store = MemoryStore::default();
        assert!(calendars(&store).is_empty());
        save_calendars(&store, &[Calendar { name: "Work".into(), url: "https://calendar.google.com/private-abc/basic.ics".into() }]).unwrap();
        assert_eq!(calendars(&store)[0].name, "Work");
        assert_eq!(host(&calendars(&store)[0].url), "calendar.google.com");
        save_calendars(&store, &[]).unwrap();
        assert!(store.get(CALENDARS).unwrap().is_none());
    }
}
