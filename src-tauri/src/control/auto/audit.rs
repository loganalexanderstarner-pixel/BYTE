//! The audit trail of autonomous runs: every step with its time, the program and the result. Typed text is never written, only how
//! much was typed (the executor already words it that way; `clean` makes sure of it again). Kept on this PC, shown and exported
//! from Settings.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const KEEP_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub at: String,
    pub job: String,
    pub stage: String,
    pub app: String,
    pub what: String,
    pub ok: bool,
    pub result: String,
}

fn one_line(s: &str, n: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect()
}

/// What may be written for a step: typed text is cut down to its length.
pub fn clean(what: &str) -> String {
    let w = one_line(what, 200);
    if let Some(rest) = w.strip_prefix("Type ") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && rest[digits.len()..].trim_start().starts_with("character") {
            return format!("Type {digits} characters");
        }
        return "Type (text not kept)".into();
    }
    w
}

pub struct Audit {
    path: PathBuf,
}

impl Audit {
    pub fn new(path: PathBuf) -> Self {
        Audit { path }
    }

    pub fn record(&self, job: &str, stage: &str, app: &str, what: &str, ok: bool, result: &str) {
        if std::fs::metadata(&self.path).map(|m| m.len() > KEEP_BYTES).unwrap_or(false) {
            let _ = std::fs::rename(&self.path, self.path.with_extension("jsonl.1"));
        }
        let e = AuditEntry { at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(), job: one_line(job, 80), stage: one_line(stage, 120), app: one_line(app, 60), what: clean(what), ok, result: one_line(result, 300) };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let (Ok(line), Ok(mut f)) = (serde_json::to_string(&e), std::fs::OpenOptions::new().create(true).append(true).open(&self.path)) {
            let _ = writeln!(f, "{line}");
        }
    }

    /// The newest `n` entries, oldest first.
    pub fn recent(&self, n: usize) -> Vec<AuditEntry> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let all: Vec<AuditEntry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let skip = all.len().saturating_sub(n);
        all.into_iter().skip(skip).collect()
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("jsonl.1"));
    }
}

fn csv_cell(s: &str) -> String {
    // A cell that starts like a formula is made harmless for spreadsheets.
    let safe = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

pub fn to_csv(entries: &[AuditEntry]) -> String {
    let mut out = String::from("time,job,stage,program,step,ok,result\r\n");
    for e in entries {
        out.push_str(&[csv_cell(&e.at), csv_cell(&e.job), csv_cell(&e.stage), csv_cell(&e.app), csv_cell(&e.what), csv_cell(if e.ok { "yes" } else { "no" }), csv_cell(&e.result)].join(","));
        out.push_str("\r\n");
    }
    out
}

pub fn to_markdown(entries: &[AuditEntry]) -> String {
    let mut out = String::from("# What BYTE did on this PC\n\nTyped text is never kept, only how much was typed.\n\n| Time | Job | Stage | Program | Step | Result |\n|---|---|---|---|---|---|\n");
    let cell = |s: &str| s.replace('|', "/");
    for e in entries {
        out.push_str(&format!("| {} | {} | {} | {} | {} | {}{} |\n", e.at, cell(&e.job), cell(&e.stage), cell(&e.app), cell(&e.what), if e.ok { "done" } else { "not done" }, if e.result.is_empty() { String::new() } else { format!(": {}", cell(&e.result)) }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!("byte-audit-{}", uuid::Uuid::new_v4().simple())).join("audit.jsonl")
    }

    #[test]
    fn typed_text_is_never_kept_even_if_a_caller_passes_it() {
        assert_eq!(clean("Type 12 characters"), "Type 12 characters");
        assert_eq!(clean("Type my password is hunter2"), "Type (text not kept)");
        assert_eq!(clean("Click item 3"), "Click item 3");
    }

    #[test]
    fn entries_are_kept_in_order_and_the_newest_come_back() {
        let a = Audit::new(tmp());
        for i in 0..5 {
            a.record("job", "stage 1", "notepad", &format!("Click item {i}"), i % 2 == 0, "");
        }
        let r = a.recent(3);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].what, "Click item 2");
        assert_eq!(r[2].what, "Click item 4");
        a.clear();
        assert!(a.recent(10).is_empty());
    }

    #[test]
    fn exports_carry_every_step_and_make_spreadsheet_formulas_harmless() {
        let e = vec![AuditEntry { at: "t".into(), job: "j".into(), stage: "s".into(), app: "a".into(), what: "=HYPERLINK(\"x\")".into(), ok: false, result: "no \"way\"".into() }];
        let csv = to_csv(&e);
        assert!(csv.starts_with("time,job,stage,program,step,ok,result\r\n"));
        assert!(csv.contains("\"'=HYPERLINK(\"\"x\"\")\""));
        assert!(csv.contains("\"no \"\"way\"\"\""));
        let md = to_markdown(&e);
        assert!(md.contains("| t | j | s | a |") && md.contains("not done"));
    }
}
