//! Making a Shortcut that starts a BYTE automation (Phase 10).
//!
//! The shortcut has two actions: a URL (`byte://run/<id>?key=<key>`) and Open
//! URLs. It's written as a .shortcut file (a binary property list), signed with
//! macOS's own `shortcuts sign` (macOS only imports signed shortcuts) and
//! opened, so the Shortcuts app shows its "Add Shortcut" button. From there it
//! can go in the menu bar, on the Dock, or be started by asking Siri.
//!
//! The key in the link is per automation and random, so a web page can't start
//! the user's automations by guessing `byte://run/1`.

use plist::{Dictionary, Value};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::automations::{self, file_stem};
use crate::error::{AppError, AppResult};
use crate::macctl::{Command, MacRunner, Runner};
use crate::state::AppState;

/// The link a Shortcut opens to run automation `id`.
pub fn run_link(id: i64, key: &str) -> String {
    format!("byte://run/{id}?key={key}")
}

fn action(identifier: &str, params: Dictionary) -> Value {
    let mut a = Dictionary::new();
    a.insert("WFWorkflowActionIdentifier".into(), Value::String(identifier.into()));
    a.insert("WFWorkflowActionParameters".into(), Value::Dictionary(params));
    Value::Dictionary(a)
}

/// The unsigned shortcut: open `url`.
pub fn shortcut_file(url: &str) -> AppResult<Vec<u8>> {
    let mut url_params = Dictionary::new();
    url_params.insert("WFURLActionURL".into(), Value::String(url.into()));
    let mut icon = Dictionary::new();
    // A teal tile with a lightning bolt, close to BYTE's own icon.
    icon.insert("WFWorkflowIconStartColor".into(), Value::Integer(431817727.into()));
    icon.insert("WFWorkflowIconGlyphNumber".into(), Value::Integer(59446.into()));
    let mut root = Dictionary::new();
    root.insert("WFWorkflowActions".into(), Value::Array(vec![action("is.workflow.actions.url", url_params), action("is.workflow.actions.openurl", Dictionary::new())]));
    root.insert("WFWorkflowClientVersion".into(), Value::String("2605.0.5".into()));
    root.insert("WFWorkflowMinimumClientVersion".into(), Value::Integer(900.into()));
    root.insert("WFWorkflowMinimumClientVersionString".into(), Value::String("900".into()));
    root.insert("WFWorkflowIcon".into(), Value::Dictionary(icon));
    root.insert("WFWorkflowImportQuestions".into(), Value::Array(vec![]));
    root.insert("WFWorkflowTypes".into(), Value::Array(vec![Value::String("MenuBar".into()), Value::String("QuickActions".into())]));
    root.insert("WFWorkflowInputContentItemClasses".into(), Value::Array(vec![]));
    root.insert("WFWorkflowHasShortcutInputVariables".into(), Value::Boolean(false));
    let mut out = Vec::new();
    Value::Dictionary(root).to_writer_binary(&mut out).map_err(|e| AppError::msg(format!("Couldn't write the shortcut: {e}")))?;
    Ok(out)
}

/// What happened when making the shortcut.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Made {
    /// The Shortcuts app is open with "Add Shortcut".
    pub opened: bool,
    /// The link, for making it by hand.
    pub link: String,
    /// Why it couldn't be opened (then the link and two steps are shown).
    pub message: String,
}

/// Makes and opens a Shortcut for automation `id`.
#[tauri::command]
pub async fn automation_shortcut(app: AppHandle, id: i64) -> AppResult<Made> {
    let (a, key) = {
        let state = app.state::<AppState>();
        let a = automations::get(&state.db, id)?.ok_or_else(|| AppError::msg("That automation is gone."))?;
        let key = automations::link_key(&state.db, id)?;
        (a, key)
    };
    let link = run_link(id, &key);
    if !cfg!(target_os = "macos") {
        return Ok(Made { opened: false, link, message: "Shortcuts are only on macOS.".into() });
    }
    let dir = std::env::temp_dir().join(format!("byte-shortcut-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let stem = file_stem(&a.name);
    let unsigned = dir.join(format!("{stem} unsigned.shortcut"));
    let signed = dir.join(format!("{stem}.shortcut"));
    std::fs::write(&unsigned, shortcut_file(&link)?)?;
    let sign = Command::Exec {
        program: "shortcuts",
        args: vec!["sign".into(), "--mode".into(), "anyone".into(), "--input".into(), unsigned.display().to_string(), "--output".into(), signed.display().to_string()],
    };
    if let Err(e) = MacRunner.run(&sign).await {
        log::warn!("shortcuts sign: {}", e.text("Shortcuts"));
        return Ok(Made {
            opened: false,
            link,
            message: "macOS couldn't sign the shortcut (signing needs you to be signed in to iCloud). You can make it in two steps instead.".into(),
        });
    }
    match MacRunner.run(&Command::Exec { program: "open", args: vec![signed.display().to_string()] }).await {
        Ok(_) => Ok(Made { opened: true, link, message: String::new() }),
        Err(e) => Ok(Made { opened: false, link, message: e.text("Shortcuts") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortcut_opens_the_automation_link() {
        let link = run_link(7, "abc123");
        assert_eq!(link, "byte://run/7?key=abc123");
        let bytes = shortcut_file(&link).unwrap();
        assert!(bytes.starts_with(b"bplist00"), "binary plist");
        let v = Value::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let root = v.as_dictionary().unwrap();
        let actions = root["WFWorkflowActions"].as_array().unwrap();
        assert_eq!(actions.len(), 2);
        let first = actions[0].as_dictionary().unwrap();
        assert_eq!(first["WFWorkflowActionIdentifier"].as_string(), Some("is.workflow.actions.url"));
        assert_eq!(first["WFWorkflowActionParameters"].as_dictionary().unwrap()["WFURLActionURL"].as_string(), Some(link.as_str()));
        assert_eq!(actions[1].as_dictionary().unwrap()["WFWorkflowActionIdentifier"].as_string(), Some("is.workflow.actions.openurl"));
        assert_eq!(root["WFWorkflowMinimumClientVersion"].as_signed_integer(), Some(900));
    }

    /// The Mac CI checks macOS reads the file (and tries signing, which needs iCloud).
    #[tokio::test]
    #[ignore]
    async fn e2e_shortcut_file_on_a_mac() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Test.shortcut");
        std::fs::write(&path, shortcut_file(&run_link(1, "k")).unwrap()).unwrap();
        let lint = std::process::Command::new("/usr/bin/plutil").arg("-lint").arg(&path).output().unwrap();
        assert!(lint.status.success(), "{}", String::from_utf8_lossy(&lint.stdout));
        let out = dir.path().join("Signed.shortcut");
        let sign = std::process::Command::new("/usr/bin/shortcuts").args(["sign", "--mode", "anyone", "--input"]).arg(&path).arg("--output").arg(&out).output();
        match sign {
            Ok(o) => eprintln!("shortcuts sign: status {:?}, stderr: {}", o.status.code(), String::from_utf8_lossy(&o.stderr)),
            Err(e) => eprintln!("shortcuts sign couldn't start: {e}"),
        }
    }
}
