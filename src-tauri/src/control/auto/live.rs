//! The live view of a running job: what it is doing now, what is next, how far along it is, and a Stop button. The same state feeds
//! the small floating window and the activity feed on the chat card. `apply` is pure (events in, view out) so it is tested.

use serde::Serialize;

use super::runner::RunEvent;

const FEED_MAX: usize = 80;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub app: String,
    pub what: String,
    /// None while the step is running.
    pub ok: Option<bool>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CheckInView {
    pub id: String,
    pub title: String,
    pub fields: Vec<(String, String)>,
    pub can_remember: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveView {
    pub job_id: String,
    pub title: String,
    pub stages_total: usize,
    pub stages_done: usize,
    /// 1-based number of the stage in progress (0 before the first).
    pub stage: usize,
    pub stage_text: String,
    pub next: String,
    /// What it is doing this moment.
    pub now: String,
    /// "running" | "waiting" | "paused" | "done" | "stopped" | "failed" | "idle"
    pub status: String,
    pub note: String,
    /// 0 to 100.
    pub percent: u32,
    pub feed: Vec<FeedItem>,
    pub check_in: Option<CheckInView>,
}

impl LiveView {
    pub fn idle() -> LiveView {
        LiveView { status: "idle".into(), ..Default::default() }
    }

    pub fn active(&self) -> bool {
        matches!(self.status.as_str(), "running" | "waiting" | "paused")
    }
}

fn percent(done: usize, total: usize) -> u32 {
    if total == 0 {
        0
    } else {
        ((done * 100) / total).min(100) as u32
    }
}

/// Updates the view for one event.
pub fn apply(v: &mut LiveView, ev: &RunEvent) {
    match ev {
        RunEvent::Started { job_id, title, stages } => {
            *v = LiveView { job_id: job_id.clone(), title: title.clone(), stages_total: stages.len(), status: "running".into(), next: stages.first().cloned().unwrap_or_default(), ..Default::default() };
        }
        RunEvent::StageStart { index, total, text, next, .. } => {
            v.stage = index + 1;
            v.stages_total = *total;
            v.stage_text = text.clone();
            v.next = next.clone();
            v.now = text.clone();
            v.status = "running".into();
            v.percent = percent(v.stages_done, *total);
        }
        RunEvent::Step { app, what, ok, detail, .. } => {
            match ok {
                None => {
                    v.now = what.clone();
                    v.feed.push(FeedItem { app: app.clone(), what: what.clone(), ok: None, detail: String::new() });
                }
                Some(done) => {
                    if let Some(last) = v.feed.iter_mut().rev().find(|f| f.ok.is_none() && f.what == *what) {
                        last.ok = Some(*done);
                        last.detail = detail.clone();
                        last.app = app.clone();
                    } else {
                        v.feed.push(FeedItem { app: app.clone(), what: what.clone(), ok: Some(*done), detail: detail.clone() });
                    }
                }
            }
            let over = v.feed.len().saturating_sub(FEED_MAX);
            v.feed.drain(..over);
        }
        RunEvent::CheckIn { id, title, fields, can_remember, .. } => {
            v.check_in = Some(CheckInView { id: id.clone(), title: title.clone(), fields: fields.clone(), can_remember: *can_remember });
            v.status = "waiting".into();
            v.note = "Waiting for your answer".into();
        }
        RunEvent::CheckInDone { .. } => {
            v.check_in = None;
            v.status = "running".into();
            v.note.clear();
        }
        RunEvent::Paused { reason, .. } => {
            v.status = "paused".into();
            v.note = format!("Paused: {reason}");
        }
        RunEvent::Resumed { .. } => {
            v.status = "running".into();
            v.note.clear();
        }
        RunEvent::StageDone { status, summary, .. } => {
            if matches!(status, super::job::StageStatus::Done | super::job::StageStatus::Skipped) {
                v.stages_done += 1;
            }
            v.percent = percent(v.stages_done, v.stages_total);
            v.note = summary.clone();
        }
        RunEvent::Finished { state, reason, .. } => {
            use super::job::JobState;
            v.status = match state {
                JobState::Done => "done",
                JobState::Failed => "failed",
                _ => "stopped",
            }
            .into();
            v.check_in = None;
            v.now = String::new();
            v.next = String::new();
            if *state == JobState::Done {
                v.percent = 100;
            }
            v.note = reason.clone();
        }
    }
}

// ------------------------------------------------------------------ the floating window

#[cfg(desktop)]
mod window {
    use super::LiveView;
    use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

    const LABEL: &str = "autolive";
    const W: f64 = 340.0;
    const H: f64 = 168.0;

    pub fn show(app: &AppHandle, v: &LiveView) {
        let w = match app.get_webview_window(LABEL) {
            Some(w) => w,
            None => {
                let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("auto_live.html".into()))
                    .title("BYTE is working")
                    .inner_size(W, H)
                    .decorations(false)
                    .resizable(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .focused(false)
                    .visible(false)
                    .build();
                match built {
                    Ok(w) => {
                        if let Ok(Some(m)) = app.primary_monitor() {
                            let scale = w.scale_factor().unwrap_or(1.0);
                            let (mp, ms) = (m.position(), m.size());
                            let x = mp.x as f64 + ms.width as f64 - (W + 16.0) * scale;
                            let y = mp.y as f64 + ms.height as f64 - (H + 64.0) * scale;
                            let _ = w.set_position(PhysicalPosition::new(x, y));
                        }
                        w
                    }
                    Err(e) => return log::warn!("live view: {e}"),
                }
            }
        };
        let json = serde_json::to_string(v).unwrap_or_else(|_| "{}".into());
        let _ = w.show();
        let _ = w.eval(format!("window.setLive && window.setLive({json})"));
    }

    pub fn hide(app: &AppHandle) {
        if let Some(w) = app.get_webview_window(LABEL) {
            let _ = w.close();
        }
    }
}

#[cfg(desktop)]
pub fn show(app: &tauri::AppHandle, v: &LiveView) {
    window::show(app, v);
}

#[cfg(desktop)]
pub fn hide(app: &tauri::AppHandle) {
    window::hide(app);
}

#[cfg(not(desktop))]
pub fn show(_app: &tauri::AppHandle, _v: &LiveView) {}

#[cfg(not(desktop))]
pub fn hide(_app: &tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::auto::job::{JobState, StageStatus};

    fn ev(v: &mut LiveView, e: RunEvent) {
        apply(v, &e);
    }

    fn started() -> LiveView {
        let mut v = LiveView::idle();
        ev(&mut v, RunEvent::Started { job_id: "j".into(), title: "Tidy up".into(), stages: vec!["one".into(), "two".into(), "three".into(), "four".into()] });
        v
    }

    #[test]
    fn the_view_follows_the_stages_and_the_progress_bar_moves_when_one_ends() {
        let mut v = started();
        assert_eq!((v.status.as_str(), v.next.as_str(), v.percent), ("running", "one", 0));
        ev(&mut v, RunEvent::StageStart { job_id: "j".into(), index: 0, total: 4, text: "one".into(), next: "two".into() });
        assert_eq!((v.stage, v.now.as_str(), v.next.as_str()), (1, "one", "two"));
        ev(&mut v, RunEvent::StageDone { job_id: "j".into(), index: 0, status: StageStatus::Done, summary: "ok".into() });
        assert_eq!(v.percent, 25);
        ev(&mut v, RunEvent::StageStart { job_id: "j".into(), index: 1, total: 4, text: "two".into(), next: "three".into() });
        ev(&mut v, RunEvent::StageDone { job_id: "j".into(), index: 1, status: StageStatus::Failed, summary: "no".into() });
        assert_eq!(v.percent, 25, "a failed stage is not progress");
    }

    #[test]
    fn a_step_is_listed_while_it_runs_and_then_marked() {
        let mut v = started();
        ev(&mut v, RunEvent::Step { job_id: "j".into(), app: "notepad".into(), what: "Click item 2".into(), ok: None, detail: String::new() });
        assert_eq!((v.now.as_str(), v.feed.len(), v.feed[0].ok), ("Click item 2", 1, None));
        ev(&mut v, RunEvent::Step { job_id: "j".into(), app: "notepad".into(), what: "Click item 2".into(), ok: Some(true), detail: String::new() });
        assert_eq!((v.feed.len(), v.feed[0].ok), (1, Some(true)));
    }

    #[test]
    fn the_feed_keeps_only_the_newest_steps() {
        let mut v = started();
        for i in 0..200 {
            ev(&mut v, RunEvent::Step { job_id: "j".into(), app: "a".into(), what: format!("Step {i}"), ok: Some(true), detail: String::new() });
        }
        assert_eq!(v.feed.len(), FEED_MAX);
        assert_eq!(v.feed.last().unwrap().what, "Step 199");
    }

    #[test]
    fn a_check_in_shows_until_answered_and_pause_shows_its_reason() {
        let mut v = started();
        ev(&mut v, RunEvent::CheckIn { job_id: "j".into(), id: "c".into(), title: "Outside".into(), fields: vec![], can_remember: false });
        assert_eq!(v.status, "waiting");
        assert!(v.check_in.is_some());
        ev(&mut v, RunEvent::CheckInDone { job_id: "j".into(), id: "c".into() });
        assert!(v.check_in.is_none() && v.status == "running");
        ev(&mut v, RunEvent::Paused { job_id: "j".into(), reason: "you started using the PC".into() });
        assert!(v.active() && v.note.contains("you started using the PC"));
        ev(&mut v, RunEvent::Resumed { job_id: "j".into() });
        assert_eq!(v.status, "running");
    }

    #[test]
    fn finishing_sets_the_end_state_and_a_done_job_is_at_100() {
        let mut v = started();
        ev(&mut v, RunEvent::Finished { job_id: "j".into(), state: JobState::Done, reason: String::new() });
        assert_eq!((v.status.as_str(), v.percent, v.active()), ("done", 100, false));
        let mut v = started();
        ev(&mut v, RunEvent::Finished { job_id: "j".into(), state: JobState::Stopped, reason: "you pressed Stop".into() });
        assert_eq!((v.status.as_str(), v.note.as_str()), ("stopped", "you pressed Stop"));
    }
}
