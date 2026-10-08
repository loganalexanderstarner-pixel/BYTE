use chrono::TimeZone;
use quick_xml::events::Event;

use super::*;
use crate::db::Db;
use crate::tasks::Task;

#[test]
fn the_app_id_is_the_identifier_in_the_app_config() {
    let conf = include_str!("../tauri.conf.json");
    assert!(conf.contains(&format!("\"identifier\": \"{APP_ID}\"")), "tauri.conf.json must carry {APP_ID}");
}

#[test]
fn a_buttons_argument_means_one_thing_or_nothing() {
    assert_eq!(parse_arg("done:42"), Some(Do::Done(42)));
    assert_eq!(parse_arg(" done:7 \n"), Some(Do::Done(7)));
    assert_eq!(parse_arg("open"), Some(Do::Open));
    for bad in ["", "done", "done:", "done:-1", "done:+1", "done:1 2", "done:1;done:2", "done:0x10", "done:１２", "done:1234567890123456789", "delete:1", "snooze", "dismiss", "DONE:1", "open:1", "done:1\u{0}"] {
        assert_eq!(parse_arg(bad), None, "{bad:?}");
    }
}

/// The toast text is well-formed XML whatever the reminder says, and says it as text.
fn read(xml: &str) -> (Vec<String>, Vec<(String, String, String)>, Vec<String>) {
    let mut r = quick_xml::Reader::from_str(xml);
    let (mut texts, mut actions, mut selections) = (Vec::new(), Vec::new(), Vec::new());
    let mut in_text = false;
    loop {
        match r.read_event().unwrap_or_else(|e| panic!("not well-formed: {e}\n{xml}")) {
            Event::Start(e) | Event::Empty(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attr = |k: &str| e.attributes().flatten().find(|a| a.key.as_ref() == k.as_bytes()).map(|a| a.unescape_value().unwrap().to_string()).unwrap_or_default();
                match name.as_str() {
                    "text" => in_text = true,
                    "action" => actions.push((attr("content"), attr("arguments"), attr("activationType"))),
                    "selection" => selections.push(attr("id")),
                    _ => {}
                }
            }
            Event::Text(t) if in_text => texts.push(t.unescape().unwrap().to_string()),
            Event::End(e) if e.name().as_ref() == b"text" => in_text = false,
            Event::Eof => break,
            _ => {}
        }
    }
    (texts, actions, selections)
}

#[test]
fn a_reminder_toast_has_snooze_done_and_dismiss_and_carries_the_title_as_plain_text() {
    let (texts, actions, selections) = read(&reminder_xml("Call Mom", 42));
    assert_eq!(texts, ["Reminder", "Call Mom"]);
    assert_eq!(actions, [("Snooze".to_string(), "snooze".to_string(), "system".to_string()), ("Done".into(), "done:42".into(), "foreground".into()), ("Dismiss".into(), "dismiss".into(), "system".into())]);
    assert_eq!(selections, ["5", "10", "60"], "minutes");
    // Whatever the title holds stays text: markup, quotes, ampersands, line breaks and control characters.
    for title in ["Tom & Jerry <b>\"quoted\" 'single'</b>", "</text><action content=\"x\" arguments=\"done:1\"/>", "line one\nline two", "bell\u{7}and\u{0}nul", "emoji 🎉 and accents é", ""] {
        let (texts, actions, _) = read(&reminder_xml(title, 5));
        assert_eq!(actions.len(), 3, "{title:?}: no extra buttons can be smuggled in");
        assert_eq!(texts.len(), if title.is_empty() { 1 } else { 2 }, "{title:?}");
        let want: String = title.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
        assert_eq!(texts.last().cloned().unwrap_or_default(), if title.is_empty() { "Reminder".to_string() } else { want }, "{title:?}");
    }
    // A very long title is cut.
    let (texts, _, _) = read(&reminder_xml(&"x".repeat(1000), 1));
    assert_eq!(texts[1].chars().count(), 200);
}

fn task(title: &str, due: Option<i64>, remind_at: Option<i64>, repeat: &str) -> Task {
    Task { id: 0, title: title.into(), notes: String::new(), due, remind_at, repeat: repeat.into(), done_at: None, created: 0 }
}

#[test]
fn done_marks_the_task_done_and_a_repeating_one_moves_on() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).unwrap();
    let now = chrono::Local.with_ymd_and_hms(2026, 10, 8, 9, 0, 0).unwrap();
    let once = crate::tasks::save(&db, &task("Call Mom", None, Some(now.timestamp_millis()), "")).unwrap();
    let line = apply(&db, &Do::Done(once.id), &now).unwrap();
    assert_eq!(line, "Marked \"Call Mom\" done");
    assert!(crate::tasks::get(&db, once.id).unwrap().unwrap().done_at.is_some());
    // Every day at 9: done today means tomorrow's, not finished for good.
    let due = (now - chrono::Duration::minutes(1)).timestamp_millis();
    let daily = crate::tasks::save(&db, &task("Vitamins", Some(due), Some(due), "daily")).unwrap();
    let line = apply(&db, &Do::Done(daily.id), &now).unwrap();
    assert_eq!(line, "\"Vitamins\" moved to its next time");
    let after = crate::tasks::get(&db, daily.id).unwrap().unwrap();
    assert!(after.done_at.is_none() && after.due.unwrap() > now.timestamp_millis());
    // A task that is gone says so instead of doing something else.
    assert!(apply(&db, &Do::Done(9999), &now).is_err());
    assert_eq!(apply(&db, &Do::Open, &now).unwrap(), "Opened BYTE");
}

/// Shows a real toast and finds it in the notification list. Pressing its Done button needs a person on this PC (no banner is
/// shown while Windows is in Do Not Disturb, and the notification list can't be driven by a script on every build): set
/// `BYTE_LIVE_TOAST_WAIT=60` to wait that many seconds for someone to press Done; without it the button isn't checked.
#[cfg(windows)]
#[test]
#[ignore = "shows a notification on the real PC; run in the desktop session with --ignored --nocapture"]
fn live_pc_a_reminder_toast_is_shown_and_its_done_button_reaches_byte() {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;
    /// Takes this test's toasts out of the person's notification list again, whatever happens.
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            use windows::core::HSTRING;
            use windows::UI::Notifications::ToastNotificationManager;
            if let Ok(h) = ToastNotificationManager::History() {
                let _ = h.ClearWithId(&HSTRING::from(APP_ID));
            }
        }
    }
    let _cleanup = Cleanup;
    let (tx, rx) = mpsc::channel::<String>();
    let tx = std::sync::Mutex::new(tx);
    let tag = format!("byte-live-{}", std::process::id());
    let xml = reminder_xml("BYTE live test: press Done", 777);
    super::win::show_with(&xml, &tag, Arc::new(move |arg| {
        let _ = tx.lock().unwrap().send(arg);
    }))
    .expect("the toast can be shown");
    std::thread::sleep(Duration::from_millis(800));
    assert!(super::win::in_list(&tag), "the toast is in the notification list");
    // A made-up tag is not in the list: the check above says something.
    assert!(!super::win::in_list("byte-live-no-such-toast"));
    let wait: u64 = std::env::var("BYTE_LIVE_TOAST_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    if wait == 0 {
        println!("toast shown and listed; its Done button was not pressed (set BYTE_LIVE_TOAST_WAIT=60 and press it)");
        return;
    }
    println!("press Done on the BYTE reminder (Win+N opens the list) within {wait} seconds...");
    let got = rx.recv_timeout(Duration::from_secs(wait)).expect("someone pressed Done and BYTE's handler was called");
    assert_eq!(got, "done:777");
    assert_eq!(parse_arg(&got), Some(Do::Done(777)));
}
