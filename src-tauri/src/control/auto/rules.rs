//! "Approve and don't ask again": a saved rule is a kind of step, the program it happens in, and the user. Rules live in a local
//! file, are listed in Settings with a remove button each, and can never unlock a hard refusal: program files and scripts,
//! dangerous key combinations, payments, shutting down. Those are checked again every time a rule is looked up, so even a rule file
//! somebody edited by hand cannot switch them on.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::control::actions::{self, Action, Modifier, Risk, Target};
use crate::control::uia::Element;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    /// "Let BYTE use this program?"
    UseApp,
    /// A button whose name is a decision: `detail` is its class (send, delete, account, confirm).
    ClickLabel,
    /// A key combination that asks; `detail` is the combination.
    Key,
    CloseWindow,
    /// A file opened by path; `detail` is its extension.
    OpenFile,
    /// A link that is not a web address; `detail` is its scheme.
    OpenLink,
}

/// What a step is, for matching: kind + detail + the program it happens in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleKey {
    pub kind: RuleKind,
    pub detail: String,
    pub app: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub kind: RuleKind,
    #[serde(default)]
    pub detail: String,
    pub app: String,
    pub user: String,
    #[serde(default)]
    pub created: String,
    /// The rule in words, for the list in Settings.
    #[serde(default)]
    pub text: String,
}

pub fn app_key(s: &str) -> String {
    let l = s.trim().to_lowercase();
    l.strip_suffix(".exe").unwrap_or(&l).to_string()
}

const CLASSES: &[(&str, &[&str])] = &[
    ("payment", &["buy", "purchase", "pay", "place order", "order now", "checkout", "check out", "transfer", "subscribe", "donate"]),
    ("delete", &["delete", "remove", "erase", "format", "uninstall", "discard", "empty recycle", "don't save", "do not save"]),
    ("send", &["send", "post", "publish", "submit"]),
    ("account", &["sign out", "log out", "shut down", "shutdown", "restart"]),
    ("confirm", &["confirm"]),
];

/// Which kind of decision a button's name is: "payment", "delete", "send", "account", "confirm", or None when it is not one.
pub fn click_class(name: &str) -> Option<&'static str> {
    let low = name.to_lowercase();
    let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).collect();
    for (class, labels) in CLASSES {
        let hit = labels.iter().any(|l| {
            let lw: Vec<&str> = l.split(' ').collect();
            words.windows(lw.len()).any(|w| w == lw.as_slice())
        });
        if hit {
            return Some(class);
        }
    }
    if actions::risky_label(name) {
        return Some("confirm");
    }
    None
}

pub fn is_payment_label(name: &str) -> bool {
    click_class(name) == Some("payment")
}

/// Why a step can never be remembered or switched off, or None when it can.
pub fn hard_refusal(action: &Action, el: Option<&Element>) -> Option<&'static str> {
    if actions::risk(action) == Risk::Never {
        return Some("BYTE refuses this whatever you choose");
    }
    match action {
        Action::Click { .. } if el.is_some_and(|e| is_payment_label(&e.name)) => Some("payments always ask"),
        Action::Power { .. } => Some("shutting down and restarting always ask"),
        Action::Key { combo } => match actions::parse_combo(combo) {
            Ok(c) if c.mods.contains(&Modifier::Win) && c.key == actions::KeyCode::Char('x') => Some("this key combination always asks"),
            Ok(c) if c.mods.contains(&Modifier::Ctrl) && c.mods.contains(&Modifier::Shift) && c.key == actions::KeyCode::Named("esc") => Some("this key combination always asks"),
            _ => None,
        },
        _ => None,
    }
}

/// The key of the step that asked, or why it can't be remembered.
pub fn key_for(action: &Action, el: Option<&Element>, app: &str) -> Result<RuleKey, &'static str> {
    if let Some(why) = hard_refusal(action, el) {
        return Err(why);
    }
    let app = app_key(app);
    let (kind, detail) = match action {
        Action::Click { .. } => match el.and_then(|e| click_class(&e.name)) {
            Some(class) => (RuleKind::ClickLabel, class.to_string()),
            None => return Err("this step does not need a rule"),
        },
        Action::Key { combo } => (RuleKind::Key, combo.to_lowercase().split_whitespace().collect::<String>()),
        Action::Close { .. } => (RuleKind::CloseWindow, String::new()),
        Action::Open { target } => match actions::classify_target(target) {
            Ok(Target::Path(p)) => (RuleKind::OpenFile, p.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()),
            Ok(Target::Uri(u)) => (RuleKind::OpenLink, u.split_once(':').map(|(s, _)| s.to_lowercase()).unwrap_or_default()),
            _ => return Err("this step does not need a rule"),
        },
        _ => return Err("this step does not need a rule"),
    };
    if app.is_empty() && kind != RuleKind::OpenLink {
        return Err("BYTE can't tell which program this is in");
    }
    Ok(RuleKey { kind, detail, app })
}

/// The key for "Let BYTE use this program?".
pub fn key_for_app(app: &str) -> Option<RuleKey> {
    let app = app_key(app);
    (!app.is_empty()).then_some(RuleKey { kind: RuleKind::UseApp, detail: String::new(), app })
}

/// A rule in words.
pub fn describe(k: &RuleKey) -> String {
    let in_app = if k.app.is_empty() { String::new() } else { format!(" in {}", k.app) };
    match k.kind {
        RuleKind::UseApp => format!("Use {} without asking", k.app),
        RuleKind::ClickLabel => {
            let what = match k.detail.as_str() {
                "delete" => "Delete, Remove or Discard buttons",
                "send" => "Send, Post or Submit buttons",
                "account" => "Sign out or Restart buttons",
                _ => "Confirm buttons",
            };
            format!("Press {what}{in_app}")
        }
        RuleKind::Key => format!("Press {}{in_app}", k.detail),
        RuleKind::CloseWindow => format!("Close windows{in_app}"),
        RuleKind::OpenFile => format!("Open .{} files{in_app}", k.detail),
        RuleKind::OpenLink => format!("Open {}: links{in_app}", k.detail),
    }
}

/// Whether a stored rule is one that may exist at all (nothing hard-refused).
fn rule_is_allowed(r: &Rule) -> bool {
    match r.kind {
        RuleKind::ClickLabel => matches!(r.detail.as_str(), "delete" | "send" | "account" | "confirm"),
        RuleKind::Key => match actions::parse_combo(&r.detail) {
            Ok(c) => actions::risk(&Action::Key { combo: r.detail.clone() }) == Risk::Asks && !(c.mods.contains(&Modifier::Win) && c.key == actions::KeyCode::Char('x')) && !(c.mods.contains(&Modifier::Ctrl) && c.mods.contains(&Modifier::Shift) && c.key == actions::KeyCode::Named("esc")),
            Err(_) => false,
        },
        RuleKind::OpenFile => !r.detail.is_empty() && actions::classify_target(&format!("C:\\x.{}", r.detail)).is_ok(),
        RuleKind::UseApp | RuleKind::CloseWindow | RuleKind::OpenLink => true,
    }
}

/// The rule that covers `key` for `user`, if one does.
pub fn find<'a>(rules: &'a [Rule], key: &RuleKey, user: &str) -> Option<&'a Rule> {
    rules.iter().find(|r| r.user == user && r.kind == key.kind && r.detail == key.detail && r.app == key.app && rule_is_allowed(r))
}

/// The list with the rule added (the same rule is not added twice).
pub fn with_added(mut rules: Vec<Rule>, key: &RuleKey, user: &str, id: String, created: String) -> Vec<Rule> {
    if find(&rules, key, user).is_none() {
        rules.push(Rule { id, kind: key.kind, detail: key.detail.clone(), app: key.app.clone(), user: user.to_string(), created, text: describe(key) });
    }
    rules
}

pub fn without(mut rules: Vec<Rule>, id: &str) -> Vec<Rule> {
    rules.retain(|r| r.id != id);
    rules
}

pub fn without_user(mut rules: Vec<Rule>, user: &str) -> Vec<Rule> {
    rules.retain(|r| r.user != user);
    rules
}

/// The rules file.
#[derive(Debug, Clone)]
pub struct RuleStore {
    path: PathBuf,
}

impl RuleStore {
    pub fn new(path: PathBuf) -> Self {
        RuleStore { path }
    }

    pub fn load(&self) -> Vec<Rule> {
        std::fs::read_to_string(&self.path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn save(&self, list: &[Rule]) -> std::io::Result<()> {
        write_atomic(&self.path, &serde_json::to_vec_pretty(list)?)
    }

    pub fn for_user(&self, user: &str) -> Vec<Rule> {
        self.load().into_iter().filter(|r| r.user == user).collect()
    }

    pub fn matches(&self, key: &RuleKey, user: &str) -> bool {
        find(&self.load(), key, user).is_some()
    }

    pub fn add(&self, key: &RuleKey, user: &str) -> std::io::Result<()> {
        let list = with_added(self.load(), key, user, uuid::Uuid::new_v4().simple().to_string(), chrono::Local::now().format("%Y-%m-%d %H:%M").to_string());
        self.save(&list)
    }

    pub fn remove(&self, id: &str) -> std::io::Result<()> {
        self.save(&without(self.load(), id))
    }

    /// Removes this user's rules (the "remove all" button).
    pub fn clear(&self, user: &str) -> std::io::Result<()> {
        self.save(&without_user(self.load(), user))
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::uia::{Kind, Rect};

    fn el(name: &str) -> Element {
        Element { id: 1, name: name.into(), kind: Kind::Button, enabled: true, rect: Rect::default(), sensitive: false }
    }

    fn act(json: &str) -> Action {
        actions::parse_plan(json).unwrap().remove(0).action
    }

    #[test]
    fn buttons_fall_into_classes_and_payments_are_hard() {
        assert_eq!(click_class("Send"), Some("send"));
        assert_eq!(click_class("Delete all"), Some("delete"));
        assert_eq!(click_class("Buy now"), Some("payment"));
        assert_eq!(click_class("Place order"), Some("payment"));
        assert_eq!(click_class("Sign out"), Some("account"));
        assert_eq!(click_class("Save"), None);
        assert_eq!(click_class("Payroll"), None);
    }

    #[test]
    fn a_payment_can_never_be_remembered() {
        let a = Action::Click { id: 1 };
        assert!(key_for(&a, Some(&el("Buy now")), "shop").is_err());
        assert!(key_for(&a, Some(&el("Checkout")), "shop").is_err());
        assert!(key_for(&a, Some(&el("Send")), "mail").is_ok());
    }

    #[test]
    fn program_files_scripts_and_dangerous_keys_never_get_a_key() {
        for t in ["C:\\tools\\setup.exe", "C:\\x\\run.bat", "C:\\x\\go.ps1", "cmd", "powershell"] {
            let a = Action::Open { target: t.into() };
            assert!(key_for(&a, None, "explorer").is_err(), "{t}");
        }
        for c in ["win+r", "ctrl+alt+delete", "win+x", "ctrl+shift+esc"] {
            assert!(key_for(&Action::Key { combo: c.into() }, None, "x").is_err(), "{c}");
        }
        assert!(key_for(&Action::Power { kind: actions::PowerKind::Shutdown }, None, "x").is_err());
    }

    #[test]
    fn a_file_by_path_is_remembered_by_extension_and_program() {
        let k = key_for(&Action::Open { target: "C:\\Users\\me\\a.PDF".into() }, None, "Explorer.exe").unwrap();
        assert_eq!(k, RuleKey { kind: RuleKind::OpenFile, detail: "pdf".into(), app: "explorer".into() });
        assert_eq!(describe(&k), "Open .pdf files in explorer");
    }

    #[test]
    fn a_rule_covers_the_same_kind_program_and_user_only() {
        let k = key_for(&Action::Click { id: 1 }, Some(&el("Send")), "outlook").unwrap();
        let rules = with_added(vec![], &k, "me", "r1".into(), "now".into());
        assert!(find(&rules, &k, "me").is_some());
        assert!(find(&rules, &k, "kid").is_none(), "another user");
        let other_app = key_for(&Action::Click { id: 1 }, Some(&el("Send")), "discord").unwrap();
        assert!(find(&rules, &other_app, "me").is_none(), "another program");
        let del = key_for(&Action::Click { id: 1 }, Some(&el("Delete")), "outlook").unwrap();
        assert!(find(&rules, &del, "me").is_none(), "another kind of step");
        assert_eq!(with_added(rules.clone(), &k, "me", "r2".into(), "now".into()).len(), 1, "no duplicates");
    }

    #[test]
    fn a_hand_edited_rule_cannot_unlock_a_hard_refusal() {
        let bad = [
            Rule { id: "1".into(), kind: RuleKind::ClickLabel, detail: "payment".into(), app: "shop".into(), user: "me".into(), created: String::new(), text: String::new() },
            Rule { id: "2".into(), kind: RuleKind::Key, detail: "win+r".into(), app: "x".into(), user: "me".into(), created: String::new(), text: String::new() },
            Rule { id: "3".into(), kind: RuleKind::OpenFile, detail: "exe".into(), app: "explorer".into(), user: "me".into(), created: String::new(), text: String::new() },
            Rule { id: "4".into(), kind: RuleKind::Key, detail: "win+x".into(), app: "x".into(), user: "me".into(), created: String::new(), text: String::new() },
        ];
        for r in &bad {
            let key = RuleKey { kind: r.kind, detail: r.detail.clone(), app: r.app.clone() };
            assert!(find(std::slice::from_ref(r), &key, "me").is_none(), "{:?}", r);
        }
    }

    #[test]
    fn removing_one_and_removing_all_for_a_user() {
        let k1 = key_for(&Action::Click { id: 1 }, Some(&el("Send")), "mail").unwrap();
        let k2 = key_for(&Action::Close { title: "x".into() }, None, "mail").unwrap();
        let mut r = with_added(vec![], &k1, "me", "a".into(), String::new());
        r = with_added(r, &k2, "me", "b".into(), String::new());
        r = with_added(r, &k1, "kid", "c".into(), String::new());
        assert_eq!(without(r.clone(), "a").len(), 2);
        let left = without_user(r, "me");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].user, "kid");
    }

    #[test]
    fn the_store_round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("byte-rules-{}", uuid::Uuid::new_v4().simple()));
        let store = RuleStore::new(dir.join("rules.json"));
        let k = key_for_app("Notepad.exe").unwrap();
        assert!(!store.matches(&k, "me"));
        store.add(&k, "me").unwrap();
        store.add(&k, "me").unwrap();
        assert!(store.matches(&k, "me"));
        assert_eq!(store.for_user("me").len(), 1);
        let id = store.for_user("me")[0].id.clone();
        store.remove(&id).unwrap();
        assert!(!store.matches(&k, "me"));
        store.add(&k, "me").unwrap();
        store.clear("me").unwrap();
        assert!(store.load().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
