use super::*;

fn feed(l: &mut Learned, msgs: &[(&str, u32)]) {
    for (t, d) in msgs {
        l.observe(t, *d);
    }
}

fn filler(l: &mut Learned, n: usize, day0: u32, days: u32) {
    for i in 0..n {
        l.observe("can you tell me what the weather will be like tomorrow", day0 + (i as u32 % days));
    }
}

#[test]
fn swearing_is_noticed_and_banter_at_byte_is_not() {
    assert!(swears("this is so fucking annoying") && swears("what the f*ck") && swears("wtf"));
    assert!(!swears("what a classic bassoon hell yes") || swears("hell yes") == false);
    assert!(aimed_at_byte("fuck you byte") && aimed_at_byte("you are a useless piece of shit") && aimed_at_byte("byte you suck"));
    assert!(!aimed_at_byte("this traffic is fucking insane") && !aimed_at_byte("shit, I forgot my keys"));
    assert!(observe_text("fuck you byte lol", 1).is_some_and(|o| !o.swears), "swearing at BYTE is not their style");
    assert!(observe_text("this is fucking great", 1).is_some_and(|o| o.swears));
    assert!(observe_text("  ", 1).is_none());
}

#[test]
fn nothing_is_decided_early_and_one_swear_changes_nothing() {
    let mut l = Learned::default();
    feed(&mut l, &[("fuck yeah that worked", 1), ("fuck you byte, just kidding", 1), ("shit that is good", 1)]);
    assert!(!l.profile().enough && !l.swearing);
    filler(&mut l, 60, 1, 1);
    l.observe("holy shit", 1);
    assert!(l.profile().enough, "enough messages now");
    assert!(!l.swearing, "a handful of swears on one day is not a pattern");
}

#[test]
fn a_steady_pattern_over_several_days_is_learned_and_it_fades_when_it_stops() {
    let mut l = Learned::default();
    for day in 0..6u32 {
        for i in 0..10 {
            l.observe(if i % 5 == 0 { "ugh this is fucking annoying" } else { "what time does the store open today" }, 100 + day);
        }
    }
    assert!(l.swearing, "20% of messages over 6 days");
    assert!(l.profile().swears);
    // A quiet stretch does not switch it off (no flip-flopping)...
    for i in 0..100 {
        l.observe("what is the weather like in memphis today", 200 + (i / 10) as u32);
    }
    assert!(l.swearing, "still on after a short quiet stretch");
    // ...a long one does.
    for i in 0..260 {
        l.observe("what is the weather like in memphis today", 220 + (i / 10) as u32);
    }
    assert!(!l.swearing);
}

#[test]
fn a_streak_on_one_or_two_days_is_not_enough() {
    let mut l = Learned::default();
    filler(&mut l, 40, 10, 5);
    for i in 0..12 {
        l.observe("this is fucking ridiculous", 50 + (i % 2));
    }
    assert!(!l.swearing, "only two days");
}

#[test]
fn terse_and_formal_writers_are_noticed() {
    let mut l = Learned::default();
    for i in 0..60 {
        l.observe("timer 5 min", i % 7);
    }
    let p = l.profile();
    assert!(p.enough && p.terse && !p.formal);
    let mut f = Learned::default();
    for i in 0..60 {
        f.observe("Could you please tell me about the schedule for the upcoming week, thank you", i % 7);
    }
    assert!(f.profile().formal && !f.profile().terse);
    assert!(f.describe().contains("polite"));
    assert!(Learned::default().describe().starts_with("Still learning"));
}

#[test]
fn the_text_is_never_kept() {
    let mut l = Learned::default();
    l.observe("my secret password is hunter2 and my address is 1 main street", 1);
    let json = serde_json::to_string(&l).unwrap();
    assert!(!json.contains("hunter2") && !json.contains("main street"), "{json}");
}

#[test]
fn each_mode_asks_for_the_right_thing() {
    let l = Learned::default();
    assert_eq!(section("", true, &l, 0), "");
    assert!(section("casual", false, &l, 0).contains("relaxed friend"));
    assert!(section("direct", false, &l, 0).contains("answer first"));
    let u = section("unhinged", true, &l, 0);
    assert!(u.contains("foul-mouthed") && u.contains("dark jokes") && u.contains("cartoonish violence") && u.contains("entertainment only"), "{u}");
    assert!(u.contains("that's what she said") && u.contains("never explain it"), "{u}");
    assert!(u.contains("one in three") && u.contains("call back"), "{u}");
    assert!(u.contains("minors") && u.contains("real crisis") && u.contains("drop the act") && u.contains("never seriously plan"), "{u}");
    assert!(!u.contains("HARD LIMITS") && !u.contains("slurs"), "no other limits: {u}");
    // The adult switch is what makes Unhinged available.
    assert!(!section("unhinged", false, &l, 0).contains("foul-mouthed"));
    assert!(valid("auto") && valid("") && !valid("evil"));
}

#[test]
fn auto_follows_the_person_but_only_as_far_as_they_allow() {
    let mut l = Learned::default();
    for day in 0..6u32 {
        for i in 0..10 {
            l.observe(if i % 4 == 0 { "damn that is fucking cool" } else { "ok" }, day);
        }
    }
    assert!(l.swearing && l.profile().terse);
    let allowed = section("auto", true, &l, 0);
    assert!(allowed.contains("Be direct") && allowed.contains("mild swearing"), "{allowed}");
    let not_allowed = section("auto", false, &l, 0);
    assert!(!not_allowed.contains("swearing") && not_allowed.contains("Be direct"), "{not_allowed}");
    // Never Unhinged by itself.
    assert!(!allowed.contains("foul-mouthed"));
    // Before it knows anything: Casual.
    assert!(section("auto", true, &Learned::default(), 0).contains("relaxed friend"));
}

#[test]
fn the_file_round_trips_and_a_damaged_one_starts_empty() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("style.json");
    assert_eq!(load(&p), Learned::default());
    let mut l = Learned::default();
    filler(&mut l, 5, 3, 2);
    save(&p, &l);
    assert_eq!(load(&p), l);
    std::fs::write(&p, "nope").unwrap();
    assert_eq!(load(&p), Learned::default());
}

#[test]
fn every_joke_is_pushed_toward_something_new() {
    let l = Learned::default();
    let a = section("unhinged", true, &l, 0);
    let b = section("unhinged", true, &l, 1);
    assert_ne!(a, b, "a different angle each time");
    for s in [&a, &b, &section("casual", false, &l, 3), &section("auto", false, &l, 5)] {
        assert!(s.contains("brand new") && s.contains("Never reuse a joke") && s.contains("stock joke"), "{s}");
    }
    // Direct does not joke, so it is not told how to.
    assert!(!section("direct", false, &l, 0).contains("brand new"));
    // A terse Auto person is treated as Direct: no joke push either.
    let mut terse = Learned::default();
    for i in 0..60 {
        terse.observe("ok", i % 7);
    }
    assert!(!section("auto", false, &terse, 0).contains("brand new"));
    // Every clean angle is used by some index, and the list wraps.
    let all: std::collections::HashSet<String> = (0..ANGLES.len()).map(|i| section("casual", false, &l, i)).collect();
    assert_eq!(all.len(), ANGLES.len());
    assert_eq!(section("casual", false, &l, 0), section("casual", false, &l, ANGLES.len()));
}

#[test]
fn unhinged_gets_the_adult_angles_and_nobody_else_does() {
    let l = Learned::default();
    // Adult angles only with Unhinged and the adult switch.
    let edgy: std::collections::HashSet<String> = (0..EDGY_ANGLES.len() * 2).map(|i| section("unhinged", true, &l, i)).collect();
    assert!(edgy.iter().any(|s| s.contains(EDGY_ANGLES[0])));
    for mode in ["casual", "direct", "auto"] {
        for i in 0..400 {
            let s = section(mode, true, &l, i);
            assert!(EDGY_ANGLES.iter().all(|a| !s.contains(a)), "{mode} {i}");
        }
    }
    // Unhinged without the switch is just Casual.
    for i in 0..400 {
        assert!(EDGY_ANGLES.iter().all(|a| !section("unhinged", false, &l, i).contains(a)));
    }
}

#[test]
fn the_angle_lists_are_big_and_have_no_repeats() {
    for list in [ANGLES, EDGY_ANGLES] {
        let set: std::collections::HashSet<&&str> = list.iter().collect();
        assert_eq!(set.len(), list.len(), "a repeated angle");
    }
    assert!(ANGLES.len() >= 100 && EDGY_ANGLES.len() >= 60);
}

// --- track:house: a separate picture per taught voice ---

#[test]
fn each_taught_voice_learns_alone_and_unknown_voices_use_the_default() {
    let mut l = Learned::default();
    for i in 0..60 {
        l.observe_for(Some("Sam"), "ok", i % 5);
        l.observe_for(None, "could you please tell me what the weather will be like tomorrow, thank you", i % 5);
    }
    assert!(l.of(Some("sam")).profile().terse, "Sam keeps messages short");
    assert!(l.of(Some(" SAM ")).profile().enough, "names match without case or spaces");
    assert!(!l.of(None).profile().terse && l.of(None).profile().formal, "the default learned from the other messages only");
    assert!(!l.of(Some("Alex")).profile().enough, "a voice with no messages yet starts fresh, not on Sam's picture");
    assert_eq!(l.of(Some("")).seen.len(), 60, "a blank name is unknown");
    assert!(section_for("auto", false, &l, Some("sam"), 0).contains("answer first"));
    assert!(section_for("auto", false, &l, None, 0).contains("relaxed friend"));
}

#[test]
fn old_files_load_unchanged_and_an_empty_people_list_is_not_written() {
    let old = r#"{"seen":[{"day":1,"words":3,"swears":false,"formal":false}],"swearing":false}"#;
    let l: Learned = serde_json::from_str(old).unwrap();
    assert_eq!(l.seen.len(), 1);
    assert!(l.people.is_empty());
    assert!(!serde_json::to_string(&l).unwrap().contains("people"));
    let mut m = l.clone();
    m.observe_for(Some("Sam"), "hello there", 2);
    let back: Learned = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back, m);
    assert!(back.describe().contains("sam:"));
}

#[test]
fn only_so_many_people_are_kept_apart() {
    let mut l = Learned::default();
    for i in 0..30 {
        l.observe_for(Some(&format!("person{i}")), "hi there", 1);
    }
    assert_eq!(l.people.len(), 20);
    assert_eq!(l.seen.len(), 10, "the rest share the default");
}
