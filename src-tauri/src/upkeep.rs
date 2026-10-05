//! Mac upkeep (Phase 10): what's using the disk, why the Mac is slow or the
//! battery drains, a health check, uninstalling apps and what opens at login.
//!
//! "what's taking up space", "free up space", "find duplicate files", "why is my
//! Mac slow", "what's draining my battery", "check my Mac", "uninstall Zoom",
//! "what opens at login", "stop Spotify from opening at login".
//!
//! Safety: looking changes nothing. Anything that removes something goes to the
//! Trash through Finder (so Finder's "Put Back" works too), only after the user
//! presses a button or OKs an approval card, and Undo puts it back. BYTE never
//! deletes for good, never empties the Trash, never touches macOS's own files or
//! apps, and only offers to remove what its own scan found (the Tauri commands
//! accept ids from the latest scan, never paths).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::{AppError, AppResult};
use crate::macctl::{self, Command, MacDone, MacRunner, Runner, Undo};
use crate::tools::SourceBook;

/// Longest a storage scan walks before showing what it has.
const SCAN_TIME: Duration = Duration::from_secs(20);
/// Most files and folders a scan looks at.
const SCAN_ENTRIES: u64 = 400_000;
/// BYTE's own bundle id: never uninstalled by itself.
const OWN_ID: &str = "com.loganstarner.byte";

/// Sizes that decide what's worth showing.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// A "big file".
    pub big: u64,
    /// Smallest file checked for copies.
    pub dup_min: u64,
    /// Untouched this long counts as old.
    pub old_days: u64,
    /// Installers in Downloads older than this are offered.
    pub installer_days: u64,
}

pub const LIMITS: Limits = Limits { big: 500 << 20, dup_min: 10 << 20, old_days: 365, installer_days: 30 };

// ------------------------------------------------------------------ scripts

/// Moves files to the Trash through Finder (so "Put Back" works). Prints one line
/// per file: where it went in the Trash, or "!" and why not.
const TRASH: &str = r#"on run argv
set out to ""
repeat with i from 1 to count argv
set f to POSIX file (item i of argv)
try
tell application "Finder" to set t to delete (f as alias)
set out to out & (POSIX path of (t as alias)) & linefeed
on error errText
set out to out & "!" & errText & linefeed
end try
end repeat
return out
end run"#;

/// Puts files back from the Trash: argv is (path in Trash, folder, name) triples.
const PUT_BACK: &str = r#"on run argv
set n to 0
repeat with i from 1 to (count argv) by 3
set wantName to item (i + 2) of argv
try
set src to (POSIX file (item i of argv)) as alias
set dst to (POSIX file (item (i + 1) of argv)) as alias
tell application "Finder"
set m to move src to dst
if (name of m) is not wantName then set name of m to wantName
end tell
set n to n + 1
end try
end repeat
return n as text
end run"#;

/// The apps with a window or Dock icon, one per line.
const FRONT_APPS: &str = r#"on run argv
tell application "System Events" to set appNames to name of every application process whose background only is false
set AppleScript's text item delimiters to linefeed
return appNames as text
end run"#;

/// Asks an app to quit (it can still ask to save).
const QUIT_APP: &str = r#"on run argv
tell application (item 1 of argv) to quit
return "ok"
end run"#;

/// Asks an app (by bundle id) to quit if it's running.
const QUIT_ID: &str = r#"on run argv
set bid to item 1 of argv
if application id bid is running then
tell application id bid to quit
end if
return "ok"
end run"#;

/// Login items: "name<tab>path" per line.
const LOGIN_LIST: &str = r#"on run argv
set out to ""
tell application "System Events"
repeat with li in login items
set out to out & (name of li) & tab & (path of li) & linefeed
end repeat
end tell
return out
end run"#;

/// Removes a login item by name; prints its path and whether it was hidden (for undo).
const LOGIN_REMOVE: &str = r#"on run argv
set wanted to item 1 of argv
tell application "System Events"
set p to path of login item wanted
set h to hidden of login item wanted
delete login item wanted
end tell
return p & tab & (h as text)
end run"#;

/// Adds a login item back (undo): path, hidden ("true"/"false").
const LOGIN_ADD: &str = r#"on run argv
tell application "System Events" to make login item at end with properties {path:(item 1 of argv), hidden:((item 2 of argv) is "true")}
return "ok"
end run"#;

#[cfg_attr(not(test), allow(dead_code))]
pub const SCRIPTS: &[&str] = &[TRASH, PUT_BACK, FRONT_APPS, QUIT_APP, QUIT_ID, LOGIN_LIST, LOGIN_REMOVE, LOGIN_ADD];

// ------------------------------------------------------------------ routing

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// What's using the disk, and what could go.
    Storage,
    /// Why it's slow / what drains the battery.
    Coach,
    /// A general health check.
    Checkup,
    Uninstall { app: String },
    LoginItems,
    LoginRemove { name: String },
}

fn clean(q: &str) -> String {
    let l = q.trim().to_lowercase().replace(['’', '‘'], "'");
    let l = l.trim_end_matches(['?', '.', '!']).trim().to_string();
    let mut s = l.as_str();
    for p in ["please ", "hey byte, ", "hey byte ", "byte, ", "can you ", "could you ", "would you "] {
        s = s.strip_prefix(p).unwrap_or(s);
    }
    s.to_string()
}

fn has_any(l: &str, words: &[&str]) -> bool {
    words.iter().any(|w| l.contains(w))
}

/// Strips filler around an app name ("the Zoom app from my Mac" → "zoom").
fn app_name(s: &str) -> String {
    let mut s = s.trim().to_string();
    for suffix in [" completely", " from my mac", " from this mac", " from the mac", " for me", " app", " application"] {
        if let Some(r) = s.strip_suffix(suffix) {
            s = r.trim().to_string();
        }
    }
    for prefix in ["the app ", "the application ", "the ", "app ", "my "] {
        if let Some(r) = s.strip_prefix(prefix) {
            s = r.trim().to_string();
        }
    }
    s.trim_matches(|c: char| c == '"' || c == '\'' || c == '“' || c == '”').trim().to_string()
}

pub fn ask(q: &str) -> Option<Ask> {
    let l = clean(q);
    if l.is_empty() || l.starts_with("what is ") || l.starts_with("what does ") || l.starts_with("explain ") || l.starts_with("why do ") {
        return None;
    }
    // Uninstalling.
    for p in ["uninstall ", "completely remove ", "fully remove ", "remove the app ", "delete the app ", "get rid of the app ", "remove app ", "delete app "] {
        if let Some(rest) = l.strip_prefix(p) {
            let app = app_name(rest);
            return (!app.is_empty() && app.len() <= 60).then_some(Ask::Uninstall { app });
        }
    }
    // Login items.
    let at_login = has_any(&l, &[" at login", " at startup", " when i log in", " when i start", " on startup", " on login", " at log in", "login item", "startup item"]);
    if at_login {
        for p in ["stop ", "don't open ", "dont open ", "don't launch ", "remove ", "disable "] {
            if let Some(rest) = l.strip_prefix(p) {
                let mut name = rest.to_string();
                for cut in [" from opening", " from launching", " from starting", " opening", " launching", " starting", " from login items", " from my login items", " from startup", " at login", " at startup", " when i log in", " on startup", " on login"] {
                    if let Some(i) = name.find(cut) {
                        name.truncate(i);
                    }
                }
                let name = app_name(&name);
                if !name.is_empty() && name != "apps" && name != "everything" && name.len() <= 60 {
                    return Some(Ask::LoginRemove { name });
                }
            }
        }
        if has_any(&l, &["what", "which", "list", "show", "see", "opens", "open ", "launch", "start"]) {
            return Some(Ask::LoginItems);
        }
    }
    // Storage.
    let space = has_any(&l, &["space", "storage", "disk", "hard drive", "ssd"]);
    if (space && has_any(&l, &["taking up", "using up", "free up", "running out", "running low", "low on", "is full", "almost full", "where did", "clean up", "cleanup", "analy", "what's using", "what is using", "eating", "hogging", "full"]))
        || has_any(&l, &["duplicate files", "find duplicates", "duplicate photos", "big files", "biggest files", "large files", "largest files", "huge files", "clean up my mac", "clean my mac", "cleanup my mac", "declutter my mac"])
    {
        return Some(Ask::Storage);
    }
    // Slow / battery.
    let mac = has_any(&l, &["mac", "computer", "laptop", "macbook", "my system"]);
    if (mac && has_any(&l, &["slow", "sluggish", "laggy", "lagging", "freezing", "beach ball", "running hot", "so hot", "overheating", "fan is loud", "fans are loud", "speed up", "faster"]))
        || has_any(&l, &["draining my battery", "drains my battery", "battery drain", "battery is draining", "battery dies", "battery life is", "battery health", "battery condition", "what's using my cpu", "what is using my cpu", "what's using my memory", "what's using my ram", "what is using my memory", "why is the fan", "keeps my mac awake", "won't sleep", "wont sleep"])
    {
        return Some(Ask::Coach);
    }
    if has_any(&l, &["check my mac", "check up on my mac", "checkup", "check-up", "health check", "is my mac ok", "is my mac okay", "is my mac healthy", "run diagnostics", "diagnose my mac", "mac's health", "mac health"]) {
        return Some(Ask::Checkup);
    }
    None
}

pub fn applies(enabled: bool, q: &str) -> bool {
    cfg!(target_os = "macos") && enabled && ask(q).is_some()
}

// ------------------------------------------------------------------ storage

/// Space a file takes on disk (a cloud file not downloaded takes none).
fn on_disk(m: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.blocks() * 512
    }
    #[cfg(not(unix))]
    {
        m.len()
    }
}

/// Folders that are really one document (removing a file inside breaks them).
const PACKAGES: &[&str] = &["app", "photoslibrary", "musiclibrary", "tvlibrary", "imovielibrary", "fcpbundle", "logicx", "band", "photolibrary", "aplibrary", "bundle", "framework", "pages", "numbers", "key", "xcodeproj", "xcworkspace", "sparsebundle", "lrlibrary", "lrdata"];

fn is_package(name: &str) -> bool {
    Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| PACKAGES.contains(&e.to_lowercase().as_str()))
}

/// A file that's fine to offer: not hidden, not inside a package or library.
fn offerable(home: &Path, p: &Path) -> bool {
    let Ok(rest) = p.strip_prefix(home) else { return false };
    let parts: Vec<&str> = rest.iter().filter_map(|c| c.to_str()).collect();
    if parts.first() == Some(&"Library") {
        return false;
    }
    let n = parts.len();
    parts.iter().enumerate().all(|(i, c)| !c.starts_with('.') && (i + 1 == n || !is_package(c)))
}

#[derive(Debug, Clone)]
pub struct FileRec {
    pub path: PathBuf,
    pub bytes: u64,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Default)]
pub struct Walk {
    pub bytes: u64,
    pub entries: u64,
    pub partial: bool,
}

/// Sizes a folder (no symlinks, skips `skip`), collecting files of at least
/// `keep_min` bytes into `keep`. Stops at the deadline or entry budget.
fn size_dir(dir: &Path, skip: &dyn Fn(&Path) -> bool, keep_min: u64, keep: &mut Vec<FileRec>, deadline: Instant, budget: &mut u64) -> Walk {
    let mut w = Walk::default();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            if *budget == 0 || Instant::now() > deadline {
                w.partial = true;
                return w;
            }
            *budget -= 1;
            w.entries += 1;
            let p = e.path();
            let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
            if m.file_type().is_symlink() {
                continue;
            }
            if m.is_dir() {
                if !skip(&p) {
                    stack.push(p);
                }
            } else {
                let b = on_disk(&m);
                w.bytes += b;
                if m.len() >= keep_min {
                    keep.push(FileRec { path: p, bytes: b, len: m.len(), modified: m.modified().ok() });
                }
            }
        }
    }
    w
}

/// Places in ~/Library that are safe to measure and to clear (tools rebuild them).
pub const DEV_CACHES: &[(&str, &str)] = &[
    ("Library/Developer/Xcode/DerivedData", "Xcode build files"),
    ("Library/Developer/Xcode/iOS DeviceSupport", "Xcode device support files"),
    ("Library/Developer/CoreSimulator/Caches", "Simulator caches"),
    ("Library/Caches/Homebrew", "Homebrew downloads"),
    ("Library/Caches/pip", "Python package downloads"),
    ("Library/Caches/Yarn", "Yarn package cache"),
    (".npm/_cacache", "npm package cache"),
    (".cache/pip", "Python package downloads"),
];

/// A folder and its size, for the chart.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sized {
    pub name: String,
    pub path: String,
    pub bytes: u64,
}

/// Something that could go, with why.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: String,
    pub title: String,
    pub why: String,
    pub bytes: u64,
    /// A few of the items, for display.
    pub items: Vec<String>,
    pub count: usize,
    /// BYTE can move these to the Trash (false: shown for information).
    pub can_trash: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BigFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub bytes: u64,
    /// Days since it last changed.
    pub days_old: Option<u64>,
}

/// The storage card.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub scan_id: String,
    /// The disk: total and free bytes (0 when unknown).
    pub total: u64,
    pub free: u64,
    pub folders: Vec<Sized>,
    pub suggestions: Vec<Suggestion>,
    pub big: Vec<BigFile>,
    /// The scan stopped early (a very full home folder); sizes are at least these.
    pub partial: bool,
}

/// What a scan found, before it becomes a card.
#[derive(Debug, Default)]
pub struct Scan {
    pub folders: Vec<(String, PathBuf, u64)>,
    pub files: Vec<FileRec>,
    pub caches: Vec<(String, PathBuf, u64)>,
    pub app_caches: u64,
    pub partial: bool,
}

/// Walks the home folder: the size of each top folder, big files, and the
/// developer caches. Doesn't look inside ~/Library (apart from caches) or the Trash.
pub fn scan(home: &Path, keep_min: u64, time: Duration, entries: u64) -> Scan {
    let deadline = Instant::now() + time;
    let mut budget = entries;
    let mut s = Scan::default();
    let lib = home.join("Library");
    let trash = home.join(".Trash");
    let cache_dirs: Vec<PathBuf> = DEV_CACHES.iter().map(|(p, _)| home.join(p)).collect();
    let skip = |p: &Path| p == lib || p == trash || cache_dirs.iter().any(|c| c == p);
    let mut hidden = 0u64;
    let mut tops: Vec<PathBuf> = std::fs::read_dir(home).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    tops.sort();
    for t in tops {
        let Ok(m) = std::fs::symlink_metadata(&t) else { continue };
        if !m.is_dir() || m.file_type().is_symlink() || skip(&t) {
            continue;
        }
        let name = t.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let w = size_dir(&t, &skip, keep_min, &mut s.files, deadline, &mut budget);
        s.partial |= w.partial;
        if name.starts_with('.') {
            hidden += w.bytes;
        } else {
            s.folders.push((name, t.clone(), w.bytes));
        }
    }
    // Only files BYTE may offer (not hidden, not inside a library or app) are kept.
    s.files.retain(|f| offerable(home, &f.path));
    if hidden > 0 {
        s.folders.push(("Hidden folders".into(), home.to_path_buf(), hidden));
    }
    // Caches have their own time, so a huge home folder can't hide them.
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut budget = entries;
    let mut none = Vec::new();
    for (i, (_, label)) in DEV_CACHES.iter().enumerate() {
        let w = size_dir(&cache_dirs[i], &|_| false, u64::MAX, &mut none, deadline, &mut budget);
        if w.bytes > 0 {
            s.caches.push((label.to_string(), cache_dirs[i].clone(), w.bytes));
        }
    }
    let caches = lib.join("Caches");
    let skip_dev = |p: &Path| cache_dirs.iter().any(|c| c == p);
    s.app_caches = size_dir(&caches, &skip_dev, u64::MAX, &mut none, deadline, &mut budget).bytes;
    s.folders.sort_by(|a, b| b.2.cmp(&a.2));
    s
}

fn days_since(t: Option<SystemTime>, now: SystemTime) -> Option<u64> {
    t.and_then(|t| now.duration_since(t).ok()).map(|d| d.as_secs() / 86_400)
}

/// Groups of files with the same contents (size, then the first 1 MB, then all of it).
pub fn duplicates(files: &[FileRec], min: u64, deadline: Instant) -> Vec<Vec<FileRec>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let hash = |p: &Path, limit: Option<u64>| -> Option<Vec<u8>> {
        let mut f = std::fs::File::open(p).ok()?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut left = limit.unwrap_or(u64::MAX);
        while left > 0 {
            let want = buf.len().min(left.min(usize::MAX as u64) as usize);
            let n = f.read(&mut buf[..want]).ok()?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            left -= n as u64;
        }
        Some(h.finalize().to_vec())
    };
    let mut by_len: HashMap<u64, Vec<&FileRec>> = HashMap::new();
    for f in files.iter().filter(|f| f.len >= min && f.bytes > 0) {
        by_len.entry(f.len).or_default().push(f);
    }
    let mut lens: Vec<u64> = by_len.iter().filter(|(_, v)| v.len() > 1).map(|(k, _)| *k).collect();
    lens.sort_unstable_by(|a, b| b.cmp(a));
    let mut out = Vec::new();
    for len in lens {
        if Instant::now() > deadline {
            break;
        }
        let mut by_head: HashMap<Vec<u8>, Vec<&FileRec>> = HashMap::new();
        for f in &by_len[&len] {
            if let Some(h) = hash(&f.path, Some(1 << 20)) {
                by_head.entry(h).or_default().push(f);
            }
        }
        for group in by_head.into_values().filter(|g| g.len() > 1) {
            let mut by_all: HashMap<Vec<u8>, Vec<&FileRec>> = HashMap::new();
            for f in group {
                let h = if len <= 1 << 20 { Some(vec![]) } else { hash(&f.path, None) };
                if let Some(h) = h {
                    by_all.entry(h).or_default().push(f);
                }
            }
            for g in by_all.into_values().filter(|g| g.len() > 1) {
                let mut g: Vec<FileRec> = g.into_iter().cloned().collect();
                // The oldest copy (then the shortest path) is the one kept.
                g.sort_by(|a, b| a.modified.cmp(&b.modified).then(a.path.as_os_str().len().cmp(&b.path.as_os_str().len())).then(a.path.cmp(&b.path)));
                out.push(g);
            }
        }
    }
    out
}

const INSTALLERS: &[&str] = &["dmg", "pkg", "mpkg"];

fn tilde(home: &Path, p: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

/// What the scan found, as a card plus the paths behind each id (what the
/// Trash buttons may remove).
pub fn storage_card(home: &Path, scan: &Scan, dups: &[Vec<FileRec>], total: u64, free: u64, lim: Limits, now: SystemTime) -> (Storage, HashMap<String, Vec<PathBuf>>) {
    let scan_id = uuid::Uuid::new_v4().simple().to_string();
    let mut allowed: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut suggestions = Vec::new();
    let show = |paths: &[PathBuf]| paths.iter().take(8).map(|p| tilde(home, p)).collect::<Vec<_>>();

    // Old installers in Downloads (top level only).
    let downloads = home.join("Downloads");
    let mut installers: Vec<&FileRec> = Vec::new();
    let installer_files: Vec<FileRec> = std::fs::read_dir(&downloads)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    let m = std::fs::symlink_metadata(&p).ok()?;
                    let ext = p.extension()?.to_str()?.to_lowercase();
                    (m.is_file() && INSTALLERS.contains(&ext.as_str())).then(|| FileRec { bytes: on_disk(&m), len: m.len(), modified: m.modified().ok(), path: p })
                })
                .collect()
        })
        .unwrap_or_default();
    for f in &installer_files {
        if days_since(f.modified, now).is_some_and(|d| d >= lim.installer_days) {
            installers.push(f);
        }
    }
    if !installers.is_empty() {
        let paths: Vec<PathBuf> = installers.iter().map(|f| f.path.clone()).collect();
        suggestions.push(Suggestion {
            id: "installers".into(),
            title: "Old installers in Downloads".into(),
            why: format!("Disk images and installers more than {} days old. The apps they installed stay installed.", lim.installer_days),
            bytes: installers.iter().map(|f| f.bytes).sum(),
            items: show(&paths),
            count: paths.len(),
            can_trash: true,
        });
        allowed.insert("installers".into(), paths);
    }

    // Copies: every file but the kept one.
    let extra: Vec<&FileRec> = dups.iter().flat_map(|g| g.iter().skip(1)).filter(|f| offerable(home, &f.path)).collect();
    if !extra.is_empty() {
        let paths: Vec<PathBuf> = extra.iter().map(|f| f.path.clone()).collect();
        let items = dups
            .iter()
            .filter(|g| g.iter().skip(1).any(|f| offerable(home, &f.path)))
            .take(8)
            .map(|g| format!("{} (keeps {})", g.iter().skip(1).map(|f| tilde(home, &f.path)).collect::<Vec<_>>().join(", "), tilde(home, &g[0].path)))
            .collect();
        suggestions.push(Suggestion {
            id: "duplicates".into(),
            title: "Duplicate files".into(),
            why: "Exact copies (the same contents, byte for byte). The oldest copy of each is kept.".into(),
            bytes: extra.iter().map(|f| f.bytes).sum(),
            items,
            count: paths.len(),
            can_trash: true,
        });
        allowed.insert("duplicates".into(), paths);
    }

    // Developer caches: their contents go, the folders stay.
    for (i, (label, dir, bytes)) in scan.caches.iter().enumerate() {
        if *bytes < 50 << 20 {
            continue;
        }
        let paths: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        if paths.is_empty() {
            continue;
        }
        let id = format!("cache-{i}");
        suggestions.push(Suggestion {
            id: id.clone(),
            title: label.clone(),
            why: format!("{} rebuilds or downloads these again when needed (the first build or install after may be slower).", if label.starts_with("Xcode") || label.starts_with("Simulator") { "Xcode" } else { "The tool" }),
            bytes: *bytes,
            items: vec![tilde(home, dir)],
            count: paths.len(),
            can_trash: true,
        });
        allowed.insert(id, paths);
    }

    if scan.app_caches >= 200 << 20 {
        suggestions.push(Suggestion {
            id: "app-caches".into(),
            title: "App caches".into(),
            why: "Apps keep these to load faster and refill them right away; macOS clears them itself when space runs low, so removing them by hand rarely helps.".into(),
            bytes: scan.app_caches,
            items: vec!["~/Library/Caches".into()],
            count: 1,
            can_trash: false,
        });
    }

    // Big files, biggest first; old ones get their own line too.
    let dup_extra: HashSet<&Path> = extra.iter().map(|f| f.path.as_path()).collect();
    let mut bigs: Vec<&FileRec> = scan.files.iter().filter(|f| f.bytes >= lim.big && offerable(home, &f.path)).collect();
    bigs.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.path.cmp(&b.path)));
    let big: Vec<BigFile> = bigs
        .iter()
        .take(25)
        .enumerate()
        .map(|(i, f)| {
            let id = format!("file-{i}");
            allowed.insert(id.clone(), vec![f.path.clone()]);
            BigFile { id, name: f.path.file_name().and_then(|n| n.to_str()).unwrap_or("").into(), path: tilde(home, &f.path), bytes: f.bytes, days_old: days_since(f.modified, now) }
        })
        .collect();
    let old: Vec<&&FileRec> = bigs.iter().filter(|f| days_since(f.modified, now).is_some_and(|d| d >= lim.old_days) && !dup_extra.contains(f.path.as_path())).collect();
    if !old.is_empty() {
        let paths: Vec<PathBuf> = old.iter().map(|f| f.path.clone()).collect();
        suggestions.push(Suggestion {
            id: "old-big".into(),
            title: "Big files not changed in a year".into(),
            why: "Worth a look: move them to an external drive or cloud storage, or remove the ones you don't need. BYTE doesn't remove these as a group; use the buttons on the list below.".into(),
            bytes: old.iter().map(|f| f.bytes).sum(),
            items: show(&paths),
            count: paths.len(),
            can_trash: false,
        });
    }

    let mut folders: Vec<Sized> = scan.folders.iter().filter(|f| f.2 > 0).map(|(n, p, b)| Sized { name: n.clone(), path: tilde(home, p), bytes: *b }).collect();
    if scan.app_caches > 0 {
        folders.push(Sized { name: "App caches".into(), path: "~/Library/Caches".into(), bytes: scan.app_caches });
    }
    for (label, dir, bytes) in &scan.caches {
        folders.push(Sized { name: label.clone(), path: tilde(home, dir), bytes: *bytes });
    }
    folders.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    folders.truncate(14);
    suggestions.sort_by(|a, b| b.can_trash.cmp(&a.can_trash).then(b.bytes.cmp(&a.bytes)));
    (Storage { scan_id, total, free, folders, suggestions, big, partial: scan.partial }, allowed)
}

/// `df -k` output → (total, free) bytes.
pub fn parse_df(out: &str) -> Option<(u64, u64)> {
    let line = out.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    let total: u64 = cols.get(1)?.parse().ok()?;
    let free: u64 = cols.get(3)?.parse().ok()?;
    Some((total * 1024, free * 1024))
}

/// Sizes like "1.2 GB" (decimal, like Finder).
pub fn size_text(b: u64) -> String {
    let b = b as f64;
    if b >= 1e12 {
        format!("{:.1} TB", b / 1e12)
    } else if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{b:.0} bytes")
    }
}

/// What the Trash buttons may remove, per scan (only the latest few are kept).
static SCANS: Lazy<Mutex<Vec<(String, HashMap<String, Vec<PathBuf>>)>>> = Lazy::new(|| Mutex::new(Vec::new()));

fn remember_scan(id: &str, allowed: HashMap<String, Vec<PathBuf>>) {
    if let Ok(mut s) = SCANS.lock() {
        s.push((id.to_string(), allowed));
        let n = s.len();
        if n > 5 {
            s.drain(..n - 5);
        }
    }
}

/// Takes the paths for an id out of a scan (each can be trashed once).
fn take_allowed(scan_id: &str, id: &str) -> Option<Vec<PathBuf>> {
    let mut s = SCANS.lock().ok()?;
    s.iter_mut().find(|(sid, _)| sid == scan_id).and_then(|(_, m)| m.remove(id))
}

/// Paths never moved to the Trash, whatever asked for it.
pub fn protected(home: &Path, p: &Path) -> bool {
    let s = p.to_string_lossy();
    if !p.is_absolute() || s.contains("/../") || s.ends_with("/..") {
        return true;
    }
    for root in ["/System", "/usr", "/bin", "/sbin", "/private", "/Library/Apple", "/Applications/Utilities", "/cores", "/opt/homebrew/bin"] {
        if s == root || s.starts_with(&format!("{root}/")) {
            return true;
        }
    }
    let exact: Vec<PathBuf> = ["", "Library", "Desktop", "Documents", "Downloads", "Pictures", "Movies", "Music", "Applications", ".Trash"].iter().map(|n| if n.is_empty() { home.to_path_buf() } else { home.join(n) }).collect();
    exact.iter().any(|e| e == p) || p == Path::new("/") || p == Path::new("/Applications")
}

/// What moving to the Trash did.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Trashed {
    pub moved: usize,
    pub bytes: u64,
    pub undo: Option<String>,
    pub error: Option<String>,
}

/// Moves files to the Trash through Finder; returns the undo step (Put Back).
pub async fn trash(runner: &dyn Runner, home: &Path, paths: &[PathBuf]) -> Trashed {
    let paths: Vec<PathBuf> = paths.iter().filter(|p| !protected(home, p) && std::fs::symlink_metadata(p).is_ok()).cloned().collect();
    if paths.is_empty() {
        return Trashed { moved: 0, bytes: 0, undo: None, error: Some("Those files aren't there any more.".into()) };
    }
    let sizes: Vec<u64> = paths.iter().map(|p| std::fs::symlink_metadata(p).map(|m| if m.is_dir() { size_dir(p, &|_| false, u64::MAX, &mut Vec::new(), Instant::now() + Duration::from_secs(5), &mut 200_000).bytes } else { on_disk(&m) }).unwrap_or(0)).collect();
    let out = match runner.run(&Command::Osa { script: TRASH, args: paths.iter().map(|p| p.display().to_string()).collect() }).await {
        Ok(o) => o,
        Err(e) => return Trashed { moved: 0, bytes: 0, undo: None, error: Some(e.text("Finder")) },
    };
    let (back, bytes, failed) = put_back_args(&paths, &sizes, &out);
    let moved = back.len() / 3;
    let undo = (moved > 0).then(|| macctl::keep_undo(Undo::Cmd(Command::Osa { script: PUT_BACK, args: back })));
    Trashed { moved, bytes, undo, error: failed.first().cloned() }
}

/// From the Trash script's lines: the Put Back arguments, bytes freed, and errors.
pub fn put_back_args(paths: &[PathBuf], sizes: &[u64], out: &str) -> (Vec<String>, u64, Vec<String>) {
    let mut back = Vec::new();
    let mut bytes = 0;
    let mut failed = Vec::new();
    for (i, line) in out.lines().map(str::trim).filter(|l| !l.is_empty()).enumerate() {
        let Some(p) = paths.get(i) else { break };
        if let Some(e) = line.strip_prefix('!') {
            failed.push(format!("{}: {}", p.file_name().and_then(|n| n.to_str()).unwrap_or("?"), e.trim()));
            continue;
        }
        let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else { continue };
        back.push(line.trim_end_matches('/').to_string());
        back.push(dir.display().to_string());
        back.push(name.to_string_lossy().to_string());
        bytes += sizes.get(i).copied().unwrap_or(0);
    }
    (back, bytes, failed)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// The storage card's "Move to Trash" (an id from the latest scan, never a path).
#[tauri::command]
pub async fn upkeep_trash(scan_id: String, id: String) -> AppResult<Trashed> {
    let paths = take_allowed(&scan_id, &id).ok_or_else(|| AppError::msg("That list is out of date; ask BYTE to check the storage again."))?;
    Ok(trash(&MacRunner, &home(), &paths).await)
}

/// The storage card's "Show in Finder" (an id from a scan, like the Trash buttons).
#[tauri::command]
pub async fn upkeep_reveal(scan_id: String, id: String) -> AppResult<()> {
    let path = SCANS.lock().ok().and_then(|s| s.iter().find(|(sid, _)| *sid == scan_id).and_then(|(_, m)| m.get(&id).and_then(|p| p.first().cloned())));
    let path = path.ok_or_else(|| AppError::msg("That list is out of date."))?;
    MacRunner.run(&Command::Exec { program: "open", args: vec!["-R".into(), path.display().to_string()] }).await.map(|_| ()).map_err(|e| AppError::msg(e.text("Finder")))
}

/// A health card's settings button (System Settings links only).
#[tauri::command]
pub async fn upkeep_open_settings(url: String) -> AppResult<()> {
    #[cfg(windows)]
    {
        return open_windows_settings(&url);
    }
    #[cfg(not(windows))]
    open_apple_settings(url).await
}

/// Opens a page of the Windows Settings app (ms-settings:privacy-microphone). The link is
/// validated to letters, digits, colon and hyphen only, because it reaches `cmd /C start`:
/// anything with an ampersand or a space could run something else.
#[cfg(any(windows, test))]
fn windows_settings_link_ok(url: &str) -> bool {
    url.starts_with("ms-settings:") && url.len() < 80 && url.chars().all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '-')
}

#[cfg(windows)]
fn open_windows_settings(url: &str) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    if !windows_settings_link_ok(url) {
        return Err(AppError::msg("Not a Windows Settings link."));
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| AppError::msg(format!("Couldn't open Settings: {e}")))
}

#[cfg(not(windows))]
async fn open_apple_settings(url: String) -> AppResult<()> {
    if !url.starts_with("x-apple.systempreferences:") || url.contains(char::is_whitespace) {
        return Err(AppError::msg("Not a System Settings link."));
    }
    MacRunner.run(&Command::Exec { program: "open", args: vec![url] }).await.map(|_| ()).map_err(|e| AppError::msg(e.text("System Settings")))
}

// ------------------------------------------------------------------- health

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Bad,
    Warn,
    Ok,
    Info,
}

/// One line of a health card.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub label: String,
    pub value: String,
    pub level: Level,
    /// What to do about it (empty when fine).
    pub tip: String,
    /// A System Settings link that fixes it.
    pub settings: Option<String>,
    pub settings_label: Option<String>,
}

/// A program using the Mac, from `top`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Proc {
    pub name: String,
    pub cpu: f32,
    pub mem: String,
    /// An app with a window: the card can ask it to quit.
    pub app: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub id: String,
    pub title: String,
    pub checks: Vec<Check>,
    pub procs: Vec<Proc>,
}

fn check(label: &str, value: impl Into<String>, level: Level, tip: impl Into<String>, pane: Option<&str>) -> Check {
    let (settings, settings_label) = match pane {
        Some(p) => (Some(macctl::settings_url(p)), Some(macctl::pane_label(p).to_string())),
        None => (None, None),
    };
    Check { label: label.into(), value: value.into(), level, tip: tip.into(), settings, settings_label }
}

/// `pmset -g batt` → (percent, on battery, state, time left).
pub fn parse_batt(out: &str) -> Option<(u8, bool, String, Option<String>)> {
    let on_battery = out.contains("'Battery Power'");
    let line = out.lines().find(|l| l.contains("InternalBattery"))?;
    let pct: u8 = line.split('%').next()?.rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    let after = line.split('%').nth(1).unwrap_or("");
    let parts: Vec<&str> = after.split(';').map(str::trim).filter(|s| !s.is_empty()).collect();
    let state = parts.first().copied().unwrap_or("").to_string();
    let left = parts.get(1).and_then(|s| s.split_whitespace().next()).filter(|t| t.contains(':') && *t != "0:00").map(str::to_string);
    Some((pct, on_battery, state, left))
}

/// `system_profiler SPPowerDataType` → (cycle count, condition, max capacity %).
pub fn parse_power(out: &str) -> (Option<u32>, Option<String>, Option<u8>) {
    let field = |k: &str| out.lines().find_map(|l| l.trim().strip_prefix(k).map(|v| v.trim().to_string()));
    (field("Cycle Count:").and_then(|v| v.parse().ok()), field("Condition:"), field("Maximum Capacity:").and_then(|v| v.trim_end_matches('%').trim().parse().ok()))
}

/// `memory_pressure` → free memory percent.
pub fn parse_mem_free(out: &str) -> Option<u8> {
    let l = out.lines().find(|l| l.contains("memory free percentage"))?;
    l.rsplit(':').next()?.trim().trim_end_matches('%').trim().parse().ok()
}

/// `top -l 2 -stats cpu,mem,command`: the programs in the last sample.
pub fn parse_top(out: &str) -> Vec<(f32, String, String)> {
    let Some(start) = out.rfind("%CPU") else { return vec![] };
    let body = &out[start..];
    body.lines()
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let cpu: f32 = it.next()?.parse().ok()?;
            let mem = it.next()?.trim_end_matches(['+', '-']).to_string();
            let name: Vec<&str> = it.collect();
            (!name.is_empty()).then(|| (cpu, mem, name.join(" ")))
        })
        .collect()
}

/// `pmset -g assertions` → programs keeping the Mac awake.
pub fn parse_awake(out: &str) -> Vec<String> {
    let mut names = Vec::new();
    for l in out.lines() {
        let l = l.trim();
        if !l.starts_with("pid ") || !(l.contains("PreventUserIdleSystemSleep") || l.contains("PreventSystemSleep") || l.contains("PreventUserIdleDisplaySleep")) {
            continue;
        }
        if let (Some(a), Some(b)) = (l.find('('), l.find("):")) {
            let n = l[a + 1..b].to_string();
            if !["powerd", "coreaudiod", "WindowServer", "sharingd", "bluetoothd", "useractivityd"].contains(&n.as_str()) && !names.contains(&n) {
                names.push(n);
            }
        }
    }
    names
}

/// `sysctl -n kern.boottime` → seconds since the epoch.
pub fn parse_boot(out: &str) -> Option<u64> {
    let s = out.split("sec =").nth(1)?;
    s.split(',').next()?.trim().parse().ok()
}

/// `tmutil latestbackup` → the backup's date ("2026-09-29-101010" in the path).
pub fn parse_backup(out: &str) -> Option<chrono::NaiveDate> {
    let last = out.lines().rev().find(|l| !l.trim().is_empty())?;
    for part in last.split(['/', '.']) {
        if part.len() >= 10 {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&part[..10], "%Y-%m-%d") {
                return Some(d);
            }
        }
    }
    None
}

/// An app name from `top`'s (possibly cut) program name, if one matches.
pub fn app_for(proc_name: &str, apps: &[String]) -> Option<String> {
    let p = proc_name.trim();
    if p.is_empty() {
        return None;
    }
    apps.iter()
        .find(|a| a.as_str() == p)
        .or_else(|| apps.iter().find(|a| p.len() >= 6 && a.starts_with(p)))
        .or_else(|| apps.iter().find(|a| a.len() >= 4 && p.starts_with(a.as_str()) && p[a.len()..].starts_with(' ')))
        .cloned()
}

/// Apps the health card may ask to quit, per card.
static QUITTABLE: Lazy<Mutex<Vec<(String, Vec<String>)>>> = Lazy::new(|| Mutex::new(Vec::new()));

async fn out_of(runner: &dyn Runner, program: &'static str, args: &[&str]) -> Option<String> {
    runner.run(&Command::Exec { program, args: args.iter().map(|s| s.to_string()).collect() }).await.ok()
}

/// Reads the Mac's state and turns it into checks. `coach`: slowness and battery
/// first (with the busiest programs); otherwise a general check-up.
pub async fn health(runner: &dyn Runner, home: &Path, coach: bool, now: SystemTime) -> Health {
    let mut checks = Vec::new();
    let now_s = now.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

    // Disk.
    if let Some((total, free)) = out_of(runner, "df", &["-k", &home.display().to_string()]).await.as_deref().and_then(parse_df).filter(|(t, _)| *t > 0) {
        let pct = free as f64 * 100.0 / total as f64;
        let (level, tip) = if pct < 5.0 {
            (Level::Bad, "Almost full: macOS needs free space to update and run smoothly. Ask BYTE \"what's taking up space\".")
        } else if pct < 12.0 {
            (Level::Warn, "Getting full; a Mac slows down when space runs low. Ask BYTE \"what's taking up space\".")
        } else {
            (Level::Ok, "")
        };
        checks.push(check("Storage", format!("{} free of {} ({pct:.0}%)", size_text(free), size_text(total)), level, tip, (level != Level::Ok).then_some("storage")));
    }
    // Memory.
    if let Some(free) = out_of(runner, "memory_pressure", &[]).await.as_deref().and_then(parse_mem_free) {
        let (level, tip) = if free < 10 {
            (Level::Bad, "Memory is nearly full, so the Mac swaps to disk and slows down. Quit apps you're not using (browser tabs count).")
        } else if free < 25 {
            (Level::Warn, "Memory is getting tight. Quitting unused apps and tabs helps.")
        } else {
            (Level::Ok, "")
        };
        checks.push(check("Memory", format!("{free}% free"), level, tip, None));
    }
    // Busy programs.
    let mut procs = Vec::new();
    if coach {
        let apps: Vec<String> = runner.run(&Command::Osa { script: FRONT_APPS, args: vec![] }).await.map(|o| o.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()).unwrap_or_default();
        if let Some(out) = out_of(runner, "top", &["-l", "2", "-o", "cpu", "-n", "8", "-stats", "cpu,mem,command"]).await {
            for (cpu, mem, name) in parse_top(&out) {
                let app = app_for(&name, &apps).filter(|a| a != "Finder" && a != "BYTE");
                procs.push(Proc { name, cpu, mem, app });
            }
        }
        let hot: Vec<&Proc> = procs.iter().filter(|p| p.cpu >= 50.0).collect();
        if hot.is_empty() {
            checks.push(check("Processor", "Nothing is working the processor hard right now", Level::Ok, "", None));
        } else {
            let names: Vec<String> = hot.iter().map(|p| format!("{} ({:.0}%)", p.name, p.cpu)).collect();
            checks.push(check("Processor", format!("Busy: {}", names.join(", ")), Level::Warn, "These are using a lot of processor time, which slows other apps, warms the Mac and drains the battery. Quit or restart them if you're not using them.", None));
        }
        if let Some(out) = out_of(runner, "pmset", &["-g", "assertions"]).await {
            let awake = parse_awake(&out);
            if !awake.is_empty() {
                checks.push(check("Keeping the Mac awake", awake.join(", "), Level::Info, "These stop the Mac from sleeping while they run (a video, a download, a call). That's normal while you use them; quit them when you're done.", None));
            }
        }
    }
    // Battery.
    if let Some((pct, on_battery, _, left)) = out_of(runner, "pmset", &["-g", "batt"]).await.as_deref().and_then(parse_batt) {
        let now_text = format!("{pct}%, {}{}", if on_battery { "on battery" } else { "plugged in" }, left.map(|l| format!(", about {l} left")).unwrap_or_default());
        checks.push(check("Battery", now_text, Level::Info, "", None));
        let (cycles, condition, max) = out_of(runner, "system_profiler", &["SPPowerDataType"]).await.as_deref().map(parse_power).unwrap_or((None, None, None));
        if let Some(c) = condition {
            let bad = !c.eq_ignore_ascii_case("normal");
            let worn = max.is_some_and(|m| m < 80);
            let value = format!("{c}{}{}", max.map(|m| format!(", {m}% of its original capacity")).unwrap_or_default(), cycles.map(|n| format!(", {n} charge cycles")).unwrap_or_default());
            let (level, tip) = if bad {
                (Level::Bad, "macOS recommends a battery service. An Apple Store or authorized service provider can replace it.")
            } else if worn {
                (Level::Warn, "The battery holds noticeably less than when new. Optimized charging (Battery settings) slows further wear.")
            } else {
                (Level::Ok, "")
            };
            checks.push(check("Battery health", value, level, tip, (level != Level::Ok).then_some("battery")));
        }
    }
    // Restarted lately?
    if let Some(boot) = out_of(runner, "sysctl", &["-n", "kern.boottime"]).await.as_deref().and_then(parse_boot) {
        let days = now_s.saturating_sub(boot) / 86_400;
        let (level, tip) = if days >= 14 { (Level::Warn, "A restart now and then clears out memory and finishes updates.") } else { (Level::Ok, "") };
        checks.push(check("Last restart", if days == 0 { "Today".into() } else if days == 1 { "Yesterday".to_string() } else { format!("{days} days ago") }, level, tip, None));
    }
    if !coach {
        if let Some(v) = out_of(runner, "sw_vers", &["-productVersion"]).await {
            checks.push(check("macOS", v.trim().to_string(), Level::Info, "", Some("software update")));
        }
        match out_of(runner, "tmutil", &["latestbackup"]).await.as_deref().and_then(parse_backup) {
            Some(d) => {
                let today = chrono::DateTime::<chrono::Local>::from(now).date_naive();
                let days = (today - d).num_days().max(0);
                let (level, tip) = if days > 14 { (Level::Warn, "The last Time Machine backup is over two weeks old. Connect the backup disk.") } else { (Level::Ok, "") };
                checks.push(check("Time Machine", if days == 0 { "Backed up today".to_string() } else { format!("Last backup {days} days ago") }, level, tip, (level != Level::Ok).then_some("time machine")));
            }
            None => checks.push(check("Time Machine", "No backup found", Level::Warn, "BYTE couldn't find a Time Machine backup. A backup protects your files if the Mac is lost or breaks. (If you do back up, macOS may just not let BYTE see it.)", Some("time machine"))),
        }
        if let Some(out) = out_of(runner, "fdesetup", &["status"]).await {
            let on = out.contains("FileVault is On");
            checks.push(check("FileVault", if on { "On" } else { "Off" }, if on { Level::Ok } else { Level::Warn }, if on { "" } else { "FileVault encrypts the disk, so nobody can read your files if the Mac is lost or stolen." }, (!on).then_some("security")));
        }
        if let Some(out) = out_of(runner, "socketfilterfw", &["--getglobalstate"]).await {
            let on = out.contains("enabled");
            checks.push(check("Firewall", if on { "On" } else { "Off" }, if on { Level::Ok } else { Level::Info }, if on { "" } else { "Worth turning on if you use public Wi-Fi (System Settings → Network → Firewall)." }, (!on).then_some("network")));
        }
    }
    checks.sort_by_key(|c| c.level);
    let id = uuid::Uuid::new_v4().simple().to_string();
    let quittable: Vec<String> = procs.iter().filter_map(|p| p.app.clone()).collect();
    if let Ok(mut q) = QUITTABLE.lock() {
        q.push((id.clone(), quittable));
        let n = q.len();
        if n > 5 {
            q.drain(..n - 5);
        }
    }
    Health { id, title: if coach { "How your Mac is doing".into() } else { "Mac check-up".into() }, checks, procs }
}

/// The health card's "Quit" button (only apps that card listed).
#[tauri::command]
pub async fn upkeep_quit(card: String, app: String) -> AppResult<bool> {
    let ok = QUITTABLE.lock().ok().is_some_and(|q| q.iter().any(|(id, apps)| *id == card && apps.contains(&app)));
    if !ok {
        return Err(AppError::msg("BYTE can only quit the apps on that card."));
    }
    MacRunner.run(&Command::Osa { script: QUIT_APP, args: vec![app.clone()] }).await.map(|_| true).map_err(|e| AppError::msg(e.text(&app)))
}

// ---------------------------------------------------------------- uninstall

/// Where apps live.
fn app_dirs(home: &Path) -> Vec<PathBuf> {
    vec![PathBuf::from("/Applications"), home.join("Applications")]
}

/// Apps whose name matches (exact names first), looking one folder deep.
pub fn find_apps(dirs: &[PathBuf], name: &str) -> Vec<PathBuf> {
    let want = name.to_lowercase();
    let mut all = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if n.ends_with(".app") {
                all.push(p);
            } else if p.is_dir() && !n.starts_with('.') && n != "Utilities" {
                if let Ok(sub) = std::fs::read_dir(&p) {
                    all.extend(sub.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "app")));
                }
            }
        }
    }
    let stem = |p: &PathBuf| p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let exact: Vec<PathBuf> = all.iter().filter(|p| stem(p) == want).cloned().collect();
    if !exact.is_empty() {
        return exact;
    }
    let mut part: Vec<PathBuf> = all.into_iter().filter(|p| want.len() >= 3 && (stem(p).contains(&want) || stem(p).split_whitespace().any(|w| w == want))).collect();
    part.sort();
    part.truncate(6);
    part
}

/// The bundle id from an app's Info.plist (XML form; the binary form is read by `defaults`).
pub fn bundle_id_xml(plist: &str) -> Option<String> {
    let i = plist.find("<key>CFBundleIdentifier</key>")?;
    let rest = &plist[i..];
    let a = rest.find("<string>")? + "<string>".len();
    let b = rest[a..].find("</string>")?;
    Some(rest[a..a + b].trim().to_string()).filter(|s| !s.is_empty())
}

async fn bundle_id(runner: &dyn Runner, app: &Path) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    if let Some(id) = std::fs::read_to_string(&plist).ok().as_deref().and_then(bundle_id_xml) {
        return Some(id);
    }
    out_of(runner, "defaults", &["read", &app.join("Contents/Info").display().to_string(), "CFBundleIdentifier"]).await.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Why an app can't be uninstalled by BYTE (None: it can, after the user's OK).
pub fn refuse_app(app: &Path, id: &str) -> Option<&'static str> {
    if id.starts_with("com.apple.") || app.starts_with("/System") || app.starts_with("/Applications/Utilities") {
        return Some("it's part of macOS");
    }
    if id == OWN_ID || id.starts_with(&format!("{OWN_ID}.")) {
        return Some("that's BYTE itself (drag it to the Trash if you want to remove it)");
    }
    None
}

/// Where apps keep their settings and data, in ~/Library.
const LEFTOVER_DIRS: &[&str] = &["Application Support", "Caches", "Preferences", "Logs", "Saved Application State", "HTTPStorages", "WebKit", "Containers", "Group Containers", "Application Scripts", "Cookies", "LaunchAgents"];
/// Folders macOS protects (sizes aren't read; Finder still moves them).
const PRIVATE_DIRS: &[&str] = &["Containers", "Group Containers", "Application Scripts"];

/// An app's files in ~/Library, with sizes where macOS lets BYTE look.
pub fn leftovers(lib: &Path, id: &str, name: &str) -> Vec<(PathBuf, Option<u64>)> {
    let id_l = id.to_lowercase();
    let name_l = name.to_lowercase();
    let mut out = Vec::new();
    for d in LEFTOVER_DIRS {
        let dir = lib.join(d);
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut hits: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
                let by_id = !id_l.is_empty() && (n == id_l || n.starts_with(&format!("{id_l}.")) || (*d == "Group Containers" && n.ends_with(&format!(".{id_l}"))));
                let by_name = name_l.len() >= 4 && ["Application Support", "Caches", "Logs"].contains(d) && n == name_l;
                by_id || by_name
            })
            .collect();
        hits.sort();
        for h in hits {
            let size = (!PRIVATE_DIRS.contains(d)).then(|| {
                std::fs::symlink_metadata(&h).map(|m| if m.is_dir() { size_dir(&h, &|_| false, u64::MAX, &mut Vec::new(), Instant::now() + Duration::from_secs(3), &mut 100_000).bytes } else { on_disk(&m) }).unwrap_or(0)
            });
            out.push((h, size));
        }
    }
    out
}

// -------------------------------------------------------------- login items

/// `LOGIN_LIST` output → (name, path).
pub fn parse_login(out: &str) -> Vec<(String, String)> {
    out.lines().filter_map(|l| l.split_once('\t')).map(|(n, p)| (n.trim().to_string(), p.trim().to_string())).filter(|(n, _)| !n.is_empty()).collect()
}

/// The login item a name means (exact, then contained either way).
pub fn match_login<'a>(items: &'a [(String, String)], want: &str) -> Option<&'a (String, String)> {
    let w = want.to_lowercase();
    items.iter().find(|(n, _)| n.to_lowercase() == w).or_else(|| items.iter().find(|(n, _)| n.to_lowercase().contains(&w) || (n.len() >= 4 && w.contains(&n.to_lowercase()))))
}

/// Background helpers in LaunchAgents folders (information only).
fn launch_agents(home: &Path) -> Vec<String> {
    let mut v = Vec::new();
    for d in [home.join("Library/LaunchAgents"), PathBuf::from("/Library/LaunchAgents")] {
        if let Ok(rd) = std::fs::read_dir(&d) {
            v.extend(rd.flatten().filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).map(str::to_string)).filter(|s| !s.starts_with("com.apple.")));
        }
    }
    v.sort();
    v.dedup();
    v
}

// ---------------------------------------------------------------------- run

fn done_card(send: Emit<'_>, app: &str, title: &str, detail: &str, ok: bool, undo: Option<String>) -> AppResult<()> {
    send(ChatEvent::MacDone(MacDone { app: app.into(), title: title.into(), detail: detail.into(), ok, undo }))
}

/// Does what the message asks. Returns notes for the answer.
pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    run_with(turn, question, &MacRunner, &home(), SystemTime::now(), cancel, send).await
}

fn health_notes(h: &Health) -> String {
    let mut lines: Vec<String> = h.checks.iter().map(|c| format!("- {}: {} [{:?}]{}", c.label, c.value, c.level, if c.tip.is_empty() { String::new() } else { format!(" — {}", c.tip) })).collect();
    if !h.procs.is_empty() {
        lines.push(format!("Busiest programs now: {}", h.procs.iter().take(6).map(|p| format!("{} {:.0}% CPU, {} memory", p.name, p.cpu, p.mem)).collect::<Vec<_>>().join("; ")));
    }
    lines.join("\n")
}

pub(crate) async fn run_with(turn: &Turn<'_>, question: &str, runner: &dyn Runner, home: &Path, now: SystemTime, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(a) = ask(question) else { return Ok(None) };
    let id = format!("byte_upkeep_{}", uuid::Uuid::new_v4().simple());
    let none = SourceBook::default;
    match a {
        Ask::Storage => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_storage".into(), args: json!({ "app": "Finder", "what": "Look at what's using space on this Mac" }) })?;
            let (total, free) = out_of(runner, "df", &["-k", &home.display().to_string()]).await.as_deref().and_then(parse_df).unwrap_or((0, 0));
            let h = home.to_path_buf();
            let (scan, dups) = tokio::task::spawn_blocking(move || {
                let s = scan(&h, LIMITS.dup_min, SCAN_TIME, SCAN_ENTRIES);
                let d = duplicates(&s.files, LIMITS.dup_min, Instant::now() + Duration::from_secs(15));
                (s, d)
            })
            .await
            .map_err(|e| AppError::msg(e.to_string()))?;
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let (card, allowed) = storage_card(home, &scan, &dups, total, free, LIMITS, now);
            remember_scan(&card.scan_id, allowed);
            let could: u64 = card.suggestions.iter().filter(|s| s.can_trash).map(|s| s.bytes).sum();
            let summary = format!("{} folders sized · {} could be freed", card.folders.len(), size_text(could));
            send(ChatEvent::ToolResult { id, ok: true, summary: summary.clone() })?;
            turn.log.record("mac_storage", &json!({}), true, &summary);
            let mut notes = String::from("BYTE looked at this Mac's storage (the card above shows it; the user can move suggested items to the Trash from the card, and Undo puts them back).\n");
            if total > 0 {
                notes.push_str(&format!("Disk: {} free of {}.\n", size_text(free), size_text(total)));
            }
            notes.push_str("Biggest folders in the home folder:\n");
            for f in card.folders.iter().take(8) {
                notes.push_str(&format!("- {} ({}): {}\n", f.name, f.path, size_text(f.bytes)));
            }
            if card.suggestions.is_empty() {
                notes.push_str("Nothing obvious to clean up.\n");
            } else {
                notes.push_str("Suggestions:\n");
                for s in &card.suggestions {
                    notes.push_str(&format!("- {}: {} ({} items) — {}\n", s.title, size_text(s.bytes), s.count, s.why));
                }
            }
            if !card.big.is_empty() {
                notes.push_str(&format!("Biggest files: {}\n", card.big.iter().take(5).map(|b| format!("{} {}", b.path, size_text(b.bytes))).collect::<Vec<_>>().join("; ")));
            }
            if card.partial {
                notes.push_str("The home folder is very large, so the scan stopped early; real sizes are at least these.\n");
            }
            notes.push_str("BYTE doesn't look inside ~/Library (apart from caches) or at macOS itself, so System Settings → General → Storage can show more (System Data, iCloud, Photos).\n\nSummarize in a few bullets: where the space goes, and the safest things to clear first. Point to the card's buttons; don't tell the user to use Terminal commands.");
            send(ChatEvent::Storage(card))?;
            Ok(Some((none(), notes)))
        }
        Ask::Coach | Ask::Checkup => {
            let coach = a == Ask::Coach;
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_health".into(), args: json!({ "app": "System", "what": if coach { "Check what's slowing the Mac or using the battery" } else { "Run a Mac check-up" } }) })?;
            let h = health(runner, home, coach, now).await;
            let issues = h.checks.iter().filter(|c| c.level <= Level::Warn).count();
            let summary = if issues == 0 { "All good".to_string() } else { format!("{issues} things to look at") };
            send(ChatEvent::ToolResult { id, ok: true, summary: summary.clone() })?;
            turn.log.record("mac_health", &json!({ "coach": coach }), true, &summary);
            let notes = format!(
                "BYTE checked this Mac just now (shown in the card above):\n{}\n\nExplain what matters most first in plain words, then practical fixes. Mention that the card has buttons for settings and for quitting busy apps. Don't suggest Terminal commands, cleaner apps or resetting SMC/NVRAM.",
                health_notes(&h)
            );
            send(ChatEvent::Health(h))?;
            Ok(Some((none(), notes)))
        }
        Ask::LoginItems => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_login_items".into(), args: json!({ "app": "System Events", "what": "List what opens at login" }) })?;
            let items = match runner.run(&Command::Osa { script: LOGIN_LIST, args: vec![] }).await {
                Ok(o) => parse_login(&o),
                Err(e) => {
                    let msg = e.text("System Events");
                    send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
                    return Ok(Some((none(), format!("BYTE couldn't read the login items: {msg}\n\n{}", macctl::TELL))));
                }
            };
            let agents = launch_agents(home);
            send(ChatEvent::ToolResult { id, ok: true, summary: format!("{} login items", items.len()) })?;
            let mut checks: Vec<Check> = items.iter().map(|(n, p)| check(n, tilde(home, Path::new(p)), Level::Info, "", None)).collect();
            if !agents.is_empty() {
                checks.push(check("Background helpers", agents.join(", "), Level::Info, "Helpers apps installed to run in the background. Turn them off in Login Items → Allow in the Background.", None));
            }
            checks.push(check("Change these", "System Settings → General → Login Items", Level::Info, "", Some("login item")));
            send(ChatEvent::Health(Health { id: uuid::Uuid::new_v4().simple().to_string(), title: "What opens at login".into(), checks, procs: vec![] }))?;
            Ok(Some((
                none(),
                format!(
                    "Apps that open when the user logs in: {}.\nBackground helpers installed by apps: {}.\n\nList them briefly. The user can say \"stop <app> from opening at login\" and BYTE will remove it (with Undo), or use the Login Items settings button on the card.",
                    if items.is_empty() { "none".to_string() } else { items.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ") },
                    if agents.is_empty() { "none".to_string() } else { agents.join(", ") }
                ),
            )))
        }
        Ask::LoginRemove { name } => {
            let items = runner.run(&Command::Osa { script: LOGIN_LIST, args: vec![] }).await.map(|o| parse_login(&o)).unwrap_or_default();
            let Some((found, path)) = match_login(&items, &name).cloned() else {
                return Ok(Some((
                    none(),
                    format!(
                        "\"{name}\" isn't in the login items BYTE can change ({}). Some apps start through their own setting or System Settings → General → Login Items → Allow in the Background; tell the user where to look.",
                        if items.is_empty() { "there are none".to_string() } else { items.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ") }
                    ),
                )));
            };
            let what = format!("Stop {found} from opening at login");
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_login_remove".into(), args: json!({ "app": "System Events", "what": what }) })?;
            let fields = vec![("App".to_string(), found.clone()), ("Note".to_string(), "The app stays installed; it just won't open by itself. Undo adds it back.".to_string())];
            if !macctl::ask_ok(&what, "System Events", fields, cancel, send).await? {
                send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
                return Ok(Some((none(), format!("The user chose to keep {found} opening at login. Nothing changed. Say so in one sentence."))));
            }
            match runner.run(&Command::Osa { script: LOGIN_REMOVE, args: vec![found.clone()] }).await {
                Ok(out) => {
                    let (p, hidden) = out.split_once('\t').map(|(a, b)| (a.to_string(), b.trim().to_string())).unwrap_or((path, "false".into()));
                    let undo = Some(macctl::keep_undo(Undo::Cmd(Command::Osa { script: LOGIN_ADD, args: vec![p, hidden] })));
                    send(ChatEvent::ToolResult { id, ok: true, summary: "Removed from login items".into() })?;
                    turn.log.record("mac_login_remove", &json!({ "name": found }), true, "removed");
                    done_card(send, "System Events", &what, "It stays installed and opens when you open it.", true, undo)?;
                    Ok(Some((none(), format!("Done: {found} no longer opens at login (it's still installed). Undo on the card adds it back."))))
                }
                Err(e) => {
                    let msg = e.text("System Events");
                    send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
                    Ok(Some((none(), format!("BYTE couldn't change the login items: {msg}\n\n{}", macctl::TELL))))
                }
            }
        }
        Ask::Uninstall { app } => {
            let found = find_apps(&app_dirs(home), &app);
            if found.is_empty() {
                return Ok(Some((none(), format!("There's no app called \"{app}\" in Applications. Ask the user for its exact name (as shown in the Applications folder)."))));
            }
            if found.len() > 1 {
                let names: Vec<String> = found.iter().map(|p| p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()).collect();
                return Ok(Some((none(), format!("Several apps match \"{app}\": {}. Ask which one to uninstall.", names.join(", ")))));
            }
            let path = found[0].clone();
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or(&app).to_string();
            let bid = bundle_id(runner, &path).await.unwrap_or_default();
            if let Some(why) = refuse_app(&path, &bid) {
                return Ok(Some((none(), format!("BYTE won't uninstall {name}: {why}. Say so briefly."))));
            }
            let what = format!("Uninstall {name}");
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_uninstall".into(), args: json!({ "app": "Finder", "what": what }) })?;
            let p2 = path.clone();
            let app_size = tokio::task::spawn_blocking(move || size_dir(&p2, &|_| false, u64::MAX, &mut Vec::new(), Instant::now() + Duration::from_secs(5), &mut 300_000).bytes).await.unwrap_or(0);
            let lib = home.join("Library");
            let (b2, n2) = (bid.clone(), name.clone());
            let left = tokio::task::spawn_blocking(move || leftovers(&lib, &b2, &n2)).await.unwrap_or_default();
            let left_bytes: u64 = left.iter().filter_map(|(_, s)| *s).sum();
            let mut list: Vec<String> = left.iter().take(10).map(|(p, s)| format!("{}{}", tilde(home, p), s.map(|b| format!(" ({})", size_text(b))).unwrap_or_default())).collect();
            if left.len() > 10 {
                list.push(format!("and {} more", left.len() - 10));
            }
            let fields = vec![
                ("App".to_string(), format!("{} ({})", tilde(home, &path), size_text(app_size))),
                ("Its files".to_string(), if list.is_empty() { "None found".to_string() } else { list.join("\n") }),
                ("Note".to_string(), "Everything goes to the Trash, so you can put it back until you empty it; Undo puts it back now. BYTE asks the app to quit first. macOS may ask for your password for apps installed for all users.".to_string()),
            ];
            if !macctl::ask_ok(&what, "Finder", fields, cancel, send).await? {
                send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
                return Ok(Some((none(), format!("The user chose to keep {name}. Nothing was removed. Say so in one sentence."))));
            }
            if !bid.is_empty() {
                let _ = runner.run(&Command::Osa { script: QUIT_ID, args: vec![bid.clone()] }).await;
            }
            let mut paths = vec![path.clone()];
            paths.extend(left.iter().map(|(p, _)| p.clone()));
            let t = trash(runner, home, &paths).await;
            let ok = t.moved > 0 && t.error.is_none();
            let detail = format!("{} of {} items moved to the Trash · {}", t.moved, paths.len(), size_text(app_size + left_bytes));
            send(ChatEvent::ToolResult { id, ok, summary: detail.clone() })?;
            turn.log.record("mac_uninstall", &json!({ "app": name }), ok, &detail);
            done_card(send, "Finder", &what, &detail, ok, t.undo)?;
            let problem = t.error.map(|e| format!(" Some items weren't moved: {e}.")).unwrap_or_default();
            Ok(Some((none(), format!("Done: BYTE moved {name} and {} of its files to the Trash.{problem} Undo on the card, or Put Back in the Trash, restores it; emptying the Trash frees the space.", t.moved.saturating_sub(1)))))
        }
    }
}

#[cfg(test)]
#[path = "upkeep_tests.rs"]
mod tests;
