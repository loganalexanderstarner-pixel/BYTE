use super::*;
use chrono::NaiveDate;

fn at(h: u32, m: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 13).unwrap().and_hms_opt(h, m, 0).unwrap()
}

fn set(secs: u64, label: &str) -> Ask {
    Ask::Set { secs, label: label.into() }
}

#[test]
fn durations_said_every_way_are_added_up() {
    assert_eq!(ask("set a timer for 10 minutes"), Some(set(600, "")));
    assert_eq!(ask("Set a timer for ten minutes."), Some(set(600, "")));
    assert_eq!(ask("set a 5 minute timer"), Some(set(300, "")));
    assert_eq!(ask("timer for 90 seconds"), Some(set(90, "")));
    assert_eq!(ask("set a timer for an hour and a half"), Some(set(5400, "")));
    assert_eq!(ask("set a timer for 2 hours 30 minutes"), Some(set(9000, "")));
    assert_eq!(ask("start a timer for half an hour"), Some(set(1800, "")));
    assert_eq!(ask("set a timer for twenty five minutes"), Some(set(1500, "")));
    assert_eq!(ask("set a timer for 1.5 hours"), Some(set(5400, "")));
    assert_eq!(ask("10 min timer"), Some(set(600, "")));
    assert_eq!(ask("set a timer for 20s"), Some(set(20, "")));
    assert_eq!(ask("Hey, BYTE, set a timer for a minute"), Some(set(60, "")));
    assert_eq!(ask("please set a timer for 45 seconds"), Some(set(45, "")));
}

#[test]
fn a_timer_can_have_a_name() {
    assert_eq!(ask("set a pasta timer for 12 minutes"), Some(set(720, "pasta")));
    assert_eq!(ask("set a laundry timer for 40 minutes"), Some(set(2400, "laundry")));
    assert_eq!(ask("set a 3 minute egg timer"), Some(set(180, "egg")));
}

#[test]
fn asking_about_and_stopping_timers() {
    assert_eq!(ask("how much time is left on my timer"), Some(Ask::Left { label: String::new() }));
    assert_eq!(ask("how long is left on the pasta timer"), Some(Ask::Left { label: "pasta".into() }));
    assert_eq!(ask("cancel the timer"), Some(Ask::Cancel { label: String::new(), all: false }));
    assert_eq!(ask("stop my pasta timer"), Some(Ask::Cancel { label: "pasta".into(), all: false }));
    assert_eq!(ask("cancel all my timers"), Some(Ask::Cancel { label: String::new(), all: true }));
    assert_eq!(ask("what timers do I have"), Some(Ask::List));
    assert_eq!(ask("list my timers"), Some(Ask::List));
}

#[test]
fn alarms_are_a_time_of_day() {
    assert_eq!(ask("set an alarm for 7:30 am"), Some(Ask::Alarm { at: NaiveTime::from_hms_opt(7, 30, 0).unwrap(), label: "Alarm".into() }));
    assert_eq!(ask("wake me up at 6am"), Some(Ask::Alarm { at: NaiveTime::from_hms_opt(6, 0, 0).unwrap(), label: "Alarm".into() }));
    assert_eq!(ask("set an alarm for 8 hours from now").is_some(), false, "'from now' is not 'in': needs the plain form below");
    assert_eq!(ask("wake me in 8 hours"), Some(set(28800, "Alarm")));
    assert_eq!(ask("cancel my alarm"), Some(Ask::Cancel { label: "alarm".into(), all: false }));
}

#[test]
fn other_sentences_are_not_timers() {
    for q in [
        "how do I set a timer in Python",
        "what is a timer interrupt",
        "explain what an alarm clock does",
        "set a timer",
        "set a timer for later",
        "what time is it",
        "remind me to call mom in 10 minutes",
        "the timer on my oven is broken",
        "set a timer for 3 days",
        "tell me about the alarm system market",
    ] {
        assert_eq!(ask(q), None, "{q}");
    }
}

#[test]
fn setting_asking_and_cancelling_change_the_list() {
    let mut timers = Vec::new();
    let d = apply(&mut timers, set(600, "pasta"), at(15, 30));
    assert_eq!(timers.len(), 1);
    assert_eq!(d.card, "Pasta timer set for 10 minutes");
    assert!(d.notes.contains("10 minutes") && d.notes.contains("3:40 PM"), "{}", d.notes);
    apply(&mut timers, set(90, ""), at(15, 31));
    assert_eq!(timers.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 2]);
    // How long is left: the pasta timer started at 15:30 for 10 minutes; at 15:35 five minutes remain.
    let d = apply(&mut timers, Ask::Left { label: "pasta".into() }, at(15, 35));
    assert!(d.notes.contains("5 minutes") && d.notes.contains("pasta timer"), "{}", d.notes);
    // Two running and no name: it asks which.
    let d = apply(&mut timers, Ask::Cancel { label: String::new(), all: false }, at(15, 35));
    assert_eq!(d.card, "Which timer?");
    assert_eq!(timers.len(), 2);
    let d = apply(&mut timers, Ask::Cancel { label: "pasta".into(), all: false }, at(15, 35));
    assert_eq!(d.removed, vec![1]);
    assert_eq!(timers.len(), 1);
    // Only one left: no name needed.
    apply(&mut timers, Ask::Cancel { label: String::new(), all: false }, at(15, 36));
    assert!(timers.is_empty());
    let d = apply(&mut timers, Ask::List, at(15, 36));
    assert_eq!(d.card, "No timers running");
}

#[test]
fn an_alarm_for_a_time_that_has_passed_is_for_tomorrow() {
    let mut timers = Vec::new();
    let d = apply(&mut timers, Ask::Alarm { at: NaiveTime::from_hms_opt(7, 0, 0).unwrap(), label: "Alarm".into() }, at(21, 0));
    assert_eq!(d.card, "Alarm set for 7:00 AM tomorrow");
    assert_eq!(timers[0].ends - ms(at(21, 0)), 10 * 3600 * 1000);
    let d = apply(&mut Vec::new(), Ask::Alarm { at: NaiveTime::from_hms_opt(23, 0, 0).unwrap(), label: "Alarm".into() }, at(21, 0));
    assert_eq!(d.card, "Alarm set for 11:00 PM");
}

#[test]
fn durations_read_out_plainly() {
    assert_eq!(duration_text(45), "45 seconds");
    assert_eq!(duration_text(60), "1 minute");
    assert_eq!(duration_text(90), "1 minute 30 seconds");
    assert_eq!(duration_text(3600 + 300 + 3), "1 hour 5 minutes 3 seconds");
}

#[test]
fn timers_survive_a_restart_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    assert!(load(&path).is_empty());
    let list = vec![Timer { id: 4, label: "pasta".into(), ends: 1_000, alarm: false }, Timer { id: 5, label: "Alarm".into(), ends: 2_000, alarm: true }];
    save(&path, &list);
    assert_eq!(load(&path), list);
    std::fs::write(&path, "not json").unwrap();
    assert!(load(&path).is_empty(), "a damaged file starts empty instead of failing");
}

#[test]
fn the_alarm_toast_is_valid_and_escapes_the_label() {
    let xml = crate::toast::timer_xml("Timer done", "Mom's <b>pasta</b> & rice");
    assert!(xml.contains(r#"scenario="alarm""#) && xml.contains("Looping.Alarm2"));
    assert!(xml.contains("Mom&apos;s &lt;b&gt;pasta&lt;/b&gt; &amp; rice") || xml.contains("Mom's &lt;b&gt;pasta&lt;/b&gt; &amp; rice"), "{xml}");
    assert!(!xml.contains("<b>"));
}

#[tokio::test]
async fn a_timer_waits_for_its_time_and_a_cancelled_one_never_rings() {
    // Ids far from the ones other tests use, since the list is shared.
    let soon = Timer { id: 9_000_001, label: "egg".into(), ends: now_ms() + 120, alarm: false };
    let cancelled = Timer { id: 9_000_002, label: "pasta".into(), ends: now_ms() + 400, alarm: false };
    TIMERS.lock().unwrap().extend([soon.clone(), cancelled.clone()]);
    let c = cancelled.clone();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(60)).await;
        TIMERS.lock().unwrap().retain(|x| x.id != c.id);
    });
    let started = std::time::Instant::now();
    assert!(wait_for(&soon, 20).await, "it ended");
    assert!(started.elapsed() >= Duration::from_millis(100), "not before its time: {:?}", started.elapsed());
    assert!(!wait_for(&cancelled, 20).await, "a cancelled timer is not rung");
    canceller.await.unwrap();
    // Taking it off the list works once; a second ring would find it gone.
    assert!(take(&soon).is_some());
    assert!(take(&soon).is_none());
}

#[tokio::test]
async fn a_timer_whose_time_has_passed_while_the_pc_slept_rings_at_once() {
    let late = Timer { id: 9_000_003, label: "late".into(), ends: now_ms() - 5_000, alarm: false };
    TIMERS.lock().unwrap().push(late.clone());
    let started = std::time::Instant::now();
    assert!(wait_for(&late, 30_000).await);
    assert!(started.elapsed() < Duration::from_millis(500));
    take(&late);
}

#[test]
fn the_spoken_line_names_what_rang() {
    let t = |label: &str, alarm: bool| Timer { id: 1, label: label.into(), ends: 0, alarm };
    assert_eq!(say(&t("pasta", false)), "Your pasta timer is done.");
    assert_eq!(say(&t("timer", false)), "Your timer is done.");
    assert_eq!(say(&t("", true)), "Your alarm is going off.");
    assert_eq!(say(&t("wake up", true)), "Your wake up is going off.");
}

#[test]
fn a_26th_timer_is_refused() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap().and_hms_opt(12, 0, 0).unwrap();
    let mut list: Vec<Timer> = (1..=25).map(|i| Timer { id: i, label: String::new(), ends: i as i64 * 1000, alarm: false }).collect();
    let done = apply(&mut list, Ask::Set { secs: 60, label: String::new() }, now);
    assert_eq!(done.card, "Too many timers");
    assert_eq!(list.len(), 25);
}
