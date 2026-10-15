//! "Find the invoice from last week": understands date words, searches only the folders the person chose (on this PC; nothing is
//! sent anywhere), reads the best few aloud and shows the best one in File Explorer, selected.
//!
//! The search itself is `filectl`'s (Windows Search, else a time-bounded walk of the folder); this adds the date words, the
//! chosen folders, and a strict rule for what may be shown: only a real path under a chosen folder, handed to Explorer as one
//! argument, never through a shell string.

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use chrono::{Datelike, Duration as Days, NaiveDate, TimeZone};

use crate::filectl::{self, Hit};
use crate::pcctl;

/// How many matches are read aloud.
pub const SPEAK_MAX: usize = 3;
/// How long one folder's walk may take when Windows Search is not available.
const WALK_BUDGET: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Lower-case words the name or text must have (safe for a search query).
    pub words: Vec<String>,
    pub when: Option<DateRange>,
}

const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

fn month_of(w: &str) -> Option<u32> {
    MONTHS.iter().position(|m| *m == w || (w.len() >= 3 && m.starts_with(w) && w.len() == 3)).map(|i| i as u32 + 1)
}

fn last_day(y: i32, m: u32) -> NaiveDate {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1).and_then(|d| d.pred_opt()).unwrap_or_else(|| NaiveDate::from_ymd_opt(y, m, 28).unwrap_or_default())
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Days::days(d.weekday().num_days_from_monday() as i64)
}

fn year_word(w: &str) -> Option<i32> {
    w.parse::<i32>().ok().filter(|y| (1990..=2100).contains(y))
}

/// Finds one date phrase in the words and returns the range plus how many words (from `at`) it used.
fn phrase_at(t: &[&str], at: usize, today: NaiveDate) -> Option<(DateRange, usize)> {
    let w = |i: usize| t.get(at + i).copied().unwrap_or("");
    let r = |from: NaiveDate, to: NaiveDate| DateRange { from, to };
    match (w(0), w(1)) {
        ("today", _) => return Some((r(today, today), 1)),
        ("yesterday", _) => return Some((r(today - Days::days(1), today - Days::days(1)), 1)),
        ("this" | "current", "week") => return Some((r(monday_of(today), today), 2)),
        ("last" | "previous", "week") => {
            let m = monday_of(today) - Days::days(7);
            return Some((r(m, m + Days::days(6)), 2));
        }
        ("this" | "current", "month") => return Some((r(today.with_day(1)?, today), 2)),
        ("last" | "previous", "month") => {
            let first = today.with_day(1)?;
            let end = first.pred_opt()?;
            return Some((r(end.with_day(1)?, end), 2));
        }
        ("this" | "current", "year") => return Some((r(NaiveDate::from_ymd_opt(today.year(), 1, 1)?, today), 2)),
        ("last" | "previous", "year") => return Some((r(NaiveDate::from_ymd_opt(today.year() - 1, 1, 1)?, NaiveDate::from_ymd_opt(today.year() - 1, 12, 31)?), 2)),
        ("recently" | "recent", _) => return Some((r(today - Days::days(14), today), 1)),
        _ => {}
    }
    // "last 3 days", "past 2 weeks", "last couple of days"
    if matches!(w(0), "last" | "past") {
        let n = w(1).parse::<i64>().ok().or(match w(1) {
            "few" => Some(3),
            "couple" => Some(2),
            _ => None,
        });
        if let Some(n) = n.filter(|n| (1..=366).contains(n)) {
            let skip_of = if w(2) == "of" { 1 } else { 0 };
            let unit = w(2 + skip_of);
            let days = match unit {
                "day" | "days" => n,
                "week" | "weeks" => n * 7,
                "month" | "months" => n * 30,
                _ => return None,
            };
            return Some((r(today - Days::days(days), today), 3 + skip_of));
        }
    }
    // "from March", "in june 2025", "from 2024", "in 2023"
    if matches!(w(0), "from" | "in" | "during" | "of") {
        if let Some(m) = month_of(w(1)) {
            let (y, used) = match year_word(w(2)) {
                Some(y) => (y, 3),
                // A month later than this one means last year's.
                None => (if m > today.month() { today.year() - 1 } else { today.year() }, 2),
            };
            return Some((r(NaiveDate::from_ymd_opt(y, m, 1)?, last_day(y, m)), used));
        }
        if let Some(y) = year_word(w(1)) {
            return Some((r(NaiveDate::from_ymd_opt(y, 1, 1)?, NaiveDate::from_ymd_opt(y, 12, 31)?), 2));
        }
    }
    None
}

const LEAD: &[&[&str]] = &[
    &["can", "you"], &["could", "you"], &["please"], &["hey", "byte"], &["show", "me"], &["find", "me"], &["find"], &["open"], &["where", "is"], &["where's"], &["wheres"], &["locate"], &["look", "for"], &["search", "for"], &["get", "me"], &["pull", "up"],
];

const FILLER: &[&str] = &[
    "my", "the", "a", "an", "file", "files", "document", "documents", "doc", "docs", "on", "in", "this", "that", "pc", "computer", "explorer", "file-explorer", "folder", "called", "named", "about", "i", "saved", "downloaded", "edited", "made", "created", "modified", "from", "of", "for", "me", "please", "somewhere", "to", "show", "it", "up", "with", "and", "or", "that", "was", "were", "last", "ive", "i've", "had", "got", "where", "is", "its", "it's", "name", "all", "any", "find", "open", "locate", "look", "search", "pull", "what", "which", "those", "these", "things", "stuff",
];

/// Reads a spoken request into words and a date range. `None` when nothing is left to look for.
pub fn parse_query(q: &str, today: NaiveDate) -> Option<Query> {
    let cleaned: String = q.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '\'' || c == '-' || c == '.' { c } else { ' ' }).collect();
    let mut toks: Vec<&str> = cleaned.split_whitespace().map(|t| t.trim_matches(['.', '-', '\''])).filter(|t| !t.is_empty()).collect();
    // Leading politeness and the verb.
    'lead: loop {
        for p in LEAD {
            if toks.len() > p.len() && toks[..p.len()] == **p {
                toks.drain(..p.len());
                continue 'lead;
            }
        }
        break;
    }
    // Date phrases, wherever they are.
    let mut when: Option<DateRange> = None;
    let mut i = 0;
    let mut kept: Vec<&str> = Vec::new();
    while i < toks.len() {
        if when.is_none() {
            if let Some((range, used)) = phrase_at(&toks, i, today) {
                when = Some(range);
                i += used;
                continue;
            }
        }
        kept.push(toks[i]);
        i += 1;
    }
    let words: Vec<String> = kept.into_iter().filter(|w| !FILLER.contains(w)).map(str::to_string).filter(|w| pcctl::find_word_ok(w)).take(6).collect();
    if words.is_empty() && when.is_none() {
        return None;
    }
    Some(Query { words, when })
}

/// Whether a request is about finding a file (so it is not mistaken for a web search).
pub fn is_find_request(q: &str) -> bool {
    let l = q.to_lowercase();
    let starts = ["find ", "find me ", "where is ", "where's ", "wheres ", "locate ", "look for ", "show me ", "pull up ", "open "];
    let l2 = l.trim_start_matches("hey byte").trim_start_matches([',', ' ']).trim_start_matches("please ").trim_start_matches("can you ").trim_start_matches("could you ");
    let verb = starts.iter().any(|s| l2.starts_with(s));
    let about_files = [" file", "document", " doc ", " docs", " pdf", "spreadsheet", "invoice", "receipt", "resume", "résumé", "photo", "picture", "presentation", "slides", " csv", "tax", "contract", "statement", "report", "essay", "paper", "homework", "download"].iter().any(|w| format!(" {l2} ").contains(w));
    let in_explorer = l2.contains("file explorer") || l2.contains("in explorer");
    let web = ["http", "www.", ".com", "online", "on the web", "on youtube", "weather", "news"].iter().any(|w| l2.contains(w));
    (verb && (about_files || in_explorer) || in_explorer && l2.contains("find")) && !web
}

// ------------------------------------------------------------------ safety

/// `path` with `.` removed and no `..` allowed; `None` when it is relative or climbs out.
pub fn clean_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => return None,
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

fn lower(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// Whether `path` may be shown: absolute, no `..`, no quote or control character, and inside one of the chosen folders.
pub fn path_allowed(path: &Path, roots: &[PathBuf]) -> bool {
    let s = path.to_string_lossy();
    if s.contains('"') || s.chars().any(|c| c.is_control()) || s.starts_with("\\\\") {
        return false;
    }
    let Some(p) = clean_path(path) else { return false };
    let pl = lower(&p);
    roots.iter().filter_map(|r| clean_path(r)).any(|r| {
        let rl = lower(&r);
        let rl = rl.trim_end_matches('\\');
        pl.len() > rl.len() && pl.starts_with(rl) && pl.as_bytes()[rl.len()] == b'\\'
    })
}

fn strip_verbatim(s: &str) -> String {
    s.strip_prefix("\\\\?\\").unwrap_or(s).to_string()
}

/// The real path (links and junctions resolved), without Windows' `\\?\` prefix.
fn real_path(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok().map(|c| PathBuf::from(strip_verbatim(&c.to_string_lossy())))
}

/// A symbolic link, or on Windows any reparse point (junction, mount point, cloud placeholder). An unreadable entry counts as one.
fn is_link(p: &Path) -> bool {
    match std::fs::symlink_metadata(p) {
        Err(_) => true,
        Ok(m) => {
            if m.file_type().is_symlink() {
                return true;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                return m.file_attributes() & 0x400 != 0;
            }
            #[cfg(not(windows))]
            false
        }
    }
}

/// Whether a search hit may be used: allowed by name, no folder between the chosen folder and the file is a link or junction, and
/// its real location is still inside the chosen folder's real location.
pub fn safe_hit(path: &Path, roots: &[PathBuf]) -> bool {
    if !path_allowed(path, roots) {
        return false;
    }
    let Some(cp) = clean_path(path) else { return false };
    let Some(root) = roots.iter().filter_map(|r| clean_path(r)).find(|r| path_allowed(&cp, std::slice::from_ref(r))) else { return false };
    let rl = lower(&root);
    let rl = rl.trim_end_matches('\\').to_string();
    for a in cp.ancestors() {
        if lower(a).trim_end_matches('\\') == rl {
            break;
        }
        if is_link(a) {
            return false;
        }
    }
    match (real_path(&cp), real_path(&root)) {
        (Some(rp), Some(rr)) => path_allowed(&rp, &[rr]),
        _ => false,
    }
}

/// The one argument that makes File Explorer open with the file selected. A single string, so spaces and commas in the path
/// never split it; it is passed to the program directly, not through `cmd` or PowerShell.
pub fn explorer_arg(path: &Path) -> String {
    format!("/select,{}", path.to_string_lossy())
}

pub fn reveal(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if !path_allowed(path, roots) {
        return Err("that file is outside the folders you chose".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe").arg(explorer_arg(path)).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        Err("showing a file in File Explorer needs Windows".into())
    }
}

/// Settings → Find files: checks a folder the person picked before it is kept. Returns the folder as it will be stored.
#[tauri::command]
pub fn control_find_folder_check(path: String) -> Result<String, String> {
    let p = clean_path(Path::new(path.trim())).ok_or("Pick a full folder path.")?;
    if !p.is_dir() {
        return Err("That folder doesn't exist.".into());
    }
    // A drive's top folder or Windows itself is too wide to be a chosen place.
    let comps = p.components().filter(|c| matches!(c, Component::Normal(_))).count();
    let low = lower(&p);
    if comps == 0 || low.contains("\\windows") || low.starts_with("c:\\windows") || low.contains("\\program files") {
        return Err("Pick a folder where you keep your own files, not a whole drive or Windows.".into());
    }
    Ok(p.to_string_lossy().into_owned())
}

// ------------------------------------------------------------------ searching

fn modified_date(path: &Path) -> Option<NaiveDate> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs() as i64;
    chrono::Local.timestamp_opt(secs, 0).single().map(|d| d.date_naive())
}

/// Keeps what changed inside the range.
pub fn in_range(modified: Option<NaiveDate>, when: &Option<DateRange>) -> bool {
    match (when, modified) {
        (None, _) => true,
        (Some(r), Some(d)) => d >= r.from && d <= r.to,
        (Some(_), None) => false,
    }
}

fn search_root(root: &Path, words: &[String]) -> Vec<(Hit, PathBuf)> {
    #[cfg(windows)]
    if !words.is_empty() {
        // Windows Search runs on its own thread so a slow index cannot hold the answer past the budget.
        let (tx, rx) = std::sync::mpsc::channel();
        let (w, r) = (words.to_vec(), root.to_path_buf());
        std::thread::spawn(move || {
            let _ = tx.send(pcctl::run_op(&pcctl::WinOp::FindFiles { words: w, root: r }));
        });
        if let Ok(Ok(out)) = rx.recv_timeout(WALK_BUDGET) {
            let hits = filectl::parse_found(&out);
            if !hits.is_empty() {
                return hits;
            }
        }
    }
    filectl::walk_find(root, words, Instant::now() + WALK_BUDGET)
}

/// The query words minus any that just name the folder being searched ("downloads", "documents", "desktop"), unless that
/// would leave nothing to look for.
fn without_place_words(words: &[String], root: &Path) -> Vec<String> {
    let name = root.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let singular = name.strip_suffix('s').unwrap_or(&name).to_string();
    let kept: Vec<String> = words.iter().filter(|w| w.as_str() != name && w.as_str() != singular).cloned().collect();
    if kept.is_empty() {
        words.to_vec()
    } else {
        kept
    }
}

/// The best matches, best first, from the chosen folders only.
pub fn search(q: &Query, roots: &[PathBuf], max: usize) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = Vec::new();
    for root in roots.iter().filter(|r| r.is_dir()) {
        // "in my downloads" names the folder being searched, not something in the file's name.
        let words = without_place_words(&q.words, root);
        let hits = search_root(root, &words);
        let ranked = filectl::rank_found(hits, &words, root);
        for p in ranked {
            if safe_hit(&p, roots) && in_range(modified_date(&p), &q.when) && !all.contains(&p) {
                all.push(p);
            }
            if all.len() >= max.max(20) {
                break;
            }
        }
    }
    // With a date and no words ("what did I download yesterday"), the newest first.
    if q.words.is_empty() {
        all.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok()));
    }
    all.truncate(max);
    all
}

/// What is said: the best few by name and folder.
pub fn say(found: &[PathBuf], opened: bool) -> String {
    let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let folder = |p: &PathBuf| p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match found {
        [] => "I didn't find a file like that in the folders you chose.".into(),
        [one] => format!("I found {}, in {}.{}", name(one), folder(one), if opened { " It's open in File Explorer." } else { "" }),
        many => {
            let top: Vec<String> = many.iter().take(SPEAK_MAX).map(name).collect();
            format!(
                "I found {} files. The best match is {}, in {}.{} Also: {}.",
                many.len(),
                name(&many[0]),
                folder(&many[0]),
                if opened { " It's open in File Explorer." } else { "" },
                top[1..].join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // 2026-10-14 is a Wednesday.
    fn today() -> NaiveDate {
        d(2026, 10, 14)
    }

    #[test]
    fn last_week_is_the_previous_monday_to_sunday() {
        let q = parse_query("find the invoice from last week", today()).unwrap();
        assert_eq!(q.words, ["invoice"]);
        assert_eq!(q.when, Some(DateRange { from: d(2026, 10, 5), to: d(2026, 10, 11) }));
    }

    #[test]
    fn other_date_words() {
        let r = |s: &str| parse_query(s, today()).unwrap().when.unwrap();
        assert_eq!(r("find my budget spreadsheet from yesterday"), DateRange { from: d(2026, 10, 13), to: d(2026, 10, 13) });
        assert_eq!(r("where's the report i made this week"), DateRange { from: d(2026, 10, 12), to: d(2026, 10, 14) });
        assert_eq!(r("find the contract from last month"), DateRange { from: d(2026, 9, 1), to: d(2026, 9, 30) });
        assert_eq!(r("find the tax return from 2024"), DateRange { from: d(2024, 1, 1), to: d(2024, 12, 31) });
        assert_eq!(r("find the receipt from march"), DateRange { from: d(2026, 3, 1), to: d(2026, 3, 31) });
        assert_eq!(r("find the receipt from december"), DateRange { from: d(2025, 12, 1), to: d(2025, 12, 31) }, "a later month means last year's");
        assert_eq!(r("find the photo from june 2023"), DateRange { from: d(2023, 6, 1), to: d(2023, 6, 30) });
        assert_eq!(r("find the pdf from the last 3 days"), DateRange { from: d(2026, 10, 11), to: today() });
        assert_eq!(r("find the pdf i downloaded in the past 2 weeks"), DateRange { from: d(2026, 9, 30), to: today() });
        assert_eq!(r("find the resume i worked on recently"), DateRange { from: d(2026, 9, 30), to: today() });
        assert_eq!(r("find the invoice from last year"), DateRange { from: d(2025, 1, 1), to: d(2025, 12, 31) });
    }

    #[test]
    fn the_folder_being_searched_is_not_a_word_in_the_name() {
        let root = Path::new("/x/Downloads");
        let w = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(without_place_words(&w(&["byte", "setup", "downloads"]), root), w(&["byte", "setup"]));
        assert_eq!(without_place_words(&w(&["download"]), root), w(&["download"]), "never leaves nothing to look for");
        assert_eq!(without_place_words(&w(&["invoice"]), root), w(&["invoice"]));
    }

    #[test]
    fn words_are_what_is_left() {
        let q = |s: &str| parse_query(s, today()).unwrap().words;
        assert_eq!(q("Hey BYTE, can you find my tax return pdf on my computer"), ["tax", "return", "pdf"]);
        assert_eq!(q("where's the resume"), ["resume"]);
        assert_eq!(q("find me the electric bill from last month"), ["electric", "bill"]);
        assert_eq!(q("open the budget spreadsheet in file explorer"), ["budget", "spreadsheet"]);
    }

    #[test]
    fn only_a_date_is_a_listing_of_that_time() {
        let q = parse_query("show me what i downloaded yesterday", today()).unwrap();
        assert!(q.words.is_empty(), "{:?}", q.words);
        assert_eq!(q.when, Some(DateRange { from: d(2026, 10, 13), to: d(2026, 10, 13) }));
    }

    #[test]
    fn nothing_to_look_for_is_none() {
        assert!(parse_query("find", today()).is_none());
        assert!(parse_query("please find the file", today()).is_none());
    }

    #[test]
    fn unsafe_words_never_reach_a_search() {
        let q = parse_query("find invoice*; del c:\\ \"x\" a'b", today()).unwrap();
        assert!(q.words.iter().all(|w| pcctl::find_word_ok(w)), "{:?}", q.words);
        assert!(!q.words.iter().any(|w| w.contains(['*', ';', '\\', '"', ':'])));
    }

    #[test]
    fn find_requests_are_told_from_the_web() {
        for yes in ["find the invoice from last week", "where's my resume", "find my tax return pdf", "hey byte find the budget spreadsheet in file explorer", "show me the photos i downloaded yesterday"] {
            assert!(is_find_request(yes), "{yes}");
        }
        for no in ["find the best pizza near me", "where is the Eiffel tower", "open notepad", "find a report on youtube about cats", "what's the weather"] {
            assert!(!is_find_request(no), "{no}");
        }
    }

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("C:\\Users\\sam\\Documents"), PathBuf::from("D:\\Work\\")]
    }

    #[test]
    fn only_paths_inside_the_chosen_folders_may_be_shown() {
        let ok = |s: &str| path_allowed(Path::new(s), &roots());
        // On Linux a `C:\...` path is not "absolute", so the path rules are checked on the shape that is the same everywhere.
        if cfg!(windows) {
            assert!(ok("C:\\Users\\sam\\Documents\\invoice march.pdf"));
            assert!(ok("c:\\users\\SAM\\documents\\sub\\a.txt"));
            assert!(ok("D:\\Work\\x.docx"));
            assert!(!ok("C:\\Users\\sam\\Documents"), "the folder itself is not a file in it");
            assert!(!ok("C:\\Users\\sam\\DocumentsOld\\a.txt"));
            assert!(!ok("C:\\Users\\sam\\Documents\\..\\Secrets\\a.txt"));
            assert!(!ok("C:\\Windows\\System32\\cmd.exe"));
            assert!(!ok("\\\\server\\share\\a.txt"));
            assert!(!ok("C:\\Users\\sam\\Documents\\a\".txt"));
            assert!(!ok("Documents\\a.txt"));
        } else {
            let r = vec![PathBuf::from("/home/sam/Documents")];
            let ok = |s: &str| path_allowed(Path::new(s), &r);
            assert!(ok("/home/sam/Documents/a b.pdf"));
            assert!(!ok("/home/sam/Documents"));
            assert!(!ok("/home/sam/Documents2/a.pdf"));
            assert!(!ok("/home/sam/Documents/../x.pdf"));
            assert!(!ok("/etc/passwd"));
            assert!(!ok("Documents/a.pdf"));
            assert!(!ok("/home/sam/Documents/a\"b.pdf"));
            assert!(!ok("/home/sam/Documents/a\nb.pdf"));
        }
    }

    #[test]
    fn explorer_gets_one_argument_even_with_spaces_and_commas() {
        let a = explorer_arg(Path::new("C:\\Users\\sam\\My Docs\\invoice, march & more.pdf"));
        assert_eq!(a, "/select,C:\\Users\\sam\\My Docs\\invoice, march & more.pdf");
        assert_eq!(a.matches("/select,").count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_hit_through_a_link_out_of_the_chosen_folder_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.pdf"), "x").unwrap();
        std::fs::write(root.path().join("ok.pdf"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        let roots = vec![root.path().to_path_buf()];
        assert!(safe_hit(&root.path().join("ok.pdf"), &roots));
        assert!(!safe_hit(&root.path().join("link").join("secret.pdf"), &roots), "through a link");
        assert!(!safe_hit(&root.path().join("missing.pdf"), &roots), "a file that is not there");
        assert!(!safe_hit(&outside.path().join("secret.pdf"), &roots), "outside by name");
    }

    #[test]
    fn dates_filter_by_modified_day() {
        let r = Some(DateRange { from: d(2026, 10, 5), to: d(2026, 10, 11) });
        assert!(in_range(Some(d(2026, 10, 5)), &r) && in_range(Some(d(2026, 10, 11)), &r));
        assert!(!in_range(Some(d(2026, 10, 12)), &r) && !in_range(Some(d(2026, 10, 4)), &r));
        assert!(!in_range(None, &r));
        assert!(in_range(None, &None));
    }

    #[test]
    fn the_answer_names_the_best_and_reads_a_few() {
        let f = |s: &str| PathBuf::from(s);
        assert_eq!(say(&[], false), "I didn't find a file like that in the folders you chose.");
        assert_eq!(say(&[f("/a/Docs/inv.pdf")], true), "I found inv.pdf, in Docs. It's open in File Explorer.");
        let t = say(&[f("/a/Docs/one.pdf"), f("/a/Work/two.pdf"), f("/a/Work/three.pdf"), f("/a/x/four.pdf")], true);
        assert_eq!(t, "I found 4 files. The best match is one.pdf, in Docs. It's open in File Explorer. Also: two.pdf, three.pdf.");
    }

    #[test]
    fn a_real_folder_is_searched_within_dates_and_chosen_roots_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("invoice march.pdf"), "x").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("invoice other.pdf"), "x").unwrap();
        let roots = vec![dir.path().to_path_buf()];
        let q = Query { words: vec!["invoice".into()], when: None };
        let found = search(&q, &roots, 5);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with("invoice march.pdf"));
        // Today's file is not in last year's range.
        let old = Query { words: vec!["invoice".into()], when: Some(DateRange { from: d(2001, 1, 1), to: d(2001, 12, 31) }) };
        assert!(search(&old, &roots, 5).is_empty());
        let now = chrono::Local::now().date_naive();
        let today_q = Query { words: vec!["invoice".into()], when: Some(DateRange { from: now - Days::days(1), to: now + Days::days(1) }) };
        assert_eq!(search(&today_q, &roots, 5).len(), 1);
    }
}
