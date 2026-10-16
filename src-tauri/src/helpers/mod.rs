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
pub mod captions;
pub mod dialogs;
pub mod dictate;
pub mod explain;
pub mod focus;
pub mod game;
pub mod ha;
pub mod meeting;
pub mod navigation;
pub mod mic;
pub mod page;
pub mod popup;
pub mod readaloud;
pub mod recipes;
pub mod region;
pub mod smartpaste;
pub mod tidy;
pub mod wakeword;

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
    Dictate,
    Page,
    Read,
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
    if s.helper_dictate && dictate::supported() {
        v.push((Hot::Dictate, s.helper_dictate_keys.clone()));
    }
    if s.helper_read && readaloud::supported() {
        v.push((Hot::Read, s.helper_read_keys.clone()));
    }
    if s.helper_page && region::supported() {
        v.push((Hot::Page, s.helper_page_keys.clone()));
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
        Hot::Dictate => dictate::start(app),
        Hot::Page => page::read_url(app),
        Hot::Read => readaloud::toggle(app),
    }
}

/// Called whenever settings change (and once at start): starts or stops the watchers.
#[cfg(desktop)]
pub fn apply(app: &AppHandle, s: &Settings) {
    wakeword::set(&s.helper_wake_phrase);
    dialogs::apply(app, s.helper_dialogs);
    smartpaste::apply(app, s.helper_smart_paste);
    tidy::apply_watch(app, s.helper_tidy && tidy::supported() && !s.kids_mode);
    let caps = s.helper_captions && captions::supported() && !s.kids_mode;
    if CAPTIONS_ON.swap(caps, std::sync::atomic::Ordering::SeqCst) != caps {
        if !caps {
            captions::stop(app);
        }
        refresh_menu(app);
    }
    let game = s.helper_game && game_supported() && !s.kids_mode;
    if GAME_ON.swap(game, std::sync::atomic::Ordering::SeqCst) != game {
        refresh_menu(app);
    }
    let tidy_menu = s.helper_tidy && tidy::supported() && !s.kids_mode;
    if TIDY_ON.swap(tidy_menu, std::sync::atomic::Ordering::SeqCst) != tidy_menu {
        refresh_menu(app);
    }
    let focus = s.helper_focus && focus::supported() && !s.kids_mode;
    if FOCUS_ON.swap(focus, std::sync::atomic::Ordering::SeqCst) != focus {
        refresh_menu(app);
    }
    let meet = s.helper_meeting && meeting::supported() && !s.kids_mode;
    if MEETING_ON.swap(meet, std::sync::atomic::Ordering::SeqCst) != meet {
        refresh_menu(app);
    }
    let page = s.helper_page && region::supported() && !s.kids_mode;
    if PAGE_ON.swap(page, std::sync::atomic::Ordering::SeqCst) != page {
        refresh_menu(app);
    }
}

/// The tray menu offers live captions.
#[cfg(desktop)]
static CAPTIONS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(desktop)]
pub fn set_captions_menu(app: &AppHandle, listening: bool) {
    if let Some(tray) = app.tray_by_id("byte") {
        let _ = tray.set_tooltip(Some(if listening { "BYTE: live captions on" } else { "BYTE" }));
    }
    refresh_menu(app);
}

/// The tray menu offers the game companion.
#[cfg(desktop)]
static GAME_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn game_supported() -> bool {
    crate::screen::supported()
}

/// The tray menu offers "Tidy my Downloads".
#[cfg(desktop)]
static TIDY_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The tray menu offers "Start a focus session".
#[cfg(desktop)]
static FOCUS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(desktop)]
pub fn set_focus_menu(app: &AppHandle, running: bool) {
    if let Some(tray) = app.tray_by_id("byte") {
        let _ = tray.set_tooltip(Some(if running { "BYTE: focus session" } else { "BYTE" }));
    }
    refresh_menu(app);
}

/// Says `text` aloud with BYTE's voice, unless a call is going on.
#[cfg(desktop)]
pub async fn speak(app: &AppHandle, text: &str) {
    use tauri::Manager;
    if crate::meeting::active() {
        return;
    }
    let state = app.state::<crate::state::AppState>();
    if let Err(e) = crate::speech::speech_say(app.clone(), state, text.to_string(), None).await {
        log::info!("helper speech: {e}");
    }
}

/// The tray menu offers "Record a meeting".
#[cfg(desktop)]
static MEETING_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The tray menu's meeting entry changes between "Record" and "Stop".
#[cfg(desktop)]
pub fn set_meeting_menu(app: &AppHandle, recording: bool) {
    if let Some(tray) = app.tray_by_id("byte") {
        let _ = tray.set_tooltip(Some(if recording { "BYTE: recording a meeting" } else { "BYTE" }));
    }
    refresh_menu(app);
}

/// The tray menu offers "Look at this page and help".
#[cfg(desktop)]
static PAGE_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    if PAGE_ON.load(std::sync::atomic::Ordering::SeqCst) {
        v.push(MenuItem::with_id(app, "helper_page", "Look at this page and help", true, None::<&str>)?);
    }
    if CAPTIONS_ON.load(std::sync::atomic::Ordering::SeqCst) {
        if captions::listening() {
            v.push(MenuItem::with_id(app, "helper_captions_recap", "What did they just say?", true, None::<&str>)?);
            v.push(MenuItem::with_id(app, "helper_captions_stop", "Stop live captions", true, None::<&str>)?);
        } else {
            v.push(MenuItem::with_id(app, "helper_captions_start", "Start live captions", true, None::<&str>)?);
        }
    }
    if GAME_ON.load(std::sync::atomic::Ordering::SeqCst) {
        v.push(MenuItem::with_id(app, "helper_game_review", "Review my last play", true, None::<&str>)?);
        v.push(MenuItem::with_id(app, "helper_game_advise", "How is my game going?", true, None::<&str>)?);
    }
    if TIDY_ON.load(std::sync::atomic::Ordering::SeqCst) {
        v.push(MenuItem::with_id(app, "helper_tidy", "Tidy my Downloads", true, None::<&str>)?);
    }
    if FOCUS_ON.load(std::sync::atomic::Ordering::SeqCst) {
        if focus::running() {
            v.push(MenuItem::with_id(app, "helper_focus_end", "End the focus session", true, None::<&str>)?);
        } else {
            v.push(MenuItem::with_id(app, "helper_focus_start", "Start a focus session", true, None::<&str>)?);
        }
    }
    if MEETING_ON.load(std::sync::atomic::Ordering::SeqCst) {
        if meeting::recording() {
            v.push(MenuItem::with_id(app, "helper_meeting_stop", "Stop the meeting recording", true, None::<&str>)?);
        } else {
            v.push(MenuItem::with_id(app, "helper_meeting_start", "Record a meeting", true, None::<&str>)?);
        }
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
        "helper_captions_start" => {
            captions::start(app);
            true
        }
        "helper_captions_stop" => {
            captions::stop(app);
            true
        }
        "helper_captions_recap" => {
            captions::recap(app);
            true
        }
        "helper_game_review" => {
            game::review(app);
            true
        }
        "helper_game_advise" => {
            game::advise(app);
            true
        }
        "helper_tidy" => {
            tidy::now(app);
            true
        }
        "helper_focus_start" => {
            focus::ask_plan(app);
            true
        }
        "helper_focus_end" => {
            focus::end(app);
            true
        }
        "helper_meeting_start" => {
            meeting::start(app);
            true
        }
        "helper_meeting_stop" => {
            meeting::stop(app);
            true
        }
        "helper_page" => {
            region::start_page(app);
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
        assert!(!s.helper_region && !s.helper_explain && !s.helper_dialogs && !s.helper_smart_paste && !s.helper_dictate && !s.helper_page && !s.helper_meeting && !s.helper_focus && !s.helper_tidy && !s.helper_recipes && !s.helper_ha && s.helper_ha_url.is_empty() && !s.helper_game && !s.helper_nav && !s.helper_read && !s.helper_captions);
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
        for k in [&s.helper_region_keys, &s.helper_explain_keys, &s.helper_dictate_keys, &s.helper_page_keys, &s.helper_read_keys] {
            assert!(crate::quick::parse_keys(k).is_ok(), "{k}");
        }
        assert_ne!(s.helper_region_keys, s.helper_explain_keys);
        assert_ne!(s.helper_dictate_keys, s.helper_explain_keys);
        assert_ne!(s.helper_dictate_keys, s.helper_region_keys);
        for other in [&s.selection_keys, &s.quick_ask_keys] {
            assert_ne!(&s.helper_region_keys, other);
            assert_ne!(&s.helper_explain_keys, other);
        }
    }

    #[test]
    fn settings_are_not_in_the_profile_sync_list() {
        for k in ["helperAskWhere", "helperRegion", "helperExplain", "helperDialogs", "helperSmartPaste", "helperRegionKeys", "helperExplainKeys", "helperDictate", "helperDictateKeys", "helperPage", "helperPageKeys", "helperMeeting", "helperMeetingMinutes", "helperFocus", "helperFocusMinutes", "helperFocusBreak", "helperFocusSpeak", "helperTidy", "helperRecipes", "helperHa", "helperHaUrl", "helperGame", "helperNav", "helperRead", "helperReadKeys", "helperReadSpeed", "helperCaptions", "helperWakePhrase"] {
            assert!(!crate::profile_sync::KEYS.iter().any(|(_, setting)| *setting == k), "{k}");
        }
    }
}
