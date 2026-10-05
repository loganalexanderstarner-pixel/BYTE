//! What's using memory right now, per app, so when a model won't load BYTE
//! can say "Google Chrome is using 2.4 GB, Slack 0.9 GB — quit them to make
//! room" instead of a vague "close other apps". Only regular apps are listed:
//! on a Mac things in a `.app` bundle outside /System, on Windows programs that
//! have a window on the desktop (not services, and not Windows itself); BYTE is not.

use std::collections::HashMap;

use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppMemory {
    /// The app's name as macOS shows it ("Google Chrome").
    pub name: String,
    /// Memory in use by the app and all its helper processes.
    pub bytes: u64,
    pub processes: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryReport {
    pub total_bytes: u64,
    /// Memory macOS can hand out right now without swapping.
    pub available_bytes: u64,
    /// Apps using the most memory, biggest first (only ones worth closing).
    pub apps: Vec<AppMemory>,
}

/// Parts of macOS that run as apps but shouldn't be quit.
const SYSTEM_APPS: &[&str] = &[
    "Finder", "Dock", "SystemUIServer", "WindowServer", "loginwindow", "Control Center", "ControlCenter",
    "Notification Center", "NotificationCenter", "Spotlight", "Siri", "TextInputMenuAgent", "universalAccessAuthWarn",
    "BYTE", "byte",
];

/// The app a process belongs to: the outermost `.app` bundle in its path
/// (Chrome's helpers live inside "Google Chrome.app"). None for daemons,
/// macOS itself and BYTE.
pub fn app_of(exe_path: &str) -> Option<String> {
    if exe_path.starts_with("/System/") || exe_path.starts_with("/usr/") || exe_path.starts_with("/sbin/") || exe_path.starts_with("/Library/Apple/") {
        return None;
    }
    let bundle = exe_path.split('/').find(|part| part.ends_with(".app"))?;
    let name = bundle.trim_end_matches(".app").to_string();
    (!name.is_empty() && !SYSTEM_APPS.iter().any(|s| s.eq_ignore_ascii_case(&name))).then_some(name)
}

/// Sums memory per app, keeps the ones over `min_bytes`, biggest first.
pub fn group_by_app(processes: impl IntoIterator<Item = (String, u64)>, min_bytes: u64, limit: usize) -> Vec<AppMemory> {
    group_with(processes, min_bytes, limit, app_of)
}

/// As `group_by_app`, with the platform's way of naming the app a process belongs to.
pub fn group_with(processes: impl IntoIterator<Item = (String, u64)>, min_bytes: u64, limit: usize, mut name_of: impl FnMut(&str) -> Option<String>) -> Vec<AppMemory> {
    let mut apps: HashMap<String, AppMemory> = HashMap::new();
    for (exe, bytes) in processes {
        if let Some(name) = name_of(&exe) {
            let e = apps.entry(name.clone()).or_insert(AppMemory { name, bytes: 0, processes: 0 });
            e.bytes += bytes;
            e.processes += 1;
        }
    }
    let mut out: Vec<AppMemory> = apps.into_values().filter(|a| a.bytes >= min_bytes).collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    out.truncate(limit);
    out
}

/// A snapshot of memory use (apps using 150 MB or more, top 8).
pub fn report() -> MemoryReport {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    #[cfg(windows)]
    let apps = win::apps(&sys);
    #[cfg(not(windows))]
    let apps = {
        let me = sysinfo::get_current_pid().ok();
        let processes = sys.processes().iter().filter(|(pid, _)| Some(**pid) != me).filter_map(|(_, p)| {
            let exe = p.exe()?.to_string_lossy().into_owned();
            Some((exe, p.memory()))
        });
        group_by_app(processes, 150 * 1_000_000, 8)
    };
    MemoryReport { total_bytes: sys.total_memory(), available_bytes: sys.available_memory(), apps }
}

/// Programs that show a window but aren't apps to offer closing: the shell, Windows' own
/// UI hosts, and the web view BYTE itself draws with. Lower case, without ".exe".
const WINDOWS_SKIP: &[&str] = &[
    "explorer", "applicationframehost", "textinputhost", "searchhost", "startmenuexperiencehost",
    "shellexperiencehost", "systemsettings", "lockapp", "widgets", "ctfmon", "dwm", "msedgewebview2", "byte",
];

/// The name to show for a Windows program, from the description its file carries
/// ("Google Chrome") or else its file name ("Chrome"). None for Windows itself and BYTE.
pub fn windows_app_name(exe_path: &str, description: Option<&str>) -> Option<String> {
    let path = exe_path.replace('/', "\\").to_lowercase();
    if path.contains("\\windows\\") {
        return None;
    }
    let file = path.rsplit('\\').next().unwrap_or(&path);
    let stem = file.strip_suffix(".exe").unwrap_or(file);
    if stem.is_empty() || WINDOWS_SKIP.contains(&stem) {
        return None;
    }
    let shown = description.map(str::trim).filter(|d| !d.is_empty()).map(str::to_string).unwrap_or_else(|| {
        let original = exe_path.rsplit(['\\', '/']).next().unwrap_or(exe_path);
        let base = original.strip_suffix(".exe").or_else(|| original.strip_suffix(".EXE")).unwrap_or(original);
        let mut c = base.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    });
    (!shown.is_empty() && !shown.eq_ignore_ascii_case("byte")).then_some(shown)
}

/// One line for error messages: "Using the most memory now: Google Chrome
/// (2.4 GB), Slack (0.9 GB). Quitting them frees about 3.3 GB."
pub fn advice(report: &MemoryReport) -> String {
    let apps: Vec<&AppMemory> = report.apps.iter().take(4).collect();
    if apps.is_empty() {
        return String::new();
    }
    let list = apps.iter().map(|a| format!("{} ({:.1} GB)", a.name, a.bytes as f64 / 1e9)).collect::<Vec<_>>().join(", ");
    let total: u64 = apps.iter().map(|a| a.bytes).sum();
    format!(" Using the most memory right now: {list}. Quitting them frees about {:.1} GB.", total as f64 / 1e9)
}

/// Asks an app to quit the normal way (it can still ask to save documents).
/// Only apps currently in the memory report can be quit.
pub fn quit_app(name: &str) -> AppResult<()> {
    let listed = report().apps.iter().any(|a| a.name == name);
    if !listed {
        return Err(AppError::msg(format!("{name} isn't one of the apps BYTE can close")));
    }
    #[cfg(target_os = "macos")]
    {
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        let out = std::process::Command::new("osascript").args(["-e", &format!("tell application \"{escaped}\" to quit")]).output()?;
        if !out.status.success() {
            return Err(AppError::msg(format!("{name} didn't quit: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        match win::close(&sys, name) {
            0 => Err(AppError::msg(format!("{name} has no open window BYTE can close"))),
            _ => Ok(()),
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Err(AppError::msg("closing apps isn't available on this system yet"))
    }
}

#[cfg(windows)]
mod win {
    //! Which programs have a window on the desktop, what they are called, and a polite close
    //! (the same message the window's own X button sends, so an app can still ask to save).

    use std::collections::{HashMap, HashSet};

    use sysinfo::{Pid, System};
    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE};

    use super::{group_with, windows_app_name, AppMemory};

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the address of the Vec `visible_windows` passed, alive for the whole call.
        let out = unsafe { &mut *(lparam.0 as *mut Vec<(HWND, u32)>) };
        if unsafe { IsWindowVisible(hwnd) }.as_bool() && unsafe { GetWindowTextLengthW(hwnd) } > 0 {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != 0 {
                out.push((hwnd, pid));
            }
        }
        BOOL::from(true)
    }

    /// Top-level windows a person can see (visible, with a title), with their process.
    fn visible_windows() -> Vec<(HWND, u32)> {
        let mut found: Vec<(HWND, u32)> = Vec::new();
        // SAFETY: the callback only touches `found`, which outlives the call.
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut found as *mut _ as isize));
        }
        found
    }

    /// "FileDescription" from a program's version resource: what Task Manager shows.
    fn description(exe: &str) -> Option<String> {
        let path: Vec<u16> = exe.encode_utf16().chain([0]).collect();
        // SAFETY: every pointer is to a live, NUL-terminated or correctly sized buffer.
        unsafe {
            let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;
            let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len = 0u32;
            let table: Vec<u16> = "\\VarFileInfo\\Translation".encode_utf16().chain([0]).collect();
            // Language and code page of the first translation; English (Unicode) when there is none.
            let lang = if VerQueryValueW(data.as_ptr().cast(), PCWSTR(table.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 4 {
                let words = ptr as *const u16;
                format!("{:04x}{:04x}", *words, *words.add(1))
            } else {
                "040904b0".to_string()
            };
            let key: Vec<u16> = format!("\\StringFileInfo\\{lang}\\FileDescription").encode_utf16().chain([0]).collect();
            if !VerQueryValueW(data.as_ptr().cast(), PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
                return None;
            }
            let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
            // The string ends at its first NUL; what follows is other data in the resource.
            let text = String::from_utf16_lossy(text);
            Some(text.split('\0').next().unwrap_or_default().trim().to_string()).filter(|t| !t.is_empty())
        }
    }

    fn exe_of(sys: &System, pid: u32) -> Option<String> {
        sys.process(Pid::from_u32(pid))?.exe().map(|e| e.to_string_lossy().into_owned())
    }

    /// Apps using the most memory: programs with a window, all their processes summed.
    pub fn apps(sys: &System) -> Vec<AppMemory> {
        let me = sysinfo::get_current_pid().ok();
        let windowed: HashSet<String> = visible_windows().iter().filter_map(|(_, pid)| exe_of(sys, *pid)).map(|e| e.to_lowercase()).collect();
        let processes = sys.processes().iter().filter(|(pid, _)| Some(**pid) != me).filter_map(|(_, p)| {
            let exe = p.exe()?.to_string_lossy().into_owned();
            windowed.contains(&exe.to_lowercase()).then(|| (exe, p.memory()))
        });
        let mut names: HashMap<String, Option<String>> = HashMap::new();
        group_with(processes, 150 * 1_000_000, 8, |exe| names.entry(exe.to_lowercase()).or_insert_with(|| windows_app_name(exe, description(exe).as_deref())).clone())
    }

    /// Asks every window of the app called `name` to close. Returns how many it asked.
    pub fn close(sys: &System, name: &str) -> usize {
        let mut names: HashMap<String, Option<String>> = HashMap::new();
        let mut asked = 0;
        for (hwnd, pid) in visible_windows() {
            let Some(exe) = exe_of(sys, pid) else { continue };
            let shown = names.entry(exe.to_lowercase()).or_insert_with(|| windows_app_name(&exe, description(&exe).as_deref())).clone();
            if shown.as_deref() == Some(name) {
                // SAFETY: posting a message to a window handle is always sound; a stale handle just fails.
                if unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok() {
                    asked += 1;
                }
            }
        }
        asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real desktop, so it needs a logged-in session with windows open (over SSH there are none).
    #[cfg(windows)]
    #[test]
    #[ignore = "needs a desktop session with windows open; run with --ignored"]
    fn lists_the_real_apps_on_this_desktop() {
        let r = report();
        for a in &r.apps {
            eprintln!("APP {:<34} {:>6.0} MB in {} processes", a.name, a.bytes as f64 / 1e6, a.processes);
        }
        assert!(!r.apps.is_empty(), "a desktop with windows open has at least one app over 150 MB");
        assert!(r.apps.iter().all(|a| !a.name.eq_ignore_ascii_case("explorer") && !a.name.eq_ignore_ascii_case("byte")));
    }


    #[test]
    fn windows_programs_are_named_the_way_task_manager_names_them() {
        // The file's own description wins ("Google Chrome"), the file name is the fallback.
        assert_eq!(windows_app_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe", Some("Google Chrome")).as_deref(), Some("Google Chrome"));
        assert_eq!(windows_app_name(r"C:\Users\sam\AppData\Local\Discord\app\Discord.exe", None).as_deref(), Some("Discord"));
        assert_eq!(windows_app_name(r"D:\tools\obsidian.exe", Some("  ")).as_deref(), Some("Obsidian"));
        // Forward slashes and any case are the same program.
        assert_eq!(windows_app_name("C:/Apps/Slack.EXE", Some("Slack")).as_deref(), Some("Slack"));
    }

    #[test]
    fn windows_itself_and_byte_are_never_offered_for_closing() {
        for exe in [
            r"C:\Windows\explorer.exe",
            r"C:\Windows\System32\ApplicationFrameHost.exe",
            r"C:\Program Files\App\byte.exe",
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\msedgewebview2.exe",
            r"C:\Users\sam\explorer.exe",
        ] {
            assert_eq!(windows_app_name(exe, Some("Whatever it says")), None, "{exe}");
        }
        // Even a file that describes itself as BYTE.
        assert_eq!(windows_app_name(r"C:\x\other.exe", Some("BYTE")), None);
    }

    #[test]
    fn windows_processes_of_one_app_add_up() {
        let chrome = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
        let apps = group_with(
            vec![(chrome.to_string(), 900_000_000), (chrome.to_string(), 700_000_000), (r"C:\Windows\explorer.exe".to_string(), 400_000_000), (r"C:\a\Tiny.exe".to_string(), 10_000_000)],
            150_000_000,
            8,
            |exe| windows_app_name(exe, if exe.ends_with("chrome.exe") { Some("Google Chrome") } else { None }),
        );
        assert_eq!(apps.len(), 1, "{apps:?}");
        assert_eq!((apps[0].name.as_str(), apps[0].bytes, apps[0].processes), ("Google Chrome", 1_600_000_000, 2));
    }


    #[test]
    fn processes_belong_to_their_outermost_app() {
        assert_eq!(app_of("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome").as_deref(), Some("Google Chrome"));
        assert_eq!(
            app_of("/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)").as_deref(),
            Some("Google Chrome")
        );
        assert_eq!(app_of("/Users/me/Applications/Slack.app/Contents/MacOS/Slack").as_deref(), Some("Slack"));
        // macOS itself, daemons and BYTE aren't offered.
        assert_eq!(app_of("/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder"), None);
        assert_eq!(app_of("/usr/libexec/trustd"), None);
        assert_eq!(app_of("/Applications/BYTE.app/Contents/MacOS/llama-server"), None);
        assert_eq!(app_of("/opt/homebrew/bin/node"), None);
    }

    #[test]
    fn groups_helpers_and_lists_the_biggest_first() {
        let procs = vec![
            ("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string(), 400_000_000),
            ("/Applications/Google Chrome.app/Contents/Frameworks/X.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/H".to_string(), 1_600_000_000),
            ("/Applications/Slack.app/Contents/MacOS/Slack".to_string(), 900_000_000),
            ("/Applications/Tiny.app/Contents/MacOS/Tiny".to_string(), 20_000_000),
            ("/usr/libexec/trustd".to_string(), 5_000_000_000),
        ];
        let apps = group_by_app(procs, 150_000_000, 8);
        assert_eq!(apps.iter().map(|a| (a.name.as_str(), a.bytes, a.processes)).collect::<Vec<_>>(), vec![("Google Chrome", 2_000_000_000, 2), ("Slack", 900_000_000, 1)]);
        let text = advice(&MemoryReport { total_bytes: 16_000_000_000, available_bytes: 2_000_000_000, apps });
        assert!(text.contains("Google Chrome (2.0 GB), Slack (0.9 GB)") && text.contains("about 2.9 GB"), "{text}");
        assert_eq!(advice(&MemoryReport { total_bytes: 1, available_bytes: 1, apps: vec![] }), "");
    }

    #[test]
    fn only_listed_apps_can_be_quit() {
        assert!(quit_app("Definitely Not Running 12345").is_err());
    }
}
