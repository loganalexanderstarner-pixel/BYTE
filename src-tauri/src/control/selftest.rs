//! The PC-control test: about 40 standard tasks (open a program, switch a window, type, click a named button, change a setting,
//! find a file, several steps, and goals that should be refused), each with a window to plan against and a plain rule for what a
//! right plan contains. A model is asked for each plan exactly as in real use (same prompt, same checks, same retries), the plan is
//! judged by the rule, and nothing is ever clicked or typed. The score is kept per model on this PC and shown in the model
//! catalog and Settings.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::actions::{Action, Step};
use super::brain;
use super::planner::{self, Asker, Outcome};
use super::uia::{self, Element, Kind, Rect, Snapshot};

const TASKS_JSON: &str = include_str!("selftest_tasks.json");
pub const FILE: &str = "control_scores.json";
pub const EVENT: &str = "control://selftest";

#[derive(Debug, Clone, Deserialize)]
struct ScreenDef {
    app: String,
    title: String,
    elements: Vec<(u32, String, String)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Task {
    pub id: String,
    pub category: String,
    pub goal: String,
    pub screen: String,
    #[serde(default)]
    pub windows: Option<Vec<String>>,
    /// Steps a right plan has, in this order (other steps may be between). `kind:text` (the text is contained, any case); `a|b`
    /// accepts either; `any:text` matches any step that mentions the text.
    #[serde(default)]
    pub check: Vec<String>,
    /// Steps a right plan never has (same form).
    #[serde(default)]
    pub forbid: Vec<String>,
    /// The right answer is "this cannot be done".
    #[serde(default)]
    pub cannot: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct Data {
    screens: BTreeMap<String, ScreenDef>,
    windows: Vec<String>,
    tasks: Vec<Task>,
}

pub struct Suite {
    data: Data,
}

fn kind_from(s: &str) -> Kind {
    match s {
        "button" => Kind::Button,
        "edit" => Kind::Edit,
        "checkbox" => Kind::CheckBox,
        "radio" => Kind::RadioButton,
        "dropdown" => Kind::ComboBox,
        "item" => Kind::ListItem,
        "menu" => Kind::MenuItem,
        "tab" => Kind::TabItem,
        "link" => Kind::Link,
        "tree" => Kind::TreeItem,
        "slider" => Kind::Slider,
        _ => Kind::Other,
    }
}

impl Suite {
    pub fn standard() -> Suite {
        Suite { data: serde_json::from_str(TASKS_JSON).expect("selftest_tasks.json is valid") }
    }

    pub fn tasks(&self) -> &[Task] {
        &self.data.tasks
    }

    /// The window a task is planned against.
    pub fn snapshot(&self, t: &Task) -> Snapshot {
        let d = &self.data.screens[&t.screen];
        Snapshot {
            app: d.app.clone(),
            title: d.title.clone(),
            elements: d.elements.iter().map(|(id, name, kind)| Element { id: *id, name: name.clone(), kind: kind_from(kind), enabled: true, rect: Rect { left: 0, top: 0, right: 10, bottom: 10 }, sensitive: false }).collect(),
        }
    }

    pub fn windows(&self, t: &Task) -> Vec<String> {
        t.windows.clone().unwrap_or_else(|| self.data.windows.clone())
    }
}

/// What each step does, as `(kind, text)` lower-case: a click's text is the name of the item it points at in the window the
/// plan was made for (empty after the window changed, when the name is not known).
fn facts(steps: &[Step], snap: &Snapshot) -> Vec<(&'static str, String)> {
    let mut fresh = true;
    let mut out = Vec::new();
    for s in steps {
        out.push(match &s.action {
            Action::Click { id } => ("click", snap.find(*id).filter(|_| fresh).map(|e| e.name.to_lowercase()).unwrap_or_default()),
            Action::Open { target } => ("open", target.to_lowercase()),
            Action::Focus { title } => ("focus", title.to_lowercase()),
            Action::Type { text } => ("type", text.to_lowercase()),
            Action::Key { combo } => ("key", combo.to_lowercase()),
            Action::Close { title } => ("close", title.to_lowercase()),
            Action::Wait { text, .. } => ("wait", text.to_lowercase()),
            Action::Scroll { .. } => ("scroll", String::new()),
            Action::Power { .. } => ("power", String::new()),
        });
        if super::plancheck::changes_view(s) {
            fresh = false;
        }
    }
    out
}

fn matches_cond(cond: &str, kind: &str, text: &str) -> bool {
    cond.split('|').any(|alt| match alt.split_once(':') {
        Some(("any", t)) => text.contains(&t.to_lowercase()),
        Some((k, t)) => k == kind && text.contains(&t.to_lowercase()),
        None => alt == kind,
    })
}

/// Whether a plan is right for the task by its rule: every wanted step in order, none of the forbidden ones.
pub fn plan_is_right(task: &Task, steps: &[Step], snap: &Snapshot) -> Result<(), String> {
    let f = facts(steps, snap);
    for bad in &task.forbid {
        if let Some((k, t)) = f.iter().find(|(k, t)| matches_cond(bad, k, t)) {
            return Err(format!("it should not {k} {t}").trim().to_string());
        }
    }
    let mut at = 0usize;
    for want in &task.check {
        match f[at..].iter().position(|(k, t)| matches_cond(want, k, t)) {
            Some(i) => at += i + 1,
            None => return Err(format!("missing or out of order: {want}")),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryScore {
    pub category: String,
    pub correct: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    /// The model key that was tested ("id:quant").
    pub model: String,
    pub total: u32,
    /// Replies that became a usable plan (or a clear "cannot").
    pub valid: u32,
    /// Plans that also did the right thing by the task's rule.
    pub correct: u32,
    /// `correct` as a percentage, 0 to 100.
    pub percent: u32,
    /// Questions asked per task on average (1 = right first time).
    pub avg_attempts: f32,
    pub at: String,
    pub by_category: Vec<CategoryScore>,
    /// Up to eight tasks that went wrong, "id: why".
    pub misses: Vec<String>,
}

/// Runs every task through the real planning loop with `asker`, calling `progress(done, total)` after each.
pub async fn run_suite(suite: &Suite, asker: &dyn Asker, model: &str, progress: &(dyn Fn(usize, usize) + Send + Sync), stop: &(dyn Fn() -> bool + Send + Sync)) -> Score {
    let total = suite.tasks().len();
    let system = brain::system_text("");
    let (mut valid, mut correct, mut attempts, mut done) = (0u32, 0u32, 0u32, 0u32);
    let mut cats: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut misses = Vec::new();
    for task in suite.tasks() {
        if stop() {
            break;
        }
        let snap = suite.snapshot(task);
        let windows = suite.windows(task);
        let user = brain::plan_prompt(&task.goal, &uia::render(&snap));
        let p = planner::plan_loop(asker, &system, &user, Some(planner::Check { snap: &snap, windows: &windows }), super::actions::MAX_STEPS, planner::ATTEMPTS).await;
        attempts += p.attempts;
        done += 1;
        let verdict: Result<(), String> = match (&p.outcome, task.cannot) {
            (Outcome::Cannot(_), true) => {
                valid += 1;
                Ok(())
            }
            (Outcome::Cannot(_), false) => {
                valid += 1;
                Err("it said it could not".into())
            }
            (Outcome::Plan(_), true) => {
                valid += 1;
                Err("it should have said it cannot".into())
            }
            (Outcome::Plan(steps), false) => {
                valid += 1;
                plan_is_right(task, steps, &snap)
            }
            (Outcome::Failed(e), _) => Err(format!("no usable plan ({e})")),
        };
        let c = cats.entry(task.category.clone()).or_default();
        c.1 += 1;
        match verdict {
            Ok(()) => {
                correct += 1;
                c.0 += 1;
            }
            Err(why) => {
                if misses.len() < 8 {
                    misses.push(format!("{}: {why}", task.id));
                }
            }
        }
        progress(done as usize, total);
    }
    let asked = done.max(1);
    Score {
        model: model.to_string(),
        total: done,
        valid,
        correct,
        percent: if done == 0 { 0 } else { (correct * 100 + done / 2) / done },
        avg_attempts: attempts as f32 / asked as f32,
        at: chrono::Local::now().format("%Y-%m-%d").to_string(),
        by_category: cats.into_iter().map(|(category, (correct, total))| CategoryScore { category, correct, total }).collect(),
        misses,
    }
}

// ------------------------------------------------------------------ kept on this PC

/// Scores by catalog model id ("qwen3.5-4b").
pub fn load_scores(path: &Path) -> BTreeMap<String, Score> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_score(path: &Path, model_id: &str, score: &Score) -> Result<(), String> {
    let mut all = load_scores(path);
    all.insert(model_id.to_string(), score.clone());
    std::fs::write(path, serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::BoxFuture;
    use serde_json::{json, Value};
    use std::sync::Mutex;

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    /// The plan a perfect model would write for a task, built from its own rule (first alternative of each wanted step).
    fn oracle(suite: &Suite, t: &Task) -> String {
        if t.cannot {
            return r#"{"steps":[],"cannot":"not something I can do"}"#.into();
        }
        let snap = suite.snapshot(t);
        let steps: Vec<Value> = t
            .check
            .iter()
            .map(|c| {
                let alt = c.split('|').next().unwrap();
                let (k, text) = alt.split_once(':').unwrap();
                match k {
                    "open" => json!({"action":"open","target":text}),
                    "focus" | "any" => json!({"action":"focus","title":suite.windows(t).iter().find(|w| w.to_lowercase().contains(text)).cloned().unwrap_or(text.to_string())}),
                    "type" => json!({"action":"type","text":text}),
                    "key" => json!({"action":"key","combo":text}),
                    "close" => json!({"action":"close","title":text}),
                    "click" => json!({"action":"click","id":snap.elements.iter().find(|e| e.name.to_lowercase().contains(text)).unwrap_or_else(|| panic!("{}: no item {text}", t.id)).id}),
                    _ => json!({"action":"key","combo":"tab"}),
                }
            })
            .collect();
        json!({ "steps": steps }).to_string()
    }

    /// A stand-in model: answers each task with `reply(goal)`.
    struct Stub<F: Fn(&str) -> String + Send + Sync>(F, Mutex<u32>);

    impl<F: Fn(&str) -> String + Send + Sync> Asker for Stub<F> {
        fn ask<'a>(&'a self, messages: &'a [Value]) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move {
                *self.1.lock().unwrap() += 1;
                let user = messages[1]["content"].as_str().unwrap_or("");
                let goal = user.lines().next().unwrap_or("").trim_start_matches("Goal: ").to_string();
                Ok((self.0)(&goal))
            })
        }
    }

    fn score_of(f: impl Fn(&str) -> String + Send + Sync) -> Score {
        let suite = Suite::standard();
        run(run_suite(&suite, &Stub(f, Mutex::new(0)), "stub:Q4", &|_, _| {}, &|| false))
    }

    #[test]
    fn the_suite_has_about_forty_tasks_in_every_category_and_every_screen_exists() {
        let s = Suite::standard();
        assert!((38..=50).contains(&s.tasks().len()), "{}", s.tasks().len());
        let cats: std::collections::BTreeSet<&str> = s.tasks().iter().map(|t| t.category.as_str()).collect();
        for c in ["open an app", "switch a window", "type text", "click a named button", "change a setting", "find a file", "several steps", "says it cannot"] {
            assert!(cats.contains(c), "{c}");
        }
        let ids: std::collections::BTreeSet<&str> = s.tasks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), s.tasks().len(), "unique ids");
        for t in s.tasks() {
            assert!(!s.snapshot(t).elements.is_empty(), "{}", t.id);
            assert!(t.cannot || !t.check.is_empty(), "{} needs a rule", t.id);
        }
    }

    #[test]
    fn a_perfect_model_scores_100_and_every_task_can_be_passed() {
        let suite = Suite::standard();
        let by_goal: BTreeMap<String, String> = suite.tasks().iter().map(|t| (t.goal.clone(), oracle(&suite, t))).collect();
        let s = score_of(|g| by_goal[g].clone());
        assert_eq!((s.total, s.valid, s.correct, s.percent), (suite.tasks().len() as u32, suite.tasks().len() as u32, suite.tasks().len() as u32, 100), "{:?}", s.misses);
        assert!((s.avg_attempts - 1.0).abs() < f32::EPSILON);
        assert!(s.misses.is_empty());
    }

    #[test]
    fn a_model_that_only_writes_prose_scores_zero_and_asks_three_times_per_task() {
        let s = score_of(|_| "Sure, I will open it for you!".into());
        assert_eq!((s.valid, s.correct, s.percent), (0, 0, 0));
        assert!((s.avg_attempts - 3.0).abs() < f32::EPSILON);
        assert_eq!(s.misses.len(), 8);
        assert!(s.misses[0].contains("no usable plan"));
    }

    #[test]
    fn a_model_that_always_opens_notepad_is_valid_but_mostly_wrong() {
        let s = score_of(|_| r#"{"steps":[{"action":"open","target":"notepad"}]}"#.into());
        assert_eq!(s.valid, s.total);
        assert!(s.correct >= 1 && s.percent < 15, "{}%", s.percent);
        let open = s.by_category.iter().find(|c| c.category == "open an app").unwrap();
        assert_eq!(open.correct, 1, "only the Notepad task is right");
    }

    #[test]
    fn saying_cannot_is_right_only_for_the_tasks_that_should_be_refused() {
        let s = score_of(|_| r#"{"steps":[],"cannot":"no"}"#.into());
        let cannot = s.by_category.iter().find(|c| c.category == "says it cannot").unwrap();
        assert_eq!((cannot.correct, cannot.total), (3, 3));
        assert_eq!(s.correct, 3);
    }

    #[test]
    fn a_wrong_item_number_is_corrected_by_the_retry_and_counted_as_two_questions() {
        let suite = Suite::standard();
        let t = suite.tasks().iter().find(|t| t.goal == "press Plus").unwrap().clone();
        let good = oracle(&suite, &t);
        let calls = Mutex::new(0);
        let stub = Stub(move |g: &str| if g == "press Plus" { let mut c = calls.lock().unwrap(); *c += 1; if *c == 1 { r#"{"steps":[{"action":"click","id":42}]}"#.to_string() } else { good.clone() } } else { r#"{"steps":[{"action":"key","combo":"tab"}]}"#.into() }, Mutex::new(0));
        let tiny = Suite { data: Data { screens: suite.data.screens.clone(), windows: suite.data.windows.clone(), tasks: vec![t] } };
        let s = run(run_suite(&tiny, &stub, "m", &|_, _| {}, &|| false));
        assert_eq!((s.correct, s.total), (1, 1));
        assert!((s.avg_attempts - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn forbidden_steps_and_order_are_judged() {
        let suite = Suite::standard();
        let t = suite.tasks().iter().find(|t| t.goal == "open Notepad, write hi and save it").unwrap();
        let snap = suite.snapshot(t);
        let steps = |j: &str| crate::control::actions::parse_plan(j).unwrap();
        assert!(plan_is_right(t, &steps(r#"{"steps":[{"action":"open","target":"notepad"},{"action":"type","text":"hi"},{"action":"key","combo":"ctrl+s"}]}"#), &snap).is_ok());
        assert!(plan_is_right(t, &steps(r#"{"steps":[{"action":"type","text":"hi"},{"action":"open","target":"notepad"},{"action":"key","combo":"ctrl+s"}]}"#), &snap).is_err(), "out of order");
        assert!(plan_is_right(t, &steps(r#"{"steps":[{"action":"open","target":"notepad"},{"action":"type","text":"hi"},{"action":"key","combo":"ctrl+s"},{"action":"power","kind":"shutdown"}]}"#), &snap).unwrap_err().contains("power"));
    }

    #[test]
    fn stopping_ends_the_run_with_what_was_done() {
        let suite = Suite::standard();
        let n = Mutex::new(0);
        let s = run(run_suite(&suite, &Stub(|_| r#"{"steps":[{"action":"open","target":"notepad"}]}"#.into(), Mutex::new(0)), "m", &|_, _| {}, &|| {
            let mut c = n.lock().unwrap();
            *c += 1;
            *c > 5
        }));
        assert_eq!(s.total, 5);
    }

    #[test]
    fn scores_are_kept_per_model_in_a_file() {
        let dir = std::env::temp_dir().join(format!("byte-scores-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(FILE);
        assert!(load_scores(&f).is_empty());
        let s = score_of(|_| "x".into());
        save_score(&f, "qwen3.5-4b", &s).unwrap();
        save_score(&f, "qwen3.5-9b", &Score { percent: 90, ..s.clone() }).unwrap();
        let all = load_scores(&f);
        assert_eq!((all.len(), all["qwen3.5-9b"].percent, all["qwen3.5-4b"].percent), (2, 90, 0));
        let _ = std::fs::remove_dir_all(dir);
    }
}
