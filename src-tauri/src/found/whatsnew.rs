//! "What's new": after BYTE has been updated, a one-time card shows the notes for the version now running. The notes are the
//! file `docs/releases/v<version>.md`, built into the app, so they work offline. Which version the person last saw is a small
//! file next to the settings (not a setting, so it never follows the person to another device).

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// The release notes for the version this build is.
pub const NOTES: &str = include_str!(concat!("../../../docs/releases/v", env!("CARGO_PKG_VERSION"), ".md"));

const SEEN: &str = "whatsnew_seen.txt";

fn seen_path(root: &Path) -> PathBuf {
    root.join(SEEN)
}

/// Pure: show the card when BYTE was updated, that is when a different version was last seen. Nothing is shown on a first
/// install (`first_run`), or when no version was ever recorded and the person has not set BYTE up yet; an upgrade from a build that
/// did not record versions counts as an update.
pub fn should_show(seen: Option<&str>, current: &str, first_run: bool) -> bool {
    match seen.map(str::trim).filter(|s| !s.is_empty()) {
        Some(v) => v != current,
        None => !first_run,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub version: String,
    pub notes: String,
    pub show: bool,
}

pub fn status_at(root: &Path, first_run: bool) -> WhatsNew {
    let current = env!("CARGO_PKG_VERSION");
    let seen = std::fs::read_to_string(seen_path(root)).ok();
    WhatsNew { version: current.into(), notes: NOTES.into(), show: should_show(seen.as_deref(), current, first_run) }
}

/// Records that this version's card was seen (or that this is a fresh install, so the first update shows one).
pub fn mark_seen_at(root: &Path) {
    let _ = std::fs::write(seen_path(root), env!("CARGO_PKG_VERSION"));
}

#[tauri::command]
pub async fn whatsnew_status(state: State<'_, AppState>) -> Result<WhatsNew, String> {
    let first_run = !state.settings.lock().await.onboarding_complete;
    let w = status_at(&state.paths.root, first_run);
    // A fresh install has nothing new to read: remember the version now so the next update is noticed.
    if first_run && !w.show {
        mark_seen_at(&state.paths.root);
    }
    Ok(w)
}

#[tauri::command]
pub fn whatsnew_seen(state: State<'_, AppState>) {
    mark_seen_at(&state.paths.root);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_update_shows_the_card_and_the_same_version_does_not() {
        assert!(should_show(Some("0.12.5"), "0.12.6", false));
        assert!(!should_show(Some("0.12.6"), "0.12.6", false));
        assert!(!should_show(Some("0.12.6\n"), "0.12.6", false));
    }

    #[test]
    fn a_fresh_install_shows_nothing_but_an_upgrade_from_an_older_build_does() {
        assert!(!should_show(None, "0.12.6", true));
        assert!(should_show(None, "0.12.6", false));
        assert!(should_show(Some("  "), "0.12.6", false));
    }

    #[test]
    fn the_notes_are_the_file_for_this_version() {
        assert!(NOTES.starts_with("# "), "release notes start with a title");
        assert!(NOTES.contains(env!("CARGO_PKG_VERSION")), "and name the version");
    }

    #[test]
    fn seeing_it_once_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        assert!(status_at(dir.path(), false).show);
        mark_seen_at(dir.path());
        assert!(!status_at(dir.path(), false).show);
        assert_eq!(status_at(dir.path(), false).version, env!("CARGO_PKG_VERSION"));
    }
}
