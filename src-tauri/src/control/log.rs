//! The activity log: what BYTE did to this PC by itself, kept on this PC only (a small JSON-lines file), shown in
//! Settings. Typed text is never written down, only how much was typed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const KEEP_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Local time, "2026-10-14 09:30:05".
    pub at: String,
    /// What was done, in words ("Click item 3", "Open notepad").
    pub what: String,
    pub ok: bool,
    /// Why not, or what came of it.
    pub detail: String,
}

pub trait Log: Send + Sync {
    fn record(&self, what: &str, ok: bool, detail: &str);
}

pub struct FileLog {
    path: PathBuf,
}

impl FileLog {
    pub fn new(path: PathBuf) -> Self {
        FileLog { path }
    }

    pub fn recent(&self, n: usize) -> Vec<Entry> {
        read_recent(&self.path, n)
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("jsonl.1"));
    }
}

impl Log for FileLog {
    fn record(&self, what: &str, ok: bool, detail: &str) {
        use std::io::Write;
        if std::fs::metadata(&self.path).map(|m| m.len() > KEEP_BYTES).unwrap_or(false) {
            let _ = std::fs::rename(&self.path, self.path.with_extension("jsonl.1"));
        }
        let e = Entry { at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(), what: one_line(what, 200), ok, detail: one_line(detail, 300) };
        if let (Ok(line), Ok(mut f)) = (serde_json::to_string(&e), std::fs::OpenOptions::new().create(true).append(true).open(&self.path)) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn one_line(s: &str, n: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect()
}

/// The newest `n` entries, newest first.
pub fn read_recent(path: &Path, n: usize) -> Vec<Entry> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().rev().filter_map(|l| serde_json::from_str::<Entry>(l).ok()).take(n).collect()
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemLog(pub Mutex<Vec<(String, bool, String)>>);

    impl Log for MemLog {
        fn record(&self, what: &str, ok: bool, detail: &str) {
            self.0.lock().unwrap().push((what.into(), ok, detail.into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_kept_newest_first_and_can_be_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let log = FileLog::new(dir.path().join("control_log.jsonl"));
        assert!(log.recent(10).is_empty());
        log.record("Open notepad", true, "");
        log.record("Click item 3", false, "item 3\nis gone");
        let r = log.recent(10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].what, "Click item 3");
        assert_eq!(r[0].detail, "item 3 is gone");
        assert!(!r[0].ok);
        assert_eq!(log.recent(1).len(), 1);
        log.clear();
        assert!(log.recent(10).is_empty());
    }

    #[test]
    fn a_damaged_line_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("l.jsonl");
        std::fs::write(&p, "not json\n{\"at\":\"x\",\"what\":\"a\",\"ok\":true,\"detail\":\"\"}\n").unwrap();
        assert_eq!(read_recent(&p, 5).len(), 1);
    }
}
