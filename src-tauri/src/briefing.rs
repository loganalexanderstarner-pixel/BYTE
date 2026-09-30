//! The daily briefing (Phase 10): today's calendar, reminders and to-dos, the
//! weather where the user lives, and a few headlines on topics they follow.
//!
//! BYTE puts it together itself from what it read. No model writes it, so nothing
//! can be invented (small models added to-dos and news that didn't exist when they
//! wrote it), and it's ready even when no model is loaded. In chat ("give me my
//! briefing") it's the answer; on a schedule (scheduler.rs) it's saved as a chat.
//! Each part is skipped when it isn't available (Mac control off, no home town,
//! web off, nothing followed).

use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::AppResult;
use crate::macctl::{self, Action, Runner};
use crate::scheduler::Answer;
use crate::state::AppState;
use crate::tasks::Task;
use crate::tools::SourceBook;

/// Headlines per followed topic.
const PER_TOPIC: usize = 3;
/// Topics looked up at most.
const MAX_TOPICS: usize = 5;

pub fn applies(q: &str) -> bool {
    let l = q.trim().to_lowercase();
    let l = l.trim_end_matches(['?', '.', '!']);
    if l.starts_with("every ") || l.starts_with("each ") || l.contains(" every ") {
        return false; // a schedule (tasks.rs)
    }
    ["briefing", "brief me", "catch me up on today", "start my day", "what's my day look like", "what does my day look like", "how does my day look", "morning summary", "daily summary", "my day today"]
        .iter()
        .any(|w| l.contains(w))
        && !l.starts_with("what is a ")
        && !l.contains("write a briefing")
        && !l.contains("briefing document")
}

/// Everything the briefing needs to know about the user's setup.
pub struct Setup<'a> {
    pub net: &'a reqwest::Client,
    pub cloud: Option<&'a crate::cloud::CloudClient>,
    pub web: bool,
    pub home: Option<&'a str>,
    /// Mac control is on (read Calendar and Reminders).
    pub mac: bool,
    pub topics: &'a [String],
    pub tasks: &'a [Task],
}

/// What BYTE found for today.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Parts {
    /// "Wednesday, September 30".
    pub date: String,
    /// (time, title) in time order; None: calendar not read; Some(Err): couldn't be read.
    pub events: Option<Result<Vec<(String, String)>, String>>,
    /// Open Apple reminders ("title (due …)").
    pub reminders: Vec<String>,
    /// BYTE to-dos due today or overdue: (title, overdue).
    pub todos: Vec<(String, bool)>,
    /// To-dos with no date.
    pub undated: usize,
    /// (source number, "18°C, light rain", today's line, rain %, high).
    pub weather: Option<Weather>,
    /// (topic, [(source number, headline)]).
    pub news: Vec<(String, Vec<(u32, String)>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    pub n: u32,
    pub place: String,
    pub now: String,
    pub today: String,
    pub rain: Option<u32>,
    pub high: Option<f64>,
    pub imperial: bool,
}

/// "Wednesday, September 30, 2026 at 9:30:00 AM" → "9:30 AM" (any locale: the time part).
fn clock(s: &str) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    let Some(i) = words.iter().position(|w| w.contains(':') && w.chars().all(|c| c.is_ascii_digit() || c == ':')) else { return s.trim().to_string() };
    let t = words[i];
    let hm = if t.matches(':').count() == 2 { t.rsplit_once(':').map(|(a, _)| a).unwrap_or(t) } else { t };
    match words.get(i + 1).filter(|w| w.eq_ignore_ascii_case("am") || w.eq_ignore_ascii_case("pm")) {
        Some(ampm) => format!("{hm} {}", ampm.to_uppercase()),
        None => hm.to_string(),
    }
}

/// Minutes after midnight, for sorting ("9:30 AM", "14:00").
fn minutes(t: &str) -> u32 {
    let pm = t.to_uppercase().contains("PM");
    let am = t.to_uppercase().contains("AM");
    let hm = t.split_whitespace().next().unwrap_or("");
    let (h, m) = hm.split_once(':').unwrap_or((hm, "0"));
    let mut h: u32 = h.parse().unwrap_or(0);
    let m: u32 = m.parse().unwrap_or(0);
    if pm && h < 12 {
        h += 12;
    }
    if am && h == 12 {
        h = 0;
    }
    h * 60 + m
}

/// Calendar notes (macctl) → (time, title) in time order.
pub fn events_from(notes: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = notes
        .lines()
        .filter_map(|l| l.strip_prefix("- "))
        .filter_map(|l| {
            let (when, rest) = l.split_once(" — ")?;
            let title = rest.rsplit_once(" (").map(|(t, _)| t).unwrap_or(rest);
            Some((clock(when), title.trim().to_string()))
        })
        .collect();
    v.sort_by_key(|(t, _)| minutes(t));
    v
}

/// Adds events from calendar links to the Mac Calendar's (or stands in for it).
pub fn merge_events(p: &mut Parts, extra: Vec<(String, String)>) {
    let mut all = match p.events.take() {
        Some(Ok(v)) => v,
        _ => vec![],
    };
    for e in extra {
        if !all.contains(&e) {
            all.push(e);
        }
    }
    all.sort_by_key(|(t, _)| if t == "All day" { 0 } else { minutes(t) + 1 });
    p.events = Some(Ok(all));
}

/// Weather text (tools/weather) → the parts the briefing shows.
pub fn weather_from(n: u32, place: &str, text: &str) -> Option<Weather> {
    let imperial = text.contains("°F");
    let now = text.lines().find_map(|l| l.strip_prefix("Now: "))?.to_string();
    let today_line = text.lines().nth(2).unwrap_or("");
    let today = today_line.split_once(": ").map(|(_, r)| r).unwrap_or(today_line).to_string();
    let rain = today.split('%').next().and_then(|s| s.rsplit(' ').next()).and_then(|s| s.parse().ok());
    let high = today.split("high ").nth(1).and_then(|s| s.split('°').next()).and_then(|s| s.parse().ok());
    Some(Weather { n, place: place.to_string(), now: now.split(", wind").next().unwrap_or(&now).to_string(), today, rain, high, imperial })
}

/// Gathers today's parts, with the numbered sources the briefing cites.
pub async fn gather(s: &Setup<'_>, runner: &dyn Runner, now: chrono::DateTime<chrono::Local>, mut step: impl FnMut(&str, &str, bool)) -> (SourceBook, Parts) {
    let mut book = SourceBook::default();
    let mut p = Parts { date: now.format("%A, %B %-d").to_string(), ..Default::default() };

    if s.mac && cfg!(target_os = "macos") {
        match macctl::read_notes(runner, &Action::EventsList { days: 1 }).await {
            Ok(n) => {
                step("mac_events_list", "Read today's calendar", true);
                p.events = Some(Ok(events_from(&n)));
            }
            Err(e) => {
                step("mac_events_list", &e.text("Calendar"), false);
                p.events = Some(Err(e.text("Calendar")));
            }
        }
        if let Ok(n) = macctl::read_notes(runner, &Action::RemindersList { list: String::new() }).await {
            step("mac_reminders_list", "Read your reminders", true);
            p.reminders = n.lines().filter_map(|l| l.strip_prefix("- ")).map(str::to_string).take(12).collect();
        }
    }

    let now_ms = now.timestamp_millis();
    let end_of_day = now.date_naive().and_hms_opt(23, 59, 59).and_then(|d| d.and_local_timezone(chrono::Local).earliest()).map(|d| d.timestamp_millis()).unwrap_or(now_ms);
    let start_of_day = now.date_naive().and_hms_opt(0, 0, 0).and_then(|d| d.and_local_timezone(chrono::Local).earliest()).map(|d| d.timestamp_millis()).unwrap_or(now_ms);
    p.todos = s.tasks.iter().filter(|t| t.done_at.is_none() && t.due.is_some_and(|d| d <= end_of_day)).map(|t| (t.title.clone(), t.due.is_some_and(|d| d < start_of_day))).collect();
    p.undated = s.tasks.iter().filter(|t| t.done_at.is_none() && t.due.is_none()).count();

    if s.web {
        if let Some(home) = s.home.filter(|h| !h.trim().is_empty()) {
            match crate::tools::weather::forecast(s.net, home).await {
                Ok((place, text)) => {
                    let n = book.add(&format!("Weather for {}", place.label()), &crate::tools::weather::source_url(&place), "");
                    step("get_weather", &format!("Weather for {}", place.label()), true);
                    p.weather = weather_from(n, &place.label(), &text);
                }
                Err(e) => step("get_weather", &e.to_string(), false),
            }
        }
        for topic in s.topics.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).take(MAX_TOPICS) {
            let query = format!("{topic} news");
            match crate::tools::search::search(s.net, s.cloud, &query, 8).await {
                Ok(found) => {
                    step("web_search", &query, true);
                    let items: Vec<(u32, String)> = found.results.iter().take(PER_TOPIC).map(|r| (book.add(&r.title, &r.url, &r.snippet), r.title.trim().to_string())).collect();
                    if !items.is_empty() {
                        p.news.push((topic.to_string(), items));
                    }
                }
                Err(e) => step("web_search", &e.to_string(), false),
            }
        }
    }
    (book, p)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What to wear or bring, from the forecast.
fn weather_tip(w: &Weather) -> Option<&'static str> {
    let cold = w.high.is_some_and(|h| if w.imperial { h < 50.0 } else { h < 10.0 });
    let hot = w.high.is_some_and(|h| if w.imperial { h >= 88.0 } else { h >= 31.0 });
    match (w.rain.unwrap_or(0) >= 40, cold, hot) {
        (true, true, _) => Some("Bring an umbrella and a warm coat."),
        (true, _, _) => Some("Bring an umbrella."),
        (_, true, _) => Some("Wear a warm coat."),
        (_, _, true) => Some("It'll be hot: drink plenty of water."),
        _ => None,
    }
}

/// The one-line summary at the top.
pub fn tldr(p: &Parts) -> String {
    let mut bits = Vec::new();
    match &p.events {
        Some(Ok(e)) if e.is_empty() => bits.push("nothing on your calendar".to_string()),
        Some(Ok(e)) => bits.push(format!("{} (first at {})", plural(e.len(), "event", "events"), e[0].0)),
        _ => {}
    }
    let overdue = p.todos.iter().filter(|t| t.1).count();
    let due = p.todos.len() + p.reminders.len();
    if due > 0 {
        bits.push(format!("{} to do{}", due, if overdue > 0 { format!(" ({overdue} overdue)") } else { String::new() }));
    }
    if let Some(w) = &p.weather {
        bits.push(w.today.split(", high").next().unwrap_or(&w.today).to_string());
    }
    if !p.news.is_empty() {
        bits.push(format!("news on {}", p.news.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(", ")));
    }
    if bits.is_empty() {
        return format!("{}: a clear day.", p.date);
    }
    let mut s = format!("{}: {}.", p.date, bits.join("; "));
    if let Some(first) = s.get(0..1) {
        s = first.to_uppercase() + &s[1..];
    }
    s
}

/// The briefing as Markdown (sections with nothing in them are left out).
pub fn compose(p: &Parts, has_topics: bool) -> String {
    let mut out = vec![format!("> **TL;DR:** {}", tldr(p))];
    match &p.events {
        Some(Ok(e)) if !e.is_empty() => out.push(format!("## Today\n{}", e.iter().map(|(t, title)| format!("- **{t}** — {title}")).collect::<Vec<_>>().join("\n"))),
        Some(Ok(_)) => out.push("## Today\nNothing on your calendar.".into()),
        Some(Err(e)) => out.push(format!("## Today\nBYTE couldn't read your calendar. {e}")),
        None => {}
    }
    let mut todo: Vec<String> = p.todos.iter().filter(|t| t.1).map(|(t, _)| format!("- ⚠️ {t} *(overdue)*")).collect();
    todo.extend(p.todos.iter().filter(|t| !t.1).map(|(t, _)| format!("- {t}")));
    todo.extend(p.reminders.iter().map(|r| format!("- {r}")));
    if !todo.is_empty() {
        let more = if p.undated > 0 { format!("\n\n{} with no date on your list (✅).", plural(p.undated, "more task", "more tasks")) } else { String::new() };
        out.push(format!("## To-dos\n{}{more}", todo.join("\n")));
    }
    if let Some(w) = &p.weather {
        let tip = weather_tip(w).map(|t| format!(" {t}")).unwrap_or_default();
        out.push(format!("## Weather\n{}: now {}. Today: {} [{}].{tip}", w.place, w.now, w.today, w.n));
    }
    if !p.news.is_empty() {
        let lines: Vec<String> = p.news.iter().flat_map(|(topic, items)| items.iter().map(move |(n, title)| format!("- **{topic}:** {title} [{n}]"))).collect();
        out.push(format!("## News\n{}", lines.join("\n")));
    } else if !has_topics {
        out.push("*Tip: choose news topics to follow in Settings → Features → Daily briefing, and say \"every weekday at 7:30, brief me\" to get this automatically.*".into());
    }
    out.join("\n\n")
}

/// Runs it in chat: gathers the parts (shown as steps); the notes are the briefing
/// itself, which BYTE sends as the answer (`agent` doesn't ask the model).
pub async fn run(turn: &Turn<'_>, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(app) = turn.app else { return Ok(None) };
    let state = tauri::Manager::state::<AppState>(app);
    let topics = state.settings.lock().await.briefing_topics.clone();
    let tasks = crate::tasks::list(&state.db, false)?;
    let setup = Setup { net: turn.net, cloud: turn.cloud, web: turn.web, home: turn.home, mac: turn.modules.mac, topics: &topics, tasks: &tasks };
    let mut steps = Vec::new();
    let (book, mut parts) = tokio::select! {
        r = gather(&setup, &macctl::MacRunner, chrono::Local::now(), |name, summary, ok| steps.push((name.to_string(), summary.to_string(), ok))) => r,
        _ = cancel.cancelled() => return Err(crate::error::AppError::Cancelled),
    };
    if state.settings.lock().await.connectors_enabled {
        if let Some(evs) = crate::connectors::events_today(turn.net, &crate::connectors::Secrets).await {
            steps.push(("calendar_links".into(), "Read your calendar links".into(), true));
            merge_events(&mut parts, evs);
        }
    }
    for (i, (name, summary, ok)) in steps.into_iter().enumerate() {
        let id = format!("byte_brief_{i}");
        send(ChatEvent::ToolCall { id: id.clone(), name: name.clone(), args: json!({ "query": summary, "what": summary, "app": "BYTE" }) })?;
        send(ChatEvent::ToolResult { id, ok, summary })?;
    }
    Ok(Some((book, compose(&parts, !topics.is_empty()))))
}

/// Puts it together with nobody watching (the scheduled briefing).
pub async fn write(state: &AppState) -> Answer {
    let (web, home, mac, topics) = {
        let s = state.settings.lock().await;
        (s.web_search, s.home_place.clone(), s.mac_control, s.briefing_topics.clone())
    };
    let tasks = crate::tasks::list(&state.db, false).unwrap_or_default();
    let setup = Setup { net: &state.net, cloud: None, web, home: home.as_deref(), mac, topics: &topics, tasks: &tasks };
    let (book, mut parts) = gather(&setup, &macctl::MacRunner, chrono::Local::now(), |_, _, _| {}).await;
    if state.settings.lock().await.connectors_enabled {
        if let Some(evs) = crate::connectors::events_today(&state.net, &crate::connectors::Secrets).await {
            merge_events(&mut parts, evs);
        }
    }
    Answer { content: compose(&parts, !topics.is_empty()), sources: serde_json::to_value(&book.sources).unwrap_or(json!([])), error: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macctl::{Command, RunError};

    #[test]
    fn calendar_links_join_the_mac_calendar() {
        let mut p = Parts { events: Some(Ok(vec![("9:30 AM".into(), "Dentist".into())])), ..Default::default() };
        merge_events(&mut p, vec![("8:00 AM".into(), "Standup".into()), ("All day".into(), "Mom's birthday".into()), ("9:30 AM".into(), "Dentist".into())]);
        assert_eq!(p.events, Some(Ok(vec![("All day".into(), "Mom's birthday".into()), ("8:00 AM".into(), "Standup".into()), ("9:30 AM".into(), "Dentist".into())])));
        let mut none = Parts { events: Some(Err("Calendar isn't allowed".into())), ..Default::default() };
        merge_events(&mut none, vec![("1:00 PM".into(), "Lunch".into())]);
        assert_eq!(none.events, Some(Ok(vec![("1:00 PM".into(), "Lunch".into())])));
    }

    #[test]
    fn briefing_requests_are_recognized() {
        for q in ["Give me my briefing", "brief me", "what does my day look like?", "catch me up on today", "morning briefing please"] {
            assert!(applies(q), "{q}");
        }
        for q in ["every weekday at 7:30, brief me", "what is a briefing", "write a briefing document for the board", "brief history of Rome"] {
            assert!(!applies(q), "{q}");
        }
    }

    #[test]
    fn calendar_and_weather_are_read() {
        let notes = "The user's calendar today (from the Calendar app; events that repeat may be missing):\n- Wednesday, September 30, 2026 at 2:00:00 PM — Project review (Work)\n- Wednesday, September 30, 2026 at 9:30:00 AM — Dentist (Home)\n- Wednesday, September 30, 2026 at 12:00:00 PM — Lunch with Sam (Home)";
        assert_eq!(events_from(notes), vec![("9:30 AM".into(), "Dentist".into()), ("12:00 PM".into(), "Lunch with Sam".into()), ("2:00 PM".into(), "Project review".into())]);
        assert_eq!(events_from("- 30.09.2026 um 14:00:00 — Termin (Arbeit)"), vec![("14:00".into(), "Termin".into())], "other locales too");
        let text = "Weather for Pittsburgh, US (Open-Meteo forecast, local time 2026-09-30 08:00):\nNow: 61°F (feels like 59°F), light rain, wind 8 mph\nWednesday Sep 30: light rain, high 64°F, low 52°F, 70% chance of rain, wind up to 14 mph\nThursday Oct 1: clear sky, high 70°F, low 50°F, 5% chance of rain\n";
        let w = weather_from(1, "Pittsburgh, US", text).unwrap();
        assert_eq!(w.now, "61°F (feels like 59°F), light rain");
        assert_eq!((w.rain, w.high, w.imperial), (Some(70), Some(64.0), true));
        assert_eq!(weather_tip(&w), Some("Bring an umbrella."));
    }

    #[test]
    fn the_briefing_says_only_what_was_found() {
        let p = Parts {
            date: "Wednesday, September 30".into(),
            events: Some(Ok(vec![("9:30 AM".into(), "Dentist".into()), ("2:00 PM".into(), "Project review".into())])),
            reminders: vec!["Call Mom (due today at 5 PM)".into()],
            todos: vec![("pay rent".into(), true), ("buy milk".into(), false)],
            undated: 2,
            weather: weather_from(1, "Pittsburgh, US", "W\nNow: 18°C (feels like 17°C), light rain, wind 5 km/h\nWednesday Sep 30: light rain, high 21°C, low 12°C, 70% chance of rain\n"),
            news: vec![("AI".into(), vec![(2, "A lab released a small open model".into())])],
        };
        let md = compose(&p, true);
        assert!(md.starts_with("> **TL;DR:** Wednesday, September 30: 2 events (first at 9:30 AM); 3 to do (1 overdue); light rain; news on AI."), "{md}");
        assert!(md.contains("## Today\n- **9:30 AM** — Dentist\n- **2:00 PM** — Project review"));
        assert!(md.contains("- ⚠️ pay rent *(overdue)*\n- buy milk\n- Call Mom (due today at 5 PM)"));
        assert!(md.contains("2 more tasks with no date"));
        assert!(md.contains("## Weather\nPittsburgh, US: now 18°C (feels like 17°C), light rain. Today: light rain, high 21°C, low 12°C, 70% chance of rain [1]. Bring an umbrella."));
        assert!(md.contains("- **AI:** A lab released a small open model [2]"));
        assert!(!md.contains("Tip:"));

        // Nothing read at all: a short, honest note and the tip.
        let empty = compose(&Parts { date: "Thursday, October 1".into(), ..Default::default() }, false);
        assert!(empty.starts_with("> **TL;DR:** Thursday, October 1: a clear day."));
        assert!(!empty.contains("## Today") && empty.contains("Tip:"));
        // A calendar that couldn't be read says so.
        let err = compose(&Parts { date: "d".into(), events: Some(Err("macOS didn't let BYTE control Calendar.".into())), ..Default::default() }, true);
        assert!(err.contains("BYTE couldn't read your calendar. macOS didn't let BYTE control Calendar."));
    }

    struct Fake;
    impl Runner for Fake {
        fn run<'a>(&'a self, _cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>> {
            Box::pin(async move { Err(RunError::NotAllowed) })
        }
    }

    #[tokio::test]
    async fn only_whats_available_goes_in() {
        let net = reqwest::Client::new();
        let now = chrono::Local::now();
        let t = |id, title: &str, due: Option<i64>| Task { id, title: title.into(), notes: String::new(), due, remind_at: None, repeat: String::new(), done_at: None, created: 0 };
        let tasks = vec![t(1, "pay rent", Some(now.timestamp_millis() - 3 * 86_400_000)), t(2, "learn piano", None), t(3, "next month thing", Some(now.timestamp_millis() + 40 * 86_400_000))];
        // Web off, no Mac control: no network, no scripts.
        let setup = Setup { net: &net, cloud: None, web: false, home: Some("Pittsburgh"), mac: false, topics: &["AI".into()], tasks: &tasks };
        let mut steps = 0;
        let (book, p) = gather(&setup, &Fake, now, |_, _, _| steps += 1).await;
        assert!(book.sources.is_empty() && steps == 0);
        assert_eq!(p.todos, vec![("pay rent".to_string(), true)]);
        assert_eq!((p.undated, p.events.is_none(), p.weather.is_none()), (1, true, true));
    }
}
