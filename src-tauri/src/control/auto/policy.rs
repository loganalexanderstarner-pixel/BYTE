//! How the step executor is told about autonomy: the level, the run contract, the protected programs, and the hooks a running job
//! listens on (pause, budgets, the live view, the audit trail). The executor holds one `Policy`; plain chat runs hold none, or one
//! with a level only.

use std::sync::Arc;

use futures_util::future::BoxFuture;

use super::contract::{Contract, Fact, Verdict};
use super::level::Level;
use super::rules::{self, app_key};
use crate::control::actions::{self, Action, Target};
use crate::control::uia::Element;

/// Why the run may not take another step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookStop {
    /// The person pressed Stop.
    Stopped,
    /// A budget ran out (the words say which).
    Budget(String),
}

/// What a running job listens on. The runner's control object implements it; tests use a recorder.
pub trait Hooks: Send + Sync {
    /// Before every step: waits while the run is paused, checks the stop flag and the step and time budgets.
    fn before_step<'a>(&'a self) -> BoxFuture<'a, Result<(), HookStop>>;
    fn step_started(&self, app: &str, what: &str);
    fn step_finished(&self, app: &str, what: &str, ok: bool, detail: &str);
    /// A message is about to be sent: spends one from the message budget.
    fn take_message(&self) -> Result<(), String>;
}

#[derive(Clone, Default)]
pub struct Policy {
    pub level: Level,
    pub contract: Option<Arc<Contract>>,
    /// Programs a job never touches (Settings, never-touch list).
    pub never_touch: Vec<String>,
    pub hooks: Option<Arc<dyn Hooks>>,
}

impl std::fmt::Debug for Policy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Policy").field("level", &self.level).field("contract", &self.contract).field("never_touch", &self.never_touch).field("hooks", &self.hooks.is_some()).finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// No contract applies.
    Go,
    /// Inside the contract.
    Inside,
    /// Outside it: the run waits for the person.
    CheckIn(String),
    /// Refused.
    Refuse(String),
}

impl Policy {
    pub fn app_verdict(&self, app: &str) -> Verdict {
        match &self.contract {
            Some(c) => c.judge(&Fact::App(app.to_string()), &self.never_touch),
            None => Verdict::Inside,
        }
    }

    /// The facts a step is about to act on.
    pub fn facts(action: &Action, el: Option<&Element>, app: &str) -> Vec<Fact> {
        let mut v = Vec::new();
        let app = app_key(app);
        match action {
            Action::Click { .. } => {
                if !app.is_empty() {
                    v.push(Fact::App(app));
                }
                match el.and_then(|e| rules::click_class(&e.name)) {
                    Some("delete") => v.push(Fact::Delete),
                    Some("payment") => v.push(Fact::Spend),
                    Some("send") => v.push(Fact::Message(String::new())),
                    _ => {}
                }
            }
            Action::Type { .. } | Action::Key { .. } | Action::Scroll { .. } | Action::Close { .. } => {
                if !app.is_empty() {
                    v.push(Fact::App(app));
                }
                if let Action::Key { combo } = action {
                    if let Ok(c) = actions::parse_combo(combo) {
                        if c.key == actions::KeyCode::Named("delete") {
                            v.push(Fact::Delete);
                        }
                    }
                }
            }
            Action::Open { target } => match actions::classify_target(target) {
                Ok(Target::Path(p)) => v.push(Fact::Folder(p)),
                Ok(Target::Uri(u)) if u.to_lowercase().starts_with("http") => v.push(Fact::App("browser".into())),
                _ => {}
            },
            Action::Power { .. } => {}
            Action::Focus { .. } | Action::Wait { .. } => {}
        }
        v
    }

    /// Judges a step against the contract. Programs in `approved` were already asked about in this run.
    pub fn gate(&self, app: &str, action: &Action, el: Option<&Element>, approved: &[String]) -> Gate {
        let Some(c) = &self.contract else { return Gate::Go };
        if matches!(action, Action::Power { .. }) {
            return Gate::CheckIn("this would shut down or restart the PC".into());
        }
        let mut outside: Option<String> = None;
        for f in Policy::facts(action, el, app) {
            if let Fact::App(a) = &f {
                if approved.contains(&app_key(a)) {
                    continue;
                }
            }
            match c.judge(&f, &self.never_touch) {
                Verdict::Inside => {}
                Verdict::Outside(w) => outside = outside.or(Some(w)),
                Verdict::Refused(w) => return Gate::Refuse(w),
            }
        }
        match outside {
            Some(w) => Gate::CheckIn(w),
            None => Gate::Inside,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::uia::{Kind, Rect};

    fn el(name: &str) -> Element {
        Element { id: 1, name: name.into(), kind: Kind::Button, enabled: true, rect: Rect::default(), sensitive: false }
    }

    fn policy() -> Policy {
        let c = Contract { apps: vec!["notepad".into(), "mail".into()], folders: vec!["C:\\Work".into()], may_message: vec!["mom".into()], ..Default::default() }.normalized();
        Policy { level: Level::Contract, contract: Some(Arc::new(c)), never_touch: vec!["bank".into()], hooks: None }
    }

    #[test]
    fn without_a_contract_everything_goes_to_the_normal_rules() {
        let p = Policy::default();
        assert_eq!(p.gate("anything", &Action::Click { id: 1 }, Some(&el("Save")), &[]), Gate::Go);
    }

    #[test]
    fn a_click_in_an_allowed_program_is_inside() {
        assert_eq!(policy().gate("Notepad.exe", &Action::Click { id: 1 }, Some(&el("Save")), &[]), Gate::Inside);
    }

    #[test]
    fn another_program_waits_and_a_protected_one_is_refused() {
        assert!(matches!(policy().gate("excel", &Action::Click { id: 1 }, Some(&el("Save")), &[]), Gate::CheckIn(_)));
        assert!(matches!(policy().gate("bank", &Action::Click { id: 1 }, Some(&el("Save")), &[]), Gate::Refuse(_)));
    }

    #[test]
    fn delete_and_payment_buttons_are_refused_and_send_buttons_wait() {
        let p = policy();
        assert!(matches!(p.gate("notepad", &Action::Click { id: 1 }, Some(&el("Delete")), &[]), Gate::Refuse(_)));
        assert!(matches!(p.gate("notepad", &Action::Click { id: 1 }, Some(&el("Buy now")), &[]), Gate::Refuse(_)));
        assert!(matches!(p.gate("mail", &Action::Click { id: 1 }, Some(&el("Send")), &[]), Gate::CheckIn(_)));
    }

    #[test]
    fn opening_a_file_outside_the_folders_waits_and_shutting_down_always_waits() {
        let p = policy();
        assert_eq!(p.gate("explorer", &Action::Open { target: "C:\\Work\\a.pdf".into() }, None, &[]), Gate::Inside);
        assert!(matches!(p.gate("explorer", &Action::Open { target: "C:\\Private\\a.pdf".into() }, None, &[]), Gate::CheckIn(_)));
        assert!(matches!(p.gate("notepad", &Action::Power { kind: actions::PowerKind::Shutdown }, None, &[]), Gate::CheckIn(_)));
    }

    #[test]
    fn the_delete_key_counts_as_deleting() {
        assert!(matches!(policy().gate("notepad", &Action::Key { combo: "delete".into() }, None, &[]), Gate::Refuse(_)));
    }
}
