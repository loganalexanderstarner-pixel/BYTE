//! Calendar links (ICS / webcal): read-only calendars from a secret address,
//! like Google Calendar's "Secret address in iCal format" or an Outlook
//! published calendar. No account or app registration is needed.
//!
//! The parser covers what calendars really send: folded lines, all-day and
//! timed events, UTC times, and the common repeat rules (daily, weekly with
//! days, monthly, yearly; INTERVAL, COUNT, UNTIL, EXDATE). Times with a named
//! time zone are read as this Mac's local time (almost always the same zone).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{Datelike, Days, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc, Weekday};
use once_cell::sync::Lazy;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// "DAILY", "WEEKLY", "MONTHLY", "YEARLY".
    pub freq: String,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<NaiveDate>,
    pub by_day: Vec<Weekday>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub summary: String,
    pub start: NaiveDateTime,
    pub all_day: bool,
    pub location: String,
    pub rule: Option<Rule>,
    pub exdates: Vec<NaiveDate>,
}

/// Joins folded lines (a line starting with a space or tab continues the one before).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\").trim().to_string()
}

/// "20260930T090000Z", "20260930T090000", "20260930" (+ params) → local time, all-day.
fn when(params: &str, value: &str) -> Option<(NaiveDateTime, bool)> {
    let v = value.trim();
    if params.contains("VALUE=DATE") && !params.contains("VALUE=DATE-TIME") || v.len() == 8 {
        let d = NaiveDate::parse_from_str(v.get(..8)?, "%Y%m%d").ok()?;
        return Some((d.and_time(NaiveTime::MIN), true));
    }
    let base = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?;
    if v.ends_with('Z') {
        return Some((Utc.from_utc_datetime(&base).with_timezone(&Local).naive_local(), false));
    }
    Some((base, false))
}

fn weekday(s: &str) -> Option<Weekday> {
    // "MO", "1MO", "-1FR": the day part (the ordinal is ignored).
    let d = s.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit());
    Some(match d {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

fn rule(value: &str) -> Option<Rule> {
    let mut r = Rule { freq: String::new(), interval: 1, count: None, until: None, by_day: vec![] };
    for part in value.split(';') {
        let (k, v) = part.split_once('=')?;
        match k {
            "FREQ" => r.freq = v.to_string(),
            "INTERVAL" => r.interval = v.parse().ok().filter(|n| *n > 0).unwrap_or(1),
            "COUNT" => r.count = v.parse().ok(),
            "UNTIL" => r.until = when("", v).map(|(t, _)| t.date()),
            "BYDAY" => r.by_day = v.split(',').filter_map(weekday).collect(),
            _ => {}
        }
    }
    ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"].contains(&r.freq.as_str()).then_some(r)
}

/// All events in a calendar file (cancelled ones left out).
pub fn parse(text: &str) -> Vec<Event> {
    let mut out = vec![];
    let mut cur: Option<(Event, bool)> = None;
    for line in unfold(text) {
        if line == "BEGIN:VEVENT" {
            cur = Some((Event { summary: String::new(), start: NaiveDateTime::MIN, all_day: false, location: String::new(), rule: None, exdates: vec![] }, false));
            continue;
        }
        if line == "END:VEVENT" {
            if let Some((e, cancelled)) = cur.take() {
                if !cancelled && e.start != NaiveDateTime::MIN {
                    out.push(e);
                }
            }
            continue;
        }
        let Some((e, cancelled)) = cur.as_mut() else { continue };
        let Some((head, value)) = line.split_once(':') else { continue };
        let (name, params) = head.split_once(';').unwrap_or((head, ""));
        match name {
            "SUMMARY" => e.summary = unescape(value),
            "LOCATION" => e.location = unescape(value),
            "DTSTART" => {
                if let Some((t, all_day)) = when(params, value) {
                    e.start = t;
                    e.all_day = all_day;
                }
            }
            "RRULE" => e.rule = rule(value),
            "EXDATE" => e.exdates.extend(value.split(',').filter_map(|v| when(params, v).map(|(t, _)| t.date()))),
            "STATUS" if value.trim() == "CANCELLED" => *cancelled = true,
            _ => {}
        }
    }
    out
}

/// Does the event happen on `day`?
pub fn occurs_on(e: &Event, day: NaiveDate) -> bool {
    let first = e.start.date();
    if day < first || e.exdates.contains(&day) {
        return false;
    }
    let Some(r) = &e.rule else { return day == first };
    if r.until.is_some_and(|u| day > u) {
        return false;
    }
    let matches = |d: NaiveDate| -> bool {
        match r.freq.as_str() {
            "DAILY" => (d - first).num_days() % r.interval as i64 == 0,
            "WEEKLY" => {
                let days = if r.by_day.is_empty() { vec![first.weekday()] } else { r.by_day.clone() };
                let week = |x: NaiveDate| (x - Days::new(x.weekday().num_days_from_monday() as u64)).num_days_from_ce() / 7;
                days.contains(&d.weekday()) && (week(d) - week(first)) % r.interval as i32 == 0
            }
            "MONTHLY" => {
                let months = (d.year() - first.year()) * 12 + d.month() as i32 - first.month() as i32;
                d.day() == first.day() && months % r.interval as i32 == 0
            }
            "YEARLY" => d.month() == first.month() && d.day() == first.day() && (d.year() - first.year()) % r.interval as i32 == 0,
            _ => false,
        }
    };
    if !matches(day) {
        return false;
    }
    match r.count {
        None => true,
        Some(count) => {
            // Count the occurrences up to `day` (bounded: counts are small in practice).
            let mut n = 0u32;
            let mut d = first;
            while d <= day && n <= count {
                if matches(d) && !e.exdates.contains(&d) {
                    n += 1;
                }
                d = match d.succ_opt() {
                    Some(x) => x,
                    None => break,
                };
                if (d - first).num_days() > 3700 {
                    break;
                }
            }
            n <= count
        }
    }
}

/// (time, title) for a day, in time order; all-day events first as "All day".
pub fn on_day(events: &[Event], day: NaiveDate) -> Vec<(String, String)> {
    let mut v: Vec<(u32, String, String)> = events
        .iter()
        .filter(|e| occurs_on(e, day))
        .map(|e| {
            let title = if e.summary.is_empty() { "(no title)".to_string() } else { e.summary.clone() };
            if e.all_day {
                (0, "All day".to_string(), title)
            } else {
                let t = e.start.time();
                (1 + t.num_seconds_from_midnight() / 60, t.format("%-I:%M %p").to_string(), title)
            }
        })
        .collect();
    v.sort();
    v.dedup();
    v.into_iter().map(|(_, t, s)| (t, s)).collect()
}


/// webcal:// → https://; only http(s) addresses.
pub fn normalize(url: &str) -> AppResult<String> {
    let u = url.trim();
    let u = u.strip_prefix("webcal://").map(|r| format!("https://{r}")).unwrap_or_else(|| u.to_string());
    let parsed = reqwest::Url::parse(&u).map_err(|_| AppError::msg("That doesn't look like a calendar address."))?;
    if !["https", "http"].contains(&parsed.scheme()) {
        return Err(AppError::msg("Calendar addresses start with https:// or webcal://."));
    }
    Ok(u)
}

/// Fetched calendars, kept 30 minutes.
static CACHE: Lazy<Mutex<HashMap<String, (Instant, Vec<Event>)>>> = Lazy::new(|| Mutex::new(HashMap::new()));
const KEEP: Duration = Duration::from_secs(30 * 60);

/// Reads a calendar address (cached).
pub async fn fetch(net: &reqwest::Client, url: &str) -> AppResult<Vec<Event>> {
    let url = normalize(url)?;
    if let Some((at, events)) = CACHE.lock().ok().and_then(|m| m.get(&url).cloned()) {
        if at.elapsed() < KEEP {
            return Ok(events);
        }
    }
    let resp = net.get(&url).timeout(Duration::from_secs(20)).send().await.map_err(|e| AppError::msg(format!("Couldn't reach the calendar: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!("The calendar address answered {}; it may have been reset. Copy it again.", resp.status())));
    }
    let text = resp.text().await.map_err(|e| AppError::msg(e.to_string()))?;
    if !text.contains("BEGIN:VCALENDAR") {
        return Err(AppError::msg("That address isn't a calendar (no iCal data). Use the calendar's secret iCal address."));
    }
    let events = parse(&text);
    if let Ok(mut m) = CACHE.lock() {
        m.insert(url, (Instant::now(), events.clone()));
    }
    Ok(events)
}

/// Today's date for callers without one.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// The next `days` days, each with its events (days without any left out).
pub fn upcoming(events: &[Event], from: NaiveDate, days: u32) -> Vec<(NaiveDate, Vec<(String, String)>)> {
    (0..days)
        .filter_map(|i| from.checked_add_days(Days::new(i as u64)))
        .map(|d| (d, on_day(events, d)))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nSUMMARY:Dentist\r\nDTSTART:20260930T093000\r\nLOCATION:Main St\\, Suite 2\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Standup with a very long title that\r\n  continues here\r\nDTSTART;TZID=America/New_York:20260901T090000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\r\nEXDATE;TZID=America/New_York:20261002T090000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Mom's birthday\r\nDTSTART;VALUE=DATE:19600930\r\nRRULE:FREQ=YEARLY\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Gym\r\nDTSTART:20260928T180000\r\nRRULE:FREQ=DAILY;INTERVAL=2;COUNT=3\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Cancelled thing\r\nDTSTART:20260930T120000\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn calendars_are_read() {
        let events = parse(ICS);
        assert_eq!(events.len(), 4, "the cancelled one is left out");
        assert_eq!(events[0].location, "Main St, Suite 2");
        assert_eq!(events[1].summary, "Standup with a very long title that continues here", "folded lines are joined");
        assert!(events[2].all_day);
        // Wednesday, September 30, 2026.
        assert_eq!(on_day(&events, d(2026, 9, 30)), vec![
            ("All day".to_string(), "Mom's birthday".to_string()),
            ("9:00 AM".to_string(), "Standup with a very long title that continues here".to_string()),
            ("9:30 AM".to_string(), "Dentist".to_string()),
            ("6:00 PM".to_string(), "Gym".to_string()),
        ]);
    }

    #[test]
    fn repeats_follow_their_rules() {
        let events = parse(ICS);
        let standup = &events[1];
        assert!(occurs_on(standup, d(2026, 10, 5)), "Monday");
        assert!(!occurs_on(standup, d(2026, 10, 6)), "Tuesday");
        assert!(!occurs_on(standup, d(2026, 10, 2)), "excluded Friday");
        assert!(!occurs_on(standup, d(2026, 8, 31)), "before it started");
        let gym = &events[3];
        assert!(occurs_on(gym, d(2026, 9, 28)) && occurs_on(gym, d(2026, 9, 30)) && occurs_on(gym, d(2026, 10, 2)));
        assert!(!occurs_on(gym, d(2026, 9, 29)), "every other day");
        assert!(!occurs_on(gym, d(2026, 10, 4)), "only 3 times");
        assert!(occurs_on(&events[2], d(2027, 9, 30)), "yearly");
        let monthly = parse("BEGIN:VEVENT\nSUMMARY:Rent\nDTSTART;VALUE=DATE:20260101\nRRULE:FREQ=MONTHLY;UNTIL=20261231\nEND:VEVENT");
        assert!(occurs_on(&monthly[0], d(2026, 10, 1)));
        assert!(!occurs_on(&monthly[0], d(2027, 1, 1)), "until");
        assert_eq!(upcoming(&events, d(2026, 10, 1), 3), vec![(d(2026, 10, 2), vec![("6:00 PM".to_string(), "Gym".to_string())])], "Oct 2: the standup is excluded, the gym's last time");
    }

    #[test]
    fn addresses_are_checked() {
        assert_eq!(normalize("webcal://calendar.google.com/x/basic.ics").unwrap(), "https://calendar.google.com/x/basic.ics");
        assert!(normalize("file:///etc/passwd").is_err());
        assert!(normalize("not a url").is_err());
    }
}
