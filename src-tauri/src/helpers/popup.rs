//! The small window the helpers show: a title, some text, a few buttons and, for a question, a text box. It is a plain page
//! (`public/helper-popup.html`) fed by `setHelper(payload)`.
//!
//! Two windows are used so that "Explain this" never steals the keyboard focus from the app the person is reading (it cannot be
//! activated), while the region popup, which has a box to type a question in, can be.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[cfg(desktop)]
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

/// Which helper owns the popup now (so a click goes to the right one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Explain,
    Region,
    Dialog,
    Paste,
    Page,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Explain | Kind::Dialog | Kind::Paste | Kind::Page => "helperpop-x",
            Kind::Region => "helperpop-r",
        }
    }
    /// Only the popups with a box to type in may take the focus.
    pub fn focusable(self) -> bool {
        matches!(self, Kind::Region)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    pub id: String,
    pub label: String,
}

pub fn button(id: &str, label: &str) -> Button {
    Button { id: id.into(), label: label.into() }
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub title: String,
    pub body: String,
    pub buttons: Vec<Button>,
    /// Shows a text box with this hint (the question for "ask a question").
    pub input: Option<String>,
    pub busy: bool,
    /// Milliseconds after which it hides itself (0 = stays until closed).
    pub auto_hide: u32,
}

static CURRENT: Mutex<Option<(Kind, Payload)>> = Mutex::new(None);

pub fn current_kind() -> Option<Kind> {
    CURRENT.lock().ok()?.as_ref().map(|(k, _)| *k)
}

/// Where a popup of `size` goes so that it stays on the screen: below and to the right of `at`, flipped when it would not fit.
pub fn place(at: (i32, i32), size: (i32, i32), screen: (i32, i32, i32, i32)) -> (i32, i32) {
    let (l, t, r, b) = screen;
    let gap = 16;
    let mut x = at.0 + gap;
    let mut y = at.1 + gap;
    if x + size.0 > r {
        x = at.0 - gap - size.0;
    }
    if y + size.1 > b {
        y = at.1 - gap - size.1;
    }
    (x.clamp(l, (r - size.0).max(l)), y.clamp(t, (b - size.1).max(t)))
}

#[cfg(desktop)]
fn window(app: &AppHandle, kind: Kind) -> Option<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window(kind.label()) {
        return Some(w);
    }
    WebviewWindowBuilder::new(app, kind.label(), WebviewUrl::App("helper-popup.html".into()))
        .title("BYTE")
        .inner_size(360.0, 150.0)
        .decorations(false)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(kind.focusable())
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| log::warn!("helper popup: {e}"))
        .ok()
}

/// Shows (or updates) the popup next to `at` (physical screen pixels; the pointer when None).
#[cfg(desktop)]
pub fn show(app: &AppHandle, kind: Kind, payload: Payload, at: Option<(i32, i32)>) {
    // Only one at a time.
    for other in [Kind::Explain, Kind::Region] {
        if other.label() != kind.label() {
            if let Some(w) = app.get_webview_window(other.label()) {
                let _ = w.hide();
            }
        }
    }
    if let Ok(mut c) = CURRENT.lock() {
        *c = Some((kind, payload.clone()));
    }
    let Some(w) = window(app, kind) else { return };
    let visible = w.is_visible().unwrap_or(false);
    if !visible {
        let pointer = at.or_else(|| app.cursor_position().ok().map(|p| (p.x as i32, p.y as i32))).unwrap_or((100, 100));
        let monitor = app.monitor_from_point(pointer.0 as f64, pointer.1 as f64).ok().flatten().or_else(|| app.primary_monitor().ok().flatten());
        let screen = monitor.map(|m| (m.position().x, m.position().y, m.position().x + m.size().width as i32, m.position().y + m.size().height as i32)).unwrap_or((0, 0, 1920, 1080));
        let scale = w.scale_factor().unwrap_or(1.0);
        let size = ((360.0 * scale) as i32, (170.0 * scale) as i32);
        let (x, y) = place(pointer, size, screen);
        let _ = w.set_size(PhysicalSize::new(size.0 as u32, size.1 as u32));
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.show();
    // A window that is not active is not given the focus (the app being read keeps it).
    if kind.focusable() {
        let _ = w.set_focus();
    }
    push(&w, &payload);
}

#[cfg(desktop)]
fn push(w: &tauri::WebviewWindow, payload: &Payload) {
    if let Ok(json) = serde_json::to_string(payload) {
        let _ = w.eval(format!("window.setHelper && window.setHelper({json})"));
    }
}

/// Changes what the open popup says.
#[cfg(desktop)]
pub fn update(app: &AppHandle, kind: Kind, payload: Payload) {
    if let Ok(mut c) = CURRENT.lock() {
        *c = Some((kind, payload.clone()));
    }
    if let Some(w) = app.get_webview_window(kind.label()) {
        push(&w, &payload);
    }
}

#[cfg(desktop)]
pub fn hide(app: &AppHandle) {
    for kind in [Kind::Explain, Kind::Region] {
        if let Some(w) = app.get_webview_window(kind.label()) {
            let _ = w.hide();
        }
    }
    if let Ok(mut c) = CURRENT.lock() {
        *c = None;
    }
}

/// The page asks for what to show when it has just loaded (the first push can arrive before it is ready).
#[tauri::command]
pub fn helper_popup_state() -> Option<Payload> {
    CURRENT.lock().ok()?.as_ref().map(|(_, p)| p.clone())
}

/// The page grows or shrinks to fit its text (CSS pixels).
#[tauri::command]
pub fn helper_popup_resize(app: tauri::AppHandle, height: f64) {
    #[cfg(desktop)]
    if let Some(kind) = current_kind() {
        if let Some(w) = app.get_webview_window(kind.label()) {
            let scale = w.scale_factor().unwrap_or(1.0);
            let h = (height.clamp(80.0, 520.0) * scale) as u32;
            let width = w.outer_size().map(|s| s.width).unwrap_or((360.0 * scale) as u32);
            let _ = w.set_size(PhysicalSize::new(width, h));
        }
    }
    #[cfg(not(desktop))]
    let _ = (app, height);
}

#[tauri::command]
pub fn helper_popup_close(app: tauri::AppHandle) {
    #[cfg(desktop)]
    hide(&app);
    #[cfg(not(desktop))]
    let _ = app;
}

/// A button was pressed (or Enter in the question box, id "send"); `text` is what is in the box.
#[tauri::command]
pub async fn helper_popup_click(app: tauri::AppHandle, id: String, text: Option<String>) -> Result<(), String> {
    let Some(kind) = current_kind() else { return Ok(()) };
    if id == "close" {
        helper_popup_close(app);
        return Ok(());
    }
    match kind {
        Kind::Region => super::region::clicked(&app, &id, text.unwrap_or_default()).await,
        Kind::Explain => super::explain::clicked(&app, &id).await,
        Kind::Dialog => super::dialogs::clicked(&app, &id).await,
        Kind::Paste => super::smartpaste::clicked(&app, &id).await,
        Kind::Page => super::page::clicked(&app, &id).await,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (i32, i32, i32, i32) = (0, 0, 1920, 1080);

    #[test]
    fn the_popup_sits_below_and_right_of_the_pointer() {
        assert_eq!(place((100, 100), (360, 170), SCREEN), (116, 116));
    }

    #[test]
    fn it_flips_to_the_other_side_at_the_screen_edge() {
        assert_eq!(place((1900, 100), (360, 170), SCREEN), (1900 - 16 - 360, 116));
        assert_eq!(place((100, 1070), (360, 170), SCREEN), (116, 1070 - 16 - 170));
    }

    #[test]
    fn it_never_leaves_the_screen_even_when_both_sides_are_tight() {
        let (x, y) = place((50, 50), (360, 170), (0, 0, 300, 100));
        assert!(x >= 0 && y >= 0);
        // A second monitor to the left keeps its own origin.
        let (x, _) = place((-1900, 100), (360, 170), (-1920, 0, 0, 1080));
        assert!((-1920..=-360).contains(&x));
    }

    #[test]
    fn only_the_popup_with_a_question_box_may_take_the_focus() {
        assert!(!Kind::Explain.focusable());
        assert!(!Kind::Dialog.focusable() && !Kind::Paste.focusable() && !Kind::Page.focusable());
        assert!(Kind::Region.focusable());
        assert_eq!(Kind::Explain.label(), Kind::Dialog.label());
        assert_ne!(Kind::Explain.label(), Kind::Region.label());
    }
}
