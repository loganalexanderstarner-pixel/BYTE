//! The approval card as an on-screen popup. When PC control, the browser or any other action needs a yes and BYTE's window is not in
//! front, a small always-on-top window that does not take the keyboard focus shows what BYTE wants to do, with "Do it" and "Don't".
//! Ctrl+Alt+Y and Ctrl+Alt+N answer from anywhere while it is up (plain Y and N would steal keys from whatever the person is typing).
//! It times out to "no" with the card's own timer, and nothing is ever approved by itself.
//!
//! With "Speak and listen for approvals" on (and Hey BYTE on), the question is also spoken and a spoken "yes" or "no" answers it: the
//! page does that through the wake listener's utterance flow (`src/lib/approvalVoice.ts`), which is the same flow, with the same voice
//! checks, as "Hey BYTE".
//!
//! The popup is the window `approvalpop` (public/approval.html). A third button ("Approve and don't ask again") is added by
//! another part later: the page has an empty `#extra` slot, and [`PopState`] has a place for the extra button's label.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::web_agent::ApprovalAsk;

pub const LABEL: &str = "approvalpop";
/// Window size in logical pixels.
pub const WIDTH: f64 = 380.0;
pub const HEIGHT: f64 = 170.0;
pub const MARGIN: f64 = 14.0;
pub const VOICE_EVENT: &str = "found://approval-voice";

static HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn start(app: &AppHandle) {
    let _ = HANDLE.set(app.clone());
}

/// What the popup shows.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PopState {
    pub id: String,
    pub title: String,
    pub lines: Vec<String>,
    /// Seconds left before it counts as "no".
    pub seconds_left: u64,
    /// More approvals are waiting behind this one.
    pub more: usize,
    /// Reserved for a third button; empty until another part sets it.
    pub extra_label: String,
}

struct Item {
    id: String,
    title: String,
    lines: Vec<String>,
    deadline: Instant,
}

/// The approvals waiting, the newest shown. Pure bookkeeping, tested below.
#[derive(Default)]
pub struct Queue(Vec<Item>);

impl Queue {
    fn push(&mut self, i: Item) {
        self.0.retain(|x| x.id != i.id);
        self.0.push(i);
    }
    fn remove(&mut self, id: &str) -> bool {
        let n = self.0.len();
        self.0.retain(|x| x.id != id);
        self.0.len() != n
    }
    fn top(&self, now: Instant) -> Option<PopState> {
        let i = self.0.last()?;
        Some(PopState {
            id: i.id.clone(),
            title: i.title.clone(),
            lines: i.lines.clone(),
            seconds_left: i.deadline.saturating_duration_since(now).as_secs(),
            more: self.0.len() - 1,
            extra_label: String::new(),
        })
    }
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue(Vec::new()));

fn clip(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>().trim_end())
    }
}

/// The few lines the popup shows: where, then up to three of what is about to happen, each cut short.
pub fn lines_for(ask: &ApprovalAsk) -> Vec<String> {
    let mut out = vec![];
    if !ask.site.trim().is_empty() {
        out.push(clip(&ask.site, 60));
    }
    for f in ask.fields.iter().filter(|f| !f.label.trim().is_empty() || !f.value.trim().is_empty()).take(3) {
        out.push(if f.label.trim().is_empty() { clip(&f.value, 70) } else { clip(&format!("{}: {}", f.label, f.value), 70) });
    }
    out
}

/// What BYTE says out loud.
pub fn question_for(ask: &ApprovalAsk) -> String {
    let title = clip(&ask.title, 120);
    format!("{} Should I do it? Say yes or no.", if title.ends_with(['.', '!', '?']) { title } else { format!("{title}.") })
}

/// Where the popup goes: the bottom-right corner of a monitor's work area (so it clears the taskbar), in physical pixels. The work
/// area can start at a negative coordinate (a monitor to the left of or above the main one); `scale` is that monitor's.
pub fn placement(work: (i32, i32, u32, u32), scale: f64) -> (i32, i32) {
    let (x, y, w, h) = work;
    let (pw, ph) = ((WIDTH * scale).round() as i32, (HEIGHT * scale).round() as i32);
    let m = (MARGIN * scale).round() as i32;
    ((x + w as i32 - pw - m).max(x), (y + h as i32 - ph - m).max(y))
}

/// Is BYTE's own window in front of the person? Then its card is already in sight and no popup is needed.
fn main_in_front(app: &AppHandle) -> bool {
    ["main", "quick"].iter().any(|l| {
        app.get_webview_window(l).is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) && w.is_focused().unwrap_or(false))
    })
}

fn wants(app: &AppHandle) -> (bool, bool) {
    let Some(state) = app.try_state::<AppState>() else { return (true, false) };
    let Ok(s) = state.settings.try_lock() else { return (true, false) };
    (s.approval_popup && !s.kids_mode, s.approval_voice && s.wake_word && !s.kids_mode)
}

/// Keeps the popup up for one approval; dropping it (answered, timed out or stopped) takes the approval out and closes or updates it.
pub struct Guard(Option<String>);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(id) = self.0.take() {
            if let Ok(mut q) = QUEUE.lock() {
                q.remove(&id);
            }
            if let Some(app) = HANDLE.get() {
                refresh(app);
            }
        }
    }
}

/// Called when an approval is asked. Shows the popup if BYTE is not in front, and asks the page to speak it when voice approval is on.
pub fn open(ask: &ApprovalAsk, wait: Duration) -> Guard {
    let Some(app) = HANDLE.get() else { return Guard(None) };
    let (popup, voice) = wants(app);
    if voice {
        let _ = app.emit(VOICE_EVENT, serde_json::json!({ "id": ask.id, "question": question_for(ask), "waitMs": wait.as_millis().min(120_000) as u64 }));
    }
    if !popup || main_in_front(app) {
        return Guard(None);
    }
    if let Ok(mut q) = QUEUE.lock() {
        q.push(Item { id: ask.id.clone(), title: clip(&ask.title, 140), lines: lines_for(ask), deadline: Instant::now() + wait });
    }
    refresh(app);
    Guard(Some(ask.id.clone()))
}

/// The state the page shows.
#[tauri::command]
pub fn approval_pop_state() -> Option<PopState> {
    QUEUE.lock().ok().and_then(|q| q.top(Instant::now()))
}

/// Brings the window in line with the queue: shown for the newest approval, closed when none is left.
fn refresh(app: &AppHandle) {
    let top = QUEUE.lock().ok().and_then(|q| q.top(Instant::now()));
    #[cfg(windows)]
    {
        match top {
            Some(_) => show(app),
            None => hide(app),
        }
    }
    #[cfg(not(windows))]
    let _ = (app, top);
}

#[cfg(windows)]
fn show(app: &AppHandle) {
    use tauri::{LogicalSize, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.eval("window.refresh && window.refresh()");
        return;
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("approval.html".into()))
        .title("BYTE needs a yes or no")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // It must never take the keyboard from what the person is typing in.
        .focusable(false)
        .focused(false)
        .visible(false)
        .build();
    let w = match built {
        Ok(w) => w,
        Err(e) => return log::warn!("approval popup: {e}"),
    };
    // The monitor with the pointer (where the person is looking), else the main one.
    let monitor = app.cursor_position().ok().and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten()).or_else(|| app.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let a = m.work_area();
        // Move first so the window takes that monitor's scale, then size it in logical pixels and place it.
        let _ = w.set_position(PhysicalPosition::new(a.position.x + 20, a.position.y + 20));
        let _ = w.set_size(LogicalSize::new(WIDTH, HEIGHT));
        let scale = w.scale_factor().unwrap_or_else(|_| m.scale_factor());
        let (x, y) = placement((a.position.x, a.position.y, a.size.width, a.size.height), scale);
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.show();
    register_keys(app);
}

#[cfg(windows)]
fn hide(app: &AppHandle) {
    unregister_keys(app);
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
}

// ------------------------------------------------------------------ the keys

static KEYS: Mutex<Option<(u32, u32)>> = Mutex::new(None);

#[cfg(windows)]
fn register_keys(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
    let mut k = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    if k.is_some() {
        return;
    }
    let m = Some(Modifiers::CONTROL | Modifiers::ALT);
    let (y, n) = (Shortcut::new(m, Code::KeyY), Shortcut::new(m, Code::KeyN));
    let gs = app.global_shortcut();
    if gs.register(y).is_ok() & gs.register(n).is_ok() {
        *k = Some((y.id(), n.id()));
    } else {
        let _ = gs.unregister(y);
        let _ = gs.unregister(n);
        log::info!("approval popup: Ctrl+Alt+Y / Ctrl+Alt+N are taken by another program; the buttons still work");
    }
}

#[cfg(windows)]
fn unregister_keys(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
    let mut k = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    if k.take().is_some() {
        let m = Some(Modifiers::CONTROL | Modifiers::ALT);
        let gs = app.global_shortcut();
        let _ = gs.unregister(Shortcut::new(m, Code::KeyY));
        let _ = gs.unregister(Shortcut::new(m, Code::KeyN));
    }
}

/// A shortcut was pressed: if it is one of the popup's, answer the approval on show. Returns true when it was.
#[cfg(desktop)]
pub fn pressed(_app: &AppHandle, shortcut: &tauri_plugin_global_shortcut::Shortcut) -> bool {
    let Some((y, n)) = *KEYS.lock().unwrap_or_else(|e| e.into_inner()) else { return false };
    let id = shortcut.id();
    if id != y && id != n {
        return false;
    }
    if let Some(top) = approval_pop_state() {
        crate::web_agent::answer_with(&top.id, id == y, Vec::new());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_agent::Field;

    fn ask(title: &str) -> ApprovalAsk {
        ApprovalAsk {
            id: "a1".into(),
            action: "mac".into(),
            title: title.into(),
            site: "Notepad".into(),
            url: String::new(),
            target: String::new(),
            fields: vec![Field { label: "Text".into(), value: "hello   world".into() }, Field { label: String::new(), value: "x".repeat(200) }],
            editable: vec![],
        }
    }

    fn item(id: &str, secs: u64) -> Item {
        Item { id: id.into(), title: id.into(), lines: vec![], deadline: Instant::now() + Duration::from_secs(secs) }
    }

    #[test]
    fn the_popup_shows_where_and_what_cut_short() {
        let l = lines_for(&ask("Type text"));
        assert_eq!(l[0], "Notepad");
        assert_eq!(l[1], "Text: hello world");
        assert!(l[2].chars().count() <= 70 && l[2].ends_with('…'));
        let mut many = ask("x");
        many.fields = (0..9).map(|i| Field { label: format!("f{i}"), value: "v".into() }).collect();
        assert_eq!(lines_for(&many).len(), 4, "the place and three fields");
    }

    #[test]
    fn the_spoken_question_is_one_sentence_and_ends_with_how_to_answer() {
        assert_eq!(question_for(&ask("Open Notepad")), "Open Notepad. Should I do it? Say yes or no.");
        assert_eq!(question_for(&ask("Close it?")), "Close it? Should I do it? Say yes or no.");
    }

    #[test]
    fn the_newest_approval_is_shown_and_the_rest_wait_behind_it() {
        let mut q = Queue::default();
        q.push(item("a", 30));
        q.push(item("b", 30));
        let t = q.top(Instant::now()).unwrap();
        assert_eq!((t.id.as_str(), t.more), ("b", 1));
        assert!(q.remove("b"));
        assert_eq!(q.top(Instant::now()).unwrap().id, "a");
        assert!(!q.remove("zzz"));
        q.remove("a");
        assert!(q.top(Instant::now()).is_none());
        // Asking again with the same id replaces it rather than doubling it.
        q.push(item("a", 30));
        q.push(item("a", 30));
        assert_eq!(q.top(Instant::now()).unwrap().more, 0);
    }

    #[test]
    fn the_countdown_never_goes_below_zero() {
        let mut q = Queue::default();
        q.push(Item { deadline: Instant::now() - Duration::from_secs(5), ..item("a", 0) });
        assert_eq!(q.top(Instant::now()).unwrap().seconds_left, 0);
    }

    #[test]
    fn placement_sits_in_the_bottom_right_of_the_work_area_at_any_scale() {
        // 1080p, taskbar 48px high: the work area ends at 1032.
        assert_eq!(placement((0, 0, 1920, 1032), 1.0), (1920 - 380 - 14, 1032 - 170 - 14));
        // 150% scale on a 4K panel.
        assert_eq!(placement((0, 0, 3840, 2064), 1.5), (3840 - 570 - 21, 2064 - 255 - 21));
        // A monitor to the left of the main one has negative coordinates.
        let (x, y) = placement((-1920, 0, 1920, 1080), 1.0);
        assert_eq!((x, y), (-380 - 14, 1080 - 170 - 14));
        // A work area smaller than the popup keeps it on the monitor.
        assert_eq!(placement((100, 50, 200, 100), 1.0), (100, 50));
    }
}
