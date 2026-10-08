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
use crate::pcctl::WinOp;
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

/// `w` as a whole word ("pc" in "my pc is slow", not in "specs").
fn has_word(l: &str, w: &str) -> bool {
    l.split(|c: char| !c.is_alphanumeric()).any(|x| x == w)
}

/// Strips filler around an app name ("the Zoom app from my Mac" → "zoom").
fn app_name(s: &str) -> String {
    let mut s = s.trim().to_string();
    for suffix in [" completely", " from my mac", " from this mac", " from the mac", " from my pc", " from this pc", " from the pc", " from my computer", " for me", " app", " application"] {
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
    let at_login = has_any(&l, &[" at login", " at startup", " when i log in", " when i start", " on startup", " on login", " at log in", "login item", "startup item", " with windows", " when windows starts", " when windows boots", " when my pc starts", " when my pc boots", " when i turn on my pc", "startup app", "startup program"]);
    if at_login {
        for p in ["stop ", "don't open ", "dont open ", "don't launch ", "remove ", "disable "] {
            if let Some(rest) = l.strip_prefix(p) {
                let mut name = rest.to_string();
                for cut in [" from opening", " from launching", " from starting", " opening", " launching", " starting", " from login items", " from my login items", " from startup", " at login", " at startup", " when i log in", " on startup", " on login", " with windows", " when windows starts", " when windows boots", " when my pc starts", " when my pc boots", " when i turn on my pc"] {
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
    // A PC reader's wording of the same questions.
    if has_any(&l, &["clean up my pc", "clean my pc", "cleanup my pc", "clean up my computer", "clean up my laptop"]) {
        return Some(Ask::Storage);
    }
    if has_any(&l, &["check my pc", "check up on my pc", "is my pc ok", "is my pc okay", "is my pc healthy", "diagnose my pc", "pc health check", "pc's health"]) {
        return Some(Ask::Checkup);
    }
    // Storage.
    let space = has_any(&l, &["space", "storage", "disk", "hard drive", "ssd"]);
    if (space && has_any(&l, &["taking up", "using up", "free up", "running out", "running low", "low on", "is full", "almost full", "where did", "clean up", "cleanup", "analy", "what's using", "what is using", "eating", "hogging", "full"]))
        || has_any(&l, &["duplicate files", "find duplicates", "duplicate photos", "big files", "biggest files", "large files", "largest files", "huge files", "clean up my mac", "clean my mac", "cleanup my mac", "declutter my mac"])
    {
        return Some(Ask::Storage);
    }
    // Slow / battery.
    let mac = has_any(&l, &["mac", "computer", "laptop", "macbook", "my system"]) || has_word(&l, "pc");
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
    (cfg!(target_os = "macos") || cfg!(windows)) && enabled && ask(q).is_some()
}

// ------------------------------------------------------------------ storage

/// Space a file takes on disk (a cloud file not downloaded takes none).
fn on_disk(m: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.blocks() * 512
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // OFFLINE, RECALL_ON_OPEN, RECALL_ON_DATA_ACCESS: a OneDrive file that is only in the cloud. It has a size
        // but takes no room here, and counting it would blame the disk for space that is free.
        const NOT_HERE: u32 = 0x1000 | 0x0004_0000 | 0x0040_0000;
        if m.file_attributes() & NOT_HERE != 0 {
            0
        } else {
            m.len()
        }
    }
    #[cfg(not(any(unix, windows)))]
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
#[cfg_attr(not(test), allow(dead_code))]
fn offerable(home: &Path, p: &Path) -> bool {
    offerable_for(false, home, p)
}

/// A path with `/` between its parts, whichever system wrote it, and lower-case: how a PC's paths are compared
/// (Windows ignores case and accepts either slash).
fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// `offerable` for a Mac (`pc` false) or a PC, where AppData takes the place of Library.
fn offerable_for(pc: bool, home: &Path, p: &Path) -> bool {
    if pc {
        let (h, f) = (norm(home), norm(p));
        let Some(rest) = f.strip_prefix(&format!("{}/", h.trim_end_matches('/'))) else { return false };
        let parts: Vec<&str> = rest.split('/').filter(|c| !c.is_empty()).collect();
        if parts.first() == Some(&"appdata") {
            return false;
        }
        let n = parts.len();
        return parts.iter().enumerate().all(|(i, c)| !c.starts_with('.') && (i + 1 == n || !is_package(c)));
    }
    let Ok(rest) = p.strip_prefix(home) else { return false };
    let parts: Vec<&str> = rest.iter().filter_map(|c| c.to_str()).collect();
    if parts.first() == Some(&"Library") {
        return false;
    }
    let n = parts.len();
    parts.iter().enumerate().all(|(i, c)| !c.starts_with('.') && (i + 1 == n || !is_package(c)))
}

/// `home/a/b` from `"a/b"`, joined part by part so a PC gets backslashes and compares equal to what the disk lists.
fn under(home: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(home.to_path_buf(), |acc, part| acc.join(part))
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

/// The same for a PC (relative to the user's folder; their contents go, the folders stay).
pub const PC_DEV_CACHES: &[(&str, &str)] = &[
    ("AppData/Local/pip/Cache", "Python package downloads"),
    (".cache/pip", "Python package downloads"),
    ("AppData/Local/npm-cache", "npm package cache"),
    (".npm/_cacache", "npm package cache"),
    ("AppData/Local/Yarn/Cache", "Yarn package cache"),
    ("AppData/Local/NuGet/v3-cache", "NuGet package downloads"),
    (".gradle/caches", "Gradle build caches"),
    (".cargo/registry/cache", "Rust package downloads"),
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
#[cfg_attr(not(test), allow(dead_code))]
pub fn scan(home: &Path, keep_min: u64, time: Duration, entries: u64) -> Scan {
    scan_for(false, home, keep_min, time, entries)
}

/// `scan` for a Mac (`pc` false) or a PC. On a PC the folder that holds the programs' own data (AppData) is left
/// alone like Library, the developer caches are the Windows ones, and Temp is measured as "app caches".
pub fn scan_for(pc: bool, home: &Path, keep_min: u64, time: Duration, entries: u64) -> Scan {
    let deadline = Instant::now() + time;
    let mut budget = entries;
    let mut s = Scan::default();
    let lib = if pc { home.join("AppData") } else { home.join("Library") };
    let trash = (!pc).then(|| home.join(".Trash"));
    let table = if pc { PC_DEV_CACHES } else { DEV_CACHES };
    let cache_dirs: Vec<PathBuf> = table.iter().map(|(p, _)| under(home, p)).collect();
    let skip = |p: &Path| p == lib || trash.as_deref() == Some(p) || cache_dirs.iter().any(|c| c == p);
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
    s.files.retain(|f| offerable_for(pc, home, &f.path));
    if hidden > 0 {
        s.folders.push(("Hidden folders".into(), home.to_path_buf(), hidden));
    }
    // Caches have their own time, so a huge home folder can't hide them.
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut budget = entries;
    let mut none = Vec::new();
    for (i, (_, label)) in table.iter().enumerate() {
        let w = size_dir(&cache_dirs[i], &|_| false, u64::MAX, &mut none, deadline, &mut budget);
        if w.bytes > 0 {
            s.caches.push((label.to_string(), cache_dirs[i].clone(), w.bytes));
        }
    }
    let caches = if pc { temp_dir_of(home) } else { lib.join("Caches") };
    let skip_dev = |p: &Path| cache_dirs.iter().any(|c| c == p);
    s.app_caches = size_dir(&caches, &skip_dev, u64::MAX, &mut none, deadline, &mut budget).bytes;
    s.folders.sort_by(|a, b| b.2.cmp(&a.2));
    s
}

/// The folder a PC's programs keep temporary files in: the system's own when it is in this user's folder (it is, unless
/// someone moved it), else the usual place.
fn temp_dir_of(home: &Path) -> PathBuf {
    let t = std::env::temp_dir();
    if norm(&t).starts_with(&format!("{}/", norm(home).trim_end_matches('/'))) {
        t
    } else {
        under(home, "AppData/Local/Temp")
    }
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
/// A PC's installers. An .exe is only one when its name says so: most .exe files in Downloads are programs people use.
const PC_INSTALLERS: &[&str] = &["msi", "msix", "msixbundle", "appx", "appxbundle", "iso"];

fn installer_like(pc: bool, path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase) else { return false };
    if !pc {
        return INSTALLERS.contains(&ext.as_str());
    }
    if PC_INSTALLERS.contains(&ext.as_str()) {
        return true;
    }
    let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    ext == "exe" && ["setup", "install", "installer"].iter().any(|w| name.contains(w))
}

fn tilde(home: &Path, p: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

/// `tilde` for a Mac or a PC; a PC writes its paths with backslashes.
fn tilde_for(pc: bool, home: &Path, p: &Path) -> String {
    let t = tilde(home, p);
    if pc {
        t.replace('/', "\\")
    } else {
        t
    }
}

/// What the scan found, as a card plus the paths behind each id (what the
/// Trash buttons may remove).
#[cfg_attr(not(test), allow(dead_code))]
pub fn storage_card(home: &Path, scan: &Scan, dups: &[Vec<FileRec>], total: u64, free: u64, lim: Limits, now: SystemTime) -> (Storage, HashMap<String, Vec<PathBuf>>) {
    storage_card_for(false, home, scan, dups, total, free, lim, now)
}

/// `storage_card` for a Mac (`pc` false) or a PC.
#[allow(clippy::too_many_arguments)]
pub fn storage_card_for(pc: bool, home: &Path, scan: &Scan, dups: &[Vec<FileRec>], total: u64, free: u64, lim: Limits, now: SystemTime) -> (Storage, HashMap<String, Vec<PathBuf>>) {
    let scan_id = uuid::Uuid::new_v4().simple().to_string();
    let mut allowed: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut suggestions = Vec::new();
    let show = |paths: &[PathBuf]| paths.iter().take(8).map(|p| tilde_for(pc, home, p)).collect::<Vec<_>>();

    // Old installers in Downloads (top level only).
    let downloads = home.join("Downloads");
    let mut installers: Vec<&FileRec> = Vec::new();
    let installer_files: Vec<FileRec> = std::fs::read_dir(&downloads)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    let m = std::fs::symlink_metadata(&p).ok()?;
                    (m.is_file() && installer_like(pc, &p)).then(|| FileRec { bytes: on_disk(&m), len: m.len(), modified: m.modified().ok(), path: p })
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
            why: if pc {
                format!("Installers more than {} days old. The programs they installed stay installed.", lim.installer_days)
            } else {
                format!("Disk images and installers more than {} days old. The apps they installed stay installed.", lim.installer_days)
            },
            bytes: installers.iter().map(|f| f.bytes).sum(),
            items: show(&paths),
            count: paths.len(),
            can_trash: true,
        });
        allowed.insert("installers".into(), paths);
    }

    // Copies: every file but the kept one.
    let extra: Vec<&FileRec> = dups.iter().flat_map(|g| g.iter().skip(1)).filter(|f| offerable_for(pc, home, &f.path)).collect();
    if !extra.is_empty() {
        let paths: Vec<PathBuf> = extra.iter().map(|f| f.path.clone()).collect();
        let items = dups
            .iter()
            .filter(|g| g.iter().skip(1).any(|f| offerable_for(pc, home, &f.path)))
            .take(8)
            .map(|g| format!("{} (keeps {})", g.iter().skip(1).map(|f| tilde_for(pc, home, &f.path)).collect::<Vec<_>>().join(", "), tilde_for(pc, home, &g[0].path)))
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
            why: format!("{} rebuilds or downloads these again when needed (the first build or install after may be slower).", if !pc && (label.starts_with("Xcode") || label.starts_with("Simulator")) { "Xcode" } else { "The tool" }),
            bytes: *bytes,
            items: vec![tilde_for(pc, home, dir)],
            count: paths.len(),
            can_trash: true,
        });
        allowed.insert(id, paths);
    }

    if scan.app_caches >= 200 << 20 {
        suggestions.push(Suggestion {
            id: "app-caches".into(),
            title: if pc { "Temporary files".into() } else { "App caches".into() },
            why: if pc {
                "Windows and programs leave these here and clear most of them on their own. Settings → System → Storage → Temporary files (Storage Sense) is the safe way to clear the rest.".into()
            } else {
                "Apps keep these to load faster and refill them right away; macOS clears them itself when space runs low, so removing them by hand rarely helps.".into()
            },
            bytes: scan.app_caches,
            items: vec![if pc { "%TEMP%".into() } else { "~/Library/Caches".into() }],
            count: 1,
            can_trash: false,
        });
    }

    // Big files, biggest first; old ones get their own line too.
    let dup_extra: HashSet<&Path> = extra.iter().map(|f| f.path.as_path()).collect();
    let mut bigs: Vec<&FileRec> = scan.files.iter().filter(|f| f.bytes >= lim.big && offerable_for(pc, home, &f.path)).collect();
    bigs.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.path.cmp(&b.path)));
    let big: Vec<BigFile> = bigs
        .iter()
        .take(25)
        .enumerate()
        .map(|(i, f)| {
            let id = format!("file-{i}");
            allowed.insert(id.clone(), vec![f.path.clone()]);
            BigFile { id, name: f.path.file_name().and_then(|n| n.to_str()).unwrap_or("").into(), path: tilde_for(pc, home, &f.path), bytes: f.bytes, days_old: days_since(f.modified, now) }
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

    let mut folders: Vec<Sized> = scan.folders.iter().filter(|f| f.2 > 0).map(|(n, p, b)| Sized { name: n.clone(), path: tilde_for(pc, home, p), bytes: *b }).collect();
    if scan.app_caches > 0 {
        folders.push(Sized { name: if pc { "Temporary files".into() } else { "App caches".into() }, path: if pc { "%TEMP%".into() } else { "~/Library/Caches".into() }, bytes: scan.app_caches });
    }
    for (label, dir, bytes) in &scan.caches {
        folders.push(Sized { name: label.clone(), path: tilde_for(pc, home, dir), bytes: *bytes });
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
    protected_for(false, home, p)
}

/// Paths a PC never moves to the Recycle Bin: Windows and its programs, other users' places, the user's own top folders.
/// Compared as lower-case text with `/` between the parts, because Windows ignores case and accepts both slashes.
pub fn protected_pc(home: &Path, p: &Path) -> bool {
    let f = norm(p);
    let absolute = f.starts_with('/') || (f.len() >= 3 && f.as_bytes()[1] == b':' && f.as_bytes()[2] == b'/');
    if !absolute || f.split('/').any(|c| c == "..") {
        return true;
    }
    let f = f.trim_end_matches('/').to_string();
    // A drive's root.
    if f.len() == 2 && f.as_bytes()[1] == b':' || f.is_empty() {
        return true;
    }
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_lowercase();
    for root in ["windows", "program files", "program files (x86)", "programdata", "$recycle.bin", "system volume information", "recovery"] {
        let r = format!("{drive}/{root}");
        if f == r || f.starts_with(&format!("{r}/")) {
            return true;
        }
    }
    let h = norm(home);
    let h = h.trim_end_matches('/');
    ["", "desktop", "documents", "downloads", "pictures", "videos", "music", "appdata", "onedrive", "appdata/local", "appdata/roaming"]
        .iter()
        .any(|n| f == if n.is_empty() { h.to_string() } else { format!("{h}/{n}") })
}

/// `protected` for a Mac (`pc` false) or a PC.
pub fn protected_for(pc: bool, home: &Path, p: &Path) -> bool {
    if pc {
        return protected_pc(home, p);
    }
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
    let pc = runner.pc();
    let paths: Vec<PathBuf> = paths.iter().filter(|p| !protected_for(pc, home, p) && std::fs::symlink_metadata(p).is_ok()).cloned().collect();
    if paths.is_empty() {
        return Trashed { moved: 0, bytes: 0, undo: None, error: Some("Those files aren't there any more.".into()) };
    }
    let sizes: Vec<u64> = paths.iter().map(|p| std::fs::symlink_metadata(p).map(|m| if m.is_dir() { size_dir(p, &|_| false, u64::MAX, &mut Vec::new(), Instant::now() + Duration::from_secs(5), &mut 200_000).bytes } else { on_disk(&m) }).unwrap_or(0)).collect();
    if pc {
        return recycle(runner, &paths, &sizes).await;
    }
    let out = match runner.run(&Command::Osa { script: TRASH, args: paths.iter().map(|p| p.display().to_string()).collect() }).await {
        Ok(o) => o,
        Err(e) => return Trashed { moved: 0, bytes: 0, undo: None, error: Some(e.text("Finder")) },
    };
    let (back, bytes, failed) = put_back_args(&paths, &sizes, &out);
    let moved = back.len() / 3;
    let undo = (moved > 0).then(|| macctl::keep_undo(Undo::Cmd(Command::Osa { script: PUT_BACK, args: back })));
    Trashed { moved, bytes, undo, error: failed.first().cloned() }
}

/// A PC's way: the Recycle Bin, one line back per file (`ok` or `!` and why), and an Undo that restores what went.
async fn recycle(runner: &dyn Runner, paths: &[PathBuf], sizes: &[u64]) -> Trashed {
    let out = match runner.run(&Command::Win(WinOp::Recycle(paths.to_vec()))).await {
        Ok(o) => o,
        Err(e) => return Trashed { moved: 0, bytes: 0, undo: None, error: Some(e.text_for("The Recycle Bin", true)) },
    };
    let mut moved: Vec<PathBuf> = Vec::new();
    let mut bytes = 0;
    let mut failed = Vec::new();
    for (i, line) in out.lines().map(str::trim).filter(|l| !l.is_empty()).enumerate() {
        let Some(p) = paths.get(i) else { break };
        match line.strip_prefix('!') {
            Some(e) => failed.push(format!("{}: {}", p.file_name().and_then(|n| n.to_str()).unwrap_or("?"), e.trim())),
            None => {
                moved.push(p.clone());
                bytes += sizes.get(i).copied().unwrap_or(0);
            }
        }
    }
    let undo = (!moved.is_empty()).then(|| macctl::keep_undo(Undo::Cmd(Command::Win(WinOp::Restore(moved.clone())))));
    Trashed { moved: moved.len(), bytes, undo, error: failed.first().cloned() }
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
    if cfg!(windows) {
        return std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"));
    }
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// The storage card's "Move to Trash" (an id from the latest scan, never a path).
#[tauri::command]
pub async fn upkeep_trash(scan_id: String, id: String) -> AppResult<Trashed> {
    let paths = take_allowed(&scan_id, &id).ok_or_else(|| AppError::msg("That list is out of date; ask BYTE to check the storage again."))?;
    if cfg!(windows) {
        return Ok(trash(&crate::pcctl::WinRunner, &home(), &paths).await);
    }
    Ok(trash(&MacRunner, &home(), &paths).await)
}

/// The storage card's "Show in Finder" (an id from a scan, like the Trash buttons).
#[tauri::command]
pub async fn upkeep_reveal(scan_id: String, id: String) -> AppResult<()> {
    let path = SCANS.lock().ok().and_then(|s| s.iter().find(|(sid, _)| *sid == scan_id).and_then(|(_, m)| m.get(&id).and_then(|p| p.first().cloned())));
    let path = path.ok_or_else(|| AppError::msg("That list is out of date."))?;
    if cfg!(windows) {
        return crate::pcctl::WinRunner.run(&Command::Win(WinOp::Reveal(path))).await.map(|_| ()).map_err(|e| AppError::msg(e.text_for("File Explorer", true)));
    }
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
    if runner.pc() {
        return health_pc(runner, coach, now).await;
    }
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

// ------------------------------------------------------- a PC's health

/// What a PC's `Snapshot` operation printed.
#[derive(Debug, Default, PartialEq)]
pub struct PcSnapshot {
    pub mem_total: u64,
    pub mem_free: u64,
    pub disk: Option<(u64, u64)>,
    /// Unix seconds.
    pub boot: Option<u64>,
    pub os: String,
    /// (percent, on mains, seconds left): only a laptop has this.
    pub power: Option<(u8, bool, Option<u64>)>,
}

/// The value after `key=` on a line of a `key=value` listing.
fn kv<'a>(out: &'a str, key: &str) -> Option<&'a str> {
    out.lines().find_map(|l| l.trim().strip_prefix(key).and_then(|r| r.strip_prefix('=')))
}

fn two_numbers(v: &str) -> Option<(u64, u64)> {
    let (a, b) = v.split_once(';')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

pub fn parse_pc_snapshot(out: &str) -> PcSnapshot {
    let (mem_total, mem_free) = kv(out, "mem").and_then(two_numbers).unwrap_or((0, 0));
    let power = kv(out, "power").and_then(|v| {
        let mut f = v.split(';');
        let pct: u8 = f.next()?.trim().parse().ok()?;
        let mains = f.next()? .trim() == "1";
        let left = f.next().and_then(|l| l.trim().parse::<i64>().ok()).filter(|l| *l >= 0).map(|l| l as u64);
        Some((pct.min(100), mains, left))
    });
    PcSnapshot { mem_total, mem_free, disk: kv(out, "disk").and_then(two_numbers).filter(|(t, _)| *t > 0), boot: kv(out, "boot").and_then(|b| b.trim().parse().ok()), os: kv(out, "os").unwrap_or("").trim().to_string(), power }
}

/// What a PC's `Security` operation printed.
#[derive(Debug, Default, PartialEq)]
pub struct PcSecurity {
    /// (virus protection on, real-time protection on, running mode, days since its definitions were updated).
    pub defender: Option<(bool, bool, String, u32)>,
    /// (profile, on) for the Domain, Private and Public networks.
    pub firewall: Vec<(String, bool)>,
    /// Windows' code for the system drive's encryption: 1 is on, 0 off.
    pub bitlocker: Option<u32>,
    pub update: Option<chrono::NaiveDate>,
    pub reboot: bool,
    /// (designed capacity, what it holds fully charged now).
    pub battery: Option<(u64, u64)>,
    pub cycles: Option<u32>,
}

pub fn parse_pc_security(out: &str) -> PcSecurity {
    let truth = |s: &str| s.trim().eq_ignore_ascii_case("true");
    PcSecurity {
        defender: kv(out, "defender").and_then(|v| {
            let f: Vec<&str> = v.split(';').collect();
            (f.len() >= 4).then(|| (truth(f[0]), truth(f[1]), f[2].trim().to_string(), f[3].trim().parse().unwrap_or(0)))
        }),
        firewall: kv(out, "firewall").map(|v| v.split(',').filter_map(|p| p.split_once(':')).map(|(n, on)| (n.trim().to_string(), truth(on))).collect()).unwrap_or_default(),
        bitlocker: kv(out, "bitlocker").and_then(|v| v.trim().parse().ok()),
        update: kv(out, "update").and_then(|v| chrono::NaiveDate::parse_from_str(v.trim(), "%Y-%m-%d").ok()),
        reboot: kv(out, "reboot").is_some_and(|v| v.trim() == "1"),
        battery: kv(out, "battery").and_then(two_numbers).filter(|(d, f)| *d > 0 && *f > 0),
        cycles: kv(out, "cycles").and_then(|v| v.trim().parse().ok()),
    }
}

/// A program's file name without its ".exe" (any case), keeping the rest as written.
fn exe_stem(exe: &str) -> &str {
    match exe.len().checked_sub(4).and_then(|at| exe.get(at..).map(|tail| (at, tail))) {
        Some((at, tail)) if tail.eq_ignore_ascii_case(".exe") => &exe[..at],
        _ => exe,
    }
}

/// A program name as people know it ("chrome.exe" is Google Chrome).
pub fn pretty_proc(exe: &str) -> String {
    let own = exe_stem(exe);
    if let Some(known) = known_program(own) {
        return known.to_string();
    }
    let mut c = own.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The name people know a program by, for the ones BYTE recognizes (file name without ".exe", any case).
fn known_program(stem: &str) -> Option<&'static str> {
    Some(match stem.to_lowercase().as_str() {
        "chrome" => "Google Chrome",
        "msedge" => "Microsoft Edge",
        "firefox" => "Firefox",
        "brave" => "Brave",
        "opera" => "Opera",
        "discord" => "Discord",
        "spotify" => "Spotify",
        "teams" | "ms-teams" => "Microsoft Teams",
        "onedrive" => "OneDrive",
        "steam" | "steamwebhelper" => "Steam",
        "code" => "Visual Studio Code",
        "llama-server" | "llama-server-vulkan" | "llama-server-cpu" => "BYTE's AI engine",
        "byte" => "BYTE",
        _ => return None,
    })
}

/// Whether the health card may offer to close this program: never Windows itself, its services, the shell, a terminal,
/// the web view other programs share, or BYTE and its engine.
pub fn quittable_pc(exe: &str) -> bool {
    let low = exe_stem(exe).to_lowercase();
    let stem = low.as_str();
    const NEVER: &[&str] = &[
        "explorer", "byte", "llama-server", "llama-server-vulkan", "llama-server-cpu", "dwm", "svchost", "system", "csrss", "winlogon", "services", "lsass", "searchhost",
        "startmenuexperiencehost", "shellexperiencehost", "textinputhost", "applicationframehost", "systemsettings", "taskmgr", "msedgewebview2", "sihost", "ctfmon",
        "fontdrvhost", "wininit", "smss", "registry", "memcompression", "conhost", "powershell", "pwsh", "cmd", "windowsterminal",
    ];
    !NEVER.contains(&stem)
}

/// A PC's `Processes` lines (`name|cpu|bytes|has a window`) as the card's list, busiest first (or biggest, when memory is what
/// is short): friendly names, and `app` only for a program with a window that is safe to ask to close. The second part pairs
/// each such friendly name with its real program name.
pub fn pc_procs(out: &str, by_memory: bool) -> (Vec<Proc>, Vec<(String, String)>) {
    let mut rows: Vec<(&str, f32, u64, &str)> = out
        .lines()
        .filter_map(|line| {
            let mut f = line.trim().split('|');
            let (exe, cpu, mem, win) = (f.next()?, f.next()?, f.next()?, f.next()?);
            Some((exe, cpu.parse().ok()?, mem.parse().ok()?, win))
        })
        .collect();
    if by_memory {
        rows.sort_by(|a, b| b.2.cmp(&a.2));
    } else {
        rows.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.2.cmp(&a.2)));
    }
    let mut procs = Vec::new();
    let mut quit = Vec::new();
    for (exe, cpu, mem, win) in rows {
        let name = pretty_proc(exe);
        let app = (win == "1" && quittable_pc(exe)).then(|| name.clone());
        if let Some(a) = &app {
            quit.push((a.clone(), exe.to_string()));
        }
        procs.push(Proc { name, cpu, mem: size_text(mem), app });
    }
    (procs, quit)
}

/// Program name behind each quittable friendly name on a card, by card id.
static PC_QUIT: Lazy<Mutex<Vec<(String, Vec<(String, String)>)>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// A `check` whose settings link is a Windows Settings page.
fn check_pc(label: &str, value: impl Into<String>, level: Level, tip: impl Into<String>, pane: Option<&str>) -> Check {
    let (settings, settings_label) = match pane {
        Some(p) => (Some(crate::pcctl::settings_uri(p)), Some(crate::pcctl::pane_label(p).to_string())),
        None => (None, None),
    };
    Check { label: label.into(), value: value.into(), level, tip: tip.into(), settings, settings_label }
}

fn ago(days: u64) -> String {
    match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        n => format!("{n} days ago"),
    }
}

/// The health card for a PC: the same questions as the Mac's, answered from what Windows says.
async fn health_pc(runner: &dyn Runner, coach: bool, now: SystemTime) -> Health {
    let mut checks = Vec::new();
    let now_s = now.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let today = chrono::DateTime::<chrono::Local>::from(now).date_naive();
    let snap = runner.run(&Command::Win(WinOp::Snapshot)).await.map(|o| parse_pc_snapshot(&o)).unwrap_or_default();
    // Disk.
    if let Some((total, free)) = snap.disk {
        let pct = free as f64 * 100.0 / total as f64;
        let (level, tip) = if pct < 5.0 {
            (Level::Bad, "Almost full: Windows needs free space to update and run smoothly. Ask BYTE \"what's taking up space\".")
        } else if pct < 12.0 {
            (Level::Warn, "Getting full; a PC slows down when space runs low. Ask BYTE \"what's taking up space\".")
        } else {
            (Level::Ok, "")
        };
        checks.push(check_pc("Storage", format!("{} free of {} ({pct:.0}%)", size_text(free), size_text(total)), level, tip, (level != Level::Ok).then_some("storage")));
    }
    // Memory.
    if snap.mem_total > 0 {
        let free = (snap.mem_free * 100 / snap.mem_total) as u8;
        let (level, tip) = if free < 10 {
            (Level::Bad, "Memory is nearly full, so Windows swaps to disk and slows down. Quit programs you're not using (browser tabs count).")
        } else if free < 25 {
            (Level::Warn, "Memory is getting tight. Quitting unused programs and tabs helps.")
        } else {
            (Level::Ok, "")
        };
        checks.push(check_pc("Memory", format!("{free}% free"), level, tip, None));
    }
    // Busy programs.
    let mut procs = Vec::new();
    let mut quit = Vec::new();
    if coach {
        if let Ok(out) = runner.run(&Command::Win(WinOp::Processes)).await {
            let tight = snap.mem_total > 0 && snap.mem_free * 100 / snap.mem_total < 25;
            let (all, q) = pc_procs(&out, tight);
            procs = all.into_iter().take(8).collect();
            quit = q.into_iter().filter(|(d, _)| procs.iter().any(|p| p.app.as_deref() == Some(d.as_str()))).collect();
        }
        let hot: Vec<&Proc> = procs.iter().filter(|p| p.cpu >= 50.0).collect();
        if hot.is_empty() {
            checks.push(check_pc("Processor", "Nothing is working the processor hard right now", Level::Ok, "", None));
        } else {
            let names: Vec<String> = hot.iter().map(|p| format!("{} ({:.0}%)", p.name, p.cpu)).collect();
            checks.push(check_pc("Processor", format!("Busy: {}", names.join(", ")), Level::Warn, "These are using a lot of processor time, which slows other programs, warms the PC and drains the battery. Quit what you're not using, or ask the busy program to stop what it's doing.", None));
        }
    }
    // The slow, read-only reading of Windows Security, updates and battery wear; only when it can matter.
    let sec = if !coach || snap.power.is_some() { runner.run(&Command::Win(WinOp::Security)).await.map(|o| parse_pc_security(&o)).unwrap_or_default() } else { PcSecurity::default() };
    // Battery.
    if let Some((pct, mains, left)) = snap.power {
        let left = left.filter(|_| !mains).map(|s| format!(", about {}h {:02}m left", s / 3600, s % 3600 / 60)).unwrap_or_default();
        checks.push(check_pc("Battery", format!("{pct}%, {}{left}", if mains { "plugged in" } else { "on battery" }), Level::Info, "", None));
        if let Some((design, full)) = sec.battery {
            let max = (full * 100 / design).min(100);
            let value = format!("Holds {max}% of its original charge{}", sec.cycles.map(|n| format!(", {n} charge cycles")).unwrap_or_default());
            let (level, tip) = if max < 50 {
                (Level::Bad, "The battery is badly worn. A replacement from the maker or a repair shop will help.")
            } else if max < 80 {
                (Level::Warn, "The battery holds noticeably less than when new. Battery saver, and keeping it between 20% and 80% charged, slows further wear.")
            } else {
                (Level::Ok, "")
            };
            checks.push(check_pc("Battery health", value, level, tip, (level != Level::Ok).then_some("battery")));
        }
    }
    // Restarted lately?
    if let Some(boot) = snap.boot {
        let days = now_s.saturating_sub(boot) / 86_400;
        let (level, tip) = if days >= 14 { (Level::Warn, "A restart now and then clears out memory and finishes updates. Choose Restart: Shut down keeps part of Windows running (Fast startup).") } else { (Level::Ok, "") };
        checks.push(check_pc("Last restart", ago(days), level, tip, None));
    }
    if !coach {
        if !snap.os.is_empty() {
            checks.push(check_pc("Windows", snap.os.clone(), Level::Info, "", Some("windows update")));
        }
        if sec.reboot {
            checks.push(check_pc("Windows Update", "A restart is waiting to finish installing updates", Level::Warn, "Restart when it suits you, so the updates take effect.", Some("windows update")));
        } else if let Some(d) = sec.update {
            let days = (today - d).num_days().max(0) as u64;
            let (level, tip) = if days > 60 { (Level::Warn, "Windows hasn't installed an update in over two months. Open Windows Update and check for updates.") } else { (Level::Ok, "") };
            checks.push(check_pc("Windows Update", format!("Last installed {}", ago(days).to_lowercase()), level, tip, (level != Level::Ok).then_some("windows update")));
        }
        match &sec.defender {
            Some((_, _, mode, _)) if mode.starts_with("Passive") || mode.starts_with("SxS") => checks.push(check_pc("Virus protection", "Another antivirus program is in charge", Level::Info, "", Some("security"))),
            Some((on, realtime, _, age)) if !on || !realtime => {
                let _ = age;
                checks.push(check_pc("Virus protection", "Off", Level::Bad, "Windows Security's virus protection is off, so this PC isn't protected. Open Windows Security and turn it on, or install an antivirus.", Some("security")));
            }
            Some((_, _, _, age)) if *age > 7 => checks.push(check_pc("Virus protection", format!("On, but its definitions are {age} days old"), Level::Warn, "Open Windows Update so Windows Security can fetch new definitions.", Some("security"))),
            Some(_) => checks.push(check_pc("Virus protection", "On and up to date", Level::Ok, "", None)),
            None => checks.push(check_pc("Virus protection", "BYTE couldn't read it (another antivirus may be installed)", Level::Info, "", Some("security"))),
        }
        if !sec.firewall.is_empty() {
            let off: Vec<&str> = sec.firewall.iter().filter(|(_, on)| !on).map(|(n, _)| n.as_str()).collect();
            if off.is_empty() {
                checks.push(check_pc("Firewall", "On", Level::Ok, "", None));
            } else if off.contains(&"Public") {
                checks.push(check_pc("Firewall", "Off for public networks", Level::Warn, "Worth turning on if you use public Wi-Fi (Windows Security → Firewall & network protection).", Some("security")));
            } else {
                checks.push(check_pc("Firewall", format!("Off for {} networks", off.join(" and ").to_lowercase()), Level::Info, "", Some("security")));
            }
        }
        match sec.bitlocker {
            Some(1) => checks.push(check_pc("Disk encryption", "On", Level::Ok, "", None)),
            Some(0) => checks.push(check_pc("Disk encryption", "Off", Level::Info, "Encryption keeps your files unreadable if the PC is lost or stolen. Windows offers it as Device encryption or BitLocker, depending on the edition.", Some("encryption"))),
            _ => {}
        }
        checks.push(check_pc("Backups", "BYTE can't see Windows Backup or File History", Level::Info, "Make sure your important files are also in OneDrive or on another drive.", Some("backup")));
    }
    checks.sort_by_key(|c| c.level);
    let id = uuid::Uuid::new_v4().simple().to_string();
    let quittable: Vec<String> = procs.iter().filter_map(|p| p.app.clone()).collect();
    if let (Ok(mut q), Ok(mut names)) = (QUITTABLE.lock(), PC_QUIT.lock()) {
        q.push((id.clone(), quittable));
        names.push((id.clone(), quit));
        let (n, m) = (q.len(), names.len());
        if n > 5 {
            q.drain(..n - 5);
        }
        if m > 5 {
            names.drain(..m - 5);
        }
    }
    Health { id, title: if coach { "How your PC is doing".into() } else { "PC check-up".into() }, checks, procs }
}

/// The health card's "Quit" button (only apps that card listed).
#[tauri::command]
pub async fn upkeep_quit(card: String, app: String) -> AppResult<bool> {
    let ok = QUITTABLE.lock().ok().is_some_and(|q| q.iter().any(|(id, apps)| *id == card && apps.contains(&app)));
    if !ok {
        return Err(AppError::msg("BYTE can only quit the apps on that card."));
    }
    if cfg!(windows) {
        // The card knows the program by its friendly name; Windows needs the real one.
        let exe = PC_QUIT.lock().ok().and_then(|q| q.iter().find(|(id, _)| *id == card).and_then(|(_, m)| m.iter().find(|(d, _)| *d == app).map(|(_, e)| e.clone())));
        let exe = exe.ok_or_else(|| AppError::msg("BYTE can only quit the apps on that card."))?;
        return crate::pcctl::WinRunner.run(&Command::Win(WinOp::Quit(exe))).await.map(|asked| asked.trim() != "0").map_err(|e| AppError::msg(e.text_for(&app, true)));
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

/// One thing that starts with Windows (`StartupList` prints these).
#[derive(Debug, Clone, PartialEq)]
pub struct StartupItem {
    /// `user` or `machine`.
    pub scope: String,
    /// `run`, `run32` or `folder`.
    pub kind: String,
    /// The registry value or file name, exactly as Windows keeps it (what the switch needs).
    pub name: String,
    pub command: String,
    pub on: bool,
}

pub fn parse_startup(out: &str) -> Vec<StartupItem> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim_end_matches(['\r', '\n']).split('\t').collect();
            (f.len() == 5 && !f[2].trim().is_empty() && matches!(f[1], "run" | "run32" | "folder") && matches!(f[0], "user" | "machine"))
                .then(|| StartupItem { scope: f[0].into(), kind: f[1].into(), name: f[2].trim().into(), command: f[3].trim().into(), on: f[4].trim() != "0" })
        })
        .collect()
}

/// The program file in a startup command: `"C:\x\a.exe" -silent` and `C:\x\a.exe /background` are both `a.exe`; a shortcut's
/// target is already just a path. Empty when there is no program in it.
pub fn exe_of_command(cmd: &str) -> String {
    let cmd = cmd.trim();
    let path = if let Some(rest) = cmd.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else if let Some(i) = cmd.to_lowercase().find(".exe") {
        &cmd[..i + 4]
    } else {
        cmd.split_whitespace().next().unwrap_or("")
    };
    path.rsplit(['\\', '/']).next().unwrap_or("").trim().to_string()
}

/// What to call a startup entry: the program's well-known name when BYTE has one; else the entry's own name with the
/// technical parts taken off (`electron.app.CurseForge` is CurseForge, `Edge…_A1306…` is Edge).
pub fn startup_title(it: &StartupItem) -> String {
    let exe = exe_of_command(&it.command);
    if let Some(k) = known_program(exe_stem(&exe)) {
        return k.to_string();
    }
    let name = it.name.trim_end_matches(".lnk").trim_end_matches(".LNK");
    if name.contains(char::is_whitespace) {
        return name.to_string();
    }
    // A long hex tail after an underscore is an id, not a name.
    let name = match name.rsplit_once('_') {
        Some((head, tail)) if tail.len() >= 16 && tail.chars().all(|c| c.is_ascii_hexdigit()) => head,
        _ => name,
    };
    if name.contains('.') {
        let generic = ["app", "electron", "com", "org", "io", "squirrel", "exe"];
        let best = name.split('.').rev().find(|seg| !seg.is_empty() && !seg.chars().all(|c| c.is_ascii_digit()) && !generic.contains(&seg.to_lowercase().as_str()));
        if let Some(b) = best {
            return b.to_string();
        }
    }
    name.to_string()
}

/// The startup entry a name means (the entry's name, its friendly name or its program, exact first, then contained).
pub fn match_startup<'a>(items: &'a [StartupItem], want: &str) -> Option<&'a StartupItem> {
    let w = want.trim().to_lowercase();
    if w.len() < 2 {
        return None;
    }
    let keys = |it: &StartupItem| -> Vec<String> {
        let exe = exe_of_command(&it.command);
        vec![it.name.to_lowercase(), startup_title(it).to_lowercase(), exe_stem(&exe).to_lowercase()]
    };
    items.iter().find(|it| keys(it).iter().any(|k| *k == w)).or_else(|| items.iter().find(|it| keys(it).iter().any(|k| !k.is_empty() && (k.contains(&w) || (k.len() >= 4 && w.contains(k.as_str()))))))
}

/// Why BYTE leaves an entry alone: Windows and drivers need theirs, and BYTE's own is in BYTE's Settings.
pub fn protected_startup(it: &StartupItem) -> Option<&'static str> {
    let cmd = it.command.to_lowercase().replace('/', "\\");
    let exe = exe_of_command(&it.command).to_lowercase();
    if cmd.contains("\\windows\\") || cmd.starts_with("%windir%") || cmd.starts_with("%systemroot%") || exe.starts_with("securityhealth") {
        Some("it belongs to Windows itself or a driver")
    } else if exe_stem(&exe) == "byte" {
        Some("that one is BYTE's own; it's in BYTE's Settings")
    } else {
        None
    }
}

/// One installed program (`Programs` prints these).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// `reg` (an installer's entry) or `appx` (a Microsoft Store app).
    pub source: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub size_kb: u64,
}

pub fn parse_programs(out: &str) -> Vec<Program> {
    let mut seen = std::collections::HashSet::new();
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim_end_matches(['\r', '\n']).split('\t').collect();
            if f.len() != 6 || !matches!(f[0], "reg" | "appx") || !crate::pcctl::program_id_ok(f[1]) || f[2].trim().is_empty() {
                return None;
            }
            // The same program is often registered in two views of the registry.
            seen.insert((f[2].trim().to_lowercase(), f[3].trim().to_string())).then(|| Program {
                source: f[0].into(),
                id: f[1].into(),
                name: f[2].trim().into(),
                version: f[3].trim().into(),
                publisher: f[4].trim().into(),
                size_kb: f[5].trim().parse().unwrap_or(0),
            })
        })
        .collect()
}

/// What to call a program: a Store app's name is `Maker.App`, so it is the last part.
pub fn program_title(p: &Program) -> String {
    if p.source == "appx" && p.name.contains('.') {
        if let Some(last) = p.name.rsplit('.').find(|s| !s.is_empty() && !s.chars().all(|c| c.is_ascii_digit())) {
            return last.to_string();
        }
    }
    p.name.clone()
}

/// The programs a name means: an exact name if there is one, else every program whose name contains it.
pub fn match_programs<'a>(items: &'a [Program], want: &str) -> Vec<&'a Program> {
    let w = want.trim().to_lowercase();
    if w.len() < 2 {
        return Vec::new();
    }
    let exact: Vec<&Program> = items.iter().filter(|p| p.name.to_lowercase() == w || program_title(p).to_lowercase() == w).collect();
    if !exact.is_empty() {
        return exact;
    }
    items.iter().filter(|p| [p.name.to_lowercase(), program_title(p).to_lowercase()].iter().any(|k| k.contains(&w) || (k.len() >= 4 && w.contains(k.as_str())))).collect()
}

/// Why BYTE won't uninstall a program: Windows' own parts, the runtimes other programs run on, drivers and hardware software,
/// the user's antivirus, and BYTE itself. (Windows' list leaves out system components already; this is the second wall.)
pub fn refuse_program(p: &Program) -> Option<&'static str> {
    let n = p.name.to_lowercase();
    let t = program_title(p).to_lowercase();
    if n == "byte" || t == "byte" {
        return Some("that's BYTE itself; it's removed from Windows' own Settings → Apps if you ever want to");
    }
    if p.source == "appx" {
        const PARTS: &[&str] = &["microsoft.windows", "microsoft.vclibs", "microsoft.ui.xaml", "microsoft.net", "microsoft.desktopappinstaller", "microsoft.store", "microsoft.microsoftedge", "microsoft.directx", "microsoft.services"];
        if PARTS.iter().any(|x| n.starts_with(x)) {
            return Some("it's part of Windows that other apps need");
        }
        return None;
    }
    const PREFIXES: &[&str] = &["microsoft visual c++", "microsoft .net", ".net ", "microsoft windows", "windows ", "microsoft edge", "microsoft update health", "directx", "vulkan run time", "intel(r)", "intel®", "nvidia graphics", "nvidia physx", "nvidia hd audio", "amd software", "amd chipset", "realtek"];
    const WORDS: &[&str] = &["webview2", "driver", "chipset", "firmware", "bluetooth", "redistributable", "runtime"];
    if PREFIXES.iter().any(|x| n.starts_with(x)) || WORDS.iter().any(|w| n.contains(w)) {
        return Some("it's part of Windows, a driver or a runtime that other programs and the PC need");
    }
    if ["antivirus", "defender", "security", "malwarebytes"].iter().any(|w| n.contains(w)) {
        return Some("it protects this PC, so BYTE leaves it for you to remove yourself");
    }
    None
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
    if cfg!(windows) {
        run_with(turn, question, &crate::pcctl::WinRunner, &home(), SystemTime::now(), cancel, send).await
    } else {
        run_with(turn, question, &MacRunner, &home(), SystemTime::now(), cancel, send).await
    }
}

fn health_notes(h: &Health) -> String {
    let mut lines: Vec<String> = h.checks.iter().map(|c| format!("- {}: {} [{:?}]{}", c.label, c.value, c.level, if c.tip.is_empty() { String::new() } else { format!(" — {}", c.tip) })).collect();
    if !h.procs.is_empty() {
        lines.push(format!("Busiest programs now: {}", h.procs.iter().take(6).map(|p| format!("{} {:.0}% CPU, {} memory", p.name, p.cpu, p.mem)).collect::<Vec<_>>().join("; ")));
    }
    lines.join("\n")
}

/// "What opens when Windows starts": the list, as a card.
async fn startup_items_pc(turn: &Turn<'_>, runner: &dyn Runner, id: String, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    send(ChatEvent::ToolCall { id: id.clone(), name: "mac_login_items".into(), args: json!({ "app": "Windows", "what": "List what opens when Windows starts" }) })?;
    let items = match runner.run(&Command::Win(WinOp::StartupList)).await {
        Ok(o) => parse_startup(&o),
        Err(e) => {
            let msg = e.text_for("Windows", true);
            send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
            return Ok(Some((SourceBook::default(), format!("BYTE couldn't read the startup list: {msg}"))));
        }
    };
    let on = items.iter().filter(|i| i.on).count();
    send(ChatEvent::ToolResult { id, ok: true, summary: format!("{on} open at startup") })?;
    turn.log.record("mac_login_items", &json!({ "count": items.len() }), true, &format!("{on} on"));
    let mut sorted: Vec<&StartupItem> = items.iter().collect();
    sorted.sort_by_key(|i| (!i.on, startup_title(i).to_lowercase()));
    let mut checks: Vec<Check> = sorted
        .iter()
        .map(|i| {
            let exe = exe_of_command(&i.command);
            let place = if i.scope == "machine" { ", for everyone on this PC" } else { "" };
            let tip = match protected_startup(i) {
                Some(why) => format!("BYTE leaves this one alone: {why}."),
                None => String::new(),
            };
            check_pc(&startup_title(i), format!("{}{}{place}", if i.on { "Opens at startup" } else { "Off" }, if exe.is_empty() { String::new() } else { format!(" ({exe})") }), Level::Info, tip, None)
        })
        .collect();
    checks.push(check_pc("Change these", "Settings → Apps → Startup", Level::Info, "Some apps start in other ways (scheduled tasks, Microsoft Store apps); Windows' own list shows those too.", Some("startup")));
    send(ChatEvent::Health(Health { id: uuid::Uuid::new_v4().simple().to_string(), title: "What opens when Windows starts".into(), checks, procs: vec![] }))?;
    let names = |want: bool| -> String {
        let v: Vec<String> = sorted.iter().filter(|i| i.on == want).map(|i| startup_title(i)).collect();
        if v.is_empty() { "none".into() } else { v.join(", ") }
    };
    Ok(Some((
        SourceBook::default(),
        format!(
            "Open when Windows starts: {}.\nTurned off already: {}.\nSome of the open ones are Windows or drivers (security tray, audio, graphics), which BYTE leaves alone.\n\nList them briefly. The user can say \"stop <app> from opening at startup\" and BYTE will turn it off (with Undo), or use the Startup settings button on the card. Some apps also start through their own settings or the Microsoft Store; Windows' Startup settings page shows those.",
            names(true),
            names(false)
        ),
    )))
}

/// "Uninstall X" on a PC: finds the program in what is installed, asks, and runs the program's own uninstaller. That cannot be
/// undone from here, and the card says so before anything happens.
async fn uninstall_pc(turn: &Turn<'_>, runner: &dyn Runner, id: String, app: String, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let none = SourceBook::default;
    let items = match runner.run(&Command::Win(WinOp::Programs)).await {
        Ok(o) => parse_programs(&o),
        Err(e) => return Ok(Some((none(), format!("BYTE couldn't read the list of installed programs: {}", e.text_for("Windows", true))))),
    };
    let found = match_programs(&items, &app);
    if found.is_empty() {
        return Ok(Some((none(), format!("There's no program called \"{app}\" among the installed programs. Ask the user for its name as shown in Settings → Apps → Installed apps."))));
    }
    if found.len() > 1 {
        let names: Vec<String> = found.iter().take(8).map(|p| program_title(p)).collect();
        return Ok(Some((none(), format!("Several programs match \"{app}\": {}. Ask which one to uninstall.", names.join(", ")))));
    }
    let p = found[0].clone();
    let name = program_title(&p);
    if let Some(why) = refuse_program(&p) {
        return Ok(Some((none(), format!("BYTE won't uninstall {name}: {why}. Say so briefly."))));
    }
    let what = format!("Uninstall {name}");
    send(ChatEvent::ToolCall { id: id.clone(), name: "mac_uninstall".into(), args: json!({ "app": "Windows", "what": what }) })?;
    let mut fields = vec![("Program".to_string(), [name.clone(), p.version.clone()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" "))];
    if !p.publisher.is_empty() && p.source == "reg" {
        fields.push(("From".to_string(), p.publisher.clone()));
    }
    if p.size_kb > 0 {
        fields.push(("Size".to_string(), size_text(p.size_kb * 1024)));
    }
    fields.push((
        "Note".to_string(),
        if p.source == "appx" {
            "This Microsoft Store app is removed for your account, with its saved data. Undo isn't possible; you can install it again from the Microsoft Store."
        } else {
            "The program's own uninstaller runs: you may see its window or a Windows permission prompt. Undo isn't possible here; to get it back, install it again from its website or the Microsoft Store. Your documents aren't touched, but its settings may stay in AppData."
        }
        .to_string(),
    ));
    if !macctl::ask_ok(&what, "Windows", fields, cancel, send).await? {
        send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
        return Ok(Some((none(), format!("The user chose to keep {name}. Nothing was removed. Say so in one sentence."))));
    }
    match runner.run(&Command::Win(WinOp::Uninstall { id: p.id.clone() })).await {
        Ok(out) => {
            let gone = out.trim() == "removed";
            send(ChatEvent::ToolResult { id, ok: true, summary: if gone { "Uninstalled".into() } else { "Uninstaller started".into() } })?;
            turn.log.record("mac_uninstall", &json!({ "name": name, "source": p.source }), true, if gone { "uninstalled" } else { "started" });
            let detail = if gone { "It's no longer installed. To get it back, install it again from its website or the Microsoft Store." } else { "Its uninstaller is open: follow it to finish. To get the program back later, install it again from its website or the Microsoft Store." };
            done_card(send, "Windows", &if gone { format!("Uninstalled {name}") } else { format!("Started the uninstaller for {name}") }, detail, true, None)?;
            Ok(Some((
                none(),
                if gone {
                    format!("Done: {name} is uninstalled. This can't be undone from BYTE; the user can install it again from its website or the Microsoft Store. Say that in one sentence.")
                } else {
                    format!("BYTE started {name}'s own uninstaller; it was still open when BYTE looked. Tell the user to follow its window (and any Windows permission prompt) to finish, and that BYTE can't undo it.")
                },
            )))
        }
        Err(e) => {
            let msg = e.text_for("Windows", true);
            send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
            Ok(Some((none(), format!("BYTE couldn't uninstall {name}: {msg}"))))
        }
    }
}

/// "Stop Spotify from opening at startup": asks first, turns the entry off (it stays installed), and can turn it back on.
async fn startup_remove_pc(turn: &Turn<'_>, runner: &dyn Runner, id: String, name: String, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let none = SourceBook::default;
    let items = runner.run(&Command::Win(WinOp::StartupList)).await.map(|o| parse_startup(&o)).unwrap_or_default();
    let Some(item) = match_startup(&items, &name).cloned() else {
        return Ok(Some((
            none(),
            format!(
                "\"{name}\" isn't in the startup list BYTE can change ({}). Some apps start through their own settings, a scheduled task or the Microsoft Store; tell the user to look in Settings → Apps → Startup.",
                if items.is_empty() { "there are none".to_string() } else { items.iter().map(startup_title).collect::<Vec<_>>().join(", ") }
            ),
        )));
    };
    let title = startup_title(&item);
    if let Some(why) = protected_startup(&item) {
        return Ok(Some((none(), format!("BYTE won't turn off {title}: {why}. Say so briefly."))));
    }
    if !item.on {
        return Ok(Some((none(), format!("{title} is already turned off at startup. Nothing changed. Say so in one sentence."))));
    }
    let what = format!("Stop {title} from opening when Windows starts");
    send(ChatEvent::ToolCall { id: id.clone(), name: "mac_login_remove".into(), args: json!({ "app": "Windows", "what": what }) })?;
    let fields = vec![
        ("App".to_string(), format!("{title} ({})", exe_of_command(&item.command))),
        ("Note".to_string(), "The app stays installed; it just won't open by itself. This is the same switch as Task Manager's Startup tab. Undo turns it back on.".to_string()),
    ];
    if !macctl::ask_ok(&what, "Windows", fields, cancel, send).await? {
        send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
        return Ok(Some((none(), format!("The user chose to keep {title} opening at startup. Nothing changed. Say so in one sentence."))));
    }
    let set = |on| Command::Win(WinOp::StartupSet { kind: item.kind.clone(), name: item.name.clone(), on });
    match runner.run(&set(false)).await {
        Ok(_) => {
            let undo = Some(macctl::keep_undo(Undo::Cmd(set(true))));
            send(ChatEvent::ToolResult { id, ok: true, summary: "Turned off at startup".into() })?;
            turn.log.record("mac_login_remove", &json!({ "name": title }), true, "turned off");
            done_card(send, "Windows", &what, "It stays installed and opens when you open it.", true, undo)?;
            Ok(Some((none(), format!("Done: {title} no longer opens when Windows starts (it's still installed). Undo on the card turns it back on."))))
        }
        Err(e) => {
            let msg = e.text_for("Windows", true);
            send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
            Ok(Some((none(), format!("BYTE couldn't change the startup list: {msg}"))))
        }
    }
}

pub(crate) async fn run_with(turn: &Turn<'_>, question: &str, runner: &dyn Runner, home: &Path, now: SystemTime, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(a) = ask(question) else { return Ok(None) };
    let id = format!("byte_upkeep_{}", uuid::Uuid::new_v4().simple());
    let none = SourceBook::default;
    let pc = runner.pc();
    match &a {
        Ask::Uninstall { app } if pc => return uninstall_pc(turn, runner, id, app.clone(), cancel, send).await,
        Ask::LoginItems if pc => return startup_items_pc(turn, runner, id, send).await,
        Ask::LoginRemove { name } if pc => return startup_remove_pc(turn, runner, id, name.clone(), cancel, send).await,
        _ => {}
    }
    match a {
        Ask::Storage => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_storage".into(), args: json!({ "app": if pc { "File Explorer" } else { "Finder" }, "what": if pc { "Look at what's using space on this PC" } else { "Look at what's using space on this Mac" } }) })?;
            let (total, free) = if pc { crate::system::disk_space_for(home) } else { out_of(runner, "df", &["-k", &home.display().to_string()]).await.as_deref().and_then(parse_df).unwrap_or((0, 0)) };
            let h = home.to_path_buf();
            let (scan, dups) = tokio::task::spawn_blocking(move || {
                let s = scan_for(pc, &h, LIMITS.dup_min, SCAN_TIME, SCAN_ENTRIES);
                let d = duplicates(&s.files, LIMITS.dup_min, Instant::now() + Duration::from_secs(15));
                (s, d)
            })
            .await
            .map_err(|e| AppError::msg(e.to_string()))?;
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let (card, allowed) = storage_card_for(pc, home, &scan, &dups, total, free, LIMITS, now);
            remember_scan(&card.scan_id, allowed);
            let could: u64 = card.suggestions.iter().filter(|s| s.can_trash).map(|s| s.bytes).sum();
            let summary = format!("{} folders sized · {} could be freed", card.folders.len(), size_text(could));
            send(ChatEvent::ToolResult { id, ok: true, summary: summary.clone() })?;
            turn.log.record("mac_storage", &json!({}), true, &summary);
            let mut notes = String::from(if pc {
                "BYTE looked at this PC's storage (the card above shows it; the user can move suggested items to the Recycle Bin from the card, and Undo puts them back).\n"
            } else {
                "BYTE looked at this Mac's storage (the card above shows it; the user can move suggested items to the Trash from the card, and Undo puts them back).\n"
            });
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
            notes.push_str(if pc {
                "BYTE doesn't look inside AppData (apart from caches) or at Windows itself, so Settings → System → Storage can show more (Apps, Temporary files, OneDrive).\n\nSummarize in a few bullets: where the space goes, and the safest things to clear first. Point to the card's buttons; don't tell the user to use command-line tools."
            } else {
                "BYTE doesn't look inside ~/Library (apart from caches) or at macOS itself, so System Settings → General → Storage can show more (System Data, iCloud, Photos).\n\nSummarize in a few bullets: where the space goes, and the safest things to clear first. Point to the card's buttons; don't tell the user to use Terminal commands."
            });
            send(ChatEvent::Storage(card))?;
            Ok(Some((none(), notes)))
        }
        Ask::Coach | Ask::Checkup => {
            let coach = a == Ask::Coach;
            let (app, what) = match (pc, coach) {
                (true, true) => ("Windows", "Check what's slowing the PC or using the battery"),
                (true, false) => ("Windows Security", "Run a PC check-up"),
                (false, true) => ("System", "Check what's slowing the Mac or using the battery"),
                (false, false) => ("System", "Run a Mac check-up"),
            };
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_health".into(), args: json!({ "app": app, "what": what }) })?;
            let h = health(runner, home, coach, now).await;
            let issues = h.checks.iter().filter(|c| c.level <= Level::Warn).count();
            let summary = if issues == 0 { "All good".to_string() } else { format!("{issues} things to look at") };
            send(ChatEvent::ToolResult { id, ok: true, summary: summary.clone() })?;
            turn.log.record("mac_health", &json!({ "coach": coach }), true, &summary);
            let notes = if pc {
                format!(
                    "BYTE checked this PC just now (shown in the card above):\n{}\n\nExplain what matters most first in plain words, then practical fixes. Mention that the card has buttons for Windows settings and for quitting busy programs. Don't suggest PowerShell or command-line steps, registry edits, \"optimizer\" or cleaner programs, or turning off Windows Security.",
                    health_notes(&h)
                )
            } else {
                format!(
                    "BYTE checked this Mac just now (shown in the card above):\n{}\n\nExplain what matters most first in plain words, then practical fixes. Mention that the card has buttons for settings and for quitting busy apps. Don't suggest Terminal commands, cleaner apps or resetting SMC/NVRAM.",
                    health_notes(&h)
                )
            };
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
