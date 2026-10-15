//! The connector log: when something was read or posted, where, and how many. Never the text of a message.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "connect-log.jsonl";
const KEEP_BYTES: u64 = 400_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Milliseconds since 1970.
    pub at: i64,
    pub connector: String,
    /// "read", "post", "draft", "auto-reply", "refused", "error".
    pub action: String,
    pub channel: String,
    pub count: u32,
    pub ok: bool,
    /// A short fixed reason ("not on the allow-list", "rate limit"), or an error from the service. No message text.
    pub note: String,
}

pub struct Log {
    path: PathBuf,
}

impl Log {
    pub fn new(dir: &std::path::Path) -> Log {
        Log { path: dir.join(FILE) }
    }

    pub fn add(&self, e: Entry) {
        if std::fs::metadata(&self.path).map(|m| m.len() > KEEP_BYTES).unwrap_or(false) {
            let _ = std::fs::rename(&self.path, self.path.with_extension("jsonl.1"));
        }
        let mut e = e;
        e.note = e.note.chars().take(160).collect();
        if let (Ok(line), Ok(mut f)) = (serde_json::to_string(&e), std::fs::OpenOptions::new().create(true).append(true).open(&self.path)) {
            let _ = writeln!(f, "{line}");
        }
    }

    pub fn note(&self, at: i64, connector: &str, action: &str, channel: &str, count: u32, ok: bool, note: &str) {
        self.add(Entry { at, connector: connector.into(), action: action.into(), channel: channel.into(), count, ok, note: note.into() });
    }

    /// The newest entries, newest first.
    pub fn recent(&self, n: usize) -> Vec<Entry> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        text.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(n).collect()
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("jsonl.1"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_and_no_bodies() {
        let d = tempfile::tempdir().unwrap();
        let l = Log::new(d.path());
        l.note(1, "discord", "read", "c1", 3, true, "");
        l.note(2, "discord", "post", "c1", 1, true, "");
        let r = l.recent(10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].action, "post");
        let raw = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(!raw.contains("text") && !raw.contains("body"));
        l.clear();
        assert!(l.recent(5).is_empty());
    }
}
