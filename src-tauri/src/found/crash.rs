//! Crash recovery: a marker file is written when BYTE starts and removed when it exits cleanly. If it is still there at the next
//! start, the last run ended without a clean exit, and BYTE offers a problem report. A panic hook writes the panic text to the
//! log and to `crash-last.txt` so the report can say what happened.
//!
//! A restart that BYTE itself makes (an update, a restore, safe mode) calls [`clean_exit`] first. A shutdown or reboot of the
//! computer is not a crash: a marker older than the computer's last boot is ignored.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

const MARKER: &str = "running.marker";
const PANIC_FILE: &str = "crash-last.txt";
/// The most panic text kept.
const PANIC_MAX: usize = 6000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub version: String,
    pub pid: u32,
    /// Unix seconds when that run started.
    pub started_at: u64,
}

/// What the start found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// No marker: the last run exited cleanly (or this is the first run).
    Clean,
    /// A marker older than the computer's last boot: the computer was shut down or restarted, not BYTE.
    Rebooted,
    /// A marker from this boot: the last run did not exit cleanly.
    Unclean(Marker),
}

/// Pure: what a marker found at start means. `boot` is the computer's boot time in unix seconds (0 when unknown).
pub fn decide(found: Option<Marker>, boot: u64) -> Verdict {
    match found {
        None => Verdict::Clean,
        Some(m) if boot > 0 && m.started_at < boot => Verdict::Rebooted,
        Some(m) => Verdict::Unclean(m),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Reads the marker left by the last run, if any (an unreadable one counts as present: the run did not finish).
pub fn read_marker(root: &Path) -> Option<Marker> {
    let text = std::fs::read_to_string(root.join(MARKER)).ok()?;
    Some(serde_json::from_str(&text).unwrap_or(Marker { version: String::new(), pid: 0, started_at: u64::MAX }))
}

/// Writes this run's marker. A failure only means a crash will not be noticed; it never stops BYTE.
pub fn write_marker(root: &Path, version: &str, started_at: u64) {
    let m = Marker { version: version.into(), pid: std::process::id(), started_at };
    if let Ok(text) = serde_json::to_string(&m) {
        let _ = std::fs::write(root.join(MARKER), text);
    }
}

pub fn remove_marker(root: &Path) {
    let _ = std::fs::remove_file(root.join(MARKER));
}

/// What the last unclean exit left, for the card.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// Version of BYTE that was running.
    pub version: String,
    /// The text of a panic, if one was written.
    pub panic: Option<String>,
}

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static FOUND: Mutex<Option<Found>> = Mutex::new(None);

fn boot_time() -> u64 {
    sysinfo::System::boot_time()
}

/// Called early at startup: notes whether the last run ended badly, then marks this run as started.
pub fn begin(root: &Path) {
    let _ = ROOT.set(root.to_path_buf());
    let version = env!("CARGO_PKG_VERSION");
    if let Verdict::Unclean(m) = decide(read_marker(root), boot_time()) {
        let panic = std::fs::read_to_string(root.join(PANIC_FILE)).ok().filter(|t| !t.trim().is_empty());
        log::warn!("the last run of BYTE {} did not exit cleanly", m.version);
        *FOUND.lock().unwrap_or_else(|e| e.into_inner()) = Some(Found { version: m.version, panic });
    }
    // The panic note belongs to the run that wrote it.
    let _ = std::fs::remove_file(root.join(PANIC_FILE));
    write_marker(root, version, now_secs());
}

/// Called on a clean exit and before BYTE restarts itself.
pub fn clean_exit() {
    if let Some(root) = ROOT.get() {
        remove_marker(root);
    }
}

/// The text kept for a panic: the message, where it happened and a short backtrace, cut to a safe size.
pub fn panic_text(info: &std::panic::PanicHookInfo<'_>, backtrace: &str) -> String {
    let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "(no message)".into());
    let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_else(|| "(unknown place)".into());
    let mut t = format!("BYTE {} panicked at {at}: {msg}\n{backtrace}", env!("CARGO_PKG_VERSION"));
    if t.len() > PANIC_MAX {
        let mut cut = PANIC_MAX;
        while !t.is_char_boundary(cut) {
            cut -= 1;
        }
        t.truncate(cut);
    }
    t
}

/// Installs the panic hook (keeps the usual one running after it).
pub fn install_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = panic_text(info, &std::backtrace::Backtrace::force_capture().to_string());
        log::error!("{text}");
        log::logger().flush();
        if let Some(root) = ROOT.get() {
            let _ = std::fs::write(root.join(PANIC_FILE), &text);
        }
        previous(info);
    }));
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashStatus {
    pub found: Option<Found>,
}

/// The unclean exit found at this start, if any.
#[tauri::command]
pub fn crash_status() -> CrashStatus {
    CrashStatus { found: FOUND.lock().unwrap_or_else(|e| e.into_inner()).clone() }
}

/// The person looked at it or turned it down: it is not offered again.
#[tauri::command]
pub fn crash_dismiss() {
    *FOUND.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The usual problem report with the panic text added (scrubbed the same way).
pub fn add_panic(report: &mut String, panic: Option<&str>, id: &crate::problemreport::Identity) {
    if let Some(p) = panic {
        report.push_str("\nWhat BYTE wrote when it stopped:\n-----\n");
        report.push_str(&crate::problemreport::scrub(p, id));
        report.push('\n');
    }
}

#[tauri::command]
pub async fn crash_report_build(app: AppHandle, state: tauri::State<'_, AppState>) -> AppResult<crate::problemreport::Report> {
    let mut r = crate::problemreport::problem_report_build(app, state).await?;
    let found = FOUND.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or_else(|| AppError::msg("BYTE did not stop unexpectedly last time."))?;
    r.text = format!("(BYTE {} closed unexpectedly the last time it ran.)\n\n{}", found.version, r.text);
    add_panic(&mut r.text, found.panic.as_deref(), &crate::problemreport::Identity::this_pc());
    if found.panic.is_some() {
        r.includes.push("What BYTE wrote about the error when it stopped".into());
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(at: u64) -> Marker {
        Marker { version: "0.12.6".into(), pid: 7, started_at: at }
    }

    #[test]
    fn no_marker_means_a_clean_exit() {
        assert_eq!(decide(None, 1000), Verdict::Clean);
    }

    #[test]
    fn a_marker_from_this_boot_means_a_crash() {
        assert_eq!(decide(Some(marker(2000)), 1000), Verdict::Unclean(marker(2000)));
        assert_eq!(decide(Some(marker(2000)), 0), Verdict::Unclean(marker(2000)), "unknown boot time still reports it");
    }

    #[test]
    fn a_marker_from_before_the_boot_is_a_shutdown_not_a_crash() {
        assert_eq!(decide(Some(marker(500)), 1000), Verdict::Rebooted);
    }

    #[test]
    fn the_marker_is_written_read_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_marker(dir.path()).is_none());
        write_marker(dir.path(), "1.2.3", 42);
        let m = read_marker(dir.path()).unwrap();
        assert_eq!((m.version.as_str(), m.started_at, m.pid), ("1.2.3", 42, std::process::id()));
        remove_marker(dir.path());
        assert!(read_marker(dir.path()).is_none());
        remove_marker(dir.path()); // removing twice is harmless
    }

    #[test]
    fn a_garbled_marker_still_counts_as_an_unclean_exit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MARKER), "{half").unwrap();
        assert!(matches!(decide(read_marker(dir.path()), 1000), Verdict::Unclean(_)));
    }

    #[test]
    fn panic_text_names_the_place_and_message_and_stays_short() {
        let seen = std::sync::Arc::new(Mutex::new(String::new()));
        let seen2 = seen.clone();
        let old = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            *seen2.lock().unwrap() = panic_text(info, &"frame\n".repeat(5000));
        }));
        let _ = std::panic::catch_unwind(|| panic!("the model went missing"));
        std::panic::set_hook(old);
        let t = seen.lock().unwrap().clone();
        assert!(t.contains("panicked at") && t.contains("the model went missing") && t.contains("crash.rs"));
        assert!(t.len() <= PANIC_MAX);
    }

    #[test]
    fn the_panic_text_is_scrubbed_in_the_report() {
        let id = crate::problemreport::Identity { user: "Jordan".into(), home: "C:\\Users\\Jordan".into(), host: "PC1".into() };
        let mut r = String::from("report\n");
        add_panic(&mut r, Some("panicked at C:\\Users\\Jordan\\x.rs:1: boom"), &id);
        assert!(r.contains("boom") && !r.contains("Jordan"));
        let mut r2 = String::from("report\n");
        add_panic(&mut r2, None, &id);
        assert_eq!(r2, "report\n");
    }
}
