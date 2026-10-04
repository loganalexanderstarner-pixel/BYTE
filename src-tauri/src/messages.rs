//! The Messages inbox (macOS, opt-in): texts you receive show up in BYTE with "Draft a reply" and "Reply".
//!
//! macOS keeps Messages' history in `~/Library/Messages/chat.db`, readable only with **Full Disk Access**
//! (System Settings → Privacy & Security), which the user turns on for BYTE. BYTE opens it **read-only**,
//! never changes it, and nothing leaves the Mac. Names come from the Contacts database the same way.
//! Sending goes through the Messages app (`macctl::MESSAGE_SEND`), into the same conversation.
//!
//! Off by default (setting `messagesInbox`); kids mode and the lock keep it closed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const NEW_EVENT: &str = "messages://new";

/// Seconds between 1970 and 2001-01-01, Apple's epoch.
const APPLE_EPOCH: i64 = 978_307_200;

/// The inbox is on (mirrors the setting, so `chat_for` never touches chat.db when it's off).
static ENABLED: AtomicBool = AtomicBool::new(false);
/// The newest message already seen by the watcher (0: not started).
static LAST_SEEN: AtomicI64 = AtomicI64::new(0);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on {
        LAST_SEEN.store(0, Ordering::Relaxed);
    }
}

pub const NEEDS_ACCESS: &str = "BYTE can't read your messages yet. Turn on Full Disk Access for BYTE: System Settings → \
Privacy & Security → Full Disk Access → switch on BYTE (use + to add it from Applications if it isn't listed), then reopen BYTE.";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub available: bool,
    pub enabled: bool,
    pub granted: bool,
    pub message: String,
}

/// A conversation, newest first.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// Messages' conversation id (`chat.guid`), used to reply in the same thread.
    pub chat: String,
    pub name: String,
    /// The other person's number or email (empty for groups).
    pub handle: String,
    pub group: bool,
    pub last_text: String,
    pub last_at: i64,
    pub last_from_me: bool,
    pub unread: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Msg {
    pub text: String,
    pub at: i64,
    pub from_me: bool,
    /// Who sent it (a name when Contacts knows them).
    pub sender: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewText {
    pub chat: String,
    pub name: String,
    pub text: String,
    pub at: i64,
}

// ------------------------------------------------------------------ reading chat.db

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn db_path() -> Option<PathBuf> {
    home().map(|h| h.join("Library/Messages/chat.db"))
}

fn open_ro(path: &std::path::Path) -> AppResult<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI)
        .map_err(|_| AppError::msg(NEEDS_ACCESS))?;
    c.busy_timeout(Duration::from_secs(2)).ok();
    // A read proves access (opening alone can succeed without it).
    c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).map_err(|_| AppError::msg(NEEDS_ACCESS))?;
    Ok(c)
}

fn open() -> AppResult<Connection> {
    let p = db_path().ok_or_else(|| AppError::msg(NEEDS_ACCESS))?;
    if !p.exists() {
        return Err(AppError::msg(NEEDS_ACCESS));
    }
    open_ro(&p)
}

/// Whether BYTE can read Messages' history (Full Disk Access is on).
pub fn has_access() -> bool {
    cfg!(target_os = "macos") && open().is_ok()
}

/// Apple's message dates: nanoseconds (or, on old Macs, seconds) since 2001 → Unix milliseconds.
pub fn unix_ms(apple: i64) -> i64 {
    let secs = if apple.abs() > 10_000_000_000 { apple / 1_000_000_000 } else { apple };
    (secs + APPLE_EPOCH) * 1000
}

/// The text inside `attributedBody` (newer macOS often leaves `text` empty): an NSAttributedString archive
/// whose NSString holds the words, after a '+' and a length (one byte, or 0x81 + 2 bytes, 0x82 + 4 bytes).
pub fn attributed_text(b: &[u8]) -> Option<String> {
    let at = b.windows(8).position(|w| w == b"NSString")? + 8;
    let plus = b[at..b.len().min(at + 12)].iter().position(|&c| c == b'+')? + at + 1;
    let (len, start) = match *b.get(plus)? {
        0x81 => (u16::from_le_bytes([*b.get(plus + 1)?, *b.get(plus + 2)?]) as usize, plus + 3),
        0x82 => (u32::from_le_bytes([*b.get(plus + 1)?, *b.get(plus + 2)?, *b.get(plus + 3)?, *b.get(plus + 4)?]) as usize, plus + 5),
        n => (n as usize, plus + 1),
    };
    let bytes = b.get(start..start.checked_add(len)?)?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// A message's words: `text`, else `attributedBody`; attachments become "[photo or file]".
fn words(text: Option<String>, body: Option<Vec<u8>>) -> String {
    let t = text.filter(|t| !t.trim().is_empty()).or_else(|| body.as_deref().and_then(attributed_text)).unwrap_or_default();
    let t = t.replace('\u{fffc}', "").trim().to_string();
    if t.is_empty() {
        "[photo or file]".into()
    } else {
        t
    }
}

// ------------------------------------------------------------------ names (Contacts)

/// Phone numbers compare by their last 10 digits; emails case-insensitively.
pub fn key_of(handle: &str) -> String {
    if handle.contains('@') {
        return handle.trim().to_lowercase();
    }
    let d: String = handle.chars().filter(char::is_ascii_digit).collect();
    d[d.len().saturating_sub(10)..].to_string()
}

static NAMES: Lazy<Mutex<Option<(Instant, HashMap<String, String>)>>> = Lazy::new(|| Mutex::new(None));

/// Names for numbers and emails, from Contacts' own databases (cached ten minutes).
fn names() -> HashMap<String, String> {
    if let Ok(g) = NAMES.lock() {
        if let Some((t, m)) = g.as_ref() {
            if t.elapsed() < Duration::from_secs(600) {
                return m.clone();
            }
        }
    }
    let mut map = HashMap::new();
    if let Some(root) = home().map(|h| h.join("Library/Application Support/AddressBook")) {
        let mut dbs = vec![root.join("AddressBook-v22.abcddb")];
        if let Ok(rd) = std::fs::read_dir(root.join("Sources")) {
            dbs.extend(rd.flatten().map(|e| e.path().join("AddressBook-v22.abcddb")));
        }
        for db in dbs.into_iter().filter(|p| p.exists()) {
            if let Ok(c) = open_ro(&db) {
                read_names(&c, &mut map);
            }
        }
    }
    if let Ok(mut g) = NAMES.lock() {
        *g = Some((Instant::now(), map.clone()));
    }
    map
}

fn read_names(c: &Connection, map: &mut HashMap<String, String>) {
    let name = |first: Option<String>, last: Option<String>, org: Option<String>| {
        let n = format!("{} {}", first.unwrap_or_default(), last.unwrap_or_default()).trim().to_string();
        if n.is_empty() {
            org.unwrap_or_default()
        } else {
            n
        }
    };
    for (table, col) in [("ZABCDPHONENUMBER", "ZFULLNUMBER"), ("ZABCDEMAILADDRESS", "ZADDRESS")] {
        let sql = format!("SELECT r.ZFIRSTNAME, r.ZLASTNAME, r.ZORGANIZATION, x.{col} FROM ZABCDRECORD r JOIN {table} x ON x.ZOWNER = r.Z_PK");
        let Ok(mut st) = c.prepare(&sql) else { continue };
        let rows = st.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)));
        if let Ok(rows) = rows {
            for (f, l, o, h) in rows.flatten() {
                let n = name(f, l, o);
                if let Some(h) = h.filter(|h| !h.trim().is_empty()) {
                    if !n.is_empty() {
                        map.entry(key_of(&h)).or_insert(n);
                    }
                }
            }
        }
    }
}

fn name_for(map: &HashMap<String, String>, handle: &str) -> String {
    map.get(&key_of(handle)).cloned().unwrap_or_else(|| handle.to_string())
}

// ------------------------------------------------------------------ queries

pub fn threads(c: &Connection, names: &HashMap<String, String>, limit: usize) -> AppResult<Vec<Thread>> {
    let mut st = c.prepare(
        "SELECT c.guid, IFNULL(c.display_name, ''), IFNULL(c.chat_identifier, ''), m.text, m.attributedBody, m.date, m.is_from_me, m.is_read,
                (SELECT count(*) FROM chat_handle_join WHERE chat_id = c.ROWID)
         FROM chat c
         JOIN message m ON m.ROWID = (SELECT max(message_id) FROM chat_message_join WHERE chat_id = c.ROWID)
         ORDER BY m.date DESC LIMIT ?1",
    )?;
    let rows = st.query_map([limit as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<Vec<u8>>>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?, r.get::<_, i64>(7)?, r.get::<_, i64>(8)?))
    })?;
    let mut out = Vec::new();
    for (chat, display, ident, text, body, date, from_me, read, members) in rows.flatten() {
        let group = members > 1;
        let name = if !display.trim().is_empty() { display } else if group { "Group chat".into() } else { name_for(names, &ident) };
        out.push(Thread {
            chat,
            name,
            handle: if group { String::new() } else { ident },
            group,
            last_text: words(text, body),
            last_at: unix_ms(date),
            last_from_me: from_me != 0,
            unread: from_me == 0 && read == 0,
        });
    }
    Ok(out)
}

pub fn thread(c: &Connection, names: &HashMap<String, String>, chat: &str, limit: usize) -> AppResult<Vec<Msg>> {
    let mut st = c.prepare(
        "SELECT m.text, m.attributedBody, m.date, m.is_from_me, IFNULL(h.id, '')
         FROM message m
         JOIN chat_message_join cmj ON cmj.message_id = m.ROWID
         JOIN chat c ON c.ROWID = cmj.chat_id
         LEFT JOIN handle h ON h.ROWID = m.handle_id
         WHERE c.guid = ?1 AND IFNULL(m.associated_message_type, 0) = 0
         ORDER BY m.date DESC LIMIT ?2",
    )?;
    let rows = st.query_map(rusqlite::params![chat, limit as i64], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<Vec<u8>>>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?)))?;
    let mut out: Vec<Msg> = rows
        .flatten()
        .map(|(text, body, date, me, h)| Msg { text: words(text, body), at: unix_ms(date), from_me: me != 0, sender: if me != 0 { "You".into() } else { name_for(names, &h) } })
        .collect();
    out.reverse();
    Ok(out)
}

/// Received texts newer than `after` (message ROWID), oldest first, with the newest id seen.
pub fn new_since(c: &Connection, names: &HashMap<String, String>, after: i64) -> AppResult<(Vec<NewText>, i64)> {
    let mut st = c.prepare(
        "SELECT m.ROWID, c.guid, IFNULL(c.display_name, ''), IFNULL(h.id, ''), m.text, m.attributedBody, m.date
         FROM message m
         JOIN chat_message_join cmj ON cmj.message_id = m.ROWID
         JOIN chat c ON c.ROWID = cmj.chat_id
         LEFT JOIN handle h ON h.ROWID = m.handle_id
         WHERE m.ROWID > ?1 AND m.is_from_me = 0 AND IFNULL(m.associated_message_type, 0) = 0
         ORDER BY m.ROWID LIMIT 20",
    )?;
    let rows = st.query_map([after], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<Vec<u8>>>(5)?, r.get::<_, i64>(6)?)))?;
    let mut last = after;
    let mut out = Vec::new();
    for (id, chat, display, h, text, body, date) in rows.flatten() {
        last = last.max(id);
        let who = name_for(names, &h);
        let name = if display.trim().is_empty() { who } else { format!("{who} in {display}") };
        out.push(NewText { chat, name, text: words(text, body), at: unix_ms(date) });
    }
    Ok((out, last))
}

fn newest_id(c: &Connection) -> i64 {
    c.query_row("SELECT IFNULL(max(ROWID), 0) FROM message", [], |r| r.get(0)).unwrap_or(0)
}

/// The one-to-one conversation with `handle`, when the inbox is on (so a new text lands in the existing thread).
pub fn chat_for(handle: &str) -> Option<String> {
    if !ENABLED.load(Ordering::Relaxed) || !cfg!(target_os = "macos") {
        return None;
    }
    let c = open().ok()?;
    chat_for_in(&c, handle)
}

fn chat_for_in(c: &Connection, handle: &str) -> Option<String> {
    let want = key_of(handle);
    let mut st = c
        .prepare(
            "SELECT c.guid, IFNULL(c.chat_identifier, '') FROM chat c
             WHERE (SELECT count(*) FROM chat_handle_join WHERE chat_id = c.ROWID) = 1 ORDER BY c.ROWID DESC",
        )
        .ok()?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).ok()?;
    let found = rows.flatten().find(|(_, ident)| !want.is_empty() && key_of(ident) == want).map(|(g, _)| g);
    found
}

// ------------------------------------------------------------------ watcher

/// Checks for new texts every 15 seconds while the inbox is on: a notification, and an event for the app.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            if ENABLED.load(Ordering::Relaxed) {
                check(&app).await;
            }
        }
    });
}

async fn check(app: &AppHandle) {
    if crate::kids::is_on() {
        return;
    }
    let notify = app.state::<AppState>().settings.lock().await.messages_notify;
    let found = tauri::async_runtime::spawn_blocking(|| -> AppResult<Vec<NewText>> {
        let c = open()?;
        let last = LAST_SEEN.load(Ordering::Relaxed);
        if last == 0 {
            // First look: start from now, so old texts don't all arrive at once.
            LAST_SEEN.store(newest_id(&c).max(1), Ordering::Relaxed);
            return Ok(Vec::new());
        }
        let (new, newest) = new_since(&c, &names(), last)?;
        LAST_SEEN.store(newest, Ordering::Relaxed);
        Ok(new)
    })
    .await;
    let new = match found {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            log::info!("messages: {e}");
            return;
        }
        Err(_) => return,
    };
    if new.is_empty() {
        return;
    }
    let _ = app.emit(NEW_EVENT, &new);
    // A locked BYTE shows who texted, not what they said.
    let locked = app.state::<AppState>().lock.is_locked();
    if notify {
        for t in new.iter().take(3) {
            crate::scheduler::notify(app, &t.name, if locked { "New message" } else { &t.text });
        }
    }
}

// ------------------------------------------------------------------ commands

fn ready(state: &AppState) -> AppResult<()> {
    if !cfg!(target_os = "macos") {
        return Err(AppError::msg("The Messages inbox needs a Mac."));
    }
    crate::lock::ensure(state)?;
    crate::kids::grownups_only()?;
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(AppError::msg("The Messages inbox is off. Turn it on in Settings → Privacy → Messages inbox."));
    }
    Ok(())
}

#[tauri::command]
pub async fn messages_status(state: State<'_, AppState>) -> AppResult<Status> {
    let enabled = state.settings.lock().await.messages_inbox;
    let available = cfg!(target_os = "macos");
    let granted = available && tauri::async_runtime::spawn_blocking(has_access).await.unwrap_or(false);
    Ok(Status { available, enabled, granted, message: if granted { String::new() } else { NEEDS_ACCESS.into() } })
}

#[tauri::command]
pub async fn messages_threads(state: State<'_, AppState>) -> AppResult<Vec<Thread>> {
    ready(&state)?;
    tauri::async_runtime::spawn_blocking(|| threads(&open()?, &names(), 60)).await.map_err(|e| AppError::msg(e.to_string()))?
}

#[tauri::command]
pub async fn messages_thread(state: State<'_, AppState>, chat: String) -> AppResult<Vec<Msg>> {
    ready(&state)?;
    tauri::async_runtime::spawn_blocking(move || thread(&open()?, &names(), &chat, 40)).await.map_err(|e| AppError::msg(e.to_string()))?
}

/// Sends from the inbox (the user pressed Send there, which is their OK): into conversation `chat`.
#[tauri::command]
pub async fn messages_send(state: State<'_, AppState>, chat: String, to: String, text: String) -> AppResult<String> {
    ready(&state)?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::msg("Write something to send first."));
    }
    let action = crate::macctl::Action::MessageSend { to: to.clone(), name: String::new(), body: text.clone(), chat };
    let out = crate::macctl::Runner::run(&crate::macctl::MacRunner, &action.command()).await.map_err(|e| AppError::msg(e.text("Messages")));
    let args = json!({ "to": to, "chars": text.chars().count() });
    match out {
        Ok(o) => {
            state.actions.record("mac_message_send", &args, true, "sent from the Messages inbox");
            Ok(if o.trim() == "sent sms" { "Sent as a text message (SMS)".into() } else { "Sent".into() })
        }
        Err(e) => {
            state.actions.record("mac_message_send", &args, false, &e.to_string());
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_dates_become_unix_ms() {
        // 2024-01-02T00:00:00Z: 725_846_400 s after 2001-01-01, as nanoseconds and as seconds.
        assert_eq!(unix_ms(725_846_400_000_000_000), 1_704_153_600_000);
        assert_eq!(unix_ms(725_846_400), 1_704_153_600_000);
    }

    #[test]
    fn reads_text_from_attributed_body() {
        let mut short = b"streamtyped\x81\xe8\x03\x84\x01@\x84\x84\x84\x12NSAttributedString\x00\x84\x84\x08NSObject\x00\x85\x92\x84\x84\x84\x08NSString\x01\x94\x84\x01+".to_vec();
        short.push(5);
        short.extend_from_slice(b"Hello\x86\x84");
        assert_eq!(attributed_text(&short).as_deref(), Some("Hello"));
        let long_text = "a".repeat(300);
        let mut long = b"..NSString\x01\x94\x84\x01+\x81".to_vec();
        long.extend_from_slice(&300u16.to_le_bytes());
        long.extend_from_slice(long_text.as_bytes());
        assert_eq!(attributed_text(&long).as_deref(), Some(long_text.as_str()));
        assert_eq!(attributed_text(b"no string here"), None);
        assert_eq!(words(None, None), "[photo or file]");
        assert_eq!(words(Some("  ".into()), Some(short)), "Hello");
    }

    #[test]
    fn keys_match_numbers_and_emails() {
        assert_eq!(key_of("+1 (412) 555-0123"), "4125550123");
        assert_eq!(key_of("4125550123"), "4125550123");
        assert_eq!(key_of("Sam@Example.com"), "sam@example.com");
    }

    /// A tiny chat.db with Messages' own columns.
    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, guid TEXT, display_name TEXT, chat_identifier TEXT);
             CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT);
             CREATE TABLE chat_handle_join (chat_id INTEGER, handle_id INTEGER);
             CREATE TABLE message (ROWID INTEGER PRIMARY KEY, text TEXT, attributedBody BLOB, date INTEGER, is_from_me INTEGER, is_read INTEGER, handle_id INTEGER, associated_message_type INTEGER);
             CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER);
             INSERT INTO handle VALUES (1, '+14125550123'), (2, 'sam@example.com');
             INSERT INTO chat VALUES (1, 'iMessage;-;+14125550123', '', '+14125550123'), (2, 'iMessage;+;chat99', 'Family', 'chat99');
             INSERT INTO chat_handle_join VALUES (1, 1), (2, 1), (2, 2);
             INSERT INTO message VALUES
               (1, 'Are you coming for dinner?', NULL, 725846400000000000, 0, 1, 1, 0),
               (2, 'Yes! 6pm', NULL, 725846460000000000, 1, 1, 0, 0),
               (3, NULL, NULL, 725846470000000000, 0, 1, 1, 2000),
               (4, 'Bring dessert', NULL, 725846500000000000, 0, 0, 1, 0),
               (5, 'Who is in?', NULL, 725846400000000000, 0, 1, 2, 0);
             INSERT INTO chat_message_join VALUES (1, 1), (1, 2), (1, 3), (1, 4), (2, 5);",
        )
        .unwrap();
        c
    }

    #[test]
    fn lists_threads_messages_and_new_texts() {
        let c = fixture();
        let names: HashMap<String, String> = [("4125550123".to_string(), "Mom".to_string())].into();
        let t = threads(&c, &names, 10).unwrap();
        assert_eq!(t[0].name, "Mom");
        assert_eq!((t[0].last_text.as_str(), t[0].unread, t[0].group), ("Bring dessert", true, false));
        assert_eq!((t[1].name.as_str(), t[1].group), ("Family", true));
        // Oldest first, reactions (associated messages) left out.
        let m = thread(&c, &names, "iMessage;-;+14125550123", 10).unwrap();
        assert_eq!(m.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["Are you coming for dinner?", "Yes! 6pm", "Bring dessert"]);
        assert_eq!((m[0].sender.as_str(), m[1].sender.as_str()), ("Mom", "You"));
        let (new, last) = new_since(&c, &names, 2).unwrap();
        assert_eq!(new.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["Bring dessert", "Who is in?"]);
        assert_eq!(new[1].name, "sam@example.com in Family");
        assert_eq!(last, 5);
        assert_eq!(chat_for_in(&c, "(412) 555-0123").as_deref(), Some("iMessage;-;+14125550123"));
        assert_eq!(chat_for_in(&c, "999"), None);
    }
}
