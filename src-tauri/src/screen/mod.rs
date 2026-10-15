//! BYTE looking at the screen, only when the person has switched it on (Settings → Screen).
//!
//! * `capture` / `wgc`: taking a picture (Windows.Graphics.Capture; no injection into games). `frame` is the pixels.
//! * `policy`: which windows are never looked at. `config`: the settings as one value.
//! * `ring` / `replay` / `sample`: the rolling last-30-seconds buffer, in memory only, and which frames to ask about.
//! * `phrases` / `prompts` / `flow`: "what do you think about this", "where did I go wrong", game advice.
//! * `read`: "read this page" aloud.
//!
//! Everything is off by default, shows an indicator while it works, never runs in a private chat, and sends nothing to
//! the cloud unless the person chose the cloud for that question (and then only words, never the picture).

// Windows is the only system with a real screen source so far; elsewhere the pieces are only exercised by tests.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod capture;
pub mod config;
pub mod flow;
pub mod frame;
pub mod phrases;
pub mod policy;
pub mod prompts;
pub mod read;
pub mod replay;
pub mod ring;
pub mod sample;
#[cfg(windows)]
pub mod wgc;

use tauri::{AppHandle, Emitter};

use crate::settings::Settings;

/// Event: the replay buffer started, paused or stopped (`replay::Status`).
pub const RECORDING_EVENT: &str = "screen://recording";
/// Event: BYTE is looking at the screen right now for one question (`{ "active": bool }`).
pub const LOOKING_EVENT: &str = "screen://looking";

/// Looking at the screen is built for this system.
pub fn supported() -> bool {
    cfg!(windows)
}

/// The replay buffer is keeping frames right now.
pub fn recording() -> bool {
    replay::recording()
}

/// Called whenever settings change (and once at start): starts or stops the replay buffer.
pub fn apply(app: &AppHandle, s: &Settings) {
    replay::apply(app, &config::Config::from_settings(s));
}

/// Called when the app quits: whatever was kept is forgotten.
pub fn shutdown() {
    replay::stop(None);
}

pub(crate) fn announce(app: &AppHandle, status: &replay::Status) {
    let _ = app.emit(RECORDING_EVENT, status);
    // The tray shows it too (the tray's id is quick.rs's "byte"); a phone has no tray.
    #[cfg(desktop)]
    if let Some(tray) = app.tray_by_id("byte") {
        let tip = if status.recording {
            format!("BYTE: recording the last {} s of your screen (memory only)", status.seconds)
        } else if status.on {
            format!("BYTE: screen replay paused ({})", status.paused.clone().unwrap_or_default())
        } else {
            "BYTE".to_string()
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

pub(crate) fn looking(app: &AppHandle, active: bool) {
    let _ = app.emit(LOOKING_EVENT, serde_json::json!({ "active": active }));
}

// ------------------------------------------------------------------ commands

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenInfo {
    pub supported: bool,
    pub status: replay::Status,
    /// Frames in memory and their size, for the Settings page.
    pub frames: usize,
}

#[tauri::command]
pub fn screen_status() -> ScreenInfo {
    ScreenInfo { supported: supported(), status: replay::status(), frames: replay::snapshot().len() }
}

/// Forgets what the replay holds right now.
#[tauri::command]
pub fn screen_clear() {
    replay::clear();
}

/// The app tells BYTE a private chat is open (or closed): while one is, nothing is kept and the buffer is emptied.
#[tauri::command]
pub fn screen_private(on: bool) {
    replay::set_private(on);
}

/// The names the default exclusion list uses, so Settings can offer "reset".
#[tauri::command]
pub fn screen_default_excludes() -> Vec<String> {
    policy::default_excludes()
}
