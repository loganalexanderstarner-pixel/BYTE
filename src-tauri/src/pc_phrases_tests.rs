//! How people actually word PC requests in the chat, and which flow each one reaches. The agent tries the flows in
//! this order (`agent::specialist`): PC control, files, upkeep. A question that reaches none of them goes to the
//! model, which then says it can't see the PC; the first live chat test on a real PC found three of those.


fn owner(q: &str) -> &'static str {
    let q = crate::router::without_wake_phrase(q);
    if crate::timers::applies(q) {
        "timer"
    } else if crate::macctl::wants_on(q, true) {
        "control"
    } else if crate::filectl::ask(q).is_some() {
        "files"
    } else if crate::upkeep::ask(q).is_some() {
        "upkeep"
    } else {
        "model"
    }
}

#[test]
fn pc_requests_reach_the_flow_that_does_them_and_questions_stay_with_the_model() {
    let table: &[(&str, &str)] = &[
        // Control.
        ("turn on dark mode", "control"),
        ("Switch my PC to dark mode", "control"),
        ("go dark", "control"),
        ("switch to light mode", "control"),
        ("set the volume to 40", "control"),
        ("Set my volume to 23 percent", "control"),
        ("turn the volume down to 10", "control"),
        ("volume 30", "control"),
        ("mute my computer", "control"),
        ("mute", "control"),
        ("unmute", "control"),
        ("turn off wifi", "control"),
        ("turn the wifi back on", "control"),
        ("put my screen to sleep", "control"),
        ("open bluetooth settings", "control"),
        ("open the display settings", "control"),
        ("open sound settings", "control"),
        ("open windows update", "control"),
        ("pause the music", "control"),
        ("skip this song", "control"),
        ("next song", "control"),
        ("play some jazz", "control"),
        ("what's playing right now", "control"),
        // Timers and alarms.
        ("set a timer for ten minutes", "timer"),
        ("Hey, BYTE, set a pasta timer for 12 minutes", "timer"),
        ("how much time is left on my timer", "timer"),
        ("cancel the timer", "timer"),
        ("set an alarm for 7 am", "timer"),
        ("wake me up at 6:30", "timer"),
        ("what timers do I have", "timer"),
        // Files.
        ("find my resume", "files"),
        ("find my tax return pdf", "files"),
        ("where is my cv", "files"),
        ("locate the budget spreadsheet", "files"),
        ("tidy up my desktop", "files"),
        ("organize my downloads folder", "files"),
        ("Tidy up my Downloads folder", "files"),
        ("sort my downloads by month", "files"),
        // Upkeep.
        ("how much space do I have left", "upkeep"),
        ("my disk is almost full", "upkeep"),
        ("free up some space", "upkeep"),
        ("What is taking up space on my PC?", "upkeep"),
        ("find duplicate files", "upkeep"),
        ("what are my biggest files", "upkeep"),
        ("why is my pc so slow", "upkeep"),
        ("What is making my PC slow right now?", "upkeep"),
        ("my computer is lagging", "upkeep"),
        ("my laptop is overheating", "upkeep"),
        ("how is my battery health", "upkeep"),
        ("is my pc healthy", "upkeep"),
        ("Give my PC a check-up", "upkeep"),
        ("run a health check", "upkeep"),
        ("uninstall zoom", "upkeep"),
        ("which apps start when windows starts", "upkeep"),
        ("stop spotify from starting with windows", "upkeep"),
        ("what's slowing down my computer", "upkeep"),
        // Second round: looser wording.
        ("turn it up", "control"),
        ("a little louder please", "control"),
        ("make the volume quieter", "control"),
        ("lower my volume", "control"),
        ("stop the music", "control"),
        ("resume the music", "control"),
        ("play the next track", "control"),
        ("disconnect from wifi", "control"),
        ("can you turn the sound off", "control"),
        ("please set the volume to fifty percent", "control"),
        ("find the pdf i downloaded yesterday", "files"),
        ("find my budget.xlsx", "files"),
        ("clean up my desktop", "files"),
        ("declutter my downloads", "files"),
        ("i'm running out of storage", "upkeep"),
        ("my c drive is full", "upkeep"),
        ("delete my duplicate photos", "upkeep"),
        ("what's eating my ram", "upkeep"),
        ("my pc fans are really loud", "upkeep"),
        ("my pc is running hot", "upkeep"),
        ("is my firewall on", "upkeep"),
        ("is defender turned on", "upkeep"),
        ("is my pc encrypted", "upkeep"),
        ("how old is my battery", "upkeep"),
        ("uninstall the app discord", "upkeep"),
        ("stop discord from opening at startup", "upkeep"),
        ("disable discord on startup", "upkeep"),
        ("what programs run at startup", "upkeep"),
        ("list my startup apps", "upkeep"),
        ("how healthy is my computer", "upkeep"),
        ("can you give my laptop a tune up", "upkeep"),
        // Said aloud after the wake phrase, the way speech-to-text writes it.
        ("Hey, BYTE, turn the volume down a bit.", "control"),
        ("Hey BYTE turn it up", "control"),
        ("okay byte, switch to dark mode", "control"),
        ("Byte, is my pc healthy?", "upkeep"),
        ("hey byte what's taking up space on my pc", "upkeep"),
        ("Hey, BYTE. Find my resume.", "files"),
        ("hey byte what is the weather in dover tennessee", "model"),
        // Questions and writing stay with the model.
        ("what is the capital of France", "model"),
        ("how do I turn on dark mode", "model"),
        ("what is dark mode", "model"),
        ("is dark mode on", "model"),
        ("what is a startup item", "model"),
        ("explain what volume means in audio", "model"),
        ("tell me a joke", "model"),
        ("summarize this article for me", "model"),
        ("what is disk space", "model"),
    ];
    let wrong: Vec<String> = table.iter().filter(|(q, want)| owner(q) != *want).map(|(q, want)| format!("{q:?}: wanted {want}, got {}", owner(q))).collect();
    assert!(wrong.is_empty(), "requests that reach the wrong flow:\n{}", wrong.join("\n"));
}

#[test]
fn the_wake_phrase_comes_off_a_spoken_request_and_nothing_else_does() {
    use crate::router::{weather_place, without_wake_phrase as w};
    assert_eq!(w("Hey, BYTE, turn the volume down a bit."), "turn the volume down a bit.");
    assert_eq!(w("hey byte what's the weather"), "what's the weather");
    assert_eq!(w("Okay Byte: set a timer"), "set a timer");
    assert_eq!(w("Byte, what time is it"), "what time is it");
    assert_eq!(w("byte size of a file in linux"), "byte size of a file in linux");
    assert_eq!(w("hey bytes of data"), "hey bytes of data");
    assert_eq!(w("hi there, byte me"), "hi there, byte me");
    // The weather question from a spoken request still finds its town.
    assert_eq!(weather_place("Hey, BYTE, what's the weather in Dover, Tennessee?").as_deref(), Some("Dover, Tennessee"));
    assert_eq!(weather_place("hey byte whats the weather in dover tennessee").as_deref(), Some("dover tennessee"));
}

#[test]
fn a_voice_lock_holds_back_doing_things_and_not_asking_things() {
    for q in ["set a timer for ten minutes", "Hey, BYTE, turn the volume down a bit", "tidy up my downloads folder", "what's taking up space on my PC", "open spotify to my gaming playlist", "uninstall zoom"] {
        assert!(crate::agent::is_pc_action(q), "{q}");
    }
    for q in ["what's the capital of France", "tell me a joke", "Hey BYTE, what's the weather in Dover, Tennessee", "how do I turn on dark mode", "I'm feeling kind of down today"] {
        assert!(!crate::agent::is_pc_action(q), "{q}");
    }
}
