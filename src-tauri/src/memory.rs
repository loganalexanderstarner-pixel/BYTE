//! What's using memory right now, per app, so when a model won't load BYTE
//! can say "Google Chrome is using 2.4 GB, Slack 0.9 GB — quit them to make
//! room" instead of a vague "close other apps". Only regular apps are listed
//! (things in a `.app` bundle outside /System); macOS itself and BYTE are not.

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
    let mut apps: HashMap<String, AppMemory> = HashMap::new();
    for (exe, bytes) in processes {
        if let Some(name) = app_of(&exe) {
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
    let me = sysinfo::get_current_pid().ok();
    let processes = sys.processes().iter().filter(|(pid, _)| Some(**pid) != me).filter_map(|(_, p)| {
        let exe = p.exe()?.to_string_lossy().into_owned();
        Some((exe, p.memory()))
    });
    MemoryReport { total_bytes: sys.total_memory(), available_bytes: sys.available_memory(), apps: group_by_app(processes, 150 * 1_000_000, 8) }
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
    #[cfg(not(target_os = "macos"))]
    {
        Err(AppError::msg("closing apps is only available on macOS"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
