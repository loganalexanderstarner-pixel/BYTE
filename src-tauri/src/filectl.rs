//! Files on the Mac (Phase 9): find them, tidy a folder, convert photos, and ask
//! about what's selected in Finder.
//!
//! "find my tax return pdf", "where's my resume", "organize my Downloads",
//! "tidy my desktop by month", "convert the selected photos to jpg", "make the
//! selected photos smaller", "summarize the files I selected in Finder".
//!
//! Safety: finding and reading change nothing. Tidying and converting show the
//! plan first (an approval card) and can be undone; moves never overwrite, only
//! top-level files move, and files still downloading are left alone. Converting
//! keeps the originals. Nothing is ever deleted except the copies BYTE made, and
//! only when the user presses Undo.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::{AppError, AppResult};
use crate::macctl::{self, Command, MacDone, MacRunner, Runner, Undo};
use crate::pcctl::WinOp;
use crate::tools::SourceBook;

/// Results listed when finding files.
const FIND_MAX: usize = 20;
/// Selected files read at once (for questions about them).
const READ_MAX: usize = 5;
/// Files changed in the last two minutes may still be downloading: left alone.
const SETTLE: Duration = Duration::from_secs(120);

/// The folders people name, relative to home.
const FOLDERS: &[(&str, &str)] = &[("downloads", "Downloads"), ("desktop", "Desktop"), ("documents", "Documents"), ("pictures", "Pictures"), ("movies", "Movies"), ("videos", "Videos"), ("music", "Music")];

/// Groups for tidying by kind (extension → folder name).
const KINDS: &[(&str, &[&str])] = &[
    ("Images", &["jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "tif", "tiff", "bmp", "svg", "raw", "cr2", "nef", "arw", "dng"]),
    ("Documents", &["pdf", "doc", "docx", "pages", "txt", "rtf", "md", "odt", "epub"]),
    ("Spreadsheets", &["xls", "xlsx", "csv", "numbers", "ods"]),
    ("Presentations", &["ppt", "pptx", "key", "odp"]),
    ("Installers", &["dmg", "pkg", "mpkg"]),
    ("Archives", &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz"]),
    ("Audio", &["mp3", "m4a", "wav", "aac", "flac", "aiff", "ogg"]),
    ("Video", &["mp4", "mov", "m4v", "avi", "mkv", "webm"]),
    ("Code", &["js", "ts", "py", "rs", "java", "c", "cpp", "h", "swift", "go", "rb", "sh", "json", "html", "css", "yml", "yaml", "toml"]),
];

/// Photo formats `sips` writes.
const PHOTO_FORMATS: &[(&str, &str, &str)] = &[("jpg", "jpeg", "jpg"), ("jpeg", "jpeg", "jpg"), ("png", "png", "png"), ("heic", "heic", "heic"), ("tiff", "tiff", "tiff"), ("tif", "tiff", "tiff")];

/// Reads what's selected in Finder: one POSIX path per line.
const FINDER_SELECTION: &str = r#"on run argv
	set out to ""
	tell application "Finder"
		set sel to selection as alias list
		repeat with f in sel
			set out to out & (POSIX path of f) & linefeed
		end repeat
	end tell
	return out
end run"#;

/// For the syntax check on macOS.
#[cfg_attr(not(test), allow(dead_code))]
pub const SCRIPTS: &[&str] = &[FINDER_SELECTION];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Kind,
    Month,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Find { query: String },
    Organize { folder: String, by: By },
    Convert { to: Option<String>, max: Option<u32> },
    AboutSelection,
}

fn starts(l: &str, words: &[&str]) -> bool {
    words.iter().any(|w| l.starts_with(w))
}

fn clean(q: &str) -> String {
    let mut l = q.trim().to_lowercase();
    for p in ["hey byte,", "hey byte", "please ", "can you ", "could you ", "would you "] {
        if let Some(rest) = l.strip_prefix(p) {
            l = rest.trim_start().to_string();
        }
    }
    l.trim_end_matches(['?', '!', '.']).trim_end_matches(" please").trim_end_matches(" for me").trim().to_string()
}

/// Words that make "find …" about files rather than the web.
const FILE_WORDS: &[&str] = &[
    " file", "files", "document", " doc ", " docs", " pdf", ".pdf", "docx", "spreadsheet", "xlsx", "presentation", "pptx", "keynote", "screenshot", "on my mac", "on this mac", "on my pc", "on this pc", "on my computer", "on my laptop", "in file explorer", "in explorer", "in my downloads", "in my documents", "on my desktop", "in finder", "resume", "résumé", "receipt", "invoice", "tax return", "my notes file", "the photo of", "photos of",
];

/// What the message asks about files, if anything.
pub fn ask(q: &str) -> Option<Ask> {
    let l = clean(q);
    if l.is_empty() || starts(&l, &["how do i", "how can i", "how to", "what is", "why "]) {
        return None;
    }
    let selected = l.contains("selected") || l.contains("i selected") || l.contains("in finder");
    // Convert or shrink the selected photos.
    if selected && (l.contains("photo") || l.contains("image") || l.contains("picture") || l.contains("screenshot")) {
        let to = PHOTO_FORMATS.iter().find(|(w, _, _)| l.contains(&format!("to {w}")) || l.contains(&format!("into {w}")) || l.contains(&format!("as {w}"))).map(|(w, _, _)| w.to_string());
        let smaller = l.contains("smaller") || l.contains("resize") || l.contains("shrink") || l.contains("compress");
        if starts(&l, &["convert", "change", "turn", "save", "make", "resize", "shrink", "compress", "export"]) && (to.is_some() || smaller) {
            let max = if smaller { Some(number_in(&l).filter(|n| (200..=8000).contains(n)).unwrap_or(1600)) } else { None };
            return Some(Ask::Convert { to, max });
        }
    }
    // Questions about the selected files.
    if selected && (l.contains("file") || l.contains("document") || l.contains("pdf") || l.contains("these")) {
        return Some(Ask::AboutSelection);
    }
    // Tidy a folder.
    if starts(&l, &["organize", "organise", "tidy", "clean up", "sort", "declutter"]) {
        if let Some((_, folder)) = FOLDERS.iter().find(|(w, _)| l.contains(w)) {
            let by = if l.contains("month") || l.contains("date") { By::Month } else { By::Kind };
            return Some(Ask::Organize { folder: folder.to_string(), by });
        }
    }
    // Find files.
    let padded = format!(" {l} ");
    if starts(&l, &["find ", "where is ", "where's ", "wheres ", "locate ", "search my mac for ", "search my pc for ", "search my computer for ", "look for "]) && FILE_WORDS.iter().any(|w| padded.contains(w)) {
        let rest = ["search my mac for ", "search my pc for ", "search my computer for ", "where is ", "where's ", "wheres ", "locate ", "look for ", "find "].iter().find_map(|p| l.strip_prefix(p)).unwrap_or(&l);
        let query = find_words(rest);
        if !query.is_empty() {
            return Some(Ask::Find { query });
        }
    }
    None
}

/// The words to search for: "my tax return pdf on my mac" → "tax return".
fn find_words(s: &str) -> String {
    const DROP: &[&str] = &["my", "the", "a", "an", "file", "files", "document", "documents", "doc", "docs", "on", "in", "this", "mac", "pc", "computer", "laptop", "explorer", "folder", "that", "called", "named", "about", "pdf", "pdfs", "i", "saved", "downloaded", "somewhere", "for"];
    s.split_whitespace().filter(|w| !DROP.contains(w)).collect::<Vec<_>>().join(" ").trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

fn number_in(l: &str) -> Option<u32> {
    l.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).next()
}

/// Asks this module should answer (module on, macOS or Windows).
pub fn applies(enabled: bool, q: &str) -> bool {
    (cfg!(target_os = "macos") || cfg!(windows)) && enabled && ask(q).is_some()
}

// ------------------------------------------------------------------ tidying

/// The group a file goes to when tidying by kind.
pub fn kind_folder(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    KINDS.iter().find(|(_, exts)| exts.contains(&ext.as_str())).map(|(k, _)| *k).unwrap_or("Other")
}

/// Installers on a PC (a Mac's are `.dmg` and `.pkg`).
const PC_INSTALLERS: &[&str] = &["exe", "msi", "msix", "msixbundle", "appx", "appxbundle", "iso"];

/// `kind_folder` for the platform: on a PC, programs and disk images are installers.
pub fn kind_folder_for(pc: bool, path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if pc && PC_INSTALLERS.contains(&ext.as_str()) {
        return "Installers";
    }
    kind_folder(path)
}

/// A name that isn't taken in `dir`: "photo.jpg", then "photo 2.jpg", …
pub fn free_name(dir: &Path, name: &str, taken: &std::collections::HashSet<PathBuf>) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = p.extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    let mut candidate = dir.join(name);
    let mut n = 2;
    while candidate.exists() || taken.contains(&candidate) {
        candidate = dir.join(format!("{stem} {n}{ext}"));
        n += 1;
    }
    candidate
}

/// Skipped when tidying: hidden files, partial downloads, very new files.
fn leave_alone(path: &Path, modified: Option<SystemTime>, now: SystemTime) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    name.starts_with('.') || matches!(ext.as_str(), "crdownload" | "download" | "part" | "partial" | "tmp") || name == "Icon\r" || modified.is_some_and(|m| now.duration_since(m).map(|d| d < SETTLE).unwrap_or(true))
}

/// `leave_alone` for the platform. On a PC also: shortcuts (they are how programs are opened from the Desktop), the files Windows
/// and Office keep beside yours (`desktop.ini`, `Thumbs.db`, `~$` lock files), and other browsers' partial downloads.
fn leave_alone_for(pc: bool, path: &Path, modified: Option<SystemTime>, now: SystemTime) -> bool {
    if leave_alone(path, modified, now) {
        return true;
    }
    if !pc {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    matches!(name.as_str(), "desktop.ini" | "thumbs.db") || name.starts_with("~$") || matches!(ext.as_str(), "lnk" | "url" | "ini" | "opdownload" | "aria2" | "unconfirmed")
}

#[cfg(windows)]
fn hidden_or_system(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x6 != 0
}

#[cfg(not(windows))]
fn hidden_or_system(_: &std::fs::Metadata) -> bool {
    false
}

/// Where each top-level file in `dir` goes: (from, to), in name order.
pub fn tidy_plan(dir: &Path, by: By, now: SystemTime) -> AppResult<Vec<(PathBuf, PathBuf)>> {
    tidy_plan_for(false, dir, by, now)
}

/// `tidy_plan` for the platform.
pub fn tidy_plan_for(pc: bool, dir: &Path, by: By, now: SystemTime) -> AppResult<Vec<(PathBuf, PathBuf)>> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| AppError::msg(format!("couldn't read {}: {e}", dir.display())))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    let mut taken = std::collections::HashSet::new();
    let mut moves = Vec::new();
    for e in entries {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() || leave_alone_for(pc, &path, meta.modified().ok(), now) || (pc && hidden_or_system(&meta)) {
            continue;
        }
        let group = match by {
            By::Kind => kind_folder_for(pc, &path).to_string(),
            By::Month => {
                let t = meta.modified().ok().map(chrono::DateTime::<chrono::Local>::from);
                t.map(|t| t.format("%Y-%m").to_string()).unwrap_or_else(|| "Older".into())
            }
        };
        let to_dir = dir.join(&group);
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        let to = free_name(&to_dir, &name, &taken);
        taken.insert(to.clone());
        moves.push((path, to));
    }
    Ok(moves)
}

/// "Images: 23 · Documents: 12 · …" for the approval card.
pub fn plan_summary(moves: &[(PathBuf, PathBuf)]) -> String {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, to) in moves {
        let group = to.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("?").to_string();
        *counts.entry(group).or_default() += 1;
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.iter().map(|(g, n)| format!("{g}: {n}")).collect::<Vec<_>>().join(" · ")
}

/// Does the moves (creating group folders); stops at the first failure and
/// returns what was done so it can be undone.
pub fn apply_moves(moves: &[(PathBuf, PathBuf)]) -> (Vec<(PathBuf, PathBuf)>, Option<String>) {
    let (done, _, err) = apply_moves_with(false, moves, &|from, to| std::fs::rename(from, to));
    (done, err)
}

/// `apply_moves` for the platform, with the move itself as a parameter (tests). On a PC a file that is open in another program
/// (Windows' "in use", "locked" or "access denied") is left where it is and the rest still move; the names come back as the
/// second part. Anything else stops the tidy, as on a Mac.
pub fn apply_moves_with(pc: bool, moves: &[(PathBuf, PathBuf)], rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>) -> (Vec<(PathBuf, PathBuf)>, Vec<String>, Option<String>) {
    let mut done = Vec::new();
    let mut in_use = Vec::new();
    for (from, to) in moves {
        if let Some(parent) = to.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return (done, in_use, Some(format!("couldn't make {}: {e}", parent.display())));
            }
        }
        if to.exists() {
            continue;
        }
        match rename(from, to) {
            Ok(()) => done.push((from.clone(), to.clone())),
            Err(e) if pc && matches!(e.raw_os_error(), Some(5 | 32 | 33)) => in_use.push(from.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()),
            Err(e) => return (done, in_use, Some(format!("couldn't move {}: {e}", from.display()))),
        }
    }
    (done, in_use, None)
}

// ------------------------------------------------------------ finding on a PC

/// Where a match came from: the file's name, or the text inside a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Name,
    Text,
}

/// `FindFiles` output: `name<TAB>path` and `text<TAB>path`.
pub fn parse_found(out: &str) -> Vec<(Hit, PathBuf)> {
    out.lines()
        .filter_map(|l| {
            let (kind, path) = l.trim_end_matches(['\r', '\n']).split_once('\t')?;
            let hit = match kind {
                "name" => Hit::Name,
                "text" => Hit::Text,
                _ => return None,
            };
            (!path.trim().is_empty()).then(|| (hit, PathBuf::from(path.trim())))
        })
        .collect()
}

/// Whether a PC path is somewhere the person keeps files: not app data, hidden folders (`.cargo`, `.git`), dependency
/// folders, the Recycle Bin, or Windows and program folders.
pub fn keep_found(path: &Path) -> bool {
    keep_below(&path.display().to_string(), 1)
}

/// `keep_found` for a path under `root` (the person's home folder): only what is below `root` is judged, so a hidden folder above
/// it doesn't matter. A path that isn't under `root` is judged whole.
pub fn keep_found_under(root: &Path, path: &Path) -> bool {
    match path.strip_prefix(root) {
        Ok(rel) => keep_below(&format!("\\{}", rel.display()), 0),
        Err(_) => keep_found(path),
    }
}

/// `skip` is how many leading parts (a drive) are not judged for being hidden.
fn keep_below(path: &str, skip: usize) -> bool {
    let low = path.to_lowercase().replace('/', "\\");
    const NOISE: &[&str] = &["\\appdata\\", "\\node_modules\\", "\\$recycle.bin\\", "\\windows\\", "\\program files", "\\programdata\\", "\\site-packages\\", "\\.git\\"];
    if NOISE.iter().any(|n| low.contains(n)) {
        return false;
    }
    // A folder or file whose name starts with a dot is hidden by convention (.rustup, .cache).
    !low.split('\\').skip(skip).any(|part| part.starts_with('.') && part.len() > 1)
}

fn name_tokens(name: &str) -> Vec<String> {
    name.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// The matches to show, best first: files whose name has every word at the start of a word of its name ("tax return 2025.pdf"),
/// then names with some of the words at the start of a word, then other name matches ("syntax.html" for "tax"), then documents
/// with the words inside; newest first within each. No duplicates, and nothing from places the person doesn't keep files
/// (judged below `home`).
pub fn rank_found(hits: Vec<(Hit, PathBuf)>, words: &[String], home: &Path) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut scored: Vec<(u8, usize, PathBuf)> = Vec::new();
    for (i, (hit, path)) in hits.into_iter().enumerate() {
        if !keep_found_under(home, &path) || !seen.insert(path.display().to_string().to_lowercase()) {
            continue;
        }
        let shown = path.display().to_string();
        let name = shown.rsplit(['\\', '/']).next().unwrap_or("");
        let tokens = name_tokens(name);
        let starts = |w: &String| tokens.iter().any(|t| t.starts_with(w.as_str()));
        let score = match hit {
            Hit::Name if words.iter().all(starts) => 4,
            Hit::Name if words.iter().any(starts) => 3,
            Hit::Name => 2,
            Hit::Text => 1,
        };
        scored.push((score, i, path));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, p)| p).collect()
}

/// When Windows Search isn't running: a bounded walk of the folder for file names that hold every word, newest first.
pub fn walk_find(root: &Path, words: &[String], deadline: std::time::Instant) -> Vec<(Hit, PathBuf)> {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut seen = 0usize;
    let walker = walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| e.depth() == 0 || e.file_type().is_file() || keep_found_under(root, e.path()));
    for e in walker.flatten() {
        seen += 1;
        if seen > 400_000 || std::time::Instant::now() > deadline {
            break;
        }
        if !e.file_type().is_file() || !keep_found_under(root, e.path()) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_lowercase();
        if words.iter().all(|w| name.contains(w.as_str())) {
            found.push((e.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH), e.path().to_path_buf()));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, p)| (Hit::Name, p)).collect()
}

// ---------------------------------------------------------------- converting

/// The `sips` command for one photo (the output is a new file; the original stays).
pub fn sips_command(src: &Path, out: &Path, format: Option<&str>, max: Option<u32>) -> Command {
    let mut args = Vec::new();
    if let Some(f) = format {
        args.extend(["-s".to_string(), "format".to_string(), f.to_string()]);
    }
    if let Some(m) = max {
        args.extend(["-Z".to_string(), m.to_string()]);
    }
    args.push(src.display().to_string());
    args.extend(["--out".to_string(), out.display().to_string()]);
    Command::Exec { program: "sips", args }
}

fn is_photo(p: &Path) -> bool {
    kind_folder(p) == "Images" && !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// Photo kinds a PC can save: (word, WIC format, file extension).
const PC_PHOTO_FORMATS: &[(&str, &str, &str)] = &[("jpg", "jpeg", "jpg"), ("jpeg", "jpeg", "jpg"), ("png", "png", "png"), ("tiff", "tiff", "tiff"), ("tif", "tiff", "tiff"), ("bmp", "bmp", "bmp")];

/// `convert_plan` for a PC: every photo gets an explicit format. With only a size given, a photo keeps its kind when a PC can save
/// that kind (JPEG, PNG, TIFF, BMP) and is saved as PNG otherwise (a GIF, WebP or HEIC photo, for example).
pub fn convert_plan_pc(files: &[PathBuf], to: Option<&str>, max: Option<u32>) -> Vec<(PathBuf, PathBuf, &'static str)> {
    let mut taken = std::collections::HashSet::new();
    let chosen = to.and_then(|t| PC_PHOTO_FORMATS.iter().find(|(w, _, _)| *w == t));
    files
        .iter()
        .filter(|f| is_photo(f))
        .map(|f| {
            let dir = f.parent().unwrap_or(Path::new("."));
            let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("photo");
            let own = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            let (_, fmt, ext) = chosen.or_else(|| PC_PHOTO_FORMATS.iter().find(|(w, _, _)| *w == own)).copied().unwrap_or(("png", "png", "png"));
            let name = if max.is_some() { format!("{stem} (small).{ext}") } else { format!("{stem}.{ext}") };
            let out = free_name(dir, &name, &taken);
            taken.insert(out.clone());
            (f.clone(), out, fmt)
        })
        .collect()
}

/// Output paths for converting: same folder, new extension (or "name (small)").
pub fn convert_plan(files: &[PathBuf], to: Option<&str>, max: Option<u32>) -> Vec<(PathBuf, PathBuf, Option<&'static str>)> {
    let mut taken = std::collections::HashSet::new();
    let fmt = to.and_then(|t| PHOTO_FORMATS.iter().find(|(w, _, _)| *w == t));
    files
        .iter()
        .filter(|f| is_photo(f))
        .map(|f| {
            let dir = f.parent().unwrap_or(Path::new("."));
            let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("photo");
            let ext = fmt.map(|(_, _, e)| e.to_string()).unwrap_or_else(|| f.extension().and_then(|e| e.to_str()).unwrap_or("jpg").to_string());
            let name = if max.is_some() { format!("{stem} (small).{ext}") } else { format!("{stem}.{ext}") };
            let out = free_name(dir, &name, &taken);
            taken.insert(out.clone());
            (f.clone(), out, fmt.map(|(_, s, _)| *s))
        })
        .collect()
}

// ---------------------------------------------------------------------- run

/// "Find my tax return" on a PC: Windows Search for names and the text inside documents; if the search service is off, a bounded
/// walk of the home folder by file name.
async fn find_pc(turn: &Turn<'_>, runner: &dyn Runner, id: String, query: &str, home: &Path, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    send(ChatEvent::ToolCall { id: id.clone(), name: "mac_find_files".into(), args: json!({ "app": "Windows Search", "what": format!("Look for files about \"{query}\"") }) })?;
    let words: Vec<String> = query.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).filter(|w| crate::pcctl::find_word_ok(w)).take(6).collect();
    if words.is_empty() {
        send(ChatEvent::ToolResult { id, ok: false, summary: "Nothing to search for".into() })?;
        return Ok(Some((SourceBook::default(), format!("BYTE couldn't make a search from \"{query}\". Ask the user for a word or two from the file's name."))));
    }
    let (hits, how) = match runner.run(&Command::Win(WinOp::FindFiles { words: words.clone(), root: home.to_path_buf() })).await {
        Ok(out) => (parse_found(&out), "Windows Search"),
        Err(_) => {
            let (root, w) = (home.to_path_buf(), words.clone());
            let walked = tokio::task::spawn_blocking(move || walk_find(&root, &w, std::time::Instant::now() + Duration::from_secs(8))).await.unwrap_or_default();
            (walked, "a look through your folders by file name, because Windows Search isn't running")
        }
    };
    let paths: Vec<PathBuf> = rank_found(hits, &words, home).into_iter().take(FIND_MAX).collect();
    let summary = format!("{} files found", paths.len());
    send(ChatEvent::ToolResult { id, ok: true, summary: summary.clone() })?;
    turn.log.record("mac_find_files", &json!({ "query": query }), true, &summary);
    if paths.is_empty() {
        return Ok(Some((SourceBook::default(), format!("BYTE searched this PC ({how}) for \"{query}\" and found nothing. Suggest other words to try."))));
    }
    let list: Vec<String> = paths
        .iter()
        .map(|p| {
            let when = std::fs::metadata(p).ok().and_then(|m| m.modified().ok()).map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%b %-d, %Y").to_string()).unwrap_or_default();
            format!("- `{}`{}", p.display(), if when.is_empty() { String::new() } else { format!(" (changed {when})") })
        })
        .collect();
    Ok(Some((SourceBook::default(), format!("Files on the user's PC that match \"{query}\" ({how}, best first):\n{}\n\nList the likeliest ones with their folder; the user can open them in File Explorer.", list.join("\n")))))
}

fn home() -> PathBuf {
    #[cfg(windows)]
    if let Some(h) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(h);
    }
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// The folder as the platform names it: a Mac's Movies is a PC's Videos.
fn folder_name(pc: bool, folder: &str) -> &str {
    match (pc, folder) {
        (true, "Movies") => "Videos",
        (false, "Videos") => "Movies",
        _ => folder,
    }
}

/// Where that folder is: on a PC where Windows says (it can be inside OneDrive), else under the home folder.
fn folder_dir(pc: bool, home: &Path, folder: &str) -> PathBuf {
    let name = folder_name(pc, folder);
    if pc {
        if let Some(p) = crate::pcctl::known_folder(name) {
            return p;
        }
    }
    home.join(name)
}

/// "~/Downloads/x.pdf" for display.
fn tilde(p: &Path) -> String {
    let h = home();
    match p.strip_prefix(&h) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

fn card(send: Emit<'_>, app: &str, title: &str, detail: &str, ok: bool, undo: Option<String>) -> AppResult<()> {
    send(ChatEvent::MacDone(MacDone { app: app.into(), title: title.into(), detail: detail.into(), ok, undo }))
}

async fn selection(runner: &dyn Runner) -> Result<Vec<PathBuf>, String> {
    let (cmd, app) = if runner.pc() { (Command::Win(WinOp::ExplorerSelection), "File Explorer") } else { (Command::Osa { script: FINDER_SELECTION, args: vec![] }, "Finder") };
    match runner.run(&cmd).await {
        Ok(out) => Ok(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect()),
        Err(e) => Err(e.text_for(app, runner.pc())),
    }
}

/// Does what the message asks with files. Returns notes for the answer.
pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    if cfg!(windows) {
        run_with(turn, question, &crate::pcctl::WinRunner, &home(), SystemTime::now(), cancel, send).await
    } else {
        run_with(turn, question, &MacRunner, &home(), SystemTime::now(), cancel, send).await
    }
}

pub(crate) async fn run_with(turn: &Turn<'_>, question: &str, runner: &dyn Runner, home: &Path, now: SystemTime, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(a) = ask(question) else { return Ok(None) };
    let id = format!("byte_files_{}", uuid::Uuid::new_v4().simple());
    let pc = runner.pc();
    let tell = if pc { "" } else { macctl::TELL };
    let app = if pc { "File Explorer" } else { "Finder" };
    if pc {
        match &a {
            Ask::Find { query } => return find_pc(turn, runner, id, query, home, send).await,
            Ask::Convert { to: Some(t), .. } if t == "heic" => {
                return Ok(Some((SourceBook::default(), "Windows can't save photos as HEIC (it can open them if Microsoft's HEIF extension is installed). Tell the user that, and offer JPG, PNG, TIFF or BMP instead.".to_string())));
            }
            _ => {}
        }
    }
    match a {
        Ask::Find { query } => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_find_files".into(), args: json!({ "app": "Finder", "what": format!("Look for files about \"{query}\"") }) })?;
            let out = runner.run(&Command::Exec { program: "mdfind", args: vec!["-onlyin".into(), home.display().to_string(), query.clone()] }).await;
            let paths: Vec<PathBuf> = match out {
                Ok(o) => o.lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains("/Library/") && !l.contains("/.")).map(PathBuf::from).take(FIND_MAX).collect(),
                Err(e) => {
                    let msg = e.text("Spotlight");
                    send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
                    return Ok(Some((SourceBook::default(), format!("BYTE couldn't search the Mac: {msg}\n\n{tell}"))));
                }
            };
            let summary = format!("{} files found", paths.len());
            send(ChatEvent::ToolResult { id, ok: true, summary: summary.clone() })?;
            turn.log.record("mac_find_files", &json!({ "query": query }), true, &summary);
            if paths.is_empty() {
                return Ok(Some((SourceBook::default(), format!("BYTE searched this Mac (Spotlight) for \"{query}\" and found nothing. Suggest other words to try.\n\n{tell}"))));
            }
            let list: Vec<String> = paths
                .iter()
                .map(|p| {
                    let when = std::fs::metadata(p).ok().and_then(|m| m.modified().ok()).map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%b %-d, %Y").to_string()).unwrap_or_default();
                    format!("- `{}`{}", tilde(p), if when.is_empty() { String::new() } else { format!(" (changed {when})") })
                })
                .collect();
            Ok(Some((SourceBook::default(), format!("Files on the user's Mac that match \"{query}\" (Spotlight, best first):\n{}\n\nList the likeliest ones with their folder; the user can open them in Finder.", list.join("\n")))))
        }
        Ask::Organize { folder, by } => {
            let folder = folder_name(pc, &folder).to_string();
            let dir = folder_dir(pc, home, &folder);
            let shown = if pc { dir.display().to_string() } else { tilde(&dir) };
            let what = format!("Tidy {folder} into folders by {}", if by == By::Kind { "kind" } else { "month" });
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_organize".into(), args: json!({ "app": app, "what": what }) })?;
            let moves = match tidy_plan_for(pc, &dir, by, now) {
                Ok(m) => m,
                Err(e) => {
                    send(ChatEvent::ToolResult { id, ok: false, summary: e.to_string() })?;
                    let hint = if pc {
                        "Windows may be blocking BYTE from that folder (Windows Security → Virus & threat protection → Ransomware protection → Controlled folder access)."
                    } else {
                        "macOS may need BYTE allowed in System Settings → Privacy & Security → Files and Folders."
                    };
                    return Ok(Some((SourceBook::default(), format!("BYTE couldn't read {folder}: {e}. {hint}\n\n{tell}"))));
                }
            };
            if moves.is_empty() {
                send(ChatEvent::ToolResult { id, ok: true, summary: "Nothing to move".into() })?;
                let left = if pc { "folders, shortcuts, hidden files and files still downloading" } else { "folders and files still downloading" };
                return Ok(Some((SourceBook::default(), format!("{folder} has no loose files to tidy ({left} are left alone).\n\n{tell}"))));
            }
            let fields = vec![
                ("Folder".to_string(), shown),
                ("Moves".to_string(), format!("{} files into: {}", moves.len(), plan_summary(&moves))),
                ("Note".to_string(), if pc { "Only loose files move (not folders or shortcuts), nothing is deleted, files open in another program stay where they are, and Undo puts everything back.".to_string() } else { "Only loose files move (not folders), nothing is deleted, and Undo puts everything back.".to_string() }),
            ];
            if !macctl::ask_ok(&what, app, fields, cancel, send).await? {
                send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
                return Ok(Some((SourceBook::default(), format!("The user chose not to tidy {folder}. Nothing was moved. Say so in one sentence."))));
            }
            let (done, in_use, err) = apply_moves_with(pc, &moves, &|from, to| std::fs::rename(from, to));
            let undo = (!done.is_empty()).then(|| macctl::keep_undo(Undo::Moves(done.clone())));
            let left = if in_use.is_empty() { String::new() } else { format!(" · {} in use, left alone", in_use.len()) };
            let detail = format!("{} of {} files moved · {}{left}", done.len(), moves.len(), plan_summary(&done));
            send(ChatEvent::ToolResult { id, ok: err.is_none(), summary: detail.clone() })?;
            turn.log.record("mac_organize", &json!({ "folder": folder }), err.is_none(), &detail);
            card(send, app, &what, &detail, err.is_none(), undo)?;
            let problem = err.map(|e| format!(" It stopped early: {e}.")).unwrap_or_default();
            let busy = if in_use.is_empty() { String::new() } else { format!(" These were open in another program, so they stayed where they are: {}.", in_use.iter().take(6).cloned().collect::<Vec<_>>().join(", ")) };
            Ok(Some((SourceBook::default(), format!("Done: BYTE moved {} files in {folder} into folders ({}).{problem}{busy} Undo on the card puts them back.\n\n{tell}", done.len(), plan_summary(&done)))))
        }
        Ask::Convert { to, max } => {
            let files = match selection(runner).await {
                Ok(f) => f,
                Err(e) => return Ok(Some((SourceBook::default(), format!("BYTE couldn't read the {app} selection: {e}\n\n{tell}")))),
            };
            let plan: Vec<(PathBuf, PathBuf, Option<&'static str>)> = if pc { convert_plan_pc(&files, to.as_deref(), max).into_iter().map(|(a, b, c)| (a, b, Some(c))).collect() } else { convert_plan(&files, to.as_deref(), max) };
            if plan.is_empty() {
                return Ok(Some((SourceBook::default(), format!("No photos are selected in {app}. Ask the user to select the photos in {app} first, then ask again."))));
            }
            let what = match (&to, max) {
                (Some(t), Some(m)) => format!("Save {} photos as {} at most {m} px", plan.len(), t.to_uppercase()),
                (Some(t), None) => format!("Save {} photos as {}", plan.len(), t.to_uppercase()),
                (None, Some(m)) => format!("Save smaller copies of {} photos (at most {m} px)", plan.len()),
                (None, None) => format!("Copy {} photos", plan.len()),
            };
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_convert".into(), args: json!({ "app": app, "what": what }) })?;
            let names: Vec<String> = plan.iter().take(8).map(|(_, out, _)| out.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()).collect();
            let fields = vec![
                ("New files".to_string(), format!("{}{}", names.join(", "), if plan.len() > 8 { format!(" and {} more", plan.len() - 8) } else { String::new() })),
                ("Note".to_string(), if pc { "Saved next to the originals, which stay as they are. Photos are turned the right way up, and the camera's details (such as where it was taken) aren't copied. Undo moves the new files to the Recycle Bin.".to_string() } else { "Saved next to the originals, which stay as they are.".to_string() }),
            ];
            if !macctl::ask_ok(&what, app, fields, cancel, send).await? {
                send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
                return Ok(Some((SourceBook::default(), "The user chose not to convert the photos. Nothing was changed. Say so in one sentence.".to_string())));
            }
            let mut made = Vec::new();
            let mut failed = Vec::new();
            for (src, out, fmt) in &plan {
                if cancel.is_cancelled() {
                    return Err(AppError::Cancelled);
                }
                let cmd = match (pc, fmt) {
                    (true, Some(f)) => Command::Win(WinOp::ConvertPhoto { src: src.clone(), out: out.clone(), format: f.to_string(), max }),
                    _ => sips_command(src, out, *fmt, max),
                };
                match runner.run(&cmd).await {
                    Ok(_) => made.push(out.clone()),
                    Err(e) => failed.push(format!("{}: {}", src.file_name().and_then(|n| n.to_str()).unwrap_or("?"), e.text_for(if pc { "Windows" } else { "sips" }, pc))),
                }
            }
            let undo = (!made.is_empty()).then(|| macctl::keep_undo(Undo::Created(made.clone())));
            let detail = format!("{} of {} saved", made.len(), plan.len());
            send(ChatEvent::ToolResult { id, ok: failed.is_empty(), summary: detail.clone() })?;
            card(send, app, &what, &detail, failed.is_empty(), undo)?;
            let problems = if failed.is_empty() { String::new() } else { format!(" These didn't work: {}.", failed.join("; ")) };
            Ok(Some((SourceBook::default(), format!("Done: BYTE saved {} new photo files next to the originals.{problems}\n\n{tell}", made.len()))))
        }
        Ask::AboutSelection => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_finder_selection".into(), args: json!({ "app": app, "what": format!("Read the files selected in {app}") }) })?;
            let files = match selection(runner).await {
                Ok(f) => f,
                Err(e) => {
                    send(ChatEvent::ToolResult { id, ok: false, summary: e.clone() })?;
                    return Ok(Some((SourceBook::default(), format!("BYTE couldn't read the {app} selection: {e}\n\n{tell}"))));
                }
            };
            if files.is_empty() {
                send(ChatEvent::ToolResult { id, ok: false, summary: "Nothing selected".into() })?;
                return Ok(Some((SourceBook::default(), format!("Nothing is selected in {app}. Ask the user to select the files first."))));
            }
            let mut parts = Vec::new();
            for f in files.iter().take(READ_MAX) {
                match crate::files::ingest(f) {
                    Ok(i) if !i.text.trim().is_empty() => parts.push(format!("File: {}\n{}", i.name, i.text.chars().take(12_000).collect::<String>())),
                    Ok(i) => parts.push(format!("File: {} (a photo or file with no text BYTE can read)", i.name)),
                    Err(e) => parts.push(format!("File: {} (couldn't be read: {e})", f.display())),
                }
            }
            let summary = format!("Read {} of {} selected files", parts.len(), files.len());
            send(ChatEvent::ToolResult { id, ok: true, summary: summary.clone() })?;
            let more = if files.len() > READ_MAX { format!("\n\n({} more files were selected; only the first {READ_MAX} were read.)", files.len() - READ_MAX) } else { String::new() };
            Ok(Some((SourceBook::default(), format!("The files the user selected in {app}:\n\n{}{more}\n\nAnswer the user's request about them.", parts.join("\n\n---\n\n")))))
        }
    }
}

#[cfg(test)]
#[path = "filectl_tests.rs"]
mod tests;
