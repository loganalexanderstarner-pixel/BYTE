//! "Copy diagnostics": one block of text the owner can paste into a chat
//! instead of describing what went wrong (docs/ANDROID.md).
//!
//! It holds device, memory, engine and settings facts and the engine's recent log.
//! It never holds chats, memories, file names or keys: `redact` strips anything
//! that looks like a BYTE key or a home folder, and engine log lines that could
//! carry message text are dropped.

use crate::engine::EngineStatus;
use crate::system::SystemInfo;

const MAX_LOG_LINES: usize = 60;
const MAX_LINE: usize = 300;
const MAX_REPORT: usize = 12_000;

/// Everything the report is built from, so the builder is a pure function.
pub struct Inputs {
    pub version: String,
    pub system: SystemInfo,
    pub available_ram: u64,
    pub engine: EngineStatus,
    pub active_model: Option<String>,
    pub context_size: Option<u32>,
    pub web_mode: String,
    pub workspace: String,
    pub offline: bool,
    pub kids_mode: bool,
    pub log: Vec<String>,
    pub i8mm: bool,
    /// Why the model is (not) loaded: see `startup_block`.
    pub startup: String,
}

/// What the launch decision saw: the chosen model, whether the catalog knows it, its files on disk against the
/// catalog's sizes, and the models folder. This is what a "No model" report needs.
pub fn startup_block(catalog: &crate::models::Catalog, models_dir: &std::path::Path, chosen: Option<&str>, note: &str) -> String {
    let mut out = vec!["Startup:".to_string()];
    if !note.is_empty() {
        out.push(format!("  last decision: {note}"));
    }
    match chosen {
        None => out.push("  chosen model: none".into()),
        Some(k) => match catalog.resolve(k) {
            Err(e) => out.push(format!("  chosen model: {k} (the catalog doesn't know it: {e})")),
            Ok((_, v)) => {
                out.push(format!("  chosen model: {k}, installed {}", crate::models::is_installed(models_dir, v)));
                for f in &v.files {
                    let have = std::fs::metadata(crate::models::file_path(models_dir, f)).map(|m| m.len().to_string()).unwrap_or_else(|_| "missing".into());
                    out.push(format!("    {}: on disk {have}, catalog says {}", f.name, f.size));
                }
            }
        },
    }
    let mut files: Vec<(String, u64)> = std::fs::read_dir(models_dir)
        .map(|d| d.flatten().filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.metadata().ok()?.len()))).collect())
        .unwrap_or_default();
    files.sort();
    out.push(format!("  models folder: {} files", files.len()));
    out.extend(files.iter().take(30).map(|(n, l)| format!("    {n} {l}")));
    out.join("\n")
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

pub fn build(i: &Inputs) -> String {
    let s = &i.system;
    let engine = match &i.engine {
        EngineStatus::NoModel => "no model downloaded".to_string(),
        EngineStatus::Stopped => "stopped".to_string(),
        EngineStatus::Starting { model } => format!("starting {model}"),
        EngineStatus::Ready { model, context, boosted, vision } => {
            format!("ready: {model}, {context}-token context, speed boost {boosted}, sees photos {vision}")
        }
        EngineStatus::Error { message } => format!("ERROR: {message}"),
    };
    let mut out = vec![
        format!("BYTE diagnostics ({})", i.version),
        format!("Device: {} ({}), {}", s.chip, if s.phone { "phone" } else { "computer" }, s.os_version),
        format!("CPU cores: {}{}", s.cpu_cores, if s.phone { format!(", i8mm {}", i.i8mm) } else { String::new() }),
        format!("Memory: {} total, {} free now, {} usable for models", gb(s.total_ram_bytes), gb(i.available_ram), gb(s.gpu_budget_bytes)),
        format!("Free storage: {}", gb(s.free_disk_bytes)),
        format!("Engine: {engine}"),
        format!(
            "Chosen model: {}, context {}",
            i.active_model.as_deref().unwrap_or("none"),
            i.context_size.map(|c| c.to_string()).unwrap_or_else(|| "auto".into())
        ),
        format!("Settings: web {}, workspace {}, offline {}, kids mode {}", i.web_mode, i.workspace, i.offline, i.kids_mode),
        i.startup.clone(),
        String::new(),
        "Engine log (latest):".to_string(),
    ];
    let lines: Vec<&String> = i.log.iter().filter(|l| log_line_is_safe(l)).collect();
    let skip = lines.len().saturating_sub(MAX_LOG_LINES);
    out.extend(lines.into_iter().skip(skip).map(|l| l.trim_end().to_string()));
    let mut text = redact(&out.join("\n"));
    if text.len() > MAX_REPORT {
        // Keep the end: the newest log lines matter most.
        let mut cut = text.len() - MAX_REPORT;
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        text = format!("(shortened)\n{}", &text[cut..]);
    }
    text
}

/// Engine log lines that might hold what someone typed.
fn log_line_is_safe(line: &str) -> bool {
    let l = line.to_lowercase();
    line.len() <= MAX_LINE && !l.contains("\"role\"") && !l.contains("\"content\"") && !l.contains("prompt:")
}

/// Removes BYTE keys, bearer tokens and home folders from text.
pub fn redact(text: &str) -> String {
    text.lines().map(redact_line).collect::<Vec<_>>().join("\n")
}

fn redact_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut after_bearer = false;
    let mut rest = line;
    loop {
        // Copy whitespace as it is; handle one word at a time.
        let ws = rest.len() - rest.trim_start().len();
        out.push_str(&rest[..ws]);
        rest = &rest[ws..];
        if rest.is_empty() {
            break;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        rest = &rest[end..];
        if after_bearer {
            out.push_str("[hidden]");
            after_bearer = false;
        } else {
            after_bearer = word.eq_ignore_ascii_case("bearer");
            out.push_str(&redact_word(word));
        }
    }
    out
}

fn redact_word(word: &str) -> String {
    let mut w = word.to_string();
    // byte_… keys, wherever they sit in the word.
    while let Some(at) = w.find("byte_") {
        let tail = &w[at + 5..];
        let n = tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').count();
        if n < 8 {
            // "byte_lib" and the like are not keys; skip past this one.
            let keep = at + 5;
            let (head, rest) = w.split_at(keep);
            return format!("{head}{}", redact_word(rest));
        }
        w.replace_range(at..at + 5 + n, "[hidden]");
    }
    for root in ["/Users/", "/home/"] {
        while let Some(at) = w.find(root) {
            let tail = &w[at + root.len()..];
            let n = tail.find('/').unwrap_or(tail.len());
            w.replace_range(at..at + root.len() + n, "~");
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            version: "0.12.6".into(),
            system: crate::system::system_info(std::path::Path::new(".")),
            available_ram: 5_000_000_000,
            engine: EngineStatus::Ready { model: "lfm2:q4".into(), context: 8192, boosted: false, vision: false },
            active_model: Some("lfm2:q4".into()),
            context_size: Some(8192),
            web_mode: "auto".into(),
            workspace: "local".into(),
            offline: false,
            kids_mode: false,
            log: vec![],
            i8mm: true,
            startup: String::new(),
        }
    }

    #[test]
    fn the_startup_block_says_why_a_model_is_missing() {
        let c = crate::models::Catalog::embedded();
        let dir = tempfile::tempdir().unwrap();
        // Chosen but missing, with an unrelated file in the folder.
        std::fs::write(dir.path().join("something.gguf"), b"abc").unwrap();
        let b = startup_block(&c, dir.path(), Some("qwen3.5-4b:Q6_K"), "launch: no chat model is downloaded");
        assert!(b.contains("last decision: launch: no chat model is downloaded"), "{b}");
        assert!(b.contains("installed false") && b.contains("on disk missing, catalog says"), "{b}");
        assert!(b.contains("something.gguf 3"), "{b}");
        // Unknown to the catalog, and nothing chosen.
        assert!(startup_block(&c, dir.path(), Some("nope:Q4"), "").contains("the catalog doesn't know it"));
        assert!(startup_block(&c, dir.path(), None, "").contains("chosen model: none"));
        // A matching file counts as installed.
        let (_, v) = c.resolve("qwen3.5-4b:Q6_K").unwrap();
        std::fs::File::create(crate::models::file_path(dir.path(), &v.files[0])).unwrap().set_len(v.files[0].size).unwrap();
        assert!(startup_block(&c, dir.path(), Some("qwen3.5-4b:Q6_K"), "").contains("installed true"));
    }

    #[test]
    fn reports_the_device_memory_and_engine() {
        let r = build(&inputs());
        assert!(r.starts_with("BYTE diagnostics (0.12.6)"));
        assert!(r.contains("Engine: ready: lfm2:q4, 8192-token context"));
        assert!(r.contains("Memory:") && r.contains("Free storage:"));
        assert!(r.contains("Settings: web auto, workspace local"));
    }

    #[test]
    fn never_holds_keys_or_home_folders() {
        let mut i = inputs();
        i.log = vec![
            "request with Authorization: Bearer byte_test_abcdef123456 sent".into(),
            "loading /Users/logan/Library/models/x.gguf".into(),
            "key=byte_test_ABCDEF-123456_xyz done".into(),
            "libbyte_lib.so loaded".into(),
        ];
        let r = build(&i);
        assert!(!r.contains("byte_test_abcdef123456"), "{r}");
        assert!(!r.contains("ABCDEF-123456"), "{r}");
        assert!(!r.contains("logan"), "{r}");
        assert!(r.contains("~/Library/models/x.gguf"));
        assert!(r.contains("libbyte_lib.so"), "short byte_ words are not keys");
    }

    #[test]
    fn never_holds_what_someone_typed() {
        let mut i = inputs();
        i.log = vec![
            r#"{"role":"user","content":"my secret plan"}"#.into(),
            "prompt: tell me about my medical results".into(),
            "x".repeat(400),
            "slot 0 | task 3 | processed 12 tokens".into(),
        ];
        let r = build(&i);
        assert!(!r.contains("secret plan") && !r.contains("medical"), "{r}");
        assert!(!r.contains(&"x".repeat(50)));
        assert!(r.contains("processed 12 tokens"));
    }

    #[test]
    fn is_short_and_keeps_the_newest_log_lines() {
        let mut i = inputs();
        i.log = (0..5000).map(|n| format!("line {n} {}", "y".repeat(100))).collect();
        let r = build(&i);
        assert!(r.len() <= MAX_REPORT + 20, "{}", r.len());
        assert!(r.contains("line 4999"));
        assert!(!r.contains("line 10 "));
    }

    #[test]
    fn an_engine_error_is_shown() {
        let mut i = inputs();
        i.engine = EngineStatus::Error { message: "out of memory".into() };
        assert!(build(&i).contains("Engine: ERROR: out of memory"));
    }
}
