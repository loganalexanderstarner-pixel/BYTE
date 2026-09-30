//! Trackers (Phase 10): packages, bills and subscriptions, birthdays and other
//! yearly dates (with gift ideas), and car and home maintenance.
//!
//! Everything is kept on this Mac. Each tracker has a next date; the scheduler's
//! tick sends one notification per date, a few days ahead (bills 3, events 14,
//! maintenance 7, packages on the day). Bills and yearly dates move on by
//! themselves; maintenance waits until it's marked done.
//!
//! Packages: BYTE recognizes the carrier from the tracking number and links to
//! the carrier's own tracking page. It doesn't read the status itself: carriers
//! block automated visits, and a guessed status would be worse than none.
//!
//! Chat requests are read with rules ("add Netflix $15.49 a month on the 12th",
//! "Sam's birthday is March 3", "change the furnace filter every 3 months",
//! "track 1Z999AA10123456784"), and lists are put together by BYTE (amounts and
//! dates are never left to a model).

use chrono::{Datelike, Days, Local, Months, NaiveDate};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager, State};

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools::SourceBook;

// ------------------------------------------------------------------ records

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Package,
    Bill,
    Event,
    Upkeep,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Package => "package",
            Kind::Bill => "bill",
            Kind::Event => "event",
            Kind::Upkeep => "upkeep",
        }
    }
    fn from_str(s: &str) -> Kind {
        match s {
            "bill" => Kind::Bill,
            "event" => Kind::Event,
            "upkeep" => Kind::Upkeep,
            _ => Kind::Package,
        }
    }
    /// Days ahead a notification is sent.
    pub fn default_notice(self) -> i64 {
        match self {
            Kind::Package => 0,
            Kind::Bill => 3,
            Kind::Event => 14,
            Kind::Upkeep => 7,
        }
    }
}

/// One tracked thing. Fields a kind doesn't use stay empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Tracker {
    pub id: i64,
    pub kind: Kind,
    pub name: String,
    /// The next date (YYYY-MM-DD): delivery, payment, the event, or maintenance due.
    pub next: Option<NaiveDate>,
    /// Days ahead to notify (None: the kind's default).
    pub notice_days: Option<i64>,
    pub notes: String,
    /// Delivered / cancelled / no longer tracked (kept for the record).
    pub done: bool,
    // Packages.
    pub carrier: String,
    pub number: String,
    // Bills: amount per cycle.
    pub amount: Option<f64>,
    pub currency: String,
    /// "weekly", "monthly", "quarterly", "yearly".
    pub cycle: String,
    // Events.
    pub person: String,
    /// "birthday", "anniversary", or anything else that repeats yearly.
    pub occasion: String,
    pub ideas: Vec<String>,
    pub budget: Option<f64>,
    // Maintenance: every N days or months, and when it was last done.
    pub every_days: Option<u32>,
    pub every_months: Option<u32>,
    pub last_done: Option<NaiveDate>,
    /// Filled when listed: the carrier's tracking page.
    pub link: String,
    /// The date a notification was sent for (so it's sent once).
    pub notified_for: Option<NaiveDate>,
}

impl Default for Tracker {
    fn default() -> Self {
        Tracker {
            id: 0,
            kind: Kind::Package,
            name: String::new(),
            next: None,
            notice_days: None,
            notes: String::new(),
            done: false,
            carrier: String::new(),
            number: String::new(),
            amount: None,
            currency: "USD".into(),
            cycle: String::new(),
            person: String::new(),
            occasion: String::new(),
            ideas: vec![],
            budget: None,
            every_days: None,
            every_months: None,
            last_done: None,
            link: String::new(),
            notified_for: None,
        }
    }
}

impl Tracker {
    fn notice(&self) -> i64 {
        self.notice_days.unwrap_or(self.kind.default_notice()).clamp(0, 60)
    }
}

// ------------------------------------------------------------------- dates

/// `date` moved on by the cycle.
fn step(date: NaiveDate, cycle: &str) -> Option<NaiveDate> {
    match cycle {
        "weekly" => date.checked_add_days(Days::new(7)),
        "monthly" => date.checked_add_months(Months::new(1)),
        "quarterly" => date.checked_add_months(Months::new(3)),
        "yearly" => date.checked_add_months(Months::new(12)),
        _ => None,
    }
}

/// The first date on or after `today` in the series through `anchor`. A monthly
/// bill on the 31st lands on the last day of shorter months, then goes back to the 31st.
pub fn next_on_or_after(anchor: NaiveDate, cycle: &str, today: NaiveDate) -> Option<NaiveDate> {
    let months = match cycle {
        "monthly" => 1,
        "quarterly" => 3,
        "yearly" => 12,
        "weekly" => {
            let mut d = anchor;
            while d < today {
                d = step(d, "weekly")?;
            }
            return Some(d);
        }
        _ => return None,
    };
    let mut n = 0u32;
    loop {
        let d = anchor.checked_add_months(Months::new(n * months))?;
        if d >= today {
            return Some(d);
        }
        n += 1;
        if n > 5000 {
            return None;
        }
    }
}

/// When maintenance is due next.
pub fn upkeep_due(t: &Tracker, today: NaiveDate) -> Option<NaiveDate> {
    let base = t.last_done.unwrap_or(today);
    match (t.every_months, t.every_days) {
        (Some(m), _) if m > 0 => base.checked_add_months(Months::new(m)),
        (_, Some(d)) if d > 0 => base.checked_add_days(Days::new(d as u64)),
        _ => None,
    }
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// The day number in "3", "3rd", "21st".
fn day_number(w: &str) -> Option<u32> {
    let w = w.trim_matches(|c: char| c == ',' || c == '.');
    let digits: String = w.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &w[digits.len()..];
    if digits.is_empty() || !(rest.is_empty() || ["st", "nd", "rd", "th"].contains(&rest)) {
        return None;
    }
    digits.parse().ok().filter(|d| (1..=31).contains(d))
}

/// A calendar date in words: "March 3", "3 March", "Mar 3rd", "3/14", "12/25/2026",
/// "today", "tomorrow", a weekday ("friday"), "on the 12th" (the next 12th).
/// Dates without a year are the next one on or after `today`.
pub fn date_in(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    let l = text.to_lowercase();
    let words: Vec<&str> = l.split_whitespace().collect();
    let month_at = |w: &str| {
        let w = w.trim_matches(|c: char| !c.is_alphabetic());
        (w.len() >= 3).then(|| MONTHS.iter().position(|m| w.starts_with(m) && m.starts_with(&w[..3]))).flatten()
    };
    let upcoming = |m: u32, d: u32, year: Option<i32>| {
        let y = year.unwrap_or(today.year());
        let date = NaiveDate::from_ymd_opt(y, m, d).or_else(|| NaiveDate::from_ymd_opt(y, m, d - 1))?;
        if year.is_none() && date < today {
            NaiveDate::from_ymd_opt(y + 1, m, d).or_else(|| NaiveDate::from_ymd_opt(y + 1, m, d - 1))
        } else {
            Some(date)
        }
    };
    let year_after = |i: usize| words.get(i).map(|w| w.trim_matches(|c: char| !c.is_ascii_digit())).and_then(|w| w.parse::<i32>().ok()).filter(|y| (2000..2200).contains(y));
    for (i, w) in words.iter().enumerate() {
        if let Some(m) = month_at(w) {
            // "March 3", "March 3rd, 2027"
            if let Some(d) = words.get(i + 1).and_then(|n| day_number(n)) {
                return upcoming(m as u32 + 1, d, year_after(i + 2));
            }
            // "3 March", "the 3rd of March"
            let before = if i >= 2 && words[i - 1] == "of" { words.get(i - 2) } else if i >= 1 { words.get(i - 1) } else { None };
            if let Some(d) = before.and_then(|n| day_number(n)) {
                return upcoming(m as u32 + 1, d, year_after(i + 1));
            }
        }
        // "3/14", "12/25/2026" (US order).
        let parts: Vec<&str> = w.trim_matches(|c: char| c == ',' || c == '.').split('/').collect();
        if (2..=3).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
            let (m, d) = (parts[0].parse::<u32>().ok()?, parts[1].parse::<u32>().ok()?);
            let y = parts.get(2).and_then(|y| y.parse::<i32>().ok()).map(|y| if y < 100 { 2000 + y } else { y });
            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                return upcoming(m, d, y);
            }
        }
    }
    if l.contains("day after tomorrow") {
        return today.checked_add_days(Days::new(2));
    }
    if l.contains("tomorrow") {
        return today.checked_add_days(Days::new(1));
    }
    if l.contains("today") {
        return Some(today);
    }
    if l.contains("yesterday") {
        return today.checked_sub_days(Days::new(1));
    }
    for (name, n) in [("monday", 0), ("tuesday", 1), ("wednesday", 2), ("thursday", 3), ("friday", 4), ("saturday", 5), ("sunday", 6)] {
        if words.iter().any(|w| w.trim_matches(|c: char| !c.is_alphabetic()).trim_end_matches('s') == name) {
            let ahead = (n - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
            return today.checked_add_days(Days::new(ahead as u64));
        }
    }
    // "on the 12th", "due the 1st": the next such day of the month.
    for (i, w) in words.iter().enumerate() {
        if i > 0 && words[i - 1] == "the" {
            if let Some(d) = day_number(w).filter(|_| w.chars().any(|c| c.is_alphabetic())) {
                let this = NaiveDate::from_ymd_opt(today.year(), today.month(), d.min(28)).and_then(|base| base.with_day(d).or(Some(base)))?;
                return if this >= today { Some(this) } else { this.checked_add_months(Months::new(1)) };
            }
        }
    }
    None
}

// ---------------------------------------------------------------- packages

/// The carrier a tracking number belongs to, and its tracking page.
pub fn carrier_of(number: &str) -> Option<(&'static str, String)> {
    let n: String = number.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_uppercase();
    let digits = n.chars().all(|c| c.is_ascii_digit());
    let carrier = if n.len() == 18 && n.starts_with("1Z") && n[2..].chars().all(|c| c.is_ascii_alphanumeric()) {
        "UPS"
    } else if n.len() == 15 && n.starts_with("TBA") && n[3..].chars().all(|c| c.is_ascii_digit()) {
        "Amazon"
    } else if n.len() == 13 && n.ends_with("US") && n[..2].chars().all(|c| c.is_ascii_alphabetic()) && n[2..11].chars().all(|c| c.is_ascii_digit()) {
        "USPS"
    } else if digits && (20..=22).contains(&n.len()) && ["91", "92", "93", "94", "95"].iter().any(|p| n.starts_with(p)) {
        "USPS"
    } else if digits && (n.len() == 12 || n.len() == 15 || (n.len() == 22 && n.starts_with("96")) || (n.len() == 20 && n.starts_with("96"))) {
        "FedEx"
    } else if (digits && n.len() == 10) || (n.starts_with("JJD") && n.len() >= 12) || (n.starts_with("JD") && n.len() == 20) {
        "DHL"
    } else {
        return None;
    };
    let link = match carrier {
        "UPS" => format!("https://www.ups.com/track?tracknum={n}"),
        "USPS" => format!("https://tools.usps.com/go/TrackConfirmAction?tLabels={n}"),
        "FedEx" => format!("https://www.fedex.com/fedextrack/?trknbr={n}"),
        "DHL" => format!("https://www.dhl.com/us-en/home/tracking/tracking-express.html?submit=1&tracking-id={n}"),
        _ => format!("https://track.amazon.com/tracking/{n}"),
    };
    Some((carrier, link))
}

/// A tracking number in a message (the first word that looks like one).
fn number_in(text: &str) -> Option<(String, &'static str, String)> {
    for w in text.split_whitespace() {
        let w = w.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if w.len() >= 10 && w.chars().any(|c| c.is_ascii_digit()) {
            if let Some((carrier, link)) = carrier_of(w) {
                return Some((w.to_uppercase(), carrier, link));
            }
        }
    }
    None
}

// -------------------------------------------------------------------- money

/// "$15.49", "15.49 dollars", "€9,99" → (amount, currency).
pub fn money_in(text: &str) -> Option<(f64, String)> {
    for w in text.split_whitespace() {
        let (cur, rest) = if let Some(r) = w.strip_prefix('$') {
            ("USD", r)
        } else if let Some(r) = w.strip_prefix('€') {
            ("EUR", r)
        } else if let Some(r) = w.strip_prefix('£') {
            ("GBP", r)
        } else {
            continue;
        };
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect();
        let num = num.trim_end_matches(['.', ',']);
        // "1,500" or "9,99".
        let value = if num.contains('.') || num.matches(',').count() > 1 || num.split(',').nth(1).is_some_and(|d| d.len() == 3) { num.replace(',', "") } else { num.replace(',', ".") };
        if let Ok(v) = value.parse::<f64>() {
            if v > 0.0 {
                return Some((v, cur.into()));
            }
        }
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if let (Ok(v), Some(unit)) = (w.replace(',', "").parse::<f64>(), words.get(i + 1)) {
            let unit = unit.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
            if v > 0.0 && ["dollars", "dollar", "bucks", "usd"].contains(&unit.as_str()) {
                return Some((v, "USD".into()));
            }
        }
    }
    None
}

pub fn money_text(v: f64, currency: &str) -> String {
    let s = if (v - v.round()).abs() < 0.005 { format!("{v:.0}") } else { format!("{v:.2}") };
    // Thousands separators: "1,500", "20,916.76".
    let (whole, cents) = s.split_once('.').map(|(w, c)| (w.to_string(), format!(".{c}"))).unwrap_or((s.clone(), String::new()));
    let digits: Vec<char> = whole.chars().collect();
    let mut grouped = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 && c.is_ascii_digit() && digits[i - 1].is_ascii_digit() {
            grouped.push(',');
        }
        grouped.push(*c);
    }
    let s = format!("{grouped}{cents}");
    match currency {
        "USD" | "" => format!("${s}"),
        "EUR" => format!("€{s}"),
        "GBP" => format!("£{s}"),
        c => format!("{s} {c}"),
    }
}

/// What a bill costs per month.
pub fn per_month(t: &Tracker) -> f64 {
    let a = t.amount.unwrap_or(0.0);
    match t.cycle.as_str() {
        "weekly" => a * 52.0 / 12.0,
        "quarterly" => a / 3.0,
        "yearly" => a / 12.0,
        _ => a,
    }
}

fn cycle_in(l: &str) -> Option<&'static str> {
    let has = |ws: &[&str]| ws.iter().any(|w| l.contains(w));
    if has(&["a month", "per month", "monthly", "every month", "/month", "/mo", "each month", "a mo"]) {
        Some("monthly")
    } else if has(&["a year", "per year", "yearly", "annually", "every year", "/year", "/yr", "annual", "each year"]) {
        Some("yearly")
    } else if has(&["a week", "per week", "weekly", "every week", "/week", "/wk", "each week"]) {
        Some("weekly")
    } else if has(&["quarterly", "every 3 months", "every three months", "a quarter", "per quarter"]) {
        Some("quarterly")
    } else {
        None
    }
}

// ----------------------------------------------------------------- storage

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Tracker> {
    let mut t: Tracker = serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default();
    t.id = r.get(0)?;
    t.kind = Kind::from_str(&r.get::<_, String>(1)?);
    if t.kind == Kind::Package {
        t.link = carrier_of(&t.number).map(|(_, l)| l).unwrap_or_default();
    }
    Ok(t)
}

pub fn list(db: &Db) -> AppResult<Vec<Tracker>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, kind, data FROM trackers ORDER BY done, next IS NULL, next, id")?;
    let v = st.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

pub fn get(db: &Db, id: i64) -> AppResult<Option<Tracker>> {
    Ok(db.conn().query_row("SELECT id, kind, data FROM trackers WHERE id = ?1", [id], row).optional()?)
}

/// Checks, fills in what follows from the rest (carrier, next dates) and saves.
pub fn save(db: &Db, t: &Tracker, today: NaiveDate) -> AppResult<Tracker> {
    let mut t = t.clone();
    t.name = t.name.trim().chars().take(100).collect();
    t.notes = t.notes.trim().chars().take(2000).collect();
    t.ideas.retain(|i| !i.trim().is_empty());
    t.ideas.truncate(30);
    match t.kind {
        Kind::Package => {
            if let Some((carrier, _)) = carrier_of(&t.number) {
                t.number = t.number.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_uppercase();
                if t.carrier.is_empty() {
                    t.carrier = carrier.into();
                }
            }
            if t.name.is_empty() {
                t.name = if t.carrier.is_empty() { "Package".into() } else { format!("{} package", t.carrier) };
            }
        }
        Kind::Bill => {
            if t.name.is_empty() {
                return Err(AppError::msg("Give the bill a name."));
            }
            if !["weekly", "monthly", "quarterly", "yearly"].contains(&t.cycle.as_str()) {
                t.cycle = "monthly".into();
            }
            if t.amount.is_some_and(|a| !(0.0..1e9).contains(&a)) {
                return Err(AppError::msg("That amount doesn't look right."));
            }
            let anchor = t.next.unwrap_or(today);
            t.next = next_on_or_after(anchor, &t.cycle, today);
        }
        Kind::Event => {
            let Some(date) = t.next else {
                return Err(AppError::msg("Say the date, like March 3."));
            };
            if t.name.is_empty() {
                t.name = match (t.person.is_empty(), t.occasion.is_empty()) {
                    (false, false) => format!("{}'s {}", t.person, t.occasion),
                    (false, true) => t.person.clone(),
                    _ => "Event".into(),
                };
            }
            t.next = next_on_or_after(date, "yearly", today);
        }
        Kind::Upkeep => {
            if t.name.is_empty() {
                return Err(AppError::msg("Say what needs doing, like “change the furnace filter”."));
            }
            if t.every_months.is_none() && t.every_days.is_none() {
                return Err(AppError::msg("Say how often, like every 3 months."));
            }
            t.next = upkeep_due(&t, today);
        }
    }
    let data = serde_json::to_string(&Tracker { link: String::new(), ..t.clone() })?;
    let next = t.next.map(|d| d.to_string());
    let id = {
        let conn = db.conn();
        if t.id > 0 {
            conn.execute("UPDATE trackers SET kind = ?1, next = ?2, done = ?3, data = ?4 WHERE id = ?5", params![t.kind.as_str(), next, t.done as i64, data, t.id])?;
            t.id
        } else {
            conn.execute(
                "INSERT INTO trackers (kind, next, done, data, created) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![t.kind.as_str(), next, t.done as i64, data, chrono::Utc::now().timestamp_millis()],
            )?;
            conn.last_insert_rowid()
        }
    };
    get(db, id)?.ok_or_else(|| AppError::msg("It wasn't saved."))
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    db.conn().execute("DELETE FROM trackers WHERE id = ?1", [id])?;
    Ok(())
}

/// "Done": a package delivered, maintenance done today (its next date moves on),
/// or a bill cancelled.
pub fn mark_done(db: &Db, id: i64, today: NaiveDate) -> AppResult<Tracker> {
    let mut t = get(db, id)?.ok_or_else(|| AppError::msg("That's no longer tracked."))?;
    match t.kind {
        Kind::Upkeep => t.last_done = Some(today),
        Kind::Event => {}
        _ => t.done = true,
    }
    save(db, &t, today)
}

/// Notifications due today; bills and yearly dates that passed move on.
pub fn take_due(db: &Db, today: NaiveDate) -> AppResult<Vec<(Tracker, String)>> {
    let mut out = vec![];
    for mut t in list(db)?.into_iter().filter(|t| !t.done) {
        // A passed bill or yearly date moves to its next time.
        if matches!(t.kind, Kind::Bill | Kind::Event) && t.next.is_some_and(|n| n < today) {
            t = save(db, &t, today)?;
        }
        let Some(next) = t.next else { continue };
        let notify_from = next.checked_sub_days(Days::new(t.notice() as u64)).unwrap_or(next);
        if today >= notify_from && t.notified_for != Some(next) {
            out.push((t.clone(), notice_text(&t, today)));
            t.notified_for = Some(next);
            save(db, &t, today)?;
        }
    }
    Ok(out)
}

/// "in 3 days", "tomorrow", "today", "2 days ago".
pub fn in_days(date: NaiveDate, today: NaiveDate) -> String {
    let d = (date - today).num_days();
    match d {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday".into(),
        d if d < 0 => format!("{} days ago", -d),
        d if d < 14 => format!("in {d} days"),
        _ => format!("on {}", date.format("%b %-d")),
    }
}

fn notice_text(t: &Tracker, today: NaiveDate) -> String {
    let when = t.next.map(|n| in_days(n, today)).unwrap_or_default();
    match t.kind {
        Kind::Package => format!("{} should arrive {when}.", t.name),
        Kind::Bill => format!("{} is due {when}{}.", t.name, t.amount.map(|a| format!(": {}", money_text(a, &t.currency))).unwrap_or_default()),
        Kind::Event => {
            let ideas = if t.ideas.is_empty() { " Ask BYTE for gift ideas.".to_string() } else { format!(" Gift ideas saved: {}.", t.ideas.join(", ")) };
            format!("{} is {when}.{ideas}", t.name)
        }
        Kind::Upkeep => {
            if t.next.is_some_and(|n| n < today) {
                format!("{} is overdue (due {when}).", t.name)
            } else {
                format!("Time to {} ({when}).", lower_first(&t.name))
            }
        }
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_lowercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Sends tracker notifications (from the scheduler's tick).
pub fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.settings.try_lock().map(|s| s.trackers_enabled).unwrap_or(true) {
        return;
    }
    match take_due(&state.db, Local::now().date_naive()) {
        Ok(due) => {
            for (t, text) in due {
                let title = match t.kind {
                    Kind::Package => "Package",
                    Kind::Bill => "Bill due",
                    Kind::Event => "Coming up",
                    Kind::Upkeep => "Maintenance",
                };
                crate::scheduler::notify(app, title, &text);
            }
        }
        Err(e) => log::warn!("trackers: {e}"),
    }
}

// --------------------------------------------------------------- summaries

/// A Markdown list BYTE writes itself (amounts and dates exact).
pub fn compose(all: &[Tracker], kind: Option<Kind>, today: NaiveDate) -> String {
    let open: Vec<&Tracker> = all.iter().filter(|t| !t.done && kind.is_none_or(|k| t.kind == k)).collect();
    let when = |t: &Tracker| t.next.map(|n| in_days(n, today)).unwrap_or_else(|| "no date".into());
    let mut out = String::new();
    let section = |k: Kind| open.iter().filter(move |t| t.kind == k).copied().collect::<Vec<_>>();
    let packages = section(Kind::Package);
    let bills = section(Kind::Bill);
    let events = section(Kind::Event);
    let upkeep = section(Kind::Upkeep);
    if kind.is_none() {
        // "What's coming up": the next 30 days, soonest first.
        let mut soon: Vec<&Tracker> = open.iter().filter(|t| t.next.is_some_and(|n| (n - today).num_days() <= 30)).copied().collect();
        soon.sort_by_key(|t| t.next);
        if soon.is_empty() {
            return "Nothing BYTE tracks is coming up in the next 30 days. See everything in the ✅ panel → Trackers.".into();
        }
        out.push_str("**Coming up in the next 30 days**\n\n");
        for t in soon {
            let extra = match t.kind {
                Kind::Bill => t.amount.map(|a| format!(" · {}", money_text(a, &t.currency))).unwrap_or_default(),
                Kind::Package => format!(" · {}", t.carrier),
                _ => String::new(),
            };
            let overdue = if t.next.is_some_and(|n| n < today) { " ⚠️ overdue" } else { "" };
            out.push_str(&format!("- **{}**: {}{extra}{overdue}\n", t.name, when(t)));
        }
        return out.trim_end().to_string();
    }
    match kind {
        Some(Kind::Package) => {
            if packages.is_empty() {
                return "No packages are being tracked. Paste a tracking number, like “track 1Z…”.".into();
            }
            out.push_str(&format!("**Packages on the way ({})**\n\n", packages.len()));
            for t in packages {
                let arrives = t.next.map(|n| format!(", expected {}", in_days(n, today))).unwrap_or_default();
                let link = carrier_of(&t.number).map(|(_, l)| format!(" · [track on {}]({l})", t.carrier)).unwrap_or_default();
                out.push_str(&format!("- **{}** ({}{arrives}){link}\n", t.name, t.number));
            }
            out.push_str("\nBYTE links to each carrier's own tracking page; it doesn't read the status itself.");
        }
        Some(Kind::Bill) => {
            if bills.is_empty() {
                return "No bills or subscriptions are being tracked. Add one like “add Netflix $15.49 a month on the 12th”.".into();
            }
            let monthly: f64 = bills.iter().map(|t| per_month(t)).sum();
            let cur = bills.first().map(|t| t.currency.clone()).unwrap_or_default();
            let mixed = bills.iter().any(|t| t.currency != cur);
            out.push_str(&format!("**Bills and subscriptions ({})**\n\n", bills.len()));
            if !mixed {
                out.push_str(&format!("> **TL;DR:** about **{} a month**, {} a year.\n\n", money_text(monthly, &cur), money_text(monthly * 12.0, &cur)));
            }
            out.push_str("| What | Amount | Next |\n|---|---|---|\n");
            for t in bills {
                let amount = t.amount.map(|a| format!("{} {}", money_text(a, &t.currency), cycle_word(&t.cycle))).unwrap_or_else(|| cycle_word(&t.cycle).into());
                out.push_str(&format!("| {} | {amount} | {} |\n", t.name, when(t)));
            }
        }
        Some(Kind::Event) => {
            if events.is_empty() {
                return "No birthdays or other dates are being tracked. Add one like “Sam's birthday is March 3”.".into();
            }
            out.push_str("**Birthdays and dates**\n\n");
            for t in events {
                let ideas = if t.ideas.is_empty() { String::new() } else { format!(" · ideas: {}", t.ideas.join(", ")) };
                let budget = t.budget.map(|b| format!(" · budget {}", money_text(b, &t.currency))).unwrap_or_default();
                out.push_str(&format!("- **{}**: {} ({}){budget}{ideas}\n", t.name, t.next.map(|n| n.format("%B %-d").to_string()).unwrap_or_default(), when(t)));
            }
        }
        Some(Kind::Upkeep) => {
            if upkeep.is_empty() {
                return "No car or home maintenance is being tracked. Add one like “change the furnace filter every 3 months”.".into();
            }
            out.push_str("**Car and home maintenance**\n\n");
            for t in upkeep {
                let last = t.last_done.map(|d| format!(", last done {}", d.format("%b %-d, %Y"))).unwrap_or_default();
                let overdue = if t.next.is_some_and(|n| n < today) { " ⚠️ overdue" } else { "" };
                out.push_str(&format!("- **{}**: due {}{overdue} (every {}{last})\n", t.name, when(t), every_text(t)));
            }
        }
        None => {}
    }
    out.trim_end().to_string()
}

fn cycle_word(c: &str) -> &'static str {
    match c {
        "weekly" => "a week",
        "quarterly" => "a quarter",
        "yearly" => "a year",
        _ => "a month",
    }
}

fn every_text(t: &Tracker) -> String {
    match (t.every_months, t.every_days) {
        (Some(1), _) => "month".into(),
        (Some(12), _) => "year".into(),
        (Some(m), _) if m % 12 == 0 => format!("{} years", m / 12),
        (Some(m), _) => format!("{m} months"),
        (_, Some(7)) => "week".into(),
        (_, Some(d)) if d % 7 == 0 => format!("{} weeks", d / 7),
        (_, Some(d)) => format!("{d} days"),
        _ => "?".into(),
    }
}

// ------------------------------------------------------------------ in chat

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Add(Tracker),
    List(Option<Kind>),
    /// Maintenance done / a package arrived / a bill cancelled (matched by name).
    Done { what: String, kind: Option<Kind> },
    /// "add AirPods to Sam's gift ideas".
    Idea { person: String, idea: String },
    /// "gift ideas for Sam": what's saved about them, for the model to suggest from.
    Suggest { person: String },
}

/// "every 3 months", "every 90 days", "every year", "every 5,000 miles" (miles are kept as a note).
fn every_in(l: &str) -> Option<(Option<u32>, Option<u32>)> {
    let i = l.find("every ")? + 6;
    let rest: Vec<&str> = l[i..].split_whitespace().collect();
    let (n, unit) = match rest.first().map(|w| w.replace(',', "")) {
        Some(w) if w.parse::<u32>().is_ok() => (w.parse::<u32>().ok()?, *rest.get(1)?),
        Some(w) => {
            let n = match w.as_str() {
                "other" => 2,
                "two" => 2,
                "three" => 3,
                "four" => 4,
                "six" => 6,
                "twelve" => 12,
                _ => 1,
            };
            let unit = if n == 1 && !["one", "a"].contains(&w.as_str()) { rest.first()? } else { rest.get(1)? };
            (n, *unit)
        }
        None => return None,
    };
    let unit = unit.trim_matches(|c: char| !c.is_alphabetic());
    match unit.trim_end_matches('s') {
        "day" => Some((None, Some(n))),
        "week" => Some((None, Some(n * 7))),
        "month" => Some((Some(n), None)),
        "year" => Some((Some(n * 12), None)),
        _ => None,
    }
}

const UPKEEP_WORDS: [&str; 24] = [
    "oil", "filter", "tire", "tyre", "brake", "battery", "inspection", "registration", "smoke detector", "gutter", "furnace", "hvac", "water heater", "dryer vent",
    "car wash", "wiper", "coolant", "rotate", "detail", "pest", "chimney", "lawn", "mower", "service",
];

fn clean(q: &str) -> String {
    q.trim().trim_end_matches(['.', '!']).trim().to_string()
}

/// Reads a tracker request from a message.
pub fn ask(q: &str, today: NaiveDate) -> Option<Ask> {
    let text = clean(q);
    let l = text.to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| l.contains(w));
    // Lists.
    if has(&["what's coming up", "what is coming up", "anything coming up", "what do i have coming up", "my trackers", "what am i tracking"]) {
        return Some(Ask::List(None));
    }
    if has(&["my packages", "packages am i", "packages on the way", "my deliveries", "where are my packages", "what packages", "which packages"]) {
        return Some(Ask::List(Some(Kind::Package)));
    }
    if has(&["my subscriptions", "my bills", "subscriptions do i", "bills do i", "what am i paying", "how much do i spend on subscriptions", "how much do i pay for subscriptions", "what subscriptions"]) {
        return Some(Ask::List(Some(Kind::Bill)));
    }
    if has(&["upcoming birthdays", "birthdays are coming", "whose birthday", "my birthdays", "birthdays coming up", "anniversaries coming up"]) {
        return Some(Ask::List(Some(Kind::Event)));
    }
    if has(&["my maintenance", "maintenance is due", "maintenance due", "what's due on my car", "what needs doing around the house", "home maintenance list", "car maintenance list"]) {
        return Some(Ask::List(Some(Kind::Upkeep)));
    }
    // Packages: a tracking number.
    if let Some((number, carrier, _)) = number_in(&text) {
        if has(&["track", "package", "order", "shipment", "delivery", "parcel", "tracking"]) {
            let name = ["for my ", "for the ", "for "]
                .iter()
                .find_map(|w| l.find(w).map(|p| text[p + w.len()..].split([',', '(']).next().unwrap_or("").trim().to_string()))
                .filter(|n| !n.is_empty() && !n.chars().any(|c| c.is_ascii_digit()) && n.split_whitespace().count() <= 6)
                .map(|n| upper_first(&n))
                .unwrap_or_default();
            let expected = if has(&["arriv", "expected", "coming", "deliver"]) { date_in(&text, today) } else { None };
            return Some(Ask::Add(Tracker { kind: Kind::Package, name, number, carrier: carrier.into(), next: expected, ..Default::default() }));
        }
    }
    // Done.
    if let Some(rest) = ["i changed ", "i replaced ", "i cleaned ", "i serviced ", "i did ", "we changed ", "we replaced ", "we cleaned ", "i got the ", "i had the "].iter().find_map(|p| l.strip_prefix(p)) {
        let end = [" today", " yesterday", " this morning", " just now", " done", " changed", " replaced", " serviced"].iter().filter_map(|w| rest.find(w)).min().unwrap_or(rest.len());
        let what = rest[..end].trim().trim_start_matches("the ").trim_start_matches("my ").to_string();
        if !what.is_empty() {
            return Some(Ask::Done { what, kind: Some(Kind::Upkeep) });
        }
    }
    if let Some(rest) = l.strip_prefix("my ") {
        if let Some(p) = rest.find(" arrived").or_else(|| rest.find(" came")).or_else(|| rest.find(" was delivered")).or_else(|| rest.find(" got delivered")) {
            return Some(Ask::Done { what: rest[..p].trim().to_string(), kind: Some(Kind::Package) });
        }
    }
    if let Some(rest) = l.strip_prefix("i cancelled ").or_else(|| l.strip_prefix("i canceled ")).or_else(|| l.strip_prefix("stop tracking ")) {
        let what = rest.trim().trim_start_matches("my ").trim_end_matches(" subscription").to_string();
        if !what.is_empty() {
            return Some(Ask::Done { what, kind: if l.starts_with("stop tracking") { None } else { Some(Kind::Bill) } });
        }
    }
    // Gift ideas.
    if let Some(p) = l.find("gift ideas for ").or_else(|| l.find("gift idea for ")).or_else(|| l.find("what should i get ")) {
        if l.starts_with("add ") {
            // "add AirPods to Sam's gift ideas" handled below.
        } else {
            let after = &text[p..];
            let after = after.split_once(" for ").map(|(_, r)| r).or_else(|| after.strip_prefix("what should i get ")).or_else(|| after.strip_prefix("What should I get ")).unwrap_or("");
            let person = after.split([' ', ',', '?']).next().unwrap_or("").trim_end_matches("'s").to_string();
            if !person.is_empty() {
                return Some(Ask::Suggest { person: upper_first(&person) });
            }
        }
    }
    if l.starts_with("add ") && (l.contains("gift ideas") || l.contains("gift list")) {
        if let Some(p) = l.find(" to ") {
            let idea = text[4..p].trim().to_string();
            let person = l[p + 4..].split("'s").next().unwrap_or("").trim().to_string();
            if !idea.is_empty() && !person.is_empty() && !person.contains(' ') {
                return Some(Ask::Idea { person: upper_first(&person), idea });
            }
        }
    }
    // Events: "Sam's birthday is March 3", "our anniversary is June 12".
    for occasion in ["birthday", "anniversary", "graduation", "wedding"] {
        if let Some(p) = l.find(&format!("{occasion} is")).or_else(|| l.find(&format!("{occasion} on"))).or_else(|| l.find(&format!("{occasion}:"))) {
            let who = l[..p].trim().trim_start_matches("remember ").trim_start_matches("add ").trim_start_matches("that ");
            let person = if who == "our" || who == "my" { String::new() } else { who.trim_end_matches("'s").trim_end_matches('’').trim_end_matches("’s").to_string() };
            if person.contains(' ') && !person.starts_with("my ") {
                continue;
            }
            let date = date_in(&l[p..], today)?;
            let person = upper_first(person.trim_start_matches("my "));
            let occasion_text = if who == "our" { format!("our {occasion}") } else { occasion.to_string() };
            let name = if person.is_empty() { upper_first(&occasion_text) } else { format!("{person}'s {occasion}") };
            let budget = if l.contains("budget") { money_in(&text).map(|(a, _)| a) } else { None };
            return Some(Ask::Add(Tracker { kind: Kind::Event, name, person, occasion: occasion.into(), next: Some(date), budget, ..Default::default() }));
        }
    }
    // Bills: an amount, a cycle and a bill-ish word or "add".
    if let (Some((amount, currency)), Some(cycle)) = (money_in(&text), cycle_in(&l)) {
        if has(&["add ", "track", "subscription", "bill", "i pay", "pay ", "costs", "is due", "due on", "renews", "membership", "rent", "insurance", "my "]) {
            let money_pos = text.find(['$', '€', '£']).unwrap_or(text.len());
            let head = text[..money_pos].trim();
            let head_l = head.to_lowercase();
            let mut name = head.to_string();
            for p in ["add a ", "add my ", "add ", "track my ", "track ", "i pay for ", "i pay ", "my ", "the "] {
                if head_l.starts_with(p) {
                    name = head[p.len()..].to_string();
                    break;
                }
            }
            let nl = name.to_lowercase();
            let cut = [" subscription", " bill", " for", " is", " costs", " at", " of", " -", ":", ","].iter().filter_map(|w| nl.find(w)).min().unwrap_or(name.len());
            let name = upper_first(name[..cut].trim());
            if name.is_empty() || name.split_whitespace().count() > 5 {
                return None;
            }
            let next = date_in(&text[money_pos..], today);
            return Some(Ask::Add(Tracker { kind: Kind::Bill, name, amount: Some(amount), currency, cycle: cycle.into(), next, ..Default::default() }));
        }
    }
    // Maintenance: "change the furnace filter every 3 months", "track oil changes every 6 months".
    if let Some((months, days)) = every_in(&l) {
        if UPKEEP_WORDS.iter().any(|w| l.contains(w)) && !l.starts_with("remind me") {
            let head = &text[..l.find("every ").unwrap_or(text.len())];
            let hl = head.to_lowercase();
            let head = ["track ", "add ", "i need to ", "we need to "].iter().find(|p| hl.starts_with(**p)).map(|p| &head[p.len()..]).unwrap_or(head);
            let name = upper_first(head.trim().trim_end_matches(','));
            if name.is_empty() {
                return None;
            }
            let last_done = if has(&["last done", "last changed", "last replaced", "did it", "changed it", "replaced it"]) { date_in(&text, today).filter(|d| *d <= today) } else { None };
            let notes = if l.contains("miles") || l.contains(" km") { text.split(" every ").nth(1).map(|s| format!("Also every {s}")).unwrap_or_default() } else { String::new() };
            return Some(Ask::Add(Tracker { kind: Kind::Upkeep, name, every_months: months, every_days: days, last_done, notes, ..Default::default() }));
        }
    }
    None
}

pub fn applies(q: &str) -> bool {
    ask(q, Local::now().date_naive()).is_some()
}

/// The tracker a name points at (words in common), of a kind if given.
fn find<'a>(all: &'a [Tracker], what: &str, kind: Option<Kind>) -> Option<&'a Tracker> {
    let words: Vec<String> = what.to_lowercase().split_whitespace().filter(|w| w.len() > 2 && !["the", "and", "package", "subscription"].contains(w)).map(|w| w.trim_end_matches('s').to_string()).collect();
    all.iter()
        .filter(|t| !t.done && kind.is_none_or(|k| t.kind == k))
        .map(|t| {
            let hay = format!("{} {} {}", t.name, t.carrier, t.person).to_lowercase();
            (words.iter().filter(|w| hay.contains(w.as_str())).count(), t)
        })
        .filter(|(n, _)| *n > 0)
        .max_by_key(|(n, _)| *n)
        .map(|(_, t)| t)
        .or_else(|| {
            // "my package arrived" with one package: that one.
            let open: Vec<&Tracker> = all.iter().filter(|t| !t.done && kind.is_some_and(|k| t.kind == k)).collect();
            (open.len() == 1 && words.is_empty()).then(|| open[0])
        })
}

fn done_card(title: &str, detail: &str) -> ChatEvent {
    ChatEvent::MacDone(crate::macctl::MacDone { app: "BYTE".into(), title: title.into(), detail: detail.into(), ok: true, undo: None })
}

/// Chat: adds, lists and updates trackers. Lists come back as BYTE's own reply.
pub async fn run(turn: &Turn<'_>, db: &Db, question: &str, send: Emit<'_>) -> AppResult<Option<(SourceBook, String, &'static str)>> {
    let today = Local::now().date_naive();
    let Some(a) = ask(question, today) else { return Ok(None) };
    let id = format!("byte_track_{}", uuid::Uuid::new_v4().simple());
    let none = SourceBook::default;
    match a {
        Ask::List(kind) => {
            let all = list(db)?;
            send(ChatEvent::ToolCall { id: id.clone(), name: "trackers_list".into(), args: json!({ "kind": kind.map(|k| k.as_str()) }) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} tracked", all.iter().filter(|t| !t.done && kind.is_none_or(|k| t.kind == k)).count()) })?;
            Ok(Some((none(), compose(&all, kind, today), "trackers_list")))
        }
        Ask::Add(t) => {
            let saved = save(db, &t, today)?;
            let (title, detail, note) = match saved.kind {
                Kind::Package => (
                    format!("Tracking {} ({})", saved.name, saved.number),
                    format!("{}{} · the ✅ panel has the tracking link", saved.carrier, saved.next.map(|n| format!(" · expected {}", in_days(n, today))).unwrap_or_default()),
                    format!("The {} tracking link is {}. BYTE doesn't read the status itself (carriers block automated visits), so point them to the link.", saved.carrier, saved.link),
                ),
                Kind::Bill => (
                    format!("Tracking {}", saved.name),
                    format!("{} {} · next {} · a reminder {} days before", saved.amount.map(|a| money_text(a, &saved.currency)).unwrap_or_default(), cycle_word(&saved.cycle), saved.next.map(|n| in_days(n, today)).unwrap_or_default(), saved.notice()),
                    format!("About {} a month.", money_text(per_month(&saved), &saved.currency)),
                ),
                Kind::Event => (
                    format!("Remembering {}", saved.name),
                    format!("{} ({}) · a reminder {} days before", saved.next.map(|n| n.format("%B %-d").to_string()).unwrap_or_default(), saved.next.map(|n| in_days(n, today)).unwrap_or_default(), saved.notice()),
                    "Offer to suggest gift ideas.".into(),
                ),
                Kind::Upkeep => (
                    format!("Tracking “{}”", saved.name),
                    format!("Every {} · next due {}", every_text(&saved), saved.next.map(|n| in_days(n, today)).unwrap_or_default()),
                    "Say “I did it today” (e.g. “I changed the oil today”) to mark it done; the next date moves on.".into(),
                ),
            };
            send(ChatEvent::ToolCall { id: id.clone(), name: "tracker_add".into(), args: json!({ "name": saved.name, "kind": saved.kind.as_str() }) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
            turn.log.record("tracker_add", &json!({ "name": saved.name, "kind": saved.kind.as_str() }), true, &detail);
            send(done_card(&title, &detail))?;
            Ok(Some((none(), format!("Done: {title} ({detail}). It's in the ✅ panel → Trackers, with a notification ahead of time. {note} Confirm in one or two short sentences."), "trackers")))
        }
        Ask::Done { what, kind } => {
            let all = list(db)?;
            let Some(t) = find(&all, &what, kind).cloned() else {
                return Ok(Some((none(), format!("BYTE isn't tracking anything called \"{what}\". {}", compose(&all, kind, today)), "trackers")));
            };
            let after = mark_done(db, t.id, today)?;
            let detail = match after.kind {
                Kind::Upkeep => format!("Next due {}", after.next.map(|n| in_days(n, today)).unwrap_or_default()),
                Kind::Package => "Marked delivered".into(),
                _ => "No longer tracked".into(),
            };
            send(ChatEvent::ToolCall { id: id.clone(), name: "tracker_done".into(), args: json!({ "name": t.name }) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
            send(done_card(&format!("Updated “{}”", t.name), &detail))?;
            Ok(Some((none(), format!("Done: \"{}\" is updated ({detail}). Confirm in one short sentence.", t.name), "trackers")))
        }
        Ask::Idea { person, idea } => {
            let all = list(db)?;
            let Some(mut t) = find(&all, &person, Some(Kind::Event)).cloned() else {
                return Ok(Some((none(), format!("BYTE doesn't have a birthday or date saved for {person}. Suggest adding one first, like \"{person}'s birthday is March 3\"."), "trackers")));
            };
            t.ideas.push(idea.clone());
            let saved = save(db, &t, today)?;
            send(done_card(&format!("Added “{idea}” to {}'s gift ideas", person), &format!("{} ideas saved", saved.ideas.len())))?;
            Ok(Some((none(), format!("Done: \"{idea}\" is saved as a gift idea for {person} ({} ideas so far: {}). Confirm in one sentence.", saved.ideas.len(), saved.ideas.join(", ")), "trackers")))
        }
        Ask::Suggest { person } => {
            let all = list(db)?;
            let about = find(&all, &person, Some(Kind::Event)).map(|t| {
                format!(
                    "Saved in BYTE: {} on {} ({}){}{}. {}",
                    t.name,
                    t.next.map(|n| n.format("%B %-d").to_string()).unwrap_or_default(),
                    t.next.map(|n| in_days(n, today)).unwrap_or_default(),
                    t.budget.map(|b| format!(", budget {}", money_text(b, &t.currency))).unwrap_or_default(),
                    if t.ideas.is_empty() { String::new() } else { format!(", ideas already saved: {}", t.ideas.join(", ")) },
                    if t.notes.is_empty() { String::new() } else { format!("Notes: {}", t.notes) }
                )
            });
            // Only notes: the model suggests ideas (the web helps when on).
            match about {
                Some(n) => Ok(Some((none(), format!("{n}\n\nSuggest 6–8 thoughtful gift ideas for {person} that fit this (and the budget if given), each with one line on why. Don't repeat the saved ideas. End by saying they can say \"add <idea> to {person}'s gift ideas\" to save one."), "trackers"))),
                None => Ok(None),
            }
        }
    }
}

// ----------------------------------------------------------------- commands

#[tauri::command]
pub fn trackers_list(state: State<'_, AppState>) -> AppResult<Vec<Tracker>> {
    list(&state.db)
}

#[tauri::command]
pub fn tracker_save(state: State<'_, AppState>, tracker: Tracker) -> AppResult<Tracker> {
    save(&state.db, &tracker, Local::now().date_naive())
}

#[tauri::command]
pub fn tracker_done(state: State<'_, AppState>, id: i64) -> AppResult<Tracker> {
    mark_done(&state.db, id, Local::now().date_naive())
}

#[tauri::command]
pub fn tracker_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete(&state.db, id)
}

/// Reads a date from words ("March 3", "the 12th", "friday") for the panel.
#[tauri::command]
pub fn tracker_date_parse(text: String) -> Option<String> {
    date_in(&text, Local::now().date_naive()).map(|d| d.to_string())
}

/// The carrier and tracking page for a number (the panel shows it as you type).
#[tauri::command]
pub fn tracker_carrier(number: String) -> Option<(String, String)> {
    carrier_of(&number).map(|(c, l)| (c.to_string(), l))
}

#[cfg(test)]
#[path = "trackers_tests.rs"]
mod tests;
