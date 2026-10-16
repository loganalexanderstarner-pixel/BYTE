//! Research that leaves its pages behind (all off by default, Settings → Control and screen → Browser):
//! - **Keep the pages BYTE opened for me:** the visible BYTE browsing window stays open after the task, and the list of pages is added
//!   to the chat as links. A later "summarise what you read" opens those links again and answers from them.
//! - **Keep researching until I'm back:** after the first round BYTE goes on with more searches and pages until the time limit ("Unlimited"
//!   is a choice), until the person is back at the computer, or until Stop.
//! - **Use my own Edge profile:** an opt-in with a strong warning (see `web_agent/cdp_browser.rs`).
//!
//! Everything that decides something is a small pure function with a test; the browser and the idle clock are behind traits or are passed in.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::tools::Source;

/// The marker line above the list of kept pages in a chat answer.
pub const FOOTER_MARK: &str = "**Pages I opened**";

// ------------------------------------------------------------------ the time limit

/// How long "keep researching until I'm back" may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Off,
    For(Duration),
    Unlimited,
}

impl Limit {
    /// `"off"`, `"15m"`, `"1h"`, `"3h"`, `"8h"` or `"unlimited"`; anything else is off.
    pub fn parse(s: &str) -> Limit {
        match s.trim().to_ascii_lowercase().as_str() {
            "unlimited" => Limit::Unlimited,
            "15m" => Limit::For(Duration::from_secs(15 * 60)),
            "1h" => Limit::For(Duration::from_secs(3600)),
            "3h" => Limit::For(Duration::from_secs(3 * 3600)),
            "8h" => Limit::For(Duration::from_secs(8 * 3600)),
            _ => Limit::Off,
        }
    }

    pub fn expired(self, elapsed: Duration) -> bool {
        match self {
            Limit::Off => true,
            Limit::For(d) => elapsed >= d,
            Limit::Unlimited => false,
        }
    }
}

/// How long after the start the person's own typing (they just asked the question) is ignored.
pub const GRACE: Duration = Duration::from_secs(60);
/// Input this recently, after the grace time, means the person is back.
pub const BACK_IDLE: Duration = Duration::from_secs(5);

/// Is the person back? `idle` is how long since their last key or mouse movement.
pub fn person_back(elapsed: Duration, idle: Duration) -> bool {
    elapsed >= GRACE && idle < BACK_IDLE
}

/// Why the loop stops, if it does.
pub fn stop_reason(limit: Limit, elapsed: Duration, idle: Duration, rounds: u32, max_rounds: u32) -> Option<&'static str> {
    if limit.expired(elapsed) {
        Some("the time limit you chose was reached")
    } else if person_back(elapsed, idle) {
        Some("you were back")
    } else if rounds >= max_rounds {
        Some("it ran out of new angles")
    } else {
        None
    }
}

/// Seconds since the last key press or mouse movement.
pub trait IdleSource: Send + Sync {
    fn idle(&self) -> Duration;
}

pub struct SystemIdle;

impl IdleSource for SystemIdle {
    fn idle(&self) -> Duration {
        #[cfg(windows)]
        {
            use windows::Win32::System::SystemInformation::GetTickCount;
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
            let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            // SAFETY: `info` is plain data with its size set, and is only written to.
            if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
                let now = unsafe { GetTickCount() };
                return Duration::from_millis(now.wrapping_sub(info.dwTime) as u64);
            }
        }
        // Unknown: act as if the person were away for a short while only, never forever.
        Duration::from_secs(30)
    }
}

// ------------------------------------------------------------------ more searches

const ANGLES: &[&str] = &[
    "latest news", "reviews and opinions", "problems and criticism", "comparison alternatives", "how it works explained", "beginner guide", "expert analysis",
    "statistics and data", "history and background", "frequently asked questions", "pros and cons", "recent changes", "common mistakes", "best practices",
    "case studies", "future outlook", "cost and pricing", "safety and risks", "official documentation", "community discussion", "myths and facts",
    "step by step", "tips from experts", "recent research",
];

/// The extra search for round `round` (0 is the first extra one). `None` when the angles are used up.
pub fn angle_query(topic: &str, round: usize) -> Option<String> {
    ANGLES.get(round).map(|a| format!("{topic} {a}"))
}

/// Rounds at most (one per angle).
pub fn max_rounds() -> u32 {
    ANGLES.len() as u32
}

// ------------------------------------------------------------------ the list in the chat

fn clean_title(t: &str, url: &str) -> String {
    let t: String = t.chars().filter(|c| !matches!(c, '[' | ']' | '\n' | '\r')).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = if t.is_empty() { url.to_string() } else { t };
    t.chars().take(90).collect()
}

/// The list added to the end of an answer: every page BYTE opened, as links. Empty when there were none.
pub fn pages_footer(sources: &[Source]) -> String {
    let mut seen: Vec<&str> = vec![];
    let mut lines = vec![];
    for s in sources.iter().filter(|s| s.url.starts_with("http")) {
        if seen.contains(&s.url.as_str()) || s.url.contains([')', ' ']) {
            continue;
        }
        seen.push(&s.url);
        lines.push(format!("- [{}]({})", clean_title(&s.title, &s.url), s.url));
        if lines.len() >= 12 {
            break;
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("\n\n{FOOTER_MARK} (kept in this chat):\n{}\n", lines.join("\n"))
}

/// "Summarise what you read", "sum up those pages", "what did you read?".
pub fn summarise_request(q: &str) -> bool {
    let q = q.to_lowercase();
    let verb = ["summari", "sum up", "recap", "what did you read", "what did you find", "go over"].iter().any(|v| q.contains(v));
    let object = ["what you read", "the pages", "those pages", "these pages", "the tabs", "those tabs", "what you found", "what you opened", "the sources", "you read", "you opened"].iter().any(|o| q.contains(o));
    (verb && object) || q.contains("what did you read")
}

/// The pages from the newest answer that has a kept list, from `(role, text)` pairs of the chat, oldest first.
pub fn kept_pages(history: &[(String, String)]) -> Vec<(String, String)> {
    let Some((_, text)) = history.iter().rev().find(|(r, t)| r == "assistant" && t.contains(FOOTER_MARK)) else { return vec![] };
    let after = text.split(FOOTER_MARK).last().unwrap_or("");
    let mut out = vec![];
    for line in after.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("- [") else { continue };
        let Some((title, tail)) = rest.split_once("](") else { continue };
        let Some(url) = tail.strip_suffix(')') else { continue };
        if url.starts_with("http") {
            out.push((title.to_string(), url.to_string()));
        }
    }
    out
}

// ------------------------------------------------------------------ the settings and the loop

/// What the three settings say for this answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opts {
    pub keep_pages: bool,
    pub until_back: Limit,
    /// When the job ends, close the research tabs instead of leaving them for the person.
    pub close_tabs: bool,
}

impl Opts {
    pub fn from_settings(s: &crate::settings::Settings, private: bool) -> Opts {
        // A private chat leaves nothing behind.
        Opts { keep_pages: s.browser_keep_pages && !private, until_back: if private { Limit::Off } else { Limit::parse(&s.browser_research_until_back) }, close_tabs: s.browser_close_tabs_when_back }
    }
}

pub async fn opts(app: Option<&tauri::AppHandle>) -> Opts {
    use tauri::Manager;
    let private = crate::browserroutes::PRIVATE.try_with(|p| *p).unwrap_or(false);
    match app {
        Some(app) => Opts::from_settings(&*app.state::<crate::state::AppState>().settings.lock().await, private),
        None => Opts { keep_pages: false, until_back: Limit::Off, close_tabs: false },
    }
}

/// The opt-in "use my own Edge profile" (never in a private chat).
pub async fn use_own_edge(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    let private = crate::browserroutes::PRIVATE.try_with(|p| *p).unwrap_or(false);
    !private && app.state::<crate::state::AppState>().settings.lock().await.browser_own_edge
}

/// Most pages from the extra rounds that go into the notes for the model (the rest are still listed as links).
const MAX_EXTRA_NOTES: usize = 8;

// ------------------------------------------------------------------ the running job

/// What the job shares with the "I'm back" button, the typed words and the voice.
#[derive(Default)]
pub struct JobState {
    back: AtomicBool,
}

static JOB: Mutex<Option<Arc<JobState>>> = Mutex::new(None);
pub const LIVE_EVENT: &str = "research://live";

fn begin_job() -> Arc<JobState> {
    let j = Arc::new(JobState::default());
    *JOB.lock().unwrap_or_else(|e| e.into_inner()) = Some(j.clone());
    j
}

fn end_job() {
    *JOB.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The person says they are back. False when no research job is running.
pub fn request_back() -> bool {
    match JOB.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(j) => {
            j.back.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// The "I'm back" button, and the typed or spoken words (the chat box sends them here while a job runs).
#[tauri::command]
pub fn research_back() -> bool {
    request_back()
}

/// The live card's state.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    pub active: bool,
    pub pages: usize,
    pub elapsed_secs: u64,
    /// None: no time limit.
    pub limit_secs: Option<u64>,
    /// Every angle is done and BYTE is waiting for the person (or the limit).
    pub standby: bool,
    /// The pages are open in real tabs in a visible browser.
    pub tabs: bool,
}

fn emit_live(app: Option<&tauri::AppHandle>, l: &Live) {
    use tauri::Emitter;
    if let Some(app) = app {
        let _ = app.emit(LIVE_EVENT, l);
    }
}

/// Why the job stops, with the person's own word ("I'm back") ahead of the time limit.
pub fn job_stop(limit: Limit, elapsed: Duration, back: bool) -> Option<&'static str> {
    if back {
        Some("you said you were back")
    } else if limit.expired(elapsed) {
        Some("the time limit you chose was reached")
    } else {
        None
    }
}

fn limit_secs(l: Limit) -> Option<u64> {
    match l {
        Limit::For(d) => Some(d.as_secs()),
        _ => None,
    }
}

/// "Keep researching until I'm back": more searches from new angles, a few pages each, until the person says they are back, the time
/// limit ends (Unlimited never does), or Stop. In real tabs (live) the job then waits, standing by, once the angles are used up. When it
/// ends, the browser is detached and the debugging port closed (`Session::finish_job`), even after Stop or an error.
/// Returns the notes from the extra rounds (numbered after the first ones, in the same source book).
#[allow(clippy::too_many_arguments)]
pub async fn continue_research(
    turn: &crate::agent::Turn<'_>,
    topic: &str,
    opts: Opts,
    idle: &dyn IdleSource,
    s: &mut crate::web_agent::Session,
    book: &mut crate::tools::SourceBook,
    cancel: &tokio_util::sync::CancellationToken,
    send: &(dyn Fn(crate::chat::ChatEvent) -> crate::error::AppResult<()> + Sync),
) -> crate::error::AppResult<String> {
    let job = begin_job();
    let tabs = s.live_tabs();
    let r = job_loop(turn, topic, opts.until_back, tabs, &job, idle, s, book, cancel, send).await;
    end_job();
    emit_live(turn.app, &Live { active: false, pages: book.sources.len(), elapsed_secs: 0, limit_secs: None, standby: false, tabs });
    // Whatever way it ended: detach the controller and close the port; the pages are left for the person unless told otherwise.
    s.finish_job(opts.close_tabs).await;
    r
}

#[allow(clippy::too_many_arguments)]
async fn job_loop(
    turn: &crate::agent::Turn<'_>,
    topic: &str,
    limit: Limit,
    tabs: bool,
    job: &JobState,
    idle: &dyn IdleSource,
    s: &mut crate::web_agent::Session,
    book: &mut crate::tools::SourceBook,
    cancel: &tokio_util::sync::CancellationToken,
    send: &(dyn Fn(crate::chat::ChatEvent) -> crate::error::AppResult<()> + Sync),
) -> crate::error::AppResult<String> {
    use crate::chat::ChatEvent;
    use crate::error::AppError;
    let start = std::time::Instant::now();
    let mut reads: Vec<crate::browserroutes::PageRead> = vec![];
    let mut round = 0usize;
    let live = |standby: bool, book: &crate::tools::SourceBook| Live { active: true, pages: book.sources.len(), elapsed_secs: start.elapsed().as_secs(), limit_secs: limit_secs(limit), standby, tabs };
    send(ChatEvent::Notice {
        text: match (limit, tabs) {
            (Limit::Unlimited, true) => "Researching in Edge tabs you can watch, with no time limit. Say \"I'm back\" or press Stop to end it.".into(),
            (Limit::For(d), true) => format!("Researching in Edge tabs you can watch, for at most {} minutes. Say \"I'm back\" or press Stop to end it.", d.as_secs() / 60),
            (Limit::Unlimited, false) => "Researching further until you are back (no time limit). Press Stop to end it.".into(),
            (Limit::For(d), false) => format!("Researching further until you are back, for at most {} minutes. Press Stop to end it.", d.as_secs() / 60),
            (Limit::Off, _) => String::new(),
        },
    })?;
    emit_live(turn.app, &live(false, book));
    let why = loop {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let back = job.back.load(Ordering::SeqCst);
        if let Some(why) = job_stop(limit, start.elapsed(), back) {
            break why;
        }
        // Without real tabs the person is not watching, so their coming back to the keyboard counts too.
        if !tabs && person_back(start.elapsed(), idle.idle()) {
            break "you were back";
        }
        let Some(query) = angle_query(topic, round) else {
            if !tabs {
                break "it ran out of new angles";
            }
            // Live tabs: everything planned has been read; stay open for the person until they say so or the limit ends.
            emit_live(turn.app, &live(true, book));
            tokio::select! {
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            continue;
        };
        round += 1;
        s.extend_steps(10);
        emit_live(turn.app, &live(false, book));
        let found = tokio::select! {
            r = crate::tools::search::search(turn.net, turn.cloud, &query, 8) => r,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        let Ok(found) = found else { continue };
        let fresh: Vec<_> = found.results.into_iter().filter(|r| !book.sources.iter().any(|b| b.url.trim_end_matches('/') == r.url.trim_end_matches('/'))).collect();
        let picks = crate::browserroutes::pick_sources(&fresh, 3);
        if picks.is_empty() {
            continue;
        }
        reads.extend(crate::browserroutes::read_sources(s, &picks, book, cancel, send, turn.app).await?);
        if s.steps_left() == 0 {
            break "the browser was stopped";
        }
    };
    send(ChatEvent::Notice { text: format!("Stopped researching: {why}. {} more pages were opened.", reads.len()) })?;
    reads.truncate(MAX_EXTRA_NOTES);
    Ok(if reads.is_empty() { String::new() } else { format!("\n\n{}", crate::browserroutes::format_notes(&format!("{topic} (further rounds)"), &reads)) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(n: u32, title: &str, url: &str) -> Source {
        Source { n, title: title.into(), url: url.into(), snippet: String::new(), read: true, meta: None }
    }

    #[test]
    fn the_limit_has_unlimited_and_off_is_the_default() {
        assert_eq!(Limit::parse("off"), Limit::Off);
        assert_eq!(Limit::parse(""), Limit::Off);
        assert_eq!(Limit::parse("nonsense"), Limit::Off);
        assert_eq!(Limit::parse("1H"), Limit::For(Duration::from_secs(3600)));
        assert_eq!(Limit::parse("unlimited"), Limit::Unlimited);
        assert!(Limit::Off.expired(Duration::ZERO));
        assert!(!Limit::Unlimited.expired(Duration::from_secs(10_000_000)));
        assert!(Limit::parse("15m").expired(Duration::from_secs(900)));
        assert!(!Limit::parse("15m").expired(Duration::from_secs(899)));
    }

    #[test]
    fn typing_the_request_does_not_count_as_being_back() {
        assert!(!person_back(Duration::from_secs(10), Duration::from_secs(1)), "inside the grace time");
        assert!(person_back(Duration::from_secs(90), Duration::from_secs(1)));
        assert!(!person_back(Duration::from_secs(90), Duration::from_secs(40)), "still away");
    }

    #[test]
    fn the_loop_stops_for_the_limit_for_the_person_or_when_out_of_angles() {
        let l = Limit::For(Duration::from_secs(1000));
        let away = Duration::from_secs(600);
        assert_eq!(stop_reason(l, Duration::from_secs(100), away, 0, 12), None);
        assert_eq!(stop_reason(l, Duration::from_secs(1000), away, 0, 12), Some("the time limit you chose was reached"));
        assert_eq!(stop_reason(l, Duration::from_secs(100), Duration::from_secs(1), 0, 12), Some("you were back"));
        assert_eq!(stop_reason(Limit::Unlimited, Duration::from_secs(99_999), away, 12, 12), Some("it ran out of new angles"));
        assert_eq!(stop_reason(Limit::Unlimited, Duration::from_secs(99_999), away, 3, 12), None, "unlimited never times out");
    }

    #[test]
    fn saying_im_back_beats_the_clock_and_unlimited_never_runs_out() {
        let l = Limit::For(Duration::from_secs(60));
        assert_eq!(job_stop(l, Duration::from_secs(10), true), Some("you said you were back"));
        assert_eq!(job_stop(l, Duration::from_secs(10), false), None);
        assert_eq!(job_stop(l, Duration::from_secs(61), false), Some("the time limit you chose was reached"));
        assert_eq!(job_stop(Limit::Unlimited, Duration::from_secs(10_000_000), false), None);
        assert_eq!(job_stop(Limit::Unlimited, Duration::ZERO, true), Some("you said you were back"));
    }

    #[test]
    fn the_back_request_reaches_only_a_running_job() {
        end_job();
        assert!(!request_back(), "nothing is running");
        let j = begin_job();
        assert!(!j.back.load(Ordering::SeqCst));
        assert!(research_back());
        assert!(j.back.load(Ordering::SeqCst));
        end_job();
        assert!(!request_back());
    }

    #[test]
    fn extra_searches_use_each_angle_once() {
        assert_eq!(angle_query("e-bikes", 0).unwrap(), "e-bikes latest news");
        assert!(angle_query("e-bikes", ANGLES.len()).is_none());
        assert_eq!(max_rounds() as usize, ANGLES.len());
    }

    #[test]
    fn the_footer_lists_each_page_once_as_a_link() {
        let f = pages_footer(&[src(1, "A [odd] title", "https://a.example/x"), src(2, "", "https://b.example"), src(3, "Dup", "https://a.example/x"), src(4, "No", "file:///c"), src(5, "Bad", "https://c.example/a b")]);
        assert!(f.starts_with("\n\n**Pages I opened**"));
        assert_eq!(f.matches("\n- [").count(), 2);
        assert!(f.contains("- [A odd title](https://a.example/x)") && f.contains("- [https://b.example](https://b.example)"));
        assert!(pages_footer(&[]).is_empty());
    }

    #[test]
    fn a_later_summary_request_finds_the_kept_pages_in_the_newest_answer() {
        let answer = format!("Here is what I found.{}", pages_footer(&[src(1, "A", "https://a.example/x"), src(2, "B", "https://b.example/y")]));
        let h = vec![("user".to_string(), "research e-bikes".to_string()), ("assistant".to_string(), answer), ("user".to_string(), "thanks".to_string()), ("assistant".to_string(), "welcome".to_string())];
        assert_eq!(kept_pages(&h), vec![("A".to_string(), "https://a.example/x".to_string()), ("B".to_string(), "https://b.example/y".to_string())]);
        assert!(kept_pages(&[("assistant".to_string(), "no list".to_string())]).is_empty());
    }

    #[test]
    fn it_notices_a_request_to_summarise_the_pages() {
        for q in ["Summarise what you read", "can you summarize those pages", "sum up the tabs you opened", "what did you read?", "recap what you found"] {
            assert!(summarise_request(q), "{q}");
        }
        for q in ["summarise this article", "what is the weather", "open the pages menu"] {
            assert!(!summarise_request(q), "{q}");
        }
    }
}
