//! Exports every chat to a folder: one Markdown file per chat (easy to read
//! anywhere) plus `byte-chats.json` with everything (for backups or moving).

use std::path::Path;

use serde_json::Value;

use crate::error::AppResult;

/// Writes the export into `<dir>/BYTE export <date>/` and returns that folder.
pub fn export_chats(dir: &Path, chats: &[Value]) -> AppResult<std::path::PathBuf> {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H.%M");
    let out = dir.join(format!("BYTE export {stamp}"));
    std::fs::create_dir_all(&out)?;
    std::fs::write(out.join("byte-chats.json"), serde_json::to_string_pretty(&serde_json::json!({ "version": 1, "chats": chats }))?)?;
    let mut used = std::collections::HashSet::new();
    for c in chats {
        let base = file_name(c.get("title").and_then(Value::as_str).unwrap_or("Chat"));
        let mut name = format!("{base}.md");
        let mut n = 2;
        while !used.insert(name.clone()) {
            name = format!("{base} ({n}).md");
            n += 1;
        }
        std::fs::write(out.join(name), markdown(c))?;
    }
    Ok(out)
}

/// A chat as readable Markdown, with each answer's sources listed under it.
pub fn markdown(c: &Value) -> String {
    let title = c.get("title").and_then(Value::as_str).unwrap_or("Chat");
    let mut md = format!("# {title}\n");
    if let Some(ts) = c.get("createdAt").and_then(Value::as_i64).and_then(chrono::DateTime::from_timestamp_millis) {
        md.push_str(&format!("\n_{}_\n", ts.with_timezone(&chrono::Local).format("%B %-d, %Y at %-I:%M %p")));
    }
    for m in c.get("messages").and_then(Value::as_array).into_iter().flatten() {
        let who = if m.get("role").and_then(Value::as_str) == Some("user") { "You" } else { "BYTE" };
        let content = m.get("content").and_then(Value::as_str).unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        md.push_str(&format!("\n## {who}\n\n{content}\n"));
        let sources: Vec<String> = m
            .get("sources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let n = s.get("n")?.as_i64()?;
                let t = s.get("title")?.as_str()?;
                let u = s.get("url")?.as_str()?;
                Some(format!("{n}. [{t}]({u})"))
            })
            .collect();
        if !sources.is_empty() {
            md.push_str(&format!("\n**Sources**\n\n{}\n", sources.join("\n")));
        }
    }
    md
}

/// A title made safe for a file name.
fn file_name(title: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_.,'()&".contains(c) { c } else { ' ' })
        .collect();
    // Drop "." and ".." path parts.
    let clean = clean.split_whitespace().filter(|w| !w.chars().all(|c| c == '.')).collect::<Vec<_>>().join(" ");
    let clean = clean.trim_matches('.').chars().take(80).collect::<String>();
    if clean.is_empty() {
        "Chat".into()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exports_markdown_and_json() {
        let dir = tempfile::tempdir().unwrap();
        let chats = vec![
            json!({ "id": "a", "title": "Trip: Lisbon/Porto?", "createdAt": 1_700_000_000_000i64, "messages": [
                { "role": "user", "content": "Plan 3 days" },
                { "role": "assistant", "content": "Day 1…", "sources": [{ "n": 1, "title": "Visit Lisbon", "url": "https://visitlisboa.com" }] },
            ]}),
            json!({ "id": "b", "title": "Trip: Lisbon/Porto?", "messages": [] }),
        ];
        let out = export_chats(dir.path(), &chats).unwrap();
        let md = std::fs::read_to_string(out.join("Trip Lisbon Porto.md")).unwrap();
        assert!(md.starts_with("# Trip: Lisbon/Porto?"));
        assert!(md.contains("## You\n\nPlan 3 days"));
        assert!(md.contains("1. [Visit Lisbon](https://visitlisboa.com)"));
        assert!(out.join("Trip Lisbon Porto (2).md").exists());
        let json: Value = serde_json::from_str(&std::fs::read_to_string(out.join("byte-chats.json")).unwrap()).unwrap();
        assert_eq!(json["chats"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("../../etc/passwd"), "etc passwd");
        assert_eq!(file_name("   "), "Chat");
    }
}
