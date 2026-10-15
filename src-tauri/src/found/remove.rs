//! "Remove BYTE's data" (Settings → Backup and help): deletes the chats and settings and/or the downloaded models, takes BYTE
//! out of the startup list, and closes BYTE so it can be uninstalled. Your notes (Documents/BYTE/Notes), saved backups and
//! anything you exported are your own files and are never touched.
//!
//! BYTE cannot delete files it still has open, so the deleting is done by a small helper that waits a few seconds after BYTE has
//! closed. What it deletes is worked out here ([`targets`]), checked to be inside BYTE's own data folder, and shown to the person
//! before they confirm.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// The name of BYTE's data folder (the app identifier): nothing is removed unless the data folder has exactly this name.
pub const FOLDER_NAME: &str = "com.loganstarner.byte";

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    /// Chats, memories, settings, logs and everything else but the models.
    pub chats: bool,
    /// The downloaded models.
    pub models: bool,
}

/// What would be deleted, for the confirmation: each path with its size in bytes.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub paths: Vec<(String, u64)>,
    pub total_bytes: u64,
}

/// The paths to delete for this choice, or why it is refused. `root` must be BYTE's data folder.
pub fn targets(root: &Path, expected_name: &str, choice: Choice) -> AppResult<Vec<PathBuf>> {
    if !choice.chats && !choice.models {
        return Err(AppError::msg("Choose what to remove first."));
    }
    let named = root.file_name().and_then(|n| n.to_str()) == Some(expected_name);
    if !named || root.components().count() < 3 || !root.is_absolute() {
        return Err(AppError::msg("BYTE's data folder is not where it should be, so nothing was removed."));
    }
    let mut out = vec![];
    for entry in std::fs::read_dir(root).map_err(|e| AppError::msg(format!("Couldn't read BYTE's data folder: {e}")))?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_models = name == "models";
        if (is_models && choice.models) || (!is_models && choice.chats) {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

fn size_of(p: &Path) -> u64 {
    walkdir::WalkDir::new(p).into_iter().flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

/// A path is safe to hand to the helper's command line.
fn quotable(p: &Path) -> bool {
    let s = p.to_string_lossy();
    !s.contains(['"', '%', '\n', '\r', '\0'])
}

/// The helper's command: wait, then delete each path. Returns the program and its arguments (`windows`: cmd, else sh).
pub fn helper_command(paths: &[PathBuf], wait_secs: u32, windows: bool) -> AppResult<(String, Vec<String>)> {
    if paths.iter().any(|p| !quotable(p)) {
        return Err(AppError::msg("A file name has a character BYTE cannot hand to the cleaner safely, so nothing was removed. Delete the folder by hand instead."));
    }
    if windows {
        let mut script = format!("ping -n {} 127.0.0.1 >nul", wait_secs + 1);
        for p in paths {
            let q = p.to_string_lossy();
            if p.is_dir() {
                script.push_str(&format!(" & rmdir /s /q \"{q}\""));
            } else {
                script.push_str(&format!(" & del /f /q \"{q}\""));
            }
        }
        Ok(("cmd".into(), vec!["/C".into(), format!("\"{script}\"")]))
    } else {
        let mut args = vec!["-c".to_string(), format!("sleep {wait_secs}; rm -rf -- \"$@\""), "byte-clean".into()];
        args.extend(paths.iter().map(|p| p.to_string_lossy().to_string()));
        Ok(("sh".into(), args))
    }
}

/// Starts the helper detached from BYTE.
pub fn spawn_helper(paths: &[PathBuf], wait_secs: u32) -> AppResult<std::process::Child> {
    let windows = cfg!(windows);
    let (prog, args) = helper_command(paths, wait_secs, windows)?;
    let mut cmd = std::process::Command::new(prog);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window; the script is passed exactly as built.
        cmd.creation_flags(0x0800_0000).raw_arg(args.join(" "));
    }
    #[cfg(not(windows))]
    cmd.args(args);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn().map_err(|e| AppError::msg(format!("Couldn't start the cleaner: {e}")))
}

fn data_root(state: &AppState) -> PathBuf {
    state.paths.root.clone()
}

/// What "Remove" would delete, listed for the person to read.
#[tauri::command]
pub fn data_remove_plan(state: State<'_, AppState>, chats: bool, models: bool) -> AppResult<Plan> {
    crate::kids::grownups_only()?;
    let list = targets(&data_root(&state), FOLDER_NAME, Choice { chats, models })?;
    let paths: Vec<(String, u64)> = list.iter().map(|p| (p.to_string_lossy().to_string(), size_of(p))).collect();
    let total_bytes = paths.iter().map(|(_, n)| n).sum();
    Ok(Plan { paths, total_bytes })
}

/// Deletes what was chosen after BYTE closes, takes BYTE out of the startup list, then closes BYTE.
#[tauri::command]
pub async fn data_remove(app: AppHandle, state: State<'_, AppState>, chats: bool, models: bool, confirm: String) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    crate::kids::grownups_only()?;
    if confirm.trim() != "REMOVE" {
        return Err(AppError::msg("Type REMOVE to confirm."));
    }
    let list = targets(&data_root(&state), FOLDER_NAME, Choice { chats, models })?;
    if chats {
        let _ = state.secrets.delete(&state.cloud_account());
    }
    // Always: BYTE no longer opens when the person signs in.
    let _ = crate::background::apply_login(&app, false);
    state.engine.stop().await;
    for e in state.extras.all().await {
        e.stop().await;
    }
    state.embedder.kill_now();
    state.looker.kill_now();
    spawn_helper(&list, 4)?;
    super::crash::clean_exit();
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (tempfile::TempDir, PathBuf) {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("Users").join("x").join(FOLDER_NAME);
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join("models").join("m.gguf"), vec![0u8; 100]).unwrap();
        std::fs::write(root.join("settings.json"), "{}").unwrap();
        std::fs::write(root.join("chats.db"), "x").unwrap();
        (base, root)
    }

    fn names(v: &[PathBuf]) -> Vec<String> {
        v.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect()
    }

    #[test]
    fn chats_means_everything_but_the_models_and_models_means_only_them() {
        let (_b, root) = tree();
        assert_eq!(names(&targets(&root, FOLDER_NAME, Choice { chats: true, models: false }).unwrap()), ["chats.db", "logs", "settings.json"]);
        assert_eq!(names(&targets(&root, FOLDER_NAME, Choice { chats: false, models: true }).unwrap()), ["models"]);
        assert_eq!(targets(&root, FOLDER_NAME, Choice { chats: true, models: true }).unwrap().len(), 4);
    }

    #[test]
    fn nothing_chosen_or_a_wrong_folder_is_refused() {
        let (_b, root) = tree();
        assert!(targets(&root, FOLDER_NAME, Choice { chats: false, models: false }).is_err());
        assert!(targets(&root, "other", Choice { chats: true, models: true }).is_err());
        assert!(targets(Path::new("/"), FOLDER_NAME, Choice { chats: true, models: true }).is_err());
        assert!(targets(Path::new("relative/com.loganstarner.byte"), FOLDER_NAME, Choice { chats: true, models: true }).is_err());
        // Even the right name directly under the top is too shallow to trust.
        assert!(targets(Path::new("/com.loganstarner.byte"), FOLDER_NAME, Choice { chats: true, models: true }).is_err());
    }

    #[test]
    fn paths_that_could_break_the_command_are_refused() {
        assert!(helper_command(&[PathBuf::from("/a/100%/b")], 1, true).is_err());
        assert!(helper_command(&[PathBuf::from("/a/\"b")], 1, false).is_err());
    }

    #[test]
    fn the_windows_helper_waits_then_deletes_each_path_quoted() {
        let (_b, root) = tree();
        let list = targets(&root, FOLDER_NAME, Choice { chats: true, models: false }).unwrap();
        let (prog, args) = helper_command(&list, 4, true).unwrap();
        assert_eq!(prog, "cmd");
        assert_eq!(args[0], "/C");
        assert!(args[1].starts_with("\"ping -n 5 127.0.0.1 >nul"));
        assert!(args[1].contains("rmdir /s /q") && args[1].contains("del /f /q") && args[1].ends_with('"'));
    }

    #[test]
    fn the_windows_uninstaller_removes_the_startup_entry_but_not_during_an_update() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.windows.conf.json")).unwrap();
        assert_eq!(conf["bundle"]["windows"]["nsis"]["installerHooks"], "./windows/hooks.nsh");
        let hooks = include_str!("../../windows/hooks.nsh");
        assert!(hooks.contains("NSIS_HOOK_POSTUNINSTALL") && hooks.contains("DeleteRegValue HKCU") && hooks.contains("\\Run\" \"BYTE\""));
        assert!(hooks.contains("$UpdateMode <> 1"), "an update must keep the startup entry");
    }

    #[cfg(unix)]
    #[test]
    fn the_helper_really_deletes_what_was_chosen_and_nothing_else() {
        let (_b, root) = tree();
        let list = targets(&root, FOLDER_NAME, Choice { chats: true, models: false }).unwrap();
        let mut child = spawn_helper(&list, 0).unwrap();
        child.wait().unwrap();
        assert!(!root.join("settings.json").exists() && !root.join("chats.db").exists() && !root.join("logs").exists());
        assert!(root.join("models").join("m.gguf").exists(), "the models were not chosen");
    }
}
