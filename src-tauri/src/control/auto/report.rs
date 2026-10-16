//! The morning report: when a job ends (or is stopped) BYTE writes what it did, stage by stage, what it read (titles and addresses
//! only), what it sent, what it skipped and why, and how much time it used.

use super::job::{Job, JobState, StageStatus};
use super::limit;

fn state_line(j: &Job) -> String {
    match j.state {
        JobState::Done => "Everything on the list is done.".into(),
        JobState::Stopped if j.stop_reason.is_empty() => "It was stopped before it finished.".into(),
        JobState::Stopped => format!("It stopped before it finished: {}.", j.stop_reason.trim_end_matches('.')),
        JobState::Failed => format!("It could not finish: {}.", j.stop_reason.trim_end_matches('.')),
        JobState::Paused => "It is paused and waiting for you.".into(),
        JobState::Running => "It is still running.".into(),
        JobState::Planned => "It has not started.".into(),
    }
}

fn dedup(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in items {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out
}

/// The full written report, as Markdown.
pub fn build(j: &Job) -> String {
    let mut o = format!("# Report: {}\n\n{}\n\n", j.title, state_line(j));
    o.push_str(&format!(
        "**Time used:** {}  \n**Steps taken:** {}  \n**Messages sent:** {}\n\n",
        limit::say_time(Some((j.used_ms / 1000).max(1) * 1000)),
        j.steps,
        j.messages
    ));
    o.push_str("## What happened, stage by stage\n\n");
    for (i, s) in j.stages.iter().enumerate() {
        let (mark, note) = match s.status {
            StageStatus::Done => ("Done", s.summary.clone()),
            StageStatus::Skipped => ("Skipped", s.reason.clone()),
            StageStatus::Failed => ("Did not finish", if s.reason.is_empty() { s.summary.clone() } else { s.reason.clone() }),
            StageStatus::Running => ("Interrupted", String::new()),
            StageStatus::Pending => ("Not started", String::new()),
        };
        o.push_str(&format!("{}. **{}** ({mark})", i + 1, s.text));
        if !note.is_empty() {
            o.push_str(&format!(": {note}"));
        }
        o.push('\n');
    }
    let read = dedup(j.stages.iter().flat_map(|s| s.read.clone()));
    o.push_str("\n## What I read\n\n");
    if read.is_empty() {
        o.push_str("Nothing outside your own programs.\n");
    } else {
        for r in read {
            o.push_str(&format!("- {r}\n"));
        }
    }
    o.push_str("\n## What I sent\n\n");
    let sent: Vec<String> = j.stages.iter().flat_map(|s| s.sent.iter().map(|x| format!("- To {}: {}", x.to, x.what))).collect();
    if sent.is_empty() {
        o.push_str("Nothing.\n");
    } else {
        o.push_str(&sent.join("\n"));
        o.push('\n');
    }
    let files = dedup(j.stages.iter().flat_map(|s| s.files.clone()));
    if !files.is_empty() {
        o.push_str("\n## Files I wrote\n\n");
        for f in files {
            o.push_str(&format!("- {f}\n"));
        }
    }
    o.push_str("\n## What I skipped, and why\n\n");
    let skipped: Vec<String> = j
        .stages
        .iter()
        .filter(|s| matches!(s.status, StageStatus::Skipped | StageStatus::Failed | StageStatus::Pending))
        .map(|s| {
            let why = if !s.reason.is_empty() {
                s.reason.clone()
            } else if s.status == StageStatus::Pending {
                if j.stop_reason.is_empty() { "the job ended first".into() } else { j.stop_reason.clone() }
            } else {
                "no reason was given".into()
            };
            format!("- {}: {why}", s.text)
        })
        .collect();
    if skipped.is_empty() {
        o.push_str("Nothing was skipped.\n");
    } else {
        o.push_str(&skipped.join("\n"));
        o.push('\n');
    }
    o.push_str(&format!(
        "\n## Limits it ran under\n\nPrograms: {}. Folders: {}. May message: {}. Never delete: {}. Never spend: yes. Time: {}. Steps: {}. Messages: {}.\n",
        or_none(&j.contract.apps),
        or_none(&j.contract.folders),
        or_none(&j.contract.may_message),
        if j.contract.never_delete { "yes" } else { "no" },
        limit::say_time(j.contract.time_cap_ms()),
        limit::say_count(j.contract.step_cap()),
        limit::say_count(j.contract.message_cap()),
    ));
    o.push_str("\nThe step-by-step trail (what was clicked, never what was typed) is in Settings, Control, What BYTE did.\n");
    o
}

fn or_none(v: &[String]) -> String {
    if v.is_empty() { "none".into() } else { v.join(", ") }
}

/// A short version to read aloud.
pub fn speakable(j: &Job) -> String {
    let done = j.stages.iter().filter(|s| s.status == StageStatus::Done).count();
    let mut t = format!("Your job \"{}\" ended. {done} of {} stages are done. ", j.title, j.stages.len());
    t.push_str(&state_line(j));
    let sent: usize = j.stages.iter().map(|s| s.sent.len()).sum();
    if sent > 0 {
        t.push_str(&format!(" I sent {sent} message{}.", if sent == 1 { "" } else { "s" }));
    }
    t.push_str(" The full report is in your chats.");
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::auto::contract::Contract;
    use crate::control::auto::job::{Job, StageDraft};
    use crate::control::auto::tool::Sent;

    fn job() -> Job {
        let d = |t: &str| StageDraft { text: t.into(), tool: "auto".into() };
        let mut j = Job::new("tidy up and tell mom", vec![d("Read the inbox"), d("Message mom"), d("Clean the desktop")], Contract::default());
        j.stages[0].status = StageStatus::Done;
        j.stages[0].summary = "Read 3 emails.".into();
        j.stages[0].read = vec!["Invoice June (mail.example.com)".into(), "Invoice June (mail.example.com)".into()];
        j.stages[1].status = StageStatus::Done;
        j.stages[1].sent = vec![Sent { to: "mom".into(), what: "a short note".into() }];
        j.stages[2].status = StageStatus::Skipped;
        j.stages[2].reason = "the contract says never delete".into();
        j.used_ms = 125_000;
        j.steps = 14;
        j.messages = 1;
        j.mark_finished(JobState::Done, "");
        j
    }

    #[test]
    fn the_report_names_what_was_done_read_sent_and_skipped() {
        let r = build(&job());
        assert!(r.contains("1. **Read the inbox** (Done): Read 3 emails."));
        assert_eq!(r.matches("Invoice June").count(), 1, "what was read is listed once");
        assert!(r.contains("- To mom: a short note"));
        assert!(r.contains("Clean the desktop: the contract says never delete"));
        assert!(r.contains("**Time used:** 2 min") || r.contains("**Time used:** 125 s"), "{r}");
        assert!(r.contains("Everything on the list is done."));
    }

    #[test]
    fn a_stopped_job_says_why_and_lists_the_stages_it_never_reached() {
        let mut j = job();
        j.stages[2].status = StageStatus::Pending;
        j.stages[2].reason.clear();
        j.mark_finished(JobState::Stopped, "the time budget is used up");
        let r = build(&j);
        assert!(r.contains("It stopped before it finished: the time budget is used up."));
        assert!(r.contains("Clean the desktop: the time budget is used up"));
        assert!(r.contains("(Not started)"));
    }

    #[test]
    fn the_spoken_version_is_short_and_points_to_the_chat() {
        let t = speakable(&job());
        assert!(t.contains("2 of 3 stages") && t.contains("I sent 1 message.") && t.contains("in your chats"));
    }
}
