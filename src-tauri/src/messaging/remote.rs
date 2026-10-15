//! "Ask the PC from the phone" (Telegram). With the switch on, a message from an allowed PRIVATE chat can:
//!
//! - start one of the person's saved routines (`run <name>`, `/list` shows them), or
//! - ask a plain question, answered by the local model, using only reading tools (no PC control).
//!
//! It is never arbitrary PC control: that needs a separate switch, "allow control from Telegram", which is off. Either way the
//! result is sent back to the chat, and "ask me before each one" makes every request wait for a yes on the PC first.
//! Group chats are ignored (anyone in the group could type), and messages from other chats never get here.

use futures_util::future::BoxFuture;

use super::config::ConnCfg;
use super::reply::Pending;
use super::service::{self, Hub};
use super::{Msg, Transport, TELEGRAM};
use crate::error::{AppError, AppResult};

/// What the PC side can do for a message (the app's real one, or a test double).
pub trait Executor: Send + Sync {
    fn routines(&self) -> Vec<String>;
    /// Is this a request to operate the PC (open, close, type, change a setting...) rather than a question?
    fn is_pc_action(&self, q: &str) -> bool;
    /// Runs a saved routine and says how it went.
    fn run_routine<'a>(&'a self, name: &'a str) -> BoxFuture<'a, AppResult<String>>;
    /// Answers a question with nobody watching; `control` lets it use PC control (and nothing else changes).
    fn ask<'a>(&'a self, q: &'a str, control: bool) -> BoxFuture<'a, AppResult<String>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Help,
    List,
    Run(String),
    Ask(String),
}

pub fn parse(text: &str) -> Cmd {
    let t = text.trim();
    let l = t.to_lowercase();
    let bare = l.trim_start_matches('/');
    if matches!(bare, "help" | "start" | "?") {
        return Cmd::Help;
    }
    if matches!(bare, "list" | "routines") {
        return Cmd::List;
    }
    for p in ["run ", "start ", "do "] {
        if let Some(rest) = bare.strip_prefix(p) {
            let name = t[t.len() - rest.len()..].trim().trim_matches('"');
            if !name.is_empty() {
                return Cmd::Run(name.to_string());
            }
        }
    }
    if let Some(rest) = bare.strip_prefix("ask ") {
        return Cmd::Ask(t[t.len() - rest.len()..].trim().to_string());
    }
    Cmd::Ask(t.to_string())
}

pub const HELP: &str = "BYTE on your PC. Send me a question and I'll answer it. \"run <name>\" starts one of your saved routines, \"/list\" shows them.";

fn cap(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 1500 {
        format!("{}…", s.chars().take(1499).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Does what the command says (after any yes) and returns the words to send back.
pub async fn execute(exec: &dyn Executor, cfg: &ConnCfg, cmd: &Cmd) -> String {
    match cmd {
        Cmd::Help => HELP.to_string(),
        Cmd::List => {
            let r = exec.routines();
            if r.is_empty() {
                "You have no saved routines yet. Make one in BYTE (Automations).".into()
            } else {
                format!("Your routines:\n- {}", r.join("\n- "))
            }
        }
        Cmd::Run(name) => match exec.routines().into_iter().find(|r| r.eq_ignore_ascii_case(name)) {
            None => format!("I don't have a routine called \"{name}\". Send /list to see them."),
            Some(found) => match exec.run_routine(&found).await {
                Ok(s) => cap(&format!("{found}: {s}")),
                Err(e) => cap(&format!("{found} didn't finish: {e}")),
            },
        },
        Cmd::Ask(q) => {
            let pc = exec.is_pc_action(q);
            if pc && !cfg.allow_control {
                return "That would operate the PC. \"Allow control from Telegram\" is off in BYTE's settings, so I only answer questions and run your saved routines from here.".into();
            }
            match exec.ask(q, pc && cfg.allow_control).await {
                Ok(a) => cap(&a),
                Err(e) => cap(&format!("I couldn't do that: {e}")),
            }
        }
    }
}

/// Handles the new Telegram messages. Replies are posted through `service::post` (allow-list, rate limit, log).
/// Returns the ids of requests now waiting for a yes.
pub async fn handle(hub: &Hub, t: &dyn Transport, exec: &dyn Executor, secret: &str, msgs: &[Msg], now: i64) -> Vec<String> {
    let mut waiting = vec![];
    let cfg = hub.config().get(TELEGRAM);
    if !cfg.enabled || !cfg.phone_ask {
        return waiting;
    }
    for m in msgs {
        if !m.private || m.from_bot || !cfg.allows(&m.channel) || m.text.trim().is_empty() {
            continue;
        }
        let cmd = parse(&m.text);
        // Help and the list never need a yes.
        let needs_yes = cfg.confirm_each && !matches!(cmd, Cmd::Help | Cmd::List);
        if needs_yes {
            let id = uuid::Uuid::new_v4().simple().to_string();
            hub.pending.add(Pending { id: id.clone(), kind: "phone".into(), connector: TELEGRAM.into(), channel: m.channel.clone(), channel_label: cfg.label(&m.channel), author: m.author.clone(), incoming: m.text.chars().take(400).collect(), draft: m.text.clone(), created: now, msg: Some(m.clone()) });
            hub.log.note(now, TELEGRAM, "draft", &m.channel, 1, true, "phone request waiting for a yes");
            let _ = service::post(hub, t, &m.channel, "Waiting for a yes on the PC before I do that.", secret, now).await;
            waiting.push(id);
            continue;
        }
        let text = execute(exec, &cfg, &cmd).await;
        if let Err(e) = service::post(hub, t, &m.channel, &text, secret, now).await {
            log::warn!("telegram: couldn't send the result back: {e}");
        }
    }
    waiting
}

/// The person said yes on the PC to a waiting phone request: run it and send the result back.
pub async fn approve(hub: &Hub, t: &dyn Transport, exec: &dyn Executor, secret: &str, id: &str, now: i64) -> AppResult<String> {
    let p = hub.pending.take(id).ok_or_else(|| AppError::msg("That request is no longer waiting."))?;
    if p.kind != "phone" {
        hub.pending.put_back(p);
        return Err(AppError::msg("That isn't a phone request."));
    }
    let cfg = hub.config().get(TELEGRAM);
    let text = execute(exec, &cfg, &parse(&p.draft)).await;
    service::post(hub, t, &p.channel, &text, secret, now).await?;
    Ok(text)
}

#[cfg(test)]
pub struct Stub {
    pub ran: std::sync::Mutex<Vec<String>>,
    pub asked: std::sync::Mutex<Vec<(String, bool)>>,
}

#[cfg(test)]
impl Stub {
    pub fn new() -> Stub {
        Stub { ran: Default::default(), asked: Default::default() }
    }
}

#[cfg(test)]
impl Executor for Stub {
    fn routines(&self) -> Vec<String> {
        vec!["Game time".into(), "Backup photos".into()]
    }
    fn is_pc_action(&self, q: &str) -> bool {
        q.to_lowercase().starts_with("open ") || q.to_lowercase().starts_with("close ")
    }
    fn run_routine<'a>(&'a self, name: &'a str) -> BoxFuture<'a, AppResult<String>> {
        self.ran.lock().unwrap().push(name.into());
        Box::pin(async { Ok("done".to_string()) })
    }
    fn ask<'a>(&'a self, q: &'a str, control: bool) -> BoxFuture<'a, AppResult<String>> {
        self.asked.lock().unwrap().push((q.into(), control));
        Box::pin(async move { Ok(format!("answer to {q}")) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::config::Allowed;
    use crate::messaging::fake::Fake;

    fn setup(confirm: bool, control: bool) -> (tempfile::TempDir, Hub) {
        let d = tempfile::tempdir().unwrap();
        let hub = Hub::open(d.path().to_path_buf(), "p");
        hub.update(|c| {
            let e = c.entry("telegram");
            e.enabled = true;
            e.phone_ask = true;
            e.confirm_each = confirm;
            e.allow_control = control;
            e.channels = vec![Allowed { id: "42".into(), name: "Me".into() }];
        })
        .unwrap();
        (d, hub)
    }

    fn msg(chat: &str, text: &str, private: bool) -> Msg {
        Msg { id: "1".into(), channel: chat.into(), author: "me".into(), text: text.into(), ts: 0, from_bot: false, private, cursor_key: "_".into(), meta: String::new() }
    }

    #[test]
    fn commands_are_read_plainly() {
        assert_eq!(parse("/start"), Cmd::Help);
        assert_eq!(parse("/list"), Cmd::List);
        assert_eq!(parse("run Game time"), Cmd::Run("Game time".into()));
        assert_eq!(parse("/run \"Backup photos\""), Cmd::Run("Backup photos".into()));
        assert_eq!(parse("what's the weather?"), Cmd::Ask("what's the weather?".into()));
        assert_eq!(parse("ask why is the sky blue"), Cmd::Ask("why is the sky blue".into()));
    }

    #[tokio::test]
    async fn off_by_default_nothing_happens() {
        let (_d, hub) = setup(false, false);
        hub.update(|c| c.entry("telegram").phone_ask = false).unwrap();
        let (f, x) = (Fake::new("telegram"), Stub::new());
        handle(&hub, &f, &x, "", &[msg("42", "hello", true)], 0).await;
        assert!(f.sent().is_empty() && x.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_question_is_answered_and_a_routine_runs_and_both_report_back() {
        let (_d, hub) = setup(false, false);
        let (f, x) = (Fake::new("telegram"), Stub::new());
        handle(&hub, &f, &x, "", &[msg("42", "what is 2+2", true), msg("42", "run game time", true)], 0).await;
        assert_eq!(f.sent()[0], ("42".to_string(), "answer to what is 2+2".to_string()));
        assert_eq!(f.sent()[1].1, "Game time: done");
        assert_eq!(*x.ran.lock().unwrap(), vec!["Game time".to_string()]);
        assert_eq!(x.asked.lock().unwrap()[0].1, false, "a plain question never gets PC control");
    }

    #[tokio::test]
    async fn pc_control_needs_its_own_switch() {
        let (_d, hub) = setup(false, false);
        let (f, x) = (Fake::new("telegram"), Stub::new());
        handle(&hub, &f, &x, "", &[msg("42", "open notepad", true)], 0).await;
        assert!(f.sent()[0].1.contains("off in BYTE's settings"));
        assert!(x.asked.lock().unwrap().is_empty());
        hub.update(|c| c.entry("telegram").allow_control = true).unwrap();
        handle(&hub, &f, &x, "", &[msg("42", "open notepad", true)], 1).await;
        assert_eq!(x.asked.lock().unwrap()[0], ("open notepad".to_string(), true));
        assert_eq!(f.sent()[1].1, "answer to open notepad", "the result is reported back");
    }

    #[tokio::test]
    async fn groups_and_other_chats_are_ignored() {
        let (_d, hub) = setup(false, true);
        let (f, x) = (Fake::new("telegram"), Stub::new());
        handle(&hub, &f, &x, "", &[msg("42", "open notepad", false), msg("99", "open notepad", true)], 0).await;
        assert!(f.sent().is_empty() && x.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn confirm_each_waits_for_a_yes_on_the_pc() {
        let (_d, hub) = setup(true, false);
        let (f, x) = (Fake::new("telegram"), Stub::new());
        let waiting = handle(&hub, &f, &x, "", &[msg("42", "run game time", true), msg("42", "/list", true)], 0).await;
        assert_eq!(waiting.len(), 1, "/list needs no yes");
        assert!(x.ran.lock().unwrap().is_empty());
        assert!(f.sent()[0].1.contains("Waiting for a yes"));
        let said = approve(&hub, &f, &x, "", &waiting[0], 1).await.unwrap();
        assert_eq!(said, "Game time: done");
        assert_eq!(*x.ran.lock().unwrap(), vec!["Game time".to_string()]);
        assert!(hub.pending.list().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_routine_is_said_plainly() {
        let (_d, hub) = setup(false, false);
        let (f, x) = (Fake::new("telegram"), Stub::new());
        handle(&hub, &f, &x, "", &[msg("42", "run nonsense", true)], 0).await;
        assert!(f.sent()[0].1.contains("don't have a routine"));
    }
}
