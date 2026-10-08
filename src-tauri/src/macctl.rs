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
use crate::pcctl::WinOp;
use crate::tools::SourceBook;

/// How long an app may take (the first time, macOS waits for the user's Allow).
const RUN_WAIT: Duration = Duration::from_secs(90);
/// How long an approval card waits.
const APPROVAL_WAIT: Duration = Duration::from_secs(600);
/// Field separators in script output (ASCII unit / record separators).
pub(crate) const US: char = '\u{1f}';
pub(crate) const RS: char = '\u{1e}';

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
    /// Recent emails: unread (empty query) or from/about someone, within `days`.
    MailList { query: String, days: u32 },
    /// A new email opened in Mail, filled in, for the user to send (BYTE never sends).
    MailDraft { to: String, name: String, subject: String, body: String },
    /// A text sent through Messages after the user approves it (and may edit it on the card). `chat` is the
    /// conversation's id from the Messages inbox when known (replies go to the same thread), else empty.
    MessageSend { to: String, name: String, body: String, chat: String },
    /// People in Contacts (for finding an address; never shown as its own action).
    ContactFind { name: String },
}

/// A fixed program run with the user's words as separate arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `osascript -e <script> <args…>`; the script reads them with `on run argv`.
    Osa { script: &'static str, args: Vec<String> },
    /// A system tool (`pmset`, `open`, `networksetup`, `shortcuts`), no shell.
    Exec { program: &'static str, args: Vec<String> },
    /// A PC operation (see pcctl.rs); the Mac runner doesn't know it.
    Win(WinOp),
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
            MailList { .. } | MailDraft { .. } => "Mail",
            MessageSend { .. } => "Messages",
            ContactFind { .. } => "Contacts",
        }
    }

    /// `app`, as the card names it on a PC (`pc`) or a Mac.
    pub fn app_for(&self, pc: bool) -> &'static str {
        if !pc {
            return self.app();
        }
        use Action::*;
        match self {
            NoteCreate { .. } | NoteFind { .. } => "BYTE Notes",
            ReminderAdd { .. } | RemindersList { .. } => "BYTE Tasks",
            EventAdd { .. } | EventsList { .. } => "Calendar",
            MusicPlay { .. } | MusicPause | MusicNext | MusicPrevious | NowPlaying => "Media",
            SafariTab => "Browser",
            DarkMode(_) | Wifi(_) | OpenSettings { .. } => "Settings",
            Volume(_) | Mute(_) => "Sound",
            SleepDisplay => "Display",
            ShortcutsList | ShortcutRun { .. } => "Shortcuts",
            MailList { .. } | MailDraft { .. } => "Mail",
            MessageSend { .. } => "Messages",
            ContactFind { .. } => "Contacts",
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
            MailList { .. } => "mac_mail_list",
            MailDraft { .. } => "mac_mail_draft",
            MessageSend { .. } => "mac_message_send",
            ContactFind { .. } => "mac_contact_find",
        }
    }

    /// Adds or changes something that stays: the user approves it first.
    pub fn needs_ok(&self) -> bool {
        matches!(self, Action::NoteCreate { .. } | Action::ReminderAdd { .. } | Action::EventAdd { .. } | Action::Wifi(false) | Action::ShortcutRun { .. } | Action::MailDraft { .. } | Action::MessageSend { .. })
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
            MailList { query, .. } if query.is_empty() => "Check your new emails".into(),
            MailList { query, .. } => format!("Look for emails about \"{query}\""),
            MailDraft { name, to, .. } => format!("Open an email to {} in Mail, ready to send", if name.is_empty() { to } else { name }),
            MessageSend { name, to, .. } => format!("Send a text to {}", if name.is_empty() { to } else { name }),
            ContactFind { name } => format!("Look up {name} in Contacts"),
        }
    }

    /// `describe`, worded for a PC (`pc`) or a Mac.
    pub fn describe_for(&self, pc: bool) -> String {
        if !pc {
            return self.describe();
        }
        use Action::*;
        match self {
            NoteCreate { title, .. } => format!("Save a note \"{title}\" in BYTE Notes"),
            NoteFind { query } => format!("Look for notes about \"{query}\" in BYTE Notes"),
            EventAdd { title, .. } => format!("Open \"{title}\" in your calendar app"),
            MusicPlay { query } if !query.is_empty() => format!("Look up \"{query}\" to play"),
            OpenSettings { pane } => format!("Open {} settings", crate::pcctl::pane_label(pane)),
            MailDraft { name, to, .. } => format!("Open an email to {} in your mail app, ready to send", if name.is_empty() { to } else { name }),
            _ => self.describe(),
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
            MailDraft { to, name, subject, body } => vec![
                ("To".into(), if name.is_empty() || name == to { to.clone() } else { format!("{name} <{to}>") }),
                ("Subject".into(), subject.clone()),
                ("Email".into(), body.chars().take(1500).collect()),
                ("Note".into(), "Mail opens it for you to read and send. BYTE doesn't send it.".into()),
            ],
            MessageSend { to, name, body, .. } => vec![
                ("To".into(), if name.is_empty() || name == to { to.clone() } else { format!("{name} ({to})") }),
                ("Text".into(), body.clone()),
                ("Note".into(), "Sent from your Messages app when you press Send. A sent text can't be taken back from BYTE.".into()),
            ],
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
            MailList { query, days } => osa(MAIL_LIST, vec![query.clone(), days.to_string()]),
            MailDraft { to, subject, body, .. } => osa(MAIL_DRAFT, vec![to.clone(), subject.clone(), body.clone()]),
            MessageSend { to, body, chat, .. } => osa(MESSAGE_SEND, vec![body.clone(), handle_of(to), chat.clone()]),
            ContactFind { name } => osa(CONTACT_FIND, vec![name.clone()]),
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

/// Recent inbox messages (all accounts): unread ones, or ones whose sender or
/// subject contains the query; at most 15.
const MAIL_LIST: &str = r#"on run argv
	set q to item 1 of argv
	set cutoff to (current date) - ((item 2 of argv) as integer) * days
	set out to ""
	tell application "Mail"
		if q is "" then
			set msgs to (messages of inbox whose read status is false and date received > cutoff)
		else
			set msgs to (messages of inbox whose date received > cutoff and (sender contains q or subject contains q))
		end if
		set k to 0
		repeat with m in msgs
			set k to k + 1
			if k > 15 then exit repeat
			set c to content of m
			if (length of c) > 400 then set c to text 1 thru 400 of c
			set out to out & (sender of m) & (character id 31) & (subject of m) & (character id 31) & ((date received of m) as string) & (character id 31) & c & (character id 30)
		end repeat
	end tell
	return out
end run"#;

/// A new message window in Mail, filled in; the user reads it and sends it.
const MAIL_DRAFT: &str = r#"on run argv
	tell application "Mail"
		set m to make new outgoing message with properties {subject:item 2 of argv, content:item 3 of argv, visible:true}
		tell m to make new to recipient at end of to recipients with properties {address:item 1 of argv}
		activate
		return id of m
	end tell
end run"#;

const MAIL_DRAFT_DELETE: &str = r#"on run argv
	tell application "Mail" to delete (first outgoing message whose id is ((item 1 of argv) as integer))
end run"#;

/// Messages opened to the person with the text filled in (and on the clipboard,
/// in case this macOS version leaves the box empty). Nothing is sent.
const MESSAGE_DRAFT: &str = r#"on run argv
	set the clipboard to (item 1 of argv)
	open location (item 2 of argv)
	return "opened"
end run"#;

/// Sends a text: into the known conversation (`chat id`, from the Messages inbox), else to the person over
/// iMessage, else over SMS (needs an iPhone with Text Message Forwarding). The text and address are argv.
const MESSAGE_SEND: &str = r#"on run argv
	set theText to item 1 of argv
	set target to item 2 of argv
	set chatId to item 3 of argv
	tell application "Messages"
		if chatId is not "" then
			send theText to chat id chatId
			return "sent"
		end if
		try
			set svc to 1st account whose service type = iMessage
			send theText to participant target of svc
			return "sent"
		on error
			set svc to 1st account whose service type = SMS
			send theText to participant target of svc
			return "sent sms"
		end try
	end tell
end run"#;

/// People whose name or nickname contains the words: name, emails, phones.
const CONTACT_FIND: &str = r#"on run argv
	set q to item 1 of argv
	set out to ""
	tell application "Contacts"
		set ps to (people whose name contains q or nickname contains q)
		set k to 0
		repeat with p in ps
			set k to k + 1
			if k > 6 then exit repeat
			set AppleScript's text item delimiters to ","
			set es to (value of emails of p) as text
			set ph to (value of phones of p) as text
			set AppleScript's text item delimiters to ""
			set out to out & (name of p) & (character id 31) & es & (character id 31) & ph & (character id 30)
		end repeat
	end tell
	return out
end run"#;

/// Every fixed script, for the syntax check on macOS (`e2e_scripts_compile`).
#[cfg_attr(not(test), allow(dead_code))]
pub const ALL_SCRIPTS: &[&str] = &[
    NOTE_CREATE, NOTE_DELETE, NOTE_FIND, REMINDER_ADD, REMINDER_DELETE, REMINDERS_LIST, EVENT_ADD, EVENT_DELETE, EVENTS_LIST, MUSIC_PLAY,
    MUSIC_CONTROL, SAFARI_TAB, DARK_MODE, VOLUME, MUTE, MAIL_LIST, MAIL_DRAFT, MAIL_DRAFT_DELETE, MESSAGE_DRAFT, MESSAGE_SEND, CONTACT_FIND,
];

/// Percent-encodes for a URL part (spaces as %20, which Messages shows as spaces).
fn url_part(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The address Messages knows: a phone number keeps only digits and "+"; an email its own characters.
pub(crate) fn handle_of(to: &str) -> String {
    if to.contains('@') {
        to.chars().filter(|c| c.is_ascii_alphanumeric() || "+@.-_".contains(*c)).collect()
    } else {
        to.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect()
    }
}

/// `sms:` link that opens Messages to a number or email with the text filled in (the fallback when sending
/// directly doesn't work).
fn sms_url(to: &str, body: &str) -> String {
    format!("sms:{}&body={}", url_part(&handle_of(to)), url_part(body))
}

/// Drops template leftovers small models add ("[Your Name]", "[Date]"): a line
/// that is only a placeholder goes, and a placeholder inside a line is removed.
fn without_placeholders(body: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        let mut l = line.to_string();
        while let (Some(a), Some(b)) = (l.find('['), l.find(']')) {
            if b <= a || b - a > 40 {
                break;
            }
            l.replace_range(a..=b, "");
        }
        let t = l.trim_end().to_string();
        if t.trim().is_empty() && !line.trim().is_empty() {
            continue;
        }
        out.push(t);
    }
    out.join("\n").trim().to_string()
}

/// One person from Contacts.
#[derive(Debug, Clone, PartialEq)]
struct Person {
    name: String,
    emails: Vec<String>,
    phones: Vec<String>,
}

fn people(out: &str) -> Vec<Person> {
    let split = |s: &str| s.split(',').map(str::trim).filter(|x| !x.is_empty() && *x != "missing value").map(String::from).collect::<Vec<_>>();
    out.split(RS)
        .filter_map(|r| {
            let f: Vec<&str> = r.split(US).collect();
            let name = f.first()?.trim();
            (!name.is_empty()).then(|| Person { name: name.to_string(), emails: split(f.get(1).unwrap_or(&"")), phones: split(f.get(2).unwrap_or(&"")) })
        })
        .collect()
}

/// "Sam Lee <sam@example.com>" → ("Sam Lee", "sam@example.com").
fn sender_parts(s: &str) -> (String, String) {
    match (s.find('<'), s.rfind('>')) {
        (Some(a), Some(b)) if b > a => (s[..a].trim().trim_matches('"').to_string(), s[a + 1..b].trim().to_string()),
        _ => (String::new(), s.trim().to_string()),
    }
}

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

pub(crate) fn pane_label(pane: &str) -> &'static str {
    pane_for(pane).map(|p| p.1).unwrap_or("System")
}

pub(crate) fn settings_url(pane: &str) -> String {
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
    MailList,
    MailDraft,
    MailReply,
    Message,
}

/// A message's action when rules can tell (no model needed), or the family
/// that needs the model to fill in details.
enum Plan {
    Ready(Action),
    Ask(Family),
}

fn plan(q: &str) -> Option<Plan> {
    plan_on(q, cfg!(windows))
}

/// `plan`, for a Mac (`pc` false) or a PC. Only the names of Settings pages differ.
fn plan_on(q: &str, pc: bool) -> Option<Plan> {
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
        let pane = if pc { crate::pcctl::pane_for(&l).unwrap_or("") } else { pane_for(&l).map(|p| p.0).unwrap_or("") }.to_string();
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
    // Mail and Messages: only clear requests to use the apps ("write an email to…"
    // stays an ordinary answer, written in the chat).
    if starts(&l, &["any new email", "any new mail", "do i have any email", "do i have any new email", "do i have new email", "do i have any mail", "check my email", "check my mail", "check my inbox", "summarize my inbox", "summarise my inbox", "summarize my email", "summarise my email", "what's in my inbox", "what is in my inbox", "show my email", "read my email", "any emails from", "did i get an email", "did i get any email", "emails from ", "email from "])
        || (l.starts_with("did ") && (l.contains(" email me") || l.contains(" mail me")))
    {
        return Some(Plan::Ask(Family::MailList));
    }
    if starts(&l, &["reply to ", "write back to ", "respond to "]) && (l.contains("email") || l.contains("mail")) {
        return Some(Plan::Ask(Family::MailReply));
    }
    if (starts(&l, &["email ", "e-mail ", "send an email to ", "send email to ", "send a mail to ", "draft an email to ", "draft an email for ", "compose an email to ", "start an email to "])
        || (starts(&l, &["write an email to ", "write a email to "]) && (l.contains(" in mail") || l.contains("mail app"))))
        && !starts(&l, &["email me", "email address", "email marketing", "email etiquette"])
    {
        return Some(Plan::Ask(Family::MailDraft));
    }
    if starts(&l, &["text ", "send a text to ", "send a message to ", "message ", "imessage ", "send an imessage to "])
        && (SEPARATORS.iter().any(|c| l.contains(c)) || after_first(q, TEXT_CUES).is_some_and(|r| plain_text(&r).is_some()))
        && !starts(&l, &["text me", "message me", "text summar", "text to speech", "message queue"])
    {
        return Some(Plan::Ask(Family::Message));
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

/// BYTE should do this on this computer (module on; macOS or Windows).
pub fn applies(enabled: bool, question: &str) -> bool {
    (cfg!(target_os = "macos") || cfg!(windows)) && enabled && wants(question)
}

/// The message asks for a Mac action (any OS; tests use this).
pub fn wants(question: &str) -> bool {
    plan(question).is_some()
}

fn number_in(l: &str) -> Option<u32> {
    l.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).next()
}

// ------------------------------------------------------------------- times

pub(crate) fn when_text(d: NaiveDateTime) -> String {
    let t = d.time();
    let time = if t.minute() == 0 { d.format("%-I %p").to_string() } else { d.format("%-I:%M %p").to_string() };
    format!("{} at {time}", d.format("%a, %b %-d"))
}

pub(crate) fn minutes_text(m: u32) -> String {
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

/// Like `after`, but uses the cue that appears first in the message ("text Mom the message is ready" is cut
/// after "text ", not after "message ").
fn after_first(q: &str, cues: &[&str]) -> Option<String> {
    let l = q.to_lowercase();
    let (i, c) = cues.iter().filter_map(|c| l.find(c).map(|i| (i, *c))).min_by_key(|(i, _)| *i)?;
    let rest = q[i + c.len()..].trim().trim_start_matches(':').trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

const TEXT_CUES: &[&str] = &["send an imessage to ", "send a text to ", "send a message to ", "imessage ", "message ", "text "];
const SEPARATORS: &[&str] = &[" that ", " saying ", ":", " to say ", " and say ", " and tell them ", " and tell her ", " and tell him ", " and tell "];
/// Words for people that are names in Contacts' relations, used without "that" / "saying".
const PEOPLE: &[&str] = &[
    "mom", "mum", "mommy", "mama", "mother", "dad", "daddy", "papa", "father", "grandma", "grandpa", "granny", "nana",
    "wife", "husband", "sister", "brother", "sis", "bro", "son", "daughter", "boyfriend", "girlfriend", "partner",
    "aunt", "uncle", "cousin", "boss",
];

/// "text Mom this is BYTE": a text written without "that" / "saying" / ":". The person is a family word, a
/// capitalized name, a phone number or an email; everything after it is the message. Nothing else counts,
/// so "text summarization models" stays an ordinary question.
fn plain_text(rest: &str) -> Option<(String, String)> {
    let mut words: Vec<&str> = rest.split_whitespace().collect();
    if words.first().is_some_and(|w| w.eq_ignore_ascii_case("my")) {
        words.remove(0);
    }
    let first = *words.first()?;
    // A phone number may be written in parts: "(412) 555-0123".
    let phone_len = words.iter().take_while(|w| w.chars().all(|c| c.is_ascii_digit() || "()+-.".contains(c))).count();
    let digits: usize = words[..phone_len].iter().map(|w| w.chars().filter(|c| c.is_ascii_digit()).count()).sum();
    let n = if phone_len > 0 && digits >= 7 {
        phone_len
    } else if first.contains('@')
        || PEOPLE.contains(&first.to_lowercase().as_str())
        || (first.chars().next().is_some_and(char::is_uppercase) && first.chars().all(|c| c.is_alphabetic() || c == '-' || c == '\'') && first != "I")
    {
        1
    } else {
        return None;
    };
    let body = words[n..].join(" ");
    (!body.trim().is_empty()).then(|| (words[..n].join(" "), body))
}

/// What the user wants to say, from "reply to Sam's email saying I can make it" → "I can make it".
fn user_words(q: &str) -> String {
    let l = q.to_lowercase();
    [" saying ", " that ", " to say ", " and say ", " and tell them ", " telling them "]
        .iter()
        .filter_map(|c| l.find(c).map(|i| i + c.len()))
        .min()
        .map(|i| q[i..].trim().trim_end_matches(['.', '!']).to_string())
        .unwrap_or_default()
}

/// A reply answers the email instead of copying it (small models sometimes repeat it word for word): with the
/// greeting and sign-off lines left out, at most half of the reply's 4-word runs may appear in their email.
fn reply_answers(body: &str, original: &str) -> bool {
    let words = |s: &str| s.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).map(String::from).collect::<Vec<_>>();
    let lines: Vec<&str> = body.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let core = if lines.len() > 2 { lines[1..lines.len() - 1].join(" ") } else { lines.join(" ") };
    let (mine, theirs) = (words(&core), words(original).join(" "));
    if mine.is_empty() {
        return false;
    }
    if theirs.is_empty() || mine.len() < 4 {
        return true;
    }
    let runs: Vec<String> = mine.windows(4).map(|w| w.join(" ")).collect();
    let copied = runs.iter().filter(|r| theirs.contains(r.as_str())).count();
    copied * 2 <= runs.len()
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
async fn details(turn: &Turn<'_>, family: Family, q: &str, now: NaiveDateTime, runner: &dyn Runner) -> AppResult<Result<Action, String>> {
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
    // A PC can't do these (see pcctl::unsupported); they come back as the action, and the answer explains.
    if runner.pc() {
        match family {
            Family::MailReply => return Ok(Ok(Action::MailList { query: String::new(), days: 1 })),
            Family::Message => return Ok(Ok(Action::MessageSend { to: String::new(), name: String::new(), body: String::new(), chat: String::new() })),
            Family::Shortcut => return Ok(Ok(Action::ShortcutsList)),
            Family::Reminder => return Ok(Ok(Action::ReminderAdd { title: String::new(), due: None, list: String::new() })),
            Family::RemindersList => return Ok(Ok(Action::RemindersList { list: String::new() })),
            _ => {}
        }
    }
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
        Family::MailList => {
            let l = q.to_lowercase();
            let days = if l.contains("today") {
                1
            } else if l.contains("month") {
                30
            } else if l.contains("week") || l.contains(" from ") || l.starts_with("did ") {
                7
            } else {
                3
            };
            let who = after(q, &["emails from ", "email from ", "mail from ", "anything from "]).or_else(|| {
                // "did Sam email me"
                l.strip_prefix("did ").map(|r| r.split(" email").next().unwrap_or(r).split(" mail").next().unwrap_or(r).trim().to_string()).filter(|s| !s.is_empty() && s.split_whitespace().count() <= 3 && s != "i")
            });
            let query = who.map(|w| without_time(w.trim_end_matches(['?', '.']).trim_end_matches(" this week").trim_end_matches(" today"))).unwrap_or_default();
            Ok(Action::MailList { query, days })
        }
        Family::MailDraft | Family::MailReply => {
            let reply = family == Family::MailReply;
            // Who it's to, by rules: "email Sam about…", "reply to Sam's email saying…".
            let who = if reply {
                after(q, &["reply to the email from ", "reply to the mail from ", "reply to ", "write back to ", "respond to "]).map(|s| {
                    let s = s.split(['\'', '’']).next().unwrap_or(&s).to_string();
                    s.split(" email").next().unwrap_or(&s).split(" saying").next().unwrap_or(&s).trim().to_string()
                })
            } else {
                after(q, &["send an email to ", "send email to ", "send a mail to ", "draft an email to ", "draft an email for ", "compose an email to ", "start an email to ", "write an email to ", "write a email to ", "e-mail ", "email "]).map(|s| {
                    let lower = s.to_lowercase();
                    let cut = [" about ", " saying ", " that ", " to say ", " to tell ", " to ask ", " and ", ":", " asking ", " letting ", " re "].iter().filter_map(|c| lower.find(c)).min().unwrap_or(s.len());
                    s[..cut].trim().to_string()
                })
            }
            .map(|w| w.trim_start_matches("my ").trim_matches(|c: char| c == ',' || c == '.').to_string())
            .unwrap_or_default();
            if who.is_empty() {
                return Ok(Err("Who should the email go to?".into()));
            }
            // A reply: the latest email from them gives the address, subject and what to answer.
            let mut original = String::new();
            let mut original_text = String::new();
            let (mut name, mut subject) = (String::new(), String::new());
            let to: String;
            if reply {
                let found = runner.run(&Action::MailList { query: who.clone(), days: 30 }.command()).await.map_err(|e| AppError::msg(e.text("Mail")))?;
                let first = found.split(RS).map(|r| r.split(US).map(str::trim).collect::<Vec<_>>()).find(|r| r.len() >= 2 && !r[0].is_empty());
                let Some(m) = first else { return Ok(Err(format!("I couldn't find a recent email from {who} in Mail. Who is it to (an email address works)?"))) };
                let (n, t) = sender_parts(m[0]);
                name = n;
                to = t;
                subject = if m[1].to_lowercase().starts_with("re:") { m[1].to_string() } else { format!("Re: {}", m[1]) };
                original = format!("From: {}\nSubject: {}\n\n{}", m[0], m[1], m.get(3).unwrap_or(&""));
                original_text = m.get(3).unwrap_or(&"").to_string();
            } else if who.contains('@') {
                to = who.split_whitespace().find(|w| w.contains('@')).unwrap_or(&who).trim_matches(|c: char| c == '<' || c == '>' || c == ',').to_string();
            } else if runner.pc() {
                return Ok(Err(format!("What's {who}'s email address? I can't look people up on a PC.")));
            } else {
                match find_person(runner, &who, |p| p.emails.clone()).await? {
                    Ok((n, addr)) => {
                        name = n;
                        to = addr;
                    }
                    Err(ask) => return Ok(Err(ask)),
                }
            }
            // The model writes the email from the request (and the email being answered).
            let context = if original.is_empty() { String::new() } else { format!("\n\nThe email they're replying to:\n<<<\n{}\n>>>", original.chars().take(2500).collect::<String>()) };
            let user = format!(
                "The user said: \"{q}\"{context}\n\nWrite the email they asked for, from them to {}: a short subject line and the email itself. Friendly and clear; \
say what they asked and nothing more (don't invent facts, dates or promises); a greeting and a short sign-off without a name.",
                // First names read naturally ("Hi Sam", not "Hi Sam Lee").
                if name.is_empty() { &who } else { name.split_whitespace().next().unwrap_or(&name) }
            );
            let schema = json!({"type":"object","properties":{"subject":{"type":"string"},"body":{"type":"string"}},"required":["subject","body"]});
            let first_name = if name.is_empty() { who.clone() } else { name.split_whitespace().next().unwrap_or(&name).to_string() };
            // Small models sometimes copy the email they're answering: ask again, plainer, then fall back to the
            // user's own words.
            let (mut body, mut new_subject) = (String::new(), String::new());
            for attempt in 0..3 {
                let ask = if attempt == 0 { user.clone() } else { format!("{user}\n\nDon't repeat their email back: answer it, in the user's words (\"{}\").", user_words(q)) };
                let v = chat::complete_json(turn.http, turn.ep, "You write emails for the user. Reply only with JSON.", &ask, schema.clone(), 700).await.ok().map(|r| crate::research::lenient_json(&r));
                let text = |k: &str| v.as_ref().and_then(|v| v.get(k)).and_then(Value::as_str).unwrap_or("").trim().to_string();
                body = without_placeholders(&text("body"));
                new_subject = text("subject");
                if !reply || reply_answers(&body, &original_text) {
                    break;
                }
                body.clear();
            }
            if body.is_empty() && reply && !user_words(q).is_empty() {
                body = format!("Hi {first_name},\n\n{}.\n\nBest,", first_upper(user_words(q).trim_end_matches('.')));
            }
            if body.is_empty() {
                return Ok(Err("What should the email say?".into()));
            }
            if subject.is_empty() {
                subject = new_subject;
            }
            Ok(Action::MailDraft { to, name, subject, body })
        }
        Family::Message => {
            let rest = after_first(q, TEXT_CUES).unwrap_or_default();
            let rl = rest.to_lowercase();
            let cut = SEPARATORS.iter().filter_map(|c| rl.find(c).map(|i| (i, c.len()))).min();
            // "text Mom that I'm late", or plainly "text Mom I'm late".
            let (who, raw) = match (cut, plain_text(&rest)) {
                (Some((i, n)), _) => (rest[..i].trim().trim_start_matches("my ").to_string(), rest[i + n..].to_string()),
                (None, Some((who, body))) => (who, body),
                (None, None) => return Ok(Err("Who should I text, and what should it say? For example: “text Mom I'm on my way”.".into())),
            };
            // The user's own words, as they wrote them.
            let body = first_upper(raw.trim().trim_matches('"').trim_end_matches('.').trim());
            if who.is_empty() || body.is_empty() {
                return Ok(Err("Who should I text, and what should it say?".into()));
            }
            let direct = who.chars().filter(|c| c.is_ascii_digit()).count() >= 7 || who.contains('@');
            let (name, to) = if direct {
                (String::new(), who.clone())
            } else {
                match find_person(runner, &who, |p| if p.phones.is_empty() { p.emails.clone() } else { p.phones.clone() }).await? {
                    Ok(x) => x,
                    Err(ask) => return Ok(Err(ask)),
                }
            };
            let chat = crate::messages::chat_for(&to).unwrap_or_default();
            Ok(Action::MessageSend { to, name, body, chat })
        }
    })
}

/// Finds one person in Contacts and their address (`pick`: emails or phones).
/// Err(question) when nobody or several people match.
async fn find_person(runner: &dyn Runner, who: &str, pick: impl Fn(&Person) -> Vec<String>) -> AppResult<Result<(String, String), String>> {
    let out = runner.run(&Action::ContactFind { name: who.to_string() }.command()).await.map_err(|e| AppError::msg(e.text("Contacts")))?;
    let found: Vec<Person> = people(&out).into_iter().filter(|p| !pick(p).is_empty()).collect();
    // An exact name wins over "Sam" also matching "Samantha" and "Sam Lee".
    let exact: Vec<&Person> = found.iter().filter(|p| p.name.eq_ignore_ascii_case(who)).collect();
    let list: Vec<&Person> = if exact.len() == 1 { exact } else { found.iter().collect() };
    match list.as_slice() {
        [] => Ok(Err(format!("I couldn't find {who} in your Contacts. What's their email address or phone number?"))),
        [p] => Ok(Ok((p.name.clone(), pick(p)[0].clone()))),
        many => Ok(Err(format!("Which {who}? {}", many.iter().take(5).map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")))),
    }
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
        if err.contains("-1743") || err.contains("Not authorized to send Apple events") || err.contains("not allowed assistive access") || err.contains("not allowed to send keystrokes") || err.contains("(1002)") {
            RunError::NotAllowed
        } else if err.contains("-2740") || err.contains("-10814") || err.contains("Unable to find application") {
            RunError::Missing
        } else {
            RunError::Failed(err.lines().last().unwrap_or(err).trim().chars().take(200).collect())
        }
    }

    /// `text`, worded for a PC (`pc`) or a Mac.
    pub fn text_for(&self, app: &str, pc: bool) -> String {
        if !pc {
            return self.text(app);
        }
        match self {
            RunError::NotAllowed => format!("Windows didn't let BYTE use {app}. Check Settings \u{2192} Privacy & security, then try again."),
            RunError::Missing => format!("{app} isn't available on this PC."),
            RunError::Timeout => format!("{app} didn't answer in time."),
            RunError::Failed(e) => format!("{app} said: {e}"),
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

    /// True for the PC's runner: the words, apps and operations of Windows instead of the Mac's.
    fn pc(&self) -> bool {
        false
    }
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
                        "mdfind" => "/usr/bin/mdfind",
                        "sips" => "/usr/bin/sips",
                        // Mac upkeep (upkeep.rs): read-only system tools.
                        "df" => "/bin/df",
                        "top" => "/usr/bin/top",
                        "memory_pressure" => "/usr/bin/memory_pressure",
                        "system_profiler" => "/usr/sbin/system_profiler",
                        "sysctl" => "/usr/sbin/sysctl",
                        "sw_vers" => "/usr/bin/sw_vers",
                        "tmutil" => "/usr/bin/tmutil",
                        "fdesetup" => "/usr/bin/fdesetup",
                        "socketfilterfw" => "/usr/libexec/ApplicationFirewall/socketfilterfw",
                        "defaults" => "/usr/bin/defaults",
                        // The terminal helper; the user approved this exact command line.
                        "zsh" => "/bin/zsh",
                        other => other,
                    };
                    let mut c = tokio::process::Command::new(path);
                    c.args(args);
                    c
                }
                Command::Win(_) => return Err(RunError::Missing),
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
    if runner.pc() {
        return match crate::pcctl::op_for(action) {
            Some(op) => runner.run(&Command::Win(op)).await,
            None => Err(RunError::Missing),
        };
    }
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

/// How to take something back.
#[derive(Debug, Clone, PartialEq)]
pub enum Undo {
    /// Run a fixed script (delete the note, reminder, event or draft).
    Cmd(Command),
    /// Move files back where they were (organizing), newest move first.
    Moves(Vec<(std::path::PathBuf, std::path::PathBuf)>),
    /// Remove files BYTE made (converted copies).
    Created(Vec<std::path::PathBuf>),
}

/// Undo steps for what BYTE did, by token (this session only).
static UNDO: Lazy<Mutex<HashMap<String, Undo>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// The step behind a card's Undo token, without running it (tests).
#[cfg(test)]
pub(crate) fn undo_step(token: &str) -> Option<Undo> {
    UNDO.lock().ok().and_then(|m| m.get(token).cloned())
}

/// Keeps an undo step; returns the token for the card's Undo button.
pub(crate) fn keep_undo(u: Undo) -> String {
    let token = uuid::Uuid::new_v4().simple().to_string();
    if let Ok(mut m) = UNDO.lock() {
        m.insert(token.clone(), u);
    }
    token
}

/// Moves files back (`moves` are (from, to) as done); never overwrites.
/// Returns how many went back.
pub(crate) fn move_back(moves: &[(std::path::PathBuf, std::path::PathBuf)]) -> usize {
    let mut n = 0;
    for (from, to) in moves.iter().rev() {
        if to.exists() && !from.exists() && std::fs::rename(to, from).is_ok() {
            n += 1;
        }
    }
    n
}

/// How to take back what `action` added, from the script's output.
fn undo_for(action: &Action, out: &str) -> Option<Command> {
    let out = out.trim();
    if out.is_empty() {
        return None;
    }
    match action {
        Action::NoteCreate { .. } => Some(Command::Osa { script: NOTE_DELETE, args: vec![out.into()] }),
        Action::ReminderAdd { .. } => Some(Command::Osa { script: REMINDER_DELETE, args: vec![out.into()] }),
        Action::MailDraft { .. } => Some(Command::Osa { script: MAIL_DRAFT_DELETE, args: vec![out.into()] }),
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
    let step = UNDO.lock().ok().and_then(|mut m| m.remove(token));
    match step {
        None => Ok(false),
        Some(Undo::Cmd(cmd)) => {
            // The same step a PC took (Restore from the Recycle Bin) or a Mac took (Put Back).
            let pc = matches!(cmd, Command::Win(_));
            let runner: &dyn Runner = if pc { &crate::pcctl::WinRunner } else { &MacRunner };
            runner.run(&cmd).await.map(|_| true).map_err(|e| AppError::msg(e.text_for("the app", pc)))
        }
        Some(Undo::Moves(moves)) => {
            let back = move_back(&moves);
            if back < moves.len() {
                return Err(AppError::msg(format!("Moved {back} of {} files back; the others were moved or renamed since.", moves.len())));
            }
            Ok(true)
        }
        Some(Undo::Created(files)) => {
            for f in &files {
                let _ = std::fs::remove_file(f);
            }
            Ok(true)
        }
    }
}

/// Shows an approval card for a Mac action and waits for the answer.
pub(crate) async fn ask_ok(title: &str, app: &str, fields: Vec<(String, String)>, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<bool> {
    Ok(ask_ok_edit(title, app, fields, &[], cancel, send).await?.0)
}

/// Like `ask_ok`, with fields the user can change on the card (`editable` labels); returns the changed ones.
pub(crate) async fn ask_ok_edit(title: &str, app: &str, fields: Vec<(String, String)>, editable: &[&str], cancel: &CancellationToken, send: Emit<'_>) -> AppResult<(bool, Vec<crate::web_agent::Field>)> {
    let ask = crate::web_agent::ApprovalAsk {
        id: format!("mac_{}", uuid::Uuid::new_v4().simple()),
        action: "mac".into(),
        title: title.into(),
        site: app.into(),
        url: String::new(),
        target: app.into(),
        fields: fields.into_iter().map(|(label, value)| crate::web_agent::Field { label, value }).collect(),
        editable: editable.iter().map(|s| s.to_string()).collect(),
    };
    let id = ask.id.clone();
    let ok = crate::web_agent::ask(ask, APPROVAL_WAIT, cancel, send).await?;
    Ok((ok, crate::web_agent::take_edits(&id)))
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

/// Runs a read-only action (calendar, reminders) and returns its notes, for the daily briefing.
pub(crate) async fn read_notes(runner: &dyn Runner, action: &Action) -> Result<String, RunError> {
    let out = runner.run(&action.command()).await?;
    Ok(read_out(action, &out).1)
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
        MailList { query, days } => {
            let r = recs(out);
            let span = if *days <= 1 { "today".to_string() } else { format!("in the last {days} days") };
            let what = if query.is_empty() { format!("unread emails {span}") } else { format!("emails from or about \"{query}\" {span}") };
            if r.is_empty() {
                return (format!("No {what}"), format!("BYTE checked Mail: no {what}."));
            }
            let items: Vec<String> = r
                .iter()
                .map(|f| format!("- From {} · {} · \"{}\": {}", f.first().unwrap_or(&""), f.get(2).unwrap_or(&""), f.get(1).unwrap_or(&""), f.get(3).unwrap_or(&"").replace(['\n', '\r'], " ")))
                .collect();
            (
                format!("{} {what}", r.len()),
                format!(
                    "The user's {what} (from Mail, each with the start of the email):\n{}\n\nSummarize them for the user: first the ones that need a reply or action, then other important ones, then the rest in one line. Don't quote long parts.",
                    items.join("\n")
                ),
            )
        }
        MailDraft { to, name, subject, .. } => {
            let who = if name.is_empty() { to } else { name };
            (format!("To {who} · {subject}"), format!("Done: BYTE opened a new email to {who} in Mail (\"{subject}\"). It isn't sent: the user reads it and presses Send."))
        }
        MessageSend { to, name, body, .. } => {
            let who = if name.is_empty() { to } else { name };
            let how = if out.trim() == "sent sms" { " as a text message (SMS)" } else { "" };
            (format!("Sent to {who}{how}"), format!("Done: BYTE sent the text to {who}{how} through Messages: \"{body}\". Tell the user it's sent (it can't be taken back from BYTE; iMessages can be unsent in Messages for a couple of minutes)."))
        }
        ContactFind { .. } => (String::new(), String::new()),
        ShortcutRun { name, .. } => {
            let o: String = out.chars().take(2000).collect();
            ("Ran it".into(), if o.trim().is_empty() { format!("Done: the shortcut \"{name}\" ran.") } else { format!("Done: the shortcut \"{name}\" ran. It returned:\n{o}") })
        }
    }
}

pub(crate) const TELL: &str = "Tell the user what happened in one or two short sentences, in plain words. Don't show code or commands. \
Don't claim anything that isn't in these notes.";

// ---------------------------------------------------------------------- run

/// Does what the message asks on the Mac. Returns notes for the written answer.
pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let now = chrono::Local::now().naive_local();
    if cfg!(windows) {
        run_with(turn, question, now, &crate::pcctl::WinRunner, cancel, send).await
    } else {
        run_with(turn, question, now, &MacRunner, cancel, send).await
    }
}

pub(crate) async fn run_with(turn: &Turn<'_>, question: &str, now: NaiveDateTime, runner: &dyn Runner, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(p) = plan_on(question, runner.pc()) else { return Ok(None) };
    let mut action = match p {
        Plan::Ready(a) => a,
        Plan::Ask(f) => {
            let got = tokio::select! {
                r = details(turn, f, question, now, runner) => r?,
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
            };
            match got {
                Ok(a) => a,
                Err(ask) => return Ok(Some((SourceBook::default(), format!("BYTE needs one more detail before doing this on the {}. Ask the user exactly this, briefly: {ask}", if runner.pc() { "PC" } else { "Mac" })))),
            }
        }
    };
    let pc = runner.pc();
    if pc {
        if let Some(note) = crate::pcctl::unsupported(&action) {
            return Ok(Some((SourceBook::default(), note)));
        }
    }
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
    let app = action.app_for(pc);
    let id = format!("byte_mac_{}", uuid::Uuid::new_v4().simple());
    let args = json!({ "app": app, "what": action.describe_for(pc) });
    send(ChatEvent::ToolCall { id: id.clone(), name: action.tool().into(), args: args.clone() })?;

    if action.needs_ok() {
        let editable: &[&str] = if matches!(action, Action::MessageSend { .. }) { &["Text"] } else { &[] };
        let mut fields = action.fields();
        if pc {
            crate::pcctl::pc_fields(&action, &mut fields);
        }
        let (ok, edits) = ask_ok_edit(&action.describe_for(pc), app, fields, editable, cancel, send).await?;
        // The text as the user left it on the card (edited, fixed or rephrased there).
        if let (Action::MessageSend { body, .. }, Some(text)) = (&mut action, edits.iter().find(|f| f.label == "Text").map(|f| f.value.trim().to_string())) {
            if !text.is_empty() {
                *body = text;
            }
        }
        if !ok {
            send(ChatEvent::ToolResult { id, ok: false, summary: "You said no".into() })?;
            turn.log.record(action.tool(), &args, false, "declined");
            return Ok(Some((SourceBook::default(), format!("The user chose not to let BYTE {}. Nothing was changed. Say so in one sentence.", lower_first(&action.describe_for(pc))))));
        }
    }

    // On a PC, notes are BYTE's own: they're saved and searched here, not in another app.
    if pc && matches!(action, Action::NoteCreate { .. } | Action::NoteFind { .. }) {
        return pc_notes(turn, id, &action, &args, send).await;
    }
    let out = tokio::select! {
        r = execute(runner, &action) => r,
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
    };
    // Sending didn't work (no iMessage account, Messages refused): open it filled in instead, for the user to send.
    if let (Err(e), Action::MessageSend { to, name, body, .. }) = (&out, &action) {
        if runner.run(&Command::Osa { script: MESSAGE_DRAFT, args: vec![body.clone(), sms_url(to, body)] }).await.is_ok() {
            let who = if name.is_empty() { to } else { name };
            let why = e.text_for(app, pc);
            send(ChatEvent::ToolResult { id, ok: false, summary: "Opened in Messages instead".into() })?;
            turn.log.record(action.tool(), &args, false, &why);
            send(ChatEvent::MacDone(MacDone { app: app.into(), title: format!("Couldn't send it, so Messages is open with the text to {who}"), detail: why.clone(), ok: false, undo: None }))?;
            return Ok(Some((SourceBook::default(), format!("BYTE couldn't send the text directly ({why}), so it opened Messages with the text to {who} filled in (also on the clipboard). Tell the user to press Send there."))));
        }
    }
    match out {
        Ok(out) => {
            let (detail, notes) = if pc { crate::pcctl::read_out(&action, &out).unwrap_or_else(|| read_out(&action, &out)) } else { read_out(&action, &out) };
            let undo = if pc { None } else { undo_for(&action, &out).map(|cmd| keep_undo(Undo::Cmd(cmd))) };
            send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
            turn.log.record(action.tool(), &args, true, &detail);
            send(ChatEvent::MacDone(MacDone { app: app.into(), title: action.describe_for(pc), detail, ok: true, undo }))?;
            Ok(Some((SourceBook::default(), format!("{notes}\n\n{TELL}"))))
        }
        Err(e) => {
            let msg = e.text_for(app, pc);
            send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
            turn.log.record(action.tool(), &args, false, &msg);
            send(ChatEvent::MacDone(MacDone { app: app.into(), title: action.describe_for(pc), detail: msg.clone(), ok: false, undo: None }))?;
            Ok(Some((SourceBook::default(), format!("BYTE tried to {} but it didn't work: {msg}\n\nExplain this to the user briefly, with the steps to fix it if there are any.", lower_first(&action.describe_for(pc))))))
        }
    }
}

/// A PC's notes are BYTE's own (Documents/BYTE/Notes): saved with an Undo that removes the file, or searched.
async fn pc_notes(turn: &Turn<'_>, id: String, action: &Action, args: &Value, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(app) = turn.app else {
        let msg = "BYTE's notes aren't available here.".to_string();
        send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
        return Ok(Some((SourceBook::default(), format!("BYTE couldn't use its notes: {msg}"))));
    };
    let app_name = action.app_for(true);
    match action {
        Action::NoteCreate { title, body } => {
            let input = crate::notes::NoteInput { title: title.clone(), body: body.clone(), source: "chat".into(), ..Default::default() };
            match crate::notes::save_note(app, &input).await {
                Ok(note) => {
                    let undo = keep_undo(Undo::Created(vec![std::path::PathBuf::from(&note.path)]));
                    let detail = format!("\"{}\" in {}", note.title, note.folder);
                    send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
                    turn.log.record(action.tool(), args, true, &detail);
                    send(ChatEvent::MacDone(MacDone { app: app_name.into(), title: action.describe_for(true), detail, ok: true, undo: Some(undo) }))?;
                    Ok(Some((SourceBook::default(), format!("Done: BYTE saved the note \"{}\" in its Notes (folder {}).\n\n{TELL}", note.title, note.folder))))
                }
                Err(e) => {
                    let msg = e.to_string();
                    send(ChatEvent::ToolResult { id, ok: false, summary: msg.clone() })?;
                    turn.log.record(action.tool(), args, false, &msg);
                    send(ChatEvent::MacDone(MacDone { app: app_name.into(), title: action.describe_for(true), detail: msg.clone(), ok: false, undo: None }))?;
                    Ok(Some((SourceBook::default(), format!("BYTE tried to save the note but it didn't work: {msg}\n\nExplain this to the user briefly."))))
                }
            }
        }
        Action::NoteFind { query } => {
            let root = crate::notes::root(app).await?;
            let found = crate::notes::search(&crate::notes::list(&root), query);
            let (detail, notes) = if found.is_empty() {
                ("No matching notes".to_string(), format!("BYTE searched the user's notes for \"{query}\" and found nothing."))
            } else {
                let list: Vec<String> = found.iter().take(5).map(|n| format!("- **{}**: {}", n.title, n.body.chars().take(400).collect::<String>().replace('\n', " "))).collect();
                (format!("{} notes found", found.len()), format!("The user's notes that match \"{query}\" (from BYTE's Notes):\n{}", list.join("\n")))
            };
            send(ChatEvent::ToolResult { id, ok: true, summary: detail.clone() })?;
            turn.log.record(action.tool(), args, true, &detail);
            send(ChatEvent::MacDone(MacDone { app: app_name.into(), title: action.describe_for(true), detail, ok: true, undo: None }))?;
            Ok(Some((SourceBook::default(), format!("{notes}\n\n{TELL}"))))
        }
        _ => Ok(None),
    }
}

pub(crate) fn lower_first(s: &str) -> String {
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
