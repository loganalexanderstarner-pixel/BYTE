//! Error-dialog helper (Windows): when a new error or installer window appears, BYTE puts a small badge on its tray icon
//! ("BYTE can explain this"). Clicking it reads the window's words with UI Automation (the text the window itself exposes, not a
//! picture of it), asks the model what happened and shows the likely fix in a small popup.
//!
//! BYTE only looks at the list of windows (titles and classes) to notice a new one; it reads inside a window only when the
//! person asks. A window on the never-look-at list, a password field and BYTE's own windows are never read.

use std::collections::HashSet;

use crate::control::uia::WinRef;
use crate::screen::policy::{self, WindowInfo};

/// How sure the window is an error or installer window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    No,
    /// A standard dialog box (class #32770) with an ordinary-looking title: badge only.
    Weak,
    /// The title names an error, a failure or an installer: badge and a small notice.
    Strong,
}

const ERROR_WORDS: &[&str] = &["error", "failed", "failure", "can't", "cannot", "couldn't", "could not", "problem", "not responding", "stopped working", "has stopped", "fatal", "exception", "crash", "unable to", "access denied", "not found", "corrupt"];
const INSTALLER_WORDS: &[&str] = &["setup", "installer", "installation", "install ", "uninstall", "update failed", "windows installer"];
/// Dialog boxes people open on purpose: never badged.
const BENIGN_TITLES: &[&str] = &[
    "save as", "save", "open", "print", "select folder", "browse for folder", "choose", "insert", "properties", "run", "file upload", "select file", "open file", "save file", "export", "import", "page setup", "find", "replace", "go to", "rename", "new folder", "confirm save as", "select a file", "attach",
];

/// Classes of a standard dialog box.
const DIALOG_CLASS: &str = "#32770";

fn has_any(hay: &str, words: &[&str]) -> bool {
    words.iter().any(|w| hay.contains(w))
}

/// Is this window an error or installer dialog, judged only from its title, class and program?
pub fn judge(w: &WinRef) -> Strength {
    let program = w.app.to_lowercase();
    let title = w.title.trim().to_lowercase();
    if w.minimised || w.tool || w.cloaked || program == "byte" || program == "explorer" && w.class != DIALOG_CLASS {
        return Strength::No;
    }
    // The desktop shell's own windows (the taskbar, search, the start menu) are not dialogs.
    if matches!(w.class.as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Progman" | "WorkerW") {
        return Strength::No;
    }
    if has_any(&title, ERROR_WORDS) || has_any(&format!("{title} "), INSTALLER_WORDS) {
        return if title.is_empty() { Strength::No } else { Strength::Strong };
    }
    if w.class == DIALOG_CLASS && !title.is_empty() && !BENIGN_TITLES.iter().any(|b| title == *b || title.starts_with(&format!("{b} "))) {
        return Strength::Weak;
    }
    Strength::No
}

/// The windows that are new since the last look and are worth a badge, strongest first. `seen` is updated to what is open now;
/// the first call (`seen` empty and `first` true) only learns what is already open, so nothing old is announced.
pub fn newly(seen: &mut HashSet<isize>, current: &[WinRef], first: bool, excludes: &[String]) -> Vec<(WinRef, Strength)> {
    let now: HashSet<isize> = current.iter().map(|w| w.id).collect();
    let mut found: Vec<(WinRef, Strength)> = Vec::new();
    if !first {
        for w in current.iter().filter(|w| !seen.contains(&w.id)) {
            let s = judge(w);
            let info = WindowInfo { program: w.app.clone(), title: w.title.clone() };
            if s != Strength::No && policy::may_capture(Some(&info), excludes, false).is_ok() {
                found.push((w.clone(), s));
            }
        }
    }
    *seen = now;
    found.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
    found
}

/// The words a dialog shows, for the model: its title, the message, the choices. Only text the window exposes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogText {
    pub title: String,
    pub app: String,
    pub lines: Vec<String>,
    pub buttons: Vec<String>,
}

/// Tidies the raw names a window gave: one line each, no blanks or repeats, capped.
pub fn tidy(raw: &[String], most: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in raw {
        let line = r.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() || out.contains(&line) {
            continue;
        }
        out.push(line.chars().take(400).collect());
        if out.len() >= most {
            break;
        }
    }
    out
}

pub fn prompt(d: &DialogText) -> String {
    let mut s = String::from(
        "A window appeared on the person's computer. Below are the words it shows. Say in plain words what happened, the most likely cause, and what to do about it, step by step. \
If it is an installer or a question with choices, say what each button would do and which one is the safe choice. If you are not sure, say what to check first.\n\n<<<\n",
    );
    s.push_str(&format!("Window title: {}\nProgram: {}\n", d.title, d.app));
    for l in &d.lines {
        s.push_str(&format!("{l}\n"));
    }
    if !d.buttons.is_empty() {
        s.push_str(&format!("Buttons: {}\n", d.buttons.join(" | ")));
    }
    s.push_str(">>>");
    s
}

/// A copy of the tray icon's pixels with a red dot in the top-right corner (RGBA).
pub fn badge_pixels(rgba: &mut [u8], w: usize, h: usize) {
    let r = (w.min(h) as f32 * 0.22).max(2.0);
    let (cx, cy) = (w as f32 - r - 1.0, r + 1.0);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            if dx * dx + dy * dy <= r * r {
                let i = (y * w + x) * 4;
                if i + 3 < rgba.len() {
                    rgba[i..i + 4].copy_from_slice(&[230, 40, 40, 255]);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the watcher (Windows)

#[cfg(desktop)]
mod watch {
    use super::*;
    use crate::helpers::popup::{self, Kind, Payload};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use tauri::{AppHandle, Manager};

    static RUNNING: AtomicBool = AtomicBool::new(false);
    static PENDING: Mutex<Option<WinRef>> = Mutex::new(None);

    pub fn pending() -> Option<WinRef> {
        PENDING.lock().ok()?.clone()
    }

    pub fn apply(app: &AppHandle, on: bool) {
        if !on {
            RUNNING.store(false, Ordering::SeqCst);
            set_pending(app, None);
            return;
        }
        if !cfg!(windows) || RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut seen: HashSet<isize> = HashSet::new();
            let mut first = true;
            while RUNNING.load(Ordering::SeqCst) {
                let excludes = app.state::<crate::state::AppState>().settings.lock().await.screen_exclude_apps.clone();
                let blocked = crate::helpers::blocked_now().is_some();
                let list = tokio::task::spawn_blocking(|| crate::control::real_screen().windows()).await.unwrap_or_default();
                let found = newly(&mut seen, &list, first, &excludes);
                first = false;
                if let (Some((w, s)), false) = (found.first(), blocked) {
                    set_pending(&app, Some(w.clone()));
                    if *s == Strength::Strong {
                        popup::show(&app, Kind::Dialog, Payload { title: "BYTE can explain this".into(), body: format!("A window called \"{}\" just appeared.", w.title.trim()), buttons: vec![popup::button("explain", "Explain it"), popup::button("close", "Not now")], auto_hide: 9_000, ..Default::default() }, None);
                    }
                }
                // The window went away by itself: the badge goes too.
                if let Some(p) = pending() {
                    if !list.iter().any(|w| w.id == p.id) {
                        set_pending(&app, None);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
        });
    }

    pub fn set_pending(app: &AppHandle, w: Option<WinRef>) {
        let on = w.is_some();
        let changed = {
            let Ok(mut p) = PENDING.lock() else { return };
            let changed = p.as_ref().map(|x| x.id) != w.as_ref().map(|x| x.id);
            *p = w;
            changed
        };
        if !changed {
            return;
        }
        if let Some(tray) = app.tray_by_id("byte") {
            let _ = tray.set_tooltip(Some(if on { "BYTE can explain this" } else { "BYTE" }));
            if on {
                if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../../icons/tray.png")) {
                    let (w, h) = (icon.width() as usize, icon.height() as usize);
                    let mut px = icon.rgba().to_vec();
                    badge_pixels(&mut px, w, h);
                    let _ = tray.set_icon(Some(tauri::image::Image::new_owned(px, w as u32, h as u32)));
                    let _ = tray.set_icon_as_template(false);
                }
            } else if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../../icons/tray.png")) {
                let _ = tray.set_icon(Some(icon));
                let _ = tray.set_icon_as_template(true);
            }
        }
        crate::helpers::refresh_menu(app);
    }

    /// Reads the pending window and explains it.
    pub async fn explain(app: &AppHandle) {
        let Some(w) = pending() else { return };
        let say = |title: &str, body: String| popup::show(app, Kind::Dialog, Payload { title: title.into(), body, buttons: vec![popup::button("close", "Close")], auto_hide: 15_000, ..Default::default() }, None);
        if let Some(why) = crate::helpers::blocked_now() {
            return say("BYTE can explain this", why.into());
        }
        let state = app.state::<crate::state::AppState>();
        let excludes = state.settings.lock().await.screen_exclude_apps.clone();
        let info = WindowInfo { program: w.app.clone(), title: w.title.clone() };
        if let Err(r) = policy::may_capture(Some(&info), &excludes, false) {
            return say("BYTE can explain this", r.say());
        }
        popup::show(app, Kind::Dialog, Payload { title: "Reading that window".into(), body: "Reading its words...".into(), busy: true, ..Default::default() }, None);
        let id = w.id;
        let read = tokio::task::spawn_blocking(move || super::win::read_text(id, &w.app, &w.title)).await.unwrap_or_else(|_| Err("BYTE couldn't read that window.".into()));
        let text = match read {
            Ok(t) => t,
            Err(e) => return say("BYTE can explain this", e),
        };
        if text.lines.is_empty() && text.buttons.is_empty() {
            return say("BYTE can explain this", "That window doesn't share its words with other programs, so BYTE can't read it. Try the region helper on it instead.".into());
        }
        match crate::helpers::ask::run(&state, crate::helpers::ask::SYSTEM, &prompt(&text), 600, true).await {
            Ok((answer, place)) => {
                if let Ok(mut l) = LAST.lock() {
                    *l = answer.clone();
                }
                set_pending(app, None);
                popup::show(app, Kind::Dialog, Payload { title: format!("What that window means (answered on {place})"), body: answer, buttons: vec![popup::button("copy", "Copy"), popup::button("close", "Close")], auto_hide: 60_000, ..Default::default() }, None);
            }
            Err(e) => say("BYTE can explain this", e.to_string()),
        }
    }

    static LAST: Mutex<String> = Mutex::new(String::new());

    pub async fn clicked(app: &AppHandle, id: &str) {
        match id {
            "explain" => explain(app).await,
            "copy" => {
                let t = LAST.lock().map(|l| l.clone()).unwrap_or_default();
                if !t.is_empty() {
                    crate::clipboard::set_quietly(&t);
                }
                popup::hide(app);
            }
            _ => popup::hide(app),
        }
    }
}

#[cfg(desktop)]
pub fn apply(app: &tauri::AppHandle, on: bool) {
    watch::apply(app, on);
}
#[cfg(desktop)]
pub fn has_pending() -> bool {
    watch::pending().is_some()
}
#[cfg(desktop)]
pub async fn explain_pending(app: &tauri::AppHandle) {
    watch::explain(app).await;
}
#[cfg(desktop)]
pub async fn clicked(app: &tauri::AppHandle, id: &str) {
    watch::clicked(app, id).await;
}
#[cfg(not(desktop))]
pub fn apply(_app: &tauri::AppHandle, _on: bool) {}
#[cfg(not(desktop))]
pub fn has_pending() -> bool {
    false
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows::core::BSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;

    /// Reads a window's words with UI Automation: static text, the names of buttons, links and check boxes, and the contents of
    /// read-only text boxes. A password field is never read.
    pub fn read_text(id: isize, app: &str, title: &str) -> Result<DialogText, String> {
        crate::control::com::ensure()?;
        let hwnd = HWND(id as *mut core::ffi::c_void);
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err("That window has closed.".into());
        }
        let auto: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(|e| e.to_string())?;
        let root = unsafe { auto.ElementFromHandle(hwnd) }.map_err(|e| e.to_string())?;
        let cond = unsafe { auto.CreateTrueCondition() }.map_err(|e| e.to_string())?;
        let all = unsafe { root.FindAll(TreeScope_Subtree, &cond) }.map_err(|e| e.to_string())?;
        let n = unsafe { all.Length() }.map_err(|e| e.to_string())?;
        let (mut lines, mut buttons) = (Vec::new(), Vec::new());
        for i in 0..n.min(600) {
            let Ok(el) = (unsafe { all.GetElement(i) }) else { continue };
            let Ok(t) = (unsafe { el.CurrentControlType() }) else { continue };
            if unsafe { el.CurrentIsPassword() }.map(|b| b.as_bool()).unwrap_or(false) {
                continue;
            }
            let name = unsafe { el.CurrentName() }.map(|b: BSTR| b.to_string()).unwrap_or_default();
            if crate::control::uia::sensitive_name(&name) {
                continue;
            }
            match t {
                UIA_TextControlTypeId => lines.push(name),
                UIA_ButtonControlTypeId | UIA_HyperlinkControlTypeId | UIA_CheckBoxControlTypeId | UIA_RadioButtonControlTypeId => buttons.push(name),
                UIA_EditControlTypeId | UIA_DocumentControlTypeId => {
                    // A read-only box (a long error text, a licence) holds its words in the value.
                    let value = unsafe { el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }.ok().and_then(|p| unsafe { p.CurrentValue() }.ok()).map(|b| b.to_string()).unwrap_or_default();
                    if !value.trim().is_empty() {
                        lines.push(value);
                    }
                }
                _ => {}
            }
        }
        Ok(DialogText { title: title.to_string(), app: app.to_string(), lines: tidy(&lines, 40), buttons: tidy(&buttons, 12) })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The real UI Automation call on a window that is not there: it must fail politely, not crash.
        #[test]
        fn reading_a_window_that_is_gone_says_so() {
            let e = read_text(0x7fff_fff0, "ghost", "x").unwrap_err();
            assert!(e.contains("closed"), "{e}");
        }
    }
}

#[cfg(not(windows))]
mod win {
    use super::*;

    pub fn read_text(_id: isize, _app: &str, _title: &str) -> Result<DialogText, String> {
        Err(crate::platform_text::here("Reading an error window isn't built for this system yet."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: isize, app: &str, title: &str, class: &str) -> WinRef {
        WinRef { id, app: app.into(), title: title.into(), class: class.into(), ..Default::default() }
    }

    #[test]
    fn error_and_installer_titles_are_strong() {
        for t in ["Error", "Setup Failed", "Installation failed", "App has stopped working", "Windows Installer", "Can't open file", "Access denied", "MyApp Setup", "Update failed"] {
            assert_eq!(judge(&w(1, "setup", t, "Chrome_WidgetWin_1")), Strength::Strong, "{t}");
        }
    }

    #[test]
    fn a_standard_dialog_with_an_unknown_title_is_weak_and_the_usual_ones_are_not_badged() {
        assert_eq!(judge(&w(1, "app", "Something happened", DIALOG_CLASS)), Strength::Weak);
        for t in ["Save As", "Open", "Print", "Select Folder", "Save As - report", "Properties"] {
            assert_eq!(judge(&w(1, "notepad", t, DIALOG_CLASS)), Strength::No, "{t}");
        }
        // An ordinary window is not a dialog.
        assert_eq!(judge(&w(1, "chrome", "A news page", "Chrome_WidgetWin_1")), Strength::No);
        assert_eq!(judge(&w(1, "chrome", "", "Chrome_WidgetWin_1")), Strength::No);
    }

    #[test]
    fn own_windows_the_taskbar_and_hidden_windows_are_never_badged() {
        assert_eq!(judge(&w(1, "byte", "Error", DIALOG_CLASS)), Strength::No);
        assert_eq!(judge(&w(1, "explorer", "Error", "Shell_TrayWnd")), Strength::No);
        let mut hidden = w(1, "app", "Error", DIALOG_CLASS);
        hidden.minimised = true;
        assert_eq!(judge(&hidden), Strength::No);
        let mut tool = w(1, "app", "Error", DIALOG_CLASS);
        tool.tool = true;
        assert_eq!(judge(&tool), Strength::No);
    }

    #[test]
    fn only_a_window_that_is_new_since_the_last_look_is_announced() {
        let mut seen = HashSet::new();
        let old = vec![w(1, "app", "Error", DIALOG_CLASS)];
        // The first look only learns what is already open.
        assert!(newly(&mut seen, &old, true, &[]).is_empty());
        assert!(newly(&mut seen, &old, false, &[]).is_empty(), "the same window is not new again");
        let mut now = old.clone();
        now.push(w(2, "setup", "Setup Failed", DIALOG_CLASS));
        now.push(w(3, "app", "Something happened", DIALOG_CLASS));
        let found = newly(&mut seen, &now, false, &[]);
        assert_eq!(found.iter().map(|(x, s)| (x.id, *s)).collect::<Vec<_>>(), vec![(2, Strength::Strong), (3, Strength::Weak)]);
        // A window that closed and a new one with a new handle.
        let found = newly(&mut seen, &[w(4, "app", "Error", DIALOG_CLASS)], false, &[]);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn a_window_on_the_never_look_at_list_is_not_announced() {
        let mut seen = HashSet::new();
        newly(&mut seen, &[], true, &[]);
        let list = vec![w(5, "1Password", "Error", DIALOG_CLASS), w(6, "msedge", "InPrivate - Error", DIALOG_CLASS)];
        assert!(newly(&mut seen, &list, false, &policy::default_excludes()).is_empty());
    }

    #[test]
    fn the_words_are_tidied_and_wrapped_as_content() {
        let raw = vec!["  The app\n couldn't  start  ".to_string(), "".to_string(), "The app couldn't start".to_string(), "Code 0xc000007b".to_string()];
        assert_eq!(tidy(&raw, 10), vec!["The app couldn't start".to_string(), "Code 0xc000007b".to_string()]);
        assert_eq!(tidy(&raw, 1).len(), 1);
        let d = DialogText { title: "Error".into(), app: "app".into(), lines: tidy(&raw, 10), buttons: vec!["OK".into(), "Cancel".into()] };
        let p = prompt(&d);
        assert!(p.contains("<<<\nWindow title: Error") && p.contains("Buttons: OK | Cancel") && p.ends_with(">>>"));
    }

    #[test]
    fn the_tray_badge_is_a_red_dot_in_the_corner_only() {
        let (w, h) = (32usize, 32usize);
        let mut px = vec![0u8; w * h * 4];
        badge_pixels(&mut px, w, h);
        let at = |x: usize, y: usize| &px[(y * w + x) * 4..(y * w + x) * 4 + 4];
        assert_eq!(at(26, 6), &[230, 40, 40, 255]);
        assert_eq!(at(2, 30), &[0, 0, 0, 0]);
        assert_eq!(at(2, 2), &[0, 0, 0, 0]);
    }
}
