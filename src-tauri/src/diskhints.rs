//! Plain-words disk hints (setting `diskHints`): before a model download, and when the disk is low, say how much is needed,
//! how much is free and what could be cleared. Only BYTE's own folders are listed, and nothing is ever deleted unless the
//! person presses the delete button of one item (the Settings card asks first).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// The disk counts as low under this much free space.
pub const LOW_BYTES: u64 = 5_000_000_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// What the delete button sends back: the path relative to BYTE's data folder, with `/`.
    pub id: String,
    pub label: String,
    pub bytes: u64,
    /// "model" | "partial" | "log" | "tool" | "old-restore"
    pub kind: String,
}

/// "3.2 GB", "450 MB", "12 KB".
pub fn size_words(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", b / 1e9)
    } else if bytes >= 1_000_000 {
        format!("{:.0} MB", b / 1e6)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} bytes")
    }
}

/// What could be cleared, largest first, at most `max` entries, in words ("old-model.gguf (4.1 GB)").
fn clearable(items: &[Item], max: usize) -> String {
    let mut v: Vec<&Item> = items.iter().filter(|i| i.bytes > 0).collect();
    v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    v.iter().take(max).map(|i| format!("{} ({})", i.label, size_words(i.bytes))).collect::<Vec<_>>().join(", ")
}

/// The message when a download does not fit. `need` is what the download still needs (with the margin), `free` what is free.
pub fn not_enough(need: u64, free: u64, items: &[Item]) -> String {
    let short = need.saturating_sub(free);
    let mut m = format!(
        "Not enough disk space. This download needs {} free and the disk has {} (it is {} short).",
        size_words(need),
        size_words(free),
        size_words(short)
    );
    let list = clearable(items, 4);
    if list.is_empty() {
        m.push_str(" Free up space by deleting files you no longer need, then try again.");
    } else {
        m.push_str(&format!(" BYTE's own files that could be cleared: {list}. Settings → Backup and help → Free up space lists them. Nothing is deleted unless you choose it."));
    }
    m
}

/// A short heads-up when the disk is getting low, even without a download. None when there is enough space.
pub fn low_disk(free: u64, items: &[Item]) -> Option<String> {
    if free == 0 || free >= LOW_BYTES {
        return None;
    }
    let mut m = format!("Your disk is getting full: {} free.", size_words(free));
    let list = clearable(items, 3);
    if !list.is_empty() {
        m.push_str(&format!(" BYTE's own files that could be cleared: {list}."));
    }
    Some(m)
}

fn dir_bytes(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/")
}

/// BYTE's own big items. `root` is the shared folder (models, logs); `data` the profile folder (tools, old restore copies).
pub fn own_items(root: &Path, models: &Path, logs: &Path, data: &Path) -> Vec<Item> {
    let mut out = Vec::new();
    let mut add = |path: PathBuf, label: String, kind: &str| {
        let bytes = if path.is_dir() { dir_bytes(&path) } else { std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) };
        out.push(Item { id: rel(root, &path), label, bytes, kind: kind.into() });
    };
    if let Ok(rd) = std::fs::read_dir(models) {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if name == "vision" {
                    if let Ok(sub) = std::fs::read_dir(&p) {
                        for s in sub.flatten() {
                            add(s.path(), format!("Image adapter {}", s.file_name().to_string_lossy()), "model");
                        }
                    }
                }
            } else if name.ends_with(".part") {
                add(p, format!("Unfinished download {}", name.trim_end_matches(".part")), "partial");
            } else if name.ends_with(".gguf") {
                add(p, format!("Model file {name}"), "model");
            }
        }
    }
    if logs.is_dir() {
        add(logs.to_path_buf(), "BYTE's log files".into(), "log");
    }
    let tools = data.join("tools");
    if tools.is_dir() {
        add(tools, "Downloaded helper tools (they download again when needed)".into(), "tool");
    }
    if let Ok(rd) = std::fs::read_dir(data) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("before-restore-") && e.path().is_dir() {
                add(e.path(), format!("Copy kept before a restore ({name})"), "old-restore");
            }
        }
    }
    out.retain(|i| i.bytes > 0);
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    out
}

/// Only the model folder's items, for the wording of a failed download.
pub fn model_items(models: &Path) -> Vec<Item> {
    let none = Path::new("");
    own_items(models, models, none, none).into_iter().filter(|i| i.kind != "log" && i.kind != "tool" && i.kind != "old-restore").collect()
}

fn items_of(state: &AppState) -> Vec<Item> {
    own_items(&state.paths.root, &state.paths.models, &state.paths.logs, &state.paths.data)
}

/// A symlink, or on Windows a junction or other reparse point: never deleted through.
pub fn is_link(p: &Path) -> bool {
    let Ok(m) = std::fs::symlink_metadata(p) else { return false };
    if m.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

/// Does `path` go through a link anywhere between `root` and itself (itself included)?
fn through_link(root: &Path, path: &Path) -> bool {
    let mut cur = path.to_path_buf();
    while cur != root && cur.starts_with(root) {
        if is_link(&cur) {
            return true;
        }
        if !cur.pop() {
            break;
        }
    }
    false
}

/// Deletes one listed item. Only ids that `own_items` lists now are accepted, so nothing else can be reached.
/// `protected` are files in use (the active model, downloads in progress): they are never deleted. Links are never followed.
pub fn delete_item(root: &Path, items: &[Item], id: &str, protected: &[PathBuf]) -> AppResult<()> {
    let item = items.iter().find(|i| i.id == id).ok_or_else(|| AppError::msg("That item is not one of BYTE's own files."))?;
    let path = root.join(&item.id);
    if !path.starts_with(root) || item.id.contains("..") {
        return Err(AppError::msg("That item is not one of BYTE's own files."));
    }
    if through_link(root, &path) {
        return Err(AppError::msg("That item is a shortcut to somewhere else, so BYTE will not delete it."));
    }
    if protected.iter().any(|p| p.starts_with(&path) || path.starts_with(p)) {
        return Err(AppError::msg("That model is in use or still downloading. Switch to another model or cancel the download first."));
    }
    if path.is_dir() {
        if item.kind == "log" {
            // Keep the folder; clear what is in it (links inside are left alone).
            for e in std::fs::read_dir(&path)?.flatten() {
                if is_link(&e.path()) {
                    continue;
                }
                let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
            }
        } else {
            std::fs::remove_dir_all(&path)?;
        }
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn disk_items(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let s = state.inner();
    let (root, models, logs, data) = (s.paths.root.clone(), s.paths.models.clone(), s.paths.logs.clone(), s.paths.data.clone());
    let free = crate::system::free_disk_for(&models);
    let items = tokio::task::spawn_blocking(move || own_items(&root, &models, &logs, &data)).await.map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "free": free, "items": items, "freeWords": size_words(free), "low": low_disk(free, &items) }))
}

#[tauri::command]
pub async fn disk_delete_item(state: tauri::State<'_, AppState>, id: String) -> AppResult<()> {
    let items = items_of(&state);
    // Files in use: the active model and anything downloading (finished file and its .part).
    let catalog = state.catalog.get();
    let mut keys = state.downloads.active_ids().await;
    if let Some(a) = state.settings.lock().await.active_model.clone() {
        keys.push(a);
    }
    let mut protected: Vec<PathBuf> = Vec::new();
    for k in keys {
        let Ok((_, variant, key)) = catalog.download_target(&k).or_else(|_| catalog.resolve(&k).map(|(m, v)| (String::new(), v.clone(), crate::models::key(m, v)))) else { continue };
        let dir = crate::models::download_dir(&state.paths.models, &key);
        for f in &variant.files {
            let p = crate::models::file_path(&dir, f);
            let mut part = p.clone().into_os_string();
            part.push(".part");
            protected.extend([p, PathBuf::from(part)]);
        }
    }
    delete_item(&state.paths.root, &items, &id, &protected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, bytes: u64) -> Item {
        Item { id: label.into(), label: label.into(), bytes, kind: "model".into() }
    }

    #[test]
    fn sizes_are_in_plain_words() {
        assert_eq!(size_words(4_100_000_000), "4.1 GB");
        assert_eq!(size_words(450_000_000), "450 MB");
        assert_eq!(size_words(12_000), "12 KB");
        assert_eq!(size_words(7), "7 bytes");
    }

    #[test]
    fn the_message_says_need_free_short_and_what_to_clear() {
        let m = not_enough(10_000_000_000, 6_000_000_000, &[item("small", 1_000_000), item("Old model", 4_000_000_000), item("empty", 0)]);
        assert!(m.contains("needs 10.0 GB"), "{m}");
        assert!(m.contains("has 6.0 GB"), "{m}");
        assert!(m.contains("4.0 GB short"), "{m}");
        assert!(m.find("Old model").unwrap() < m.find("small").unwrap(), "largest first");
        assert!(!m.contains("empty"));
        assert!(m.contains("Nothing is deleted unless you choose it"));
    }

    #[test]
    fn with_nothing_of_ours_to_clear_it_says_what_to_do_instead() {
        assert!(not_enough(2_000_000_000, 1_000_000_000, &[]).contains("deleting files you no longer need"));
    }

    #[test]
    fn a_low_disk_gets_a_hint_and_a_roomy_one_does_not() {
        assert!(low_disk(2_000_000_000, &[item("Old", 3_000_000_000)]).unwrap().contains("2.0 GB free"));
        assert_eq!(low_disk(50_000_000_000, &[]), None);
        assert_eq!(low_disk(0, &[]), None, "an unreadable disk is not called low");
    }

    #[cfg(unix)]
    #[test]
    fn links_are_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        for d in [&root, &root.join("models"), &outside] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(outside.join("precious.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("logs")).unwrap();
        std::os::unix::fs::symlink(outside.join("precious.txt"), root.join("models").join("x.gguf")).unwrap();
        let items = vec![
            Item { id: "logs".into(), label: "logs".into(), bytes: 1, kind: "log".into() },
            Item { id: "models/x.gguf".into(), label: "x".into(), bytes: 1, kind: "model".into() },
        ];
        assert!(delete_item(&root, &items, "logs", &[]).unwrap_err().to_string().contains("shortcut"));
        assert!(delete_item(&root, &items, "models/x.gguf", &[]).is_err());
        assert!(outside.join("precious.txt").exists());
    }

    #[test]
    fn items_are_listed_from_our_folders_and_only_those_can_be_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let models = root.join("models");
        let logs = root.join("logs");
        let data = root.join("profiles").join("p");
        for d in [&models, &logs, &data] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(models.join("a.gguf"), vec![0u8; 3000]).unwrap();
        std::fs::write(models.join("b.gguf.part"), vec![0u8; 100]).unwrap();
        std::fs::write(models.join("notes.txt"), b"not ours to list").unwrap();
        std::fs::write(logs.join("byte.log"), vec![0u8; 50]).unwrap();
        std::fs::create_dir_all(data.join("before-restore-1")).unwrap();
        std::fs::write(data.join("before-restore-1").join("x"), vec![0u8; 10]).unwrap();
        std::fs::write(data.join("settings.json"), b"{}").unwrap();
        let items = own_items(&root, &models, &logs, &data);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(items[0].bytes, 3000, "largest first");
        assert!(labels.iter().any(|l| l.contains("Unfinished download b.gguf")));
        assert!(!labels.iter().any(|l| l.contains("notes.txt") || l.contains("settings")));
        assert!(delete_item(&root, &items, "models/a.gguf", &[]).is_ok());
        assert!(!models.join("a.gguf").exists());
        assert!(delete_item(&root, &items, "../../etc/passwd", &[]).is_err());
        assert!(delete_item(&root, &items, "models/notes.txt", &[]).is_err(), "not listed");
        assert!(data.join("settings.json").exists());
        let err = delete_item(&root, &items, "models/b.gguf.part", &[models.join("b.gguf.part")]).unwrap_err();
        assert!(err.to_string().contains("in use"), "a download in progress is protected");
        assert!(models.join("b.gguf.part").exists());
        delete_item(&root, &items, "logs", &[]).unwrap();
        assert!(logs.is_dir() && !logs.join("byte.log").exists(), "the log folder stays, its files go");
    }
}
