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

/// AppleScript treats some words as parameter names ("since" broke the Mail check on
/// macOS); no fixed script may use one as a variable. Runs everywhere, not just on a Mac.
#[test]
fn scripts_avoid_applescript_keywords_as_variables() {
    const RESERVED: &[&str] = &["since", "given", "result", "from", "thru", "through", "returning", "into", "onto", "against", "instead", "beside", "until", "while", "error", "space", "tab", "return", "it", "me", "my", "version", "date", "time", "text", "list", "record", "number", "string", "character", "word", "paragraph", "item", "id", "name", "class", "contents", "reference", "every", "some", "count"];
    let mut scripts: Vec<&str> = ALL_SCRIPTS.to_vec();
    scripts.extend(crate::selection::SCRIPTS);
    scripts.extend(crate::filectl::SCRIPTS);
    scripts.extend(crate::upkeep::SCRIPTS);
    for s in scripts {
        for line in s.lines() {
            let l = line.trim();
            let Some(rest) = l.strip_prefix("set ") else { continue };
            let var = rest.split_whitespace().next().unwrap_or("");
            if rest.split_whitespace().nth(1) == Some("to") {
                assert!(!RESERVED.contains(&var), "variable `{var}` is an AppleScript word: {l}");
            }
        }
    }
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
    assert_eq!(UNDO.lock().unwrap().get(&token), Some(&Undo::Cmd(Command::Osa { script: REMINDER_DELETE, args: vec!["x-apple-reminder://ABC".into()] })));
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

#[test]
fn mail_and_messages_are_routed_only_when_asked_for() {
    assert_eq!(family("Any new emails?"), Some(Family::MailList));
    assert_eq!(family("summarize my inbox"), Some(Family::MailList));
    assert_eq!(family("did Sam email me?"), Some(Family::MailList));
    assert_eq!(family("emails from Jen this week"), Some(Family::MailList));
    assert_eq!(family("Reply to Sam's email saying I can make it"), Some(Family::MailReply));
    assert_eq!(family("email Jen about the party on Saturday"), Some(Family::MailDraft));
    assert_eq!(family("send an email to sam@example.com asking for the slides"), Some(Family::MailDraft));
    assert_eq!(family("text Mom that I'm running late"), Some(Family::Message));
    assert_eq!(family("message Sam: see you at 7"), Some(Family::Message));
    for q in [
        "write an email to my landlord about the heating",
        "how do I write a professional email?",
        "email me when it's done",
        "text summarization models",
        "What is a good email subject line for a job application?",
        "reply to this comment politely",
    ] {
        assert!(!matches!(family(q), Some(Family::MailDraft | Family::MailReply | Family::Message | Family::MailList)), "{q}");
    }
}

#[test]
fn mail_and_messages_keep_words_out_of_scripts() {
    let evil = "x\" & (do shell script \"rm -rf ~\") & \"";
    for a in [
        Action::MailList { query: evil.into(), days: 3 },
        Action::MailDraft { to: evil.into(), name: String::new(), subject: evil.into(), body: evil.into() },
        Action::MessageDraft { to: "+1 555 123 4567".into(), name: String::new(), body: evil.into() },
        Action::ContactFind { name: evil.into() },
    ] {
        let Command::Osa { script, args } = a.command() else { panic!("{a:?}") };
        assert!(ALL_SCRIPTS.contains(&script) && !script.contains("rm -rf"), "{a:?}");
        assert!(args.iter().any(|x| x.contains("rm") || x.contains("rm%20")), "{a:?}");
    }
    assert_eq!(sms_url("+1 (555) 123-4567", "Running late, 10 min!"), "sms:%2B15551234567&body=Running%20late%2C%2010%20min%21");
    assert_eq!(sms_url("mom@example.com", "hi"), "sms:mom%40example.com&body=hi");
    assert!(Action::MailDraft { to: "a@b.c".into(), name: String::new(), subject: "s".into(), body: "b".into() }.needs_ok());
    assert!(Action::MessageDraft { to: "1".into(), name: String::new(), body: "b".into() }.needs_ok());
    assert!(!Action::MailList { query: String::new(), days: 3 }.needs_ok());
}

#[test]
fn template_placeholders_are_removed() {
    assert_eq!(without_placeholders("Hi Sam,\n\nI can make it.\n\nBest,\n[Your Name]"), "Hi Sam,\n\nI can make it.\n\nBest,");
    assert_eq!(without_placeholders("See you on [Date] at 7."), "See you on  at 7.");
    assert_eq!(without_placeholders("Use a[i] carefully"), "Use a carefully");
}

#[test]
fn contacts_and_senders_are_read() {
    let out = format!("Sam Lee{US}sam@example.com,sam.lee@work.example{US}+1 555 0100{RS}Mom{US}missing value{US}+1 555 0199{RS}");
    let p = people(&out);
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].emails, vec!["sam@example.com", "sam.lee@work.example"]);
    assert!(p[1].emails.is_empty() && p[1].phones == vec!["+1 555 0199"]);
    assert_eq!(sender_parts("Sam Lee <sam@example.com>"), ("Sam Lee".into(), "sam@example.com".into()));
    assert_eq!(sender_parts("\"Lee, Sam\" <sam@example.com>"), ("Lee, Sam".into(), "sam@example.com".into()));
    assert_eq!(sender_parts("sam@example.com"), (String::new(), "sam@example.com".into()));
}

#[tokio::test]
async fn a_text_finds_the_number_asks_first_and_never_sends() {
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok(format!("Mom{US}{US}+1 555 0199{RS}")));
    fake.replies.lock().unwrap().push(Ok("opened".into()));
    let (out, ev) = flow("text Mom that I'm running late", &fake, Some(true)).await;
    let card = ev.iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.clone()) } else { None }).expect("approval card");
    assert_eq!(card.title, "Open Messages with a text to Mom");
    assert!(card.fields.iter().any(|f| f.label == "Text" && f.value == "I'm running late"), "{:?}", card.fields);
    let ran = fake.ran.lock().unwrap().clone();
    assert_eq!(ran[1], Command::Osa { script: MESSAGE_DRAFT, args: vec!["I'm running late".into(), "sms:%2B15550199&body=I%27m%20running%20late".into()] });
    assert!(out.unwrap().1.contains("It isn't sent"));
}

#[tokio::test]
async fn several_matches_ask_which_one() {
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok(format!("Sam Lee{US}sam@example.com{US}{RS}Samantha Ruiz{US}sr@example.com{US}{RS}")));
    let (out, ev) = flow("email Sam about Friday", &fake, None).await;
    assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))));
    assert!(out.unwrap().1.contains("Which Sam? Sam Lee, Samantha Ruiz"));
    // Nobody found: ask for the address.
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok(String::new()));
    let (out, _) = flow("text Grandpa that I'll call tonight", &fake, None).await;
    assert!(out.unwrap().1.contains("couldn't find Grandpa in your Contacts"));
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
    let a = details(&turn, Family::Event, q, now(), &Fake::default()).await.unwrap().unwrap();
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
    let a = details(&turn, Family::Note, q, now(), &Fake::default()).await.unwrap().unwrap();
    eprintln!("{a:?}");
    match a {
        Action::NoteCreate { title, body } => assert!(!title.is_empty() && body.starts_with("gate code 4417") && body.contains("blue pot"), "{title} / {body}"),
        other => panic!("{other:?}"),
    }
    let q = "Remind me to renew my passport next week";
    let a = details(&turn, Family::Reminder, q, now(), &Fake::default()).await.unwrap().unwrap();
    eprintln!("{a:?}");
    match a {
        Action::ReminderAdd { title, due, .. } => {
            assert!(title.to_lowercase().contains("passport") && !title.to_lowercase().contains("next week"), "{title}");
            assert_eq!(due, Some(at(2026, 10, 5, 9, 0)));
        }
        other => panic!("{other:?}"),
    }
}

/// A real small model writes an email reply from the user's words and the email being answered.
#[tokio::test]
#[ignore]
async fn e2e_mail_reply_draft() {
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
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok(format!(
        "Sam Lee <sam@example.com>{US}Dinner on Friday?{US}Tuesday, September 29, 2026 at 9:00:00 AM{US}Hi! A few of us are getting dinner at Luca's on Friday at 7. Can you come?{RS}"
    )));
    let a = details(&turn, Family::MailReply, "Reply to Sam's email saying I can make it and I'll bring dessert", now(), &fake).await.unwrap().unwrap();
    eprintln!("{a:?}");
    match a {
        Action::MailDraft { to, name, subject, body } => {
            assert_eq!((to.as_str(), name.as_str(), subject.as_str()), ("sam@example.com", "Sam Lee", "Re: Dinner on Friday?"));
            let b = body.to_lowercase();
            assert!(b.contains("dessert") && !body.contains('['), "{body}");
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
    // Compile them all before failing, so one bad script can't hide another.
    let mut bad = Vec::new();
    let mut all: Vec<&str> = ALL_SCRIPTS.to_vec();
    all.extend(crate::selection::SCRIPTS);
    all.extend(crate::filectl::SCRIPTS);
    all.extend(crate::upkeep::SCRIPTS);
    for (i, s) in all.iter().enumerate() {
        let out = std::process::Command::new("/usr/bin/osacompile").arg("-o").arg(dir.path().join(format!("{i}.scpt"))).arg("-e").arg(s).output().unwrap();
        if !out.status.success() {
            bad.push(format!("script {i}: {}\n{s}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    assert!(bad.is_empty(), "{} scripts don't compile:\n{}", bad.len(), bad.join("\n\n"));
    let now = MacRunner.run(&Command::Osa { script: "on run argv\nreturn (output volume of (get volume settings)) as string\nend run", args: vec![] }).await.unwrap();
    let v: u8 = now.trim().parse().unwrap_or(50);
    let set = MacRunner.run(&Action::Volume(v).command()).await.unwrap();
    assert_eq!(set.trim(), v.to_string());
}
