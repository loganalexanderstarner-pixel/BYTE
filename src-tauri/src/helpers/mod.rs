//! Everyday helpers: small things BYTE does anywhere on the computer, each with its own switch (off until switched on, per
//! person, kept on this device).
//!
//! * `region`: a hotkey draws a box on the screen; only that box is read, in memory, and a small popup offers what to do with it.
//! * `explain`: a hotkey over selected text shows a short explanation next to the pointer.
//! * `dialogs`: a new error or installer window puts "BYTE can explain this" on the tray icon.
//! * `smartpaste`: a copied tracking number, address, link or code gets a small toast with the useful action.
//! * `popup`: the small window those use. `ask`: one question to the model, where the person chose.
//!
//! The rule for all of them: nothing leaves the computer unless the person chose Cloud in `helperAskWhere`; text taken from
//! the screen goes to the cloud only when that setting says "cloud" (never with "auto"); nothing runs in a private chat or in
//! kids mode; a window on the never-look-at list is never read; pictures are kept in memory only.

pub mod ask;
pub mod dialogs;
pub mod explain;
pub mod popup;
pub mod region;
pub mod smartpaste;

use crate::settings::Settings;

#[cfg(desktop)]
use tauri::AppHandle;

pub fn default_ask_where() -> String {
    "local".into()
}

/// Default keys: Ctrl+Alt+R / Ctrl+Alt+E on Windows, Alt+Cmd+R / Alt+Cmd+E elsewhere.
pub fn default_region_keys() -> String {
    if cfg!(windows) { "Control+Alt+KeyR".into() } else { "Alt+Super+KeyR".into() }
}

pub fn default_explain_keys() -> String {
    if cfg!(windows) { "Control+Alt+KeyE".into() } else { "Alt+Super+KeyE".into() }
}

/// The helpers may not run in kids mode or while a private chat is open.
pub fn blocked_now() -> Option<&'static str> {
    if crate::kids::is_on() {
        return Some("Kids mode is on, so the helpers are off.");
    }
    if crate::screen::replay::is_private() {
        return Some("This is a private chat, so the helpers stay off.");
    }
    None
}

/// Which hotkey is which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hot {
    Region,
    Explain,
}

/// The keys each hotkey needs, from the settings (a switched-off helper has none).
pub fn wanted(s: &Settings) -> Vec<(Hot, String)> {
    let mut v = Vec::new();
    if s.helper_region && region::supported() {
        v.push((Hot::Region, s.helper_region_keys.clone()));
    }
    if s.helper_explain && explain::supported() {
        v.push((Hot::Explain, s.helper_explain_keys.clone()));
    }
    v
}

#[cfg(desktop)]
static REGISTERED: std::sync::Mutex<Vec<(u32, Hot)>> = std::sync::Mutex::new(Vec::new());

/// Registers the helper hotkeys (called by `quick::apply_shortcuts` after it cleared every shortcut). `taken`: ids in use.
#[cfg(desktop)]
pub fn register(app: &AppHandle, s: &Settings, taken: &[Option<u32>]) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let mut now: Vec<(u32, Hot)> = Vec::new();
    for (hot, keys) in wanted(s) {
        match crate::quick::parse_keys(&keys) {
            Ok(sc) if taken.contains(&Some(sc.id())) || now.iter().any(|(i, _)| *i == sc.id()) => {
                log::warn!("{keys} is already another BYTE shortcut");
            }
            Ok(sc) => match app.global_shortcut().register(sc) {
                Ok(()) => now.push((sc.id(), hot)),
                Err(e) => log::warn!("couldn't register {keys}: {e}"),
            },
            Err(e) => log::warn!("{e}"),
        }
    }
    if let Ok(mut r) = REGISTERED.lock() {
        *r = now;
    }
}

#[cfg(desktop)]
pub fn which(id: u32) -> Option<Hot> {
    REGISTERED.lock().ok()?.iter().find(|(i, _)| *i == id).map(|(_, h)| *h)
}

/// A helper hotkey was pressed.
#[cfg(desktop)]
pub fn pressed(app: &AppHandle, hot: Hot) {
    match hot {
        Hot::Region => region::start(app),
        Hot::Explain => explain::start(app),
    }
}

/// Called whenever settings change (and once at start): starts or stops the watchers.
#[cfg(desktop)]
pub fn apply(app: &AppHandle, s: &Settings) {
    dialogs::apply(app, s.helper_dialogs);
    smartpaste::apply(app, s.helper_smart_paste);
}

#[cfg(not(desktop))]
pub fn apply(_app: &tauri::AppHandle, _s: &Settings) {}

// ---------------------------------------------------------------- the tray menu

/// Extra entries for the tray icon's menu (only the ones that apply right now).
#[cfg(desktop)]
pub fn tray_items(app: &AppHandle) -> tauri::Result<Vec<tauri::menu::MenuItem<tauri::Wry>>> {
    use tauri::menu::MenuItem;
    let mut v = Vec::new();
    if dialogs::has_pending() {
        v.push(MenuItem::with_id(app, "helper_dialog", "BYTE can explain this", true, None::<&str>)?);
    }
    Ok(v)
}

/// A tray menu entry was chosen; true when it was one of the helpers'.
#[cfg(desktop)]
pub fn tray_event(app: &AppHandle, id: &str) -> bool {
    match id {
        "helper_dialog" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { dialogs::explain_pending(&app).await });
            true
        }
        _ => false,
    }
}

/// Rebuilds the tray menu (an entry appeared or went away).
#[cfg(desktop)]
pub fn refresh_menu(app: &AppHandle) {
    crate::quick::refresh_tray_menu(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_helper_is_off_by_default_and_the_default_choice_is_local() {
        let s = Settings::default();
        assert!(!s.helper_region && !s.helper_explain && !s.helper_dialogs && !s.helper_smart_paste);
        assert_eq!(s.helper_ask_where, "local");
        assert!(wanted(&s).is_empty());
    }

    #[test]
    fn a_helper_that_is_switched_on_asks_for_its_own_keys() {
        let s = Settings { helper_explain: true, ..Settings::default() };
        let w = wanted(&s);
        if explain::supported() {
            assert_eq!(w, vec![(Hot::Explain, s.helper_explain_keys.clone())]);
        } else {
            assert!(w.is_empty());
        }
    }

    #[test]
    fn the_default_keys_are_real_shortcuts_and_differ_from_the_others() {
        let s = Settings::default();
        for k in [&s.helper_region_keys, &s.helper_explain_keys] {
            assert!(crate::quick::parse_keys(k).is_ok(), "{k}");
        }
        assert_ne!(s.helper_region_keys, s.helper_explain_keys);
        for other in [&s.selection_keys, &s.quick_ask_keys] {
            assert_ne!(&s.helper_region_keys, other);
            assert_ne!(&s.helper_explain_keys, other);
        }
    }

    #[test]
    fn settings_are_not_in_the_profile_sync_list() {
        for k in ["helperAskWhere", "helperRegion", "helperExplain", "helperDialogs", "helperSmartPaste", "helperRegionKeys", "helperExplainKeys"] {
            assert!(!crate::profile_sync::KEYS.iter().any(|(_, setting)| *setting == k), "{k}");
        }
    }
}
