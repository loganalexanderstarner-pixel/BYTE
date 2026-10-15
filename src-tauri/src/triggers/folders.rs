//! Finding new files: the Downloads folder (a download finished) and any folder the person picked. A file counts once it has
//! stopped growing and is not a browser's partial file, so a download is announced when it is really done. The first look at
//! a folder only learns what is already there.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const PARTIAL: &[&str] = &["crdownload", "part", "partial", "tmp", "download", "opdownload", "temp", "!ut", "aria2"];

#[derive(Default)]
pub struct Watch {
    /// Sizes at the last look, for files not yet announced.
    sizes: HashMap<PathBuf, u64>,
    /// Files already handled (or there from the start).
    done: HashSet<PathBuf>,
    started: bool,
}

pub fn is_partial(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| PARTIAL.contains(&e.to_lowercase().as_str())).unwrap_or(false) || p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.') || n.starts_with("~$")).unwrap_or(false)
}

impl Watch {
    /// One look at `dir` (not its subfolders): the files that are new and finished since the last look.
    pub fn scan(&mut self, dir: &Path) -> Vec<PathBuf> {
        let files: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_file().then(|| (e.path(), m.len()))
            })
            .collect();
        if !self.started {
            self.started = true;
            self.done.extend(files.into_iter().map(|(p, _)| p));
            return vec![];
        }
        let mut found = vec![];
        let present: HashSet<&PathBuf> = files.iter().map(|(p, _)| p).collect();
        self.sizes.retain(|p, _| present.contains(p));
        for (p, size) in &files {
            if self.done.contains(p) || is_partial(p) {
                continue;
            }
            if self.sizes.get(p) == Some(size) && *size > 0 {
                self.done.insert(p.clone());
                self.sizes.remove(p);
                found.push(p.clone());
            } else {
                self.sizes.insert(p.clone(), *size);
            }
        }
        found.sort();
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_files_are_ignored_and_a_new_one_is_announced_once_it_stops_growing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("old.txt"), "old").unwrap();
        let mut w = Watch::default();
        assert!(w.scan(d.path()).is_empty(), "the first look only learns");
        std::fs::write(d.path().join("new.zip"), "12345").unwrap();
        assert!(w.scan(d.path()).is_empty(), "seen once: it might still be growing");
        std::fs::write(d.path().join("new.zip"), "1234567").unwrap();
        assert!(w.scan(d.path()).is_empty(), "it grew");
        assert_eq!(w.scan(d.path()), vec![d.path().join("new.zip")]);
        assert!(w.scan(d.path()).is_empty(), "announced once");
    }

    #[test]
    fn partial_downloads_are_never_announced() {
        let d = tempfile::tempdir().unwrap();
        let mut w = Watch::default();
        w.scan(d.path());
        std::fs::write(d.path().join("movie.mp4.crdownload"), "xx").unwrap();
        std::fs::write(d.path().join("~$doc.docx"), "x").unwrap();
        for _ in 0..3 {
            assert!(w.scan(d.path()).is_empty());
        }
        std::fs::rename(d.path().join("movie.mp4.crdownload"), d.path().join("movie.mp4")).unwrap();
        assert!(w.scan(d.path()).is_empty());
        assert_eq!(w.scan(d.path()).len(), 1);
    }

    #[test]
    fn a_missing_folder_is_quiet() {
        let mut w = Watch::default();
        assert!(w.scan(Path::new("/definitely/not/here")).is_empty());
    }
}
