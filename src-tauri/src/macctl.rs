//! Mac control (Phase 9): BYTE does things in Mac apps when asked. "Remind me to
//! call Mom tomorrow at 3pm", "make a note: …", "what's on my calendar this week",
//! "turn on dark mode", "set the volume to 30", "play some jazz", "run my Morning
//! shortcut", "open Bluetooth settings".
//!
//! Safety by design:
//! - Every AppleScript is a fixed text in this file. The user's words (titles,
//!   notes, search terms) are passed as `argv` items, never pasted into a script,
//!   so nothing they (or the model) write can become code.
//! - Anything that adds or changes something asks first with an approval card;
//!   reading, music and volume don't (they change nothing lasting).
//! - Notes, reminders and events BYTE adds can be undone from the result card.
//! - macOS asks once per app ("BYTE wants to control Notes"); if that was denied,
//!   BYTE says where to turn it back on.
//!
//! Small models pick the details (a title, a time) with `complete_json`; common
//! commands and times ("tomorrow at 3pm", "in 20 minutes") are read by rules, so
//! they work even when the model gets them wrong.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{Datelike, Duration as Days, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Weekday};
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::{self, ChatEvent};
use crate::error::{AppError, AppResult};
use crate::tools::SourceBook;

/// How long an app may take (the first time, macOS waits for the user's Allow).
const RUN_WAIT: Duration = Duration::from_secs(90);
/// How long an approval card waits.
const APPROVAL_WAIT: Duration = Duration::from_secs(600);
/// Field separators in script output (ASCII unit / record separators).
const US: char = '\u{1f}';
const RS: char = '\u{1e}';

// ------------------------------------------------------------------ actions

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    On,
    Off,
    Toggle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    NoteCreate { title: String, body: String },
    NoteFind { query: String },
    ReminderAdd { title: String, due: Option<NaiveDateTime>, list: String },
    RemindersList { list: String },
    EventAdd { title: String, start: NaiveDateTime, minutes: u32, calendar: String },
    EventsList { days: u32 },
    MusicPlay { query: String },
    MusicPause,
    MusicNext,
    MusicPrevious,
    NowPlaying,
    SafariTab,
    DarkMode(Switch),
    Volume(u8),
    Mute(bool),
    Wifi(bool),
    SleepDisplay,
    OpenSettings { pane: String },
    ShortcutsList,
    ShortcutRun { name: String, input: String },
}

/// A fixed program run with the user's words as separate arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `osascript -e <script> <args…>`; the script reads them with `on run argv`.
    Osa { script: &'static str, args: Vec<String> },
    /// A system tool (`pmset`, `open`, `networksetup`, `shortcuts`), no shell.
    Exec { program: &'static str, args: Vec<String> },
}

impl Action {
    /// The app it uses, for the cards and the permission message.
    pub fn app(&self) -> &'static str {
        use Action::*;
        match self {
            NoteCreate { .. } | NoteFind { .. } => "Notes",
            ReminderAdd { .. } | RemindersList { .. } => "Reminders",
            EventAdd { .. } | EventsList { .. } => "Calendar",
            MusicPlay { .. } | MusicPause | MusicNext | MusicPrevious | NowPlaying => "Music",
            SafariTab => "Safari",
            DarkMode(_) => "System Events",
            Volume(_) | Mute(_) | Wifi(_) | SleepDisplay | OpenSettings { .. } => "System Settings",
            ShortcutsList | ShortcutRun { .. } => "Shortcuts",
        }
    }

    /// Tool name for the activity list.
    pub fn tool(&self) -> &'static str {
        use Action::*;
        match self {
            NoteCreate { .. } => "mac_note_create",
            NoteFind { .. } => "mac_note_find",
            ReminderAdd { .. } => "mac_reminder_add",
            RemindersList { .. } => "mac_reminders_list",
            EventAdd { .. } => "mac_event_add",
            EventsList { .. } => "mac_events_list",
            MusicPlay { .. } | MusicPause | MusicNext | MusicPrevious | NowPlaying => "mac_music",
            SafariTab => "mac_safari_tab",
            DarkMode(_) => "mac_dark_mode",
            Volume(_) | Mute(_) => "mac_volume",
            Wifi(_) => "mac_wifi",
            SleepDisplay => "mac_sleep_display",
            OpenSettings { .. } => "mac_open_settings",
            ShortcutsList => "mac_shortcuts_list",
            ShortcutRun { .. } => "mac_shortcut_run",
        }
    }

    /// Adds or changes something that stays: the user approves it first.
    pub fn needs_ok(&self) -> bool {
        matches!(self, Action::NoteCreate { .. } | Action::ReminderAdd { .. } | Action::EventAdd { .. } | Action::Wifi(false) | Action::ShortcutRun { .. })
    }

    /// One line saying what BYTE will do (the approval card's title).
    pub fn describe(&self) -> String {
        use Action::*;
        match self {
            NoteCreate { title, .. } => format!("Add a note \"{title}\" in Notes"),
            NoteFind { query } => format!("Look for notes about \"{query}\""),
            ReminderAdd { title, .. } => format!("Add the reminder \"{title}\""),
            RemindersList { .. } => "Read your reminders".into(),
            EventAdd { title, .. } => format!("Add \"{title}\" to your calendar"),
            EventsList { days } => format!("Read your calendar ({})", if *days <= 1 { "today".to_string() } else { format!("next {days} days") }),
            MusicPlay { query } if query.is_empty() => "Play music".into(),
            MusicPlay { query } => format!("Play \"{query}\" in Music"),
            MusicPause => "Pause the music".into(),
            MusicNext => "Skip to the next song".into(),
            MusicPrevious => "Go back a song".into(),
            NowPlaying => "See what's playing".into(),
            SafariTab => "Read Safari's current tab".into(),
            DarkMode(Switch::On) => "Turn on dark mode".into(),
            DarkMode(Switch::Off) => "Turn off dark mode".into(),
            DarkMode(Switch::Toggle) => "Switch dark mode".into(),
            Volume(v) => format!("Set the volume to {v}%"),
            Mute(true) => "Mute the sound".into(),
            Mute(false) => "Unmute the sound".into(),
            Wifi(true) => "Turn Wi-Fi on".into(),
            Wifi(false) => "Turn Wi-Fi off".into(),
            SleepDisplay => "Turn the display off".into(),
            OpenSettings { pane } => format!("Open {} settings", pane_label(pane)),
            ShortcutsList => "List your shortcuts".into(),
            ShortcutRun { name, .. } => format!("Run the shortcut \"{name}\""),
        }
    }

    /// What goes on the approval card, field by field.
    pub fn fields(&self) -> Vec<(String, String)> {
        use Action::*;
        match self {
            NoteCreate { title, body } => vec![("Title".into(), title.clone()), ("Note".into(), body.chars().take(600).collect())],
            ReminderAdd { title, due, list } => {
                let mut f = vec![("Reminder".into(), title.clone()), ("When".into(), due.map(when_text).unwrap_or_else(|| "No date".into()))];
                if !list.is_empty() {
                    f.push(("List".into(), list.clone()));
                }
                f
            }
            EventAdd { title, start, minutes, calendar } => {
                let mut f = vec![("Event".into(), title.clone()), ("Starts".into(), when_text(*start)), ("Length".into(), minutes_text(*minutes))];
                if !calendar.is_empty() {
                    f.push(("Calendar".into(), calendar.clone()));
                }
                f
            }
            ShortcutRun { name, input } => {
                let mut f = vec![("Shortcut".into(), name.clone())];
                if !input.is_empty() {
                    f.push(("Input".into(), input.clone()));
                }
                f
            }
            Wifi(false) => vec![("Note".into(), "BYTE's web search stops working until Wi-Fi is back on.".into())],
            _ => vec![],
        }
    }

    /// The fixed script or tool for this action.
    pub fn command(&self) -> Command {
        use Action::*;
        let osa = |script: &'static str, args: Vec<String>| Command::Osa { script, args };
        let exec = |program: &'static str, args: Vec<&str>| Command::Exec { program, args: args.into_iter().map(String::from).collect() };
        match self {
            NoteCreate { title, body } => osa(NOTE_CREATE, vec![note_html(title, body)]),
            NoteFind { query } => osa(NOTE_FIND, vec![query.clone()]),
            ReminderAdd { title, due, list } => osa(REMINDER_ADD, vec![title.clone(), due.map(date_arg).unwrap_or_default(), list.clone()]),
            RemindersList { list } => osa(REMINDERS_LIST, vec![list.clone()]),
            EventAdd { title, start, minutes, calendar } => osa(EVENT_ADD, vec![title.clone(), date_arg(*start), minutes.to_string(), calendar.clone()]),
            EventsList { days } => osa(EVENTS_LIST, vec![days.to_string()]),
            MusicPlay { query } => osa(MUSIC_PLAY, vec![query.clone()]),
            MusicPause => osa(MUSIC_CONTROL, vec!["pause".into()]),
            MusicNext => osa(MUSIC_CONTROL, vec!["next".into()]),
            MusicPrevious => osa(MUSIC_CONTROL, vec!["previous".into()]),
            NowPlaying => osa(MUSIC_CONTROL, vec!["now".into()]),
            SafariTab => osa(SAFARI_TAB, vec![]),
            DarkMode(s) => osa(DARK_MODE, vec![match s {
                Switch::On => "on",
                Switch::Off => "off",
                Switch::Toggle => "toggle",
            }
            .into()]),
            Volume(v) => osa(VOLUME, vec![v.to_string()]),
            Mute(m) => osa(MUTE, vec![if *m { "true" } else { "false" }.into()]),
            // The Wi-Fi device (en0, en1…) is looked up first (see `run`).
            Wifi(_) => exec("networksetup", vec!["-listallhardwareports"]),
            SleepDisplay => exec("pmset", vec!["displaysleepnow"]),
            OpenSettings { pane } => Command::Exec { program: "open", args: vec![settings_url(pane)] },
            ShortcutsList => exec("shortcuts", vec!["list"]),
            ShortcutRun { name, .. } => Command::Exec { program: "shortcuts", args: vec!["run".into(), name.clone()] },
        }
    }
}

// ------------------------------------------------------------------ scripts
//
// Every script takes its values from `argv`. Output fields are separated by
// US (character id 31), records by RS (character id 30).

// Dates go in as "YYYY MM DD HH MM" and `mkdate` builds them (locale-proof,
// unlike date strings); REMINDER_ADD and EVENT_ADD each carry a copy.

const NOTE_CREATE: &str = r#"on run argv
	tell application "Notes"
		set n to make new note with properties {body:item 1 of argv}
		return id of n
	end tell
end run"#;

const NOTE_DELETE: &str = r#"on run argv
	tell application "Notes" to delete note id (item 1 of argv)
end run"#;

const NOTE_FIND: &str = r#"on run argv
	set q to item 1 of argv
	set out to ""
	tell application "Notes"
		set found to (notes whose name contains q)
		if (count of found) is 0 then set found to (notes whose plaintext contains q)
		set k to 0
		repeat with n in found
			set k to k + 1
			if k > 5 then exit repeat
			set t to plaintext of n
			if (length of t) > 400 then set t to text 1 thru 400 of t
			set out to out & (name of n) & (character id 31) & t & (character id 30)
		end repeat
	end tell
	return out
end run"#;

const REMINDER_ADD: &str = concat!(
    r#"on run argv
	tell application "Reminders"
		set L to default list
		if (item 3 of argv) is not "" then
			try
				set L to list (item 3 of argv)
			end try
		end if
		set r to make new reminder at end of reminders of L with properties {name:item 1 of argv}
		if (item 2 of argv) is not "" then set due date of r to my mkdate(item 2 of argv)
		return id of r
	end tell
end run
"#,
    r#"
on mkdate(s)
	set AppleScript's text item delimiters to " "
	set p to text items of s
	set AppleScript's text item delimiters to ""
	set d to current date
	set day of d to 1
	set year of d to (item 1 of p) as integer
	set month of d to (item 2 of p) as integer
	set day of d to (item 3 of p) as integer
	set hours of d to (item 4 of p) as integer
	set minutes of d to (item 5 of p) as integer
	set seconds of d to 0
	return d
end mkdate
"#
);

const REMINDER_DELETE: &str = r#"on run argv
	tell application "Reminders" to delete reminder id (item 1 of argv)
end run"#;

const REMINDERS_LIST: &str = r#"on run argv
	set out to ""
	tell application "Reminders"
		set L to default list
		if (item 1 of argv) is not "" then
			try
				set L to list (item 1 of argv)
			end try
		end if
		set k to 0
		repeat with r in (reminders of L whose completed is false)
			set k to k + 1
			if k > 25 then exit repeat
			set d to due date of r
			if d is missing value then
				set ds to ""
			else
				set ds to d as string
			end if
			set out to out & (name of r) & (character id 31) & ds & (character id 30)
		end repeat
		return (name of L) & (character id 30) & out
	end tell
end run"#;

const EVENT_ADD: &str = concat!(
    r#"on run argv
	tell application "Calendar"
		set c to missing value
		if (item 4 of argv) is not "" then
			try
				set c to first calendar whose name is (item 4 of argv) and writable is true
			end try
		end if
		if c is missing value then set c to first calendar whose writable is true
		set s to my mkdate(item 2 of argv)
		set e to make new event at end of events of c with properties {summary:item 1 of argv, start date:s, end date:s + ((item 3 of argv) as integer) * minutes}
		return (uid of e) & (character id 31) & (name of c)
	end tell
end run
"#,
    r#"
on mkdate(s)
	set AppleScript's text item delimiters to " "
	set p to text items of s
	set AppleScript's text item delimiters to ""
	set d to current date
	set day of d to 1
	set year of d to (item 1 of p) as integer
	set month of d to (item 2 of p) as integer
	set day of d to (item 3 of p) as integer
	set hours of d to (item 4 of p) as integer
	set minutes of d to (item 5 of p) as integer
	set seconds of d to 0
	return d
end mkdate
"#
);

const EVENT_DELETE: &str = r#"on run argv
	tell application "Calendar"
		tell calendar (item 2 of argv) to delete (first event whose uid is (item 1 of argv))
	end tell
end run"#;

/// Events from the start of today for N days, every calendar (repeating
/// events show only on the day they were first made; a known AppleScript limit).
const EVENTS_LIST: &str = r#"on run argv
	set d0 to current date
	set time of d0 to 0
	set d1 to d0 + ((item 1 of argv) as integer) * days
	set out to ""
	tell application "Calendar"
		repeat with c in calendars
			set cn to name of c
			repeat with e in (every event of c whose start date is greater than or equal to d0 and start date is less than d1)
				set out to out & (summary of e) & (character id 31) & ((start date of e) as string) & (character id 31) & cn & (character id 30)
			end repeat
		end repeat
	end tell
	return out
end run"#;

const MUSIC_PLAY: &str = r#"on run argv
	set q to item 1 of argv
	tell application "Music"
		if q is "" then
			play
		else
			set found to (search playlist "Library" for q)
			if found is {} then return "none"
			play item 1 of found
		end if
		delay 0.5
		if player state is playing then return (name of current track) & (character id 31) & (artist of current track)
		return "stopped"
	end tell
end run"#;

const MUSIC_CONTROL: &str = r#"on run argv
	set what to item 1 of argv
	if application "Music" is not running then return "not running"
	tell application "Music"
		if what is "pause" then pause
		if what is "next" then next track
		if what is "previous" then previous track
		delay 0.3
		if player state is playing then return (name of current track) & (character id 31) & (artist of current track)
		return "stopped"
	end tell
end run"#;

const SAFARI_TAB: &str = r#"on run argv
	if application "Safari" is not running then return "not running"
	tell application "Safari"
		if (count of windows) is 0 then return "no window"
		return (name of current tab of front window) & (character id 31) & (URL of current tab of front window)
	end tell
end run"#;

const DARK_MODE: &str = r#"on run argv
	set what to item 1 of argv
	tell application "System Events"
		tell appearance preferences
			if what is "on" then set dark mode to true
			if what is "off" then set dark mode to false
			if what is "toggle" then set dark mode to not dark mode
			return dark mode as string
		end tell
	end tell
end run"#;

const VOLUME: &str = r#"on run argv
	set volume output volume ((item 1 of argv) as integer) without output muted
	return (output volume of (get volume settings)) as string
end run"#;

const MUTE: &str = r#"on run argv
	if (item 1 of argv) is "true" then
		set volume with output muted
	else
		set volume without output muted
	end if
	return (output muted of (get volume settings)) as string
end run"#;

/// Every fixed script, for the syntax check on macOS (`e2e_scripts_compile`).
#[cfg_attr(not(test), allow(dead_code))]
pub const ALL_SCRIPTS: &[&str] = &[
    NOTE_CREATE, NOTE_DELETE, NOTE_FIND, REMINDER_ADD, REMINDER_DELETE, REMINDERS_LIST, EVENT_ADD, EVENT_DELETE, EVENTS_LIST, MUSIC_PLAY,
    MUSIC_CONTROL, SAFARI_TAB, DARK_MODE, VOLUME, MUTE,
];

fn date_arg(d: NaiveDateTime) -> String {
    format!("{} {} {} {} {}", d.year(), d.month(), d.day(), d.hour(), d.minute())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A note's body: the title in bold, then each line as its own paragraph
/// (Notes takes HTML and names the note after its first line).
fn note_html(title: &str, body: &str) -> String {
    let mut h = format!("<div><b>{}</b></div>", html_escape(title.trim()));
    for line in body.lines() {
        let l = line.trim_end();
        if l.is_empty() {
            h.push_str("<div><br></div>");
        } else {
            h.push_str(&format!("<div>{}</div>", html_escape(l)));
        }
    }
    h
}

// ---------------------------------------------------------------- settings

/// System Settings panes BYTE can open, by the words people use.
const PANES: &[(&str, &str, &str)] = &[
    ("wi-fi", "Wi-Fi", "com.apple.wifi-settings-extension"),
    ("wifi", "Wi-Fi", "com.apple.wifi-settings-extension"),
    ("bluetooth", "Bluetooth", "com.apple.BluetoothSettings"),
    ("battery", "Battery", "com.apple.Battery-Settings.extension"),
    ("display", "Displays", "com.apple.Displays-Settings.extension"),
    ("sound", "Sound", "com.apple.Sound-Settings.extension"),
    ("notification", "Notifications", "com.apple.Notifications-Settings.extension"),
    ("focus", "Focus", "com.apple.Focus-Settings.extension"),
    ("automation", "Automation (Privacy)", "com.apple.settings.PrivacySecurity.extension?Privacy_Automation"),
    ("privacy", "Privacy & Security", "com.apple.settings.PrivacySecurity.extension"),
    ("security", "Privacy & Security", "com.apple.settings.PrivacySecurity.extension"),
    ("accessibility", "Accessibility", "com.apple.Accessibility-Settings.extension"),
    ("wallpaper", "Wallpaper", "com.apple.Wallpaper-Settings.extension"),
    ("appearance", "Appearance", "com.apple.Appearance-Settings.extension"),
    ("keyboard", "Keyboard", "com.apple.Keyboard-Settings.extension"),
    ("trackpad", "Trackpad", "com.apple.Trackpad-Settings.extension"),
    ("mouse", "Mouse", "com.apple.Mouse-Settings.extension"),
    ("network", "Network", "com.apple.Network-Settings.extension"),
    ("software update", "Software Update", "com.apple.Software-Update-Settings.extension"),
    ("update", "Software Update", "com.apple.Software-Update-Settings.extension"),
    ("storage", "Storage", "com.apple.settings.Storage"),
    ("login item", "Login Items", "com.apple.LoginItems-Settings.extension"),
    ("printer", "Printers & Scanners", "com.apple.Print-Scan-Settings.extension"),
    ("screen time", "Screen Time", "com.apple.Screen-Time-Settings.extension"),
    ("desktop", "Desktop & Dock", "com.apple.Desktop-Settings.extension"),
    ("dock", "Desktop & Dock", "com.apple.Desktop-Settings.extension"),
    ("password", "Passwords", "com.apple.Passwords-Settings.extension"),
    ("users", "Users & Groups", "com.apple.Users-Groups-Settings.extension"),
    ("date", "Date & Time", "com.apple.Date-Time-Settings.extension"),
    ("time machine", "Time Machine", "com.apple.Time-Machine-Settings.extension"),
];

fn pane_for(text: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    let t = text.to_lowercase();
    // Longest key first, so "software update" beats "update" and "automation" beats "privacy".
    let mut best: Option<&(&str, &str, &str)> = None;
    for p in PANES {
        if t.contains(p.0) && best.is_none_or(|b| p.0.len() > b.0.len()) {
            best = Some(p);
        }
    }
    best
}

fn pane_label(pane: &str) -> &'static str {
    pane_for(pane).map(|p| p.1).unwrap_or("System")
}

fn settings_url(pane: &str) -> String {
    match pane_for(pane) {
        Some(p) => format!("x-apple.systempreferences:{}", p.2),
        None => "x-apple.systempreferences:".into(),
    }
}

// ------------------------------------------------------------------ routing

/// Starts of messages that ask BYTE to do something on the Mac.
fn starts(l: &str, words: &[&str]) -> bool {
    words.iter().any(|w| l.starts_with(w))
}

/// Questions about *how* to do something get an answer, not an action.
fn is_how_to(l: &str) -> bool {
    starts(l, &["how do i", "how can i", "how to", "how would i", "is there a way", "why ", "what is ", "what's the difference", "can you explain", "explain"])
}

/// The message without "please", "hey byte," and a trailing "please"/"?".
fn clean(q: &str) -> String {
    let mut l = q.trim().to_lowercase();
    for p in ["hey byte,", "hey byte", "byte,", "please ", "can you ", "could you ", "would you ", "will you ", "i want you to ", "go ahead and "] {
        if let Some(rest) = l.strip_prefix(p) {
            l = rest.trim_start().to_string();
        }
    }
    l.trim_end_matches(['?', '!', '.']).trim_end_matches(" please").trim_end_matches(" for me").trim().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Note,
    NoteFind,
    Reminder,
    RemindersList,
    Event,
    EventsList,
    Music,
    Shortcut,
}

/// A message's action when rules can tell (no model needed), or the family
/// that needs the model to fill in details.
enum Plan {
    Ready(Action),
    Ask(Family),
}

fn plan(q: &str) -> Option<Plan> {
    let l = clean(q);
    if l.is_empty() || is_how_to(&l) || l.len() > 600 {
        return None;
    }
    use Action::*;
    let ready = |a: Action| Some(Plan::Ready(a));
    // System toggles.
    if l.contains("dark mode") && !l.contains("app") {
        if starts(&l, &["turn on", "enable", "switch on", "switch to dark", "use dark", "go dark", "put on"]) || l.ends_with("dark mode on") {
            return ready(DarkMode(Switch::On));
        }
        if starts(&l, &["turn off", "disable", "switch off", "exit dark", "leave dark", "stop using dark"]) || l.ends_with("dark mode off") || l.contains("light mode") {
            return ready(DarkMode(Switch::Off));
        }
        if starts(&l, &["toggle", "switch dark mode", "flip"]) {
            return ready(DarkMode(Switch::Toggle));
        }
    }
    if starts(&l, &["turn on light mode", "switch to light mode", "use light mode"]) {
        return ready(DarkMode(Switch::Off));
    }
    if starts(&l, &["mute", "silence the"]) && !l.contains("notification") {
        return ready(Mute(true));
    }
    if starts(&l, &["unmute"]) {
        return ready(Mute(false));
    }
    if l.contains("volume") && starts(&l, &["set", "turn", "change", "put", "make", "volume"]) {
        if let Some(n) = number_in(&l) {
            return ready(Volume(n.min(100) as u8));
        }
        if l.contains("max") || l.contains("all the way up") {
            return ready(Volume(100));
        }
    }
    if l.contains("wi-fi") || l.contains("wifi") {
        if starts(&l, &["turn on", "enable", "switch on"]) || l.ends_with(" on") {
            return ready(Wifi(true));
        }
        if starts(&l, &["turn off", "disable", "switch off"]) || l.ends_with(" off") {
            return ready(Wifi(false));
        }
    }
    if starts(&l, &["turn off the display", "turn off my display", "turn off the screen", "turn off my screen", "sleep the display", "put the display to sleep", "put my display to sleep", "put the screen to sleep"]) {
        return ready(SleepDisplay);
    }
    if starts(&l, &["open ", "show ", "take me to ", "go to "]) && (l.contains("settings") || l.contains("preferences")) {
        let pane = pane_for(&l).map(|p| p.0).unwrap_or("").to_string();
        return ready(OpenSettings { pane });
    }
    // Safari.
    if (l.contains("safari") || l.contains("this tab") || l.contains("current tab") || l.contains("this page"))
        && starts(&l, &["what's", "what is", "which", "read", "summarize", "summarise", "what am i", "tell me"])
        && (l.contains("tab") || l.contains("safari"))
    {
        return ready(SafariTab);
    }
    // Music.
    if starts(&l, &["pause the music", "pause music", "stop the music", "stop music", "pause the song", "pause"]) && l.split_whitespace().count() <= 4 {
        return ready(MusicPause);
    }
    if starts(&l, &["next song", "skip this song", "skip song", "skip the song", "next track", "skip this track", "skip"]) && l.split_whitespace().count() <= 4 {
        return ready(MusicNext);
    }
    if starts(&l, &["previous song", "go back a song", "last song", "previous track", "play the previous"]) {
        return ready(MusicPrevious);
    }
    if starts(&l, &["what's playing", "what is playing", "what song is this", "what song is playing", "which song is this"]) {
        return ready(NowPlaying);
    }
    if starts(&l, &["resume the music", "resume music", "play music", "play some music", "play my music", "resume"]) && l.split_whitespace().count() <= 4 {
        return ready(MusicPlay { query: String::new() });
    }
    if l.starts_with("play ") && !l.contains(" game") && !l.contains("video") && !l.contains("youtube") && !l.contains("role") && !l.contains("a quiz") {
        return Some(Plan::Ask(Family::Music));
    }
    // Shortcuts.
    if starts(&l, &["list my shortcuts", "show my shortcuts", "what shortcuts do i have", "which shortcuts do i have"]) {
        return ready(ShortcutsList);
    }
    if l.starts_with("run ") && l.contains("shortcut") {
        return Some(Plan::Ask(Family::Shortcut));
    }
    // Reminders.
    if starts(&l, &["remind me", "add a reminder", "add reminder", "set a reminder", "set reminder", "create a reminder", "make a reminder", "new reminder"])
        || (starts(&l, &["add "]) && (l.contains("to my reminders") || l.contains("to reminders") || l.contains("to my to-do") || l.contains("to my todo")))
    {
        return Some(Plan::Ask(Family::Reminder));
    }
    if starts(&l, &["what are my reminders", "show my reminders", "list my reminders", "what's on my to-do", "what's on my todo", "what reminders do i have", "read my reminders", "my reminders"]) {
        return Some(Plan::Ask(Family::RemindersList));
    }
    // Calendar.
    if (starts(&l, &["add ", "put ", "schedule ", "create an event", "create a calendar event", "make an event", "new event", "book "]) && (l.contains("calendar") || l.starts_with("schedule ") || l.contains("event")))
        && !l.contains("meeting notes")
    {
        return Some(Plan::Ask(Family::Event));
    }
    if (l.contains("calendar") || l.contains("my schedule") || l.contains("my day") || l.contains("my week") || l.contains("meetings") || l.contains("events"))
        && starts(&l, &["what's on", "what is on", "what do i have", "what have i got", "show my", "read my", "what's my", "what is my", "do i have", "am i free", "am i busy", "any meetings", "how busy"])
    {
        return Some(Plan::Ask(Family::EventsList));
    }
    // Notes.
    if starts(&l, &["make a note", "create a note", "add a note", "new note", "take a note", "write a note", "save a note", "note that", "note down", "jot down", "save this to notes", "save that to notes", "put this in notes", "put that in notes", "save this in notes", "save that in notes"])
        || (starts(&l, &["save ", "put ", "add "]) && (l.contains(" in notes") || l.contains(" to notes") || l.contains(" to my notes") || l.contains(" in my notes") || l.contains("notes app")))
    {
        return Some(Plan::Ask(Family::Note));
    }
    if (l.contains("my notes") || l.contains("in notes") || l.contains("notes app")) && starts(&l, &["find", "search", "look for", "look up", "what did i write", "show me", "open"]) {
        return Some(Plan::Ask(Family::NoteFind));
    }
    None
}

/// BYTE should do this on the Mac (module on, macOS).
pub fn applies(enabled: bool, question: &str) -> bool {
    cfg!(target_os = "macos") && enabled && wants(question)
}

/// The message asks for a Mac action (any OS; tests use this).
pub fn wants(question: &str) -> bool {
    plan(question).is_some()
}

fn number_in(l: &str) -> Option<u32> {
    l.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).next()
}

// ------------------------------------------------------------------- times

fn when_text(d: NaiveDateTime) -> String {
    let t = d.time();
    let time = if t.minute() == 0 { d.format("%-I %p").to_string() } else { d.format("%-I:%M %p").to_string() };
    format!("{} at {time}", d.format("%a, %b %-d"))
}

fn minutes_text(m: u32) -> String {
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// A clock time in the text: "3pm", "3:30 pm", "15:00", "noon", "midnight".
fn time_in(l: &str) -> Option<NaiveTime> {
    if l.contains("noon") || l.contains("midday") {
        return NaiveTime::from_hms_opt(12, 0, 0);
    }
    if l.contains("midnight") {
        return NaiveTime::from_hms_opt(23, 59, 0);
    }
    let words: Vec<&str> = l.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let w = w.trim_matches(|c: char| c == ',' || c == '.' || c == '?' || c == '!');
        let next = words.get(i + 1).map(|n| n.trim_matches(|c: char| !c.is_alphabetic()).replace('.', "")).unwrap_or_default();
        let (num, suffix) = match w.find(|c: char| c.is_alphabetic()) {
            Some(k) => (&w[..k], w[k..].replace('.', "")),
            None => (w, String::new()),
        };
        let ampm = if suffix == "am" || suffix == "pm" {
            Some(suffix.clone())
        } else if suffix.is_empty() && (next == "am" || next == "pm") {
            Some(next.clone())
        } else {
            None
        };
        let (h, m) = match num.split_once(':') {
            Some((h, m)) => (h.parse::<u32>().ok(), m.parse::<u32>().ok()),
            None => (num.parse::<u32>().ok(), Some(0)),
        };
        let (Some(mut h), Some(m)) = (h, m) else { continue };
        if !suffix.is_empty() && ampm.is_none() {
            continue;
        }
        let colon = num.contains(':');
        let after_at = i > 0 && matches!(words[i - 1], "at" | "@" | "by" | "from");
        match ampm.as_deref() {
            Some("pm") if h < 12 => h += 12,
            Some("am") if h == 12 => h = 0,
            Some(_) => {}
            // A bare number counts only as "at 3" / "at 15:00".
            None if colon || after_at => {
                // "at 3" means the afternoon for 1–6, like people say it.
                if !colon && (1..=6).contains(&h) {
                    h += 12;
                }
            }
            None => continue,
        }
        if h < 24 && m < 60 {
            return NaiveTime::from_hms_opt(h, m, 0);
        }
    }
    None
}

fn weekday_in(l: &str) -> Option<Weekday> {
    const DAYS: &[(&str, Weekday)] = &[
        ("monday", Weekday::Mon),
        ("tuesday", Weekday::Tue),
        ("wednesday", Weekday::Wed),
        ("thursday", Weekday::Thu),
        ("friday", Weekday::Fri),
        ("saturday", Weekday::Sat),
        ("sunday", Weekday::Sun),
    ];
    DAYS.iter().find(|(n, _)| l.contains(n)).map(|(_, d)| *d)
}

/// A date and time in the message ("tomorrow at 3pm", "friday 9:30am", "in 20
/// minutes", "tonight"), relative to `now`. None when it names no time at all.
pub fn when_in(text: &str, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let l = text.to_lowercase();
    // "in 20 minutes", "in 2 hours", "in an hour"
    if let Some(i) = l.find(" in ").map(|i| i + 4).or_else(|| l.strip_prefix("in ").map(|_| 3)) {
        let rest = &l[i..];
        let mut w = rest.split_whitespace();
        let n = w.next().and_then(|n| match n {
            "a" | "an" | "one" => Some(1),
            "two" => Some(2),
            "five" => Some(5),
            "ten" => Some(10),
            "fifteen" => Some(15),
            "thirty" => Some(30),
            "half" => None,
            n => n.parse::<i64>().ok(),
        });
        let unit = w.next().unwrap_or("");
        if rest.starts_with("half an hour") || rest.starts_with("half hour") {
            return Some(now + Days::minutes(30));
        }
        if let Some(n) = n {
            if unit.starts_with("min") {
                return Some(now + Days::minutes(n));
            }
            if unit.starts_with("hour") || unit.starts_with("hr") {
                return Some(now + Days::hours(n));
            }
            if unit.starts_with("day") {
                return Some((now + Days::days(n)).date().and_time(time_in(&l).unwrap_or(NaiveTime::from_hms_opt(9, 0, 0).unwrap())));
            }
        }
    }
    let today = now.date();
    let mut day: Option<NaiveDate> = None;
    let mut default_time = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    if l.contains("day after tomorrow") {
        day = Some(today + Days::days(2));
    } else if l.contains("tomorrow") {
        day = Some(today + Days::days(1));
    } else if l.contains("tonight") {
        day = Some(today);
        default_time = NaiveTime::from_hms_opt(20, 0, 0).unwrap();
    } else if l.contains("this evening") {
        day = Some(today);
        default_time = NaiveTime::from_hms_opt(18, 0, 0).unwrap();
    } else if l.contains("this afternoon") {
        day = Some(today);
        default_time = NaiveTime::from_hms_opt(15, 0, 0).unwrap();
    } else if l.contains("today") || l.contains("this morning") {
        day = Some(today);
    } else if l.contains("next week") {
        day = Some(today + Days::days(7 - today.weekday().num_days_from_monday() as i64));
    } else if let Some(wd) = weekday_in(&l) {
        // The coming one ("friday" on a Friday means next week's).
        let ahead = (wd.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
        day = Some(today + Days::days(if ahead == 0 { 7 } else { ahead }));
    }
    if l.contains("morning") {
        default_time = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    } else if l.contains("afternoon") {
        default_time = NaiveTime::from_hms_opt(15, 0, 0).unwrap();
    } else if l.contains("evening") {
        default_time = NaiveTime::from_hms_opt(18, 0, 0).unwrap();
    } else if l.contains("night") {
        default_time = NaiveTime::from_hms_opt(20, 0, 0).unwrap();
    }
    let time = time_in(&l);
    match (day, time) {
        (Some(d), t) => Some(d.and_time(t.unwrap_or(default_time))),
        // A time with no day: today, or tomorrow if it has passed.
        (None, Some(t)) => {
            let d = if t > now.time() { today } else { today + Days::days(1) };
            Some(d.and_time(t))
        }
        (None, None) => None,
    }
}

/// "YYYY-MM-DD HH:MM" (what the model is asked for).
fn parse_model_when(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim().replace('T', " ");
    NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M").or_else(|_| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")).ok()
}

/// Length of an event in the message ("for 2 hours", "30 minute", "1.5 hours").
fn minutes_in(l: &str) -> Option<u32> {
    let words: Vec<&str> = l.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let unit = words.get(i + 1).copied().unwrap_or("");
        let n = match *w {
            "an" | "a" | "one" => Some(1.0),
            "half" if unit.starts_with("an") || unit.starts_with("hour") => return Some(30),
            "two" => Some(2.0),
            "three" => Some(3.0),
            w => w.trim_end_matches(['-']).parse::<f32>().ok(),
        };
        if let Some(n) = n {
            if unit.starts_with("hour") || unit.starts_with("hr") || unit.starts_with("h-") {
                return Some((n * 60.0).round() as u32);
            }
            if unit.starts_with("min") {
                return Some(n.round() as u32);
            }
        }
    }
    None
}

// -------------------------------------------------------------- the details

/// Text after the first of `cues` (case-insensitive), without time words.
fn after(q: &str, cues: &[&str]) -> Option<String> {
    let l = q.to_lowercase();
    for c in cues {
        if let Some(i) = l.find(c) {
            let rest = q[i + c.len()..].trim().trim_start_matches(':').trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// Drops "tomorrow at 3pm", "on Friday", "in 20 minutes"… from a title.
fn without_time(s: &str) -> String {
    let mut words: Vec<&str> = s.split_whitespace().collect();
    let timeish = |w: &str| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != ':').to_lowercase();
        matches!(
            w.as_str(),
            "today" | "tomorrow" | "tonight" | "morning" | "afternoon" | "evening" | "noon" | "midnight" | "am" | "pm" | "minutes" | "minute" | "mins"
                | "hours" | "hour" | "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
        ) || w.ends_with("am") && w[..w.len() - 2].chars().all(|c| c.is_ascii_digit() || c == ':') && w.len() > 2
            || w.ends_with("pm") && w[..w.len() - 2].chars().all(|c| c.is_ascii_digit() || c == ':') && w.len() > 2
            || !w.is_empty() && w.chars().all(|c| c.is_ascii_digit() || c == ':')
    };
    let glue = |w: &str| matches!(w.to_lowercase().as_str(), "at" | "on" | "in" | "this" | "next" | "by" | "the" | "day" | "after" | "a" | "an" | "for");
    // Trim time words (and the little words around them) from the end, then the start.
    while let Some(w) = words.last() {
        if timeish(w) || (glue(w) && words.len() > 1) {
            words.pop();
        } else {
            break;
        }
    }
    while let Some(w) = words.first() {
        if timeish(w) || (glue(w) && words.len() > 1 && words.get(1).is_some_and(|n| timeish(n) || glue(n))) {
            words.remove(0);
        } else {
            break;
        }
    }
    let t = words.join(" ");
    t.trim_matches(|c: char| c == ',' || c == '.' || c == ' ').to_string()
}

fn first_upper(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A reminder's title by rules: "remind me to call Mom tomorrow at 3" → "Call Mom".
fn reminder_title(q: &str) -> Option<String> {
    let t = after(q, &["remind me to ", "remind me about ", "remind me that ", "reminder to ", "reminder for ", "reminder: ", "remind me ", "add ", "reminder "])?;
    let t = t.split(" to my reminders").next().unwrap_or(&t).split(" to reminders").next().unwrap_or(&t).to_string();
    let t = without_time(&t);
    let t = t.strip_prefix("to ").unwrap_or(&t);
    (!t.is_empty()).then(|| first_upper(t))
}

/// The model fills in what rules can't. Returns the action, or a message for
/// the user when something essential is missing.
async fn details(turn: &Turn<'_>, family: Family, q: &str, now: NaiveDateTime) -> AppResult<Result<Action, String>> {
    let today = now.format("%A, %B %-d, %Y, %-I:%M %p").to_string();
    let ask = |what: &str, schema: Value| {
        let user = format!("Today is {today}.\nThe user said: \"{q}\"\n\n{what}");
        async move { chat::complete_json(turn.http, turn.ep, "You read requests for a Mac assistant and fill in JSON exactly. Reply only with JSON.", &user, schema, 300).await.ok().map(|r| crate::research::lenient_json(&r)).filter(Value::is_object) }
    };
    let text = |v: &Option<Value>, k: &str| v.as_ref().and_then(|v| v.get(k)).and_then(Value::as_str).unwrap_or("").trim().to_string();
    // A list or calendar name counts only if the user said it next to the word
    // ("my Work calendar", "the Groceries list"): small models invent them.
    let lower = q.to_lowercase();
    let named = |v: &Option<Value>, k: &str, noun: &str| {
        let n = text(v, k);
        let l = n.to_lowercase();
        let said = !l.is_empty() && [format!("{l} {noun}"), format!("{noun} {l}"), format!("\"{l}\" {noun}"), format!("{noun} \"{l}\"")].iter().any(|p| lower.contains(p.as_str()));
        if said { n } else { String::new() }
    };
    Ok(match family {
        Family::Reminder => {
            let v = ask(
                "Fill in the reminder: its short title (what to do, without the time), when it's due as \"YYYY-MM-DD HH:MM\" (\"\" if no time was given), and the list name if they named one (else \"\").",
                json!({"type":"object","properties":{"title":{"type":"string"},"due":{"type":"string"},"list":{"type":"string"}},"required":["title","due","list"]}),
            )
            .await;
            let rule = reminder_title(q);
            let model = text(&v, "title");
            // Prefer the model's title unless it kept the time words or is empty.
            let title = if model.is_empty() || when_in(&model, now).is_some() { rule.unwrap_or_default() } else { first_upper(&without_time(&model)) };
            // Times: the rules win when they find one (small models slip on dates).
            let due = when_in(q, now).or_else(|| parse_model_when(&text(&v, "due")));
            if title.is_empty() {
                return Ok(Err("What should the reminder say?".into()));
            }
            Ok(Action::ReminderAdd { title, due, list: named(&v, "list", "list") })
        }
        Family::RemindersList => Ok(Action::RemindersList { list: after(q, &["in my ", "on my "]).map(|s| s.split(" list").next().unwrap_or("").trim().to_string()).filter(|s| !s.contains("reminder") && !s.contains("to-do") && !s.contains("todo")).unwrap_or_default() }),
        Family::Event => {
            let v = ask(
                "Fill in the calendar event: a short title (without the date or time), when it starts as \"YYYY-MM-DD HH:MM\", how many minutes it lasts (60 if not said), and the calendar name if they named one (else \"\").",
                json!({"type":"object","properties":{"title":{"type":"string"},"start":{"type":"string"},"minutes":{"type":"integer"},"calendar":{"type":"string"}},"required":["title","start","minutes","calendar"]}),
            )
            .await;
            let start = when_in(q, now).or_else(|| parse_model_when(&text(&v, "start")));
            let mut title = first_upper(&without_time(&text(&v, "title")));
            if title.is_empty() {
                title = after(q, &["add ", "put ", "schedule ", "book "]).map(|t| first_upper(&without_time(t.split(" to my calendar").next().unwrap_or(&t).split(" on my calendar").next().unwrap_or(&t)))).unwrap_or_default();
            }
            let minutes = minutes_in(&q.to_lowercase()).or_else(|| v.as_ref().and_then(|v| v["minutes"].as_u64()).map(|m| m as u32)).unwrap_or(60).clamp(5, 24 * 60);
            match (title.is_empty(), start) {
                (true, _) => Err("What's the event called?".into()),
                (_, None) => Err(format!("When is \"{title}\"? Tell me the day and time and I'll add it.")),
                (false, Some(start)) => Ok(Action::EventAdd { title, start, minutes, calendar: named(&v, "calendar", "calendar") }),
            }
        }
        Family::EventsList => {
            let l = q.to_lowercase();
            let days = if l.contains("week") {
                7
            } else if l.contains("tomorrow") {
                2
            } else if l.contains("weekend") {
                (7 - now.weekday().num_days_from_monday()).max(2)
            } else {
                1
            };
            Ok(Action::EventsList { days })
        }
        Family::Note => {
            let v = ask(
                "Fill in the note: a short title (a few words) and the note's text, exactly what they asked to save (keep their words and lists; no extra comments).",
                json!({"type":"object","properties":{"title":{"type":"string"},"body":{"type":"string"}},"required":["title","body"]}),
            )
            .await;
            let rule = after(q, &["note that ", "note down ", "jot down ", "make a note: ", "make a note of ", "make a note ", "create a note: ", "create a note ", "add a note: ", "add a note ", "new note: ", "new note ", "take a note: ", "take a note ", "write a note: ", "write a note ", "save a note: ", "save a note "])
                .map(|s| s.split(" in notes").next().unwrap_or(&s).split(" to notes").next().unwrap_or(&s).trim_start_matches("saying ").trim_start_matches("that ").to_string());
            // A note keeps the user's exact words (small models drop or reword
            // details like codes); the model's text is used only when there's no
            // "note: …" part to take them from.
            let mut body = rule.clone().filter(|r| !r.trim().is_empty()).unwrap_or_else(|| text(&v, "body"));
            // "save this to notes": the last answer is the note.
            let l = q.to_lowercase();
            if (l.contains("this") || l.contains("that") || l.contains("the answer") || l.contains("your answer")) && body.len() < 40 {
                if let Some(prev) = turn.history.iter().rev().find(|m| m.role == "assistant") {
                    body = chat::question_text(&prev.content).to_string();
                }
            }
            let mut title = text(&v, "title");
            if title.is_empty() {
                title = body.lines().next().unwrap_or("").chars().take(60).collect();
            }
            if body.trim().is_empty() {
                return Ok(Err("What should the note say?".into()));
            }
            Ok(Action::NoteCreate { title: first_upper(title.trim()), body })
        }
        Family::NoteFind => {
            let v = ask("What are they looking for in their notes? Give the search words only (one to four words).", json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]})).await;
            let query = text(&v, "query");
            let query = if query.is_empty() { after(q, &["about ", "for ", "called ", "named "]).unwrap_or_default() } else { query };
            if query.is_empty() {
                return Ok(Err("What should I look for in your notes?".into()));
            }
            Ok(Action::NoteFind { query: query.trim_end_matches(['?', '.']).to_string() })
        }
        Family::Music => {
            let rule = after(q, &["play "]).map(|s| s.trim_end_matches(" in music").trim_end_matches(" on music").trim_end_matches(" in apple music").trim_start_matches("some ").trim_start_matches("the song ").trim_start_matches("my ").to_string());
            let v = ask("What should play? Give only the song, artist, album or genre words to search their music library for.", json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]})).await;
            let query = text(&v, "query");
            let query = if query.is_empty() { rule.unwrap_or_default() } else { query };
            Ok(Action::MusicPlay { query: query.trim_end_matches(" music").to_string() })
        }
        Family::Shortcut => {
            let rule = after(q, &["run my ", "run the ", "run "]).map(|s| s.replace(" shortcut", "").replace("shortcut ", "").trim_matches('"').trim().to_string());
            Ok(Action::ShortcutRun { name: rule.unwrap_or_default(), input: String::new() })
        }
    })
}

/// The installed shortcut closest to what was asked (exact, then contains).
fn pick_shortcut(asked: &str, installed: &str) -> Option<String> {
    let a = asked.to_lowercase();
    let names: Vec<&str> = installed.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    names
        .iter()
        .find(|n| n.to_lowercase() == a)
        .or_else(|| names.iter().find(|n| !a.is_empty() && n.to_lowercase().contains(&a)))
        .or_else(|| names.iter().find(|n| !a.is_empty() && a.contains(&n.to_lowercase())))
        .map(|s| s.to_string())
}

/// The Wi-Fi device from `networksetup -listallhardwareports`.
fn wifi_device(ports: &str) -> Option<String> {
    let mut lines = ports.lines();
    while let Some(l) = lines.next() {
        if l.contains("Wi-Fi") || l.contains("AirPort") {
            return lines.next().and_then(|d| d.strip_prefix("Device:")).map(|d| d.trim().to_string());
        }
    }
    None
}

// ------------------------------------------------------------------ running

/// Why a command failed, in words for the user.
#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    /// macOS blocked BYTE from controlling the app (Automation permission).
    NotAllowed,
    /// Not on this Mac (the app or tool is missing).
    Missing,
    /// Took too long.
    Timeout,
    Failed(String),
}

impl RunError {
    fn from_stderr(err: &str) -> RunError {
        if err.contains("-1743") || err.contains("Not authorized to send Apple events") || err.contains("not allowed assistive access") {
            RunError::NotAllowed
        } else if err.contains("-2740") || err.contains("-10814") || err.contains("Unable to find application") {
            RunError::Missing
        } else {
            RunError::Failed(err.lines().last().unwrap_or(err).trim().chars().take(200).collect())
        }
    }

    pub fn text(&self, app: &str) -> String {
        match self {
            RunError::NotAllowed => format!(
                "macOS didn't let BYTE control {app}. To allow it: System Settings → Privacy & Security → Automation → BYTE → turn on {app}. (If BYTE isn't listed, try again and click OK when macOS asks.)"
            ),
            RunError::Missing => format!("{app} isn't available on this Mac."),
            RunError::Timeout => format!("{app} didn't answer in time. If macOS is asking whether BYTE may control it, click OK and try again."),
            RunError::Failed(e) => format!("{app} said: {e}"),
        }
    }
}

/// Runs commands (the Mac; tests use a fake).
pub trait Runner: Send + Sync {
    fn run<'a>(&'a self, cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>>;
}

/// The real runner: `osascript` and system tools, no shell.
pub struct MacRunner;

impl Runner for MacRunner {
    fn run<'a>(&'a self, cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>> {
        Box::pin(async move {
            let mut c = match cmd {
                Command::Osa { script, args } => {
                    let mut c = tokio::process::Command::new("/usr/bin/osascript");
                    c.arg("-e").arg(script).args(args);
                    c
                }
                Command::Exec { program, args } => {
                    let path = match *program {
                        "open" => "/usr/bin/open",
                        "pmset" => "/usr/bin/pmset",
                        "networksetup" => "/usr/sbin/networksetup",
                        "shortcuts" => "/usr/bin/shortcuts",
                        other => other,
                    };
                    let mut c = tokio::process::Command::new(path);
                    c.args(args);
                    c
                }
            };
            c.kill_on_drop(true).stdin(std::process::Stdio::null());
            let out = match tokio::time::timeout(RUN_WAIT, c.output()).await {
                Err(_) => return Err(RunError::Timeout),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(RunError::Missing),
                Ok(Err(e)) => return Err(RunError::Failed(e.to_string())),
                Ok(Ok(o)) => o,
            };
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
            } else {
                Err(RunError::from_stderr(&String::from_utf8_lossy(&out.stderr)))
            }
        })
    }
}

/// Runs an action: Wi-Fi needs its device first, shortcuts must exist.
async fn execute(runner: &dyn Runner, action: &Action) -> Result<String, RunError> {
    match action {
        Action::Wifi(on) => {
            let ports = runner.run(&action.command()).await?;
            let dev = wifi_device(&ports).ok_or(RunError::Missing)?;
            runner.run(&Command::Exec { program: "networksetup", args: vec!["-setairportpower".into(), dev, if *on { "on" } else { "off" }.into()] }).await
        }
        _ => runner.run(&action.command()).await,
    }
}

// --------------------------------------------------------------------- undo

/// Undo commands for what BYTE added, by token (this session only).
static UNDO: Lazy<Mutex<HashMap<String, Command>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// How to take back what `action` added, from the script's output.
fn undo_for(action: &Action, out: &str) -> Option<Command> {
    let out = out.trim();
    if out.is_empty() {
        return None;
    }
    match action {
        Action::NoteCreate { .. } => Some(Command::Osa { script: NOTE_DELETE, args: vec![out.into()] }),
        Action::ReminderAdd { .. } => Some(Command::Osa { script: REMINDER_DELETE, args: vec![out.into()] }),
        Action::EventAdd { .. } => {
            let (uid, cal) = out.split_once(US)?;
            Some(Command::Osa { script: EVENT_DELETE, args: vec![uid.into(), cal.into()] })
        }
        _ => None,
    }
}

/// Takes back something BYTE added (the card's Undo). False when it can't any
/// more (BYTE restarted, or it was already undone).
pub async fn undo(token: &str) -> AppResult<bool> {
    let cmd = UNDO.lock().ok().and_then(|mut m| m.remove(token));
    let Some(cmd) = cmd else { return Ok(false) };
    MacRunner.run(&cmd).await.map(|_| true).map_err(|e| AppError::msg(e.text("the app")))
}

// -------------------------------------------------------------------- cards

/// The result card: what BYTE did in which app, with Undo when it added something.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MacDone {
    pub app: String,
    pub title: String,
    /// Short details (the reminder's time, the song playing…).
    pub detail: String,
    pub ok: bool,
    /// Token for `mac_undo` (None: nothing to take back).
    pub undo: Option<String>,
}

/// What the script printed, as short text for the card and notes for the model.
fn read_out(action: &Action, out: &str) -> (String, String) {
    use Action::*;
    fn recs(s: &str) -> Vec<Vec<&str>> {
        s.split(RS).map(|r| r.split(US).map(str::trim).collect::<Vec<_>>()).filter(|r| !r.iter().all(|f| f.is_empty())).collect()
    }
    match action {
        NoteCreate { title, .. } => (format!("\"{title}\""), format!("Done: BYTE added the note \"{title}\" to the Notes app.")),
        ReminderAdd { title, due, list } => {
            let when = due.map(when_text).map(|w| format!(", due {w}")).unwrap_or_default();
            let list = if list.is_empty() { String::new() } else { format!(" in the list \"{list}\"") };
            (format!("{title}{when}"), format!("Done: BYTE added the reminder \"{title}\"{when}{list} to Reminders."))
        }
        EventAdd { title, start, minutes, .. } => {
            let cal = out.split_once(US).map(|(_, c)| c.trim().to_string()).unwrap_or_default();
            let on = if cal.is_empty() { String::new() } else { format!(" (calendar \"{cal}\")") };
            (format!("{} · {}", when_text(*start), minutes_text(*minutes)), format!("Done: BYTE added \"{title}\" on {} for {}{on} to Calendar.", when_text(*start), minutes_text(*minutes)))
        }
        NoteFind { query } => {
            let r = recs(out);
            if r.is_empty() {
                return ("No matching notes".into(), format!("BYTE searched the user's Notes for \"{query}\" and found nothing."));
            }
            let list: Vec<String> = r.iter().map(|f| format!("- **{}**: {}", f.first().unwrap_or(&""), f.get(1).unwrap_or(&"").replace('\n', " "))).collect();
            (format!("{} notes found", r.len()), format!("The user's notes that match \"{query}\" (from the Notes app):\n{}", list.join("\n")))
        }
        RemindersList { .. } => {
            let mut parts = out.splitn(2, RS);
            let list_name = parts.next().unwrap_or("").trim().to_string();
            let r = recs(parts.next().unwrap_or(""));
            if r.is_empty() {
                return (format!("Nothing open in {list_name}"), format!("The user's Reminders list \"{list_name}\" has no open reminders."));
            }
            let items: Vec<String> = r.iter().map(|f| match f.get(1).filter(|d| !d.is_empty()) {
                Some(d) => format!("- {} (due {d})", f[0]),
                None => format!("- {}", f[0]),
            }).collect();
            (format!("{} open in {list_name}", r.len()), format!("Open reminders in the user's \"{list_name}\" list:\n{}", items.join("\n")))
        }
        EventsList { days } => {
            let mut r = recs(out);
            let span = if *days <= 1 { "today".to_string() } else { format!("in the next {days} days") };
            if r.is_empty() {
                return (format!("Nothing {span}"), format!("The user's calendar has no events {span} (repeating events may not show)."));
            }
            r.sort_by(|a, b| a.get(1).cmp(&b.get(1)));
            let items: Vec<String> = r.iter().take(40).map(|f| format!("- {} — {} ({})", f.get(1).unwrap_or(&""), f[0], f.get(2).unwrap_or(&""))).collect();
            (format!("{} events {span}", r.len()), format!("The user's calendar {span} (from the Calendar app; events that repeat may be missing):\n{}", items.join("\n")))
        }
        MusicPlay { query } => match out {
            "none" => ("Not in your library".into(), format!("BYTE searched the Music library for \"{query}\" and found nothing to play.")),
            "stopped" => ("Nothing playing".into(), "Music didn't start playing (the library may be empty).".into()),
            o => {
                let (song, artist) = o.split_once(US).unwrap_or((o, ""));
                (format!("{song} — {artist}"), format!("Done: now playing \"{song}\" by {artist} in Music."))
            }
        },
        MusicPause | MusicNext | MusicPrevious | NowPlaying => match out {
            "not running" => ("Music isn't open".into(), "The Music app isn't open, so nothing is playing.".into()),
            "stopped" => ("Nothing playing".into(), if matches!(action, MusicPause) { "Done: the music is paused.".into() } else { "Nothing is playing in Music.".into() }),
            o => {
                let (song, artist) = o.split_once(US).unwrap_or((o, ""));
                (format!("{song} — {artist}"), format!("Now playing \"{song}\" by {artist} in Music."))
            }
        },
        SafariTab => match out {
            "not running" | "no window" => ("Safari isn't open".into(), "Safari has no open window, so there's no current tab.".into()),
            o => {
                let (title, url) = o.split_once(US).unwrap_or((o, ""));
                (title.to_string(), format!("Safari's current tab: \"{title}\" — {url}"))
            }
        },
        DarkMode(_) => {
            let on = out.trim() == "true";
            (if on { "Dark mode is on" } else { "Dark mode is off" }.into(), format!("Done: dark mode is now {}.", if on { "on" } else { "off" }))
        }
        Volume(v) => (format!("Volume {}%", out.trim().parse::<u8>().unwrap_or(*v)), format!("Done: the volume is now {}%.", out.trim().parse::<u8>().unwrap_or(*v))),
        Mute(m) => (if *m { "Muted" } else { "Sound on" }.into(), format!("Done: the sound is {}.", if *m { "muted" } else { "on" })),
        Wifi(on) => (if *on { "Wi-Fi on" } else { "Wi-Fi off" }.into(), format!("Done: Wi-Fi is {}.", if *on { "on" } else { "off" })),
        SleepDisplay => ("Display off".into(), "Done: the display is off.".into()),
        OpenSettings { pane } => (format!("{} settings", pane_label(pane)), format!("Done: System Settings is open at {}.", pane_label(pane))),
        ShortcutsList => {
            let names: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            if names.is_empty() {
                return ("No shortcuts".into(), "The user has no shortcuts in the Shortcuts app.".into());
            }
            (format!("{} shortcuts", names.len()), format!("The user's shortcuts (Shortcuts app):\n{}", names.iter().take(80).map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n")))
        }
        ShortcutRun { name, .. } => {
            let o: String = out.chars().take(2000).collect();
            ("Ran it".into(), if o.trim().is_empty() { format!("Done: the shortcut \"{name}\" ran.") } else { format!("Done: the shortcut \"{name}\" ran. It returned:\n{o}") })
        }
    }
}

const TELL: &str = "Tell the user what happened in one or two short sentences, in plain words. Don't show code or commands. \
Don't claim anything that isn't in these notes.";

// ---------------------------------------------------------------------- run

/// Does what the message asks on the Mac. Returns notes for the written answer.
pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    run_with(turn, question, chrono::Local::now().naive_local(), &MacRunner, cancel, send).await
}

pub(crate) async fn run_with(turn: &Turn<'_>, question: &str, now: NaiveDateTime, runner: &dyn Runner, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(p) = plan(question) else { return Ok(None) };
    let mut action = match p {
        Plan::Ready(a) => a,
        Plan::Ask(f) => {
            let got = tokio::select! {
                r = details(turn, f, question, now) => r?,
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
            };
            match got {
                Ok(a) => a,
                Err(ask) => return Ok(Some((SourceBook::default(), format!("BYTE needs one more detail before doing this on the Mac. Ask the user exactly this, briefly: {ask}")))),
            }
        }
    };
    // A shortcut must exist: pick the installed one closest to what was asked.
    if let Action::ShortcutRun { name, input } = &action {
        let installed = runner.run(&Action::ShortcutsList.command()).await.unwrap_or_default();
        match pick_shortcut(name, &installed) {
            Some(real) => action = Action::ShortcutRun { name: real, input: input.clone() },
            None => {
                let some: Vec<&str> = installed.lines().filter(|l| !l.trim().is_empty()).take(12).collect();
                return Ok(Some((SourceBook::default(), format!("There's no shortcut called \"{name}\" on this Mac. Their shortcuts: {}. Ask which one they meant.", if some.is_empty() { "none".into() } else { some.join(", ") }))));
            }
        }
    }
    let app = action.app();
    let id = format!("byte_mac_{}", uuid::Uuid::new_v4().simple());
    let args = json!({ "app": app, "what": action.describe() });
    send(ChatEvent::ToolCall { id: id.clone(), name: action.tool().into(), args: args.clone() })?;

    if action.needs_ok() {
        let ask = crate::web_agent::ApprovalAsk {
            id: format!("mac_{}", uuid::Uuid::new_v4().simple()),
            action: "mac".into(),
            title: action.describe(),
            site: app.into(),
            url: String::new(),
            target: app.into(),
            fields: action.fields().into_iter().map(|(label, value)| crate::web_agent::Field { label, value }).collect(),
        };
        let ok = crate::web_agent::ask(ask, APPROVAL_WAIT, cancel, send).await?;
        if !ok {
            send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
            turn.log.record(action.tool(), &args, false, "declined");
            return Ok(Some((SourceBook::default(), format!("The user chose not to let BYTE {}. Nothing was changed. Say so in one sentence.", lower_first(&action.describe())))));
        }
    }

    let out = tokio::select! {
        r = execute(runner, &action) => r,
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
    };
    match out {
        Ok(out) => {
            let (detail, notes) = read_out(&action, &out);
            let undo = undo_for(&action, &out).map(|cmd| {
                let token = uuid::Uuid::new_v4().simple().to_string();
                if let Ok(mut m) = UNDO.lock() {
                    m.insert(token.clone(), cmd);
                }
                token
            });
            send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
            turn.log.record(action.tool(), &args, true, &detail);
            send(ChatEvent::MacDone(MacDone { app: app.into(), title: action.describe(), detail, ok: true, undo }))?;
            Ok(Some((SourceBook::default(), format!("{notes}\n\n{TELL}"))))
        }
        Err(e) => {
            let msg = e.text(app);
            send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
            turn.log.record(action.tool(), &args, false, &msg);
            send(ChatEvent::MacDone(MacDone { app: app.into(), title: action.describe(), detail: msg.clone(), ok: false, undo: None }))?;
            Ok(Some((SourceBook::default(), format!("BYTE tried to {} but it didn't work: {msg}\n\nExplain this to the user briefly, with the steps to fix it if there are any.", lower_first(&action.describe())))))
        }
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[tauri::command]
pub async fn mac_undo(token: String) -> AppResult<bool> {
    undo(&token).await
}

#[cfg(test)]
#[path = "macctl_tests.rs"]
mod tests;
