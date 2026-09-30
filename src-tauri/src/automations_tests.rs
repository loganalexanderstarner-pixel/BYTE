use super::*;
use chrono::Utc;
use std::sync::Mutex;

fn ask(p: &str) -> Step {
    Step::Ask { prompt: p.into() }
}

#[test]
fn steps_and_triggers_are_read_from_words() {
    let d = plan("Every weekday at 8am, find the latest AI news, then summarize it in five bullets, then save it to a file called AI news").unwrap();
    assert_eq!(d.trigger, "weekdays 08:00");
    assert_eq!(d.steps, vec![ask("find the latest AI news"), ask("summarize it in five bullets"), Step::SaveFile { name: "AI news".into() }]);
    assert_eq!(d.name, "Find the latest AI news");

    let d = plan("research the best budget espresso machines, then write a short buying guide and save it to a file").unwrap();
    assert_eq!(d.trigger, "manual");
    assert_eq!(d.steps.len(), 3);
    assert_eq!(d.steps[2], Step::SaveFile { name: d.name.clone() });

    let d = plan("when BYTE opens, give me my briefing and send me a notification").unwrap();
    assert_eq!(d.trigger, "launch");
    assert_eq!(d.steps, vec![Step::Briefing, Step::Notify { title: "Daily briefing".into(), body: "{previous}".into() }]);

    let d = plan("every morning check the weather in Pittsburgh then add it to my to-do list").unwrap();
    assert_eq!(d.trigger, "daily 07:30");
    assert_eq!(d.steps, vec![ask("check the weather in Pittsburgh"), Step::AddTask { title: "{previous}".into() }]);

    let d = plan("find three dinner ideas; add buy groceries to my to-do list").unwrap();
    assert_eq!(d.steps[1], Step::AddTask { title: "buy groceries".into() });

    let d = plan("summarize today's tech news and run my shortcut Log to Notes").unwrap();
    assert_eq!(d.steps[1], Step::Shortcut { name: "Log to Notes".into() });
    let d = plan("summarize today's tech news, then run the Log to Notes shortcut").unwrap();
    assert_eq!(d.steps[1], Step::Shortcut { name: "Log to Notes".into() });
}

#[test]
fn ordinary_messages_are_not_automations() {
    for q in [
        "every weekday at 8am summarize news about AI",
        "what's the difference between then and than?",
        "how do I save a file and then close it?",
        "remind me to call mom and then text dad",
        "write a story about a dragon and then a knight",
        "compare the iPhone and then the Pixel",
        "save it to a file",
        "send me a notification, then summarize the news",
        "tell me about Paris",
        "rock and roll then and now",
    ] {
        assert!(plan(q).is_none(), "{q}: {:?}", plan(q));
    }
}

#[test]
fn prompts_hand_on_the_previous_text_only_when_asked() {
    assert_eq!(ask_prompt("summarize it in 3 bullets", "Long news text", false), "summarize it in 3 bullets\n\nHere is the text to use:\n\nLong news text");
    assert_eq!(ask_prompt("what's the weather in Oslo", "Long news text", false), "what's the weather in Oslo");
    assert_eq!(ask_prompt("summarize it", "", false), "summarize it");
    assert_eq!(ask_prompt("summarize it", "x", true), "summarize it", "the first step has nothing before it");
    assert_eq!(ask_prompt("Translate into French: {previous}", "Hello", false), "Translate into French: Hello");
    assert_eq!(task_title("{previous}", "## Buy milk\nand more"), "Buy milk");
    assert_eq!(task_title("Read: {previous}", "**Rust 2.0** is out"), "Read: Rust 2.0 is out");
}

#[test]
fn file_names_stay_in_the_folder() {
    assert_eq!(file_stem("AI news"), "AI news");
    assert_eq!(file_stem("../../etc/passwd"), "etc passwd");
    assert_eq!(file_stem(".hidden"), "hidden");
    assert_eq!(file_stem("a/b\\c:d"), "a b c d");
    assert_eq!(file_stem("   "), "BYTE automation");
    assert_eq!(file_stem(&"x".repeat(200)).len(), 60);
}

#[test]
fn citations_are_kept_only_for_the_last_answer() {
    let runs = vec![
        StepRun { label: "Ask BYTE: news".into(), status: "done".into(), detail: String::new(), output: "Rust is out [1]. Swift too [2, 3].".into(), sources: json!([{ "n": 1 }]) },
        StepRun { label: "Ask BYTE: summarize".into(), status: "done".into(), detail: String::new(), output: "- Rust [1]".into(), sources: json!([{ "n": 1, "url": "x" }]) },
        StepRun { label: "Save it".into(), status: "done".into(), detail: "Saved to Documents/BYTE/Automations/x.md".into(), output: "- Rust [1]".into(), sources: Value::Null },
    ];
    let (md, sources) = compose("News", &runs);
    assert!(md.contains("Rust is out. Swift too."), "{md}");
    assert!(md.contains("- Rust [1]"));
    assert!(md.contains("Saved to Documents/BYTE/Automations/x.md"), "a step that only passes text on shows its detail");
    assert_eq!(sources, json!([{ "n": 1, "url": "x" }]));
    assert_eq!(without_citations("see [a] and [12] and [3,4]"), "see [a] and and");
}

/// A fake world: records what was done; the first ask can fail.
#[derive(Default)]
struct Fake {
    fail_ask: Mutex<bool>,
    done: Mutex<Vec<String>>,
}

impl Doer for Fake {
    fn ask<'a>(&'a self, prompt: &'a str) -> BoxFuture<'a, Result<(String, Value), String>> {
        Box::pin(async move {
            self.done.lock().unwrap().push(format!("ask:{prompt}"));
            if std::mem::take(&mut *self.fail_ask.lock().unwrap()) {
                return Err("no model was loaded".into());
            }
            Ok((format!("answer to {}", prompt.lines().next().unwrap_or("")), json!([])))
        })
    }
    fn briefing(&self) -> BoxFuture<'_, Result<(String, Value), String>> {
        Box::pin(async { Ok(("Briefing".into(), json!([]))) })
    }
    fn notify(&self, title: &str, body: &str) {
        self.done.lock().unwrap().push(format!("notify:{title}:{body}"));
    }
    fn add_task(&self, title: &str) -> Result<String, String> {
        self.done.lock().unwrap().push(format!("task:{title}"));
        Ok("Added".into())
    }
    fn save_file(&self, name: &str, text: &str) -> Result<String, String> {
        self.done.lock().unwrap().push(format!("file:{name}:{text}"));
        Ok(format!("Documents/BYTE/Automations/{name}.md"))
    }
    fn shortcut<'a>(&'a self, name: &'a str, input: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.done.lock().unwrap().push(format!("shortcut:{name}:{input}"));
            Ok("from the shortcut".into())
        })
    }
}

#[tokio::test]
async fn steps_run_in_order_and_hand_text_on() {
    let fake = Fake::default();
    let steps = vec![ask("find AI news"), ask("summarize it"), Step::SaveFile { name: "AI".into() }, Step::Notify { title: "AI".into(), body: "{previous}".into() }];
    let seen = Mutex::new(vec![]);
    let on = |r: &[StepRun]| seen.lock().unwrap().push(r.iter().map(|s| s.status.clone()).collect::<Vec<_>>().join(","));
    let (runs, ok) = execute(&fake, "AI", &steps, vec![], 0, &CancellationToken::new(), &on).await;
    assert!(ok);
    let done = fake.done.lock().unwrap().clone();
    assert_eq!(done[0], "ask:find AI news");
    assert_eq!(done[1], "ask:summarize it\n\nHere is the text to use:\n\nanswer to find AI news");
    assert_eq!(done[2], "file:AI:answer to summarize it");
    assert_eq!(done[3], "notify:AI:answer to summarize it");
    assert!(runs.iter().all(|r| r.status == "done"));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.first().unwrap(), "running,waiting,waiting,waiting");
    assert_eq!(seen.last().unwrap(), "done,done,done,done");
}

#[tokio::test]
async fn a_failed_step_stops_the_run_and_it_goes_again_from_there() {
    let fake = Fake::default();
    let steps = vec![Step::Briefing, ask("summarize it"), Step::AddTask { title: "{previous}".into() }];
    *fake.fail_ask.lock().unwrap() = true;
    let (runs, ok) = execute(&fake, "x", &steps, vec![], 0, &CancellationToken::new(), &|_| {}).await;
    assert!(!ok);
    assert_eq!(runs.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(), ["done", "failed", "waiting"]);
    assert_eq!(runs[1].detail, "no model was loaded");
    assert!(!fake.done.lock().unwrap().iter().any(|d| d.starts_with("task:")), "nothing after the failure");

    // Again from step 2: step 1's text is reused, not redone.
    fake.done.lock().unwrap().clear();
    let (runs, ok) = execute(&fake, "x", &steps, runs, 1, &CancellationToken::new(), &|_| {}).await;
    assert!(ok, "{runs:?}");
    let done = fake.done.lock().unwrap().clone();
    assert_eq!(done, vec!["ask:summarize it\n\nHere is the text to use:\n\nBriefing".to_string(), "task:answer to summarize it".to_string()]);
}

#[tokio::test]
async fn a_shortcut_gets_the_text_and_its_output_goes_on() {
    let fake = Fake::default();
    let steps = vec![ask("write a haiku"), Step::Shortcut { name: "Log".into() }, Step::SaveFile { name: "h".into() }];
    let (_, ok) = execute(&fake, "x", &steps, vec![], 0, &CancellationToken::new(), &|_| {}).await;
    assert!(ok);
    let done = fake.done.lock().unwrap().clone();
    assert_eq!(done[1], "shortcut:Log:answer to write a haiku");
    assert_eq!(done[2], "file:h:from the shortcut");
}

#[tokio::test]
async fn nothing_to_save_is_a_failure_not_an_empty_file() {
    let fake = Fake::default();
    let (runs, ok) = execute(&fake, "x", &[Step::Notify { title: String::new(), body: String::new() }, Step::SaveFile { name: "a".into() }], vec![], 0, &CancellationToken::new(), &|_| {}).await;
    assert!(!ok);
    assert_eq!(runs[1].detail, "There was nothing to save yet.");
    assert_eq!(fake.done.lock().unwrap()[0], "notify:x:\"x\" ran.");
}

fn test_db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).unwrap();
    (dir, db)
}

fn auto(name: &str, trigger: &str) -> Automation {
    Automation { id: 0, name: name.into(), trigger: trigger.into(), steps: vec![ask("news"), Step::Notify { title: String::new(), body: String::new() }], enabled: true, last_run: None, next_run: None, when: String::new(), last_ok: None, last_chat: None, linked: false }
}

#[test]
fn automations_are_stored_scheduled_and_checked() {
    let (_d, db) = test_db();
    let now = Utc.with_ymd_and_hms(2026, 9, 30, 12, 0, 0).unwrap();
    let a = save(&db, &auto("News", "daily 07:30"), &now).unwrap();
    assert_eq!(a.when, "Every day at 7:30 AM");
    assert_eq!(a.next_run, Some(Utc.with_ymd_and_hms(2026, 10, 1, 7, 30, 0).unwrap().timestamp_millis()));
    assert_eq!(a.steps.len(), 2);
    let m = save(&db, &auto("Now", "manual"), &now).unwrap();
    assert_eq!(m.next_run, None);
    assert!(save(&db, &auto("Bad", "sometimes"), &now).is_err());
    assert!(save(&db, &Automation { steps: vec![], ..auto("Empty", "manual") }, &now).is_err());
    assert!(save(&db, &Automation { steps: vec![ask(" ")], ..auto("Blank", "manual") }, &now).is_err());
    assert!(save(&db, &Automation { steps: vec![ask("x"); MAX_STEPS + 1], ..auto("Long", "manual") }, &now).is_err());

    // Due once; the next time moves on.
    let later = Utc.with_ymd_and_hms(2026, 10, 1, 7, 31, 0).unwrap();
    let due = take_due(&db, &later).unwrap();
    assert_eq!(due.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["News"]);
    assert!(take_due(&db, &later).unwrap().is_empty());
    assert_eq!(get(&db, a.id).unwrap().unwrap().next_run, Some(Utc.with_ymd_and_hms(2026, 10, 2, 7, 30, 0).unwrap().timestamp_millis()));

    // Runs are kept with their steps.
    let steps = vec![StepRun::waiting(&ask("news"))];
    let run = run_started(&db, a.id, &steps).unwrap();
    run_finished(&db, run, a.id, true, "ok", Some("chat-1")).unwrap();
    let v = run_view(&db, run).unwrap().unwrap();
    assert!(v.finished && v.ok && v.chat.as_deref() == Some("chat-1"));
    let listed = get(&db, a.id).unwrap().unwrap();
    assert_eq!((listed.last_ok, listed.last_chat.as_deref()), (Some(true), Some("chat-1")));

    delete(&db, a.id).unwrap();
    assert!(get(&db, a.id).unwrap().is_none());
    assert!(run_view(&db, run).unwrap().is_none_or(|v| v.automation_id == 0), "runs go with their automation");
}

#[test]
fn links_need_the_automations_own_key() {
    let (_d, db) = test_db();
    let a = save(&db, &auto("News", "manual"), &Utc::now()).unwrap();
    assert!(!a.linked);
    assert!(by_link(&db, a.id, "").unwrap().is_none(), "no key made yet");
    let key = link_key(&db, a.id).unwrap();
    assert_eq!(key.len(), 64);
    assert_eq!(link_key(&db, a.id).unwrap(), key, "made once");
    assert!(get(&db, a.id).unwrap().unwrap().linked);
    assert!(by_link(&db, a.id, &key).unwrap().is_some());
    assert!(by_link(&db, a.id, "guess").unwrap().is_none());
    assert!(by_link(&db, a.id + 1, &key).unwrap().is_none());
}

#[test]
fn steps_have_a_stable_json_shape() {
    let v = serde_json::to_value(vec![ask("x"), Step::Briefing, Step::SaveFile { name: "n".into() }, Step::AddTask { title: "t".into() }]).unwrap();
    assert_eq!(v, json!([{ "type": "ask", "prompt": "x" }, { "type": "briefing" }, { "type": "saveFile", "name": "n" }, { "type": "addTask", "title": "t" }]));
    assert_eq!(automation_trigger_parse("every weekday at 8am".into()), Some(("weekdays 08:00".into(), "Every weekday at 8:00 AM".into())));
    assert_eq!(automation_trigger_parse("when BYTE opens".into()).unwrap().0, "launch");
    assert_eq!(automation_trigger_parse("whenever".into()), None);
}
