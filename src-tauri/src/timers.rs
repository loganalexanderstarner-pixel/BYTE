//! Timers and alarms: "set a timer for ten minutes", "how long is left on my pasta timer", "cancel the timer", "wake me
//! at 7:30". An alarm is a timer that ends at a clock time.
//!
//! They are kept in memory and in `timers.json` (so a restart does not lose them), and each one has a small task that
//! sleeps until it ends and rings: a Windows alarm toast (a looping sound until it is dismissed), a plain notification
//! elsewhere, and the `timer://done` event for the app to speak. Reminders (tasks.rs) work in whole minutes and are
//! checked every 30 seconds; a 20-second egg timer cannot live there.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{Local, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::AppResult;
use crate::state::AppState;
use crate::tools::SourceBook;

pub const DONE_EVENT: &str = "timer://done";
const FILE: &str = "timers.json";
/// The longest timer: a day. Further ahead is an alarm or a reminder.
/// Most timers and alarms running at once, so a runaway loop or a noisy room cannot pile them up.
const MOST: usize = 25;
const LONGEST: u64 = 24 * 3600;
/// A timer that ended while BYTE was closed still rings on the next start if it ended within this long ago.
const MISSED_WITHIN_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Timer {
    pub id: u64,
    pub label: String,
    /// When it ends (ms since 1970).
    pub ends: i64,
    #[serde(default)]
    pub alarm: bool,
}

static TIMERS: Mutex<Vec<Timer>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Set { secs: u64, label: String },
    Alarm { at: NaiveTime, label: String },
    Left { label: String },
    Cancel { label: String, all: bool },
    List,
}

// ------------------------------------------------------------------ words

/// Lowercase words and numbers, with digits split from letters ("10min" is "10", "min").
fn tokens(l: &str) -> Vec<String> {
    let chars: Vec<char> = l.to_lowercase().chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut num = false;
    for (i, &c) in chars.iter().enumerate() {
        let is_num = c.is_ascii_digit() || (c == '.' && i > 0 && chars[i - 1].is_ascii_digit() && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()));
        let is_word = c.is_alphabetic() || c == '\'';
        if is_num || is_word {
            if !cur.is_empty() && num != is_num {
                out.push(std::mem::take(&mut cur));
            }
            num = is_num;
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn small_number(w: &str) -> Option<f64> {
    Some(match w {
        "zero" => 0.0,
        "one" => 1.0,
        "two" => 2.0,
        "three" => 3.0,
        "four" => 4.0,
        "five" => 5.0,
        "six" => 6.0,
        "seven" => 7.0,
        "eight" => 8.0,
        "nine" => 9.0,
        "ten" => 10.0,
        "eleven" => 11.0,
        "twelve" => 12.0,
        "thirteen" => 13.0,
        "fourteen" => 14.0,
        "fifteen" => 15.0,
        "sixteen" => 16.0,
        "seventeen" => 17.0,
        "eighteen" => 18.0,
        "nineteen" => 19.0,
        _ => return None,
    })
}

fn tens(w: &str) -> Option<f64> {
    Some(match w {
        "twenty" => 20.0,
        "thirty" => 30.0,
        "forty" => 40.0,
        "fifty" => 50.0,
        "sixty" => 60.0,
        "ninety" => 90.0,
        "seventy" => 70.0,
        "eighty" => 80.0,
        _ => return None,
    })
}

/// Seconds in one unit word, if it is one. The one-letter forms ("5m", "90s") only count glued to a number.
fn unit(w: &str) -> Option<f64> {
    Some(match w {
        "second" | "seconds" | "sec" | "secs" | "s" => 1.0,
        "minute" | "minutes" | "min" | "mins" | "m" => 60.0,
        "hour" | "hours" | "hr" | "hrs" | "h" => 3600.0,
        _ => return None,
    })
}

/// A number starting at token `i`: how much it is and how many tokens it took.
fn value_at(t: &[String], i: usize) -> Option<(f64, usize)> {
    let w = t[i].as_str();
    if let Ok(v) = w.parse::<f64>() {
        return Some((v, 1));
    }
    if let Some(ten) = tens(w) {
        return match t.get(i + 1).and_then(|n| small_number(n)).filter(|n| (1.0..10.0).contains(n)) {
            Some(one) => Some((ten + one, 2)),
            None => Some((ten, 1)),
        };
    }
    if let Some(v) = small_number(w) {
        return Some((v, 1));
    }
    match w {
        "a" | "an" => Some((1.0, 1)),
        // "half an hour", "half a minute", "half hour"
        "half" => match t.get(i + 1).map(String::as_str) {
            Some("an") | Some("a") => Some((0.5, 2)),
            _ => Some((0.5, 1)),
        },
        _ => None,
    }
}

/// Every "number unit" in the words added up: "an hour and a half", "2 hours 30 minutes", "90 seconds".
fn seconds_in(l: &str) -> f64 {
    let t = tokens(l);
    let mut total = 0.0;
    let mut i = 0;
    while i < t.len() {
        let counted = value_at(&t, i).and_then(|(v, used)| {
            let word = t.get(i + used)?;
            // "m", "h" and "s" alone are letters; they are units only right after a typed number ("5m", "90s").
            let typed = t[i].chars().all(|c| c.is_ascii_digit() || c == '.');
            let u = unit(word).filter(|_| word.len() > 1 || typed)?;
            Some((v * u, u, used + 1))
        });
        match counted {
            Some((add, u, used)) => {
                total += add;
                i += used;
                let half = ["and", "a", "half"];
                if u > 1.0 && t.len() >= i + 3 && t[i..i + 3].iter().map(String::as_str).eq(half) {
                    total += 0.5 * u;
                    i += 3;
                }
            }
            None => i += 1,
        }
    }
    total
}

/// "pasta" in "set a pasta timer for 12 minutes".
fn label_of(t: &[String]) -> String {
    const SKIP: &[&str] = &[
        "set", "start", "make", "create", "put", "begin", "run", "give", "me", "a", "an", "the", "my", "new", "another", "for", "on", "up", "please", "cancel", "stop", "delete", "clear", "remove", "turn", "off", "kill", "end", "dismiss", "how", "much", "long", "time", "is", "left", "remaining", "what", "whats", "what's", "do", "i", "have", "of", "and", "all", "any", "show", "list", "wake", "at", "in", "from", "now", "that", "this", "am", "pm", "oclock", "o'clock", "tomorrow", "today", "tonight", "morning", "evening", "noon", "midnight",
    ];
    let cut = t.iter().position(|w| w == "timer" || w == "timers" || w == "alarm" || w == "alarms").unwrap_or(t.len());
    let words: Vec<&str> = t[..cut]
        .iter()
        .map(String::as_str)
        .filter(|w| !SKIP.contains(w) && w.parse::<f64>().is_err() && small_number(w).is_none() && tens(w).is_none() && unit(w).is_none() && *w != "half" && *w != "hundred")
        .collect();
    if words.len() > 3 {
        return String::new();
    }
    words.join(" ")
}

// ------------------------------------------------------------------ asking

pub fn applies(q: &str) -> bool {
    ask(q).is_some()
}

pub fn ask(q: &str) -> Option<Ask> {
    let l = crate::macctl::clean(crate::router::without_wake_phrase(q));
    if l.is_empty() || ["how do i", "how can i", "how to", "what is a ", "what's a ", "explain", "why "].iter().any(|p| l.starts_with(p)) {
        return None;
    }
    let t = tokens(&l);
    let has = |w: &str| t.iter().any(|x| x == w || *x == format!("{w}s"));
    let (timer, alarm, wake) = (has("timer"), has("alarm"), l.starts_with("wake me"));
    if !timer && !alarm && !wake {
        return None;
    }
    let starts = |ws: &[&str]| ws.iter().any(|w| l.starts_with(w));
    let label = label_of(&t);
    // "cancel my alarm" means the alarms, not a timer that happens to be running too.
    let target = if label.is_empty() && alarm && !timer { "alarm".to_string() } else { label.clone() };
    if starts(&["cancel", "stop", "delete", "clear", "remove", "turn off", "kill", "end ", "dismiss", "never mind"]) {
        return Some(Ask::Cancel { label: target, all: l.contains("all ") || l.contains("every ") });
    }
    if starts(&["list", "show"]) || ["what timers", "which timers", "what alarms", "which alarms", "any timers", "any alarms", "timers do i have", "alarms do i have", "timers are running", "timers i have", "how many timers"].iter().any(|p| l.contains(p)) {
        return Some(Ask::List);
    }
    if ["how much time", "how long", "time left", "time is left", "left on", "remaining", "how many minutes", "how many seconds"].iter().any(|p| l.contains(p)) {
        return Some(Ask::Left { label: target });
    }
    let secs = seconds_in(&l);
    if alarm || wake {
        if l.contains(" in ") && secs >= 1.0 && secs <= LONGEST as f64 {
            return Some(Ask::Set { secs: secs.round() as u64, label: "Alarm".into() });
        }
        if let Some(at) = crate::scheduler::time_in(&l) {
            return Some(Ask::Alarm { at, label: if label.is_empty() { "Alarm".into() } else { label } });
        }
        return None;
    }
    (timer && (1.0..=LONGEST as f64).contains(&secs)).then(|| Ask::Set { secs: secs.round() as u64, label })
}

// ------------------------------------------------------------------ doing

fn ms(dt: NaiveDateTime) -> i64 {
    Local.from_local_datetime(&dt).earliest().map(|t| t.timestamp_millis()).unwrap_or_else(|| dt.and_utc().timestamp_millis())
}

/// "1 hour 5 minutes", "45 seconds", "1 minute 30 seconds".
pub fn duration_text(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    let part = |n: u64, w: &str| (n > 0).then(|| format!("{n} {w}{}", if n == 1 { "" } else { "s" }));
    let parts: Vec<String> = [part(h, "hour"), part(m, "minute"), part(s, "second")].into_iter().flatten().collect();
    if parts.is_empty() {
        "0 seconds".into()
    } else {
        parts.join(" ")
    }
}

fn clock(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|t| t.format("%-I:%M %p").to_string()).unwrap_or_default()
}

fn name(t: &Timer) -> String {
    if t.label.is_empty() || t.label.eq_ignore_ascii_case("timer") {
        if t.alarm { "alarm".into() } else { "timer".into() }
    } else if t.alarm || t.label.eq_ignore_ascii_case("alarm") {
        t.label.to_lowercase()
    } else {
        format!("{} timer", t.label)
    }
}

/// What a request did, in words for the card and for the model.
#[derive(Debug, Default, PartialEq)]
pub struct Done {
    pub card: String,
    pub detail: String,
    pub notes: String,
    pub started: Option<Timer>,
    pub removed: Vec<u64>,
}

fn matching<'a>(timers: &'a [Timer], label: &str) -> Vec<&'a Timer> {
    if label.is_empty() {
        return timers.iter().collect();
    }
    let l = label.to_lowercase();
    timers.iter().filter(|t| t.label.to_lowercase().contains(&l) || (l == "alarm" && t.alarm)).collect()
}

fn left_text(t: &Timer, now_ms: i64) -> String {
    format!("{}: {} left (rings at {})", name(t), duration_text(((t.ends - now_ms).max(0) / 1000) as u64), clock(t.ends))
}

/// Changes the list for one request. `now` is the local time.
pub fn apply(timers: &mut Vec<Timer>, a: Ask, now: NaiveDateTime) -> Done {
    let now_ms = ms(now);
    let next_id = timers.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    if timers.len() >= MOST && matches!(a, Ask::Set { .. } | Ask::Alarm { .. }) {
        return Done { card: "Too many timers".into(), detail: String::new(), notes: format!("BYTE already has {MOST} timers running and can't add another. Say so in one short sentence."), started: None, removed: vec![] };
    }
    match a {
        Ask::Set { secs, label } => {
            let t = Timer { id: next_id, label, ends: now_ms + secs as i64 * 1000, alarm: false };
            let (len, rings) = (duration_text(secs), clock(t.ends));
            let kind = name(&t);
            timers.push(t.clone());
            Done {
                card: format!("{} for {len}", if kind == "timer" { "Timer set".to_string() } else { format!("{} set", capital(&kind)) }),
                detail: format!("Rings at {rings}"),
                notes: format!("Done: a {kind} is set for {len} and rings at {rings}. Confirm in one short sentence that says how long."),
                started: Some(t),
                removed: vec![],
            }
        }
        Ask::Alarm { at, label } => {
            let mut dt = now.date().and_time(at);
            if dt <= now {
                dt += chrono::Duration::days(1);
            }
            let tomorrow = dt.date() != now.date();
            let t = Timer { id: next_id, label, ends: ms(dt), alarm: true };
            let when = format!("{}{}", clock(t.ends), if tomorrow { " tomorrow" } else { "" });
            timers.push(t.clone());
            Done {
                card: format!("Alarm set for {when}"),
                detail: "Rings even if BYTE is in the background".into(),
                notes: format!("Done: the alarm is set for {when}. Confirm in one short sentence."),
                started: Some(t),
                removed: vec![],
            }
        }
        Ask::Left { label } => {
            let found = matching(timers, &label);
            if found.is_empty() {
                return Done { card: "No timer running".into(), detail: String::new(), notes: "There is no timer or alarm running. Say so in one short sentence and offer to set one.".into(), ..Default::default() };
            }
            let lines: Vec<String> = found.iter().map(|t| left_text(t, now_ms)).collect();
            Done { card: lines[0].clone(), detail: String::new(), notes: format!("Time left: {}. Say it in one short sentence.", lines.join("; ")), ..Default::default() }
        }
        Ask::Cancel { label, all } => {
            let found: Vec<Timer> = matching(timers, &label).into_iter().cloned().collect();
            if found.is_empty() {
                return Done { card: "No timer to cancel".into(), notes: "There is no timer or alarm running. Say so in one short sentence.".into(), ..Default::default() };
            }
            if found.len() > 1 && label.is_empty() && !all {
                let names: Vec<String> = found.iter().map(|t| format!("{} ({} left)", name(t), duration_text(((t.ends - now_ms).max(0) / 1000) as u64))).collect();
                return Done { card: "Which timer?".into(), detail: names.join(", "), notes: format!("{} timers are running: {}. Ask which one to cancel, or offer to cancel them all.", found.len(), names.join(", ")), ..Default::default() };
            }
            let ids: Vec<u64> = found.iter().map(|t| t.id).collect();
            timers.retain(|t| !ids.contains(&t.id));
            let what = if found.len() == 1 { format!("The {}", name(&found[0])) } else { format!("All {} timers", found.len()) };
            Done { card: format!("{what} is cancelled").replace("are cancelled is cancelled", "cancelled"), notes: format!("Done: {} cancelled. Confirm in one short sentence.", what.to_lowercase()), removed: ids, ..Default::default() }
        }
        Ask::List => {
            if timers.is_empty() {
                return Done { card: "No timers running".into(), notes: "No timer or alarm is running. Say so in one short sentence.".into(), ..Default::default() };
            }
            let mut sorted: Vec<&Timer> = timers.iter().collect();
            sorted.sort_by_key(|t| t.ends);
            let lines: Vec<String> = sorted.iter().map(|t| left_text(t, now_ms)).collect();
            Done { card: format!("{} running", lines.len()), detail: lines.join("\n"), notes: format!("Running now: {}. Read them out briefly.", lines.join("; ")), ..Default::default() }
        }
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

// ------------------------------------------------------------------ keeping

pub fn load(path: &Path) -> Vec<Timer> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(path: &Path, timers: &[Timer]) {
    let tmp = path.with_extension("json.tmp");
    let write = serde_json::to_string(timers).map_err(std::io::Error::other).and_then(|s| std::fs::write(&tmp, s)).and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = write {
        log::warn!("timers: could not save: {e}");
    }
}

fn persist(app: &AppHandle, timers: &[Timer]) {
    save(&app.state::<AppState>().paths.data.join(FILE), timers);
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Waits until the timer ends, looking at the clock every `step_ms` (30 s in the app: it notices a cancelled timer and a PC
/// that slept). `true` when it ended, `false` when it was cancelled first.
async fn wait_for(t: &Timer, step_ms: i64) -> bool {
    loop {
        let left = t.ends - now_ms();
        if left <= 0 {
            return TIMERS.lock().map(|g| g.iter().any(|x| x.id == t.id)).unwrap_or(false);
        }
        tokio::time::sleep(Duration::from_millis(left.min(step_ms) as u64)).await;
        if !TIMERS.lock().map(|g| g.iter().any(|x| x.id == t.id)).unwrap_or(false) {
            return false;
        }
    }
}

/// Takes the ended timer off the list; the rest of the list, to save, if it was still there.
fn take(t: &Timer) -> Option<Vec<Timer>> {
    let mut g = TIMERS.lock().ok()?;
    if !g.iter().any(|x| x.id == t.id) {
        return None;
    }
    g.retain(|x| x.id != t.id);
    Some(g.clone())
}

fn watch(app: AppHandle, t: Timer) {
    tauri::async_runtime::spawn(async move {
        if !wait_for(&t, 30_000).await {
            return;
        }
        if let Some(rest) = take(&t) {
            persist(&app, &rest);
            ring(&app, &t);
        }
    });
}

fn ring(app: &AppHandle, t: &Timer) {
    let title = if t.alarm { "Alarm" } else { "Timer done" };
    let text = if t.label.is_empty() || t.label.eq_ignore_ascii_case("timer") || t.label.eq_ignore_ascii_case("alarm") { name(t) } else { t.label.clone() };
    #[cfg(windows)]
    let shown = crate::toast::win::show(app, &crate::toast::timer_xml(title, &capital(&text))).is_ok();
    #[cfg(not(windows))]
    let shown = false;
    if !shown {
        crate::scheduler::notify(app, title, &capital(&text));
    }
    let _ = app.emit(DONE_EVENT, json!({ "label": text, "alarm": t.alarm, "say": say(t) }));
}

/// What BYTE says out loud when it rings: "Your pasta timer is done." / "Your alarm is going off."
pub fn say(t: &Timer) -> String {
    let n = name(t);
    if t.alarm { format!("Your {n} is going off.") } else { format!("Your {n} is done.") }
}

/// The running timers, soonest first (for the timers list in the app).
pub fn running() -> Vec<Timer> {
    let mut v = TIMERS.lock().map(|g| g.clone()).unwrap_or_default();
    v.sort_by_key(|t| t.ends);
    v
}

/// Cancels one by id; true when it was running.
pub fn cancel_id(app: &AppHandle, id: u64) -> bool {
    let rest = {
        let Ok(mut g) = TIMERS.lock() else { return false };
        let before = g.len();
        g.retain(|x| x.id != id);
        if g.len() == before {
            return false;
        }
        g.clone()
    };
    persist(app, &rest);
    true
}

/// At launch: bring back the timers that were running, ring the ones that ended while BYTE was closed (within the hour).
pub fn resume(app: AppHandle) {
    let path = app.state::<AppState>().paths.data.join(FILE);
    let now = now_ms();
    let mut keep = Vec::new();
    let mut missed = Vec::new();
    for t in load(&path) {
        if t.ends > now {
            keep.push(t);
        } else if now - t.ends <= MISSED_WITHIN_MS {
            missed.push(t);
        }
    }
    if let Ok(mut g) = TIMERS.lock() {
        *g = keep.clone();
    }
    save(&path, &keep);
    for t in keep {
        watch(app.clone(), t);
    }
    for t in missed {
        ring(&app, &Timer { label: format!("{} (ended while BYTE was closed)", t.label), ..t });
    }
}

// ------------------------------------------------------------------ in chat

pub async fn run(turn: &Turn<'_>, question: &str, _cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(a) = ask(question) else { return Ok(None) };
    let id = format!("byte_timer_{}", uuid::Uuid::new_v4().simple());
    let (done, snapshot) = {
        let mut g = TIMERS.lock().map_err(|_| crate::error::AppError::msg("timers are busy"))?;
        let d = apply(&mut g, a, Local::now().naive_local());
        (d, g.clone())
    };
    send(ChatEvent::ToolCall { id: id.clone(), name: "timer".into(), args: json!({ "request": question }) })?;
    send(ChatEvent::ToolResult { id, ok: true, summary: done.card.clone() })?;
    turn.log.record("timer", &json!({ "request": question }), true, &done.card);
    if let Some(app) = turn.app {
        persist(app, &snapshot);
        if let Some(t) = &done.started {
            watch(app.clone(), t.clone());
        }
    }
    send(ChatEvent::MacDone(crate::macctl::MacDone { app: "BYTE".into(), title: done.card.clone(), detail: done.detail.clone(), ok: true, undo: None }))?;
    Ok(Some((SourceBook::default(), done.notes)))
}

#[cfg(test)]
#[path = "timers_tests.rs"]
mod tests;
