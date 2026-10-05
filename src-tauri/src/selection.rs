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
use crate::macctl::{Command, RunError, Runner};
// Only the AppleScript path uses it; Windows has its own implementation below.
#[cfg(not(windows))]
use crate::macctl::MacRunner;

/// The default hotkey, in Tauri's shortcut syntax (⌥⌘B on a Mac; Settings → Keyboard changes it).
#[cfg(not(windows))]
pub const HOTKEY: &str = "Alt+Super+KeyB";
/// Ctrl+Alt+B on Windows. Not Alt+Win+B: Windows already uses that to toggle HDR.
#[cfg(windows)]
pub const HOTKEY: &str = "Ctrl+Alt+KeyB";

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


/// The Windows way to do what the AppleScripts above do: find the app in front,
/// press Ctrl+C in it, and later put text back with Ctrl+V.
///
/// Three things are different from the Mac and each is handled on purpose:
///
/// * The person is still HOLDING the hotkey's modifiers when it fires, so a
///   synthetic Ctrl+C would arrive as Ctrl+Alt+C. It waits for them to be released.
/// * There is no "activate an app by name". The window handle is remembered at
///   capture time and checked to still belong to the same program before pasting,
///   so text can never land in a different window that merely reused the handle.
/// * Windows blocks synthetic input into a window running as administrator (UIPI)
///   without any error, so a failed send says so rather than reporting "nothing
///   was selected".
#[cfg(windows)]
mod win {
    use std::sync::Mutex;

    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU,
        VK_RWIN, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE};

    /// The window the last capture came from, and the program it belonged to.
    static LAST: Mutex<Option<(isize, String)>> = Mutex::new(None);

    fn hwnd(h: isize) -> HWND {
        HWND(h as *mut core::ffi::c_void)
    }

    /// The program (file name without ".exe") a window belongs to.
    pub fn program_of(window: HWND) -> Option<String> {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        if pid == 0 {
            return None;
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
        let _ = unsafe { CloseHandle(process) };
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned())
    }

    /// The window in front and its program.
    pub fn front() -> Option<(isize, String)> {
        let w = unsafe { GetForegroundWindow() };
        if w.0.is_null() {
            return None;
        }
        Some((w.0 as isize, program_of(w)?))
    }

    pub fn remember(h: isize, app: &str) {
        if let Ok(mut l) = LAST.lock() {
            *l = Some((h, app.to_string()));
        }
    }

    /// The remembered window, only if it still exists and is still that program.
    pub fn remembered(app: &str) -> Option<isize> {
        let (h, name) = LAST.lock().ok()?.clone()?;
        let w = hwnd(h);
        (unsafe { IsWindow(Some(w)) }.as_bool() && name.eq_ignore_ascii_case(app) && program_of(w).is_some_and(|p| p.eq_ignore_ascii_case(app))).then_some(h)
    }

    pub fn activate(h: isize) {
        let w = hwnd(h);
        unsafe {
            if IsIconic(w).as_bool() {
                let _ = ShowWindow(w, SW_RESTORE);
            }
            let _ = SetForegroundWindow(w);
        }
    }

    /// True while Ctrl, Alt, Shift or a Windows key is physically down.
    pub fn modifiers_down() -> bool {
        [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|k| (unsafe { GetAsyncKeyState(i32::from(k.0)) } as u16) & 0x8000 != 0)
    }

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() }, time: 0, dwExtraInfo: 0 } },
        }
    }

    /// Ctrl + `vk` in one call, so nothing can slip between the key presses.
    pub fn ctrl_chord(vk: VIRTUAL_KEY) -> Result<(), String> {
        let inputs = [key(VK_CONTROL, false), key(vk, false), key(vk, true), key(VK_CONTROL, true)];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            Err("BYTE couldn't send the keys to that app. If it's running as administrator, Windows blocks this; copy the text yourself, or run BYTE as administrator too.".into())
        }
    }
}

/// Waits (up to a second) for the hotkey's own modifiers to be let go.
#[cfg(windows)]
async fn wait_for_modifiers_up() {
    for _ in 0..50 {
        if !win::modifiers_down() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// Windows: copies the selection from the app in front, leaving the clipboard as it was.
#[cfg(windows)]
pub async fn capture_windows() -> AppResult<Captured> {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_C;
    let (handle, app) = win::front().unwrap_or((0, String::new()));
    if app.eq_ignore_ascii_case("byte") {
        return Err(AppError::msg("Select some text in another app first, then press Ctrl+Alt+B."));
    }
    if handle != 0 {
        win::remember(handle, &app);
    }
    wait_for_modifiers_up().await;
    let before_count = crate::clipboard::board::change_count();
    let before = crate::clipboard::board::text();
    win::ctrl_chord(VK_C).map_err(AppError::msg)?;
    // The app copies asynchronously: wait (up to ~0.6 s) for the clipboard to change.
    let mut copied = None;
    for _ in 0..12 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if crate::clipboard::board::change_count() != before_count {
            copied = crate::clipboard::board::text();
            break;
        }
    }
    if let Some(b) = &before {
        crate::clipboard::set_quietly(b);
    }
    match copied.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        Some(text) => Ok(Captured { text, app }),
        None => Err(AppError::msg("Nothing was selected (or that app doesn't allow copying). Select some text, then press Ctrl+Alt+B.")),
    }
}

/// Windows: puts text into the app it came from: on the clipboard, then Ctrl+V there.
#[cfg(windows)]
pub async fn paste_into_windows(app: &str, text: &str) -> AppResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_V;
    let handle = win::remembered(app).ok_or_else(|| AppError::msg(format!("{app} isn't open any more, so there's nowhere to paste. Copy the result instead.")))?;
    crate::clipboard::set_quietly(text);
    win::activate(handle);
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    wait_for_modifiers_up().await;
    win::ctrl_chord(VK_V).map_err(AppError::msg)
}

/// The hotkey was pressed: read the selection and open the studio with it.
pub fn pressed(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        #[cfg(windows)]
        let result = capture_windows().await;
        #[cfg(not(windows))]
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
    #[cfg(windows)]
    return paste_into_windows(&app_name, &text).await;
    #[cfg(not(windows))]
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

    /// The Win32 pieces that need no one at the keyboard: resolving a window's
    /// program name through OpenProcess and QueryFullProcessImageNameW, and the
    /// "modifiers are down" check. Whether Ctrl+C then reaches another app needs an
    /// interactive desktop and a person, and is not claimed here.
    #[cfg(windows)]
    #[test]
    fn windows_resolves_program_names_and_reads_modifier_state() {
        use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;
        let desktop = unsafe { GetDesktopWindow() };
        eprintln!("desktop window program: {:?}", win::program_of(desktop));
        eprintln!("modifiers down right now: {}", win::modifiers_down());
        // A handle that was never remembered must never be pasted into.
        assert!(win::remembered("definitely-not-running").is_none());
        win::remember(0x7fff_fff0, "ghost");
        assert!(win::remembered("ghost").is_none(), "a window that no longer exists is refused");
    }
}
