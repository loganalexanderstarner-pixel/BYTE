//! The selection hotkey (⌥⌘B, Mac control): select text in any app, press it,
//! and the writing studio opens with that text: rewrite, shorten, fix, translate,
//! write a reply or explain it, then "Paste into <app>" puts the result back.
//!
//! How: BYTE notes the app in front, presses ⌘C for you (macOS asks once for
//! Accessibility permission), reads the clipboard and puts back what was on it
//! before. Pasting back activates that app and presses ⌘V. The scripts are fixed
//! (macctl pattern); the app name travels as an argument.

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::error::{AppError, AppResult};
use crate::macctl::{Command, MacRunner, RunError, Runner};

/// The hotkey, in Tauri's shortcut syntax (⌥⌘B on a Mac).
pub const HOTKEY: &str = "Alt+Super+KeyB";

const FRONT_APP: &str = r#"on run argv
	tell application "System Events" to return name of first application process whose frontmost is true
end run"#;

const PRESS_COPY: &str = r#"on run argv
	tell application "System Events" to keystroke "c" using command down
end run"#;

const PASTE_INTO: &str = r#"on run argv
	tell application (item 1 of argv) to activate
	delay 0.35
	tell application "System Events" to keystroke "v" using command down
end run"#;

/// For the syntax check on macOS.
#[cfg_attr(not(test), allow(dead_code))]
pub const SCRIPTS: &[&str] = &[FRONT_APP, PRESS_COPY, PASTE_INTO];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Captured {
    pub text: String,
    /// The app it came from ("" if unknown).
    pub app: String,
}

/// Why the text couldn't be read, for the user.
fn explain(e: &RunError) -> String {
    match e {
        RunError::NotAllowed => "BYTE needs permission to copy the selected text: System Settings → Privacy & Security → Accessibility → turn on BYTE (then press ⌥⌘B again).".into(),
        other => other.text("System Events"),
    }
}

/// Copies the selection from the app in front, leaving the clipboard as it was.
pub async fn capture(runner: &dyn Runner) -> AppResult<Captured> {
    let app = runner.run(&Command::Osa { script: FRONT_APP, args: vec![] }).await.unwrap_or_default().trim().to_string();
    if app == "BYTE" {
        return Err(AppError::msg("Select some text in another app first, then press ⌥⌘B."));
    }
    let before_count = crate::clipboard::board::change_count();
    let before = crate::clipboard::board::text();
    runner.run(&Command::Osa { script: PRESS_COPY, args: vec![] }).await.map_err(|e| AppError::msg(explain(&e)))?;
    // The app copies asynchronously: wait (up to ~0.6 s) for the clipboard to change.
    let mut copied = None;
    for _ in 0..12 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if crate::clipboard::board::change_count() != before_count {
            copied = crate::clipboard::board::text();
            break;
        }
    }
    // Put the user's clipboard back the way it was.
    if let Some(b) = &before {
        crate::clipboard::set_quietly(b);
    }
    match copied.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        Some(text) => Ok(Captured { text, app }),
        None => Err(AppError::msg("Nothing was selected (or that app doesn't allow copying). Select some text, then press ⌥⌘B.")),
    }
}

/// Puts text into an app: on the clipboard, then ⌘V there.
pub async fn paste_into(runner: &dyn Runner, app: &str, text: &str) -> AppResult<()> {
    crate::clipboard::set_quietly(text);
    runner.run(&Command::Osa { script: PASTE_INTO, args: vec![app.to_string()] }).await.map_err(|e| AppError::msg(explain(&e)))?;
    Ok(())
}

/// Registers or removes the hotkey to match the setting.
pub fn apply(app: &tauri::AppHandle, on: bool) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    // The hotkey copies with System Events, so it's macOS only for now.
    let on = on && cfg!(target_os = "macos");
    let gs = app.global_shortcut();
    let registered = gs.is_registered(HOTKEY);
    if on && !registered {
        if let Err(e) = gs.register(HOTKEY) {
            log::warn!("couldn't register {HOTKEY}: {e}");
        }
    } else if !on && registered {
        let _ = gs.unregister(HOTKEY);
    }
}

/// The hotkey was pressed: read the selection and open the studio with it.
pub fn pressed(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = capture(&MacRunner).await;
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        let _ = match result {
            Ok(c) => app.emit("selection://captured", c),
            Err(e) => app.emit("selection://error", e.to_string()),
        };
    });
}

/// "Paste into <app>" in the writing studio.
#[tauri::command]
pub async fn selection_paste(app_name: String, text: String) -> AppResult<()> {
    if app_name.trim().is_empty() || app_name == "BYTE" {
        return Err(AppError::msg("There's no app to paste into."));
    }
    paste_into(&MacRunner, &app_name, &text).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        ran: Mutex<Vec<Command>>,
        front: String,
    }
    impl Runner for Fake {
        fn run<'a>(&'a self, cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>> {
            self.ran.lock().unwrap().push(cmd.clone());
            let out = match cmd {
                Command::Osa { script, .. } if *script == FRONT_APP => Ok(self.front.clone()),
                _ => Ok(String::new()),
            };
            Box::pin(async move { out })
        }
    }

    #[tokio::test]
    async fn the_app_name_is_an_argument_and_byte_itself_is_refused() {
        let fake = Fake { front: "BYTE".into(), ..Default::default() };
        assert!(capture(&fake).await.unwrap_err().to_string().contains("another app"));
        let fake = Fake::default();
        paste_into(&fake, "Notes\" & (do shell script \"rm\")", "hi").await.unwrap();
        let ran = fake.ran.lock().unwrap().clone();
        assert_eq!(ran, vec![Command::Osa { script: PASTE_INTO, args: vec!["Notes\" & (do shell script \"rm\")".into()] }]);
        assert!(SCRIPTS.iter().all(|s| !s.contains("do shell script")));
    }

    /// On a Mac: the scripts compile, and the clipboard round-trips (history
    /// ignores BYTE's own writes).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn e2e_selection_scripts_and_clipboard() {
        let dir = tempfile::tempdir().unwrap();
        for (i, s) in SCRIPTS.iter().enumerate() {
            let out = std::process::Command::new("/usr/bin/osacompile").arg("-o").arg(dir.path().join(format!("{i}.scpt"))).arg("-e").arg(s).output().unwrap();
            assert!(out.status.success(), "script {i}: {}", String::from_utf8_lossy(&out.stderr));
        }
        let before = crate::clipboard::board::change_count();
        crate::clipboard::set_quietly("BYTE clipboard test 42");
        assert_eq!(crate::clipboard::board::text().as_deref(), Some("BYTE clipboard test 42"));
        assert_ne!(crate::clipboard::board::change_count(), before);
        assert!(!crate::clipboard::board::secret());
    }

    #[test]
    fn accessibility_errors_say_where_to_allow_it() {
        assert!(explain(&RunError::NotAllowed).contains("Privacy & Security → Accessibility → turn on BYTE"));
    }
}
