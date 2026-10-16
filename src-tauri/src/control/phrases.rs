//! What is said out loud and handled here without a model: "how's my PC", "lock the computer", "take a screenshot", "mic off",
//! "brightness 40", "skip this song", "open notepad", "switch to Chrome", "scroll down", "find the invoice from last week", and a
//! routine's own phrase. Reading is by plain rules (a model splits these badly and is slower); doing is the fixed functions
//! of this module. Each group has its own setting (see `Flags`).

use std::path::PathBuf;

use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::actions::{self, Action, Step};
use super::sysctl::{MediaKey, SysOp};
use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::AppResult;
use crate::tools::SourceBook;

/// Which parts are on for this turn (from Settings; off in private chats and kids mode).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub pc_control: bool,
    pub routines: bool,
    pub find_files: bool,
    pub pc_status: bool,
    /// The person turned the approval cards off (Settings → PC control → "Never ask, just do it").
    pub never_ask: bool,
    /// Guided help: a how-to question about the window in front gets a walk-through offer (control/guide.rs).
    pub guided_help: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Status,
    Sys(SysOp),
    /// A step the executor does (open, focus, close, scroll, key, type).
    Direct(Action),
    Find(String),
    /// Anything else that asks BYTE to do something on the PC ("open Edge and search for …"): planned from the screen, then run step by step.
    Task(String),
    Stop,
}

fn clean(q: &str) -> String {
    let mut l = q.trim().to_lowercase();
    loop {
        let before = l.clone();
        for p in ["hey byte,", "hey byte", "ok byte,", "okay byte,", "please ", "can you ", "could you ", "would you ", "will you ", "byte,", "go ahead and "] {
            if let Some(rest) = l.strip_prefix(p) {
                l = rest.trim_start_matches([',', ' ']).to_string();
            }
        }
        if l == before {
            break;
        }
    }
    l.trim_end_matches(['?', '!', '.', ',']).trim_end_matches(" please").trim_end_matches(" for me").trim_end_matches(" now").trim().to_string()
}

fn number(l: &str) -> Option<u8> {
    l.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse::<u32>().ok()).find(|n| *n <= 100).map(|n| n as u8)
}

fn has_any(l: &str, words: &[&str]) -> bool {
    words.iter().any(|w| l.contains(w))
}

fn about_pc(l: &str) -> bool {
    l.split(|c: char| !c.is_alphanumeric()).any(|w| matches!(w, "pc" | "computer" | "machine" | "system" | "rig" | "desktop" | "laptop"))
}

/// "how's my pc", "how is the computer doing", "pc status", "is my pc running hot".
pub fn status_ask(l: &str) -> bool {
    let health = has_any(l, ["how's", "hows", "how is", "how are", "status", "health", "running hot", "too hot", "overheating", "temperature", "check on", "check my", "how much memory", "how much ram", "free space", "disk space", "storage left"].as_slice());
    (health && about_pc(l) && !has_any(l, ["weather", "outside"].as_slice())) || l == "pc status" || l == "system status"
}

/// A fixed PC control from a sentence.
pub fn sys_ask(l: &str) -> Option<SysOp> {
    let pcish = about_pc(l) || l.split_whitespace().count() <= 3;
    if (l.starts_with("lock ") || l == "lock" || l.contains("lock the screen") || l.contains("lock my screen") || l.contains("lock the pc")) && pcish && !l.contains("unlock") {
        return Some(SysOp::Lock);
    }
    if (l.contains("to sleep") || l.contains("sleep mode") || l == "sleep" || l == "go to sleep" || l.starts_with("sleep the ")) && (about_pc(l) || l == "sleep" || l == "go to sleep") && !l.contains("display") && !l.contains("screen") && !l.contains("monitor") {
        return Some(SysOp::Sleep);
    }
    if l.contains("screenshot") || l.contains("screen shot") || l.contains("screen capture") || l == "capture the screen" || l == "capture my screen" {
        // "what's in my screenshot", "find my screenshot" are questions about a file, not a capture.
        if l.starts_with("take ") || l.starts_with("make ") || l.starts_with("grab ") || l.starts_with("snap ") || l.starts_with("capture ") || l == "screenshot" || l.starts_with("screenshot") {
            return Some(SysOp::Screenshot);
        }
    }
    if l.contains("brightness") || (l.contains("dim") && has_any(l, ["screen", "display", "monitor"].as_slice())) || l.contains("brighter") {
        if let Some(n) = number(l) {
            if l.contains("set") || l.contains("to ") || l.contains('%') || l.contains("percent") || l.starts_with("brightness") {
                return Some(SysOp::Brightness(n));
            }
        }
        if l.contains("up") || l.contains("increase") || l.contains("raise") || l.contains("brighter") || l.contains("higher") {
            return Some(SysOp::BrightnessStep(15));
        }
        if l.contains("down") || l.contains("decrease") || l.contains("lower") || l.contains("dim") || l.contains("darker") || l.contains("reduce") {
            return Some(SysOp::BrightnessStep(-15));
        }
    }
    let mic = l.contains("mic") || l.contains("microphone");
    if mic && !l.contains("test") && !l.contains("settings") {
        if has_any(l, ["unmute", "turn on", "turn the mic on", "mic on", "microphone on", "mic back on", "enable", "switch on", "un-mute"].as_slice()) {
            return Some(SysOp::MicMute(Some(false)));
        }
        if has_any(l, ["mute", "turn off", "mic off", "disable", "switch off", "silence"].as_slice()) {
            return Some(SysOp::MicMute(Some(true)));
        }
        if l.contains("toggle") {
            return Some(SysOp::MicMute(None));
        }
    }
    if l == "next track" || l == "next song" || l == "skip" || l == "skip this song" || l == "skip this track" || l == "skip song" || l == "skip track" || l == "next" {
        return Some(SysOp::Media(MediaKey::Next));
    }
    if l == "previous track" || l == "previous song" || l == "last song" || l == "go back a song" || l == "previous" {
        return Some(SysOp::Media(MediaKey::Previous));
    }
    if l == "play pause" || l == "play/pause" || l == "toggle playback" || l == "pause or play" {
        return Some(SysOp::Media(MediaKey::PlayPause));
    }
    None
}

const NOT_PROGRAM: &[&str] = &["it", "that", "this", "the", "up", "my", "a", "settings", "bluetooth", "wifi", "wi-fi", "sound", "display"];

/// Program words from "open notepad": at most three words, no joiners.
fn program_words(rest: &str) -> Option<String> {
    let r = rest.trim().trim_start_matches("the ").trim_start_matches("my ").trim_end_matches(" app").trim_end_matches(" program").trim_end_matches(" application").trim();
    let words: Vec<&str> = r.split_whitespace().collect();
    if words.is_empty() || words.len() > 3 || words.iter().any(|w| matches!(*w, "and" | "then" | "to" | "with" | "for" | "in" | "on" | "at" | "from" | "settings")) || NOT_PROGRAM.contains(&r) {
        return None;
    }
    Some(r.to_string())
}

/// "open notepad" -> open; "switch to chrome" -> focus; "close notepad" -> close; "scroll down"; "press ctrl s"; "type hello".
pub fn direct_ask(l: &str) -> Option<Action> {
    for p in ["open ", "launch ", "start ", "run "] {
        if let Some(rest) = l.strip_prefix(p) {
            let name = program_words(rest)?;
            // A web address or a file is for the browser agent or the file finder.
            if name.contains('.') || name.contains('/') || name.contains(':') || super::find::is_find_request(l) {
                return None;
            }
            let a = Action::Open { target: name };
            return (actions::risk(&a) != actions::Risk::Never).then_some(a);
        }
    }
    for p in ["switch to ", "focus on ", "bring up ", "go back to "] {
        if let Some(rest) = l.strip_prefix(p) {
            let t = program_words(rest)?;
            return Some(Action::Focus { title: t });
        }
    }
    for p in ["close ", "quit ", "exit "] {
        if let Some(rest) = l.strip_prefix(p) {
            if matches!(rest.trim(), "this window" | "the window" | "window" | "this" | "it" | "that") {
                return Some(Action::Key { combo: "alt+f4".into() });
            }
            let t = program_words(rest)?;
            return Some(Action::Close { title: t });
        }
    }
    if matches!(l, "scroll down" | "scroll up" | "page down" | "page up") {
        return Some(Action::Scroll { direction: if l.ends_with("up") { actions::Direction::Up } else { actions::Direction::Down }, amount: Some(if l.starts_with("page") { 8 } else { 4 }) });
    }
    if let Some(rest) = l.strip_prefix("press ") {
        let combo = rest.split_whitespace().map(|w| match w {
            "control" => "ctrl",
            "windows" | "windows-key" => "win",
            "escape" => "esc",
            "return" => "enter",
            "plus" => "+",
            "page-down" => "pagedown",
            "page-up" => "pageup",
            o => o,
        }).filter(|w| *w != "and").collect::<Vec<_>>().join("+");
        let a = Action::Key { combo };
        return (actions::validate(&a).is_ok() && actions::risk(&a) != actions::Risk::Never).then_some(a);
    }
    None
}

pub fn ask(q: &str) -> Option<Ask> {
    let l = clean(q);
    if l.is_empty() {
        return None;
    }
    if matches!(l.as_str(), "stop" | "stop that" | "cancel that" | "stop controlling my pc" | "stop controlling the computer" | "never mind stop" | "abort") {
        return Some(Ask::Stop);
    }
    if status_ask(&l) {
        return Some(Ask::Status);
    }
    if let Some(op) = sys_ask(&l) {
        return Some(Ask::Sys(op));
    }
    if super::find::is_find_request(&l) {
        return Some(Ask::Find(q.to_string()));
    }
    direct_ask(&l).map(Ask::Direct).or_else(|| looks_like_pc_action(q).then(|| Ask::Task(q.to_string())))
}

/// A step for the executor from the plain action.
pub fn step(a: Action) -> Step {
    Step { action: a, expect: actions::Expect::default() }
}

fn gated(flags: &Flags, a: &Ask) -> bool {
    match a {
        Ask::Status => flags.pc_status,
        Ask::Sys(_) | Ask::Direct(_) | Ask::Task(_) | Ask::Stop => flags.pc_control,
        Ask::Find(_) => flags.find_files,
    }
}

/// A request to DO something on the PC ("can you open up Edge for me and search…", "switch to Chrome"), as opposed to a question about
/// something ("how do I open a bank account"). Only the start of the sentence counts, after politeness.
pub fn looks_like_pc_action(q: &str) -> bool {
    let mut l = q.trim().to_lowercase();
    for p in ["hey byte,", "hey byte", "ok byte,", "okay byte,", "byte,"] {
        if let Some(r) = l.strip_prefix(p) {
            l = r.trim_start_matches([',', ' ']).to_string();
        }
    }
    for p in ["can you ", "could you ", "would you ", "will you ", "please ", "i want you to ", "i need you to ", "go ahead and ", "just "] {
        while let Some(r) = l.strip_prefix(p) {
            l = r.to_string();
        }
    }
    ["open ", "launch ", "start up ", "switch to ", "go to the desktop", "close the ", "close my ", "minimize ", "maximize ", "mute the ", "turn up the volume", "turn down the volume", "turn the volume", "lock the computer", "lock my pc", "put the computer to sleep", "take a screenshot"].iter().any(|v| l.starts_with(v))
}

/// When a PC request comes in while PC control is switched off in Settings: what the model is told to say (and the person is shown),
/// instead of it answering "I can't control your desktop". `None` when the request is not one, or control is on (or held off for a
/// private chat or kids mode, which is not the person's setting).
pub fn off_hint(turn: &Turn<'_>, q: &str) -> Option<String> {
    if !cfg!(windows) || turn.app.is_none() || turn.modules.control.pc_control || !looks_like_pc_action(q) {
        return None;
    }
    let on_in_settings = super::settings_now(turn.app?).is_some_and(|s| s.pc_control);
    if on_in_settings {
        return None;
    }
    Some("The user asked BYTE to do something on their PC, but PC control is switched off (it starts off, and BYTE asks before anything risky). Say so in one or two short, friendly sentences and tell them how to turn it on: Settings, Control and screen, \"Let BYTE use this PC for you\". Do not say you are unable to control a computer, and do not pretend you did it. If part of the request can be done without controlling the PC (for example a web search), do that part.".to_string())
}

/// Whether this turn is one of ours: a switched-on group's phrase, or a routine's.
pub fn applies(turn: &Turn<'_>, q: &str) -> bool {
    let flags = turn.modules.control;
    if !(flags.pc_control || flags.routines || flags.find_files || flags.pc_status) || turn.app.is_none() {
        return false;
    }
    let l = clean(q);
    // The Mac/PC fixed controls and web phrases come first for what they already understand.
    if let Some(a) = ask(q) {
        if gated(&flags, &a) && !(matches!(a, Ask::Direct(_)) && crate::macctl::wants(&l)) {
            return true;
        }
    }
    flags.routines && turn.app.is_some_and(|app| super::routines::match_phrase(q, &super::routines::load(&routines_file(app))).is_some())
}

fn routines_file(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.state::<crate::state::AppState>().paths.data.join(super::routines::FILE)
}

fn card(send: Emit<'_>, turn: &Turn<'_>, tool: &str, title: &str, detail: &str, ok: bool) -> AppResult<()> {
    let id = format!("byte_pc_{}", uuid::Uuid::new_v4().simple());
    send(ChatEvent::ToolCall { id: id.clone(), name: tool.into(), args: json!({ "request": title }) })?;
    send(ChatEvent::ToolResult { id, ok, summary: title.to_string() })?;
    turn.log.record(tool, &json!({ "request": title }), ok, detail);
    send(ChatEvent::MacDone(crate::macctl::MacDone { app: "PC".into(), title: title.to_string(), detail: detail.to_string(), ok, undo: None }))?;
    Ok(())
}

fn say_notes(text: &str) -> String {
    format!("Done: {text}")
}

pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(app) = turn.app else { return Ok(None) };
    let flags = turn.modules.control;
    // A routine's own phrase.
    if flags.routines {
        let list = super::routines::load(&routines_file(app));
        if let Some(r) = super::routines::match_phrase(question, &list).cloned() {
            let app2 = app.clone();
            let r2 = r.clone();
            let report = tokio::task::spawn_blocking(move || super::routines::run(&r2, &super::routines::PcDoer { app: Some(&app2) })).await.map_err(|e| crate::error::AppError::msg(e.to_string()))?;
            let said = report.say(&r);
            super::activity(app, &format!("Routine \"{}\"", r.phrase), report.failed.is_none(), &said);
            card(send, turn, "pc_routine", &said, &r.steps.iter().map(|s| s.label()).collect::<Vec<_>>().join(" · "), report.failed.is_none())?;
            return Ok(Some((SourceBook::default(), say_notes(&said))));
        }
    }
    let Some(a) = ask(question).filter(|a| gated(&flags, a)) else { return Ok(None) };
    let (title, detail, ok, notes) = match a {
        Ask::Stop => {
            super::request_stop();
            super::guide::send_cmd(super::guide::Cmd::Stop);
            ("Stopped".to_string(), String::new(), true, say_notes("Stopped."))
        }
        Ask::Status => {
            let text = tokio::task::spawn_blocking(super::status::spoken).await.unwrap_or_default();
            super::activity(app, "How's my PC", true, "");
            ("PC status".to_string(), text.clone(), true, format!("The PC's status is below, put together by BYTE from the PC's own numbers. Say it to the user in your own short words, keeping every number and unit exactly:\n{text}"))
        }
        Ask::Sys(op) => {
            if super::sysctl::risk(&op) == actions::Risk::Asks && !flags.never_ask {
                let yes = crate::macctl::ask_ok("BYTE wants to do this on your PC", "PC control", vec![("Step".into(), super::sysctl::describe(&op))], cancel, send).await?;
                if !yes {
                    super::activity(app, &super::sysctl::describe(&op), false, "you said no");
                    return Ok(Some((SourceBook::default(), say_notes("Okay, I didn't do that."))));
                }
            }
            let input = super::real_input();
            let res = tokio::task::spawn_blocking(move || super::sysctl::run(&op, &*input)).await.map_err(|e| crate::error::AppError::msg(e.to_string()))?;
            match res {
                Ok(out) => {
                    let said = super::sysctl::say(&op, &out);
                    super::activity(app, &super::sysctl::describe(&op), true, "");
                    (super::sysctl::describe(&op), said.clone(), true, say_notes(&said))
                }
                Err(e) => {
                    super::activity(app, &super::sysctl::describe(&op), false, &e);
                    (super::sysctl::describe(&op), e.clone(), false, format!("BYTE could not do that: {e}. Tell the user in one short sentence."))
                }
            }
        }
        Ask::Direct(action) => {
            let desc = actions::describe(&action);
            let report = super::run_plan_steps(app, vec![step(action)], question, cancel, send).await;
            (desc, report.summary.clone(), report.ok, if report.ok { say_notes(&report.summary) } else { format!("BYTE could not finish: {}. Tell the user in one short sentence.", report.summary) })
        }
        Ask::Task(goal) => match super::plan_goal(app, &goal).await {
            Ok((steps, _)) => {
                let n = steps.len();
                let report = super::run_plan_steps(app, steps, &goal, cancel, send).await;
                let title = format!("{n} step{}", if n == 1 { "" } else { "s" });
                (title, report.summary.clone(), report.ok, if report.ok { say_notes(&report.summary) } else { format!("BYTE could not finish: {}. Tell the user in one short sentence.", report.summary) })
            }
            Err(e) => {
                super::activity(app, "Planning", false, &e);
                ("Could not plan that".to_string(), e.clone(), false, format!("BYTE could not plan that: {e}. Tell the user in one or two short sentences, and offer what you can do without controlling the PC."))
            }
        },
        Ask::Find(text) => {
            let (roots, today) = (find_roots(app), chrono::Local::now().date_naive());
            let Some(q) = super::find::parse_query(&text, today) else { return Ok(None) };
            if roots.is_empty() {
                let m = "Find files only looks in folders you choose. Choose them in Settings, then ask again.";
                return Ok(Some((SourceBook::default(), format!("BYTE has no folders to search. Tell the user: {m}"))));
            }
            let roots2 = roots.clone();
            let found = tokio::task::spawn_blocking(move || super::find::search(&q, &roots2, 5)).await.unwrap_or_default();
            let opened = match found.first() {
                Some(best) => super::find::reveal(best, &roots).is_ok(),
                None => false,
            };
            let said = super::find::say(&found, opened);
            super::activity(app, "Find files", !found.is_empty(), &format!("{} found", found.len()));
            ("Find files".to_string(), said.clone(), !found.is_empty(), format!("Tell the user, in these words or very close: {said}"))
        }
    };
    card(send, turn, "pc_control", &title, &detail, ok)?;
    Ok(Some((SourceBook::default(), notes)))
}

fn find_roots(app: &tauri::AppHandle) -> Vec<PathBuf> {
    super::settings_now(app).map(|s| s.find_folders.iter().map(PathBuf::from).collect()).unwrap_or_default()
}

#[cfg(test)]
mod off_tests {
    use super::looks_like_pc_action as a;

    #[test]
    fn a_free_form_request_becomes_a_task_for_the_planner() {
        use super::{ask, Ask};
        assert!(matches!(ask("can you open up microsoft edge for me and search for reddit posts on nuclear fusion for a project"), Some(Ask::Task(_))));
        assert!(matches!(ask("open notepad"), Some(Ask::Direct(_))), "a simple phrase keeps its fast path");
        assert!(ask("how do I open a bank account").is_none());
    }

    #[test]
    fn a_request_to_do_something_is_told_from_a_question_about_it() {
        assert!(a("can you open up microsoft edge for me and search for reddit posts on nuclear fusion for a project"));
        assert!(a("Hey BYTE, please open Notepad"));
        assert!(a("switch to chrome"));
        assert!(a("could you mute the sound"));
        assert!(!a("how do I open a bank account"));
        assert!(!a("what does open source mean"));
        assert!(!a("tell me about the start of the war"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(q: &str) -> Option<Ask> {
        ask(q)
    }

    #[test]
    fn how_is_my_pc() {
        for q in ["how's my pc", "Hey BYTE, how is my computer doing?", "how's my pc running", "is my computer running hot", "pc status", "check on my pc", "how much ram is my computer using", "how's the system"] {
            assert_eq!(a(q), Some(Ask::Status), "{q}");
        }
        for q in ["how's the weather", "how's it going", "how is the economy", "status of my order"] {
            assert_ne!(a(q), Some(Ask::Status), "{q}");
        }
    }

    #[test]
    fn lock_sleep_screenshot() {
        assert_eq!(a("lock my computer"), Some(Ask::Sys(SysOp::Lock)));
        assert_eq!(a("lock the screen"), Some(Ask::Sys(SysOp::Lock)));
        assert_eq!(a("lock"), Some(Ask::Sys(SysOp::Lock)));
        assert_eq!(a("put the pc to sleep"), Some(Ask::Sys(SysOp::Sleep)));
        assert_eq!(a("put my computer to sleep please"), Some(Ask::Sys(SysOp::Sleep)));
        assert_eq!(a("go to sleep"), Some(Ask::Sys(SysOp::Sleep)));
        assert_eq!(a("take a screenshot"), Some(Ask::Sys(SysOp::Screenshot)));
        assert_eq!(a("hey byte screenshot"), Some(Ask::Sys(SysOp::Screenshot)));
        assert_ne!(a("where is my screenshot from yesterday"), Some(Ask::Sys(SysOp::Screenshot)));
        assert_ne!(a("turn off the screen"), Some(Ask::Sys(SysOp::Sleep)), "a display is not the PC");
        assert_ne!(a("unlock my computer"), Some(Ask::Sys(SysOp::Lock)));
        assert_ne!(a("how do i lock a door"), Some(Ask::Sys(SysOp::Lock)));
    }

    #[test]
    fn brightness_and_microphone() {
        assert_eq!(a("set the brightness to 40"), Some(Ask::Sys(SysOp::Brightness(40))));
        assert_eq!(a("brightness 70 percent"), Some(Ask::Sys(SysOp::Brightness(70))));
        assert_eq!(a("turn the brightness up"), Some(Ask::Sys(SysOp::BrightnessStep(15))));
        assert_eq!(a("dim the screen"), Some(Ask::Sys(SysOp::BrightnessStep(-15))));
        assert_eq!(a("make the screen brighter"), Some(Ask::Sys(SysOp::BrightnessStep(15))));
        assert_eq!(a("mute my mic"), Some(Ask::Sys(SysOp::MicMute(Some(true)))));
        assert_eq!(a("mute the microphone"), Some(Ask::Sys(SysOp::MicMute(Some(true)))));
        assert_eq!(a("unmute my mic"), Some(Ask::Sys(SysOp::MicMute(Some(false)))));
        assert_eq!(a("turn the microphone on"), Some(Ask::Sys(SysOp::MicMute(Some(false)))));
        assert_ne!(a("how do i test my microphone"), Some(Ask::Sys(SysOp::MicMute(Some(true)))));
    }

    #[test]
    fn media_keys() {
        assert_eq!(a("skip this song"), Some(Ask::Sys(SysOp::Media(MediaKey::Next))));
        assert_eq!(a("next track"), Some(Ask::Sys(SysOp::Media(MediaKey::Next))));
        assert_eq!(a("previous song"), Some(Ask::Sys(SysOp::Media(MediaKey::Previous))));
        assert_eq!(a("play pause"), Some(Ask::Sys(SysOp::Media(MediaKey::PlayPause))));
    }

    #[test]
    fn direct_open_switch_close_scroll_press() {
        assert_eq!(a("open notepad"), Some(Ask::Direct(Action::Open { target: "notepad".into() })));
        assert_eq!(a("hey byte can you launch the calculator app"), Some(Ask::Direct(Action::Open { target: "calculator".into() })));
        assert_eq!(a("open visual studio code"), Some(Ask::Direct(Action::Open { target: "visual studio code".into() })));
        assert_eq!(a("switch to chrome"), Some(Ask::Direct(Action::Focus { title: "chrome".into() })));
        assert_eq!(a("close notepad"), Some(Ask::Direct(Action::Close { title: "notepad".into() })));
        assert_eq!(a("close this window"), Some(Ask::Direct(Action::Key { combo: "alt+f4".into() })));
        assert_eq!(a("scroll down"), Some(Ask::Direct(Action::Scroll { direction: actions::Direction::Down, amount: Some(4) })));
        assert_eq!(a("press control s"), Some(Ask::Direct(Action::Key { combo: "ctrl+s".into() })));
        assert_eq!(a("press escape"), Some(Ask::Direct(Action::Key { combo: "esc".into() })));
    }

    #[test]
    fn things_that_belong_elsewhere_or_are_refused_are_not_taken() {
        for q in ["open bluetooth settings", "open example.com", "open https://example.com", "open notepad and type hello", "open the pod bay doors and", "open cmd", "open powershell", "open", "close", "press", "press win r", "press ctrl alt delete", "open the file i downloaded yesterday", "start a timer for ten minutes"] {
            assert!(!matches!(a(q), Some(Ask::Direct(_))), "{q}: {:?}", a(q));
        }
    }

    #[test]
    fn find_requests_become_find_asks() {
        assert!(matches!(a("find the invoice from last week"), Some(Ask::Find(_))));
        assert!(matches!(a("hey byte where's my resume"), Some(Ask::Find(_))));
    }

    #[test]
    fn stop_is_understood() {
        assert_eq!(a("stop"), Some(Ask::Stop));
        assert_eq!(a("hey byte, cancel that"), Some(Ask::Stop));
        assert_eq!(a("stop the music"), None);
    }

    #[test]
    fn each_group_has_its_own_switch() {
        let off = Flags::default();
        for q in ["how's my pc", "lock my computer", "open notepad", "find the invoice from last week", "stop"] {
            assert!(!gated(&off, &ask(q).unwrap()), "{q}");
        }
        assert!(gated(&Flags { pc_status: true, ..off }, &Ask::Status));
        assert!(!gated(&Flags { pc_status: true, ..off }, &Ask::Sys(SysOp::Lock)));
        assert!(gated(&Flags { pc_control: true, ..off }, &Ask::Sys(SysOp::Lock)));
        assert!(gated(&Flags { find_files: true, ..off }, &Ask::Find(String::new())));
        assert!(!gated(&Flags { pc_control: true, ..off }, &Ask::Find(String::new())));
    }

    #[test]
    fn typed_actions_from_phrases_are_valid_and_never_dangerous() {
        for q in ["open notepad", "switch to chrome", "close notepad", "press ctrl s", "scroll up"] {
            if let Some(Ask::Direct(act)) = ask(q) {
                assert!(actions::validate(&act).is_ok(), "{q}");
                assert_ne!(actions::risk(&act), actions::Risk::Never, "{q}");
            } else {
                panic!("{q}");
            }
        }
    }
}
