//! Voice routines: a named phrase ("game time") and a list of fixed PC steps. Say the phrase to run it, or give it a schedule
//! ("weekdays 07:00"). Steps are a closed list, each checked before it is saved, and each runs through a fixed PC operation
//! (no shell, nothing the person typed becomes a command). Closing a program asks it to close like its window's X button, so a
//! program with unsaved work asks the person itself.

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::actions::{classify_target, Target};
use crate::error::{AppError, AppResult};
use crate::scheduler::{next_after, parse_spec};
use crate::state::AppState;

pub const FILE: &str = "routines.json";
pub const MAX_STEPS: usize = 12;
pub const MAX_ROUTINES: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum RStep {
    /// A program by name, "spotify".
    OpenApp { name: String },
    /// Asks the program's windows to close.
    CloseApp { name: String },
    Volume { percent: u8 },
    /// Turns pop-up notifications off or on (Windows' notification switch).
    DoNotDisturb { on: bool },
    /// A web address (http or https only).
    OpenUrl { url: String },
    /// One of the user's automations, by name.
    RunAutomation { name: String },
    // --- track:autonomy begin --- steps made by "watch me do this once" (control/auto/recorder.rs)
    /// Click the button or item with this name in the window whose title contains `window`.
    ClickElement { window: String, name: String },
    /// Press a key combination ("ctrl+s", "enter").
    PressKeys { combo: String },
    /// Type this text (only when the person chose to keep it).
    TypeText { text: String },
    // --- track:autonomy end ---
}

impl RStep {
    pub fn label(&self) -> String {
        match self {
            RStep::OpenApp { name } => format!("Open {name}"),
            RStep::CloseApp { name } => format!("Close {name}"),
            RStep::Volume { percent } => format!("Set the volume to {percent}%"),
            RStep::DoNotDisturb { on: true } => "Turn notifications off (back on at the next start or after 12 hours)".into(),
            RStep::DoNotDisturb { on: false } => "Turn notifications on".into(),
            RStep::OpenUrl { url } => format!("Open {url}"),
            RStep::RunAutomation { name } => format!("Run the automation \"{name}\""),
            // --- track:autonomy begin ---
            RStep::ClickElement { window, name } => format!("Click \"{name}\" in {window}"),
            RStep::PressKeys { combo } => format!("Press {combo}"),
            RStep::TypeText { text } => format!("Type {} characters", text.chars().count()),
            // --- track:autonomy end ---
        }
    }

    pub fn check(&self) -> Result<(), String> {
        match self {
            RStep::OpenApp { name } => match classify_target(name) {
                Ok(Target::Program(_)) => Ok(()),
                Ok(_) => Err(format!("\"{name}\" is not a program name; use Open a web address for links")),
                Err(e) => Err(e.to_string()),
            },
            RStep::CloseApp { name } => {
                let n = name.trim();
                if n.is_empty() || n.chars().count() > 60 || !n.chars().all(|c| c.is_alphanumeric() || " .-_".contains(c)) {
                    return Err("a program name is needed (letters, numbers, spaces)".into());
                }
                if matches!(n.to_lowercase().trim_end_matches(".exe"), "explorer" | "winlogon" | "csrss" | "services" | "lsass" | "svchost" | "system" | "dwm") {
                    return Err("BYTE won't close a part of Windows".into());
                }
                Ok(())
            }
            RStep::Volume { percent } if *percent > 100 => Err("the volume is 0 to 100".into()),
            RStep::Volume { .. } | RStep::DoNotDisturb { .. } => Ok(()),
            RStep::OpenUrl { url } => match classify_target(url) {
                Ok(Target::Uri(u)) if u.to_lowercase().starts_with("http://") || u.to_lowercase().starts_with("https://") => Ok(()),
                _ => Err("only http and https addresses can be opened".into()),
            },
            RStep::RunAutomation { name } if name.trim().is_empty() || name.chars().count() > 80 => Err("an automation name is needed".into()),
            RStep::RunAutomation { .. } => Ok(()),
            // --- track:autonomy begin ---
            RStep::ClickElement { window, name } => {
                if window.trim().is_empty() || name.trim().is_empty() || window.chars().count() > 200 || name.chars().count() > 200 {
                    Err("a window and the name of a button are needed".into())
                } else if super::uia::sensitive_name(name) {
                    Err("BYTE does not click password or card fields".into())
                } else {
                    Ok(())
                }
            }
            RStep::PressKeys { combo } => match super::actions::parse_combo(combo) {
                Ok(_) if super::actions::risk(&super::actions::Action::Key { combo: combo.clone() }) == super::actions::Risk::Never => Err("BYTE refuses that key combination".into()),
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            RStep::TypeText { text } => super::actions::validate(&super::actions::Action::Type { text: text.clone() }).map_err(|e| e.to_string()),
            // --- track:autonomy end ---
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    /// What is said, "game time".
    pub phrase: String,
    pub steps: Vec<RStep>,
    /// A schedule in `scheduler` form ("weekdays 07:00"), or empty for only-when-said.
    pub schedule: String,
    pub enabled: bool,
}

impl Default for Routine {
    fn default() -> Self {
        Routine { id: String::new(), phrase: String::new(), steps: vec![], schedule: String::new(), enabled: true }
    }
}

/// Lower-case words only: "Game time!" -> "game time".
pub fn normalize(phrase: &str) -> String {
    phrase.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whole-phrase check for saving.
pub fn check(r: &Routine, others: &[Routine]) -> Result<(), String> {
    let p = normalize(&r.phrase);
    if p.chars().count() < 3 || p.chars().count() > 60 {
        return Err("The phrase should be 3 to 60 letters, like \"game time\".".into());
    }
    if p.split(' ').count() > 8 {
        return Err("The phrase is too long to say.".into());
    }
    if p.starts_with("hey byte") {
        return Err("Leave out \"Hey BYTE\"; it is added when you say it.".into());
    }
    if others.iter().any(|o| o.id != r.id && normalize(&o.phrase) == p) {
        return Err("Another routine already has that phrase.".into());
    }
    if r.steps.is_empty() {
        return Err("Add at least one step.".into());
    }
    if r.steps.len() > MAX_STEPS {
        return Err(format!("A routine can have at most {MAX_STEPS} steps."));
    }
    for (i, s) in r.steps.iter().enumerate() {
        s.check().map_err(|e| format!("Step {}: {e}", i + 1))?;
    }
    if !r.schedule.trim().is_empty() && parse_spec(r.schedule.trim()).is_none() {
        return Err("The schedule isn't one BYTE understands (for example \"weekdays 07:00\").".into());
    }
    Ok(())
}

// ------------------------------------------------------------------ saying a routine

const LEAD_WORDS: &[&str] = &["hey", "byte", "ok", "okay", "please", "can", "could", "would", "you", "run", "start", "begin", "do", "activate", "launch", "trigger", "use", "set", "up", "turn", "on", "my", "the", "routine", "mode", "let's", "lets", "go", "into", "to"];

/// The routine a spoken sentence names, if it says its phrase and little else ("hey byte, run my game time routine").
pub fn match_phrase<'a>(q: &str, routines: &'a [Routine]) -> Option<&'a Routine> {
    let n = normalize(q);
    let words: Vec<&str> = n.split(' ').filter(|w| !w.is_empty()).collect();
    routines.iter().filter(|r| r.enabled).filter_map(|r| {
        let p = normalize(&r.phrase);
        let pw: Vec<&str> = p.split(' ').collect();
        // The phrase as a run of words, with only lead and trail words around it.
        let at = words.windows(pw.len()).position(|w| w == pw.as_slice())?;
        let before_ok = words[..at].iter().all(|w| LEAD_WORDS.contains(w));
        let after_ok = words[at + pw.len()..].iter().all(|w| matches!(*w, "routine" | "mode" | "please" | "now" | "for" | "me"));
        (before_ok && after_ok).then_some((pw.len(), r))
    }).max_by_key(|(len, _)| *len).map(|(_, r)| r)
}

// ------------------------------------------------------------------ running

/// What a step does on a real PC, behind a trait so a run is tested with a fake.
pub trait Doer {
    fn do_step(&self, step: &RStep) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub done: usize,
    pub failed: Option<(usize, String)>,
}

impl RunReport {
    pub fn say(&self, r: &Routine) -> String {
        match &self.failed {
            None => format!("{} is done.", sentence_case(&r.phrase)),
            Some((i, why)) => format!("{} stopped at step {}: {why}", sentence_case(&r.phrase), i + 1),
        }
    }
}

fn sentence_case(s: &str) -> String {
    let n = normalize(s);
    let mut c = n.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Runs the steps in order; stops at the first one that fails.
pub fn run(r: &Routine, doer: &dyn Doer) -> RunReport {
    for (i, s) in r.steps.iter().enumerate() {
        if let Err(e) = s.check().and_then(|_| doer.do_step(s)) {
            return RunReport { done: i, failed: Some((i, e)) };
        }
    }
    RunReport { done: r.steps.len(), failed: None }
}

pub struct PcDoer<'a> {
    pub app: Option<&'a AppHandle>,
}

impl Doer for PcDoer<'_> {
    fn do_step(&self, step: &RStep) -> Result<(), String> {
        use crate::pcctl::{run_op, WinOp};
        let op = |o: WinOp| run_op(&o).map(|_| ()).map_err(|e| e.text_for("that", true));
        match step {
            RStep::OpenApp { name } => op(WinOp::Open(name.trim().to_string())),
            RStep::OpenUrl { url } => op(WinOp::Open(url.trim().to_string())),
            RStep::CloseApp { name } => op(WinOp::Quit(name.trim().to_string())),
            RStep::Volume { percent } => op(WinOp::Volume(*percent)),
            RStep::DoNotDisturb { on } => {
                let marker = self.app.map(|a| marker_file(a));
                notifications(!*on, marker.as_deref())
            }
            // --- track:autonomy begin ---
            RStep::ClickElement { window, name } => {
                let screen = super::real_screen();
                screen.focus_window(window).map_err(|e| e.to_string())?;
                std::thread::sleep(std::time::Duration::from_millis(400));
                let shot = screen.snapshot().map_err(|e| e.to_string())?;
                let el = shot.find_named(name).cloned().ok_or_else(|| format!("\"{name}\" is not in {window} now"))?;
                if !el.enabled {
                    return Err(format!("\"{name}\" is switched off"));
                }
                screen.click(&el).map_err(|e| e.to_string())
            }
            RStep::PressKeys { combo } => {
                let c = super::actions::parse_combo(combo).map_err(|e| e.to_string())?;
                super::real_input().key_combo(&c).map_err(|e| e.to_string())
            }
            RStep::TypeText { text } => super::real_input().type_text(text).map_err(|e| e.to_string()),
            // --- track:autonomy end ---
            RStep::RunAutomation { name } => {
                let app = self.app.ok_or("automations aren't available here")?;
                let state = app.state::<AppState>();
                let list = crate::automations::list(&state.db).map_err(|e| e.to_string())?;
                let a = list.into_iter().find(|a| a.name.trim().eq_ignore_ascii_case(name.trim())).ok_or_else(|| format!("there is no automation called \"{name}\""))?;
                crate::automations::start(app, a, 0).map(|_| ()).map_err(|e| e.to_string())
            }
        }
    }
}

/// A note that BYTE turned notifications off (its time, in seconds), so they are turned back on later even if BYTE is closed.
pub const NOTIFY_MARKER: &str = "notifications_off";
/// Notifications BYTE turned off come back after this long at the latest.
pub const NOTIFY_OFF_MAX_SECS: u64 = 12 * 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub(crate) fn marker_file(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.data.join(NOTIFY_MARKER)
}

/// Whether notifications BYTE turned off at `since` are due back at `now`.
pub fn notify_due(since: u64, now: u64) -> bool {
    now.saturating_sub(since) >= NOTIFY_OFF_MAX_SECS
}

/// When the marker says notifications were turned off, if it does.
pub fn marker_since(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok().and_then(|t| t.trim().parse().ok())
}

/// Turns notifications back on if BYTE turned them off and `always` or the time is up. Returns whether it did.
pub fn restore_notifications(path: &Path, always: bool, turn_on: &dyn Fn() -> Result<(), String>) -> bool {
    let Some(since) = marker_since(path) else { return false };
    if !(always || notify_due(since, now_secs())) {
        return false;
    }
    if turn_on().is_ok() {
        let _ = std::fs::remove_file(path);
        return true;
    }
    false
}

/// Windows' own master switch for pop-up notifications (Settings → System → Notifications), set for this user. Turning them
/// off is noted in `marker`, and BYTE turns them back on at its next start or after 12 hours, whichever comes first.
pub(crate) fn notifications(on: bool, marker: Option<&Path>) -> Result<(), String> {
    set_toasts(on)?;
    if let Some(m) = marker {
        if on {
            let _ = std::fs::remove_file(m);
        } else {
            let _ = std::fs::write(m, now_secs().to_string());
        }
    }
    Ok(())
}

fn set_toasts(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let value = if on { "1" } else { "0" };
        let status = std::process::Command::new("reg.exe")
            .args(["add", r"HKCU\Software\Microsoft\Windows\CurrentVersion\PushNotifications", "/v", "ToastEnabled", "/t", "REG_DWORD", "/d", value, "/f"])
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err("Windows would not change the notification setting".into()) }
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        Err("this needs Windows".into())
    }
}

// ------------------------------------------------------------------ storage

pub fn load(path: &Path) -> Vec<Routine> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(path: &Path, list: &[Routine]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list)?)?;
    std::fs::rename(tmp, path)
}

fn file(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.data.join(FILE)
}

// ------------------------------------------------------------------ schedule

/// Routines whose schedule time fell in `(from, to]`.
pub fn due<Tz: TimeZone>(list: &[Routine], from: &DateTime<Tz>, to: &DateTime<Tz>) -> Vec<Routine> {
    list.iter()
        .filter(|r| r.enabled && !r.schedule.trim().is_empty())
        .filter(|r| parse_spec(r.schedule.trim()).and_then(|spec| next_after(&spec, from)).is_some_and(|t| t <= *to))
        .cloned()
        .collect()
}

/// Starts the loop that runs scheduled routines (every 30 seconds).
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = chrono::Local::now();
        // Notifications a routine turned off before BYTE was last closed come back now.
        restore_notifications(&marker_file(&app), true, &|| set_toasts(true));
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            restore_notifications(&marker_file(&app), false, &|| set_toasts(true));
            let now = chrono::Local::now();
            let on = super::settings_now(&app).is_some_and(|s| s.routines_enabled);
            if on {
                for r in due(&load(&file(&app)), &last, &now) {
                    let app2 = app.clone();
                    tokio::task::spawn_blocking(move || {
                        let report = run(&r, &PcDoer { app: Some(&app2) });
                        super::activity(&app2, &format!("Routine \"{}\" (scheduled)", r.phrase), report.failed.is_none(), &report.failed.as_ref().map(|f| f.1.clone()).unwrap_or_default());
                        crate::scheduler::notify(&app2, "BYTE routine", &report.say(&r));
                    });
                }
            }
            last = now;
        }
    });
}

// ------------------------------------------------------------------ commands

#[tauri::command]
pub fn routines_list(app: AppHandle) -> Vec<Routine> {
    load(&file(&app))
}

#[tauri::command]
pub fn routine_save(app: AppHandle, mut routine: Routine) -> AppResult<Routine> {
    let path = file(&app);
    let mut list = load(&path);
    if routine.id.trim().is_empty() {
        routine.id = uuid::Uuid::new_v4().simple().to_string();
    }
    routine.phrase = normalize(&routine.phrase);
    check(&routine, &list).map_err(AppError::msg)?;
    match list.iter_mut().find(|r| r.id == routine.id) {
        Some(slot) => *slot = routine.clone(),
        None => {
            if list.len() >= MAX_ROUTINES {
                return Err(AppError::msg("That is the most routines BYTE keeps."));
            }
            list.push(routine.clone());
        }
    }
    save(&path, &list)?;
    Ok(routine)
}

#[tauri::command]
pub fn routine_delete(app: AppHandle, id: String) -> AppResult<()> {
    let path = file(&app);
    let mut list = load(&path);
    list.retain(|r| r.id != id);
    save(&path, &list)?;
    Ok(())
}

/// Runs one now (the "Run" button). Returns the sentence BYTE would say.
#[tauri::command]
pub async fn routine_run(app: AppHandle, id: String) -> AppResult<String> {
    let r = load(&file(&app)).into_iter().find(|r| r.id == id).ok_or_else(|| AppError::msg("That routine is gone."))?;
    let app2 = app.clone();
    let r2 = r.clone();
    let report = tokio::task::spawn_blocking(move || run(&r2, &PcDoer { app: Some(&app2) })).await.map_err(|e| AppError::msg(e.to_string()))?;
    super::activity(&app, &format!("Routine \"{}\"", r.phrase), report.failed.is_none(), &report.failed.as_ref().map(|f| f.1.clone()).unwrap_or_default());
    Ok(report.say(&r))
}

/// "weekdays 07:00" from words ("every weekday at 7"), for the editor's schedule box.
#[tauri::command]
pub fn routine_schedule_parse(text: String) -> Option<(String, String)> {
    crate::scheduler::spec_in(&text).map(|s| (s.text(), s.describe()))
}

#[tauri::command]
pub fn routines_enabled_state(state: State<'_, AppState>) -> bool {
    state.settings.try_lock().map(|s| s.routines_enabled).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};
    use std::sync::Mutex;

    fn routine(phrase: &str, steps: Vec<RStep>) -> Routine {
        Routine { id: phrase.replace(' ', "-"), phrase: phrase.into(), steps, ..Default::default() }
    }

    fn game() -> Routine {
        routine("game time", vec![RStep::OpenApp { name: "steam".into() }, RStep::Volume { percent: 40 }, RStep::DoNotDisturb { on: true }])
    }

    #[test]
    fn the_phrase_runs_the_routine_with_or_without_polite_words() {
        let list = vec![game(), routine("movie night", vec![RStep::Volume { percent: 25 }])];
        for q in ["game time", "Hey BYTE, game time!", "please start my game time routine", "run game time", "hey byte can you run the movie night routine", "activate game time mode"] {
            assert!(match_phrase(q, &list).is_some(), "{q}");
        }
        assert_eq!(match_phrase("hey byte, movie night", &list).unwrap().phrase, "movie night");
    }

    #[test]
    fn other_sentences_do_not_run_it() {
        let list = vec![game()];
        for q in ["what is game time on the clock in basketball", "game time is at 8 tonight can you remind me", "open game", "time", "play a game"] {
            assert!(match_phrase(q, &list).is_none(), "{q}");
        }
        let mut off = game();
        off.enabled = false;
        assert!(match_phrase("game time", &[off]).is_none(), "a switched-off routine is not run");
    }

    #[test]
    fn the_longer_phrase_wins() {
        let list = vec![routine("work", vec![RStep::Volume { percent: 10 }]), routine("work mode focus", vec![RStep::Volume { percent: 5 }])];
        assert_eq!(match_phrase("work mode focus", &list).unwrap().phrase, "work mode focus");
        assert_eq!(match_phrase("work", &list).unwrap().phrase, "work");
    }

    #[test]
    fn steps_are_checked() {
        assert!(RStep::OpenApp { name: "spotify".into() }.check().is_ok());
        assert!(RStep::OpenApp { name: "cmd".into() }.check().is_err());
        assert!(RStep::OpenApp { name: "powershell.exe".into() }.check().is_err());
        assert!(RStep::OpenApp { name: "https://x.com".into() }.check().is_err());
        assert!(RStep::OpenApp { name: "".into() }.check().is_err());
        assert!(RStep::OpenUrl { url: "https://example.com/a".into() }.check().is_ok());
        assert!(RStep::OpenUrl { url: "file:///C:/x".into() }.check().is_err());
        assert!(RStep::OpenUrl { url: "javascript:alert(1)".into() }.check().is_err());
        assert!(RStep::OpenUrl { url: "ms-settings:sound".into() }.check().is_err(), "only web addresses");
        assert!(RStep::CloseApp { name: "Discord".into() }.check().is_ok());
        assert!(RStep::CloseApp { name: "explorer".into() }.check().is_err());
        assert!(RStep::CloseApp { name: "a; b".into() }.check().is_err());
        assert!(RStep::Volume { percent: 101 }.check().is_err());
        assert!(RStep::RunAutomation { name: " ".into() }.check().is_err());
    }

    #[test]
    fn a_routine_is_checked_as_a_whole() {
        let others = vec![game()];
        let mut r = routine("movie night", vec![RStep::Volume { percent: 20 }]);
        assert!(check(&r, &others).is_ok());
        r.phrase = "Game Time!".into();
        assert!(check(&r, &others).unwrap_err().contains("already"));
        r.phrase = "hey byte do it".into();
        assert!(check(&r, &others).is_err());
        r.phrase = "ok".into();
        assert!(check(&r, &others).is_err());
        r.phrase = "movie night".into();
        r.steps = vec![];
        assert!(check(&r, &others).unwrap_err().contains("at least one"));
        r.steps = vec![RStep::Volume { percent: 1 }; MAX_STEPS + 1];
        assert!(check(&r, &others).is_err());
        r.steps = vec![RStep::OpenApp { name: "cmd".into() }];
        assert!(check(&r, &others).unwrap_err().starts_with("Step 1"));
        r.steps = vec![RStep::Volume { percent: 1 }];
        r.schedule = "every blue moon".into();
        assert!(check(&r, &others).is_err());
        r.schedule = "weekdays 07:00".into();
        assert!(check(&r, &others).is_ok());
        // Editing itself is not a clash.
        let mut same = game();
        same.phrase = "game time".into();
        assert!(check(&same, &others).is_ok());
    }

    #[test]
    fn unknown_step_types_do_not_load() {
        assert!(serde_json::from_str::<RStep>("{\"type\":\"deleteFiles\",\"path\":\"C:/\"}").is_err());
        assert!(serde_json::from_str::<RStep>("{\"type\":\"openApp\",\"name\":\"x\",\"extra\":1}").is_err());
        let s: RStep = serde_json::from_str("{\"type\":\"doNotDisturb\",\"on\":true}").unwrap();
        assert_eq!(s, RStep::DoNotDisturb { on: true });
    }

    struct Fake(Mutex<Vec<String>>, Option<usize>);

    impl Doer for Fake {
        fn do_step(&self, s: &RStep) -> Result<(), String> {
            let mut l = self.0.lock().unwrap();
            if Some(l.len()) == self.1 {
                return Err("it would not start".into());
            }
            l.push(s.label());
            Ok(())
        }
    }

    #[test]
    fn a_run_does_the_steps_in_order_and_stops_at_a_failure() {
        let f = Fake(Mutex::default(), None);
        let rep = run(&game(), &f);
        assert_eq!(rep, RunReport { done: 3, failed: None });
        assert_eq!(f.0.lock().unwrap().as_slice(), ["Open steam", "Set the volume to 40%", "Turn notifications off (back on at the next start or after 12 hours)"]);
        assert_eq!(rep.say(&game()), "Game time is done.");
        let f = Fake(Mutex::default(), Some(1));
        let rep = run(&game(), &f);
        assert_eq!(rep.done, 1);
        assert_eq!(rep.say(&game()), "Game time stopped at step 2: it would not start");
    }

    #[test]
    fn a_step_that_is_not_valid_is_not_run_even_if_it_was_stored() {
        let f = Fake(Mutex::default(), None);
        let bad = routine("sneaky", vec![RStep::OpenApp { name: "cmd".into() }]);
        assert!(run(&bad, &f).failed.is_some());
        assert!(f.0.lock().unwrap().is_empty());
    }

    #[test]
    fn scheduled_routines_come_due_once() {
        let mut r = game();
        r.schedule = "weekdays 07:00".into();
        let other = routine("never", vec![RStep::Volume { percent: 1 }]);
        let list = vec![r, other];
        let t = |h, m| Utc.from_utc_datetime(&NaiveDate::from_ymd_opt(2026, 10, 14).unwrap().and_hms_opt(h, m, 0).unwrap());
        assert_eq!(due(&list, &t(6, 59), &t(7, 0)).len(), 1);
        assert_eq!(due(&list, &t(7, 0), &t(7, 1)).len(), 0, "not again");
        assert_eq!(due(&list, &t(6, 0), &t(6, 30)).len(), 0);
        // Saturday has no weekday run.
        let sat = |h| Utc.from_utc_datetime(&NaiveDate::from_ymd_opt(2026, 10, 17).unwrap().and_hms_opt(h, 0, 0).unwrap());
        assert_eq!(due(&list, &sat(6), &sat(8)).len(), 0);
    }

    #[test]
    fn notifications_that_bytes_turned_off_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let m = dir.path().join(NOTIFY_MARKER);
        assert!(!restore_notifications(&m, true, &|| Ok(())), "nothing was turned off");
        std::fs::write(&m, now_secs().to_string()).unwrap();
        assert!(!restore_notifications(&m, false, &|| Ok(())), "not yet");
        assert!(m.exists());
        assert!(!restore_notifications(&m, true, &|| Err("no".into())), "a failure keeps the note");
        assert!(m.exists());
        assert!(restore_notifications(&m, true, &|| Ok(())), "at the next start");
        assert!(!m.exists());
        std::fs::write(&m, (now_secs() - NOTIFY_OFF_MAX_SECS - 5).to_string()).unwrap();
        assert!(restore_notifications(&m, false, &|| Ok(())), "after 12 hours");
        assert!(notify_due(0, NOTIFY_OFF_MAX_SECS) && !notify_due(10, 20));
    }

    #[test]
    fn storage_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(FILE);
        assert!(load(&p).is_empty());
        save(&p, &[game()]).unwrap();
        assert_eq!(load(&p), vec![game()]);
        std::fs::write(&p, "garbage").unwrap();
        assert!(load(&p).is_empty());
    }
}
