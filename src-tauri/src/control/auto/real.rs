//! The parts of autonomous mode that touch the app: the real stage executor (PC steps, the browser, writing files, plugged-in tools),
//! the check-in approver, events to the window and the UI, the stop key, the report chat, and the commands Settings calls.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

use super::audit::{Audit, AuditEntry};
use super::contract::Contract;
use super::job::{self, Job, JobState, JobStore, StageDraft};
use super::level::Level;
use super::live::{self, LiveView};
use super::policy::{Hooks, Policy};
use super::power::SystemAwake;
use super::rules::{RuleKey, RuleStore};
use super::runner::{self, Answer, CtlSetup, Events, RunCtl, RunEvent, StageEnd, StageExec, StageResult};
use super::tool::{self, CallBlock, ToolCall};
use super::watch::SystemPresence;
use crate::control::{self, brain, executor::Approver};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const EVENT: &str = "autonomy://event";

struct Active {
    ctl: Arc<RunCtl>,
    cancel: CancellationToken,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static VIEW: Mutex<Option<LiveView>> = Mutex::new(None);
static STOP_ID: Mutex<Option<u32>> = Mutex::new(None);

fn active_ctl() -> Option<Arc<RunCtl>> {
    ACTIVE.lock().ok().and_then(|a| a.as_ref().map(|x| x.ctl.clone()))
}

pub fn is_running() -> bool {
    ACTIVE.lock().map(|a| a.is_some()).unwrap_or(false)
}

// ------------------------------------------------------------------ events

struct AppEvents {
    app: AppHandle,
}

impl Events for AppEvents {
    fn emit(&self, ev: RunEvent) {
        let view = {
            let mut g = VIEW.lock().unwrap_or_else(|p| p.into_inner());
            let v = g.get_or_insert_with(LiveView::idle);
            live::apply(v, &ev);
            v.clone()
        };
        let _ = self.app.emit(EVENT, &view);
        if view.active() || matches!(ev, RunEvent::Finished { .. }) {
            live::show(&self.app, &view);
        }
        if matches!(ev, RunEvent::Finished { .. }) {
            let app = self.app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                if !is_running() {
                    live::hide(&app);
                }
            });
        }
    }

    fn notify(&self, text: &str) {
        crate::scheduler::notify(&self.app, "BYTE", text);
    }
}

// ------------------------------------------------------------------ approving inside a job

/// The approver of a job: a card becomes a check-in that waits for the person. Saved rules still answer without asking.
struct JobApprover {
    ctl: Arc<RunCtl>,
    rules: RuleStore,
    user: String,
}

impl Approver for JobApprover {
    fn approve<'a>(&'a self, title: &'a str, fields: Vec<(String, String)>) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.ctl.checkin(title, fields, false).await != Answer::No })
    }

    fn approve_step<'a>(&'a self, title: &'a str, fields: Vec<(String, String)>, key: Option<RuleKey>) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            if let Some(k) = &key {
                if self.rules.matches(k, &self.user) {
                    return true;
                }
            }
            match self.ctl.checkin(title, fields, key.is_some()).await {
                Answer::No => false,
                Answer::Yes => true,
                Answer::YesAlways => {
                    if let Some(k) = &key {
                        let _ = self.rules.add(k, &self.user);
                    }
                    true
                }
            }
        })
    }
}

// ------------------------------------------------------------------ the real stage executor

/// Which kind of work a stage with the tool "auto" is.
pub fn route(text: &str, tool: &str, plugged_in: &[String]) -> String {
    let t = tool.trim().to_lowercase();
    if matches!(t.as_str(), "pc" | "browser" | "file") || plugged_in.iter().any(|p| p.to_lowercase() == t) {
        return t;
    }
    let low = text.to_lowercase();
    if low.contains("http") || low.contains("website") || low.contains("web site") || low.contains("search the web") || low.contains("look up online") || low.contains(".com") {
        "browser".into()
    } else if low.starts_with("write a file") || low.starts_with("save a file") || low.contains("write a text file") {
        "file".into()
    } else {
        "pc".into()
    }
}

/// The goal given to whatever does a stage: the stage, what the earlier stages found, and the limits.
pub fn stage_goal(job: &Job, ix: usize) -> String {
    let mut g = format!("{}\n", job.stages[ix].text);
    let before: Vec<String> = job.stages[..ix].iter().filter(|s| !s.summary.is_empty()).map(|s| format!("- {}: {}", s.text, s.summary.chars().take(200).collect::<String>())).collect();
    if !before.is_empty() {
        g.push_str(&format!("\nEarlier stages (information only):\n{}\n", before.join("\n")));
    }
    g.push_str("\nThis is one stage of a larger job the person approved; do only this stage. Never enter passwords, card numbers or codes.");
    g
}

struct RealExec {
    app: AppHandle,
    cancel: CancellationToken,
    rules: RuleStore,
    user: String,
}

impl StageExec for RealExec {
    fn run<'a>(&'a self, job: &'a Job, ix: usize, ctl: &'a Arc<RunCtl>) -> BoxFuture<'a, StageResult> {
        Box::pin(async move {
            let plugged = tool::registry().names();
            let kind = route(&job.stages[ix].text, &job.stages[ix].tool, &plugged);
            match kind.as_str() {
                "browser" => self.browser(job, ix, ctl).await,
                "file" => self.file(job, ix, ctl).await,
                "pc" => self.pc(job, ix, ctl).await,
                name => self.tool(name, job, ix, ctl).await,
            }
        })
    }
}

fn fail(why: &str) -> StageResult {
    StageResult { end: StageEnd::Failed(why.to_string()), summary: why.to_string(), read: vec![], sent: vec![], files: vec![] }
}

fn skip(why: &str) -> StageResult {
    StageResult { end: StageEnd::Skipped(why.to_string()), summary: String::new(), read: vec![], sent: vec![], files: vec![] }
}

impl RealExec {
    fn level(&self) -> Level {
        control::settings_now(&self.app).map(|s| Level::from_settings(&s.control_autonomy, s.control_never_ask)).unwrap_or_default()
    }

    async fn pc(&self, job: &Job, ix: usize, ctl: &Arc<RunCtl>) -> StageResult {
        let Some(settings) = control::settings_now(&self.app) else { return fail("settings are busy") };
        let policy = Policy { level: self.level(), contract: Some(Arc::new(job.contract.clone())), never_touch: control::never_touch_apps(&settings), hooks: Some(ctl.clone() as Arc<dyn Hooks>) };
        let goal = stage_goal(job, ix);
        let screen = control::describe_foreground_async().await;
        let steps = match control::plan_with_model(&self.app, brain::plan_prompt(&goal, &screen)).await {
            Ok(s) => s,
            Err(e) => return fail(&format!("could not plan: {e}")),
        };
        let approver = JobApprover { ctl: ctl.clone(), rules: self.rules.clone(), user: self.user.clone() };
        let r = control::run_steps_with(&self.app, &settings, steps, &job.stages[ix].text, &self.cancel, &approver, Some(policy)).await;
        let summary = r.summary.clone();
        if r.ok {
            StageResult::done(&summary)
        } else if r.stopped {
            StageResult { end: StageEnd::Failed("stopped".into()), summary, read: vec![], sent: vec![], files: vec![] }
        } else if r.declined {
            skip("you said no to a step")
        } else if summary.starts_with("I didn't do that") {
            skip(summary.trim_end_matches('.'))
        } else {
            fail(&summary)
        }
    }

    async fn browser(&self, job: &Job, ix: usize, ctl: &Arc<RunCtl>) -> StageResult {
        if !job.contract.allows_app("browser") {
            let ok = self.level() == Level::Never || ctl.checkin("BYTE wants to use its browser", vec![("Stage".into(), job.stages[ix].text.clone()), ("Outside the run contract".into(), "the browser is not one of the programs this job may use".into())], false).await != Answer::No;
            if !ok {
                return skip("the browser is not in the run contract");
            }
        }
        let state = self.app.state::<AppState>();
        let question = format!("{}\n\nNobody is watching. If a page needs a login, a payment, a code or personal details, stop and say what you could not do. Do not send or post anything.", stage_goal(job, ix));
        let request = crate::commands::ChatRequest {
            request_id: format!("job-{}", uuid::Uuid::new_v4().simple()),
            messages: vec![crate::chat::ChatMessage::new("user", question)],
            mode: crate::settings::Mode::Auto,
            thinking: crate::settings::ThinkingPref::Auto,
            model: None,
            private: false,
            project_id: None,
            assistant_id: None,
            cloud: None,
            fresh: true,
            spoken: false,
            task: Some(crate::agent::Task::Browse),
            style_notes: String::new(),
            speaker: None,
            mood: None,
        };
        let seen: Arc<Mutex<(String, Vec<String>)>> = Arc::new(Mutex::new((String::new(), vec![])));
        let sink = seen.clone();
        let c2 = ctl.clone();
        let level = self.level();
        let channel: Channel<crate::chat::ChatEvent> = Channel::new(move |body: InvokeResponseBody| {
            let InvokeResponseBody::Json(s) = body else { return Ok(()) };
            let Ok(e) = serde_json::from_str::<Value>(&s) else { return Ok(()) };
            match e["kind"].as_str() {
                Some("content") => sink.lock().unwrap_or_else(|p| p.into_inner()).0.push_str(e["delta"].as_str().unwrap_or("")),
                Some("sources") => {
                    let mut g = sink.lock().unwrap_or_else(|p| p.into_inner());
                    for src in e["sources"].as_array().cloned().unwrap_or_default() {
                        let line = format!("{} ({})", src["title"].as_str().unwrap_or("page"), src["url"].as_str().unwrap_or(""));
                        if !g.1.contains(&line) {
                            g.1.push(line);
                        }
                    }
                }
                Some("toolCall") | Some("tool_call") => c2.audit("browser", &format!("Browser: {}", e["name"].as_str().unwrap_or("step")), true, ""),
                Some("approval") => {
                    // A card from the browser agent (submit, download): it waits for the person like any check-in.
                    let id = e["id"].as_str().unwrap_or("").to_string();
                    let title = e["title"].as_str().unwrap_or("The browser needs your OK").to_string();
                    let fields: Vec<(String, String)> = e["fields"].as_array().cloned().unwrap_or_default().iter().map(|f| (f["label"].as_str().unwrap_or("").to_string(), f["value"].as_str().unwrap_or("").to_string())).collect();
                    let c3 = c2.clone();
                    tauri::async_runtime::spawn(async move {
                        let ok = if level == Level::Never { true } else { c3.checkin(&title, fields, false).await != Answer::No };
                        crate::web_agent::answer_with(&id, ok, Vec::new());
                    });
                }
                _ => {}
            }
            Ok(())
        });
        let run = crate::backend::answer(&state, request, &channel);
        let cancel = self.cancel.clone();
        let result = tokio::select! {
            r = run => r,
            _ = cancel.cancelled() => Err(AppError::Cancelled),
        };
        let (text, read) = seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
        match result {
            Ok(()) if !text.trim().is_empty() => {
                ctl.audit("browser", "Browser stage finished", true, "");
                StageResult { end: StageEnd::Done, summary: text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(400).collect(), read, sent: vec![], files: vec![] }
            }
            Ok(()) => fail("the browser gave no answer"),
            Err(e) => fail(&e.to_string()),
        }
    }

    async fn file(&self, job: &Job, ix: usize, ctl: &Arc<RunCtl>) -> StageResult {
        if job.contract.folders.is_empty() {
            return skip("no folder is in the run contract");
        }
        let sys = format!("Write a text file for the user. Reply with JSON only: {{\"path\":\"<full path>\",\"content\":\"<the text>\"}}. The path must be inside one of these folders: {}. Plain text, Markdown or CSV only.", job.contract.folders.join("; "));
        let messages = vec![json!({"role":"system","content":sys}), json!({"role":"user","content":stage_goal(job, ix)})];
        let reply = match control::ask_model(&self.app, &messages).await {
            Ok(r) => r,
            Err(e) => return fail(&e.to_string()),
        };
        let v: Value = match control::actions::extract_json(&reply).and_then(|j| serde_json::from_str(j).ok()) {
            Some(v) => v,
            None => return fail("the model did not give a file"),
        };
        let (path, content) = (v["path"].as_str().unwrap_or(""), v["content"].as_str().unwrap_or(""));
        if content.chars().count() > super::filew::MAX_CHARS {
            return fail("the file would be too big");
        }
        let target = match super::filew::plan_write(&job.contract, path, &|p| p.exists()) {
            Ok(t) => t,
            Err(w) => return skip(&w),
        };
        if let Some(dir) = target.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&target, content) {
            Ok(()) => {
                let shown = target.display().to_string();
                ctl.audit("files", &format!("Write file {shown}"), true, &format!("{} characters", content.chars().count()));
                StageResult { end: StageEnd::Done, summary: format!("Saved {shown}."), read: vec![], sent: vec![], files: vec![shown] }
            }
            Err(e) => fail(&e.to_string()),
        }
    }

    async fn tool(&self, name: &str, job: &Job, ix: usize, ctl: &Arc<RunCtl>) -> StageResult {
        let Some(t) = tool::registry().get(name) else { return fail(&format!("the tool {name} is not available")) };
        let messages = vec![
            json!({"role":"system","content":format!("Fill in the arguments for the tool \"{name}\": {}. Reply with JSON only: the arguments object.", t.describe())}),
            json!({"role":"user","content":stage_goal(job, ix)}),
        ];
        let reply = match control::ask_model(&self.app, &messages).await {
            Ok(r) => r,
            Err(e) => return fail(&e.to_string()),
        };
        let args: Value = match control::actions::extract_json(&reply).and_then(|j| serde_json::from_str(j).ok()) {
            Some(v) => v,
            None => return fail("the model did not give the arguments"),
        };
        let call = ToolCall { tool: name.to_string(), args };
        match tool::check_call(&*t, &call, &job.contract, ctl.meter()) {
            Ok(()) => {}
            Err(CallBlock::Refused(w)) => return skip(&w),
            Err(CallBlock::Budget(w)) => return skip(&w),
            Err(CallBlock::Outside(w)) => {
                let ok = self.level() == Level::Never || ctl.checkin(&format!("BYTE wants to use {name}: outside the run contract"), vec![("Who".into(), t.recipients(&call.args).join(", ")), ("Outside the run contract".into(), w.clone())], false).await != Answer::No;
                if !ok {
                    return skip(&w);
                }
                for _ in t.recipients(&call.args) {
                    if let Err(b) = ctl.meter().take_message() {
                        return skip(&b);
                    }
                }
            }
        }
        let out = t.run(&call).await;
        for s in &out.sent {
            ctl.audit(name, &format!("Message to {}", s.to), out.ok, &s.what);
        }
        if out.ok {
            StageResult { end: StageEnd::Done, summary: out.summary, read: out.read, sent: out.sent, files: vec![] }
        } else {
            StageResult { end: StageEnd::Failed(out.summary.clone()), summary: out.summary, read: out.read, sent: out.sent, files: vec![] }
        }
    }
}

// ------------------------------------------------------------------ starting, finishing

fn allowed(app: &AppHandle) -> Result<crate::settings::Settings, String> {
    let s = control::settings_now(app).ok_or("settings are busy, try again")?;
    if !cfg!(windows) {
        return Err("Autonomous mode needs Windows.".into());
    }
    if s.kids_mode {
        return Err("Autonomous mode is off in kids mode.".into());
    }
    if !s.pc_control {
        return Err("Turn on PC control first (Settings, Control).".into());
    }
    if !s.auto_mode || !s.auto_accepted {
        return Err("Turn on autonomous mode and accept the notice first.".into());
    }
    Ok(s)
}

fn store(app: &AppHandle) -> JobStore {
    JobStore::new(super::jobs_dir(app))
}

fn start_job(app: &AppHandle, job: Job) -> Result<String, String> {
    let settings = allowed(app)?;
    if is_running() {
        return Err("A job is already running.".into());
    }
    let id = job.id.clone();
    let events: Arc<dyn Events> = Arc::new(AppEvents { app: app.clone() });
    let ctl = RunCtl::new(&job, CtlSetup { clock: Arc::new(control::executor::RealClock(std::time::Instant::now())), events, audit: Some(Arc::new(Audit::new(super::audit_file(app)))), presence: Arc::new(SystemPresence), pause_on_input: settings.auto_pause_on_input, notify: settings.auto_notify });
    let cancel = CancellationToken::new();
    if let Ok(mut a) = ACTIVE.lock() {
        *a = Some(Active { ctl: ctl.clone(), cancel: cancel.clone() });
    }
    if let Ok(mut v) = VIEW.lock() {
        *v = Some(LiveView::idle());
    }
    // The stop key only exists while a job runs.
    crate::quick::apply_shortcuts(app, &settings);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let exec = RealExec { app: app2.clone(), cancel: cancel.clone(), rules: RuleStore::new(super::rules_file(&app2)), user: super::user_id(&app2) };
        let awake = SystemAwake::default();
        let keep = settings.auto_keep_awake;
        let mut done = runner::run_job(&exec, &store(&app2), &ctl, &awake, keep, job).await;
        if let Ok(mut a) = ACTIVE.lock() {
            *a = None;
        }
        if let Some(s) = control::settings_now(&app2) {
            crate::quick::apply_shortcuts(&app2, &s);
        }
        finish(&app2, &mut done).await;
    });
    Ok(id)
}

/// Writes the report as a chat, tells the person, and reads it aloud when they asked for that.
async fn finish(app: &AppHandle, job: &mut Job) {
    let report = super::report::build(job);
    let state = app.state::<AppState>();
    let now = chrono::Utc::now().timestamp_millis();
    let chat_id = uuid::Uuid::new_v4().to_string();
    let conv = json!({
        "id": chat_id, "title": format!("Job report: {}", job.title), "createdAt": now, "updatedAt": now,
        "messages": [
            { "id": uuid::Uuid::new_v4().to_string(), "role": "user", "content": format!("Autonomous job: {}", job.request), "status": "done" },
            { "id": uuid::Uuid::new_v4().to_string(), "role": "assistant", "content": report, "status": "done", "mode": "auto" },
        ],
    });
    if state.db.save(&conv).is_ok() {
        let _ = state.db.conn().execute("UPDATE conversations SET title_locked = 1 WHERE id = ?1", [&chat_id]);
        job.chat_id = chat_id;
        let _ = store(app).save(job);
    }
    let _ = app.emit("autonomy://report", json!({ "jobId": job.id, "chatId": job.chat_id }));
    let (notify, speak) = control::settings_now(app).map(|s| (s.auto_notify, s.auto_speak_report)).unwrap_or((true, false));
    if notify {
        crate::scheduler::notify(app, "BYTE job ended", &format!("{}: {}", job.title, job.stop_reason.clone()).trim_end_matches(": ").to_string());
    }
    if speak {
        let text = super::report::speakable(job);
        let _ = crate::speech::speech_feed(app.clone(), app.state::<AppState>(), uuid::Uuid::new_v4().to_string(), text, true, None).await;
    }
    store(app).prune();
}

/// At launch: jobs that were running when BYTE closed wait for the person to resume them.
pub fn start(app: &AppHandle) {
    let found = store(app).recover();
    if !found.is_empty() {
        let names: Vec<String> = found.iter().map(|j| j.title.clone()).collect();
        let _ = app.emit("autonomy://interrupted", &names);
        crate::scheduler::notify(app, "BYTE", &format!("A job was interrupted when BYTE closed: {}. Open Settings, Control to resume it.", names.join(", ")));
    }
}

// ------------------------------------------------------------------ the stop key

/// Registers the stop key while a job is running (called by `quick::apply_shortcuts` after it cleared every shortcut).
pub fn register_stop_key(app: &AppHandle, s: &crate::settings::Settings, taken: &[Option<u32>]) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let mut id = None;
    if is_running() {
        match crate::quick::parse_keys(&s.auto_stop_key) {
            Ok(sc) if !taken.contains(&Some(sc.id())) => match app.global_shortcut().register(sc) {
                Ok(()) => id = Some(sc.id()),
                Err(e) => log::warn!("couldn't register the stop key {}: {e}", s.auto_stop_key),
            },
            Ok(_) => log::warn!("the stop key is already another BYTE shortcut"),
            Err(e) => log::warn!("{e}"),
        }
    }
    if let Ok(mut g) = STOP_ID.lock() {
        *g = id;
    }
}

pub fn is_stop_key(id: u32) -> bool {
    STOP_ID.lock().map(|g| *g == Some(id)).unwrap_or(false)
}

pub fn stop_now() {
    let a = ACTIVE.lock().ok();
    if let Some(Some(a)) = a.as_deref() {
        a.ctl.stop();
        a.cancel.cancel();
    }
    control::request_stop();
}

// ------------------------------------------------------------------ commands

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoStatus {
    pub supported: bool,
    pub running: bool,
    pub view: LiveView,
    pub tools: Vec<String>,
}

#[tauri::command]
pub fn auto_status() -> AutoStatus {
    AutoStatus { supported: cfg!(windows), running: is_running(), view: VIEW.lock().ok().and_then(|v| v.clone()).unwrap_or_else(LiveView::idle), tools: tool::registry().names() }
}

#[tauri::command]
pub fn auto_jobs(app: AppHandle) -> Vec<Job> {
    store(&app).list()
}

#[tauri::command]
pub fn auto_job_delete(app: AppHandle, id: String) {
    store(&app).delete(&id);
}

#[tauri::command]
pub fn auto_report(app: AppHandle, id: String) -> AppResult<String> {
    store(&app).load(&id).map(|j| super::report::build(&j)).ok_or_else(|| AppError::msg("that job isn't there any more"))
}

/// Splits what the person wrote into stages for them to check. A list they wrote themselves needs no model.
#[tauri::command]
pub async fn auto_split(app: AppHandle, request: String) -> AppResult<Vec<StageDraft>> {
    allowed(&app).map_err(AppError::msg)?;
    let max = control::settings_now(&app).and_then(|s| super::limit::count(&s.auto_max_stages, Some(10))).map(|n| n as usize);
    let plugged = tool::registry().names();
    if let Some(list) = job::already_a_list(&request) {
        return job::parse_stages(&serde_json::to_string(&list.iter().map(|d| d.text.clone()).collect::<Vec<_>>()).unwrap_or_default(), max, &plugged).map_err(AppError::msg);
    }
    let messages = job::split_prompt(&request, &tool::registry().descriptions());
    let reply = control::ask_model(&app, &messages).await?;
    job::parse_stages(&reply, max, &plugged).map_err(AppError::msg)
}

#[tauri::command]
pub fn auto_start(app: AppHandle, request: String, stages: Vec<StageDraft>, contract: Contract) -> AppResult<String> {
    if stages.is_empty() {
        return Err(AppError::msg("There is nothing to do yet."));
    }
    let job = Job::new(&request, stages, contract.clone());
    let _ = std::fs::write(contract_file(&app), serde_json::to_vec_pretty(&job.contract).unwrap_or_default());
    start_job(&app, job).map_err(AppError::msg)
}

/// Continues a stopped, failed or interrupted job from its first unfinished stage (with new limits when the person changed them).
#[tauri::command]
pub fn auto_job_resume(app: AppHandle, id: String, contract: Option<Contract>) -> AppResult<String> {
    let mut job = store(&app).load(&id).ok_or_else(|| AppError::msg("that job isn't there any more"))?;
    if job.state == JobState::Done || job.state == JobState::Running {
        return Err(AppError::msg("That job is not waiting."));
    }
    if let Some(c) = contract {
        job.contract = c.normalized();
    }
    for s in &mut job.stages {
        if s.status == job::StageStatus::Failed {
            s.status = job::StageStatus::Pending;
        }
    }
    start_job(&app, job).map_err(AppError::msg)
}

#[tauri::command]
pub fn auto_stop() {
    stop_now();
}

#[tauri::command]
pub fn auto_pause() {
    if let Some(c) = active_ctl() {
        c.pause("you paused it");
    }
}

#[tauri::command]
pub fn auto_resume() {
    if let Some(c) = active_ctl() {
        c.resume();
    }
}

#[tauri::command]
pub fn auto_checkin_answer(id: String, answer: String) -> bool {
    let a = match answer.as_str() {
        "yes" => Answer::Yes,
        "always" => Answer::YesAlways,
        _ => Answer::No,
    };
    active_ctl().map(|c| c.answer(&id, a)).unwrap_or(false)
}

fn contract_file(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.data.join("auto_contract.json")
}

/// The contract used last (or the default), to fill in the form.
#[tauri::command]
pub fn auto_contract(app: AppHandle) -> Contract {
    std::fs::read_to_string(contract_file(&app)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

#[tauri::command]
pub fn auto_audit(app: AppHandle, limit: Option<usize>) -> Vec<AuditEntry> {
    Audit::new(super::audit_file(&app)).recent(limit.unwrap_or(200).min(2000))
}

#[tauri::command]
pub fn auto_audit_clear(app: AppHandle) {
    Audit::new(super::audit_file(&app)).clear();
}

/// Writes the audit trail to a file in Documents/BYTE and returns where. `format`: "csv" or "md".
#[tauri::command]
pub fn auto_audit_export(app: AppHandle, format: String) -> AppResult<String> {
    let entries = Audit::new(super::audit_file(&app)).recent(100_000);
    let (text, ext) = if format == "csv" { (super::audit::to_csv(&entries), "csv") } else { (super::audit::to_markdown(&entries), "md") };
    let dir = crate::paths::user_documents_dir(&app)?.join("BYTE");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("BYTE audit trail {}.{ext}", chrono::Local::now().format("%Y-%m-%d %H%M%S")));
    std::fs::write(&file, text)?;
    Ok(file.display().to_string())
}

/// Checks a stop key against the other shortcuts (Settings calls this before saving it).
#[tauri::command]
pub fn auto_stop_key_check(state: State<'_, AppState>, keys: String) -> Result<(), String> {
    let s = state.settings.try_lock().map_err(|_| "settings are busy".to_string())?;
    super::watch::validate_stop_key(&keys, &[s.quick_ask_keys.clone(), s.selection_keys.clone(), s.talk_hotkey.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::auto::job::StageDraft;

    #[test]
    fn auto_stages_are_routed_by_their_words() {
        assert_eq!(route("Open notepad and type a note", "auto", &[]), "pc");
        assert_eq!(route("Look up the opening hours on their website", "auto", &[]), "browser");
        assert_eq!(route("anything", "file", &[]), "file");
        assert_eq!(route("Tell Mom", "discord", &["discord".into()]), "discord");
        assert_eq!(route("Tell Mom", "discord", &[]), "pc");
    }

    #[test]
    fn a_stage_goal_carries_earlier_findings_and_the_safety_line() {
        let mut j = Job::new("x", vec![StageDraft { text: "one".into(), tool: "pc".into() }, StageDraft { text: "two".into(), tool: "pc".into() }], Contract::default());
        j.stages[0].summary = "found 3 files".into();
        let g = stage_goal(&j, 1);
        assert!(g.starts_with("two") && g.contains("one: found 3 files") && g.contains("Never enter passwords"));
        assert!(!stage_goal(&j, 0).contains("Earlier stages"));
    }
}
