use std::sync::{Arc, Mutex as StdMutex};

use super::*;
use crate::engine::Endpoint;

fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, 0).unwrap()
}

/// Tuesday, September 29, 2026, 10:15 in the morning.
fn now() -> NaiveDateTime {
    at(2026, 9, 29, 10, 15)
}

fn ready(q: &str) -> Option<Action> {
    match plan(q)? {
        Plan::Ready(a) => Some(a),
        Plan::Ask(_) => None,
    }
}

fn family(q: &str) -> Option<Family> {
    match plan(q)? {
        Plan::Ask(f) => Some(f),
        Plan::Ready(_) => None,
    }
}

#[test]
fn commands_are_recognized_and_questions_are_not() {
    assert_eq!(ready("Turn on dark mode"), Some(Action::DarkMode(Switch::On)));
    assert_eq!(ready("please turn off dark mode"), Some(Action::DarkMode(Switch::Off)));
    assert_eq!(ready("switch to light mode"), Some(Action::DarkMode(Switch::Off)));
    assert_eq!(ready("Set the volume to 30%"), Some(Action::Volume(30)));
    assert_eq!(ready("turn the volume up to 150"), Some(Action::Volume(100)));
    assert_eq!(ready("mute"), Some(Action::Mute(true)));
    assert_eq!(ready("Turn off wifi"), Some(Action::Wifi(false)));
    assert_eq!(ready("turn wi-fi on"), Some(Action::Wifi(true)));
    assert_eq!(ready("pause the music"), Some(Action::MusicPause));
    assert_eq!(ready("next song"), Some(Action::MusicNext));
    assert_eq!(ready("what's playing?"), Some(Action::NowPlaying));
    assert_eq!(ready("what's this tab in Safari about"), Some(Action::SafariTab));
    assert_eq!(ready("open bluetooth settings"), Some(Action::OpenSettings { pane: "bluetooth".into() }));
    assert_eq!(ready("Open software update settings"), Some(Action::OpenSettings { pane: "software update".into() }));
    assert_eq!(ready("list my shortcuts"), Some(Action::ShortcutsList));
    assert_eq!(family("Remind me to call Mom tomorrow at 3pm"), Some(Family::Reminder));
    assert_eq!(family("add milk to my reminders"), Some(Family::Reminder));
    assert_eq!(family("What's on my calendar this week?"), Some(Family::EventsList));
    assert_eq!(family("Add dentist to my calendar on Friday at 2pm"), Some(Family::Event));
    assert_eq!(family("make a note: gate code 4417"), Some(Family::Note));
    assert_eq!(family("save this to notes"), Some(Family::Note));
    assert_eq!(family("find my notes about the lease"), Some(Family::NoteFind));
    assert_eq!(family("play some jazz"), Some(Family::Music));
    assert_eq!(family("run my Morning shortcut"), Some(Family::Shortcut));
    for q in [
        "How do I turn on dark mode?",
        "What is dark mode?",
        "why does my wifi keep dropping",
        "play a game of chess with me",
        "Explain how reminders work in iOS",
        "What's the capital of France?",
        "Write me a note to my landlord about the heating",
        "Can you explain how to schedule a meeting across time zones",
        "Let's play a quiz",
    ] {
        assert!(!wants(q), "{q}");
    }
}

#[test]
fn times_are_read_from_the_message() {
    let n = now();
    assert_eq!(when_in("remind me to call Mom tomorrow at 3pm", n), Some(at(2026, 9, 30, 15, 0)));
    assert_eq!(when_in("at 3:30 pm", n), Some(at(2026, 9, 29, 15, 30)));
    assert_eq!(when_in("at 9am", n), Some(at(2026, 9, 30, 9, 0)), "9am has passed today, so tomorrow");
    assert_eq!(when_in("friday at 2", n), Some(at(2026, 10, 2, 14, 0)), "a bare 2 means 2 pm");
    assert_eq!(when_in("on tuesday", n), Some(at(2026, 10, 6, 9, 0)), "today is Tuesday: the next one");
    assert_eq!(when_in("in 20 minutes", n), Some(at(2026, 9, 29, 10, 35)));
    assert_eq!(when_in("remind me in an hour to stretch", n), Some(at(2026, 9, 29, 11, 15)));
    assert_eq!(when_in("tonight", n), Some(at(2026, 9, 29, 20, 0)));
    assert_eq!(when_in("tomorrow morning", n), Some(at(2026, 9, 30, 9, 0)));
    assert_eq!(when_in("at noon", n), Some(at(2026, 9, 29, 12, 0)));
    assert_eq!(when_in("at 15:45", n), Some(at(2026, 9, 29, 15, 45)));
    assert_eq!(when_in("buy 2 cartons of milk", n), None, "a count isn't a time");
    assert_eq!(when_in("call Mom", n), None);
    assert_eq!(parse_model_when("2026-10-01 08:30"), Some(at(2026, 10, 1, 8, 30)));
    assert_eq!(parse_model_when("soon"), None);
    assert_eq!(minutes_in("for 2 hours"), Some(120));
    assert_eq!(minutes_in("a 30 minute call"), Some(30));
    assert_eq!(minutes_in("for half an hour"), Some(30));
    assert_eq!(when_text(at(2026, 9, 30, 15, 0)), "Wed, Sep 30 at 3 PM");
    assert_eq!(when_text(at(2026, 9, 30, 9, 5)), "Wed, Sep 30 at 9:05 AM");
}

#[test]
fn titles_drop_the_time_words() {
    assert_eq!(reminder_title("Remind me to call Mom tomorrow at 3pm").as_deref(), Some("Call Mom"));
    assert_eq!(reminder_title("remind me to take out the trash tonight").as_deref(), Some("Take out the trash"));
    assert_eq!(reminder_title("add milk to my reminders").as_deref(), Some("Milk"));
    assert_eq!(reminder_title("remind me in 20 minutes to check the oven").as_deref(), Some("Check the oven"));
    assert_eq!(without_time("Dentist on Friday at 2pm"), "Dentist");
}

#[test]
fn user_words_never_become_script_code() {
    let evil = "x\" & (do shell script \"rm -rf ~\") & \"";
    for a in [
        Action::NoteCreate { title: evil.into(), body: evil.into() },
        Action::NoteFind { query: evil.into() },
        Action::ReminderAdd { title: evil.into(), due: Some(now()), list: evil.into() },
        Action::EventAdd { title: evil.into(), start: now(), minutes: 30, calendar: evil.into() },
        Action::MusicPlay { query: evil.into() },
        Action::ShortcutRun { name: evil.into(), input: String::new() },
    ] {
        match a.command() {
            Command::Osa { script, args } => {
                assert!(ALL_SCRIPTS.contains(&script), "{a:?} uses a fixed script");
                assert!(!script.contains("rm -rf") && !script.contains("do shell script"), "{a:?}");
                assert!(args.iter().any(|x| x.contains("rm -rf")), "the words travel as an argument: {a:?}");
            }
            Command::Exec { program, args } => {
                assert_eq!(program, "shortcuts");
                assert_eq!(args[1], evil, "one argument, no shell");
            }
        }
    }
    // No fixed script runs shell commands at all.
    assert!(ALL_SCRIPTS.iter().all(|s| !s.contains("do shell script")));
    // Notes get escaped HTML (the text shows as typed).
    assert_eq!(note_html("A <b>", "1 & 2\n\nthree"), "<div><b>A &lt;b&gt;</b></div><div>1 &amp; 2</div><div><br></div><div>three</div>");
    // Dates go in as numbers, not locale strings.
    assert_eq!(date_arg(at(2026, 9, 30, 15, 5)), "2026 9 30 15 5");
    assert!(REMINDER_ADD.contains("on mkdate(s)") && EVENT_ADD.contains("on mkdate(s)"));
}

#[test]
fn only_lasting_changes_ask_first() {
    assert!(Action::ReminderAdd { title: "a".into(), due: None, list: String::new() }.needs_ok());
    assert!(Action::NoteCreate { title: "a".into(), body: "b".into() }.needs_ok());
    assert!(Action::EventAdd { title: "a".into(), start: now(), minutes: 60, calendar: String::new() }.needs_ok());
    assert!(Action::ShortcutRun { name: "a".into(), input: String::new() }.needs_ok());
    assert!(Action::Wifi(false).needs_ok());
    for a in [Action::Wifi(true), Action::Volume(20), Action::MusicPause, Action::DarkMode(Switch::On), Action::EventsList { days: 1 }, Action::SafariTab] {
        assert!(!a.needs_ok(), "{a:?}");
    }
}

#[test]
fn settings_links_and_devices() {
    assert_eq!(settings_url("bluetooth"), "x-apple.systempreferences:com.apple.BluetoothSettings");
    assert_eq!(settings_url("automation privacy"), "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Automation");
    assert_eq!(settings_url(""), "x-apple.systempreferences:");
    let ports = "Hardware Port: Ethernet\nDevice: en1\nEthernet Address: aa\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: bb\n";
    assert_eq!(wifi_device(ports).as_deref(), Some("en0"));
    assert_eq!(wifi_device("Hardware Port: Thunderbolt\nDevice: bridge0\n"), None);
    let list = "Morning Routine\nLog Water\nShare Screenshot\n";
    assert_eq!(pick_shortcut("morning", list).as_deref(), Some("Morning Routine"));
    assert_eq!(pick_shortcut("Log Water", list).as_deref(), Some("Log Water"));
    assert_eq!(pick_shortcut("the log water one", list).as_deref(), Some("Log Water"));
    assert_eq!(pick_shortcut("backup", list), None);
}

#[test]
fn errors_say_how_to_fix_them() {
    let e = RunError::from_stderr("execution error: Not authorized to send Apple events to Notes. (-1743)");
    assert_eq!(e, RunError::NotAllowed);
    assert!(e.text("Notes").contains("Privacy & Security → Automation → BYTE → turn on Notes"));
    assert_eq!(RunError::from_stderr("Unable to find application named 'Music'"), RunError::Missing);
    assert_eq!(RunError::from_stderr("0:10: syntax error: oops (-2741)"), RunError::Failed("0:10: syntax error: oops (-2741)".into()));
}

#[test]
fn script_output_becomes_notes() {
    let (d, n) = read_out(&Action::EventsList { days: 1 }, &format!("Standup{US}Tuesday 9:00{US}Work{RS}Lunch{US}Tuesday 12:00{US}Home{RS}"));
    assert_eq!(d, "2 events today");
    assert!(n.contains("- Tuesday 9:00 — Standup (Work)") && n.contains("Lunch"));
    let (d, _) = read_out(&Action::RemindersList { list: String::new() }, &format!("Reminders{RS}Milk{US}{RS}Call Mom{US}Wed 3 PM{RS}"));
    assert_eq!(d, "2 open in Reminders");
    let (d, n) = read_out(&Action::MusicPlay { query: "jazz".into() }, &format!("So What{US}Miles Davis"));
    assert_eq!(d, "So What — Miles Davis");
    assert!(n.contains("\"So What\" by Miles Davis"));
    assert_eq!(read_out(&Action::MusicPlay { query: "zzz".into() }, "none").0, "Not in your library");
    assert_eq!(read_out(&Action::DarkMode(Switch::Toggle), "true").0, "Dark mode is on");
    let undo = undo_for(&Action::EventAdd { title: "a".into(), start: now(), minutes: 60, calendar: String::new() }, &format!("UID-1{US}Home"));
    assert_eq!(undo, Some(Command::Osa { script: EVENT_DELETE, args: vec!["UID-1".into(), "Home".into()] }));
    assert_eq!(undo_for(&Action::Volume(3), "3"), None);
}

// ------------------------------------------------------------ the whole flow

/// Records commands and answers them from a list.
#[derive(Default)]
struct Fake {
    ran: StdMutex<Vec<Command>>,
    replies: StdMutex<Vec<Result<String, RunError>>>,
}

impl Runner for Fake {
    fn run<'a>(&'a self, cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>> {
        self.ran.lock().unwrap().push(cmd.clone());
        let r = {
            let mut r = self.replies.lock().unwrap();
            if r.is_empty() {
                Ok(String::new())
            } else {
                r.remove(0)
            }
        };
        Box::pin(async move { r })
    }
}

type Seen = Arc<StdMutex<Vec<ChatEvent>>>;

async fn flow(q: &str, fake: &Fake, approve: Option<bool>) -> (Option<(SourceBook, String)>, Vec<ChatEvent>) {
    let dir = tempfile::tempdir().unwrap();
    let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
    let http = chat::local_client();
    // No model running: details come from the rules.
    let ep = Endpoint { base_url: "http://127.0.0.1:9".into(), api_key: "k".into(), model: "m".into(), context: 8192, vision: false, cloud: None };
    let history = vec![chat::ChatMessage::new("user", q)];
    let plan = crate::router::plan_turn(crate::settings::Mode::Auto, crate::settings::ThinkingPref::Off, q);
    let turn = Turn {
        http: &http, cloud: None, net: &http, ep: &ep, system: "", history: &history, plan, mode: crate::settings::Mode::Auto, web: false, memory: false, log: &log, files: None,
        app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false,
        modules: crate::agent::Modules { mac: true, ..Default::default() },
    };
    let seen: Seen = Arc::default();
    let s2 = seen.clone();
    let send = move |e: ChatEvent| {
        s2.lock().unwrap().push(e);
        Ok(())
    };
    if let Some(ok) = approve {
        let s3 = seen.clone();
        tokio::spawn(async move {
            loop {
                let id = s3.lock().unwrap().iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.id.clone()) } else { None });
                if let Some(id) = id {
                    crate::web_agent::answer(&id, ok);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
    }
    let out = run_with(&turn, q, now(), fake, &CancellationToken::new(), &send).await.unwrap();
    let ev = seen.lock().unwrap().clone();
    (out, ev)
}

#[tokio::test]
async fn a_reminder_asks_first_then_can_be_undone() {
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok("x-apple-reminder://ABC".into()));
    let (out, ev) = flow("Remind me to call Mom tomorrow at 3pm", &fake, Some(true)).await;
    let card = ev.iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.clone()) } else { None }).expect("approval card");
    assert_eq!(card.action, "mac");
    assert_eq!(card.title, "Add the reminder \"Call Mom\"");
    assert!(card.fields.iter().any(|f| f.label == "When" && f.value == "Wed, Sep 30 at 3 PM"), "{:?}", card.fields);
    let ran = fake.ran.lock().unwrap().clone();
    assert_eq!(ran, vec![Command::Osa { script: REMINDER_ADD, args: vec!["Call Mom".into(), "2026 9 30 15 0".into(), String::new()] }]);
    let done = ev.iter().find_map(|e| if let ChatEvent::MacDone(d) = e { Some(d.clone()) } else { None }).unwrap();
    assert!(done.ok && done.app == "Reminders");
    let token = done.undo.expect("undo token");
    assert_eq!(UNDO.lock().unwrap().get(&token), Some(&Command::Osa { script: REMINDER_DELETE, args: vec!["x-apple-reminder://ABC".into()] }));
    let notes = out.unwrap().1;
    assert!(notes.contains("added the reminder \"Call Mom\", due Wed, Sep 30 at 3 PM"), "{notes}");
}

#[tokio::test]
async fn saying_no_changes_nothing() {
    let fake = Fake::default();
    let (out, ev) = flow("make a note: gate code 4417", &fake, Some(false)).await;
    assert!(fake.ran.lock().unwrap().is_empty(), "nothing ran");
    assert!(ev.iter().any(|e| matches!(e, ChatEvent::ApprovalDone { ok: false, .. })));
    assert!(out.unwrap().1.contains("chose not to"));
}

#[tokio::test]
async fn toggles_run_without_asking_and_permission_errors_explain() {
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok("true".into()));
    let (out, ev) = flow("turn on dark mode", &fake, None).await;
    assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))));
    assert!(out.unwrap().1.contains("dark mode is now on"));

    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Err(RunError::NotAllowed));
    let (out, ev) = flow("pause the music", &fake, None).await;
    let done = ev.iter().find_map(|e| if let ChatEvent::MacDone(d) = e { Some(d.clone()) } else { None }).unwrap();
    assert!(!done.ok && done.detail.contains("Automation → BYTE → turn on Music"));
    assert!(out.unwrap().1.contains("didn't work"));
}

#[tokio::test]
async fn wifi_finds_its_device_and_shortcuts_must_exist() {
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok("Hardware Port: Wi-Fi\nDevice: en0\n".into()));
    flow("turn wifi on", &fake, None).await;
    let ran = fake.ran.lock().unwrap().clone();
    assert_eq!(ran[1], Command::Exec { program: "networksetup", args: vec!["-setairportpower".into(), "en0".into(), "on".into()] });

    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok("Morning Routine\nLog Water\n".into()));
    let (out, _) = flow("run my backup shortcut", &fake, None).await;
    assert_eq!(fake.ran.lock().unwrap().len(), 1, "only the list ran");
    assert!(out.unwrap().1.contains("no shortcut called \"backup\""));
}

#[tokio::test]
async fn an_event_without_a_time_asks_for_one() {
    let fake = Fake::default();
    let (out, ev) = flow("add dentist to my calendar", &fake, None).await;
    assert!(fake.ran.lock().unwrap().is_empty() && !ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))));
    assert!(out.unwrap().1.contains("When is \"Dentist\"?"));
}

/// A real small model fills in the details (rules back it up): Qwen3 0.6B in CI.
#[tokio::test]
#[ignore]
async fn e2e_details_from_a_real_model() {
    let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
    let dir = tempfile::tempdir().unwrap();
    let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
    let http = chat::local_client();
    let history = vec![];
    let plan = crate::router::plan_turn(crate::settings::Mode::Auto, crate::settings::ThinkingPref::Off, "");
    let turn = Turn {
        http: &http, cloud: None, net: &http, ep: &ep, system: "", history: &history, plan, mode: crate::settings::Mode::Auto, web: false, memory: false, log: &log, files: None,
        app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false,
        modules: crate::agent::Modules { mac: true, ..Default::default() },
    };
    let q = "Add lunch with Sam to my calendar on Friday at 1pm for 90 minutes";
    let a = details(&turn, Family::Event, q, now()).await.unwrap().unwrap();
    eprintln!("{a:?}");
    match a {
        Action::EventAdd { title, start, minutes, calendar } => {
            assert!(title.to_lowercase().contains("lunch"), "{title}");
            assert_eq!(start, at(2026, 10, 2, 13, 0));
            assert_eq!(minutes, 90);
            assert_eq!(calendar, "", "no calendar was named");
        }
        other => panic!("{other:?}"),
    }
    let q = "make a note: gate code 4417, the spare key is under the blue pot";
    let a = details(&turn, Family::Note, q, now()).await.unwrap().unwrap();
    eprintln!("{a:?}");
    match a {
        Action::NoteCreate { title, body } => assert!(!title.is_empty() && body.starts_with("gate code 4417") && body.contains("blue pot"), "{title} / {body}"),
        other => panic!("{other:?}"),
    }
    let q = "Remind me to renew my passport next week";
    let a = details(&turn, Family::Reminder, q, now()).await.unwrap().unwrap();
    eprintln!("{a:?}");
    match a {
        Action::ReminderAdd { title, due, .. } => {
            assert!(title.to_lowercase().contains("passport") && !title.to_lowercase().contains("next week"), "{title}");
            assert_eq!(due, Some(at(2026, 10, 5, 9, 0)));
        }
        other => panic!("{other:?}"),
    }
}

/// On a Mac: every fixed script compiles (`osacompile`), and a harmless
/// round trip works (reading and re-setting the volume needs no permission).
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore]
async fn e2e_scripts_compile_and_run() {
    let dir = tempfile::tempdir().unwrap();
    for (i, s) in ALL_SCRIPTS.iter().enumerate() {
        let out = std::process::Command::new("/usr/bin/osacompile").arg("-o").arg(dir.path().join(format!("{i}.scpt"))).arg("-e").arg(s).output().unwrap();
        assert!(out.status.success(), "script {i} doesn't compile: {}\n{s}", String::from_utf8_lossy(&out.stderr));
    }
    let now = MacRunner.run(&Command::Osa { script: "on run argv\nreturn (output volume of (get volume settings)) as string\nend run", args: vec![] }).await.unwrap();
    let v: u8 = now.trim().parse().unwrap_or(50);
    let set = MacRunner.run(&Action::Volume(v).command()).await.unwrap();
    assert_eq!(set.trim(), v.to_string());
}
