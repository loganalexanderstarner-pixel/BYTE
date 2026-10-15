//! Quick Ask on phones (`quick.rs` is the desktop version: a floating window, a
//! menu-bar icon and global shortcuts, none of which exist on Android).
//!
//! The same calls exist here so shared code doesn't need to know which one it
//! has. On a phone, Quick Ask is BYTE itself coming to the front; Ask BYTE (the
//! assistant role, the text-selection menu) arrives with the Android plugin
//! (docs/ANDROID.md, A4).

use tauri::{AppHandle, Emitter};

use crate::error::AppResult;
use crate::settings::Settings;

pub const LABEL: &str = "main";

/// No shortcuts to check: there is no keyboard to press them on.
pub fn check(_s: &Settings) -> AppResult<()> {
    Ok(())
}

pub fn apply_shortcuts(_app: &AppHandle, _s: &Settings) {}

/// No menu bar on a phone.
pub fn apply_tray(_app: &AppHandle, _on: bool) {}

pub fn sync_offline_item(_on: bool) {}

pub fn on_blur(_w: &tauri::Window) {}

/// "Hey BYTE" and Ask BYTE bring BYTE to the front, listening.
pub fn show(app: &AppHandle) {
    crate::background::show_main(app);
    let _ = app.emit_to("main", "quick://shown", ());
}

#[tauri::command]
pub fn quick_toggle(app: AppHandle) {
    show(&app);
}

#[tauri::command]
pub fn quick_hide(_app: AppHandle) {}

#[tauri::command]
pub fn quick_open(app: AppHandle, conversation_id: Option<String>) {
    crate::background::show_main(&app);
    let _ = app.emit_to("main", "quick://open", conversation_id);
}
