//! Explorer right-click "Ask BYTE" (Windows): a per-user entry on every file's right-click menu that opens the file in a new BYTE chat.
//! It lives under HKCU\Software\Classes\*\shell\AskBYTE (no administrator needed, nothing machine-wide), is added when the switch is
//! turned on and removed when it is turned off.
//!
//! How a click reaches BYTE: Explorer starts `byte.exe --ask "<file>"`. That process writes the path into a small inbox folder and, if
//! BYTE is already running, quits at once; the running BYTE picks the file up within a second and shows it attached in a new chat.
//! If BYTE was not running, this process carries on and starts normally, then picks the file up itself. Nothing is sent anywhere:
//! the file only waits attached in the chat box until the person sends it. (Windows tray icons cannot take dropped files or text, so
//! "drop on the tray icon" is not possible; drop files on the BYTE window instead.)

use std::path::{Path, PathBuf};

pub const FLAG: &str = "--ask";
pub const VERB_KEY: &str = r"HKCU\Software\Classes\*\shell\AskBYTE";
/// Event sent to the window with the path.
pub const EVENT: &str = "helpers://ask-file";

pub fn supported() -> bool {
    cfg!(windows)
}

/// The path after `--ask`, if the command line has one that is a plain file path (not an option).
pub fn parse_ask_arg(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == FLAG)?;
    let p = args.get(i + 1)?.trim();
    if p.is_empty() || p.starts_with('-') || p.len() > 1000 || p.contains(['\0', '\n', '\r']) {
        return None;
    }
    Some(p.to_string())
}

/// The `reg.exe` calls that add the verb for the program at `exe`.
pub fn add_commands(exe: &str) -> Result<Vec<Vec<String>>, String> {
    if exe.contains('"') || exe.contains('%') || exe.is_empty() {
        return Err("BYTE's own path has a character the menu entry can't carry.".into());
    }
    let cmd_key = format!(r"{VERB_KEY}\command");
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
    Ok(vec![
        s(&["add", VERB_KEY, "/ve", "/d", "Ask BYTE", "/f"]),
        vec!["add".into(), VERB_KEY.into(), "/v".into(), "Icon".into(), "/d".into(), format!("\"{exe}\""), "/f".into()],
        vec!["add".into(), cmd_key, "/ve".into(), "/d".into(), format!("\"{exe}\" {FLAG} \"%1\""), "/f".into()],
    ])
}

pub fn remove_command() -> Vec<String> {
    ["delete", VERB_KEY, "/f"].iter().map(|x| x.to_string()).collect()
}

/// Where the paths wait: %LOCALAPPDATA%\BYTE\ask-inbox (the same for every BYTE process of this person).
pub fn inbox_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("XDG_DATA_HOME")).map(PathBuf::from)?;
    Some(base.join("BYTE").join("ask-inbox"))
}

pub fn write_inbox(dir: &Path, file: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let name = format!("{:020}-{}-{:04}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0), std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    std::fs::write(dir.join(name), file)
}

/// The waiting paths (oldest first); each is removed as it is read. Only plain existing files are returned.
pub fn take_inbox(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect();
    entries.sort();
    let mut out = Vec::new();
    for p in entries.into_iter().take(20) {
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let _ = std::fs::remove_file(&p);
        let f = text.trim();
        if !f.is_empty() && f.len() <= 1000 && Path::new(f).is_file() {
            out.push(f.to_string());
        }
    }
    out
}

/// At the very start of the program: a launch from the right-click menu while BYTE is already running hands the file over and ends.
/// Returns true when this process should quit. Also marks this process as the running BYTE.
pub fn handoff_at_start() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let file = parse_ask_arg(&args);
    let first = win::claim();
    if let (Some(f), Some(dir)) = (file, inbox_dir()) {
        let _ = write_inbox(&dir, &f);
        return !first;
    }
    false
}

#[cfg(windows)]
mod win {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    /// Takes the "BYTE is running" marker; false when another BYTE of this person already holds it. The marker lasts until the process ends.
    pub fn claim() -> bool {
        match unsafe { CreateMutexW(None, false, w!("Local\\BYTE-is-running")) } {
            Ok(h) => {
                let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
                // The handle is kept open on purpose until the process exits.
                std::mem::forget(h);
                !existed
            }
            Err(_) => true,
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn claim() -> bool {
        true
    }
}

#[cfg(windows)]
fn reg(args: &[String]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg.exe").args(args).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

#[cfg(not(windows))]
fn reg(_args: &[String]) -> Result<(), String> {
    Err("this needs Windows".into())
}

/// Adds or removes the right-click entry to match the switch.
pub fn apply_verb(on: bool) -> Result<(), String> {
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?.display().to_string();
        for c in add_commands(&exe)? {
            reg(&c)?;
        }
        Ok(())
    } else {
        // Deleting a key that is not there is not a problem.
        let _ = reg(&remove_command());
        Ok(())
    }
}

#[cfg(desktop)]
mod watch {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::{AppHandle, Emitter, Manager};

    static RUNNING: AtomicBool = AtomicBool::new(false);
    static VERB: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None);

    pub fn apply(app: &AppHandle, on: bool) {
        if supported() {
            let mut last = VERB.lock().ok();
            if last.as_deref().copied().flatten() != Some(on) {
                if let Err(e) = apply_verb(on) {
                    log::warn!("Explorer menu entry: {e}");
                } else if let Some(l) = last.as_mut() {
                    **l = Some(on);
                }
            }
        }
        if !on {
            RUNNING.store(false, Ordering::SeqCst);
            // A file left in the inbox while the switch was off is not opened later.
            if let Some(d) = inbox_dir() {
                let _ = take_inbox(&d);
            }
            return;
        }
        if !supported() || RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while RUNNING.load(Ordering::SeqCst) {
                if let Some(dir) = inbox_dir() {
                    for f in take_inbox(&dir) {
                        if crate::helpers::blocked_now().is_some() {
                            continue;
                        }
                        crate::background::show_main(&app);
                        let _ = app.emit_to("main", EVENT, f);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                let _ = app.state::<crate::state::AppState>();
            }
        });
    }
}

#[cfg(desktop)]
pub fn apply(app: &tauri::AppHandle, on: bool) {
    watch::apply(app, on);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_file_after_the_flag_is_read_and_options_are_not_files() {
        assert_eq!(parse_ask_arg(&a(&["byte.exe", "--ask", r"C:\Users\me\a b.pdf"])), Some(r"C:\Users\me\a b.pdf".into()));
        assert_eq!(parse_ask_arg(&a(&["byte.exe"])), None);
        assert_eq!(parse_ask_arg(&a(&["byte.exe", "--ask"])), None);
        assert_eq!(parse_ask_arg(&a(&["byte.exe", "--ask", "--background"])), None);
        assert_eq!(parse_ask_arg(&a(&["byte.exe", "--ask", ""])), None);
        assert_eq!(parse_ask_arg(&a(&["byte.exe", "--background"])), None);
    }

    #[test]
    fn the_menu_entry_is_per_user_and_runs_only_byte_with_the_clicked_file() {
        let cmds = add_commands(r"C:\Program Files\BYTE\byte.exe").unwrap();
        assert_eq!(cmds.len(), 3);
        assert!(cmds.iter().all(|c| c[1].starts_with(r"HKCU\Software\Classes\*\shell\AskBYTE")));
        let last = cmds.last().unwrap();
        assert_eq!(last[4], "\"C:\\Program Files\\BYTE\\byte.exe\" --ask \"%1\"");
        assert!(add_commands("evil\" & calc \"").is_err());
        assert!(add_commands("100%").is_err());
        assert!(add_commands("").is_err());
        assert_eq!(remove_command(), a(&["delete", VERB_KEY, "/f"]));
    }

    #[test]
    fn the_inbox_hands_over_each_existing_file_once_and_empties() {
        let d = tempfile::tempdir().unwrap();
        let inbox = d.path().join("inbox");
        let file = d.path().join("report.pdf");
        std::fs::write(&file, "x").unwrap();
        write_inbox(&inbox, &file.display().to_string()).unwrap();
        write_inbox(&inbox, &d.path().join("gone.pdf").display().to_string()).unwrap();
        write_inbox(&inbox, "").unwrap();
        // A folder is not a file.
        write_inbox(&inbox, &d.path().display().to_string()).unwrap();
        let got = take_inbox(&inbox);
        assert_eq!(got, vec![file.display().to_string()]);
        assert!(take_inbox(&inbox).is_empty());
        assert!(take_inbox(&d.path().join("none")).is_empty());
    }

    #[test]
    fn other_systems_say_so() {
        if !cfg!(windows) {
            assert!(apply_verb(true).is_err());
            assert!(handoff_at_start() == false);
        }
    }
}
