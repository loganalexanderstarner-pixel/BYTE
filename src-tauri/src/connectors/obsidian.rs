//! An Obsidian vault: a folder of Markdown notes on this Mac. BYTE searches it
//! and, after the user's OK, adds new notes in the vault's `BYTE/` folder
//! (never changing notes that are already there). Undo removes a note BYTE made.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Notes read per search (big vaults stay quick).
const MAX_FILES: usize = 20_000;
const MAX_BYTES: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    pub title: String,
    /// The passage around the first match.
    pub snippet: String,
    pub score: usize,
}

/// A vault folder must exist and be a folder.
pub fn check(path: &str) -> AppResult<PathBuf> {
    let p = PathBuf::from(path.trim());
    if !p.is_dir() {
        return Err(AppError::msg("That folder doesn't exist."));
    }
    Ok(p)
}

/// The vault's notes (hidden folders like .obsidian and .trash skipped).
pub fn notes(vault: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![vault.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md")) && e.metadata().map(|m| m.len() <= MAX_BYTES).unwrap_or(false) {
                out.push(p);
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out
}

fn words(q: &str) -> Vec<String> {
    const SKIP: [&str; 24] = ["the", "and", "for", "what", "about", "does", "did", "my", "notes", "note", "vault", "obsidian", "say", "says", "in", "of", "on", "search", "to", "with", "me", "find", "tell", "do"];
    q.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !SKIP.contains(w))
        .map(str::to_string)
        .collect()
}

/// Notes that mention the words, best first (a word in the title counts three times).
pub fn search(vault: &Path, query: &str, limit: usize) -> Vec<Hit> {
    let terms = words(query);
    if terms.is_empty() {
        return vec![];
    }
    let mut hits: Vec<Hit> = notes(vault)
        .into_iter()
        .filter_map(|path| {
            let title = path.file_stem()?.to_string_lossy().to_string();
            let text = std::fs::read_to_string(&path).ok()?;
            let (tl, bl) = (title.to_lowercase(), text.to_lowercase());
            let score: usize = terms.iter().map(|t| tl.matches(t.as_str()).count() * 3 + bl.matches(t.as_str()).count()).sum();
            let all = terms.iter().all(|t| tl.contains(t.as_str()) || bl.contains(t.as_str()));
            (score > 0 && (all || terms.len() == 1)).then(|| {
                let at = terms.iter().filter_map(|t| bl.find(t.as_str())).min().unwrap_or(0);
                Hit { snippet: around(&text, at, 500), path, title, score }
            })
        })
        .collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.title.cmp(&b.title)));
    hits.truncate(limit);
    hits
}

/// About `len` characters around byte `at` (on character boundaries).
fn around(text: &str, at: usize, len: usize) -> String {
    let start = text.char_indices().map(|(i, _)| i).take_while(|i| *i <= at.saturating_sub(len / 3)).last().unwrap_or(0);
    let s: String = text[start..].chars().take(len).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A safe file name from a title.
pub fn stem(title: &str) -> String {
    let s: String = title.chars().map(|c| if "/\\:*?\"<>|#^[]".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.trim_start_matches('.').chars().take(80).collect();
    if s.trim().is_empty() { "Note from BYTE".into() } else { s.trim().to_string() }
}

/// Writes a new note in `<vault>/BYTE/` (a new name if one exists); returns its path.
pub fn write(vault: &Path, title: &str, body: &str) -> AppResult<PathBuf> {
    let dir = vault.join("BYTE");
    std::fs::create_dir_all(&dir)?;
    let base = stem(title);
    let mut path = dir.join(format!("{base}.md"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base} {n}.md"));
        n += 1;
    }
    std::fs::write(&path, format!("# {}\n\n{}\n", title.trim(), body.trim()))?;
    Ok(path)
}

/// A link that opens the note in Obsidian.
pub fn open_link(path: &Path) -> String {
    format!("obsidian://open?path={}", urlencoding(&path.to_string_lossy()))
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vault_is_searched_and_written_to_safely() {
        let dir = tempfile::tempdir().unwrap();
        let v = dir.path();
        std::fs::create_dir_all(v.join(".obsidian")).unwrap();
        std::fs::write(v.join(".obsidian/workspace.md"), "sourdough").unwrap();
        std::fs::create_dir_all(v.join("Cooking")).unwrap();
        std::fs::write(v.join("Cooking/Sourdough.md"), "Feed the starter at 9pm. Bake at 250°C for 20 minutes, then lower to 230°C.").unwrap();
        std::fs::write(v.join("Journal.md"), "Made sourdough today, it was great.").unwrap();
        std::fs::write(v.join("Taxes.md"), "Nothing about bread.").unwrap();
        let hits = search(v, "what do my notes say about sourdough", 5);
        assert_eq!(hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["Sourdough", "Journal"], "the title counts more; hidden folders are skipped");
        assert!(hits[0].snippet.contains("Feed the starter"));
        assert!(search(v, "the notes", 5).is_empty(), "no real words");

        let p = write(v, "Grocery / list: today?", "- milk").unwrap();
        assert_eq!(p, v.join("BYTE/Grocery list today.md"));
        let p2 = write(v, "Grocery / list: today?", "- eggs").unwrap();
        assert_eq!(p2, v.join("BYTE/Grocery list today 2.md"), "never overwrites");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# Grocery / list: today?\n\n- milk\n");
        assert_eq!(stem("../../.ssh/id"), ".. .. .ssh id".trim_start_matches('.').trim());
        assert!(open_link(&p).starts_with("obsidian://open?path=/"));
        assert!(check("/definitely/not/here").is_err());
    }
}
