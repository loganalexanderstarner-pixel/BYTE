//! How a plan is asked for and read back: the retry loop (with the exact error given to the model), where the question goes
//! (this PC's model, or BYTE Cloud when the person allowed it), and what is sent. The loop takes the model as a trait so the
//! same code runs in the app and in the self-test with a stand-in model.

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::actions::{self, Step};
use super::brain;
use super::plancheck;
use super::uia::Snapshot;

/// Tries at a usable plan (the first, plus corrections that carry the exact error).
pub const ATTEMPTS: u32 = 3;

/// Starts an error that means "the model says this cannot be done" (not a failure to plan).
pub const CANNOT: &str = "BYTE can't do that: ";

/// Whatever answers the planning question.
pub trait Asker: Send + Sync {
    fn ask<'a>(&'a self, messages: &'a [Value]) -> BoxFuture<'a, Result<String, String>>;
}

/// What a plan is checked against.
#[derive(Clone, Copy)]
pub struct Check<'a> {
    pub snap: &'a Snapshot,
    pub windows: &'a [String],
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Plan(Vec<Step>),
    /// The model says the goal cannot (or should not) be done, and why.
    Cannot(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub outcome: Outcome,
    /// Questions asked (1 = right the first time).
    pub attempts: u32,
}

/// Asks, reads, checks; on a refused reply asks again with the exact error, up to `attempts` times.
pub async fn plan_loop(asker: &dyn Asker, system: &str, user: &str, check: Option<Check<'_>>, limit: usize, attempts: u32) -> Planned {
    let mut messages = vec![json!({"role":"system","content":system}), json!({"role":"user","content":user})];
    let mut last = String::from("no reply");
    for n in 1..=attempts.max(1) {
        let reply = match asker.ask(&messages).await {
            Ok(r) => r,
            Err(e) => return Planned { outcome: Outcome::Failed(e), attempts: n },
        };
        if let Some(why) = plancheck::cannot_reason(&reply) {
            return Planned { outcome: Outcome::Cannot(why), attempts: n };
        }
        let checked = actions::parse_plan_with(&reply, limit).and_then(|steps| match check {
            Some(c) => plancheck::check_plan(steps, c.snap, c.windows),
            None => Ok(steps),
        });
        match checked {
            Ok(steps) => return Planned { outcome: Outcome::Plan(steps), attempts: n },
            Err(e) => {
                last = e.to_string();
                messages.push(json!({"role":"assistant","content":reply}));
                messages.push(json!({"role":"user","content":plancheck::fix_text(&last, limit)}));
            }
        }
    }
    Planned { outcome: Outcome::Failed(last), attempts: attempts.max(1) }
}

// ------------------------------------------------------------------ where the question goes

/// The setting "Ask where".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Local,
    Cloud,
    Auto,
}

pub fn parse_where(s: &str) -> Where {
    match s.trim().to_lowercase().as_str() {
        "cloud" => Where::Cloud,
        "auto" => Where::Auto,
        _ => Where::Local,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Local,
    Cloud,
}

/// What the choice depends on besides the setting.
#[derive(Debug, Clone, Copy)]
pub struct Cloud {
    /// "Plan with BYTE Cloud" is on.
    pub allowed: bool,
    /// The person confirmed once that the window's button names are sent.
    pub confirmed: bool,
    /// Connected, and the window may be sent (not a private or excluded window).
    pub ready: bool,
}

/// Local unless the person chose Cloud (or Auto with a planner that is too small) AND allowed cloud planning. The reason is for the log.
pub fn decide(w: Where, cloud: Cloud, planner_b: Option<f32>) -> (Route, Option<&'static str>) {
    let can = cloud.allowed && cloud.confirmed;
    match w {
        Where::Local => (Route::Local, None),
        Where::Cloud | Where::Auto if !can => (Route::Local, Some("cloud planning is off, so the plan was made on this PC")),
        Where::Cloud | Where::Auto if !cloud.ready => (Route::Local, Some("BYTE Cloud is not available for this window, so the plan was made on this PC")),
        Where::Cloud => (Route::Cloud, None),
        Where::Auto => match planner_b {
            Some(b) if b < brain::MIN_PLANNER_B => (Route::Cloud, None),
            _ => (Route::Local, None),
        },
    }
}

/// A conversation as one system text and one user text (the cloud takes a single message): the retries are folded in as
/// "your earlier reply" and "what was wrong".
pub fn flatten(messages: &[Value]) -> (String, String) {
    let mut system = String::new();
    let mut user = String::new();
    for m in messages {
        let text = m["content"].as_str().unwrap_or("");
        match m["role"].as_str() {
            Some("system") => system.push_str(text),
            Some("assistant") => user.push_str(&format!("\n\nYour earlier reply:\n{text}")),
            _ => {
                if !user.is_empty() {
                    user.push_str("\n\n");
                }
                user.push_str(text);
            }
        }
    }
    (system, user)
}

/// The prompt the cloud is sent: nothing but the goal and the window's item names (what the setting says), never typed text from
/// the screen, secret fields or the window's content.
pub fn cloud_payload_note() -> &'static str {
    "the goal and the names of the buttons and boxes in the window in front"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::uia::fake::{el, snap};
    use crate::control::uia::Kind;
    use std::sync::Mutex;

    /// A model that answers from a script and remembers what it was asked.
    struct Stub {
        replies: Mutex<Vec<Result<String, String>>>,
        seen: Mutex<Vec<Vec<Value>>>,
    }

    impl Stub {
        fn new(r: &[&str]) -> Self {
            Stub { replies: Mutex::new(r.iter().rev().map(|s| Ok(s.to_string())).collect()), seen: Mutex::default() }
        }
    }

    impl Asker for Stub {
        fn ask<'a>(&'a self, messages: &'a [Value]) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move {
                self.seen.lock().unwrap().push(messages.to_vec());
                self.replies.lock().unwrap().pop().unwrap_or_else(|| Err("no more replies".into()))
            })
        }
    }

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    #[test]
    fn a_good_first_reply_is_one_question() {
        let s = Stub::new(&[r#"{"steps":[{"action":"open","target":"notepad"}]}"#]);
        let p = run(plan_loop(&s, "sys", "open notepad", None, 12, ATTEMPTS));
        assert_eq!(p.attempts, 1);
        assert!(matches!(p.outcome, Outcome::Plan(ref v) if v.len() == 1));
    }

    #[test]
    fn a_click_on_a_missing_item_is_sent_back_with_the_exact_error_and_then_fixed() {
        let sn = snap("Calc", vec![el(1, "Seven", Kind::Button), el(2, "Plus", Kind::Button)]);
        let s = Stub::new(&[r#"{"steps":[{"action":"click","id":9}]}"#, r#"{"steps":[{"action":"click","id":2}]}"#]);
        let w: Vec<String> = vec![];
        let p = run(plan_loop(&s, "sys", "press plus", Some(Check { snap: &sn, windows: &w }), 12, ATTEMPTS));
        assert_eq!(p.attempts, 2);
        let seen = s.seen.lock().unwrap();
        let retry = seen[1].last().unwrap()["content"].as_str().unwrap().to_string();
        assert!(retry.contains("step 1: item 9 is not in the list") && retry.contains("2 button \"Plus\""), "{retry}");
    }

    #[test]
    fn a_plan_over_the_limit_gets_a_shortening_prompt() {
        let many = format!(r#"{{"steps":[{}]}}"#, vec![r#"{"action":"key","combo":"tab"}"#; 5].join(","));
        let s = Stub::new(&[&many, r#"{"steps":[{"action":"key","combo":"tab"}]}"#]);
        let p = run(plan_loop(&s, "sys", "x", None, 3, ATTEMPTS));
        assert_eq!(p.attempts, 2);
        let retry = s.seen.lock().unwrap()[1].last().unwrap()["content"].as_str().unwrap().to_string();
        assert!(retry.contains("the plan has 5 steps; at most 3") && retry.contains("at most 3 steps"), "{retry}");
    }

    #[test]
    fn a_model_that_keeps_failing_gives_up_with_the_last_error() {
        let s = Stub::new(&["nonsense", "still nonsense", "no"]);
        let p = run(plan_loop(&s, "sys", "x", None, 12, 3));
        assert_eq!(p.attempts, 3);
        assert!(matches!(p.outcome, Outcome::Failed(ref e) if e.contains("no JSON plan")));
    }

    #[test]
    fn saying_it_cannot_is_an_answer_not_a_failure() {
        let s = Stub::new(&[r#"{"steps":[],"cannot":"needs a password"}"#]);
        let p = run(plan_loop(&s, "sys", "log in", None, 12, 3));
        assert_eq!(p, Planned { outcome: Outcome::Cannot("needs a password".into()), attempts: 1 });
    }

    #[test]
    fn a_broken_model_stops_at_once() {
        let s = Stub { replies: Mutex::new(vec![Err("the control model failed (500)".into())]), seen: Mutex::default() };
        let p = run(plan_loop(&s, "sys", "x", None, 12, 3));
        assert_eq!(p.attempts, 1);
        assert!(matches!(p.outcome, Outcome::Failed(_)));
    }

    const ALL: Cloud = Cloud { allowed: true, confirmed: true, ready: true };

    #[test]
    fn local_is_the_default_and_cloud_needs_the_toggle_and_the_one_time_confirmation() {
        assert_eq!(parse_where(""), Where::Local);
        assert_eq!(parse_where("Cloud"), Where::Cloud);
        assert_eq!(parse_where("auto"), Where::Auto);
        assert_eq!(decide(Where::Local, ALL, Some(0.6)).0, Route::Local);
        assert_eq!(decide(Where::Cloud, ALL, Some(14.0)).0, Route::Cloud);
        assert_eq!(decide(Where::Cloud, Cloud { allowed: false, ..ALL }, None), (Route::Local, Some("cloud planning is off, so the plan was made on this PC")));
        assert_eq!(decide(Where::Cloud, Cloud { confirmed: false, ..ALL }, None).0, Route::Local);
        assert_eq!(decide(Where::Cloud, Cloud { ready: false, ..ALL }, None).0, Route::Local);
    }

    #[test]
    fn auto_goes_to_the_cloud_only_when_the_planner_is_too_small_and_the_toggle_is_on() {
        assert_eq!(decide(Where::Auto, ALL, Some(0.6)).0, Route::Cloud);
        assert_eq!(decide(Where::Auto, ALL, Some(brain::MIN_PLANNER_B)).0, Route::Local);
        assert_eq!(decide(Where::Auto, ALL, Some(14.0)).0, Route::Local);
        assert_eq!(decide(Where::Auto, ALL, None).0, Route::Local, "an unknown size is not blamed");
        assert_eq!(decide(Where::Auto, Cloud { allowed: false, ..ALL }, Some(0.6)).0, Route::Local);
    }

    #[test]
    fn a_retry_conversation_folds_into_one_message_for_the_cloud() {
        let m = vec![json!({"role":"system","content":"rules"}), json!({"role":"user","content":"Goal: x"}), json!({"role":"assistant","content":"{bad"}), json!({"role":"user","content":"That plan was refused: why"})];
        let (s, u) = flatten(&m);
        assert_eq!(s, "rules");
        assert!(u.starts_with("Goal: x") && u.contains("Your earlier reply:\n{bad") && u.ends_with("That plan was refused: why"), "{u}");
    }
}
