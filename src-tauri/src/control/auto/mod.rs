//! Autonomous mode: "approve and don't ask again" rules, the autonomy level, run contracts, jobs that run stage after stage,
//! the live view, the audit trail, the morning report and the macro recorder. Everything that touches Windows sits behind a trait
//! with a test double.

pub mod audit;
pub mod contract;
pub mod job;
pub mod level;
pub mod limit;
pub mod policy;
pub mod power;
pub mod rules;
pub mod tool;
pub mod watch;
pub mod cmd;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::settings::Settings;
use crate::state::AppState;

/// A field on an approval card that makes it show "Approve and don't ask again". The card hides it and shows its value as the rule.
pub const REMEMBER_FIELD: &str = "__remember";

pub fn rules_file(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.data.join("control_rules.json")
}

pub fn jobs_dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.data.join("jobs")
}

pub fn audit_file(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.data.join("auto_audit.jsonl")
}

/// The person the rules belong to: the active profile.
pub fn user_id(app: &AppHandle) -> String {
    crate::profiles::Profiles::load(&app.state::<AppState>().paths.root).active
}

/// The policy for a run started from chat: only the "ask only outside a run contract" level needs one. Its contract is the list of
/// programs allowed without asking, nothing deleted, nobody messaged.
pub fn chat_policy(s: &Settings, level: level::Level) -> Option<policy::Policy> {
    if level != level::Level::Contract {
        return None;
    }
    let c = contract::Contract { apps: s.control_allowed_apps.clone(), ..Default::default() }.normalized();
    Some(policy::Policy { level, contract: Some(Arc::new(c)), never_touch: Vec::new(), hooks: None })
}
