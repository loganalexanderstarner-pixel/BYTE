//! PC control by voice (Windows): BYTE reads the front window as numbered items (UI Automation, not pixels), a model plans a few
//! steps as strict JSON, and a verifying executor does them, asking first when a step is risky.
//!
//! - `actions`: the typed step list, strict parser, risk classes.   - `uia`: the window as numbered marks.
//! - `input`: keys and mouse.   - `executor`: the plan/verify/re-plan loop.   - `brain`: which model plans, where it runs.
//! - `log`: the activity log.   - `routines`, `find`, `status`, `sysctl`, `phrases`: what is said out loud and routed here.
//!
//! The lead's agent loop calls `run_plan`, `describe_foreground` and `tool_specs` / `run_tool` (all behind the `pcControl`
//! setting). Everything that touches Windows sits behind a trait, so the logic runs under test on any system.

pub mod actions;
pub mod brain;
pub mod com;
pub mod executor;
pub mod find;
pub mod guide;
pub mod input;
pub mod log;
pub mod phrases;
pub mod routines;
pub mod status;
pub mod sysctl;
pub mod uia;

pub use phrases::Flags;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use futures_util::future::BoxFuture;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::agent::Emit;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use actions::{PowerKind, Step, Target};
use log::Log as _;
use executor::{Approver, Launcher, ReplanRequest, Replanner, Report};

pub const STATE_EVENT: &str = "control://state";
pub const TOOL: &str = "pc_control";
pub const TOOLS: &[&str] = &[TOOL];

static STOP: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static BRAIN: OnceLock<brain::ControlBrain> = OnceLock::new();
static LOG: OnceLock<log::FileLog> = OnceLock::new();

/// Stops a running plan before its next step.
pub fn request_stop() {
    STOP.store(true, Ordering::SeqCst);
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

fn brain(app: &AppHandle) -> &'static brain::ControlBrain {
    BRAIN.get_or_init(|| brain::ControlBrain::new(app.state::<AppState>().paths.root.join("control.pid")))
}

fn activity_log(app: &AppHandle) -> &'static log::FileLog {
    LOG.get_or_init(|| log::FileLog::new(app.state::<AppState>().paths.data.join("control_log.jsonl")))
}

/// At launch: clears a control model left running by a crash.
pub fn start(app: &AppHandle) {
    brain(app).reap_stale();
    routines::start(app.clone());
}

pub fn kill_now(app: &AppHandle) {
    brain(app).kill_now();
}

/// Settings read without waiting (a busy settings lock means "not now").
pub fn settings_now(app: &AppHandle) -> Option<crate::settings::Settings> {
    app.state::<AppState>().settings.try_lock().ok().map(|s| s.clone())
}

pub fn enabled(app: &AppHandle) -> bool {
    settings_now(app).is_some_and(|s| s.pc_control)
}

// ------------------------------------------------------------------ the real PC

#[cfg(windows)]
pub fn real_input() -> Box<dyn input::Input> {
    Box::new(input::WinInput)
}

#[cfg(windows)]
pub fn real_screen() -> Box<dyn uia::Screen> {
    Box::new(uia::WinScreen::new(real_input()))
}

#[cfg(not(windows))]
pub fn real_input() -> Box<dyn input::Input> {
    Box::new(NoPc)
}

#[cfg(not(windows))]
pub fn real_screen() -> Box<dyn uia::Screen> {
    Box::new(NoPc)
}

/// Stands in on systems BYTE cannot control.
#[cfg(not(windows))]
struct NoPc;

#[cfg(not(windows))]
impl input::Input for NoPc {
    fn key_combo(&self, _: &actions::Combo) -> Result<(), input::InputError> {
        Err(input::InputError("PC control needs Windows".into()))
    }
    fn type_text(&self, _: &str) -> Result<(), input::InputError> {
        Err(input::InputError("PC control needs Windows".into()))
    }
    fn click_at(&self, _: i32, _: i32) -> Result<(), input::InputError> {
        Err(input::InputError("PC control needs Windows".into()))
    }
    fn scroll(&self, _: i32) -> Result<(), input::InputError> {
        Err(input::InputError("PC control needs Windows".into()))
    }
}

#[cfg(not(windows))]
impl uia::Screen for NoPc {
    fn snapshot(&self) -> Result<uia::Snapshot, uia::ScreenError> {
        Err(uia::ScreenError::Unsupported)
    }
    fn click(&self, _: &uia::Element) -> Result<(), uia::ScreenError> {
        Err(uia::ScreenError::Unsupported)
    }
    fn focus_window(&self, _: &str) -> Result<String, uia::ScreenError> {
        Err(uia::ScreenError::Unsupported)
    }
    fn window_titles(&self) -> Vec<String> {
        vec![]
    }
    fn close_window(&self, _: &str) -> Result<(), uia::ScreenError> {
        Err(uia::ScreenError::Unsupported)
    }
}

/// Starts programs and changes the power state through the PC's own fixed operations (no shell).
pub struct PcLauncher;

impl Launcher for PcLauncher {
    fn open(&self, target: &Target) -> Result<(), String> {
        let text = match target {
            Target::Program(s) | Target::Uri(s) | Target::Path(s) => s.clone(),
        };
        crate::pcctl::run_op(&crate::pcctl::WinOp::Open(text)).map(|_| ()).map_err(|e| e.text_for("that program", true))
    }

    fn power(&self, kind: PowerKind) -> Result<(), String> {
        sysctl::power(kind)
    }
}

// ------------------------------------------------------------------ reading and planning

/// The front window as numbered items, for a model to read. A short sentence when it can't be read.
pub fn describe_foreground() -> String {
    match real_screen().snapshot() {
        Ok(s) => uia::render(&s),
        Err(e) => format!("(the front window can't be read: {e})"),
    }
}

async fn describe_foreground_async() -> String {
    tokio::task::spawn_blocking(describe_foreground).await.unwrap_or_default()
}

/// One reply from the control model: its own engine when one is chosen and downloaded, else the chat model. While a game runs
/// and `controlGameRam` is on, its own engine runs on the processor only.
pub async fn ask_model(app: &AppHandle, messages: &[Value]) -> AppResult<String> {
    let state = app.state::<AppState>();
    let (setting, game_ram) = {
        let s = state.settings.lock().await;
        (s.control_model.clone(), s.control_game_ram)
    };
    let catalog = state.catalog.get();
    let (source, why) = brain::choose_source(&setting, &catalog, &state.paths.models);
    if let Some(w) = why {
        activity_log(app).record("Control model", false, &w);
    }
    let b = brain(app);
    match source {
        brain::Source::Own(key) => {
            let game = if game_ram { tokio::task::spawn_blocking(|| crate::busy::read().busy).await.unwrap_or(false) } else { false };
            let cpu_only = brain::placement_for(game_ram, game) == crate::system::Placement::CpuOnly;
            b.complete(app, &state.paths.models, &catalog, &key, cpu_only, messages).await
        }
        brain::Source::Chat => {
            let ep = state.engine.endpoint().await.ok_or_else(|| AppError::msg("no model is running to plan with (start one in Models, or pick a control model in Settings)"))?;
            b.complete_at(&ep, messages).await
        }
    }
}

/// Asks the model for a plan and reads it; one correction round when the reply is not a valid plan.
pub async fn plan_with_model(app: &AppHandle, user: String) -> Result<Vec<Step>, String> {
    let mut messages = vec![json!({"role":"system","content":brain::SYSTEM}), json!({"role":"user","content":user})];
    let mut last = String::new();
    for _ in 0..2 {
        let reply = ask_model(app, &messages).await.map_err(|e| e.to_string())?;
        let limit = settings_now(app).map(|s| executor::step_limit(&s.control_max_steps)).unwrap_or(actions::MAX_STEPS);
        match actions::parse_plan_with(&reply, limit) {
            Ok(p) => return Ok(p),
            Err(e) => {
                last = e.to_string();
                messages.push(json!({"role":"assistant","content":reply}));
                messages.push(json!({"role":"user","content":brain::fix_prompt(&last)}));
            }
        }
    }
    Err(format!("the control model did not give a usable plan ({last})"))
}

struct ModelReplanner<'a> {
    app: &'a AppHandle,
}

impl Replanner for ModelReplanner<'_> {
    fn replan<'a>(&'a self, req: ReplanRequest) -> BoxFuture<'a, Result<Vec<Step>, String>> {
        Box::pin(async move { plan_with_model(self.app, brain::replan_prompt(&req)).await })
    }
}

/// The approval card, through the same card the Mac and PC actions use.
struct CardApprover<'a> {
    cancel: &'a CancellationToken,
    send: Emit<'a>,
}

impl Approver for CardApprover<'_> {
    fn approve<'a>(&'a self, title: &'a str, fields: Vec<(String, String)>) -> BoxFuture<'a, bool> {
        Box::pin(async move { crate::macctl::ask_ok(title, "PC control", fields, self.cancel, self.send).await.unwrap_or(false) })
    }
}

/// What a run needs from its caller.
pub struct PlanCtx<'a> {
    /// What the person asked for, in their words (shown on cards, given to a re-plan).
    pub goal: String,
    pub cancel: &'a CancellationToken,
    pub send: Emit<'a>,
}

/// One line in the activity log.
pub fn activity(app: &AppHandle, what: &str, ok: bool, detail: &str) {
    use log::Log;
    activity_log(app).record(what, ok, detail);
}

/// Runs steps the phrase rules built (no model): the same executor, so asking, verifying and logging apply.
pub async fn run_plan_steps(app: &AppHandle, steps: Vec<Step>, goal: &str, cancel: &CancellationToken, send: Emit<'_>) -> Report {
    let Some(settings) = settings_now(app) else {
        return Report { ok: false, stopped: false, declined: false, replans: 0, steps: vec![], summary: "Settings are busy, try again.".into() };
    };
    run_steps(app, &settings, steps, &PlanCtx { goal: goal.to_string(), cancel, send }).await
}

fn set_active(app: &AppHandle, on: bool, goal: &str) {
    ACTIVE.store(on, Ordering::SeqCst);
    let _ = app.emit(STATE_EVENT, json!({ "active": on, "goal": goal }));
}

/// Runs a plan of steps (the JSON a model wrote). Errors only for things that stop it before any step: the setting is off, or
/// the plan is not valid. A plan that runs and fails comes back as a `Report` with `ok: false`.
pub async fn run_plan(app: &AppHandle, plan_json: &str, ctx: &PlanCtx<'_>) -> Result<Report, String> {
    let settings = settings_now(app).ok_or("settings are busy, try again")?;
    if !settings.pc_control {
        return Err("PC control is off (Settings → PC control)".into());
    }
    let steps = actions::parse_plan_with(plan_json, executor::step_limit(&settings.control_max_steps)).map_err(|e| e.to_string())?;
    Ok(run_steps(app, &settings, steps, ctx).await)
}

async fn run_steps(app: &AppHandle, settings: &crate::settings::Settings, steps: Vec<Step>, ctx: &PlanCtx<'_>) -> Report {
    STOP.store(false, Ordering::SeqCst);
    set_active(app, true, &ctx.goal);
    let watcher = {
        let cancel = ctx.cancel.clone();
        tokio::spawn(async move {
            cancel.cancelled().await;
            request_stop();
        })
    };
    let screen: std::sync::Arc<dyn uia::Screen> = real_screen().into();
    let input: std::sync::Arc<dyn input::Input> = real_input().into();
    let launcher: std::sync::Arc<dyn Launcher> = std::sync::Arc::new(PcLauncher);
    let approver = CardApprover { cancel: ctx.cancel, send: ctx.send };
    let replanner = ModelReplanner { app };
    let clock = executor::RealClock(std::time::Instant::now());
    let env = executor::Env { screen, input, launcher, approver: &approver, replanner: &replanner, log: activity_log(app), clock: &clock, stop: &STOP };
    let cfg = executor::Config { goal: ctx.goal.clone(), allowed_apps: settings.control_allowed_apps.clone(), confirm_risky: settings.control_confirm_risky, max_run_ms: Some(executor::run_limit_ms(&settings.control_run_limit)), max_replans: Some(executor::replan_limit(&settings.control_max_replans)), never_ask: settings.control_never_ask };
    let report = executor::run(&env, &cfg, steps).await;
    watcher.abort();
    set_active(app, false, "");
    report
}

/// A report as text for the model that asked.
pub fn report_text(r: &Report) -> String {
    let mut out = format!("{}\n", r.summary);
    for s in &r.steps {
        let mark = match &s.outcome {
            executor::Outcome::Done => "done".to_string(),
            executor::Outcome::Declined => "declined by the user".into(),
            executor::Outcome::Stopped => "stopped".into(),
            executor::Outcome::Failed(w) => format!("failed: {w}"),
        };
        out.push_str(&format!("- {}: {mark}\n", s.what));
    }
    out
}

// ------------------------------------------------------------------ the agent tool

/// The tool the agent loop offers when `pcControl` is on.
pub fn tool_specs() -> Vec<Value> {
    vec![json!({
        "type": "function",
        "function": {
            "name": TOOL,
            "description": "Operate this Windows PC for the user: open or switch programs, click buttons, type, press keys, close windows. Give the goal in plain words; BYTE reads the front window, plans the steps, asks the user before anything risky, and reports what happened. Do not use it for questions or for web pages (use the browser tools).",
            "parameters": {
                "type": "object",
                "properties": { "goal": { "type": "string", "description": "What the user wants done on the PC, in one sentence." } },
                "required": ["goal"]
            }
        }
    })]
}

/// Runs the tool: plans from what is in front, runs, and reports. `(ok, summary for the card, text for the model)`.
pub async fn run_tool(app: &AppHandle, args: &Value, ctx_cancel: &CancellationToken, send: Emit<'_>) -> (bool, String, String) {
    let goal = args.get("goal").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if goal.is_empty() {
        return (false, "No goal given".into(), "The goal was empty.".into());
    }
    let Some(settings) = settings_now(app) else { return (false, "Settings busy".into(), "Try again in a moment.".into()) };
    if !settings.pc_control {
        return (false, "PC control is off".into(), "PC control is off. Tell the user to turn it on in Settings → PC control.".into());
    }
    let screen = describe_foreground_async().await;
    let steps = match plan_with_model(app, brain::plan_prompt(&goal, &screen)).await {
        Ok(s) => s,
        Err(e) => return (false, "Couldn't plan".into(), format!("BYTE could not plan this: {e}")),
    };
    let ctx = PlanCtx { goal: goal.clone(), cancel: ctx_cancel, send };
    let report = run_steps(app, &settings, steps, &ctx).await;
    (report.ok, report.summary.clone(), report_text(&report))
}

// ------------------------------------------------------------------ commands for Settings

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlStatus {
    pub supported: bool,
    pub active: bool,
    /// The control model in words: its name, or "the chat model".
    pub model_label: String,
    pub models: Vec<brain::CapableModel>,
}

#[tauri::command]
pub async fn control_status(state: State<'_, AppState>) -> AppResult<ControlStatus> {
    let setting = state.settings.lock().await.control_model.clone();
    let catalog = state.catalog.get();
    let models = brain::capable_models(&catalog, &state.paths.models, &setting);
    let label = match brain::choose_source(&setting, &catalog, &state.paths.models).0 {
        brain::Source::Chat => "the chat model".to_string(),
        brain::Source::Own(k) => catalog.resolve(&k).map(|(m, _)| m.name.clone()).unwrap_or(k),
    };
    Ok(ControlStatus { supported: cfg!(windows), active: is_active(), model_label: label, models })
}

#[tauri::command]
pub fn control_log(app: AppHandle, limit: Option<usize>) -> Vec<log::Entry> {
    activity_log(&app).recent(limit.unwrap_or(100).min(500))
}

#[tauri::command]
pub fn control_log_clear(app: AppHandle) {
    activity_log(&app).clear();
}

#[tauri::command]
pub fn control_stop() {
    request_stop();
}

/// Programs that are open now, for the "allowed apps" picker.
#[tauri::command]
pub async fn control_running_apps() -> Vec<String> {
    tokio::task::spawn_blocking(|| {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let mut names: Vec<String> = sys.processes().values().map(|p| p.name().to_string_lossy().to_lowercase()).map(|n| n.strip_suffix(".exe").unwrap_or(&n).to_string()).filter(|n| !n.is_empty()).collect();
        names.sort();
        names.dedup();
        names
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_is_one_function_with_a_required_goal() {
        let s = tool_specs();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0]["function"]["name"], TOOL);
        assert_eq!(s[0]["function"]["parameters"]["required"][0], "goal");
        assert!(TOOLS.contains(&TOOL));
    }

    #[test]
    fn a_report_reads_as_text_for_the_model() {
        let r = Report {
            ok: false,
            stopped: false,
            declined: false,
            replans: 1,
            steps: vec![
                executor::StepReport { what: "Open notepad".into(), outcome: executor::Outcome::Done },
                executor::StepReport { what: "Click item 4".into(), outcome: executor::Outcome::Failed("not found".into()) },
            ],
            summary: "I couldn't finish: not found.".into(),
        };
        let t = report_text(&r);
        assert!(t.starts_with("I couldn't finish"));
        assert!(t.contains("- Open notepad: done") && t.contains("- Click item 4: failed: not found"));
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_the_front_window_says_it_cannot_be_read() {
        assert!(describe_foreground().contains("can't be read"));
    }
}
