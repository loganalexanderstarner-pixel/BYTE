//! Safe mode: a run (started with `--safe`, or from Settings → Backup and help → "Restart in safe mode") in which every optional
//! feature that listens, looks at the screen or controls things is off, without changing the saved settings.
//!
//! How: the settings are read as usual, then the keys in [`FORCED`] are switched off in memory and their saved values are kept
//! here. Whenever the settings are written to disk, [`Safe::for_disk`] puts those saved values back, so the file keeps what the
//! person chose. While safe mode is on, a change that would switch one of those features on is refused with a plain message.

use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};
use crate::settings::Settings;

/// The setting names (as saved) that safe mode holds off, and the value each is held at. Every one of them listens, looks at
/// the screen, controls the PC or the Mac, or acts without being asked.
pub const FORCED: &[(&str, Value)] = &[
    ("wakeWord", Value::Bool(false)),
    ("pcControl", Value::Bool(false)),
    ("controlNeverAsk", Value::Bool(false)),
    ("guidedHelp", Value::Bool(false)),
    ("routinesEnabled", Value::Bool(false)),
    ("findFiles", Value::Bool(false)),
    ("screenAsk", Value::Bool(false)),
    ("replayBuffer", Value::Bool(false)),
    ("gameAdvisor", Value::Bool(false)),
    ("readIt", Value::Bool(false)),
    ("browserDriveReal", Value::Bool(false)),
    ("browserLiveResearch", Value::Bool(false)),
    ("webAgentEnabled", Value::Bool(false)),
    ("clipboardHistory", Value::Bool(false)),
    ("selectionHotkey", Value::Bool(false)),
    ("messagesInbox", Value::Bool(false)),
    ("terminalEnabled", Value::Bool(false)),
    ("macControl", Value::Bool(false)),
    ("automationsEnabled", Value::Bool(false)),
    ("connectorsEnabled", Value::Bool(false)),
];

/// True when the command line or the environment asks for safe mode.
pub fn requested(args: &[String], env: Option<&str>) -> bool {
    args.iter().any(|a| a == "--safe") || matches!(env, Some("1" | "true"))
}

/// The state of one run.
#[derive(Debug, Default)]
pub struct Safe {
    /// What the person had saved for each forced key, when safe mode is on.
    held: Option<Map<String, Value>>,
}

impl Safe {
    pub fn active(&self) -> bool {
        self.held.is_some()
    }

    /// Switches the forced keys off and remembers what they were. Returns the names that were actually on.
    pub fn force_off(&mut self, s: &mut Settings) -> Vec<String> {
        let mut v = serde_json::to_value(&*s).unwrap_or(Value::Null);
        let Some(obj) = v.as_object_mut() else { return vec![] };
        let mut held = self.held.take().unwrap_or_default();
        let mut was_on = vec![];
        for (k, off) in FORCED {
            if let Some(cur) = obj.get(*k).cloned() {
                if cur != *off {
                    was_on.push((*k).to_string());
                }
                // The first value seen is the saved one; later calls keep it.
                held.entry((*k).to_string()).or_insert(cur);
                obj.insert((*k).to_string(), off.clone());
            }
        }
        if let Ok(next) = serde_json::from_value::<Settings>(v) {
            *s = next;
        }
        self.held = Some(held);
        was_on
    }

    /// What to write to the settings file: the settings with the forced keys back at what the person saved.
    pub fn for_disk(&self, s: &Settings) -> serde_json::Result<Value> {
        let mut v = serde_json::to_value(s)?;
        if let (Some(held), Some(obj)) = (&self.held, v.as_object_mut()) {
            for (k, saved) in held {
                obj.insert(k.clone(), saved.clone());
            }
        }
        Ok(v)
    }

    /// Refuses a change that would turn a held-off feature on during this run.
    pub fn check_patch(&self, patch: &Value) -> AppResult<()> {
        if !self.active() {
            return Ok(());
        }
        let Some(obj) = patch.as_object() else { return Ok(()) };
        for (k, off) in FORCED {
            if let Some(v) = obj.get(*k) {
                if v != off {
                    return Err(AppError::msg("Safe mode is on, so this feature stays off for this run. Restart BYTE normally to turn it on."));
                }
            }
        }
        Ok(())
    }
}

static SAFE: Mutex<Safe> = Mutex::new(Safe { held: None });
static FORCED_NOW: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn lock() -> std::sync::MutexGuard<'static, Safe> {
    SAFE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Called when the settings are first read: in safe mode they are held off, otherwise left alone.
pub fn apply(s: &mut Settings) {
    let args: Vec<String> = std::env::args().collect();
    if !requested(&args, std::env::var("BYTE_SAFE").ok().as_deref()) {
        return;
    }
    let was_on = lock().force_off(s);
    log::warn!("safe mode: {} optional feature(s) held off for this run", was_on.len());
    *FORCED_NOW.lock().unwrap_or_else(|e| e.into_inner()) = was_on;
}

pub fn active() -> bool {
    lock().active()
}

/// The settings file's content (see [`Safe::for_disk`]).
pub fn for_disk(s: &Settings) -> serde_json::Result<Value> {
    lock().for_disk(s)
}

pub fn check_patch(patch: &Value) -> AppResult<()> {
    lock().check_patch(patch)
}

/// For the banner.
pub fn status() -> Value {
    json!({ "active": active(), "heldOff": FORCED_NOW.lock().unwrap_or_else(|e| e.into_inner()).clone() })
}

/// Restarts BYTE in safe mode (or back to normal): the choice travels in the environment, which the restart keeps.
#[tauri::command]
pub fn safe_mode_status() -> Value {
    status()
}

#[tauri::command]
pub fn safe_mode_restart(app: tauri::AppHandle, safe: bool) {
    if safe {
        std::env::set_var("BYTE_SAFE", "1");
    } else {
        std::env::remove_var("BYTE_SAFE");
        // A `--safe` on the command line is dropped too: restart with a clean list.
        let exe = std::env::current_exe().ok();
        if let Some(exe) = exe {
            let rest: Vec<_> = std::env::args_os().skip(1).filter(|a| a != "--safe").collect();
            if std::process::Command::new(exe).args(rest).spawn().is_ok() {
                super::crash::clean_exit();
                app.exit(0);
                return;
            }
        }
    }
    super::crash::clean_exit();
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn busy() -> Settings {
        Settings { wake_word: true, pc_control: true, screen_ask: true, replay_buffer: true, control_never_ask: true, theme: "forest".into(), ..Settings::default() }
    }

    #[test]
    fn the_flag_or_the_variable_asks_for_safe_mode() {
        assert!(requested(&["byte".into(), "--safe".into()], None));
        assert!(requested(&["byte".into()], Some("1")));
        assert!(!requested(&["byte".into(), "--background".into()], None));
        assert!(!requested(&[], Some("0")));
    }

    #[test]
    fn every_forced_feature_is_off_in_memory_and_other_settings_stay() {
        let mut s = busy();
        let mut safe = Safe::default();
        let was_on = safe.force_off(&mut s);
        assert!(!s.wake_word && !s.pc_control && !s.screen_ask && !s.replay_buffer && !s.control_never_ask);
        assert_eq!(s.theme, "forest");
        for k in ["wakeWord", "pcControl", "screenAsk", "replayBuffer", "controlNeverAsk"] {
            assert!(was_on.contains(&k.to_string()), "{k}");
        }
        // Everything held off really is a setting name, so a typo in the table cannot hide a feature.
        let names: Vec<String> = serde_json::to_value(Settings::default()).unwrap().as_object().unwrap().keys().cloned().collect();
        for (k, _) in FORCED {
            assert!(names.contains(&k.to_string()), "unknown setting {k}");
        }
    }

    #[test]
    fn the_file_keeps_what_the_person_saved() {
        let mut s = busy();
        let mut safe = Safe::default();
        safe.force_off(&mut s);
        // Something else changes during the run and is saved.
        s.theme = "paper".into();
        let disk = safe.for_disk(&s).unwrap();
        assert_eq!(disk["wakeWord"], json!(true));
        assert_eq!(disk["pcControl"], json!(true));
        assert_eq!(disk["theme"], json!("paper"));
        let back: Settings = serde_json::from_value(disk).unwrap();
        assert!(back.wake_word && back.control_never_ask);
    }

    #[test]
    fn forcing_twice_does_not_lose_the_saved_value() {
        let mut s = busy();
        let mut safe = Safe::default();
        safe.force_off(&mut s);
        safe.force_off(&mut s);
        assert_eq!(safe.for_disk(&s).unwrap()["wakeWord"], json!(true));
    }

    #[test]
    fn outside_safe_mode_nothing_changes() {
        let safe = Safe::default();
        let s = busy();
        assert_eq!(safe.for_disk(&s).unwrap()["wakeWord"], json!(true));
        assert!(safe.check_patch(&json!({"wakeWord": true})).is_ok());
    }

    #[test]
    fn turning_a_held_feature_on_is_refused_but_turning_it_off_or_other_changes_pass() {
        let mut s = busy();
        let mut safe = Safe::default();
        safe.force_off(&mut s);
        assert!(safe.check_patch(&json!({"wakeWord": true})).is_err());
        assert!(safe.check_patch(&json!({"pcControl": true, "theme": "x"})).is_err());
        assert!(safe.check_patch(&json!({"wakeWord": false})).is_ok());
        assert!(safe.check_patch(&json!({"theme": "paper"})).is_ok());
    }
}
