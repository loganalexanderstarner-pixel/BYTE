//! "What BYTE did" (off until chosen): a short summary of a day from the two activity logs, the PC control log (`control_log.jsonl`)
//! and the tool log (`actions.jsonl`). Only what kind of thing and when: counts and a time range, never a page, a search, a file name
//! or any typed text. It is shown in the morning briefing and in Settings.

use std::path::Path;

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Local time.
    pub at: NaiveDateTime,
    /// A plain kind, never a detail: "PC control: click", "Web: search"...
    pub group: &'static str,
    pub kind: &'static str,
    pub ok: bool,
}

/// What a control-log line says in words ("Click item 3", "Type 5 characters", "Open notepad") reduced to its kind.
pub fn control_kind(what: &str) -> &'static str {
    let w = what.trim().to_lowercase();
    let first = w.split_whitespace().next().unwrap_or("");
    match first {
        "click" => "clicks",
        "type" => "typing",
        "press" | "key" => "key presses",
        "open" => "programs or files opened",
        "close" => "windows closed",
        "focus" => "windows brought forward",
        "scroll" => "scrolls",
        "wait" => "waits",
        "re-planned" => "new plans",
        "shut" | "restart" | "sign" | "power" => "power actions",
        _ => "other steps",
    }
}

/// A tool name from actions.jsonl reduced to what it was.
pub fn tool_kind(tool: &str) -> (&'static str, &'static str) {
    match tool {
        "web_search" => ("Web", "searches"),
        "read_page" => ("Web", "pages read"),
        "academic_search" => ("Web", "paper searches"),
        "weather" => ("Web", "weather lookups"),
        "find_places" => ("Web", "place lookups"),
        "calculate" => ("Tools", "calculations"),
        "remember" => ("Tools", "memories saved"),
        "search_my_files" => ("Your files", "file searches"),
        t if t.starts_with("browser_") || t.starts_with("open_url") || t == "click_element" || t == "fill_field" => ("Browser", "steps"),
        _ => ("Other tools", "uses"),
    }
}

/// Reads the control log lines (`{"at":"2026-10-14 09:30:05","what":..,"ok":..}`).
pub fn read_control(text: &str) -> Vec<Event> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            let at = NaiveDateTime::parse_from_str(v["at"].as_str()?, "%Y-%m-%d %H:%M:%S").ok()?;
            Some(Event { at, group: "PC control", kind: control_kind(v["what"].as_str().unwrap_or("")), ok: v["ok"].as_bool().unwrap_or(false) })
        })
        .collect()
}

/// Reads the tool log lines (`{"ts":"<rfc3339>","tool":..,"ok":..}`); times become local.
pub fn read_actions(text: &str) -> Vec<Event> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            let ts = chrono::DateTime::parse_from_rfc3339(v["ts"].as_str()?).ok()?;
            let (group, kind) = tool_kind(v["tool"].as_str()?);
            Some(Event { at: ts.with_timezone(&Local).naive_local(), group, kind, ok: v["ok"].as_bool().unwrap_or(false) })
        })
        .collect()
}

fn hm(t: NaiveDateTime) -> String {
    t.format("%-I:%M %p").to_string()
}

fn count(n: usize, kind: &str) -> String {
    let one = match kind {
        "clicks" => "click",
        "key presses" => "key press",
        "scrolls" => "scroll",
        "waits" => "wait",
        "new plans" => "new plan",
        "power actions" => "power action",
        "other steps" => "other step",
        "searches" => "search",
        "paper searches" => "paper search",
        "weather lookups" => "weather lookup",
        "place lookups" => "place lookup",
        "calculations" => "calculation",
        "memories saved" => "memory saved",
        "file searches" => "file search",
        "pages read" => "page read",
        "steps" => "step",
        "uses" => "use",
        k => k,
    };
    format!("{n} {}", if n == 1 { one } else { kind })
}

/// The lines for one day, groups in a fixed order. Empty when nothing was done.
pub fn lines_for(day: NaiveDate, events: &[Event]) -> Vec<String> {
    let mut out = vec![];
    for group in ["PC control", "Web", "Browser", "Your files", "Tools", "Other tools"] {
        let todays: Vec<&Event> = events.iter().filter(|e| e.group == group && e.at.date() == day).collect();
        if todays.is_empty() {
            continue;
        }
        let first = todays.iter().map(|e| e.at).min().unwrap();
        let last = todays.iter().map(|e| e.at).max().unwrap();
        let mut kinds: Vec<(&str, usize)> = vec![];
        for e in &todays {
            match kinds.iter_mut().find(|(k, _)| *k == e.kind) {
                Some(k) => k.1 += 1,
                None => kinds.push((e.kind, 1)),
            }
        }
        kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let list = kinds.iter().take(4).map(|(k, n)| count(*n, k)).collect::<Vec<_>>().join(", ");
        let failed = todays.iter().filter(|e| !e.ok).count();
        let when = if first == last { format!("at {}", hm(first)) } else { format!("between {} and {}", hm(first), hm(last)) };
        let bad = if failed > 0 { format!("; {failed} did not work") } else { String::new() };
        out.push(format!("{group}: {list}, {when}{bad}."));
    }
    out
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    /// "2026-10-14".
    pub day: String,
    pub lines: Vec<String>,
}

pub fn summaries(control_text: &str, actions_text: &str, from: NaiveDate, days: u32) -> Vec<DaySummary> {
    let mut events = read_control(control_text);
    events.extend(read_actions(actions_text));
    (0..days)
        .filter_map(|i| from.checked_sub_days(chrono::Days::new(i as u64)))
        .map(|d| DaySummary { day: d.to_string(), lines: lines_for(d, &events) })
        .collect()
}

/// The briefing section for a day, or "" when there was nothing.
pub fn briefing_section(day_label: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    format!("## What BYTE did {day_label}\n{}", lines.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"))
}

fn read_file(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// Yesterday's section for the morning briefing (only when the setting is on).
pub fn briefing_for(state: &crate::state::AppState) -> String {
    let on = state.settings.try_lock().map(|s| s.daily_summary && !s.kids_mode).unwrap_or(false);
    if !on {
        return String::new();
    }
    let yesterday = Local::now().date_naive() - chrono::Days::new(1);
    let s = summaries(&read_file(&state.paths.data.join("control_log.jsonl")), &read_file(&state.paths.data.join("actions.jsonl")), yesterday, 1);
    briefing_section("yesterday", &s.first().map(|d| d.lines.clone()).unwrap_or_default())
}

/// The last `days` days (today first) for Settings.
#[tauri::command]
pub fn activity_summary(state: tauri::State<'_, crate::state::AppState>, days: Option<u32>) -> Vec<DaySummary> {
    summaries(&read_file(&state.paths.data.join("control_log.jsonl")), &read_file(&state.paths.data.join("actions.jsonl")), Local::now().date_naive(), days.unwrap_or(7).clamp(1, 31))
}

#[allow(dead_code)]
fn _local_ok() -> chrono::LocalResult<chrono::DateTime<Local>> {
    Local.timestamp_opt(0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTROL: &str = r#"{"at":"2026-10-14 09:12:03","what":"Open notepad","ok":true,"detail":""}
{"at":"2026-10-14 09:12:09","what":"Click item 3","ok":true,"detail":""}
{"at":"2026-10-14 09:13:00","what":"Type 5 characters","ok":true,"detail":""}
{"at":"2026-10-14 09:20:44","what":"Click item 9","ok":false,"detail":"item 9 is not in the window"}
{"at":"2026-10-13 18:00:00","what":"Click item 1","ok":true,"detail":""}
not json
"#;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
    }

    #[test]
    fn control_lines_become_kinds_without_details() {
        assert_eq!(control_kind("Click item 3"), "clicks");
        assert_eq!(control_kind("Open notepad"), "programs or files opened");
        assert_eq!(control_kind("Type 5 characters"), "typing");
        assert_eq!(control_kind("Something odd"), "other steps");
    }

    #[test]
    fn a_day_is_counted_with_a_time_range_and_failures() {
        let ev = read_control(CONTROL);
        let l = lines_for(day(), &ev);
        assert_eq!(l, vec!["PC control: 2 clicks, 1 programs or files opened, 1 typing, between 9:12 AM and 9:20 AM; 1 did not work.".to_string()]);
        assert_eq!(lines_for(NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(), &ev), vec!["PC control: 1 click, at 6:00 PM.".to_string()]);
        assert!(lines_for(NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(), &ev).is_empty());
    }

    #[test]
    fn nothing_that_was_typed_searched_or_opened_appears() {
        let actions = r#"{"ts":"2026-10-14T09:00:00+00:00","tool":"web_search","args":{"query":"my secret medical question"},"ok":true,"summary":"5 results"}
{"ts":"2026-10-14T09:01:00+00:00","tool":"read_page","args":{"url":"https://private.example/x"},"ok":true,"summary":"read it"}"#;
        let all = summaries(CONTROL, actions, day(), 2);
        let text = format!("{all:?}");
        for secret in ["secret", "medical", "private.example", "notepad", "item 3", "5 characters"] {
            assert!(!text.contains(secret), "{secret} must not appear: {text}");
        }
    }

    #[test]
    fn tool_log_times_are_local_and_grouped() {
        let local = Local.with_ymd_and_hms(2026, 10, 14, 10, 30, 0).unwrap();
        let actions = format!(
            "{{\"ts\":\"{}\",\"tool\":\"web_search\",\"ok\":true}}\n{{\"ts\":\"{}\",\"tool\":\"read_page\",\"ok\":false}}",
            local.to_rfc3339(),
            (local + chrono::Duration::minutes(5)).to_rfc3339()
        );
        let l = lines_for(day(), &read_actions(&actions));
        assert_eq!(l, vec!["Web: 1 page read, 1 search, between 10:30 AM and 10:35 AM; 1 did not work.".to_string()]);
    }

    #[test]
    fn the_briefing_section_is_empty_without_activity() {
        assert_eq!(briefing_section("yesterday", &[]), "");
        assert!(briefing_section("yesterday", &["Web: 1 searches, at 9:00 AM.".into()]).starts_with("## What BYTE did yesterday\n- Web"));
    }
}
