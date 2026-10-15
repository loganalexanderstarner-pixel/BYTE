//! Runs a job: stage after stage, each stage saved to the job file when it ends, so a restart continues at the first stage that is
//! not finished. The run's control object (`RunCtl`) is what the step executor listens on: pause, stop, budgets, the live view and
//! the audit trail. Everything outside the loop (what a stage really does, where events go) is behind a trait.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::sync::oneshot;

use super::audit::Audit;
use super::contract::Meter;
use super::job::{Job, JobState, JobStore, StageStatus};
use super::policy::{HookStop, Hooks};
use super::power::{AwakeGuard, KeepAwake};
use super::tool::Sent;
use super::watch::{Presence, Takeover};
use crate::control::executor::Clock;

/// What the live view, the chat card and the notifications are told.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunEvent {
    #[serde(rename_all = "camelCase")]
    Started { job_id: String, title: String, stages: Vec<String> },
    #[serde(rename_all = "camelCase")]
    StageStart { job_id: String, index: usize, total: usize, text: String, next: String },
    #[serde(rename_all = "camelCase")]
    Step { job_id: String, app: String, what: String, ok: Option<bool>, detail: String },
    #[serde(rename_all = "camelCase")]
    CheckIn { job_id: String, id: String, title: String, fields: Vec<(String, String)>, can_remember: bool },
    #[serde(rename_all = "camelCase")]
    CheckInDone { job_id: String, id: String },
    #[serde(rename_all = "camelCase")]
    Paused { job_id: String, reason: String },
    #[serde(rename_all = "camelCase")]
    Resumed { job_id: String },
    #[serde(rename_all = "camelCase")]
    StageDone { job_id: String, index: usize, status: StageStatus, summary: String },
    #[serde(rename_all = "camelCase")]
    Finished { job_id: String, state: JobState, reason: String },
}

pub trait Events: Send + Sync {
    fn emit(&self, ev: RunEvent);
    /// A plain notification (waiting for an answer, finished).
    fn notify(&self, _text: &str) {}
}

/// What the person answered on a check-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    No,
    Yes,
    /// Yes, and don't ask again for this kind of step.
    YesAlways,
}

pub struct RunCtl {
    job_id: String,
    stop: AtomicBool,
    paused: AtomicBool,
    pause_reason: Mutex<String>,
    budget_hit: Mutex<Option<String>>,
    meter: Meter,
    time_cap_ms: Option<u64>,
    base_ms: u64,
    clock: Arc<dyn Clock>,
    started_at: u64,
    events: Arc<dyn Events>,
    audit: Option<Arc<Audit>>,
    stage_text: Mutex<String>,
    pending: Mutex<HashMap<String, oneshot::Sender<Answer>>>,
    pause_on_input: bool,
    takeover: Mutex<Takeover>,
    presence: Arc<dyn Presence>,
    own_input: AtomicU64,
    notify: bool,
}

pub struct CtlSetup {
    pub clock: Arc<dyn Clock>,
    pub events: Arc<dyn Events>,
    pub audit: Option<Arc<Audit>>,
    pub presence: Arc<dyn Presence>,
    pub pause_on_input: bool,
    pub notify: bool,
}

impl RunCtl {
    pub fn new(job: &Job, s: CtlSetup) -> Arc<RunCtl> {
        let started_at = s.clock.now_ms();
        Arc::new(RunCtl {
            job_id: job.id.clone(),
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            pause_reason: Mutex::new(String::new()),
            budget_hit: Mutex::new(None),
            meter: Meter::for_contract(&job.contract, job.steps, job.messages),
            time_cap_ms: job.contract.time_cap_ms(),
            base_ms: job.used_ms,
            clock: s.clock,
            started_at,
            events: s.events,
            audit: s.audit,
            stage_text: Mutex::new(String::new()),
            pending: Mutex::new(HashMap::new()),
            pause_on_input: s.pause_on_input,
            takeover: Mutex::new(Takeover::default()),
            presence: s.presence,
            own_input: AtomicU64::new(0),
            notify: s.notify,
        })
    }

    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    pub fn pause(&self, reason: &str) {
        if let Ok(mut r) = self.pause_reason.lock() {
            *r = reason.to_string();
        }
        if !self.paused.swap(true, Ordering::SeqCst) {
            self.events.emit(RunEvent::Paused { job_id: self.job_id.clone(), reason: reason.to_string() });
            if self.notify {
                self.events.notify(&format!("BYTE paused: {reason}"));
            }
        }
    }

    pub fn resume(&self) {
        if self.paused.swap(false, Ordering::SeqCst) {
            if let Ok(mut t) = self.takeover.lock() {
                t.reset();
            }
            self.events.emit(RunEvent::Resumed { job_id: self.job_id.clone() });
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn meter(&self) -> &Meter {
        &self.meter
    }

    pub fn used_ms(&self) -> u64 {
        self.base_ms + self.clock.now_ms().saturating_sub(self.started_at)
    }

    pub fn budget_hit(&self) -> Option<String> {
        self.budget_hit.lock().ok().and_then(|b| b.clone())
    }

    fn hit(&self, why: String) -> HookStop {
        if let Ok(mut b) = self.budget_hit.lock() {
            b.get_or_insert(why.clone());
        }
        HookStop::Budget(why)
    }

    /// The time budget, checked between stages and before every step.
    pub fn time_check(&self) -> Result<(), String> {
        match self.time_cap_ms {
            Some(cap) if self.used_ms() >= cap => Err(format!("the time budget ({}) is used up", super::limit::say_time(Some(cap)))),
            _ => Ok(()),
        }
    }

    pub fn set_stage(&self, text: &str) {
        if let Ok(mut t) = self.stage_text.lock() {
            *t = text.to_string();
        }
    }

    /// Writes one line in the audit trail for the stage in progress.
    pub fn audit(&self, app: &str, what: &str, ok: bool, result: &str) {
        if let Some(a) = &self.audit {
            let stage = self.stage_text.lock().map(|t| t.clone()).unwrap_or_default();
            a.record(&self.job_id, &stage, app, what, ok, result);
        }
    }

    /// Tells the person something is outside the contract (or risky) and waits for the answer. Stop answers No.
    pub async fn checkin(&self, title: &str, fields: Vec<(String, String)>, can_remember: bool) -> Answer {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (tx, mut rx) = oneshot::channel();
        if let Ok(mut p) = self.pending.lock() {
            p.insert(id.clone(), tx);
        }
        self.events.emit(RunEvent::CheckIn { job_id: self.job_id.clone(), id: id.clone(), title: title.to_string(), fields, can_remember });
        if self.notify {
            self.events.notify(&format!("BYTE is waiting for you: {title}"));
        }
        let answer = loop {
            tokio::select! {
                r = &mut rx => break r.unwrap_or(Answer::No),
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.stopped() {
                        break Answer::No;
                    }
                }
            }
        };
        if let Ok(mut p) = self.pending.lock() {
            p.remove(&id);
        }
        self.events.emit(RunEvent::CheckInDone { job_id: self.job_id.clone(), id });
        answer
    }

    /// The person answered a check-in. False when it is no longer waiting.
    pub fn answer(&self, id: &str, a: Answer) -> bool {
        let tx = self.pending.lock().ok().and_then(|mut p| p.remove(id));
        tx.map(|tx| tx.send(a).is_ok()).unwrap_or(false)
    }
}

impl Hooks for RunCtl {
    fn before_step<'a>(&'a self) -> BoxFuture<'a, Result<(), HookStop>> {
        Box::pin(async move {
            loop {
                if self.stopped() {
                    return Err(HookStop::Stopped);
                }
                if self.pause_on_input && !self.is_paused() {
                    if let Some(last) = self.presence.last_input() {
                        let took = self.takeover.lock().map(|mut t| t.check(last, self.own_input.load(Ordering::SeqCst))).unwrap_or(false);
                        if took {
                            self.pause("you started using the PC. Press Resume when you are done");
                        }
                    }
                }
                if self.is_paused() {
                    self.clock.sleep(250).await;
                    tokio::task::yield_now().await;
                    continue;
                }
                break;
            }
            if let Err(w) = self.time_check() {
                return Err(self.hit(w));
            }
            self.meter.take_step().map_err(|w| self.hit(w))
        })
    }

    fn step_started(&self, app: &str, what: &str) {
        self.events.emit(RunEvent::Step { job_id: self.job_id.clone(), app: app.to_string(), what: what.to_string(), ok: None, detail: String::new() });
    }

    fn step_finished(&self, app: &str, what: &str, ok: bool, detail: &str) {
        self.own_input.store(self.presence.now(), Ordering::SeqCst);
        self.audit(app, what, ok, detail);
        self.events.emit(RunEvent::Step { job_id: self.job_id.clone(), app: app.to_string(), what: what.to_string(), ok: Some(ok), detail: detail.to_string() });
    }

    fn take_message(&self) -> Result<(), String> {
        self.meter.take_message()
    }
}

// ------------------------------------------------------------------ stages

/// What ended a stage.
#[derive(Debug, Clone, PartialEq)]
pub enum StageEnd {
    Done,
    /// Not done, and the job goes on with the next stage (the contract forbade it, or the person said no).
    Skipped(String),
    /// Not done, and the job stops here (later stages may depend on it).
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageResult {
    pub end: StageEnd,
    pub summary: String,
    pub read: Vec<String>,
    pub sent: Vec<Sent>,
    pub files: Vec<String>,
}

impl StageResult {
    pub fn done(summary: &str) -> StageResult {
        StageResult { end: StageEnd::Done, summary: summary.into(), read: vec![], sent: vec![], files: vec![] }
    }
}

/// What a stage really does (plan and run PC steps, browse, write a file, call a tool).
pub trait StageExec: Send + Sync {
    fn run<'a>(&'a self, job: &'a Job, index: usize, ctl: &'a Arc<RunCtl>) -> BoxFuture<'a, StageResult>;
}

/// Runs the job from its first unfinished stage to the end (or a stop), saving after every stage. Returns the job as it ended.
pub async fn run_job(exec: &dyn StageExec, store: &JobStore, ctl: &Arc<RunCtl>, awake: &dyn KeepAwake, keep_awake: bool, mut job: Job) -> Job {
    let _awake = AwakeGuard::new(awake, keep_awake);
    job.mark_started();
    let _ = store.save(&job);
    ctl.events.emit(RunEvent::Started { job_id: job.id.clone(), title: job.title.clone(), stages: job.stages.iter().map(|s| s.text.clone()).collect() });
    let total = job.stages.len();
    let mut ended: Option<(JobState, String)> = None;
    while let Some(ix) = job.next_index() {
        if ctl.stopped() {
            ended = Some((JobState::Stopped, "you pressed Stop".into()));
            break;
        }
        if let Err(w) = ctl.time_check() {
            ended = Some((JobState::Stopped, w));
            break;
        }
        job.stages[ix].status = StageStatus::Running;
        job.current = ix;
        let _ = store.save(&job);
        let text = job.stages[ix].text.clone();
        ctl.set_stage(&text);
        let next = job.stages.get(ix + 1).map(|s| s.text.clone()).unwrap_or_default();
        ctl.events.emit(RunEvent::StageStart { job_id: job.id.clone(), index: ix, total, text, next });
        let t0 = ctl.clock.now_ms();
        let steps0 = ctl.meter.steps();
        let r = exec.run(&job, ix, ctl).await;
        let budget = ctl.budget_hit();
        {
            let s = &mut job.stages[ix];
            s.ms += ctl.clock.now_ms().saturating_sub(t0);
            s.steps += ctl.meter.steps().saturating_sub(steps0);
            s.summary = r.summary.clone();
            s.read = r.read.clone();
            s.sent = r.sent.clone();
            s.files = r.files.clone();
            match (&r.end, ctl.stopped(), &budget) {
                (_, true, _) => {
                    s.status = StageStatus::Pending;
                    s.reason = "you pressed Stop".into();
                }
                (_, _, Some(w)) => {
                    s.status = StageStatus::Pending;
                    s.reason = w.clone();
                }
                (StageEnd::Done, _, _) => s.status = StageStatus::Done,
                (StageEnd::Skipped(w), _, _) => {
                    s.status = StageStatus::Skipped;
                    s.reason = w.clone();
                }
                (StageEnd::Failed(w), _, _) => {
                    s.status = StageStatus::Failed;
                    s.reason = w.clone();
                }
            }
        }
        job.used_ms = ctl.used_ms();
        job.steps = ctl.meter.steps();
        job.messages = ctl.meter.messages();
        let _ = store.save(&job);
        let st = job.stages[ix].clone();
        ctl.events.emit(RunEvent::StageDone { job_id: job.id.clone(), index: ix, status: st.status, summary: if st.reason.is_empty() { st.summary } else { st.reason.clone() } });
        if ctl.stopped() {
            ended = Some((JobState::Stopped, "you pressed Stop".into()));
            break;
        }
        if let Some(w) = budget {
            ended = Some((JobState::Stopped, w));
            break;
        }
        if let StageEnd::Failed(w) = r.end {
            ended = Some((JobState::Failed, format!("stage {} did not finish ({w})", ix + 1)));
            break;
        }
    }
    let (state, why) = ended.unwrap_or((JobState::Done, String::new()));
    job.used_ms = ctl.used_ms();
    job.steps = ctl.meter.steps();
    job.messages = ctl.meter.messages();
    job.mark_finished(state, &why);
    let _ = store.save(&job);
    ctl.events.emit(RunEvent::Finished { job_id: job.id.clone(), state, reason: why });
    job
}

#[cfg(test)]
pub mod fake {
    use super::*;

    #[derive(Default)]
    pub struct Collect(pub Mutex<Vec<RunEvent>>, pub Mutex<Vec<String>>);

    impl Events for Collect {
        fn emit(&self, ev: RunEvent) {
            self.0.lock().unwrap().push(ev);
        }
        fn notify(&self, text: &str) {
            self.1.lock().unwrap().push(text.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Collect;
    use super::*;
    use crate::control::auto::contract::Contract;
    use crate::control::auto::job::StageDraft;
    use crate::control::auto::power::FakeAwake;
    use crate::control::auto::watch::FakePresence;
    use crate::control::executor::fake::FakeClock;

    struct Setup {
        ev: Arc<Collect>,
        clock: Arc<FakeClock>,
        presence: Arc<FakePresence>,
    }

    fn ctl_for(job: &Job, pause_on_input: bool) -> (Arc<RunCtl>, Setup) {
        let ev = Arc::new(Collect::default());
        let clock = Arc::new(FakeClock::default());
        let presence = Arc::new(FakePresence { now: 0.into(), input: 0.into() });
        let ctl = RunCtl::new(job, CtlSetup { clock: clock.clone(), events: ev.clone(), audit: None, presence: presence.clone(), pause_on_input, notify: true });
        (ctl, Setup { ev, clock, presence })
    }

    fn job(n: usize, contract: Contract) -> Job {
        Job::new("do things", (0..n).map(|i| StageDraft { text: format!("stage {i}"), tool: "pc".into() }).collect(), contract)
    }

    fn store() -> (JobStore, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("byte-run-{}", uuid::Uuid::new_v4().simple()));
        (JobStore::new(d.clone()), d)
    }

    /// Does what it is told for each stage index.
    struct Script {
        seen: Mutex<Vec<usize>>,
        ends: Vec<StageEnd>,
        stop_after: Option<usize>,
        steps_each: u32,
    }

    impl StageExec for Script {
        fn run<'a>(&'a self, _job: &'a Job, ix: usize, ctl: &'a Arc<RunCtl>) -> BoxFuture<'a, StageResult> {
            Box::pin(async move {
                self.seen.lock().unwrap().push(ix);
                for _ in 0..self.steps_each {
                    if ctl.before_step().await.is_err() {
                        break;
                    }
                    ctl.step_started("notepad", "Click item 1");
                    ctl.step_finished("notepad", "Click item 1", true, "");
                }
                if self.stop_after == Some(ix) {
                    ctl.stop();
                }
                let end = self.ends.get(ix).cloned().unwrap_or(StageEnd::Done);
                StageResult { end, summary: format!("did {ix}"), read: vec![format!("page {ix}")], sent: vec![], files: vec![] }
            })
        }
    }

    fn script(ends: Vec<StageEnd>) -> Script {
        Script { seen: Mutex::new(vec![]), ends, stop_after: None, steps_each: 1 }
    }

    #[tokio::test]
    async fn stages_run_in_order_each_one_is_saved_and_the_pc_is_kept_awake_then_released() {
        let (st, dir) = store();
        let j = job(3, Contract::default());
        let (ctl, s) = ctl_for(&j, false);
        let awake = FakeAwake::default();
        let ex = script(vec![]);
        let out = run_job(&ex, &st, &ctl, &awake, true, j.clone()).await;
        assert_eq!(out.state, JobState::Done);
        assert_eq!(*ex.seen.lock().unwrap(), vec![0, 1, 2]);
        assert!(out.stages.iter().all(|x| x.status == StageStatus::Done) && out.steps == 3);
        assert_eq!(st.load(&j.id).unwrap().state, JobState::Done, "the file has the final state");
        assert_eq!((awake.holds.load(Ordering::SeqCst), awake.releases.load(Ordering::SeqCst)), (1, 1));
        let ev = s.ev.0.lock().unwrap();
        assert!(matches!(ev.first(), Some(RunEvent::Started { .. })) && matches!(ev.last(), Some(RunEvent::Finished { state: JobState::Done, .. })));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn stop_ends_the_job_and_a_later_run_picks_up_at_the_first_unfinished_stage() {
        let (st, dir) = store();
        let j = job(3, Contract::default());
        let (ctl, _s) = ctl_for(&j, false);
        let mut ex = script(vec![]);
        ex.stop_after = Some(0);
        let out = run_job(&ex, &st, &ctl, &FakeAwake::default(), false, j.clone()).await;
        assert_eq!(out.state, JobState::Stopped);
        assert_eq!(out.stages[0].status, StageStatus::Pending, "a stage that was stopped is not counted as done");
        // BYTE is restarted: the job is loaded from its file and run again with a fresh control object.
        let loaded = st.load(&j.id).unwrap();
        let (ctl2, _s2) = ctl_for(&loaded, false);
        let ex2 = script(vec![]);
        let out2 = run_job(&ex2, &st, &ctl2, &FakeAwake::default(), false, loaded).await;
        assert_eq!(*ex2.seen.lock().unwrap(), vec![0, 1, 2]);
        assert_eq!(out2.state, JobState::Done);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_finished_stage_is_not_run_again_after_a_restart() {
        let (st, dir) = store();
        let mut j = job(3, Contract::default());
        j.stages[0].status = StageStatus::Done;
        j.stages[1].status = StageStatus::Running;
        j.state = JobState::Running;
        st.save(&j).unwrap();
        st.recover();
        let loaded = st.load(&j.id).unwrap();
        let (ctl, _s) = ctl_for(&loaded, false);
        let ex = script(vec![]);
        let out = run_job(&ex, &st, &ctl, &FakeAwake::default(), false, loaded).await;
        assert_eq!(*ex.seen.lock().unwrap(), vec![1, 2]);
        assert_eq!(out.state, JobState::Done);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_failed_stage_stops_the_job_and_a_skipped_one_does_not() {
        let (st, dir) = store();
        let j = job(3, Contract::default());
        let (ctl, _s) = ctl_for(&j, false);
        let ex = script(vec![StageEnd::Skipped("the contract says never delete".into()), StageEnd::Failed("window not found".into())]);
        let out = run_job(&ex, &st, &ctl, &FakeAwake::default(), false, j).await;
        assert_eq!(out.state, JobState::Failed);
        assert_eq!(out.stages[0].status, StageStatus::Skipped);
        assert_eq!(out.stages[1].status, StageStatus::Failed);
        assert_eq!(out.stages[2].status, StageStatus::Pending);
        assert!(out.stop_reason.contains("stage 2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_step_budget_stops_the_job_between_steps() {
        let (st, dir) = store();
        let j = job(3, Contract { step_budget: "2".into(), ..Default::default() });
        let (ctl, _s) = ctl_for(&j, false);
        let mut ex = script(vec![]);
        ex.steps_each = 2;
        let out = run_job(&ex, &st, &ctl, &FakeAwake::default(), false, j).await;
        assert_eq!(out.state, JobState::Stopped);
        assert!(out.stop_reason.contains("step budget (2)"), "{}", out.stop_reason);
        assert_eq!(out.stages[0].status, StageStatus::Done);
        assert_eq!(out.stages[1].status, StageStatus::Pending, "it can be resumed with a bigger budget");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn unlimited_budgets_never_stop_the_job() {
        let (st, dir) = store();
        let j = job(2, Contract { step_budget: "unlimited".into(), time_budget: "unlimited".into(), message_budget: "unlimited".into(), ..Default::default() });
        let (ctl, s) = ctl_for(&j, false);
        let mut ex = script(vec![]);
        ex.steps_each = 500;
        s.clock.0.store(10_000_000_000, Ordering::SeqCst);
        let out = run_job(&ex, &st, &ctl, &FakeAwake::default(), false, j).await;
        assert_eq!(out.state, JobState::Done);
        assert_eq!(out.steps, 1000);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_time_budget_stops_the_job() {
        let (st, dir) = store();
        let j = job(3, Contract { time_budget: "1m".into(), ..Default::default() });
        let (ctl, s) = ctl_for(&j, false);
        s.clock.0.store(61_000, Ordering::SeqCst);
        let ex = script(vec![]);
        let out = run_job(&ex, &st, &ctl, &FakeAwake::default(), false, j).await;
        assert_eq!(out.state, JobState::Stopped);
        assert!(out.stop_reason.contains("time budget"));
        assert!(ex.seen.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn using_the_mouse_pauses_the_run_until_resume_when_that_is_switched_on() {
        let j = job(1, Contract::default());
        let (ctl, s) = ctl_for(&j, true);
        assert!(ctl.before_step().await.is_ok());
        ctl.step_finished("notepad", "Click item 1", true, "");
        s.presence.input.store(50_000, Ordering::SeqCst);
        s.presence.now.store(51_000, Ordering::SeqCst);
        let c2 = ctl.clone();
        let waiter = tokio::spawn(async move { c2.before_step().await });
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert!(ctl.is_paused(), "the person moved the mouse");
        assert!(!waiter.is_finished());
        ctl.resume();
        assert!(waiter.await.unwrap().is_ok());
        let ev = s.ev.0.lock().unwrap();
        assert!(ev.iter().any(|e| matches!(e, RunEvent::Paused { .. })) && ev.iter().any(|e| matches!(e, RunEvent::Resumed { .. })));
        assert_eq!(s.ev.1.lock().unwrap().len(), 1, "a notification was sent for the pause");
    }

    #[tokio::test]
    async fn without_the_setting_input_never_pauses() {
        let j = job(1, Contract::default());
        let (ctl, s) = ctl_for(&j, false);
        s.presence.input.store(50_000, Ordering::SeqCst);
        assert!(ctl.before_step().await.is_ok() && !ctl.is_paused());
    }

    #[tokio::test]
    async fn a_check_in_waits_for_the_answer_and_stop_answers_no() {
        let j = job(1, Contract::default());
        let (ctl, s) = ctl_for(&j, false);
        let c2 = ctl.clone();
        let t = tokio::spawn(async move { c2.checkin("Outside the contract", vec![("Why".into(), "x".into())], true).await });
        let id = loop {
            tokio::task::yield_now().await;
            let found = s.ev.0.lock().unwrap().iter().find_map(|e| if let RunEvent::CheckIn { id, .. } = e { Some(id.clone()) } else { None });
            if let Some(i) = found {
                break i;
            }
        };
        assert!(ctl.answer(&id, Answer::YesAlways));
        assert_eq!(t.await.unwrap(), Answer::YesAlways);
        assert!(!ctl.answer(&id, Answer::Yes), "answered once");
        let c3 = ctl.clone();
        let t2 = tokio::spawn(async move { c3.checkin("again", vec![], false).await });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        ctl.stop();
        assert_eq!(t2.await.unwrap(), Answer::No);
    }
}
