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
use crate::tools::SourceBook;

/// Results listed when finding files.
const FIND_MAX: usize = 20;
/// Selected files read at once (for questions about them).
const READ_MAX: usize = 5;
/// Files changed in the last two minutes may still be downloading: left alone.
const SETTLE: Duration = Duration::from_secs(120);

/// The folders people name, relative to home.
const FOLDERS: &[(&str, &str)] = &[("downloads", "Downloads"), ("desktop", "Desktop"), ("documents", "Documents"), ("pictures", "Pictures"), ("movies", "Movies"), ("music", "Music")];

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
    " file", "files", "document", " doc ", " docs", " pdf", ".pdf", "docx", "spreadsheet", "xlsx", "presentation", "pptx", "keynote", "screenshot", "on my mac", "on this mac", "in my downloads", "in my documents", "on my desktop", "in finder", "resume", "résumé", "receipt", "invoice", "tax return", "my notes file", "the photo of", "photos of",
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
    if starts(&l, &["find ", "where is ", "where's ", "wheres ", "locate ", "search my mac for ", "look for "]) && FILE_WORDS.iter().any(|w| padded.contains(w)) {
        let rest = ["search my mac for ", "where is ", "where's ", "wheres ", "locate ", "look for ", "find "].iter().find_map(|p| l.strip_prefix(p)).unwrap_or(&l);
        let query = find_words(rest);
        if !query.is_empty() {
            return Some(Ask::Find { query });
        }
    }
    None
}

/// The words to search for: "my tax return pdf on my mac" → "tax return".
fn find_words(s: &str) -> String {
    const DROP: &[&str] = &["my", "the", "a", "an", "file", "files", "document", "documents", "doc", "docs", "on", "in", "this", "mac", "computer", "folder", "that", "called", "named", "about", "pdf", "pdfs", "i", "saved", "downloaded", "somewhere", "for"];
    s.split_whitespace().filter(|w| !DROP.contains(w)).collect::<Vec<_>>().join(" ").trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

fn number_in(l: &str) -> Option<u32> {
    l.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).next()
}

/// Asks this module should answer (module on, macOS).
pub fn applies(enabled: bool, q: &str) -> bool {
    cfg!(target_os = "macos") && enabled && ask(q).is_some()
}

// ------------------------------------------------------------------ tidying

/// The group a file goes to when tidying by kind.
pub fn kind_folder(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    KINDS.iter().find(|(_, exts)| exts.contains(&ext.as_str())).map(|(k, _)| *k).unwrap_or("Other")
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

/// Where each top-level file in `dir` goes: (from, to), in name order.
pub fn tidy_plan(dir: &Path, by: By, now: SystemTime) -> AppResult<Vec<(PathBuf, PathBuf)>> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| AppError::msg(format!("couldn't read {}: {e}", dir.display())))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    let mut taken = std::collections::HashSet::new();
    let mut moves = Vec::new();
    for e in entries {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() || leave_alone(&path, meta.modified().ok(), now) {
            continue;
        }
        let group = match by {
            By::Kind => kind_folder(&path).to_string(),
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
    let mut done = Vec::new();
    for (from, to) in moves {
        if let Some(parent) = to.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return (done, Some(format!("couldn't make {}: {e}", parent.display())));
            }
        }
        if to.exists() {
            continue;
        }
        match std::fs::rename(from, to) {
            Ok(()) => done.push((from.clone(), to.clone())),
            Err(e) => return (done, Some(format!("couldn't move {}: {e}", from.display()))),
        }
    }
    (done, None)
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

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
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
    match runner.run(&Command::Osa { script: FINDER_SELECTION, args: vec![] }).await {
        Ok(out) => Ok(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect()),
        Err(e) => Err(e.text("Finder")),
    }
}

/// Does what the message asks with files. Returns notes for the answer.
pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    run_with(turn, question, &MacRunner, SystemTime::now(), cancel, send).await
}

pub(crate) async fn run_with(turn: &Turn<'_>, question: &str, runner: &dyn Runner, now: SystemTime, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(a) = ask(question) else { return Ok(None) };
    let id = format!("byte_files_{}", uuid::Uuid::new_v4().simple());
    let tell = macctl::TELL;
    match a {
        Ask::Find { query } => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_find_files".into(), args: json!({ "app": "Finder", "what": format!("Look for files about \"{query}\"") }) })?;
            let out = runner.run(&Command::Exec { program: "mdfind", args: vec!["-onlyin".into(), home().display().to_string(), query.clone()] }).await;
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
            let dir = home().join(&folder);
            let what = format!("Tidy {folder} into folders by {}", if by == By::Kind { "kind" } else { "month" });
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_organize".into(), args: json!({ "app": "Finder", "what": what }) })?;
            let moves = match tidy_plan(&dir, by, now) {
                Ok(m) => m,
                Err(e) => {
                    send(ChatEvent::ToolResult { id, ok: false, summary: e.to_string() })?;
                    return Ok(Some((SourceBook::default(), format!("BYTE couldn't read {folder}: {e}. macOS may need BYTE allowed in System Settings → Privacy & Security → Files and Folders.\n\n{tell}"))));
                }
            };
            if moves.is_empty() {
                send(ChatEvent::ToolResult { id, ok: true, summary: "Nothing to move".into() })?;
                return Ok(Some((SourceBook::default(), format!("{folder} has no loose files to tidy (folders and files still downloading are left alone).\n\n{tell}"))));
            }
            let fields = vec![
                ("Folder".to_string(), tilde(&dir)),
                ("Moves".to_string(), format!("{} files into: {}", moves.len(), plan_summary(&moves))),
                ("Note".to_string(), "Only loose files move (not folders), nothing is deleted, and Undo puts everything back.".to_string()),
            ];
            if !macctl::ask_ok(&what, "Finder", fields, cancel, send).await? {
                send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
                return Ok(Some((SourceBook::default(), format!("The user chose not to tidy {folder}. Nothing was moved. Say so in one sentence."))));
            }
            let (done, err) = apply_moves(&moves);
            let undo = (!done.is_empty()).then(|| macctl::keep_undo(Undo::Moves(done.clone())));
            let detail = format!("{} of {} files moved · {}", done.len(), moves.len(), plan_summary(&done));
            send(ChatEvent::ToolResult { id, ok: err.is_none(), summary: detail.clone() })?;
            turn.log.record("mac_organize", &json!({ "folder": folder }), err.is_none(), &detail);
            card(send, "Finder", &what, &detail, err.is_none(), undo)?;
            let problem = err.map(|e| format!(" It stopped early: {e}.")).unwrap_or_default();
            Ok(Some((SourceBook::default(), format!("Done: BYTE moved {} files in {folder} into folders ({}).{problem} Undo on the card puts them back.\n\n{tell}", done.len(), plan_summary(&done)))))
        }
        Ask::Convert { to, max } => {
            let files = match selection(runner).await {
                Ok(f) => f,
                Err(e) => return Ok(Some((SourceBook::default(), format!("BYTE couldn't read the Finder selection: {e}\n\n{tell}")))),
            };
            let plan = convert_plan(&files, to.as_deref(), max);
            if plan.is_empty() {
                return Ok(Some((SourceBook::default(), "No photos are selected in Finder. Ask the user to select the photos in Finder first, then ask again.".to_string())));
            }
            let what = match (&to, max) {
                (Some(t), Some(m)) => format!("Save {} photos as {} at most {m} px", plan.len(), t.to_uppercase()),
                (Some(t), None) => format!("Save {} photos as {}", plan.len(), t.to_uppercase()),
                (None, Some(m)) => format!("Save smaller copies of {} photos (at most {m} px)", plan.len()),
                (None, None) => format!("Copy {} photos", plan.len()),
            };
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_convert".into(), args: json!({ "app": "Finder", "what": what }) })?;
            let names: Vec<String> = plan.iter().take(8).map(|(_, out, _)| out.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()).collect();
            let fields = vec![
                ("New files".to_string(), format!("{}{}", names.join(", "), if plan.len() > 8 { format!(" and {} more", plan.len() - 8) } else { String::new() })),
                ("Note".to_string(), "Saved next to the originals, which stay as they are.".to_string()),
            ];
            if !macctl::ask_ok(&what, "Finder", fields, cancel, send).await? {
                send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
                return Ok(Some((SourceBook::default(), "The user chose not to convert the photos. Nothing was changed. Say so in one sentence.".to_string())));
            }
            let mut made = Vec::new();
            let mut failed = Vec::new();
            for (src, out, fmt) in &plan {
                if cancel.is_cancelled() {
                    return Err(AppError::Cancelled);
                }
                match runner.run(&sips_command(src, out, *fmt, max)).await {
                    Ok(_) => made.push(out.clone()),
                    Err(e) => failed.push(format!("{}: {}", src.file_name().and_then(|n| n.to_str()).unwrap_or("?"), e.text("sips"))),
                }
            }
            let undo = (!made.is_empty()).then(|| macctl::keep_undo(Undo::Created(made.clone())));
            let detail = format!("{} of {} saved", made.len(), plan.len());
            send(ChatEvent::ToolResult { id, ok: failed.is_empty(), summary: detail.clone() })?;
            card(send, "Finder", &what, &detail, failed.is_empty(), undo)?;
            let problems = if failed.is_empty() { String::new() } else { format!(" These didn't work: {}.", failed.join("; ")) };
            Ok(Some((SourceBook::default(), format!("Done: BYTE saved {} new photo files next to the originals.{problems}\n\n{tell}", made.len()))))
        }
        Ask::AboutSelection => {
            send(ChatEvent::ToolCall { id: id.clone(), name: "mac_finder_selection".into(), args: json!({ "app": "Finder", "what": "Read the files selected in Finder" }) })?;
            let files = match selection(runner).await {
                Ok(f) => f,
                Err(e) => {
                    send(ChatEvent::ToolResult { id, ok: false, summary: e.clone() })?;
                    return Ok(Some((SourceBook::default(), format!("BYTE couldn't read the Finder selection: {e}\n\n{tell}"))));
                }
            };
            if files.is_empty() {
                send(ChatEvent::ToolResult { id, ok: false, summary: "Nothing selected".into() })?;
                return Ok(Some((SourceBook::default(), "Nothing is selected in Finder. Ask the user to select the files first.".to_string())));
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
            Ok(Some((SourceBook::default(), format!("The files the user selected in Finder:\n\n{}{more}\n\nAnswer the user's request about them.", parts.join("\n\n---\n\n")))))
        }
    }
}

#[cfg(test)]
#[path = "filectl_tests.rs"]
mod tests;
