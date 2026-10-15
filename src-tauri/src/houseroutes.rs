//! Spoken and typed phrases that BYTE answers by itself, with nothing invented by a model (setting `recap`):
//!
//! - "where did I leave off?" / "where was I?": a recap built from the app's own latest chats, notes and open to-dos.
//! - "note: call the dentist" / "make a note ..." / "note to self ...": saved as a note in the Inbox folder of Notes
//!   ("todo: ..." or "task: ..." goes to the to-do list instead).
//! - "good morning" / "good morning BYTE": the daily briefing (briefing.rs), the same one "give me my briefing" gives.
//!
//! The one hook is in `agent::specialist` (marked `track:house`). The answer is the notes text itself: agent.rs sends it
//! as the reply for the `house` kind, without asking the model.

use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::AppResult;
use crate::tools::SourceBook;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Recap,
    /// A quick note (the text after "note:").
    Note(String),
    /// A to-do ("todo: ...").
    Todo(String),
    /// "good morning": the briefing.
    Morning,
}

fn clean(q: &str) -> String {
    q.trim().trim_end_matches(['.', '!', '?']).trim().to_string()
}

/// Which of these phrases is this, if any? Questions about notes in general ("what is a note", "note that the sky is blue") are not.
pub fn classify(q: &str) -> Option<Route> {
    let text = clean(q);
    let low = text.to_lowercase();
    let low = low.trim_start_matches(|c: char| !c.is_alphanumeric());
    // Notes: only an explicit leading phrase, so ordinary sentences containing "note" or "task" never save anything.
    // "note:" and "todo:" need the colon; the long forms need "that", "to" or a colon ("make a note of that" is not a note).
    for (prefix, todo) in [
        ("todo:", true),
        ("to-do:", true),
        ("note:", false),
        ("quick note:", false),
        ("note to self:", false),
        ("note to self,", false),
        ("note to self ", false),
        ("make a note:", false),
        ("make a note that ", false),
        ("make a note to ", false),
        ("take a note:", false),
        ("take a note that ", false),
        ("take a note to ", false),
        ("jot down ", false),
    ] {
        if low.starts_with(prefix) {
            let rest = text[text.len() - low.len() + prefix.len()..].trim().trim_start_matches([':', ',']).trim();
            let rest = rest.strip_prefix("that ").unwrap_or(rest).trim();
            if rest.to_lowercase().starts_with("of ") && prefix.starts_with("jot") {
                return None;
            }
            return (!rest.is_empty()).then(|| if todo { Route::Todo(rest.to_string()) } else { Route::Note(rest.to_string()) });
        }
    }
    let l = low.strip_prefix("hey byte").unwrap_or(low).trim_start_matches([',', ' ']);
    if matches!(l, "good morning" | "good morning byte" | "good morning, byte" | "morning byte") {
        return Some(Route::Morning);
    }
    // Whole phrases at the start, ending at a word boundary: "where was iPhone first sold" is a question, not a recap.
    const STARTS: &[&str] = &["where did i leave off", "where did we leave off", "what was i working on", "what was i doing", "what was i up to", "pick up where i left off", "remind me where i left off", "what did i do last time", "what did i do last session"];
    const EXACT: &[&str] = &["where was i", "catch me up"];
    let at_boundary = |p: &str| l.starts_with(p) && l[p.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
    (EXACT.contains(&l) || STARTS.iter().any(|p| at_boundary(p))).then_some(Route::Recap)
}

/// "5 minutes ago", "2 hours ago", "yesterday", "3 days ago".
pub fn ago(now_ms: i64, then_ms: i64) -> String {
    let mins = ((now_ms - then_ms).max(0) / 60_000) as i64;
    match mins {
        0 => "just now".into(),
        1 => "a minute ago".into(),
        2..=59 => format!("{mins} minutes ago"),
        60..=119 => "an hour ago".into(),
        120..=1439 => format!("{} hours ago", mins / 60),
        1440..=2879 => "yesterday".into(),
        _ => format!("{} days ago", mins / 1440),
    }
}

pub struct RecapChat {
    pub title: String,
    pub updated: i64,
    pub summary: Option<String>,
}

pub struct RecapNote {
    pub title: String,
    pub updated: i64,
}

pub struct RecapTask {
    pub title: String,
    pub overdue: bool,
}

/// The recap, in Markdown, built only from what is passed in.
pub fn compose_recap(now_ms: i64, chats: &[RecapChat], notes: &[RecapNote], tasks: &[RecapTask]) -> String {
    if chats.is_empty() && notes.is_empty() && tasks.is_empty() {
        return "There is nothing to catch you up on yet: no earlier chats, no notes and nothing on your to-do list.".into();
    }
    let mut out = vec!["Here is where you left off.".to_string()];
    if let Some(c) = chats.first() {
        let more = c.summary.as_deref().filter(|s| !s.trim().is_empty()).map(|s| format!(" {}", s.trim())).unwrap_or_default();
        out.push(format!("**Your last chat** was \"{}\", {}.{more}", c.title, ago(now_ms, c.updated)));
    }
    if chats.len() > 1 {
        let lines: Vec<String> = chats.iter().skip(1).map(|c| format!("- {} ({})", c.title, ago(now_ms, c.updated))).collect();
        out.push(format!("**Before that**\n{}", lines.join("\n")));
    }
    if !notes.is_empty() {
        let lines: Vec<String> = notes.iter().map(|n| format!("- {} ({})", n.title, ago(now_ms, n.updated))).collect();
        out.push(format!("**Latest notes**\n{}", lines.join("\n")));
    }
    if !tasks.is_empty() {
        let lines: Vec<String> = tasks.iter().map(|t| format!("- {}{}", t.title, if t.overdue { " (overdue)" } else { "" })).collect();
        out.push(format!("**Still on your to-do list**\n{}", lines.join("\n")));
    }
    out.join("\n\n")
}

/// A note's title: its first words.
pub fn note_title(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(8).collect();
    let mut t = words.join(" ");
    if text.split_whitespace().count() > 8 {
        t.push('…');
    }
    let mut c = t.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_else(|| "Note".into())
}

/// The hook. `Ok(None)`: not one of these phrases (or the setting is off), carry on with the other routes.
pub async fn try_route(turn: &Turn<'_>, q: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String, &'static str)>> {
    let Some(app) = turn.app else { return Ok(None) };
    let Some(route) = classify(q) else { return Ok(None) };
    let state = tauri::Manager::state::<crate::state::AppState>(app);
    if !state.settings.lock().await.recap || crate::kids::is_on() || state.lock.is_locked() {
        return Ok(None);
    }
    let none = SourceBook::default;
    let now = chrono::Utc::now().timestamp_millis();
    let step = |name: &str, summary: &str| -> AppResult<()> {
        let id = format!("byte_house_{}", uuid::Uuid::new_v4().simple());
        send(ChatEvent::ToolCall { id: id.clone(), name: name.into(), args: json!({ "what": summary, "app": "BYTE" }) })?;
        send(ChatEvent::ToolResult { id, ok: true, summary: summary.into() })
    };
    match route {
        Route::Morning => {
            if !turn.modules.tasks {
                return Ok(None);
            }
            Ok(crate::briefing::run(turn, cancel, send).await?.map(|(b, n)| (b, n, "house")))
        }
        Route::Recap => {
            let chats: Vec<RecapChat> = state
                .db
                .list()?
                .into_iter()
                // Not the chat being written right now (new, just touched).
                .filter(|c| !(c.message_count < 2 && now - c.updated_at < 120_000))
                .filter(|c| c.message_count > 0)
                .take(4)
                .map(|c| RecapChat { title: c.title, updated: c.updated_at, summary: c.summary })
                .collect();
            let mut chats = chats;
            chats.sort_by_key(|c| std::cmp::Reverse(c.updated));
            let notes: Vec<RecapNote> = match crate::notes::root(app).await {
                Ok(root) => {
                    let mut n = crate::notes::list(&root);
                    n.sort_by_key(|n| std::cmp::Reverse(n.updated));
                    n.into_iter().take(3).map(|n| RecapNote { title: n.title, updated: n.updated }).collect()
                }
                Err(_) => Vec::new(),
            };
            let tasks: Vec<RecapTask> = crate::tasks::list(&state.db, false)?.into_iter().take(4).map(|t| RecapTask { overdue: t.due.is_some_and(|d| d < now), title: t.title }).collect();
            step("recap", "Looked at your latest chats, notes and to-dos")?;
            Ok(Some((none(), compose_recap(now, &chats, &notes, &tasks), "house")))
        }
        Route::Note(text) => {
            let input = crate::notes::NoteInput { title: note_title(&text), folder: crate::notes::INBOX.into(), body: text.clone(), source: "voice note".into(), ..Default::default() };
            let saved = crate::notes::save_note(app, &input).await?;
            step("note_add", &format!("Saved \"{}\" to Notes", saved.title))?;
            Ok(Some((none(), format!("Saved a note: \"{text}\". It is in Notes, in the Inbox folder."), "house")))
        }
        Route::Todo(text) => {
            let t = crate::tasks::save(&state.db, &crate::tasks::Task { id: 0, title: text.clone(), notes: String::new(), due: None, remind_at: None, repeat: String::new(), done_at: None, created: 0 })?;
            step("task_add", &format!("Added \"{}\" to your to-do list", t.title))?;
            Ok(Some((none(), format!("Added \"{text}\" to your to-do list."), "house")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_need_an_explicit_phrase() {
        assert_eq!(classify("note: call the dentist"), Some(Route::Note("call the dentist".into())));
        assert_eq!(classify("Note to self, buy milk."), Some(Route::Note("buy milk".into())));
        assert_eq!(classify("make a note that the meeting moved to 3"), Some(Route::Note("the meeting moved to 3".into())));
        assert_eq!(classify("take a note: call mom"), Some(Route::Note("call mom".into())));
        assert_eq!(classify("jot down eggs and flour"), Some(Route::Note("eggs and flour".into())));
        assert_eq!(classify("todo: renew passport"), Some(Route::Todo("renew passport".into())));
        assert_eq!(classify("note:"), None, "an empty note is nothing");
        for not in ["what is a note in music", "please note that the sky is blue", "i noted it down yesterday", "notes app", "denote: x"] {
            assert_eq!(classify(not), None, "{not}");
        }
    }

    #[test]
    fn leave_off_phrases_and_good_morning() {
        assert_eq!(classify("Where did I leave off?"), Some(Route::Recap));
        assert_eq!(classify("hey byte, where was I"), Some(Route::Recap));
        assert_eq!(classify("what was I working on yesterday"), Some(Route::Recap));
        assert_eq!(classify("good morning"), Some(Route::Morning));
        assert_eq!(classify("Good morning, BYTE!"), Some(Route::Morning));
        assert_eq!(classify("good morning sunshine, write me a poem about spring"), None);
        assert_eq!(classify("where was the first olympics held"), None);
    }

    #[test]
    fn ordinary_questions_are_not_recaps_or_notes() {
        for not in [
            "where was iPhone first sold",
            "where was Italy unified",
            "where was I born",
            "what did i do last summer",
            "what was i thinking about the war",
            "Task: summarize this",
            "task: write an email",
            "make a note of that",
            "note that the sky is blue",
            "todo list apps",
            "catch me up on today",
        ] {
            assert_eq!(classify(not), None, "{not}");
        }
        assert_eq!(classify("what's 40 euros in dollars"), None, "money goes to the normal answer");
        assert_eq!(classify("Catch me up"), Some(Route::Recap));
        assert_eq!(classify("what did I do last time?"), Some(Route::Recap));
        assert_eq!(classify("make a note to call mom"), Some(Route::Note("call mom".into())));
    }

    #[test]
    fn ago_is_in_plain_words() {
        let now = 10_000_000_000;
        assert_eq!(ago(now, now - 20_000), "just now");
        assert_eq!(ago(now, now - 5 * 60_000), "5 minutes ago");
        assert_eq!(ago(now, now - 90 * 60_000), "an hour ago");
        assert_eq!(ago(now, now - 5 * 3_600_000), "5 hours ago");
        assert_eq!(ago(now, now - 30 * 3_600_000), "yesterday");
        assert_eq!(ago(now, now - 3 * 86_400_000), "3 days ago");
        assert_eq!(ago(now, now + 5), "just now");
    }

    #[test]
    fn the_recap_only_says_what_was_given() {
        let now = 100_000_000;
        assert!(compose_recap(now, &[], &[], &[]).contains("nothing to catch you up on"));
        let r = compose_recap(
            now,
            &[RecapChat { title: "Trip to Denver".into(), updated: now - 3_600_000 * 3, summary: Some("Planned flights.".into()) }, RecapChat { title: "Resume".into(), updated: now - 86_400_000 * 2, summary: None }],
            &[RecapNote { title: "Call the dentist".into(), updated: now - 60_000 * 10 }],
            &[RecapTask { title: "Renew passport".into(), overdue: true }],
        );
        assert!(r.contains("\"Trip to Denver\", 3 hours ago. Planned flights."));
        assert!(r.contains("- Resume (2 days ago)"));
        assert!(r.contains("- Call the dentist (10 minutes ago)"));
        assert!(r.contains("- Renew passport (overdue)"));
        assert!(!r.contains("**Latest notes**\n\n"));
        let only_chat = compose_recap(now, &[RecapChat { title: "A".into(), updated: now, summary: None }], &[], &[]);
        assert!(!only_chat.contains("Latest notes") && !only_chat.contains("to-do"));
    }

    #[test]
    fn note_titles_are_the_first_words() {
        assert_eq!(note_title("call the dentist"), "Call the dentist");
        assert_eq!(note_title("one two three four five six seven eight nine"), "One two three four five six seven eight…");
    }
}
