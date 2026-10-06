//! Notes (Phase 11): plain Markdown files the user owns, in
//! `~/Documents/BYTE/Notes/<folder>/<title>.md` (or the folder chosen in
//! Settings), with a small front matter block (tags, created, source URL, chat).
//! They open in any editor or Obsidian and outlive BYTE. With the knowledge
//! base on, the notes folder is one of its sources, so BYTE can cite notes.
//!
//! The web clipper (`byte://clip?url=…&sel=…`, from a bookmarklet) saves a page
//! as a note in "Clips": BYTE fetches and reads the page itself.
//!
//! Every path stays inside the notes folder: names are cleaned, folders are one
//! level deep, and ids that try to climb out are refused.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const INBOX: &str = "Inbox";
pub const CLIPS: &str = "Clips";
/// Longest page text kept in a clip.
const CLIP_CHARS: usize = 20_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// "<folder>/<file>.md", relative to the notes folder.
    pub id: String,
    pub title: String,
    pub folder: String,
    pub tags: Vec<String>,
    /// Milliseconds.
    pub created: i64,
    pub updated: i64,
    pub source: String,
    pub chat: String,
    pub body: String,
    /// The full path, for "Reveal in Finder".
    pub path: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteInput {
    /// Set when changing an existing note.
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub chat: String,
}

// ------------------------------------------------------------------ names and paths

/// A title or folder name that is safe as one path part: ordinary characters, no leading dots, ≤ 80 chars.
pub fn clean_name(name: &str, fallback: &str) -> String {
    let c: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_()&,'!?".contains(c) { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let c: String = c.trim_start_matches(['.', '-', ' ']).chars().take(80).collect();
    let c = c.trim().trim_end_matches(['.', ' ']).to_string();
    if c.is_empty() { fallback.to_string() } else { c }
}

/// The note's full path for an id, only if it stays inside `root` (folder/file.md, one level).
pub fn resolve(root: &Path, id: &str) -> AppResult<PathBuf> {
    let parts: Vec<&str> = id.split('/').collect();
    let ok = parts.len() == 2
        && parts.iter().all(|p| !p.is_empty() && *p != "." && *p != ".." && !p.starts_with('.') && !p.contains('\\'))
        && parts[1].ends_with(".md");
    if !ok {
        return Err(AppError::msg("That isn't one of your notes."));
    }
    Ok(root.join(parts[0]).join(parts[1]))
}

/// A free file name in `dir` for `title` ("Title.md", "Title 2.md", …), keeping `keep` if it's the same file.
fn free_name(dir: &Path, title: &str, keep: Option<&Path>) -> String {
    let stem = clean_name(title, "Untitled");
    for n in 1.. {
        let name = if n == 1 { format!("{stem}.md") } else { format!("{stem} {n}.md") };
        let p = dir.join(&name);
        if !p.exists() || keep == Some(p.as_path()) {
            return name;
        }
    }
    unreachable!()
}

// ------------------------------------------------------------------ front matter

/// Splits "---\nkey: value\n---\nbody" into (fields, body). Text without front matter is all body.
pub fn parse(text: &str) -> (Vec<(String, String)>, String) {
    let t = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Some(rest) = t.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let head = &rest[..end];
            let body = rest[end + 4..].strip_prefix('\n').unwrap_or(&rest[end + 4..]);
            let fields = head
                .lines()
                .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_lowercase(), v.trim().trim_matches('"').to_string())))
                .collect();
            return (fields, body.to_string());
        }
    }
    (vec![], t.to_string())
}

/// "[a, b]" or "a, b" → tags.
pub fn tags_of(v: &str) -> Vec<String> {
    v.trim_matches(['[', ']']).split(',').map(|t| t.trim().trim_matches(['"', '\'', '#']).to_string()).filter(|t| !t.is_empty()).collect()
}

fn quote(v: &str) -> String {
    let v = v.replace(['\n', '\r'], " ");
    if v.contains(':') || v.contains('#') || v.starts_with(['[', '"', '\'']) { format!("\"{}\"", v.replace('"', "'")) } else { v }
}

/// The file's text: front matter (only fields that are set) then the body.
pub fn print(title: &str, tags: &[String], created: i64, source: &str, chat: &str, body: &str) -> String {
    let mut head = format!("---\ntitle: {}\n", quote(title));
    if !tags.is_empty() {
        head.push_str(&format!("tags: [{}]\n", tags.iter().map(|t| clean_tag(t)).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(", ")));
    }
    let when = chrono::DateTime::from_timestamp_millis(created).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
    head.push_str(&format!("created: {when}\n"));
    if !source.is_empty() {
        head.push_str(&format!("source: {}\n", quote(source)));
    }
    if !chat.is_empty() {
        head.push_str(&format!("chat: {}\n", quote(chat)));
    }
    format!("{head}---\n\n{}\n", body.trim_end())
}

fn clean_tag(t: &str) -> String {
    t.trim().trim_start_matches('#').chars().filter(|c| c.is_alphanumeric() || "-_ /".contains(*c)).collect::<String>().trim().to_lowercase()
}

fn millis(t: std::time::SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Reads one note file.
pub fn read(root: &Path, path: &Path) -> Option<Note> {
    let text = std::fs::read_to_string(path).ok()?;
    let meta = std::fs::metadata(path).ok()?;
    let rel = path.strip_prefix(root).ok()?;
    let folder = rel.parent()?.to_string_lossy().to_string();
    let file = rel.file_name()?.to_string_lossy().to_string();
    let (fields, body) = parse(&text);
    let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let updated = meta.modified().map(millis).unwrap_or(0);
    let created = chrono::NaiveDateTime::parse_from_str(&get("created"), "%Y-%m-%d %H:%M")
        .ok()
        .and_then(|d| d.and_local_timezone(chrono::Local).single())
        .map(|d| d.timestamp_millis())
        .unwrap_or(updated);
    let title = Some(get("title")).filter(|t| !t.is_empty()).unwrap_or_else(|| file.trim_end_matches(".md").to_string());
    Some(Note { id: format!("{folder}/{file}"), title, folder, tags: tags_of(&get("tags")), created, updated, source: get("source"), chat: get("chat"), body, path: path.display().to_string() })
}

/// Every note, newest first (folders one level deep; hidden files skipped).
pub fn list(root: &Path) -> Vec<Note> {
    let mut out = vec![];
    let Ok(dirs) = std::fs::read_dir(root) else { return out };
    for d in dirs.flatten().filter(|d| d.path().is_dir() && !d.file_name().to_string_lossy().starts_with('.')) {
        let Ok(files) = std::fs::read_dir(d.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            let name = f.file_name().to_string_lossy().to_string();
            if p.is_file() && name.ends_with(".md") && !name.starts_with('.') {
                out.extend(read(root, &p));
            }
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out
}

/// Writes a note (new or changed). A changed title or folder moves the file.
pub fn save(root: &Path, input: &NoteInput, now: i64) -> AppResult<Note> {
    let folder = clean_name(&input.folder, INBOX);
    let dir = root.join(&folder);
    std::fs::create_dir_all(&dir)?;
    let old = input.id.as_deref().map(|id| resolve(root, id)).transpose()?;
    let prior = old.as_deref().and_then(|p| read(root, p));
    let name = free_name(&dir, &input.title, old.as_deref());
    let path = dir.join(name);
    let created = prior.as_ref().map(|p| p.created).unwrap_or(now);
    let title = clean_name(&input.title, "Untitled");
    std::fs::write(&path, print(&title, &input.tags, created, &input.source, &input.chat, &input.body))?;
    if let Some(old) = old.filter(|o| *o != path && o.exists()) {
        std::fs::remove_file(old)?;
    }
    read(root, &path).ok_or_else(|| AppError::msg("BYTE couldn't read the note back."))
}

/// Notes matching all the words of `query` (title, tags and text), best first: title hits, then how often.
pub fn search(notes: &[Note], query: &str) -> Vec<Note> {
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return notes.to_vec();
    }
    let mut scored: Vec<(usize, &Note)> = notes
        .iter()
        .filter_map(|n| {
            let title = n.title.to_lowercase();
            let all = format!("{title} {} {}", n.tags.join(" "), n.body.to_lowercase());
            words.iter().all(|w| all.contains(w)).then(|| (words.iter().map(|w| if title.contains(w) { 100 } else { 0 } + all.matches(w.as_str()).count()).sum(), n))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.updated.cmp(&a.1.updated)));
    scored.into_iter().map(|(_, n)| n.clone()).collect()
}

// ------------------------------------------------------------------ clips

/// A clipped page as a note body: where it came from, the selection as a quote, then the page.
pub fn clip_body(url: &str, title: &str, selection: &str, text: &str, summary: &[String]) -> String {
    let mut b = format!("Clipped from [{}]({url}) on {}.\n", title.replace(['[', ']'], ""), chrono::Local::now().format("%B %-d, %Y"));
    if !summary.is_empty() {
        b.push_str("\n> **TL;DR**\n");
        for s in summary {
            b.push_str(&format!("> - {s}\n"));
        }
    }
    let sel = selection.trim();
    if !sel.is_empty() {
        b.push_str("\n## What you selected\n\n");
        for l in sel.lines() {
            b.push_str(&format!("> {l}\n"));
        }
    }
    let page: String = text.chars().take(CLIP_CHARS).collect();
    b.push_str("\n## The page\n\n");
    b.push_str(page.trim());
    if text.chars().count() > CLIP_CHARS {
        b.push_str("\n\n*(The rest of the page wasn't kept.)*");
    }
    b
}

// ------------------------------------------------------------------ app glue

/// The notes folder for these settings, without creating it (backups).
pub fn dir_for(s: &crate::settings::Settings) -> PathBuf {
    match s.notes_dir.as_deref().filter(|d| !d.trim().is_empty()) {
        Some(d) => PathBuf::from(d),
        None => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Documents").join("BYTE").join("Notes"),
    }
}

/// The notes folder: the one chosen in Settings, else ~/Documents/BYTE/Notes.
pub async fn root(app: &AppHandle) -> AppResult<PathBuf> {
    let state = app.state::<AppState>();
    let chosen = state.settings.lock().await.notes_dir.clone();
    let dir = match chosen.filter(|d| !d.trim().is_empty()) {
        Some(d) => PathBuf::from(d),
        None => crate::paths::user_documents_dir(app)?.join("BYTE").join("Notes"),
    };
    std::fs::create_dir_all(dir.join(INBOX))?;
    Ok(dir)
}

/// Keeps the knowledge base's copy of the notes current (when the knowledge base is on).
fn refresh_kb(app: &AppHandle, root: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if !state.settings.lock().await.kb_enabled {
            return;
        }
        if let Ok(id) = crate::kb::add_source(&state.db, &root) {
            if let Err(e) = crate::kb::index(&app, Some(id)).await {
                log::warn!("notes in the knowledge base: {e}");
            }
        }
    });
}

pub async fn save_note(app: &AppHandle, input: &NoteInput) -> AppResult<Note> {
    let r = root(app).await?;
    let n = save(&r, input, chrono::Utc::now().timestamp_millis())?;
    refresh_kb(app, r);
    Ok(n)
}

/// Fetches a page and saves it as a note in Clips.
pub async fn clip(app: &AppHandle, url: &str, selection: &str) -> AppResult<Note> {
    let state = app.state::<AppState>();
    let page = crate::tools::fetch::fetch_page(&state.net, url).await?;
    let title = if page.title.trim().is_empty() { url.to_string() } else { page.title.clone() };
    let body = clip_body(&page.url, &title, selection, &page.text, &[]);
    let input = NoteInput { title, folder: CLIPS.into(), tags: vec!["clip".into()], body, source: page.url.clone(), ..Default::default() };
    save_note(app, &input).await
}

#[tauri::command]
pub async fn notes_list(app: AppHandle, query: Option<String>) -> AppResult<Vec<Note>> {
    crate::lock::ensure(&app.state::<crate::state::AppState>())?;
    crate::kids::grownups_only()?;
    let r = root(&app).await?;
    let all = list(&r);
    Ok(match query.filter(|q| !q.trim().is_empty()) {
        Some(q) => search(&all, &q),
        None => all,
    })
}

#[tauri::command]
pub async fn note_get(app: AppHandle, id: String) -> AppResult<Note> {
    crate::lock::ensure(&app.state::<crate::state::AppState>())?;
    crate::kids::grownups_only()?;
    let r = root(&app).await?;
    read(&r, &resolve(&r, &id)?).ok_or_else(|| AppError::msg("That note isn't there any more."))
}

#[tauri::command]
pub async fn note_save(app: AppHandle, note: NoteInput) -> AppResult<Note> {
    save_note(&app, &note).await
}

/// Moves a note to the Trash (macOS) or deletes it.
#[tauri::command]
pub async fn note_delete(app: AppHandle, id: String) -> AppResult<()> {
    let r = root(&app).await?;
    let p = resolve(&r, &id)?;
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        let t = crate::upkeep::trash(&crate::macctl::MacRunner, &home, std::slice::from_ref(&p)).await;
        if t.moved == 0 {
            return Err(AppError::msg(t.error.unwrap_or_else(|| "BYTE couldn't move that note to the Trash.".into())));
        }
    }
    #[cfg(not(target_os = "macos"))]
    std::fs::remove_file(&p)?;
    refresh_kb(&app, r);
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesInfo {
    pub dir: String,
    pub folders: Vec<String>,
}

#[tauri::command]
pub async fn notes_info(app: AppHandle) -> AppResult<NotesInfo> {
    let r = root(&app).await?;
    let mut folders: Vec<String> = std::fs::read_dir(&r)?
        .flatten()
        .filter(|d| d.path().is_dir())
        .map(|d| d.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.'))
        .collect();
    folders.sort_by_key(|f| (f != INBOX, f.to_lowercase()));
    Ok(NotesInfo { dir: r.display().to_string(), folders })
}

/// Clips a page into the notes (the palette's "Clip a page", or a pasted link).
#[tauri::command]
pub async fn note_clip(app: AppHandle, url: String, selection: Option<String>) -> AppResult<Note> {
    clip(&app, &url, selection.as_deref().unwrap_or("")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(title: &str, folder: &str, body: &str) -> NoteInput {
        NoteInput { title: title.into(), folder: folder.into(), body: body.into(), tags: vec!["Ideas".into(), "#work".into()], ..Default::default() }
    }

    #[test]
    fn names_stay_inside_the_folder() {
        assert_eq!(clean_name("../../etc/passwd", "x"), "etc passwd");
        assert_eq!(clean_name(".hidden", "x"), "hidden");
        assert_eq!(clean_name("   ", "Inbox"), "Inbox");
        assert_eq!(clean_name("Trip: Lisbon / May", "x"), "Trip Lisbon May");
        let root = Path::new("/n");
        assert!(resolve(root, "Inbox/a.md").is_ok());
        for bad in ["../a.md", "Inbox/../../a.md", "a.md", "Inbox/x/y.md", "Inbox/.a.md", "Inbox/a.txt", "/etc/a.md", "Inbox\\..\\a.md"] {
            assert!(resolve(root, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn front_matter_round_trips() {
        let t = print("Trip: Lisbon", &["travel".into(), "#Food".into()], 1_727_800_000_000, "https://x.com/a", "", "Body\n\n- one");
        let (f, body) = parse(&t);
        assert_eq!(body.trim(), "Body\n\n- one");
        assert_eq!(f.iter().find(|(k, _)| k == "title").unwrap().1, "Trip: Lisbon");
        assert_eq!(tags_of(&f.iter().find(|(k, _)| k == "tags").unwrap().1), vec!["travel", "food"]);
        // No front matter: all body.
        assert_eq!(parse("# Hi\ntext").1, "# Hi\ntext");
    }

    #[test]
    fn saves_lists_renames_and_searches() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        let a = save(r, &input("Groceries", "", "milk, eggs, coffee filters"), 1).unwrap();
        assert_eq!((a.folder.as_str(), a.id.as_str()), ("Inbox", "Inbox/Groceries.md"));
        assert_eq!(a.tags, vec!["ideas", "work"]);
        // Same title again: a second file, not an overwrite.
        let b = save(r, &input("Groceries", "", "bread"), 2).unwrap();
        assert_eq!(b.id, "Inbox/Groceries 2.md");
        // Rename + move keeps one file and the created time.
        let mut change = input("Shopping list", "Home", "milk, eggs, coffee filters, bread");
        change.id = Some(a.id.clone());
        let moved = save(r, &change, 9).unwrap();
        assert_eq!(moved.id, "Home/Shopping list.md");
        assert!(!r.join("Inbox/Groceries.md").exists());
        assert_eq!(list(r).len(), 2);
        let found = search(&list(r), "coffee");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Shopping list");
        assert!(search(&list(r), "coffee tea").is_empty());
        // A hostile id is refused.
        let mut bad = input("x", "", "");
        bad.id = Some("../../x.md".into());
        assert!(save(r, &bad, 1).is_err());
    }

    #[test]
    fn clips_keep_the_selection_and_the_source() {
        let b = clip_body("https://ex.com/a", "A [great] page", "the key line\nand more", &"word ".repeat(6000), &["Short point".into()]);
        assert!(b.starts_with("Clipped from [A great page](https://ex.com/a)"));
        assert!(b.contains("> the key line\n> and more"));
        assert!(b.contains("> - Short point"));
        assert!(b.contains("wasn't kept"));
    }
}
