use super::*;

fn s(talk: &str, lock: &str) -> Value {
    json!({ "talkMode": talk, "voiceLock": lock, "wakeWord": true, "personality": { "warmth": 3 }, "userName": "Logan" })
}

#[test]
fn only_what_follows_the_person_is_noticed() {
    assert_eq!(changed_keys(&s("casual", "off"), &s("direct", "off")), vec!["talk_mode"]);
    assert_eq!(changed_keys(&s("casual", "off"), &s("casual", "all")), vec!["voice_lock"]);
    // The wake word switch is this device's own business.
    let mut b = s("casual", "off");
    b["wakeWord"] = json!(false);
    assert!(changed_keys(&s("casual", "off"), &b).is_empty());
    assert!(KEYS.iter().all(|(_, setting)| !["wakeWord", "quickAskKeys", "selectionKeys", "bytVoice", "byteVoice", "speechVoice", "voiceModel"].contains(setting)));
}

#[test]
fn a_device_sends_only_what_it_changed_itself() {
    let mut clock = Clock::default();
    assert_eq!(outgoing(&s("casual", "off"), &clock, None), json!({}), "a fresh device has changed nothing");
    clock.0.insert("talk_mode".into(), 500);
    clock.0.insert(LEARNED.into(), 700);
    let o = outgoing(&s("direct", "off"), &clock, Some(&json!({ "seen": [] })));
    assert_eq!(o["talk_mode"], json!({ "v": "direct", "t": 500 }));
    assert_eq!(o[LEARNED]["t"], 700);
    assert!(o.get("voice_lock").is_none());
}

#[test]
fn newer_values_from_the_account_are_applied_and_older_ones_are_not() {
    let mut clock = Clock::default();
    clock.0.insert("talk_mode".into(), 500);
    clock.0.insert("voice_lock".into(), 900);
    let remote = json!({
        "talk_mode": { "v": "unhinged", "t": 600 },      // newer than this device's 500: applied
        "voice_lock": { "v": "off", "t": 100 },          // older than 900: ignored
        "user_name": { "v": "Logan S", "t": 50 },        // this device never set it: the account's is applied
        "something_new": { "v": 1, "t": 999 },           // a key this version doesn't know: ignored
        "personality": { "v": { "warmth": 3 }, "t": 40 } // already the same: no change, but the time is adopted
    });
    let (patch, learned, next) = incoming(&remote, &s("casual", "actions"), &clock);
    assert_eq!(patch, json!({ "talkMode": "unhinged", "userName": "Logan S" }));
    assert!(learned.is_none());
    assert_eq!(next.0["talk_mode"], 600);
    assert_eq!(next.0["voice_lock"], 900);
    assert_eq!(next.0["personality"], 40);
    assert!(!next.0.contains_key("something_new"));
    // Asking again with the new clock brings nothing more: no ping-pong between devices.
    let (again, _, _) = incoming(&remote, &s("unhinged", "actions"), &next);
    assert_eq!(again, json!({}));
}

#[test]
fn what_auto_learned_comes_back_as_a_whole_when_newer() {
    let mut clock = Clock::default();
    clock.0.insert(LEARNED.into(), 100);
    let (patch, learned, next) = incoming(&json!({ LEARNED: { "v": { "seen": [], "swearing": false }, "t": 200 } }), &s("auto", "off"), &clock);
    assert_eq!(patch, json!({}));
    assert_eq!(learned, Some(json!({ "seen": [], "swearing": false })));
    assert_eq!(next.0[LEARNED], 200);
    assert!(incoming(&json!({ LEARNED: { "v": {}, "t": 50 } }), &s("auto", "off"), &clock).1.is_none());
}

#[test]
fn the_clock_file_round_trips_and_a_damaged_one_starts_empty() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join(CLOCK_FILE);
    assert_eq!(load_clock(&p), Clock::default());
    let mut c = Clock::default();
    c.0.insert("talk_mode".into(), 5);
    save_clock(&p, &c);
    assert_eq!(load_clock(&p), c);
    std::fs::write(&p, "??").unwrap();
    assert_eq!(load_clock(&p), Clock::default());
}
