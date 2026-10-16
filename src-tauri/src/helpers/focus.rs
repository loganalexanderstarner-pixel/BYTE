//! Focus mode: a timed work session. BYTE turns pop-up notifications off (Windows' own switch, the one the routines use, turned
//! back on when the session ends, when BYTE next starts, or after 12 hours), says your plan out loud, times the session, nudges
//! you to take a break every so often and tells you when the time is up.
//!
//! Counts have an "unlimited" choice: a session length of 0 has no end, and a break gap of 0 means no nudges.

use std::sync::Mutex;

pub const DEFAULT_MINUTES: u32 = 25;
pub const DEFAULT_BREAK: u32 = 50;

pub fn default_minutes() -> u32 {
    DEFAULT_MINUTES
}
pub fn default_break() -> u32 {
    DEFAULT_BREAK
}

/// What happens next in a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Nothing,
    /// Time for a break (the gap since the last one has passed).
    Break,
    /// The session length is up.
    End,
}

/// `elapsed`: seconds since the start; `last_break`: seconds at the last nudge (0 at the start). Lengths in minutes, 0 = none/unlimited.
pub fn next_event(elapsed: u64, session_minutes: u32, break_minutes: u32, last_break: u64) -> Event {
    if session_minutes != 0 && elapsed >= u64::from(session_minutes) * 60 {
        return Event::End;
    }
    if break_minutes != 0 && elapsed.saturating_sub(last_break) >= u64::from(break_minutes) * 60 {
        return Event::Break;
    }
    Event::Nothing
}

/// How long, in words.
pub fn minutes_say(m: u64) -> String {
    match m {
        0 => "less than a minute".into(),
        1 => "1 minute".into(),
        m if m < 60 => format!("{m} minutes"),
        m => {
            let (h, r) = (m / 60, m % 60);
            let hours = if h == 1 { "1 hour".to_string() } else { format!("{h} hours") };
            if r == 0 { hours } else { format!("{hours} {r} minutes") }
        }
    }
}

/// What BYTE says when the session starts.
pub fn spoken_plan(plan: &str, session_minutes: u32) -> String {
    let p = plan.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = if session_minutes == 0 { "with no time limit".to_string() } else { format!("for {}", minutes_say(u64::from(session_minutes))) };
    if p.is_empty() { format!("Focus session started {length}. Notifications are off.") } else { format!("Focus session started {length}. Your plan: {p}. Notifications are off.") }
}

struct Session {
    started: std::time::Instant,
    plan: String,
    minutes: u32,
    break_every: u32,
    notifications_off: bool,
}

static RUN: Mutex<Option<Session>> = Mutex::new(None);

pub fn running() -> bool {
    RUN.lock().map(|r| r.is_some()).unwrap_or(false)
}

pub fn supported() -> bool {
    cfg!(windows)
}

#[cfg(desktop)]
mod flow {
    use super::*;
    use crate::helpers::popup::{self, Kind, Payload};
    use tauri::{AppHandle, Manager};

    fn say(app: &AppHandle, title: &str, body: String, hide: u32) {
        popup::show(app, Kind::Focus, Payload { title: title.into(), body, buttons: vec![popup::button("close", "Close")], auto_hide: hide, ..Default::default() }, None);
    }

    /// The tray entry: ask what the plan is.
    pub async fn ask_plan(app: &AppHandle) {
        let state = app.state::<crate::state::AppState>();
        let (on, minutes) = {
            let s = state.settings.lock().await;
            (s.helper_focus, s.helper_focus_minutes)
        };
        if !on || running() {
            return;
        }
        if let Some(why) = crate::helpers::blocked_now() {
            return say(app, "Focus mode", why.into(), 6_000);
        }
        let length = if minutes == 0 { "with no time limit".to_string() } else { format!("for {}", minutes_say(u64::from(minutes))) };
        popup::show(
            app,
            Kind::Focus,
            Payload { title: "Start a focus session".into(), body: format!("Notifications go off {length}. What will you work on? (You can leave this empty.)"), input: Some("Your plan".into()), buttons: vec![popup::button("send", "Start"), popup::button("close", "Cancel")], ..Default::default() },
            None,
        );
    }

    pub async fn begin(app: &AppHandle, plan: String) {
        let state = app.state::<crate::state::AppState>();
        let (minutes, break_every, speak) = {
            let s = state.settings.lock().await;
            (s.helper_focus_minutes, s.helper_focus_break, s.helper_focus_speak)
        };
        let marker = crate::control::routines::marker_file(app);
        let off = crate::control::routines::notifications(false, Some(&marker)).is_ok();
        if let Ok(mut r) = RUN.lock() {
            *r = Some(Session { started: std::time::Instant::now(), plan: plan.clone(), minutes, break_every, notifications_off: off });
        }
        crate::helpers::set_focus_menu(app, true);
        let note = if off { "Notifications are off." } else { "BYTE couldn't turn notifications off on this system, but the timer is running." };
        say(app, "Focus session started", format!("{}\n{note}", if plan.trim().is_empty() { "Good luck.".to_string() } else { plan.clone() }), 6_000);
        if speak {
            crate::helpers::speak(app, &spoken_plan(&plan, minutes)).await;
        }
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_break = 0u64;
            while running() {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                let (elapsed, m, b) = match RUN.lock().ok().and_then(|r| r.as_ref().map(|s| (s.started.elapsed().as_secs(), s.minutes, s.break_every))) {
                    Some(x) => x,
                    None => break,
                };
                match next_event(elapsed, m, b, last_break) {
                    Event::End => {
                        finish(&app2, true).await;
                        break;
                    }
                    Event::Break => {
                        last_break = elapsed;
                        say(&app2, "Time for a break", format!("You've been focused for {}. Stand up, stretch, drink some water. The session keeps running.", minutes_say(elapsed / 60)), 20_000);
                        crate::helpers::speak(&app2, "Time for a short break.").await;
                    }
                    Event::Nothing => {}
                }
            }
        });
    }

    pub async fn finish(app: &AppHandle, done: bool) {
        let Some(s) = RUN.lock().ok().and_then(|mut r| r.take()) else { return };
        if s.notifications_off {
            let marker = crate::control::routines::marker_file(app);
            let _ = crate::control::routines::notifications(true, Some(&marker));
        }
        crate::helpers::set_focus_menu(app, false);
        let spent = minutes_say(s.started.elapsed().as_secs() / 60);
        let what = if s.plan.trim().is_empty() { String::new() } else { format!(" on: {}", s.plan.trim()) };
        say(app, if done { "Focus session finished" } else { "Focus session ended" }, format!("{spent} focused{what}. Notifications are back on."), 15_000);
        if done {
            crate::helpers::speak(app, &format!("Focus session finished. {spent}. Notifications are back on.")).await;
        }
    }

    pub async fn clicked(app: &AppHandle, id: &str, text: String) {
        match id {
            "send" if !running() => begin(app, text).await,
            "close" | "stop" => {
                if running() && id == "stop" {
                    finish(app, false).await;
                } else {
                    popup::hide(app);
                }
            }
            _ => {}
        }
    }
}

#[cfg(desktop)]
pub fn ask_plan(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { flow::ask_plan(&app).await });
}
#[cfg(desktop)]
pub fn end(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { flow::finish(&app, false).await });
}
#[cfg(desktop)]
pub async fn clicked(app: &tauri::AppHandle, id: &str, text: String) {
    flow::clicked(app, id, text).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_ends_at_its_length_and_nudges_a_break_in_between() {
        assert_eq!(next_event(10, 25, 50, 0), Event::Nothing);
        assert_eq!(next_event(25 * 60, 25, 50, 0), Event::End);
        assert_eq!(next_event(50 * 60, 120, 50, 0), Event::Break);
        assert_eq!(next_event(60 * 60, 120, 50, 50 * 60), Event::Nothing);
        assert_eq!(next_event(100 * 60, 120, 50, 50 * 60), Event::Break);
        // The end wins when both are due.
        assert_eq!(next_event(50 * 60, 50, 50, 0), Event::End);
    }

    #[test]
    fn zero_means_unlimited_and_no_nudges() {
        assert_eq!(next_event(10 * 3600, 0, 0, 0), Event::Nothing);
        assert_eq!(next_event(10 * 3600, 0, 50, 9 * 3600 + 3000), Event::Nothing);
        assert_eq!(next_event(10 * 3600, 0, 50, 0), Event::Break);
    }

    #[test]
    fn durations_are_said_in_words() {
        assert_eq!(minutes_say(0), "less than a minute");
        assert_eq!(minutes_say(1), "1 minute");
        assert_eq!(minutes_say(25), "25 minutes");
        assert_eq!(minutes_say(60), "1 hour");
        assert_eq!(minutes_say(135), "2 hours 15 minutes");
    }

    #[test]
    fn the_spoken_plan_names_the_length_and_the_plan() {
        assert_eq!(spoken_plan("  write the\nreport ", 25), "Focus session started for 25 minutes. Your plan: write the report. Notifications are off.");
        assert!(spoken_plan("", 0).contains("with no time limit") && !spoken_plan("", 0).contains("Your plan"));
    }

    #[test]
    fn defaults() {
        assert_eq!((default_minutes(), default_break()), (25, 50));
    }
}
