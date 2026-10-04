use super::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// Wednesday, September 30, 2026.
fn today() -> NaiveDate {
    d(2026, 9, 30)
}

#[test]
fn dates_are_read_from_words() {
    let t = today();
    assert_eq!(date_in("March 3", t), Some(d(2027, 3, 3)));
    assert_eq!(date_in("on the 3rd of march", t), Some(d(2027, 3, 3)));
    assert_eq!(date_in("Oct 12th", t), Some(d(2026, 10, 12)));
    assert_eq!(date_in("december 25, 2027", t), Some(d(2027, 12, 25)));
    assert_eq!(date_in("5/14", t), Some(d(2027, 5, 14)));
    assert_eq!(date_in("12/25/26", t), Some(d(2026, 12, 25)));
    assert_eq!(date_in("tomorrow", t), Some(d(2026, 10, 1)));
    assert_eq!(date_in("arriving friday", t), Some(d(2026, 10, 2)));
    assert_eq!(date_in("due on the 12th", t), Some(d(2026, 10, 12)));
    assert_eq!(date_in("on the 30th", t), Some(d(2026, 9, 30)));
    assert_eq!(date_in("feb 29", t), Some(d(2027, 2, 28)), "no Feb 29 next year: the 28th");
    assert_eq!(date_in("may I ask something", t), None, "\"may\" alone isn't a date");
    assert_eq!(date_in("5 things", t), None);
}

#[test]
fn series_move_on_and_months_keep_their_day() {
    assert_eq!(next_on_or_after(d(2026, 1, 31), "monthly", d(2026, 2, 10)), Some(d(2026, 2, 28)));
    assert_eq!(next_on_or_after(d(2026, 1, 31), "monthly", d(2026, 3, 1)), Some(d(2026, 3, 31)), "back to the 31st");
    assert_eq!(next_on_or_after(d(2024, 3, 3), "yearly", today()), Some(d(2027, 3, 3)));
    assert_eq!(next_on_or_after(d(2026, 9, 30), "yearly", today()), Some(d(2026, 9, 30)), "today counts");
    assert_eq!(next_on_or_after(d(2026, 9, 1), "weekly", today()), Some(d(2026, 10, 6)));
    assert_eq!(next_on_or_after(d(2026, 7, 15), "quarterly", today()), Some(d(2026, 10, 15)));
}

#[test]
fn carriers_are_recognized_from_the_number() {
    let c = |n: &str| carrier_of(n).map(|(c, _)| c);
    assert_eq!(c("1Z999AA10123456784"), Some("UPS"));
    assert_eq!(c("9400 1000 0000 0000 0000 00"), Some("USPS"));
    assert_eq!(c("EA123456789US"), Some("USPS"));
    assert_eq!(c("123456789012"), Some("FedEx"));
    assert_eq!(c("1234567890"), Some("DHL"));
    assert_eq!(c("TBA123456789012"), Some("Amazon"));
    assert_eq!(c("hello"), None);
    assert_eq!(c("12345"), None);
    assert_eq!(carrier_of("1z999aa10123456784").unwrap().1, "https://www.ups.com/track?tracknum=1Z999AA10123456784");
}

#[test]
fn money_and_cycles() {
    assert_eq!(money_in("Netflix $15.49 a month"), Some((15.49, "USD".into())));
    assert_eq!(money_in("rent is $1,500 monthly"), Some((1500.0, "USD".into())));
    assert_eq!(money_in("Spotify €9,99"), Some((9.99, "EUR".into())));
    assert_eq!(money_in("costs 12 dollars"), Some((12.0, "USD".into())));
    assert_eq!(money_in("no money here"), None);
    assert_eq!(money_text(15.0, "USD"), "$15");
    assert_eq!(money_text(9.99, "EUR"), "€9.99");
    assert_eq!(money_text(1500.0, "USD"), "$1,500");
    assert_eq!(money_text(20916.76, "USD"), "$20,916.76");
    assert_eq!(money_text(999.5, "USD"), "$999.50");
    assert_eq!(money_text(1234567.0, "USD"), "$1,234,567");
    let bill = |amount: f64, cycle: &str| Tracker { kind: Kind::Bill, amount: Some(amount), cycle: cycle.into(), ..Default::default() };
    assert_eq!(per_month(&bill(120.0, "yearly")), 10.0);
    assert_eq!(per_month(&bill(30.0, "quarterly")), 10.0);
    assert!((per_month(&bill(12.0, "weekly")) - 52.0).abs() < 1e-9);
}

#[test]
fn requests_are_understood() {
    let t = today();
    let add = |q: &str| match ask(q, t) {
        Some(Ask::Add(x)) => x,
        other => panic!("{q}: {other:?}"),
    };
    let p = add("track my package 1Z999AA10123456784 for my new running shoes, arriving friday");
    assert_eq!((p.kind, p.carrier.as_str(), p.number.as_str(), p.name.as_str(), p.next), (Kind::Package, "UPS", "1Z999AA10123456784", "New running shoes", Some(d(2026, 10, 2))));
    let b = add("Add Netflix $15.49 a month on the 12th");
    assert_eq!((b.kind, b.name.as_str(), b.amount, b.cycle.as_str(), b.next), (Kind::Bill, "Netflix", Some(15.49), "monthly", Some(d(2026, 10, 12))));
    let b = add("my rent is $1,500 per month due on the 1st");
    assert_eq!((b.name.as_str(), b.amount, b.next), ("Rent", Some(1500.0), Some(d(2026, 10, 1))));
    let b = add("track my Spotify subscription, $11.99 monthly");
    assert_eq!(b.name, "Spotify");
    assert_eq!(add("Amazon Prime costs $139 a year").cycle, "yearly");
    let e = add("Sam's birthday is March 3");
    assert_eq!((e.kind, e.name.as_str(), e.person.as_str(), e.next), (Kind::Event, "Sam's birthday", "Sam", Some(d(2027, 3, 3))));
    let e = add("our anniversary is June 12");
    assert_eq!((e.name.as_str(), e.person.as_str()), ("Our anniversary", ""));
    let e = add("Mom's birthday is on 5/14, budget $80");
    assert_eq!((e.person.as_str(), e.budget), ("Mom", Some(80.0)));
    let u = add("change the furnace filter every 3 months");
    assert_eq!((u.kind, u.name.as_str(), u.every_months, u.every_days), (Kind::Upkeep, "Change the furnace filter", Some(3), None));
    let u = add("track oil changes every 6 months or 5,000 miles");
    assert_eq!((u.name.as_str(), u.every_months), ("Oil changes", Some(6)));
    assert!(u.notes.contains("5,000 miles"));
    assert_eq!(add("rotate the tires every year").every_months, Some(12));
    assert_eq!(add("clean the dryer vent every 2 weeks").every_days, Some(14));

    assert_eq!(ask("what's coming up?", t), Some(Ask::List(None)));
    assert_eq!(ask("What subscriptions do I have?", t), Some(Ask::List(Some(Kind::Bill))));
    assert_eq!(ask("where are my packages", t), Some(Ask::List(Some(Kind::Package))));
    assert_eq!(ask("any upcoming birthdays?", t), Some(Ask::List(Some(Kind::Event))));
    assert_eq!(ask("I changed the oil today", t), Some(Ask::Done { what: "oil".into(), kind: Some(Kind::Upkeep) }));
    assert_eq!(ask("my running shoes arrived", t), Some(Ask::Done { what: "running shoes".into(), kind: Some(Kind::Package) }));
    assert_eq!(ask("I cancelled Netflix", t), Some(Ask::Done { what: "netflix".into(), kind: Some(Kind::Bill) }));
    assert_eq!(ask("add AirPods to Sam's gift ideas", t), Some(Ask::Idea { person: "Sam".into(), idea: "AirPods".into() }));
    assert_eq!(ask("gift ideas for Sam?", t), Some(Ask::Suggest { person: "Sam".into() }));
}

#[test]
fn ordinary_messages_are_left_alone() {
    for q in [
        "how much does Netflix cost a month?",
        "what is a tracking number",
        "remind me to change the oil every 3 months",
        "every weekday at 8am summarize AI news",
        "When is Lincoln's birthday",
        "I changed my mind",
        "my flight is at 9",
        "what's the weather like",
        "tell me about the birthday paradox",
    ] {
        let a = ask(q, today());
        assert!(!matches!(a, Some(Ask::Add(_)) | Some(Ask::Idea { .. })), "{q}: {a:?}");
    }
    assert!(ask("I changed my mind", today()).is_some_and(|a| matches!(a, Ask::Done { .. })), "a Done that matches nothing is answered as not tracked");
}

fn test_db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).unwrap();
    (dir, db)
}

#[test]
fn saved_trackers_get_their_dates_and_notify_once() {
    let (_d, db) = test_db();
    let t = today();
    let bill = save(&db, &Tracker { kind: Kind::Bill, name: "Netflix".into(), amount: Some(15.49), cycle: "monthly".into(), next: Some(d(2026, 10, 2)), ..Default::default() }, t).unwrap();
    assert_eq!(bill.next, Some(d(2026, 10, 2)));
    let filter = save(&db, &Tracker { kind: Kind::Upkeep, name: "Change the furnace filter".into(), every_months: Some(3), last_done: Some(d(2026, 6, 1)), ..Default::default() }, t).unwrap();
    assert_eq!(filter.next, Some(d(2026, 9, 1)), "overdue");
    let bday = save(&db, &Tracker { kind: Kind::Event, name: "Sam's birthday".into(), person: "Sam".into(), next: Some(d(2020, 3, 3)), ..Default::default() }, t).unwrap();
    assert_eq!(bday.next, Some(d(2027, 3, 3)));
    let pkg = save(&db, &Tracker { kind: Kind::Package, number: "1z999aa10123456784".into(), ..Default::default() }, t).unwrap();
    assert_eq!((pkg.name.as_str(), pkg.carrier.as_str()), ("UPS package", "UPS"));
    assert!(pkg.link.starts_with("https://www.ups.com/"));
    assert!(save(&db, &Tracker { kind: Kind::Upkeep, name: "x".into(), ..Default::default() }, t).is_err(), "how often is needed");
    assert!(save(&db, &Tracker { kind: Kind::Event, name: "x".into(), ..Default::default() }, t).is_err(), "a date is needed");

    // Bill due in 2 days (notice 3) and the overdue filter: once each.
    let due = take_due(&db, t).unwrap();
    let names: Vec<&str> = due.iter().map(|(t, _)| t.name.as_str()).collect();
    assert_eq!(names, ["Change the furnace filter", "Netflix"]);
    assert_eq!(due[1].1, "Netflix is due in 2 days: $15.49.");
    assert!(due[0].1.contains("overdue"));
    assert!(take_due(&db, t).unwrap().is_empty(), "sent once");

    // After the bill's date it moves on by itself (and a new notice comes near the next one).
    let later = d(2026, 10, 3);
    assert!(take_due(&db, later).unwrap().is_empty());
    assert_eq!(get(&db, bill.id).unwrap().unwrap().next, Some(d(2026, 11, 2)));
    assert_eq!(take_due(&db, d(2026, 10, 30)).unwrap().len(), 1);

    // Maintenance done today: the next date moves 3 months on.
    let done = mark_done(&db, filter.id, t).unwrap();
    assert_eq!((done.last_done, done.next), (Some(t), Some(d(2026, 12, 30))));
    // A delivered package leaves the lists.
    assert!(mark_done(&db, pkg.id, t).unwrap().done);
    assert!(!compose(&list(&db).unwrap(), Some(Kind::Package), t).contains("UPS package ("));
}

#[test]
fn lists_are_composed_with_exact_amounts() {
    let t = today();
    let all = vec![
        Tracker { id: 1, kind: Kind::Bill, name: "Netflix".into(), amount: Some(15.49), cycle: "monthly".into(), next: Some(d(2026, 10, 12)), ..Default::default() },
        Tracker { id: 2, kind: Kind::Bill, name: "Amazon Prime".into(), amount: Some(139.0), cycle: "yearly".into(), next: Some(d(2027, 2, 1)), ..Default::default() },
        Tracker { id: 3, kind: Kind::Event, name: "Sam's birthday".into(), next: Some(d(2026, 10, 9)), ideas: vec!["AirPods".into()], ..Default::default() },
    ];
    let bills = compose(&all, Some(Kind::Bill), t);
    assert!(bills.contains("about **$27.07 a month**, $324.88 a year"), "{bills}");
    assert!(bills.contains("| Netflix | $15.49 a month | in 12 days |"), "{bills}");
    let soon = compose(&all, None, t);
    assert!(soon.contains("**Sam's birthday**: in 9 days"), "{soon}");
    assert!(soon.contains("**Netflix**: in 12 days · $15.49"), "{soon}");
    assert!(!soon.contains("Amazon Prime"), "more than 30 days away");
    assert!(compose(&[], Some(Kind::Upkeep), t).contains("furnace filter every 3 months"));
}
