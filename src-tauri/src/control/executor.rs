//! Runs a plan of steps: check, ask when risky, do, verify, retry once, re-plan when the screen is not what was expected,
//! stop at once when the stop flag is set, and write every step to the activity log. Everything it touches is behind a trait,
//! so the whole loop is tested with fakes (no Windows, no model, no real time).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::Serialize;

use super::actions::{self, Action, Expect, Risk, Step, Target, WaitFor};
// --- track:autonomy begin ---
use super::auto::contract::Verdict;
use super::auto::level::Level;
use super::auto::policy::{Gate, HookStop, Policy};
use super::auto::rules::{self, RuleKey};
// --- track:autonomy end ---
use super::input::Input;
use super::log::Log;
use super::uia::{self, Element, Screen};

/// How many times a plan may be replaced by a new one after something did not go as expected.
pub const MAX_REPLANS: u32 = 2;
const POLL_MS: u64 = 150;
const RETRY_PAUSE_MS: u64 = 300;
/// How long a result is waited for after a step that has an expectation.
const VERIFY_MS: u64 = 3_000;
/// A whole run may take no longer than this (wall clock), however many steps and waits it has, unless the person chose a longer limit.
pub const MAX_RUN_MS: u64 = 90_000;

/// Steps allowed in one plan for the setting's text: "12" (default), "25", "50", "100" or "unlimited".
pub fn step_limit(setting: &str) -> usize {
    match setting {
        "25" => 25,
        "50" => 50,
        "100" => 100,
        "unlimited" => usize::MAX,
        _ => super::actions::MAX_STEPS,
    }
}

/// Times a run may re-plan after a step fails, for the setting's text: "2" (default), "5", "10" or "unlimited".
pub fn replan_limit(setting: &str) -> u32 {
    match setting {
        "5" => 5,
        "10" => 10,
        "unlimited" => u32::MAX,
        _ => MAX_REPLANS,
    }
}

/// The wall-clock limit for the setting's text: "90s" (default), "3m", "10m", "30m", "1h" or "unlimited" (no time limit; the Stop button, the
/// step limit per plan and the approval cards still apply).
pub fn run_limit_ms(setting: &str) -> u64 {
    match setting {
        "3m" => 3 * 60_000,
        "10m" => 10 * 60_000,
        "30m" => 30 * 60_000,
        "1h" => 60 * 60_000,
        "unlimited" => u64::MAX,
        _ => MAX_RUN_MS,
    }
}

/// Starts programs and changes the power state (Windows: ShellExecute, shutdown).
pub trait Launcher: Send + Sync {
    fn open(&self, target: &Target) -> Result<(), String>;
    fn power(&self, kind: actions::PowerKind) -> Result<(), String>;
}

/// The approval card.
pub trait Approver: Send + Sync {
    fn approve<'a>(&'a self, title: &'a str, fields: Vec<(String, String)>) -> BoxFuture<'a, bool>;

    // --- track:autonomy begin ---
    /// As `approve`, for a step that has a saved-rule key: an approver that keeps "approve and don't ask again" rules looks the
    /// key up first (a matching rule answers without a card) and offers the second button. Others just ask.
    fn approve_step<'a>(&'a self, title: &'a str, fields: Vec<(String, String)>, _key: Option<RuleKey>) -> BoxFuture<'a, bool> {
        self.approve(title, fields)
    }
    // --- track:autonomy end ---
}

/// What the model is told when a plan did not work.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplanRequest {
    pub goal: String,
    /// The steps that already worked.
    pub done: Vec<String>,
    pub failed: String,
    pub reason: String,
    /// The front window as numbered marks (`uia::render`), empty when it can't be read.
    pub screen: String,
}

pub trait Replanner: Send + Sync {
    fn replan<'a>(&'a self, req: ReplanRequest) -> BoxFuture<'a, Result<Vec<Step>, String>>;
}

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
    fn sleep<'a>(&'a self, ms: u64) -> BoxFuture<'a, ()>;
}

pub struct RealClock(pub std::time::Instant);

impl Clock for RealClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
    fn sleep<'a>(&'a self, ms: u64) -> BoxFuture<'a, ()> {
        Box::pin(tokio::time::sleep(std::time::Duration::from_millis(ms)))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub goal: String,
    /// Program names (lower-case, no .exe) BYTE may operate without asking. Empty: ask once per program per run.
    pub allowed_apps: Vec<String>,
    /// Relaxes the card for button labels (Send, Buy...) and key combinations. Shutting down, closing windows and opening files
    /// by path always ask, whatever this says.
    pub confirm_risky: bool,
    /// Wall-clock cap for the whole run; `None` is `MAX_RUN_MS`.
    pub max_run_ms: Option<u64>,
    /// How many times a run may re-plan after a step fails; `None` is `MAX_REPLANS`.
    pub max_replans: Option<u32>,
    /// The person turned the approval cards off: nothing asks first (refused things, like running a program file, stay refused).
    pub never_ask: bool,
    // --- track:autonomy begin ---
    /// The autonomy level, the run contract and the live hooks of an autonomous job. `None`: a plain run.
    pub policy: Option<Policy>,
    // --- track:autonomy end ---
}

pub struct Env<'a> {
    /// Shared so slow Windows calls can run on a blocking thread, off the async workers.
    pub screen: Arc<dyn Screen>,
    pub input: Arc<dyn Input>,
    pub launcher: Arc<dyn Launcher>,
    pub approver: &'a dyn Approver,
    pub replanner: &'a dyn Replanner,
    pub log: &'a dyn Log,
    pub clock: &'a dyn Clock,
    pub stop: &'a AtomicBool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "why")]
pub enum Outcome {
    Done,
    Declined,
    Failed(String),
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepReport {
    pub what: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Every step done (after any re-plan).
    pub ok: bool,
    pub stopped: bool,
    pub declined: bool,
    pub replans: u32,
    pub steps: Vec<StepReport>,
    /// One sentence for the person.
    pub summary: String,
}

enum Fail {
    /// A step failed even after a retry: worth re-planning.
    Step(String),
    /// The person said no: the run ends, nothing is re-planned.
    Declined,
    Stopped,
    // --- track:autonomy begin ---
    /// The contract or the never-touch list forbids it: the plan ends, nothing is re-planned.
    Blocked(String),
    // --- track:autonomy end ---
}

struct Run<'a, 'b> {
    env: &'b Env<'a>,
    cfg: &'b Config,
    approved_apps: Vec<String>,
    /// The program the last step acted in (lower-case).
    last_app: String,
}

/// A card to show, with the key a "don't ask again" rule would have.
struct Card {
    title: String,
    fields: Vec<(String, String)>,
    key: Option<RuleKey>,
}

fn app_key(s: &str) -> String {
    let l = s.trim().to_lowercase();
    l.strip_suffix(".exe").unwrap_or(&l).to_string()
}

impl Run<'_, '_> {
    fn stopped(&self) -> bool {
        self.env.stop.load(Ordering::SeqCst)
    }

    /// Asks (once per program per run) before BYTE operates a program that is not in the allowed list. `Err` when the program is
    /// forbidden (the never-touch list).
    async fn app_ok(&mut self, app: &str) -> Result<bool, String> {
        let k = app_key(app);
        if k.is_empty() {
            return Ok(true);
        }
        self.last_app = k.clone();
        // --- track:autonomy begin ---
        let mut outside = None;
        if let Some(p) = &self.cfg.policy {
            if p.contract.is_some() {
                match p.app_verdict(&k) {
                    Verdict::Refused(w) => return Err(w),
                    Verdict::Inside => return Ok(true),
                    Verdict::Outside(w) => {
                        if p.level == Level::Never || self.approved_apps.contains(&k) {
                            return Ok(true);
                        }
                        outside = Some(w);
                    }
                }
            }
        }
        // --- track:autonomy end ---
        if outside.is_none() && (self.cfg.allowed_apps.iter().any(|a| app_key(a) == k) || self.approved_apps.contains(&k)) {
            return Ok(true);
        }
        let mut fields = vec![("Program".into(), k.clone()), ("Why".into(), self.cfg.goal.clone())];
        let (title, key) = match &outside {
            Some(w) => {
                fields.push(("Outside the run contract".into(), w.clone()));
                ("BYTE needs your OK: this program is outside the run contract", None)
            }
            None => ("Let BYTE use this program?", rules::key_for_app(&k)),
        };
        let ok = self.env.approver.approve_step(title, fields, key).await;
        if ok {
            self.approved_apps.push(k);
        }
        Ok(ok)
    }

    /// Runs a screen call on a blocking thread (UI Automation can take a while and must not hold an async worker).
    async fn on_screen<T: Send + 'static>(&self, f: impl FnOnce(&dyn Screen) -> T + Send + 'static) -> Option<T> {
        let s = self.env.screen.clone();
        tokio::task::spawn_blocking(move || f(&*s)).await.ok()
    }

    async fn on_input<T: Send + 'static>(&self, f: impl FnOnce(&dyn Input) -> T + Send + 'static) -> Option<T> {
        let i = self.env.input.clone();
        tokio::task::spawn_blocking(move || f(&*i)).await.ok()
    }

    async fn snap(&self) -> Result<uia::Snapshot, uia::ScreenError> {
        self.on_screen(|s| s.snapshot()).await.unwrap_or_else(|| Err(uia::ScreenError::Failed("the screen reader stopped".into())))
    }

    async fn poll(&self, timeout_ms: u64, c: Check) -> bool {
        let start = self.env.clock.now_ms();
        loop {
            let c2 = c.clone();
            if self.on_screen(move |s| eval(s, &c2)).await.unwrap_or(false) {
                return true;
            }
            if self.stopped() || self.env.clock.now_ms().saturating_sub(start) >= timeout_ms {
                return false;
            }
            self.env.clock.sleep(POLL_MS).await;
        }
    }

    /// Does the step need the person's yes first?
    fn needs_card(&self, step: &Step, el: Option<&Element>, gate: &Gate) -> Option<Card> {
        // --- track:autonomy begin ---
        let level = self.cfg.policy.as_ref().map(|p| p.level);
        let payment = matches!(step.action, Action::Click { .. }) && el.is_some_and(|e| rules::is_payment_label(&e.name));
        let mut fields = vec![("Step".to_string(), actions::describe(&step.action))];
        if let Some(e) = el {
            fields.push(("Item".into(), e.name.clone()));
        }
        if !self.cfg.goal.is_empty() {
            fields.push(("For".into(), self.cfg.goal.clone()));
        }
        if let (Gate::CheckIn(why), true) = (gate, level != Some(Level::Never)) {
            fields.push(("Outside the run contract".into(), why.clone()));
            return Some(Card { title: "BYTE needs your OK: this is outside the run contract".into(), fields, key: None });
        }
        let inside_contract = matches!(gate, Gate::Inside) && level == Some(Level::Contract) && !matches!(step.action, Action::Power { .. });
        // Payments always ask, whatever the level says.
        if !payment && (self.cfg.never_ask || level == Some(Level::Never) || inside_contract) {
            return None;
        }
        if !payment && !must_ask(&step.action, el, self.cfg.confirm_risky) {
            return None;
        }
        let key = rules::key_for(&step.action, el, &self.last_app).ok();
        Some(Card { title: "BYTE wants to do this on your PC".into(), fields, key })
        // --- track:autonomy end ---
    }

    async fn attempt(&self, step: &Step, el: Option<&Element>) -> Result<(), String> {
        let env = self.env;
        match &step.action {
            Action::Click { id } => {
                let Some(e) = el else { return Err(format!("item {id} is not in the window")) };
                let e = e.clone();
                self.on_screen(move |s| s.click(&e).map_err(|e| e.to_string())).await.unwrap_or_else(|| Err("the screen reader stopped".into()))
            }
            Action::Type { text } => {
                let text = text.clone();
                self.on_input(move |i| i.type_text(&text).map_err(|e| e.to_string())).await.unwrap_or_else(|| Err("typing stopped".into()))
            }
            Action::Key { combo } => {
                let c = actions::parse_combo(combo).map_err(|e| e.to_string())?;
                self.on_input(move |i| i.key_combo(&c).map_err(|e| e.to_string())).await.unwrap_or_else(|| Err("typing stopped".into()))
            }
            Action::Focus { title } => {
                let title = title.clone();
                self.on_screen(move |s| s.focus_window(&title).map(|_| ()).map_err(|e| e.to_string())).await.unwrap_or_else(|| Err("the screen reader stopped".into()))
            }
            Action::Open { target } => {
                let t = actions::classify_target(target).map_err(|e| e.to_string())?;
                let l = env.launcher.clone();
                tokio::task::spawn_blocking(move || l.open(&t)).await.unwrap_or_else(|_| Err("starting the program stopped".into()))
            }
            Action::Wait { what, text, .. } => {
                let ms = actions::wait_ms(&step.action);
                let found = match what {
                    WaitFor::Window => self.poll(ms, Check::WindowOpen(text.clone())).await,
                    WaitFor::Element => self.poll(ms, Check::Element(text.clone())).await,
                };
                if found {
                    Ok(())
                } else {
                    Err(format!("\"{text}\" did not show up in {} seconds", ms / 1000))
                }
            }
            Action::Scroll { direction, amount } => {
                let n = amount.unwrap_or(3) as i32;
                let n = if *direction == actions::Direction::Down { n } else { -n };
                self.on_input(move |i| i.scroll(n).map_err(|e| e.to_string())).await.unwrap_or_else(|| Err("scrolling stopped".into()))
            }
            Action::Close { title } => {
                let title = title.clone();
                self.on_screen(move |s| s.close_window(&title).map_err(|e| e.to_string())).await.unwrap_or_else(|| Err("the screen reader stopped".into()))
            }
            Action::Power { kind } => {
                let (l, k) = (env.launcher.clone(), *kind);
                tokio::task::spawn_blocking(move || l.power(k)).await.unwrap_or_else(|_| Err("the power change stopped".into()))
            }
        }
    }

    /// Whether what the step should have caused happened.
    async fn verify(&self, step: &Step) -> Result<(), String> {
        let mut checks: Vec<(String, bool)> = Vec::new();
        if let Action::Focus { title } = &step.action {
            let ok = self.poll(VERIFY_MS, Check::FrontTitle(title.clone())).await;
            checks.push((format!("\"{title}\" is not in front"), ok));
        }
        if let Some(w) = &step.expect.window {
            let ok = self.poll(VERIFY_MS, Check::FrontTitle(w.clone())).await;
            checks.push((format!("the front window is not \"{w}\""), ok));
        }
        if let Some(e) = &step.expect.element {
            let ok = self.poll(VERIFY_MS, Check::Element(e.clone())).await;
            checks.push((format!("there is no \"{e}\" in the front window"), ok));
        }
        match checks.into_iter().find(|(_, ok)| !ok) {
            Some((why, _)) => Err(why),
            None => Ok(()),
        }
    }

    async fn one(&mut self, step: &Step) -> Result<(), Fail> {
        if self.stopped() {
            return Err(Fail::Stopped);
        }
        // Programs: the allowed list. Opening names one; anything else acts on the one in front.
        match &step.action {
            Action::Open { target } => {
                if let Ok(Target::Program(p)) = actions::classify_target(target) {
                    match self.app_ok(&p).await {
                        Ok(true) => {}
                        Ok(false) => return Err(Fail::Declined),
                        Err(w) => return Err(Fail::Blocked(w)),
                    }
                }
            }
            Action::Wait { .. } | Action::Focus { .. } | Action::Power { .. } => {}
            _ => {
                if let Ok(s) = self.snap().await {
                    match self.app_ok(&s.app).await {
                        Ok(true) => {}
                        Ok(false) => return Err(Fail::Declined),
                        Err(w) => return Err(Fail::Blocked(w)),
                    }
                }
            }
        }
        // The element a click means, read now.
        let el = match &step.action {
            Action::Click { id } => {
                let shot = self.snap().await.map_err(|e| Fail::Step(e.to_string()))?;
                match shot.find(*id) {
                    Some(e) if e.sensitive => return Err(Fail::Step("that is a password or card field; BYTE does not touch those".into())),
                    Some(e) if e.enabled => Some(e.clone()),
                    Some(e) => return Err(Fail::Step(format!("item {id} (\"{}\") is disabled", e.name))),
                    None => return Err(Fail::Step(format!("item {id} is not in the window any more"))),
                }
            }
            _ => None,
        };
        // --- track:autonomy begin ---
        let approved = self.approved_apps.clone();
        let gate = match &self.cfg.policy {
            Some(p) => p.gate(&self.last_app, &step.action, el.as_ref(), &approved),
            None => Gate::Go,
        };
        if let Gate::Refuse(why) = &gate {
            return Err(Fail::Blocked(why.clone()));
        }
        // --- track:autonomy end ---
        if let Some(card) = self.needs_card(step, el.as_ref(), &gate) {
            if !self.env.approver.approve_step(&card.title, card.fields, card.key).await {
                return Err(Fail::Declined);
            }
        }
        // --- track:autonomy begin ---
        if let (Some(hooks), Action::Click { .. }) = (self.cfg.policy.as_ref().and_then(|p| p.hooks.clone()), &step.action) {
            if el.as_ref().is_some_and(|e| rules::click_class(&e.name) == Some("send")) {
                hooks.take_message().map_err(Fail::Blocked)?;
            }
        }
        // --- track:autonomy end ---
        let mut result = self.attempt(step, el.as_ref()).await;
        if result.is_err() && !matches!(step.action, Action::Wait { .. } | Action::Power { .. }) {
            self.env.clock.sleep(RETRY_PAUSE_MS).await;
            if self.stopped() {
                return Err(Fail::Stopped);
            }
            result = self.attempt(step, el.as_ref()).await;
        }
        result.map_err(Fail::Step)?;
        self.verify(step).await.map_err(Fail::Step)
    }
}

#[derive(Debug, Clone)]
enum Check {
    WindowOpen(String),
    Element(String),
    FrontTitle(String),
}

fn eval(screen: &dyn Screen, c: &Check) -> bool {
    match c {
        Check::WindowOpen(t) => {
            let t = t.to_lowercase();
            screen.snapshot().map(|s| s.title.to_lowercase().contains(&t)).unwrap_or(false) || screen.window_titles().iter().any(|w| w.to_lowercase().contains(&t))
        }
        Check::Element(t) => screen.snapshot().map(|s| s.find_named(t).is_some()).unwrap_or(false),
        Check::FrontTitle(t) => {
            let t = t.to_lowercase();
            screen.snapshot().map(|s| s.title.to_lowercase().contains(&t)).unwrap_or(false)
        }
    }
}

/// Whether a step needs the approval card. Shutting down, restarting, signing out, closing a window and opening a file by path
/// ALWAYS ask; `confirm_risky` (the "ask before risky steps" setting) only governs button labels and key combinations.
pub fn must_ask(action: &Action, el: Option<&Element>, confirm_risky: bool) -> bool {
    let always = match action {
        Action::Power { .. } | Action::Close { .. } => true,
        // A file by path, or a link that is not a web address (`steam://run/...` starts programs).
        Action::Open { .. } => actions::risk(action) == Risk::Asks,
        _ => false,
    };
    if always {
        return true;
    }
    let by_risk = actions::risk(action) == Risk::Asks;
    let by_label = matches!(action, Action::Click { .. }) && el.is_some_and(|e| actions::risky_label(&e.name));
    (by_risk || by_label) && confirm_risky
}

fn log_text(a: &Action) -> String {
    match a {
        // Typed text may be a password: only its length is written down.
        Action::Type { text } => format!("Type {} characters", text.chars().count()),
        other => actions::describe(other),
    }
}

/// Runs the plan. `steps` must already be parsed and validated (`actions::parse_plan`).
pub async fn run(env: &Env<'_>, cfg: &Config, steps: Vec<Step>) -> Report {
    let mut run = Run { env, cfg, approved_apps: Vec::new(), last_app: String::new() };
    // --- track:autonomy begin ---
    let hooks = cfg.policy.as_ref().and_then(|p| p.hooks.clone());
    // --- track:autonomy end ---
    let mut queue: VecDeque<Step> = steps.into();
    let mut reports: Vec<StepReport> = Vec::new();
    let mut replans = 0u32;
    let mut done: Vec<String> = Vec::new();
    let started = env.clock.now_ms();
    let cap = cfg.max_run_ms.unwrap_or(MAX_RUN_MS);
    let finish = |ok: bool, stopped: bool, declined: bool, replans: u32, steps: Vec<StepReport>, summary: String| Report { ok, stopped, declined, replans, steps, summary };

    while let Some(step) = queue.pop_front() {
        let what = log_text(&step.action);
        if env.clock.now_ms().saturating_sub(started) > cap {
            env.log.record(&what, false, "ran out of time");
            reports.push(StepReport { what, outcome: Outcome::Failed("ran out of time".into()) });
            return finish(false, false, false, replans, reports, format!("I stopped after {} seconds without finishing.", cap / 1000));
        }
        // --- track:autonomy begin ---
        if let Some(h) = &hooks {
            match h.before_step().await {
                Ok(()) => {}
                Err(HookStop::Stopped) => {
                    env.log.record(&what, false, "stopped");
                    reports.push(StepReport { what, outcome: Outcome::Stopped });
                    return finish(false, true, false, replans, reports, "Stopped.".into());
                }
                Err(HookStop::Budget(why)) => {
                    env.log.record(&what, false, &why);
                    reports.push(StepReport { what, outcome: Outcome::Failed(why.clone()) });
                    return finish(false, false, false, replans, reports, format!("I stopped: {why}."));
                }
            }
            h.step_started(&run.last_app, &what);
        }
        let outcome = run.one(&step).await;
        if let Some(h) = &hooks {
            let (ok, detail) = match &outcome {
                Ok(()) => (true, String::new()),
                Err(Fail::Stopped) => (false, "stopped".to_string()),
                Err(Fail::Declined) => (false, "you said no".to_string()),
                Err(Fail::Blocked(w)) | Err(Fail::Step(w)) => (false, w.clone()),
            };
            h.step_finished(&run.last_app, &what, ok, &detail);
        }
        // --- track:autonomy end ---
        match outcome {
            Ok(()) => {
                env.log.record(&what, true, "");
                reports.push(StepReport { what: what.clone(), outcome: Outcome::Done });
                done.push(what);
            }
            Err(Fail::Stopped) => {
                env.log.record(&what, false, "stopped");
                reports.push(StepReport { what, outcome: Outcome::Stopped });
                return finish(false, true, false, replans, reports, "Stopped.".into());
            }
            // --- track:autonomy begin ---
            Err(Fail::Blocked(why)) => {
                env.log.record(&what, false, &why);
                reports.push(StepReport { what, outcome: Outcome::Failed(why.clone()) });
                return finish(false, false, false, replans, reports, format!("I didn't do that: {why}."));
            }
            // --- track:autonomy end ---
            Err(Fail::Declined) => {
                env.log.record(&what, false, "you said no");
                reports.push(StepReport { what, outcome: Outcome::Declined });
                return finish(false, false, true, replans, reports, "Okay, I didn't do that.".into());
            }
            Err(Fail::Step(why)) => {
                env.log.record(&what, false, &why);
                reports.push(StepReport { what: what.clone(), outcome: Outcome::Failed(why.clone()) });
                if replans >= cfg.max_replans.unwrap_or(MAX_REPLANS) {
                    return finish(false, false, false, replans, reports, format!("I couldn't finish: {why}."));
                }
                let screen = run.snap().await.map(|s| uia::render(&s)).unwrap_or_default();
                let req = ReplanRequest { goal: cfg.goal.clone(), done: done.clone(), failed: what, reason: why.clone(), screen };
                match env.replanner.replan(req).await {
                    Ok(new) if !new.is_empty() => {
                        replans += 1;
                        env.log.record("Re-planned", true, &format!("{} new steps", new.len()));
                        queue = new.into();
                    }
                    Ok(_) => return finish(false, false, false, replans, reports, format!("I couldn't finish: {why}.")),
                    Err(e) => return finish(false, false, false, replans, reports, format!("I couldn't finish: {why} ({e}).")),
                }
            }
        }
    }
    let n = done.len();
    finish(true, false, false, replans, reports, if n == 1 { "Done.".into() } else { format!("Done, {n} steps.") })
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeLauncher {
        pub log: Mutex<Vec<String>>,
        pub fail: Mutex<u32>,
    }

    impl Launcher for FakeLauncher {
        fn open(&self, t: &Target) -> Result<(), String> {
            let mut f = self.fail.lock().unwrap();
            if *f > 0 {
                *f -= 1;
                return Err("could not start".into());
            }
            self.log.lock().unwrap().push(format!("open {t:?}"));
            Ok(())
        }
        fn power(&self, k: actions::PowerKind) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("power {k:?}"));
            Ok(())
        }
    }

    pub struct FakeApprover {
        pub answer: bool,
        pub asked: Mutex<Vec<String>>,
    }

    impl FakeApprover {
        pub fn new(answer: bool) -> Self {
            FakeApprover { answer, asked: Mutex::default() }
        }
    }

    impl Approver for FakeApprover {
        fn approve<'a>(&'a self, title: &'a str, fields: Vec<(String, String)>) -> BoxFuture<'a, bool> {
            self.asked.lock().unwrap().push(format!("{title} {}", fields.iter().map(|f| f.1.clone()).collect::<Vec<_>>().join("|")));
            Box::pin(async move { self.answer })
        }
    }

    pub struct FakeReplanner {
        pub plans: Mutex<VecDeque<Result<Vec<Step>, String>>>,
        pub requests: Mutex<Vec<ReplanRequest>>,
    }

    impl FakeReplanner {
        pub fn new(plans: Vec<Result<Vec<Step>, String>>) -> Self {
            FakeReplanner { plans: Mutex::new(plans.into()), requests: Mutex::default() }
        }
    }

    impl Replanner for FakeReplanner {
        fn replan<'a>(&'a self, req: ReplanRequest) -> BoxFuture<'a, Result<Vec<Step>, String>> {
            self.requests.lock().unwrap().push(req);
            let next = self.plans.lock().unwrap().pop_front().unwrap_or_else(|| Err("no more plans".into()));
            Box::pin(async move { next })
        }
    }

    #[derive(Default)]
    pub struct FakeClock(pub AtomicU64);

    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
        fn sleep<'a>(&'a self, ms: u64) -> BoxFuture<'a, ()> {
            self.0.fetch_add(ms, Ordering::SeqCst);
            Box::pin(async {})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use crate::control::input::fake::FakeInput;
    use crate::control::log::fake::MemLog;
    use crate::control::uia::fake::*;
    use crate::control::uia::Kind;

    fn step(json: &str) -> Step {
        actions::parse_plan(json).unwrap().remove(0)
    }

    struct Rig {
        screen: Arc<FakeScreen>,
        input: Arc<FakeInput>,
        launcher: Arc<FakeLauncher>,
        approver: FakeApprover,
        replanner: FakeReplanner,
        log: MemLog,
        clock: FakeClock,
        stop: AtomicBool,
    }

    impl Rig {
        fn new(shots: Vec<uia::Snapshot>) -> Self {
            Rig { screen: Arc::new(FakeScreen::new(shots)), input: Arc::new(FakeInput::default()), launcher: Arc::new(FakeLauncher::default()), approver: FakeApprover::new(true), replanner: FakeReplanner::new(vec![]), log: MemLog::default(), clock: FakeClock::default(), stop: AtomicBool::new(false) }
        }
        async fn go(&self, cfg: &Config, steps: Vec<Step>) -> Report {
            let env = Env { screen: self.screen.clone(), input: self.input.clone(), launcher: self.launcher.clone(), approver: &self.approver, replanner: &self.replanner, log: &self.log, clock: &self.clock, stop: &self.stop };
            run(&env, cfg, steps).await
        }
    }

    fn cfg() -> Config {
        Config { goal: "test".into(), allowed_apps: vec!["notepad".into()], confirm_risky: true, max_run_ms: None, max_replans: None, never_ask: false, policy: None }
    }

    fn plan(json: &str) -> Vec<Step> {
        actions::parse_plan(json).unwrap()
    }

    #[tokio::test]
    async fn a_plan_runs_in_order_and_is_logged() {
        let r = Rig::new(vec![snap("Untitled - Notepad", vec![el(1, "File", Kind::MenuItem), el(2, "Text editor", Kind::Edit)])]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"click\",\"id\":2},{\"action\":\"type\",\"text\":\"hello\"},{\"action\":\"key\",\"combo\":\"ctrl+s\"}]")).await;
        assert!(rep.ok, "{rep:?}");
        assert_eq!(rep.summary, "Done, 3 steps.");
        assert_eq!(r.screen.log.lock().unwrap().as_slice(), ["click 2"]);
        assert_eq!(r.input.log.lock().unwrap().len(), 2);
        let log = r.log.0.lock().unwrap();
        assert_eq!(log[1].0, "Type 5 characters", "typed text is not written down");
        assert!(log.iter().all(|l| l.1));
    }

    #[tokio::test]
    async fn a_risky_step_asks_and_no_means_nothing_happens() {
        let mut r = Rig::new(vec![snap("W", vec![])]);
        r.approver = FakeApprover::new(false);
        let rep = r.go(&cfg(), plan("[{\"action\":\"power\",\"kind\":\"shutdown\"}]")).await;
        assert!(rep.declined && !rep.ok);
        assert!(r.launcher.log.lock().unwrap().is_empty());
        assert_eq!(r.approver.asked.lock().unwrap().len(), 1);
        assert_eq!(rep.steps[0].outcome, Outcome::Declined);
    }

    #[tokio::test]
    async fn yes_lets_a_risky_step_run() {
        let r = Rig::new(vec![snap("W", vec![])]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"power\",\"kind\":\"restart\"}]")).await;
        assert!(rep.ok);
        assert_eq!(r.launcher.log.lock().unwrap().as_slice(), ["power Restart"]);
    }

    #[tokio::test]
    async fn turning_the_confirmation_off_relaxes_labels_and_keys_but_never_power_close_or_paths() {
        let c = Config { confirm_risky: false, ..cfg() };
        let r = Rig::new(vec![snap("Mail", vec![el(1, "Send", Kind::Button)])]);
        assert!(r.go(&c, plan("[{\"action\":\"click\",\"id\":1},{\"action\":\"key\",\"combo\":\"alt+f4\"}]")).await.ok);
        assert!(r.approver.asked.lock().unwrap().is_empty(), "labels and key combinations are relaxed");
        for p in [
            "[{\"action\":\"close\",\"title\":\"Notes\"}]",
            "[{\"action\":\"power\",\"kind\":\"shutdown\"}]",
            "[{\"action\":\"power\",\"kind\":\"restart\"}]",
            "[{\"action\":\"power\",\"kind\":\"sign_out\"}]",
            "[{\"action\":\"open\",\"target\":\"C:\\\\Docs\\\\a.pdf\"}]",
            "[{\"action\":\"open\",\"target\":\"steam://run/440\"}]",
        ] {
            let r = Rig::new(vec![snap("W", vec![])]);
            assert!(r.go(&c, plan(p)).await.ok, "{p}");
            assert_eq!(r.approver.asked.lock().unwrap().len(), 1, "{p} must ask");
        }
        assert!(actions::parse_plan("[{\"action\":\"open\",\"target\":\"cmd\"}]").is_err());
    }

    #[tokio::test]
    async fn never_ask_skips_every_card_but_program_files_stay_refused() {
        let c = Config { never_ask: true, ..cfg() };
        for p in [
            "[{\"action\":\"close\",\"title\":\"Notes\"}]",
            "[{\"action\":\"power\",\"kind\":\"shutdown\"}]",
            "[{\"action\":\"open\",\"target\":\"C:\\\\Docs\\\\a.pdf\"}]",
            "[{\"action\":\"open\",\"target\":\"steam://run/440\"}]",
        ] {
            let r = Rig::new(vec![snap("W", vec![])]);
            assert!(r.go(&c, plan(p)).await.ok, "{p}");
            assert!(r.approver.asked.lock().unwrap().is_empty(), "{p} must not ask when the cards are off");
        }
        // Reading the plan still refuses program files and scripts: the setting cannot reach that.
        assert!(actions::parse_plan("[{\"action\":\"open\",\"target\":\"C:\\\\x\\\\setup.exe\"}]").is_err());
        assert!(actions::parse_plan("[{\"action\":\"open\",\"target\":\"cmd\"}]").is_err());
    }

    #[test]
    fn the_step_and_replan_settings_map_to_limits() {
        assert_eq!(step_limit("12"), 12);
        assert_eq!(step_limit(""), 12);
        assert_eq!(step_limit("50"), 50);
        assert_eq!(step_limit("unlimited"), usize::MAX);
        assert_eq!(replan_limit("2"), 2);
        assert_eq!(replan_limit("junk"), 2);
        assert_eq!(replan_limit("10"), 10);
        assert_eq!(replan_limit("unlimited"), u32::MAX);
    }

    #[test]
    fn the_run_limit_setting_maps_to_milliseconds() {
        assert_eq!(run_limit_ms("90s"), 90_000);
        assert_eq!(run_limit_ms(""), 90_000);
        assert_eq!(run_limit_ms("junk"), 90_000);
        assert_eq!(run_limit_ms("3m"), 180_000);
        assert_eq!(run_limit_ms("30m"), 1_800_000);
        assert_eq!(run_limit_ms("1h"), 3_600_000);
        assert_eq!(run_limit_ms("unlimited"), u64::MAX);
    }

    #[test]
    fn must_ask_is_decided_by_step_kind() {
        let a = |j: &str| serde_json::from_str::<Action>(j).unwrap();
        assert!(must_ask(&a("{\"action\":\"power\",\"kind\":\"shutdown\"}"), None, false));
        assert!(must_ask(&a("{\"action\":\"close\",\"title\":\"x\"}"), None, false));
        assert!(!must_ask(&a("{\"action\":\"open\",\"target\":\"notepad\"}"), None, false));
        assert!(!must_ask(&a("{\"action\":\"open\",\"target\":\"https://example.com\"}"), None, true));
        assert!(!must_ask(&a("{\"action\":\"key\",\"combo\":\"alt+f4\"}"), None, false));
        assert!(must_ask(&a("{\"action\":\"key\",\"combo\":\"alt+f4\"}"), None, true));
    }

    #[tokio::test]
    async fn a_run_that_takes_too_long_stops() {
        let r = Rig::new(vec![snap("A", vec![]), snap("A", vec![]), snap("A", vec![]), snap("Open", vec![])]);
        let c = Config { max_run_ms: Some(300), ..cfg() };
        let rep = r.go(&c, plan("[{\"action\":\"wait\",\"for\":\"window\",\"text\":\"Open\",\"timeout_ms\":2000},{\"action\":\"key\",\"combo\":\"enter\"}]")).await;
        assert!(!rep.ok && rep.summary.contains("seconds"), "{rep:?}");
        assert!(r.input.log.lock().unwrap().is_empty(), "the second step never ran");
    }

    #[tokio::test]
    async fn clicking_a_send_button_asks_first() {
        let r = Rig::new(vec![snap("Mail", vec![el(1, "Send", Kind::Button), el(2, "Save draft", Kind::Button)])]);
        let c = Config { allowed_apps: vec!["notepad".into()], ..cfg() };
        assert!(r.go(&c, plan("[{\"action\":\"click\",\"id\":2}]")).await.ok);
        assert!(r.approver.asked.lock().unwrap().is_empty());
        assert!(r.go(&c, plan("[{\"action\":\"click\",\"id\":1}]")).await.ok);
        assert_eq!(r.approver.asked.lock().unwrap().len(), 1);
        assert!(r.approver.asked.lock().unwrap()[0].contains("Send"));
    }

    #[tokio::test]
    async fn a_program_not_in_the_allowed_list_is_asked_about_once_per_run() {
        let r = Rig::new(vec![snap("W", vec![el(1, "OK", Kind::Button)])]);
        let c = Config { allowed_apps: vec![], ..cfg() };
        let rep = r.go(&c, plan("[{\"action\":\"click\",\"id\":1},{\"action\":\"click\",\"id\":1},{\"action\":\"key\",\"combo\":\"enter\"}]")).await;
        assert!(rep.ok);
        assert_eq!(r.approver.asked.lock().unwrap().len(), 1, "once, for notepad");
    }

    #[tokio::test]
    async fn saying_no_to_a_program_ends_the_run() {
        let mut r = Rig::new(vec![snap("W", vec![el(1, "OK", Kind::Button)])]);
        r.approver = FakeApprover::new(false);
        let c = Config { allowed_apps: vec![], ..cfg() };
        let rep = r.go(&c, plan("[{\"action\":\"click\",\"id\":1}]")).await;
        assert!(rep.declined);
        assert!(r.screen.log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn opening_a_program_checks_the_allowed_list_too() {
        let r = Rig::new(vec![snap("W", vec![])]);
        let rep = r.go(&Config { allowed_apps: vec!["Spotify.exe".into()], ..cfg() }, plan("[{\"action\":\"open\",\"target\":\"spotify\"}]")).await;
        assert!(rep.ok && r.approver.asked.lock().unwrap().is_empty());
        let rep = r.go(&Config { allowed_apps: vec![], ..cfg() }, plan("[{\"action\":\"open\",\"target\":\"calc\"}]")).await;
        assert!(rep.ok);
        assert_eq!(r.approver.asked.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_failed_attempt_is_retried_once() {
        let r = Rig::new(vec![snap("W", vec![el(1, "OK", Kind::Button)])]);
        *r.screen.fail_click.lock().unwrap() = 1;
        let rep = r.go(&cfg(), plan("[{\"action\":\"click\",\"id\":1}]")).await;
        assert!(rep.ok, "{rep:?}");
        assert_eq!(r.screen.log.lock().unwrap().as_slice(), ["click 1"]);
        assert!(r.replanner.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_missing_element_replans_with_the_screen_and_what_was_done() {
        let mut r = Rig::new(vec![snap("Dialog", vec![el(1, "OK", Kind::Button)])]);
        r.replanner = FakeReplanner::new(vec![Ok(plan("[{\"action\":\"click\",\"id\":1}]"))]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"key\",\"combo\":\"ctrl+n\"},{\"action\":\"click\",\"id\":9}]")).await;
        assert!(rep.ok, "{rep:?}");
        assert_eq!(rep.replans, 1);
        let req = r.replanner.requests.lock().unwrap()[0].clone();
        assert_eq!(req.done, ["Press ctrl+n"]);
        assert_eq!(req.failed, "Click item 9");
        assert!(req.screen.contains("1 button \"OK\""), "{}", req.screen);
        assert!(req.reason.contains("not in the window"));
        assert_eq!(r.screen.log.lock().unwrap().as_slice(), ["click 1"]);
    }

    #[tokio::test]
    async fn replanning_gives_up_after_the_limit() {
        let mut r = Rig::new(vec![snap("W", vec![])]);
        let bad = || Ok(plan("[{\"action\":\"click\",\"id\":9}]"));
        r.replanner = FakeReplanner::new(vec![bad(), bad(), bad()]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"click\",\"id\":9}]")).await;
        assert!(!rep.ok);
        assert_eq!(rep.replans, MAX_REPLANS);
        assert_eq!(r.replanner.requests.lock().unwrap().len() as u32, MAX_REPLANS);
        assert!(rep.summary.starts_with("I couldn't finish"));
    }

    #[tokio::test]
    async fn a_failing_replanner_ends_the_run_with_a_reason() {
        let mut r = Rig::new(vec![snap("W", vec![])]);
        r.replanner = FakeReplanner::new(vec![Err("the model is off".into())]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"click\",\"id\":3}]")).await;
        assert!(!rep.ok && rep.summary.contains("the model is off"));
    }

    #[tokio::test]
    async fn an_expectation_that_does_not_come_true_is_a_failure_not_a_pass() {
        let mut r = Rig::new(vec![snap("Untitled - Notepad", vec![])]);
        r.replanner = FakeReplanner::new(vec![Err("stop".into())]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"key\",\"combo\":\"ctrl+s\",\"expect\":{\"window\":\"Save As\"}}]")).await;
        assert!(!rep.ok);
        assert!(r.replanner.requests.lock().unwrap()[0].reason.contains("Save As"));
        // The key was pressed only once: a failed expectation is not retried (it could toggle something twice).
        assert_eq!(r.input.log.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_expectation_that_comes_true_after_a_while_passes() {
        let r = Rig::new(vec![snap("Untitled - Notepad", vec![]), snap("Untitled - Notepad", vec![]), snap("Save As", vec![el(1, "Save", Kind::Button)])]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"key\",\"combo\":\"ctrl+s\",\"expect\":{\"window\":\"Save As\"}}]")).await;
        assert!(rep.ok, "{rep:?}");
    }

    #[tokio::test]
    async fn focus_is_verified() {
        let mut r = Rig::new(vec![snap("Something else", vec![])]);
        r.replanner = FakeReplanner::new(vec![Err("x".into())]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"focus\",\"title\":\"Calculator\"}]")).await;
        assert!(!rep.ok);
        let r2 = Rig::new(vec![snap("Calculator", vec![])]);
        assert!(r2.go(&cfg(), plan("[{\"action\":\"focus\",\"title\":\"Calculator\"}]")).await.ok);
    }

    #[tokio::test]
    async fn wait_succeeds_when_the_window_appears_and_times_out_when_not() {
        let r = Rig::new(vec![snap("A", vec![]), snap("A", vec![]), snap("Open", vec![])]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"wait\",\"for\":\"window\",\"text\":\"Open\",\"timeout_ms\":2000}]")).await;
        assert!(rep.ok, "{rep:?}");
        let mut r = Rig::new(vec![snap("A", vec![])]);
        r.replanner = FakeReplanner::new(vec![Err("x".into())]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"wait\",\"for\":\"element\",\"text\":\"Save\",\"timeout_ms\":1000}]")).await;
        assert!(!rep.ok);
        assert!(r.clock.now_ms() >= 1000 && r.clock.now_ms() < 3000, "{}", r.clock.now_ms());
    }

    #[tokio::test]
    async fn the_stop_flag_ends_the_run_before_the_next_step() {
        let r = Rig::new(vec![snap("W", vec![el(1, "OK", Kind::Button)])]);
        r.stop.store(true, Ordering::SeqCst);
        let rep = r.go(&cfg(), plan("[{\"action\":\"click\",\"id\":1},{\"action\":\"click\",\"id\":1}]")).await;
        assert!(rep.stopped && !rep.ok);
        assert!(r.screen.log.lock().unwrap().is_empty());
        assert_eq!(rep.steps.len(), 1);
    }

    #[tokio::test]
    async fn a_password_field_is_never_clicked() {
        let mut r = Rig::new(vec![snap("W", vec![uia::Element { sensitive: true, ..el(1, "Password", Kind::Edit) }])]);
        r.replanner = FakeReplanner::new(vec![Err("x".into())]);
        let rep = r.go(&cfg(), plan("[{\"action\":\"click\",\"id\":1}]")).await;
        assert!(!rep.ok);
        assert!(r.replanner.requests.lock().unwrap()[0].reason.contains("password"));
        assert!(r.screen.log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_disabled_item_is_a_failure_with_a_clear_reason() {
        let mut r = Rig::new(vec![snap("W", vec![uia::Element { enabled: false, ..el(1, "Save", Kind::Button) }])]);
        r.replanner = FakeReplanner::new(vec![Err("x".into())]);
        r.go(&cfg(), plan("[{\"action\":\"click\",\"id\":1}]")).await;
        assert!(r.replanner.requests.lock().unwrap()[0].reason.contains("disabled"));
    }

    #[tokio::test]
    async fn open_failures_are_retried_then_replanned() {
        let mut r = Rig::new(vec![snap("W", vec![])]);
        *r.launcher.fail.lock().unwrap() = 1;
        assert!(r.go(&cfg(), plan("[{\"action\":\"open\",\"target\":\"notepad\"}]")).await.ok);
        *r.launcher.fail.lock().unwrap() = 2;
        r.replanner = FakeReplanner::new(vec![Err("x".into())]);
        assert!(!r.go(&cfg(), plan("[{\"action\":\"open\",\"target\":\"notepad\"}]")).await.ok);
    }

    #[tokio::test]
    async fn scroll_goes_the_right_way() {
        let r = Rig::new(vec![snap("W", vec![])]);
        r.go(&cfg(), plan("[{\"action\":\"scroll\",\"direction\":\"down\",\"amount\":5},{\"action\":\"scroll\",\"direction\":\"up\"}]")).await;
        assert_eq!(r.input.log.lock().unwrap().as_slice(), ["scroll 5", "scroll -3"]);
    }

    #[test]
    fn step_helper_parses() {
        assert!(matches!(step("[{\"action\":\"click\",\"id\":1}]").action, Action::Click { id: 1 }));
        let _ = Expect::default();
    }

    // --- track:autonomy begin ---
    use crate::control::auto::contract::Contract;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Rec {
        seen: Mutex<Vec<String>>,
        steps_left: Mutex<u32>,
        stop: bool,
    }

    impl crate::control::auto::policy::Hooks for Rec {
        fn before_step<'a>(&'a self) -> BoxFuture<'a, Result<(), HookStop>> {
            Box::pin(async move {
                if self.stop {
                    return Err(HookStop::Stopped);
                }
                let mut n = self.steps_left.lock().unwrap();
                if *n == 0 {
                    return Err(HookStop::Budget("the step budget (2) is used up".into()));
                }
                *n -= 1;
                Ok(())
            })
        }
        fn step_started(&self, app: &str, what: &str) {
            self.seen.lock().unwrap().push(format!("start {app} {what}"));
        }
        fn step_finished(&self, app: &str, what: &str, ok: bool, detail: &str) {
            self.seen.lock().unwrap().push(format!("end {app} {what} {ok} {detail}"));
        }
        fn take_message(&self) -> Result<(), String> {
            Ok(())
        }
    }

    fn job_cfg(level: Level, apps: &[&str], hooks: Option<Arc<Rec>>) -> Config {
        let c = Contract { apps: apps.iter().map(|a| a.to_string()).collect(), ..Default::default() }.normalized();
        let hooks: Option<Arc<dyn crate::control::auto::policy::Hooks>> = hooks.map(|h| h as Arc<dyn crate::control::auto::policy::Hooks>);
        Config { allowed_apps: vec![], policy: Some(Policy { level, contract: Some(Arc::new(c)), never_touch: vec!["bank".into()], hooks }), ..cfg() }
    }

    #[tokio::test]
    async fn a_payment_button_asks_even_when_the_cards_are_off() {
        let r = Rig::new(vec![snap("Shop", vec![el(1, "Buy now", Kind::Button)])]);
        let c = Config { never_ask: true, ..cfg() };
        assert!(r.go(&c, plan("[{\"action\":\"click\",\"id\":1}]")).await.ok);
        assert_eq!(r.approver.asked.lock().unwrap().len(), 1, "a payment is never skipped");
    }

    #[tokio::test]
    async fn inside_the_contract_the_contract_level_asks_nothing_but_the_ask_level_does() {
        let steps = "[{\"action\":\"close\",\"title\":\"Notes\"}]";
        let r = Rig::new(vec![snap("W", vec![el(1, "x", Kind::Button)])]);
        assert!(r.go(&job_cfg(Level::Contract, &["notepad"], None), plan("[{\"action\":\"key\",\"combo\":\"alt+f4\"}]")).await.ok);
        assert!(r.approver.asked.lock().unwrap().is_empty());
        let r = Rig::new(vec![snap("W", vec![el(1, "x", Kind::Button)])]);
        assert!(r.go(&job_cfg(Level::Ask, &["notepad"], None), plan("[{\"action\":\"key\",\"combo\":\"alt+f4\"}]")).await.ok);
        assert_eq!(r.approver.asked.lock().unwrap().len(), 1);
        let _ = steps;
    }

    #[tokio::test]
    async fn a_program_outside_the_contract_waits_for_an_answer_and_no_ends_the_run() {
        let mut r = Rig::new(vec![snap("W", vec![el(1, "x", Kind::Button)])]);
        r.approver = FakeApprover::new(false);
        let rep = r.go(&job_cfg(Level::Contract, &["excel"], None), plan("[{\"action\":\"click\",\"id\":1}]")).await;
        assert!(rep.declined && r.screen.log.lock().unwrap().is_empty());
        assert!(r.approver.asked.lock().unwrap()[0].contains("outside the run contract"));
    }

    #[tokio::test]
    async fn the_never_touch_list_and_never_delete_stop_the_plan_without_a_question() {
        let mut shot = snap("Bank", vec![el(1, "Save", Kind::Button)]);
        shot.app = "bank".into();
        let r = Rig::new(vec![shot]);
        let rep = r.go(&job_cfg(Level::Never, &["bank"], None), plan("[{\"action\":\"click\",\"id\":1}]")).await;
        assert!(!rep.ok && rep.summary.contains("never-touch"), "{rep:?}");
        let r = Rig::new(vec![snap("W", vec![el(1, "Delete all", Kind::Button)])]);
        let rep = r.go(&job_cfg(Level::Never, &["notepad"], None), plan("[{\"action\":\"click\",\"id\":1}]")).await;
        assert!(rep.summary.contains("never delete") && r.screen.log.lock().unwrap().is_empty() && r.approver.asked.lock().unwrap().is_empty(), "{rep:?}");
    }

    #[tokio::test]
    async fn hooks_see_every_step_and_a_spent_budget_or_stop_ends_the_run() {
        let rec = Arc::new(Rec { steps_left: Mutex::new(2), ..Default::default() });
        let r = Rig::new(vec![snap("W", vec![el(1, "x", Kind::Button)])]);
        let rep = r.go(&job_cfg(Level::Contract, &["notepad"], Some(rec.clone())), plan("[{\"action\":\"scroll\",\"direction\":\"down\"},{\"action\":\"scroll\",\"direction\":\"down\"},{\"action\":\"scroll\",\"direction\":\"down\"}]")).await;
        assert!(!rep.ok && rep.summary.contains("step budget"), "{rep:?}");
        assert_eq!(rec.seen.lock().unwrap().iter().filter(|l| l.starts_with("end")).count(), 2);
        let rec = Arc::new(Rec { stop: true, ..Default::default() });
        let r = Rig::new(vec![snap("W", vec![])]);
        let rep = r.go(&job_cfg(Level::Contract, &["notepad"], Some(rec)), plan("[{\"action\":\"scroll\",\"direction\":\"down\"}]")).await;
        assert!(rep.stopped);
    }
    // --- track:autonomy end ---
}
