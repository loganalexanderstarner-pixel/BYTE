//! Downloads tidy-up: BYTE watches the Downloads folder and, when loose files have piled up, offers to sort them into folders
//! (Pictures, Documents, Installers, Archives, Music, Videos) inside Downloads. It only ever moves files within Downloads, never
//! deletes anything, leaves files that are still downloading or were just changed, and every sort can be undone from the popup
//! (a log of exactly what moved is kept, so Undo puts each file back where it was).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Files changed less than this many seconds ago are left alone (they may still be downloading).
pub const MIN_AGE_SECS: u64 = 600;
/// Loose files that make an offer worthwhile.
pub const OFFER_AT: usize = 10;
pub const UNDO_FILE: &str = "downloads_tidy_undo.json";

pub fn supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

/// The folder a file goes to, by its extension; None leaves it where it is.
pub fn category(name: &str) -> Option<&'static str> {
    let ext = Path::new(name).extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "svg" | "tif" | "tiff" => "Pictures",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt" | "ods" | "csv" | "md" | "epub" => "Documents",
        "exe" | "msi" | "msix" | "appx" | "dmg" | "pkg" | "iso" => "Installers",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" => "Archives",
        "mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac" | "opus" => "Music",
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "wmv" | "m4v" => "Videos",
        _ => return None,
    })
}

/// Names of the folders BYTE sorts into (never sorted again, never counted as loose files).
pub const FOLDERS: &[&str] = &["Pictures", "Documents", "Installers", "Archives", "Music", "Videos"];

const PARTIAL: &[&str] = &["crdownload", "part", "tmp", "download", "opdownload", "partial", "!ut", "aria2"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    /// Seconds since the file last changed.
    pub age_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub name: String,
    pub folder: &'static str,
}

/// The moves to offer: loose, finished, recognised files only.
pub fn plan(entries: &[Entry], min_age: u64) -> Vec<Move> {
    entries
        .iter()
        .filter(|e| !e.is_dir && !e.name.starts_with('.') && !e.name.eq_ignore_ascii_case("desktop.ini") && e.age_secs >= min_age)
        .filter(|e| {
            let ext = Path::new(&e.name).extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            !PARTIAL.contains(&ext.as_str())
        })
        .filter_map(|e| category(&e.name).map(|folder| Move { name: e.name.clone(), folder }))
        .collect()
}

/// "name.ext" to "name (2).ext" until the name is free in `dir`.
pub fn free_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = p.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    (2..10_000).map(|n| format!("{stem} ({n}){ext}")).find(|c| !dir.join(c).exists()).unwrap_or_else(|| name.to_string())
}

/// The loose files of a folder, with how long ago each changed.
pub fn scan(dir: &Path) -> Vec<Entry> {
    let now = std::time::SystemTime::now();
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.filter_map(|e| e.ok())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let age = meta.modified().ok().and_then(|m| now.duration_since(m).ok()).map(|d| d.as_secs()).unwrap_or(0);
            Some(Entry { name: e.file_name().to_string_lossy().into_owned(), is_dir: meta.is_dir(), age_secs: age })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Log {
    pub dir: PathBuf,
    /// (original path, where it went)
    pub moved: Vec<(PathBuf, PathBuf)>,
}

/// Does the moves. Files that cannot be moved are skipped. Returns what was done (the undo log).
pub fn apply(dir: &Path, moves: &[Move]) -> Log {
    let mut log = Log { dir: dir.to_path_buf(), moved: vec![] };
    for m in moves {
        // A name is one file in this folder, never a path.
        if m.name.contains(['/', '\\']) || m.name.contains("..") {
            continue;
        }
        let from = dir.join(&m.name);
        if !from.is_file() {
            continue;
        }
        let target_dir = dir.join(m.folder);
        if std::fs::create_dir_all(&target_dir).is_err() {
            continue;
        }
        let to = target_dir.join(free_name(&target_dir, &m.name));
        if std::fs::rename(&from, &to).is_ok() {
            log.moved.push((from, to));
        }
    }
    log
}

/// Puts every file back where it was. A file that has been moved or deleted since is skipped, and so is one whose old place is now
/// taken (it stays in its folder). Empty sort folders are removed. Returns how many went back.
pub fn undo(log: &Log) -> usize {
    let mut back = 0;
    for (orig, now) in log.moved.iter().rev() {
        if now.is_file() && !orig.exists() && std::fs::rename(now, orig).is_ok() {
            back += 1;
        }
    }
    for f in FOLDERS {
        let p = log.dir.join(f);
        if p.is_dir() && std::fs::read_dir(&p).map(|mut r| r.next().is_none()).unwrap_or(false) {
            let _ = std::fs::remove_dir(&p);
        }
    }
    back
}

pub fn save_log(path: &Path, log: &Log) {
    if let Ok(t) = serde_json::to_string(log) {
        let _ = std::fs::write(path, t);
    }
}

pub fn load_log(path: &Path) -> Option<Log> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The Downloads folder of this person.
pub fn downloads_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)?;
    let d = home.join("Downloads");
    d.is_dir().then_some(d)
}

/// "12 files: 5 Pictures, 4 Documents, 3 Installers".
pub fn describe(moves: &[Move]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for m in moves {
        match counts.iter_mut().find(|(f, _)| *f == m.folder) {
            Some(c) => c.1 += 1,
            None => counts.push((m.folder, 1)),
        }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    format!("{} file{}: {}", moves.len(), if moves.len() == 1 { "" } else { "s" }, counts.iter().map(|(f, n)| format!("{n} to {f}")).collect::<Vec<_>>().join(", "))
}

#[cfg(desktop)]
mod flow {
    use super::*;
    use crate::helpers::popup::{self, Kind, Payload};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use tauri::{AppHandle, Manager};

    static RUNNING: AtomicBool = AtomicBool::new(false);
    static OFFER: Mutex<Option<(PathBuf, Vec<Move>)>> = Mutex::new(None);

    fn undo_path(app: &AppHandle) -> PathBuf {
        app.state::<crate::state::AppState>().paths.data.join(UNDO_FILE)
    }

    pub fn apply(app: &AppHandle, on: bool) {
        if !on {
            RUNNING.store(false, Ordering::SeqCst);
            return;
        }
        if !supported() || RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_offer: Option<std::time::Instant> = None;
            // A first look a little after start, then every ten minutes.
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            while RUNNING.load(Ordering::SeqCst) {
                let recently = last_offer.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(6 * 3600));
                if !recently && crate::helpers::blocked_now().is_none() {
                    if let Some(dir) = downloads_dir() {
                        let d2 = dir.clone();
                        let moves = tokio::task::spawn_blocking(move || plan(&scan(&d2), MIN_AGE_SECS)).await.unwrap_or_default();
                        if moves.len() >= OFFER_AT {
                            offer(&app, dir, moves);
                            last_offer = Some(std::time::Instant::now());
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            }
        });
    }

    fn offer(app: &AppHandle, dir: PathBuf, moves: Vec<Move>) {
        let text = describe(&moves);
        if let Ok(mut o) = OFFER.lock() {
            *o = Some((dir, moves));
        }
        popup::show(
            app,
            Kind::Tidy,
            Payload { title: "Tidy your Downloads?".into(), body: format!("{text} can be sorted into folders inside Downloads. Nothing is deleted, and you can undo it."), buttons: vec![popup::button("sort", "Sort them"), popup::button("close", "Not now")], auto_hide: 60_000, ..Default::default() },
            None,
        );
    }

    /// The tray entry: look now.
    pub fn now(app: &AppHandle) {
        let Some(dir) = downloads_dir() else {
            return popup::show(app, Kind::Tidy, Payload { title: "Tidy your Downloads".into(), body: "BYTE couldn't find your Downloads folder.".into(), buttons: vec![popup::button("close", "Close")], auto_hide: 6_000, ..Default::default() }, None);
        };
        let moves = plan(&scan(&dir), MIN_AGE_SECS);
        if moves.is_empty() {
            return popup::show(app, Kind::Tidy, Payload { title: "Tidy your Downloads".into(), body: "Nothing to sort: no loose finished files that BYTE recognises.".into(), buttons: vec![popup::button("close", "Close")], auto_hide: 6_000, ..Default::default() }, None);
        }
        offer(app, dir, moves);
    }

    pub fn clicked(app: &AppHandle, id: &str) {
        match id {
            "sort" => {
                let Some((dir, moves)) = OFFER.lock().ok().and_then(|mut o| o.take()) else { return popup::hide(app) };
                let log = super::apply(&dir, &moves);
                save_log(&undo_path(app), &log);
                let body = if log.moved.is_empty() { "Nothing could be moved.".to_string() } else { format!("Sorted {} file{} into folders inside Downloads.", log.moved.len(), if log.moved.len() == 1 { "" } else { "s" }) };
                let buttons = if log.moved.is_empty() { vec![popup::button("close", "Close")] } else { vec![popup::button("undo", "Undo"), popup::button("close", "Close")] };
                popup::show(app, Kind::Tidy, Payload { title: "Downloads tidied".into(), body, buttons, auto_hide: 30_000, ..Default::default() }, None);
            }
            "undo" => {
                let path = undo_path(app);
                let back = load_log(&path).map(|l| super::undo(&l)).unwrap_or(0);
                let _ = std::fs::remove_file(&path);
                popup::show(app, Kind::Tidy, Payload { title: "Undone".into(), body: format!("Put {back} file{} back.", if back == 1 { "" } else { "s" }), buttons: vec![popup::button("close", "Close")], auto_hide: 8_000, ..Default::default() }, None);
            }
            _ => popup::hide(app),
        }
    }
}

#[cfg(desktop)]
pub fn apply_watch(app: &tauri::AppHandle, on: bool) {
    flow::apply(app, on);
}
#[cfg(desktop)]
pub fn now(app: &tauri::AppHandle) {
    flow::now(app);
}
#[cfg(desktop)]
pub async fn clicked(app: &tauri::AppHandle, id: &str) {
    flow::clicked(app, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, age: u64) -> Entry {
        Entry { name: name.into(), is_dir: false, age_secs: age }
    }

    #[test]
    fn files_are_sorted_by_type_and_unknown_ones_are_left() {
        assert_eq!(category("a.JPG"), Some("Pictures"));
        assert_eq!(category("report.pdf"), Some("Documents"));
        assert_eq!(category("setup.exe"), Some("Installers"));
        assert_eq!(category("x.zip"), Some("Archives"));
        assert_eq!(category("song.mp3"), Some("Music"));
        assert_eq!(category("clip.mkv"), Some("Videos"));
        assert_eq!(category("weird.xyz"), None);
        assert_eq!(category("noextension"), None);
    }

    #[test]
    fn unfinished_fresh_hidden_and_folder_entries_are_never_moved() {
        let mut dir = e("Photos", 99999);
        dir.is_dir = true;
        let list = vec![e("a.png", 99999), e("b.png", 5), e("c.crdownload", 99999), e(".hidden.png", 99999), e("desktop.ini", 99999), dir, e("movie.mp4.part", 99999), e("d.pdf", 700)];
        let p = plan(&list, MIN_AGE_SECS);
        assert_eq!(p.iter().map(|m| (m.name.as_str(), m.folder)).collect::<Vec<_>>(), vec![("a.png", "Pictures"), ("d.pdf", "Documents")]);
    }

    #[test]
    fn sorting_moves_files_keeps_names_free_and_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.png"), "1").unwrap();
        std::fs::write(d.path().join("b.pdf"), "2").unwrap();
        std::fs::create_dir(d.path().join("Pictures")).unwrap();
        std::fs::write(d.path().join("Pictures").join("a.png"), "old").unwrap();
        let moves = plan(&scan(d.path()).into_iter().map(|mut e| { e.age_secs = 99999; e }).collect::<Vec<_>>(), MIN_AGE_SECS);
        assert_eq!(moves.len(), 2);
        let log = apply(d.path(), &moves);
        assert_eq!(log.moved.len(), 2);
        assert_eq!(std::fs::read_to_string(d.path().join("Pictures").join("a.png")).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(d.path().join("Pictures").join("a (2).png")).unwrap(), "1");
        assert!(d.path().join("Documents").join("b.pdf").is_file());
        assert!(!d.path().join("a.png").exists());
    }

    #[test]
    fn undo_puts_files_back_and_removes_only_empty_sort_folders() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.png"), "1").unwrap();
        std::fs::write(d.path().join("b.pdf"), "2").unwrap();
        let moves = vec![Move { name: "a.png".into(), folder: "Pictures" }, Move { name: "b.pdf".into(), folder: "Documents" }];
        let log = apply(d.path(), &moves);
        // The person has since put their own file into Documents: that folder must stay.
        std::fs::write(d.path().join("Documents").join("mine.txt"), "m").unwrap();
        assert_eq!(undo(&log), 2);
        assert!(d.path().join("a.png").is_file() && d.path().join("b.pdf").is_file());
        assert!(!d.path().join("Pictures").exists());
        assert!(d.path().join("Documents").join("mine.txt").is_file());
    }

    #[test]
    fn undo_skips_a_file_whose_old_place_is_taken_or_that_is_gone() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.png"), "1").unwrap();
        std::fs::write(d.path().join("b.png"), "2").unwrap();
        let log = apply(d.path(), &[Move { name: "a.png".into(), folder: "Pictures" }, Move { name: "b.png".into(), folder: "Pictures" }]);
        std::fs::write(d.path().join("a.png"), "new file").unwrap();
        std::fs::remove_file(d.path().join("Pictures").join("b.png")).unwrap();
        assert_eq!(undo(&log), 0);
        assert_eq!(std::fs::read_to_string(d.path().join("a.png")).unwrap(), "new file");
        assert!(d.path().join("Pictures").join("a.png").is_file());
    }

    #[test]
    fn the_undo_log_round_trips_and_the_offer_is_described() {
        let d = tempfile::tempdir().unwrap();
        let log = Log { dir: d.path().into(), moved: vec![("/x/a".into(), "/x/P/a".into())] };
        let p = d.path().join(UNDO_FILE);
        save_log(&p, &log);
        assert_eq!(load_log(&p), Some(log));
        assert_eq!(load_log(&d.path().join("none.json")), None);
        let m = vec![Move { name: "a".into(), folder: "Pictures" }, Move { name: "b".into(), folder: "Pictures" }, Move { name: "c".into(), folder: "Music" }];
        assert_eq!(describe(&m), "3 files: 2 to Pictures, 1 to Music");
    }

    #[test]
    fn a_file_is_moved_only_within_downloads() {
        assert!(FOLDERS.iter().all(|f| !f.contains(['/', '\\', '.'])));
        let d = tempfile::tempdir().unwrap();
        let inner = d.path().join("inner");
        std::fs::create_dir(&inner).unwrap();
        std::fs::write(d.path().join("secret.png"), "x").unwrap();
        let log = apply(&inner, &[Move { name: "../secret.png".into(), folder: "Pictures" }, Move { name: "a/b.png".into(), folder: "Pictures" }]);
        assert!(log.moved.is_empty());
        assert!(d.path().join("secret.png").is_file());
    }
}
