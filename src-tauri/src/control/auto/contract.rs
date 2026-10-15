//! The run contract: what an autonomous job may use, set by the person before it starts. Steps inside it go ahead; steps outside
//! it pause the run and wait; a few things are refused whatever the contract says.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::limit;
use super::rules::app_key;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Contract {
    /// Programs (lower-case, no .exe) the job may operate. "browser" is BYTE's own browser.
    pub apps: Vec<String>,
    /// Folders the job may read from or write into.
    pub folders: Vec<String>,
    /// Who it may message (names or addresses as the connector knows them). "*" is anyone.
    pub may_message: Vec<String>,
    pub never_delete: bool,
    /// Always on: no payment is ever made by a job. Kept as a field so the contract reads whole.
    pub never_spend: bool,
    /// "30m", "2h", "unlimited"...
    pub time_budget: String,
    pub step_budget: String,
    pub message_budget: String,
}

impl Default for Contract {
    fn default() -> Self {
        Contract { apps: vec![], folders: vec![], may_message: vec![], never_delete: true, never_spend: true, time_budget: "30m".into(), step_budget: "200".into(), message_budget: "5".into() }
    }
}

impl Contract {
    /// The contract as it is enforced: spending stays off, names are tidy.
    pub fn normalized(mut self) -> Contract {
        self.never_spend = true;
        self.apps = tidy(self.apps.iter().map(|a| app_key(a)).collect());
        self.folders = tidy(self.folders.iter().map(|f| f.trim().to_string()).collect());
        self.may_message = tidy(self.may_message.iter().map(|m| m.trim().to_string()).collect());
        self
    }

    pub fn time_cap_ms(&self) -> Option<u64> {
        limit::time_ms(&self.time_budget, Some(30 * 60_000))
    }

    pub fn step_cap(&self) -> Option<u64> {
        limit::count(&self.step_budget, Some(200))
    }

    pub fn message_cap(&self) -> Option<u64> {
        limit::count(&self.message_budget, Some(5))
    }

    pub fn allows_app(&self, app: &str) -> bool {
        let k = app_key(app);
        !k.is_empty() && self.apps.iter().any(|a| *a == k)
    }

    /// Whether `path` is inside one of the folders (case-insensitive, `..` never counts).
    pub fn allows_folder(&self, path: &str) -> bool {
        let p = norm_path(path);
        if p.split('/').any(|s| s == "..") {
            return false;
        }
        self.folders.iter().any(|f| {
            let f = norm_path(f);
            !f.is_empty() && (p == f || p.starts_with(&format!("{f}/")))
        })
    }

    pub fn allows_recipient(&self, who: &str) -> bool {
        let w = who.trim().to_lowercase();
        self.may_message.iter().any(|m| m == "*" || (!w.is_empty() && m.to_lowercase() == w))
    }

    /// Judges one thing a step is about to do. `never_touch` is the protected list from Settings.
    pub fn judge(&self, fact: &Fact, never_touch: &[String]) -> Verdict {
        match fact {
            Fact::App(a) => {
                let k = app_key(a);
                if never_touch.iter().any(|n| app_key(n) == k) && !k.is_empty() {
                    Verdict::Refused(format!("{k} is on your never-touch list"))
                } else if self.allows_app(&k) {
                    Verdict::Inside
                } else {
                    Verdict::Outside(format!("{k} is not one of the programs this job may use"))
                }
            }
            Fact::Folder(p) => {
                if self.allows_folder(p) {
                    Verdict::Inside
                } else {
                    Verdict::Outside(format!("{p} is not in the folders this job may use"))
                }
            }
            Fact::Message(to) => {
                if self.allows_recipient(to) {
                    Verdict::Inside
                } else if to.trim().is_empty() {
                    Verdict::Outside("this would send a message and the job may not message just anyone".into())
                } else {
                    Verdict::Outside(format!("{to} is not someone this job may message"))
                }
            }
            Fact::Delete if self.never_delete => Verdict::Refused("the contract says never delete".into()),
            Fact::Delete => Verdict::Outside("this deletes something".into()),
            Fact::Spend => Verdict::Refused("a job never spends money".into()),
        }
    }
}

fn tidy(mut v: Vec<String>) -> Vec<String> {
    v.retain(|s| !s.is_empty());
    let mut seen: Vec<String> = Vec::new();
    v.retain(|s| {
        let k = s.to_lowercase();
        if seen.contains(&k) {
            false
        } else {
            seen.push(k);
            true
        }
    });
    v
}

fn norm_path(p: &str) -> String {
    p.trim().replace('\\', "/").trim_end_matches('/').to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    App(String),
    Folder(String),
    /// A message to this person ("" when not known).
    Message(String),
    Delete,
    Spend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Inside,
    /// Outside the contract: the run pauses and asks.
    Outside(String),
    /// Not done, whatever anyone says.
    Refused(String),
}

/// The counters of one run: steps and messages used, with the caps from the contract.
#[derive(Debug, Default)]
pub struct Meter {
    steps: AtomicU64,
    messages: AtomicU64,
    step_cap: Option<u64>,
    message_cap: Option<u64>,
}

impl Meter {
    pub fn new(step_cap: Option<u64>, message_cap: Option<u64>, steps: u64, messages: u64) -> Meter {
        Meter { steps: AtomicU64::new(steps), messages: AtomicU64::new(messages), step_cap, message_cap }
    }

    pub fn for_contract(c: &Contract, steps: u64, messages: u64) -> Meter {
        Meter::new(c.step_cap(), c.message_cap(), steps, messages)
    }

    pub fn steps(&self) -> u64 {
        self.steps.load(Ordering::SeqCst)
    }

    pub fn messages(&self) -> u64 {
        self.messages.load(Ordering::SeqCst)
    }

    /// Counts a step; the error says which budget ran out (the step is not counted then).
    pub fn take_step(&self) -> Result<(), String> {
        take(&self.steps, self.step_cap, "step")
    }

    pub fn take_message(&self) -> Result<(), String> {
        take(&self.messages, self.message_cap, "message")
    }
}

fn take(counter: &AtomicU64, cap: Option<u64>, what: &str) -> Result<(), String> {
    loop {
        let now = counter.load(Ordering::SeqCst);
        if let Some(c) = cap {
            if now >= c {
                return Err(format!("the {what} budget ({c}) is used up"));
            }
        }
        if counter.compare_exchange(now, now + 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c() -> Contract {
        Contract { apps: vec!["notepad".into(), "browser".into()], folders: vec!["C:\\Users\\me\\Work".into()], may_message: vec!["Mom".into()], ..Default::default() }.normalized()
    }

    #[test]
    fn programs_inside_and_outside() {
        let k = c();
        assert_eq!(k.judge(&Fact::App("Notepad.exe".into()), &[]), Verdict::Inside);
        assert!(matches!(k.judge(&Fact::App("excel".into()), &[]), Verdict::Outside(_)));
    }

    #[test]
    fn the_never_touch_list_wins_even_over_an_allowed_program() {
        let k = c();
        assert!(matches!(k.judge(&Fact::App("notepad".into()), &["Notepad".into()]), Verdict::Refused(_)));
    }

    #[test]
    fn folders_match_whole_folder_names_and_never_dotdot() {
        let k = c();
        assert!(k.allows_folder("c:/users/me/work/notes/a.txt"));
        assert!(k.allows_folder("C:\\Users\\me\\Work"));
        assert!(!k.allows_folder("C:\\Users\\me\\Workshop\\a.txt"));
        assert!(!k.allows_folder("C:\\Users\\me\\Work\\..\\Secrets\\a.txt"));
        assert!(!k.allows_folder("D:\\other"));
    }

    #[test]
    fn messages_only_to_the_listed_people_unless_anyone() {
        let mut k = c();
        assert_eq!(k.judge(&Fact::Message("mom".into()), &[]), Verdict::Inside);
        assert!(matches!(k.judge(&Fact::Message("Boss".into()), &[]), Verdict::Outside(_)));
        assert!(matches!(k.judge(&Fact::Message(String::new()), &[]), Verdict::Outside(_)));
        k.may_message.push("*".into());
        assert_eq!(k.judge(&Fact::Message("Boss".into()), &[]), Verdict::Inside);
    }

    #[test]
    fn never_delete_and_never_spend_are_refusals() {
        let mut k = c();
        assert!(matches!(k.judge(&Fact::Delete, &[]), Verdict::Refused(_)));
        k.never_delete = false;
        assert!(matches!(k.judge(&Fact::Delete, &[]), Verdict::Outside(_)));
        k.never_spend = false;
        assert!(matches!(k.judge(&Fact::Spend, &[]), Verdict::Refused(_)));
        assert!(k.normalized().never_spend, "spending cannot be switched on");
    }

    #[test]
    fn budgets_stop_at_the_cap_and_unlimited_never_does() {
        let m = Meter::new(Some(2), Some(1), 0, 0);
        assert!(m.take_step().is_ok() && m.take_step().is_ok());
        assert!(m.take_step().unwrap_err().contains("step budget (2)"));
        assert_eq!(m.steps(), 2);
        assert!(m.take_message().is_ok());
        assert!(m.take_message().is_err());
        let u = Meter::new(None, None, 0, 0);
        for _ in 0..1000 {
            assert!(u.take_step().is_ok());
        }
    }

    #[test]
    fn the_contract_reads_its_budgets_from_text() {
        let k = Contract { time_budget: "unlimited".into(), step_budget: "unlimited".into(), message_budget: "3".into(), ..Default::default() };
        assert_eq!((k.time_cap_ms(), k.step_cap(), k.message_cap()), (None, None, Some(3)));
        let d = Contract::default();
        assert_eq!((d.time_cap_ms(), d.step_cap(), d.message_cap()), (Some(1_800_000), Some(200), Some(5)));
    }

    #[test]
    fn tidying_drops_empty_and_repeated_names() {
        let k = Contract { apps: vec!["Notepad.exe".into(), "notepad".into(), " ".into()], ..Default::default() }.normalized();
        assert_eq!(k.apps, vec!["notepad".to_string()]);
    }
}
