//! Quick Ask, the menu-bar icon and BYTE's global shortcuts (Phase 11).
//!
//! Quick Ask is a small window over whatever app is in front (⌥Space by
//! default, or a click on the menu-bar icon): type, get the answer there, and
//! "Open in BYTE" continues the chat in the main window. It's the same web app
//! as the main window (`main.tsx` picks the Quick Ask view by window label), so
//! chats it starts are ordinary saved chats.
//!
//! Global shortcuts: Quick Ask's and the selection hotkey's keys come from
//! settings and can be changed (Settings → Keyboard). One handler sees every
//! press and sends it to whichever shortcut it is.

use std::str::FromStr;
use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut};

use crate::error::{AppError, AppResult};
use crate::settings::Settings;

pub const LABEL: &str = "quick";
const TRAY: &str = "byte";
const WIDTH: f64 = 680.0;
const HEIGHT: f64 = 480.0;

// ------------------------------------------------------------------ shortcuts

/// Shortcut text ("Alt+Space") → a shortcut, or a plain reason it can't be one.
pub fn parse_keys(keys: &str) -> Result<Shortcut, String> {
    let k = keys.trim();
    if k.is_empty() {
        return Err("Press a key combination for the shortcut.".into());
    }
    let sc = Shortcut::from_str(k).map_err(|_| format!("BYTE doesn't understand the shortcut \"{k}\"."))?;
    // ⇧ alone would take a capital letter away from every app.
    if !sc.mods.intersects(Modifiers::ALT | Modifiers::SUPER | Modifiers::CONTROL) {
        return Err("A shortcut needs ⌘, ⌥ or ⌃ with the key, so normal typing still works.".into());
    }
    Ok(sc)
}

/// "Alt+Super+KeyB" → "⌥⌘B" on a Mac, "Alt+Win+B" on Windows (for messages; the UI
/// has its own copy in lib/keys.ts).
pub fn pretty(keys: &str) -> String {
    pretty_for(keys, cfg!(windows))
}

/// As `pretty`, for a chosen platform, so both forms are testable on any machine.
pub fn pretty_for(keys: &str, windows: bool) -> String {
    let (mut ctrl, mut alt, mut shift, mut meta) = (false, false, false, false);
    let mut key = String::new();
    for part in keys.split('+').map(str::trim) {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            // Command on a Mac; on Windows CmdOrCtrl is Ctrl and Super is the Windows key.
            "cmdorctrl" | "commandorcontrol" => {
                if windows {
                    ctrl = true
                } else {
                    meta = true
                }
            }
            "super" | "cmd" | "command" | "meta" => meta = true,
            _ => key = part.trim_start_matches("Key").trim_start_matches("Digit").to_string(),
        }
    }
    if key.eq_ignore_ascii_case("space") {
        key = "Space".into();
    }
    if windows {
        let mut parts: Vec<&str> = Vec::new();
        if ctrl { parts.push("Ctrl") }
        if alt { parts.push("Alt") }
        if shift { parts.push("Shift") }
        if meta { parts.push("Win") }
        parts.push(&key);
        return parts.join("+");
    }
    let mut out = String::new();
    if ctrl { out.push('⌃') }
    if alt { out.push('⌥') }
    if shift { out.push('⇧') }
    if meta { out.push('⌘') }
    out + &key
}

/// The shortcut settings are usable: each parses, and the two differ.
pub fn check(s: &Settings) -> AppResult<()> {
    let quick = if s.quick_ask { Some(parse_keys(&s.quick_ask_keys).map_err(AppError::msg)?) } else { None };
    let sel = if s.selection_hotkey { Some(parse_keys(&s.selection_keys).map_err(AppError::msg)?) } else { None };
    if let (Some(q), Some(x)) = (quick, sel) {
        if q.id() == x.id() {
            return Err(AppError::msg(format!("{} is already the selection shortcut. Pick different keys.", pretty(&s.quick_ask_keys))));
        }
    }
    Ok(())
}

/// What is registered now: (Quick Ask, selection).
static CURRENT: Mutex<(Option<u32>, Option<u32>)> = Mutex::new((None, None));

/// Registers the shortcuts the settings ask for (and removes the rest).
pub fn apply_shortcuts(app: &AppHandle, s: &Settings) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut now = (None, None);
    if s.quick_ask {
        match parse_keys(&s.quick_ask_keys) {
            Ok(sc) => match gs.register(sc) {
                Ok(()) => now.0 = Some(sc.id()),
                Err(e) => log::warn!("couldn't register {}: {e}", s.quick_ask_keys),
            },
            Err(e) => log::warn!("{e}"),
        }
    }
    // The selection hotkey copies with System Events on a Mac and sends Ctrl+C on Windows.
    if s.selection_hotkey && s.mac_control && cfg!(any(target_os = "macos", windows)) {
        match parse_keys(&s.selection_keys) {
            Ok(sc) if Some(sc.id()) != now.0 => match gs.register(sc) {
                Ok(()) => now.1 = Some(sc.id()),
                Err(e) => log::warn!("couldn't register {}: {e}", s.selection_keys),
            },
            Ok(_) => {}
            Err(e) => log::warn!("{e}"),
        }
    }
    if let Ok(mut c) = CURRENT.lock() {
        *c = now;
    }
}

/// A global shortcut was pressed.
pub fn pressed(app: &AppHandle, shortcut: &Shortcut) {
    let (quick, sel) = CURRENT.lock().map(|c| *c).unwrap_or((None, None));
    let id = shortcut.id();
    if Some(id) == quick {
        toggle(app, None);
    } else if Some(id) == sel {
        crate::selection::pressed(app);
    }
}

// ------------------------------------------------------------------ the window

fn window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Ok(w);
    }
    let b = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Quick Ask")
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(420.0, 150.0)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .resizable(true)
        .visible(false);
    // A Mac window keeps its rounded corners and shadow with the title bar hidden.
    #[cfg(target_os = "macos")]
    let b = b.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let b = b.decorations(false);
    b.build()
}

/// Where the window goes: under the menu-bar icon, or high on the screen with the pointer.
fn place(app: &AppHandle, w: &WebviewWindow, tray: Option<Rect>) {
    let size = w.outer_size().unwrap_or(tauri::PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    if let Some(rect) = tray {
        let scale = w.scale_factor().unwrap_or(1.0);
        let pos = rect.position.to_physical::<f64>(scale);
        let sz = rect.size.to_physical::<f64>(scale);
        let x = pos.x + sz.width / 2.0 - size.width as f64 / 2.0;
        let _ = w.set_position(PhysicalPosition::new(x.max(0.0), pos.y + sz.height + 6.0));
        return;
    }
    let at = app.cursor_position().ok();
    let monitor = at.and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten()).or_else(|| app.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let (mp, ms) = (m.position(), m.size());
        let x = mp.x as f64 + (ms.width as f64 - size.width as f64) / 2.0;
        let y = mp.y as f64 + ms.height as f64 * 0.2;
        let _ = w.set_position(PhysicalPosition::new(x, y));
    } else {
        let _ = w.center();
    }
}

/// Shows Quick Ask (or hides it when it's already in front).
pub fn toggle(app: &AppHandle, tray: Option<Rect>) {
    let w = match window(app) {
        Ok(w) => w,
        Err(e) => return log::warn!("Quick Ask window: {e}"),
    };
    if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    place(app, &w, tray);
    let _ = w.show();
    let _ = w.set_focus();
    let _ = w.emit("quick://shown", ());
}

/// Shows Quick Ask (never hides it): the wake word.
pub fn show(app: &AppHandle) {
    let w = match window(app) {
        Ok(w) => w,
        Err(e) => return log::warn!("Quick Ask window: {e}"),
    };
    if !w.is_visible().unwrap_or(false) {
        place(app, &w, None);
    }
    let _ = w.show();
    let _ = w.set_focus();
    let _ = w.emit("quick://shown", ());
}

/// Quick Ask hides when you click elsewhere, like Spotlight.
pub fn on_blur(w: &tauri::Window) {
    if w.label() == LABEL {
        let _ = w.hide();
    }
}

#[tauri::command]
pub fn quick_toggle(app: AppHandle) {
    toggle(&app, None);
}

#[tauri::command]
pub fn quick_hide(app: AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/// "Open in BYTE": the main window shows that chat.
#[tauri::command]
pub fn quick_open(app: AppHandle, conversation_id: Option<String>) {
    quick_hide(app.clone());
    crate::background::show_main(&app);
    let _ = app.emit_to("main", "quick://open", conversation_id);
}

// ------------------------------------------------------------------ menu bar

/// Adds or removes the menu-bar icon to match the setting.
pub fn apply_tray(app: &AppHandle, on: bool) {
    let exists = app.tray_by_id(TRAY).is_some();
    if on && !exists {
        if let Err(e) = build_tray(app) {
            log::warn!("menu-bar icon: {e}");
        }
    } else if !on && exists {
        let _ = app.remove_tray_by_id(TRAY);
    }
}

/// The menu's "Offline" tick, kept so a change in Settings shows there too.
static OFFLINE_ITEM: Mutex<Option<CheckMenuItem<tauri::Wry>>> = Mutex::new(None);

pub fn sync_offline_item(on: bool) {
    if let Some(item) = OFFLINE_ITEM.lock().ok().and_then(|i| i.clone()) {
        let _ = item.set_checked(on);
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let offline = CheckMenuItem::with_id(app, "offline", "Offline (no internet)", true, crate::offline::is_offline(), None::<&str>)?;
    if let Ok(mut slot) = OFFLINE_ITEM.lock() {
        *slot = Some(offline.clone());
    }
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "quick", "Ask BYTE…", true, None::<&str>)?,
            &MenuItem::with_id(app, "show", "Show BYTE", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &offline,
            &MenuItem::with_id(app, "lock", "Lock BYTE", crate::lock::AVAILABLE, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit BYTE", true, None::<&str>)?,
        ],
    )?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id(TRAY)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("BYTE")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "quick" => toggle(app, None),
            "show" => crate::background::show_main(app),
            "offline" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    crate::privacy::set_offline(&app, !crate::offline::is_offline()).await;
                });
            }
            "lock" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if app.state::<crate::state::AppState>().settings.lock().await.lock_enabled {
                        crate::lock::set_locked(&app, true);
                    } else {
                        crate::background::show_main(&app);
                        let _ = app.emit("settings://open", "privacy");
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = e {
                toggle(tray.app_handle(), Some(rect));
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_parse_and_read_nicely() {
        assert!(parse_keys("Alt+Space").is_ok());
        assert!(parse_keys("Alt+Super+KeyB").is_ok());
        assert!(parse_keys("CmdOrCtrl+Shift+K").is_ok());
        assert!(parse_keys("").is_err());
        assert!(parse_keys("Banana+Q").is_err());
        // ⇧ alone, or no modifier, would take keys away from typing.
        assert!(parse_keys("Shift+A").unwrap_err().contains("⌘, ⌥ or ⌃"));
        assert!(parse_keys("KeyA").is_err());
        assert_eq!(pretty_for("Alt+Space", false), "⌥Space");
        assert_eq!(pretty_for("Alt+Super+KeyB", false), "⌥⌘B");
        assert_eq!(pretty_for("Control+Shift+Digit1", false), "⌃⇧1");
        // Windows names its modifiers, and Super is the Windows key.
        assert_eq!(pretty_for("Control+Alt+KeyB", true), "Ctrl+Alt+B");
        assert_eq!(pretty_for("Alt+Super+KeyB", true), "Alt+Win+B");
        assert_eq!(pretty_for("CmdOrCtrl+Shift+KeyK", true), "Ctrl+Shift+K");
        assert_eq!(pretty_for("CmdOrCtrl+Shift+KeyK", false), "⇧⌘K");
    }

    #[test]
    fn the_two_shortcuts_must_differ() {
        let mut s = Settings::default();
        assert!(check(&s).is_ok());
        s.quick_ask_keys = s.selection_keys.clone();
        assert!(check(&s).unwrap_err().to_string().contains("already the selection shortcut"));
        // Off shortcuts don't count.
        s.selection_hotkey = false;
        assert!(check(&s).is_ok());
        s.quick_ask_keys = "Shift+Q".into();
        assert!(check(&s).is_err());
        s.quick_ask = false;
        assert!(check(&s).is_ok());
    }
}
