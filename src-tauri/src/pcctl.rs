//! PC control (Windows): what `macctl.rs` does on a Mac, on a PC. "Turn on dark mode", "set the volume
//! to 30", "pause the music", "open Bluetooth settings", "email Sam about Friday", "add lunch with Sam
//! to my calendar tomorrow at noon".
//!
//! It reuses everything in `macctl.rs` that isn't about Apple: the sentence reader, the approval card, the
//! result card and the notes for the answer. Only the last step differs, and it follows the same safety
//! rules:
//! - Every operation here is a fixed Rust function. The user's words (a song, an address, a subject) are
//!   data handed to it, never part of a command line or a script.
//! - Anything that adds something lasting asks first with the approval card.
//! - BYTE never sends an email: it opens the draft for the person to read and send.
//!
//! What a PC does instead of the Mac's apps: notes and reminders stay inside BYTE (its own Notes and Tasks),
//! the email draft and the calendar entry open in whatever the person has set as their mail and calendar app,
//! music is controlled through the media session Windows already keeps for Spotify, the browser and the rest.
//! What Windows has no way to do (Shortcuts, reading Apple Messages, Safari's tab) is said plainly.

use chrono::{Datelike, Duration as Days, NaiveDateTime, Timelike};
use futures_util::future::BoxFuture;

use crate::macctl::{Action, Command, RunError, Runner, Switch};

/// A PC operation. Pure data: `run_op` is the only code that touches Windows.
#[derive(Debug, Clone, PartialEq)]
pub enum WinOp {
    DarkMode(Switch),
    /// 0 to 100.
    Volume(u8),
    Mute(bool),
    Wifi(bool),
    SleepDisplay,
    /// Opens an `ms-settings:` page, a `mailto:` link or a web address with whatever Windows uses for it.
    Open(String),
    /// Writes the event as an .ics file and opens it; the calendar app asks the person to save it.
    AddEvent { title: String, start: NaiveDateTime, minutes: u32 },
    /// Starts a search for a song in Spotify when it is installed, else in YouTube Music in the browser.
    PlayMusic(String),
    MediaPlay,
    MediaPause,
    MediaNext,
    MediaPrevious,
    NowPlaying,
    /// Moves files to the Recycle Bin (never deletes). Prints one line per file: `ok`, or `!` and why not.
    Recycle(Vec<std::path::PathBuf>),
    /// Puts files back from the Recycle Bin where they came from. Prints how many went back.
    Restore(Vec<std::path::PathBuf>),
    /// Shows a file in File Explorer, selected.
    Reveal(std::path::PathBuf),
    /// Memory, disk, boot time and power, as lines: `mem=<total>;<available>` and `disk=<total>;<free>` (bytes),
    /// `boot=<unix seconds>`, `os=<name>`, and on a laptop `power=<percent>;<on mains 1/0>;<seconds left, -1 unknown>`.
    Snapshot,
    /// The programs using the PC, one line per program name: `name|cpu percent|bytes|has a window 1/0`, busiest first.
    Processes,
    /// Windows Security, the firewall, disk encryption, updates and battery wear, read-only, as `key=value` lines.
    Security,
    /// Asks a program's windows to close, by program name; prints how many windows were asked.
    Quit(String),
    /// What starts with Windows, read-only: one line per entry, tab-separated `scope`, `kind`, `name`, `command`, `on 1/0`.
    /// Scope is `user` or `machine`; kind is `run` (the Run key), `run32` (the 32-bit Run key) or `folder` (a Startup folder).
    StartupList,
    /// Turns one entry on or off the way Task Manager's Startup tab does (Windows' own "approved" flag, for this user): the
    /// entry itself, the program and its files stay as they are. `kind` and `name` are as `StartupList` printed them.
    StartupSet { kind: String, name: String, on: bool },
    /// The programs installed for this user or the whole PC, read-only, tab-separated: `source` (`reg` or `appx`), `id`, `name`,
    /// `version`, `publisher`, `size in KB` (0 when unknown). Windows components, updates and drivers' sub-entries are left out.
    Programs,
    /// Runs one listed program's own uninstaller, found again by its `id` (never a command from anywhere else). A Microsoft Store
    /// app is removed for this user. Prints `removed` when the program is gone, `started` when its uninstaller is still working.
    Uninstall { id: String },
    /// Asks Windows Search (its index, so names and the text inside documents) for files under `root` that match every word;
    /// prints `name<TAB>path` for matches in the file name and `text<TAB>path` for matches inside documents, newest first. The
    /// words are letters, digits and a few joiners only (`find_word_ok`); they reach the query through a file.
    FindFiles { words: Vec<String>, root: std::path::PathBuf },
}

/// Whether a search word is safe to put in a Windows Search query: letters, digits, `-`, `_` and `.`, up to 40 of them.
pub fn find_word_ok(w: &str) -> bool {
    !w.is_empty() && w.chars().count() <= 40 && w.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Where Windows keeps one of the person's folders, which may be inside OneDrive: `Downloads`, `Desktop`, `Documents`, `Pictures`,
/// `Videos` or `Music`. `None` when it isn't a PC or the name isn't one of those.
pub fn known_folder(name: &str) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        win::known_folder(name)
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        None
    }
}

/// Whether `id` is one that `WinOp::Programs` prints: a registry entry (`reg|HKCU|<key name>`, also `HKLM` and `HKLM32` for the
/// 32-bit view) or a Store package (`appx|<full package name>`). Nothing else is ever handed to the uninstall script.
pub fn program_id_ok(id: &str) -> bool {
    let plain = |s: &str, extra: &[char]| !s.is_empty() && s.len() <= 200 && s.chars().all(|c| c.is_alphanumeric() || extra.contains(&c));
    let mut parts = id.splitn(3, '|');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("reg"), Some("HKCU" | "HKLM" | "HKLM32"), Some(key)) => {
            // Key names are what installers chose: GUIDs in braces, product names with spaces, dots and dashes. Never a path.
            !key.is_empty() && key.len() <= 200 && !key.contains(['\\', '/', '|', '"', '\n', '\r', '\t']) && key.chars().all(|c| !c.is_control())
        }
        (Some("appx"), Some(full), None) => plain(full, &['.', '_', '-', '~']),
        _ => false,
    }
}

/// The operation that does `action` on a PC, when Windows can do it from here.
pub fn op_for(action: &Action) -> Option<WinOp> {
    use Action::*;
    Some(match action {
        DarkMode(s) => WinOp::DarkMode(*s),
        Volume(v) => WinOp::Volume(*v),
        Mute(m) => WinOp::Mute(*m),
        Wifi(on) => WinOp::Wifi(*on),
        SleepDisplay => WinOp::SleepDisplay,
        OpenSettings { pane } => WinOp::Open(settings_uri(pane)),
        MusicPlay { query } if query.trim().is_empty() => WinOp::MediaPlay,
        MusicPlay { query } => WinOp::PlayMusic(query.trim().to_string()),
        MusicPause => WinOp::MediaPause,
        MusicNext => WinOp::MediaNext,
        MusicPrevious => WinOp::MediaPrevious,
        NowPlaying => WinOp::NowPlaying,
        MailDraft { to, subject, body, .. } => WinOp::Open(mailto(to, subject, body)),
        EventAdd { title, start, minutes, .. } => WinOp::AddEvent { title: title.clone(), start: *start, minutes: *minutes },
        _ => return None,
    })
}

/// Why `action` can't be done on a PC, as a note for the model to tell the user. `None` when it can, or when BYTE's
/// own notes handle it (see `macctl::run_with`).
pub fn unsupported(action: &Action) -> Option<String> {
    use Action::*;
    let note = match action {
        ShortcutsList | ShortcutRun { .. } => {
            "Windows has no Shortcuts app. Tell the user BYTE can open a program or run a PowerShell command they approve instead, and offer that."
        }
        SafariTab => "BYTE can't read the tab open in a Windows browser. Tell the user to paste the page's address and BYTE will read it.",
        MessageSend { .. } => {
            "Texting from a PC needs a phone linked through Phone Link, and Windows gives other programs no way to send through it. Tell the user BYTE can't send texts on a PC, and offer to write the text so they can paste it."
        }
        ContactFind { .. } => "Windows has no contacts app BYTE can search. Ask the user for the email address or phone number.",
        ReminderAdd { .. } | RemindersList { .. } => {
            "On a PC, reminders live in BYTE's own Tasks. Tell the user to turn Tasks on in Settings, then ask again."
        }
        EventsList { .. } => {
            "Windows has no calendar that other programs can read, so BYTE can't look at the user's calendar on a PC. Say so, and offer to add an event instead."
        }
        MailList { .. } => "BYTE can't read the inbox of a PC's mail app. Say so, and offer to write a new email instead.",
        _ => return None,
    };
    Some(note.to_string())
}

// -------------------------------------------------------------------- settings pages

/// Windows Settings pages by the words people use: (key, label, `ms-settings:` page). The longest key found in the
/// message wins, so "windows update" beats "update" and "night light" beats "light".
const PANES: &[(&str, &str, &str)] = &[
    ("wi-fi", "Wi-Fi", "network-wifi"),
    ("wifi", "Wi-Fi", "network-wifi"),
    ("bluetooth", "Bluetooth & devices", "bluetooth"),
    ("sound", "Sound", "sound"),
    ("volume", "Sound", "sound"),
    ("audio", "Sound", "sound"),
    ("display", "Display", "display"),
    ("screen", "Display", "display"),
    ("brightness", "Display", "display"),
    ("resolution", "Display", "display"),
    ("night light", "Night light", "nightlight"),
    ("startup", "Startup apps", "startupapps"),
    ("startup apps", "Startup apps", "startupapps"),
    ("encryption", "Device encryption", "deviceencryption"),
    ("bitlocker", "Device encryption", "deviceencryption"),
    ("battery", "Battery", "batterysaver"),
    ("power", "Power & sleep", "powersleep"),
    ("sleep", "Power & sleep", "powersleep"),
    ("notification", "Notifications", "notifications"),
    ("privacy", "Privacy & security", "privacy"),
    ("security", "Windows Security", "windowsdefender"),
    ("antivirus", "Windows Security", "windowsdefender"),
    ("accessibility", "Accessibility", "easeofaccess"),
    ("wallpaper", "Background", "personalization-background"),
    ("background", "Background", "personalization-background"),
    ("appearance", "Colors", "personalization-colors"),
    ("colors", "Colors", "personalization-colors"),
    ("personalization", "Personalization", "personalization"),
    ("theme", "Themes", "themes"),
    ("taskbar", "Taskbar", "personalization-taskbar"),
    ("keyboard", "Typing", "typing"),
    ("mouse", "Mouse", "mousetouchpad"),
    ("trackpad", "Touchpad", "devices-touchpad"),
    ("touchpad", "Touchpad", "devices-touchpad"),
    ("network", "Network & internet", "network"),
    ("vpn", "VPN", "network-vpn"),
    ("airplane", "Airplane mode", "network-airplanemode"),
    ("windows update", "Windows Update", "windowsupdate"),
    ("software update", "Windows Update", "windowsupdate"),
    ("update", "Windows Update", "windowsupdate"),
    ("storage", "Storage", "storagesense"),
    ("startup", "Startup apps", "startupapps"),
    ("login item", "Startup apps", "startupapps"),
    ("default apps", "Default apps", "defaultapps"),
    ("installed apps", "Installed apps", "appsfeatures"),
    ("apps", "Apps", "appsfeatures"),
    ("printer", "Printers & scanners", "printers"),
    ("password", "Sign-in options", "signinoptions"),
    ("sign-in", "Sign-in options", "signinoptions"),
    ("pin", "Sign-in options", "signinoptions"),
    ("account", "Your info", "yourinfo"),
    ("users", "Other users", "otherusers"),
    ("date", "Date & time", "dateandtime"),
    ("time", "Date & time", "dateandtime"),
    ("language", "Language & region", "regionlanguage"),
    ("backup", "Backup", "backup"),
    ("gaming", "Gaming", "gaming-gamebar"),
    ("game mode", "Game Mode", "gaming-gamemode"),
    ("about", "About", "about"),
];

fn pane_row(text: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    let t = text.to_lowercase();
    let mut best: Option<&(&str, &str, &str)> = None;
    for p in PANES {
        if t.contains(p.0) && best.is_none_or(|b| p.0.len() > b.0.len()) {
            best = Some(p);
        }
    }
    best
}

/// The key of the Settings page the words ask for ("bluetooth", "windows update"), if any.
pub fn pane_for(text: &str) -> Option<&'static str> {
    pane_row(text).map(|p| p.0)
}

pub fn pane_label(pane: &str) -> &'static str {
    pane_row(pane).map(|p| p.1).unwrap_or("Settings")
}

/// The `ms-settings:` address for a pane key; Settings' home page when there's none.
pub fn settings_uri(pane: &str) -> String {
    match pane_row(pane) {
        Some(p) => format!("ms-settings:{}", p.2),
        None => "ms-settings:".into(),
    }
}

// -------------------------------------------------------------------- mail, calendar, music

/// Percent-encodes one part of a URL (everything except letters, digits and `-_.~`).
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A `mailto:` link that opens a new email in the person's mail app. Line breaks become CRLF, as mail apps expect;
/// the body is cut at 1,800 characters because Windows hands the whole link to the app and long ones are refused.
pub fn mailto(to: &str, subject: &str, body: &str) -> String {
    let body: String = body.chars().take(1800).collect::<String>().replace("\r\n", "\n").replace('\n', "\r\n");
    format!("mailto:{}?subject={}&body={}", encode(to.trim()), encode(subject), encode(&body))
}

/// Text escaped for an iCalendar value.
#[cfg_attr(not(windows), allow(dead_code))] // used by the Windows runner (and the tests everywhere)
fn ics_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

#[cfg_attr(not(windows), allow(dead_code))] // used by the Windows runner (and the tests everywhere)
fn ics_time(t: NaiveDateTime) -> String {
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}", t.year(), t.month(), t.day(), t.hour(), t.minute(), t.second())
}

/// An iCalendar file with one event. The times have no time zone, so every calendar app reads them as local time.
#[cfg_attr(not(windows), allow(dead_code))] // used by the Windows runner (and the tests everywhere)
pub fn ics(title: &str, start: NaiveDateTime, minutes: u32, uid: &str, now: NaiveDateTime) -> String {
    let end = start + Days::minutes(i64::from(minutes.max(1)));
    [
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//BYTE//Calendar//EN".to_string(),
        "BEGIN:VEVENT".to_string(),
        format!("UID:{uid}@byte"),
        format!("DTSTAMP:{}", ics_time(now)),
        format!("DTSTART:{}", ics_time(start)),
        format!("DTEND:{}", ics_time(end)),
        format!("SUMMARY:{}", ics_text(title)),
        "END:VEVENT".to_string(),
        "END:VCALENDAR".to_string(),
    ]
    .join("\r\n")
        + "\r\n"
}

/// Where a song search opens when Spotify isn't installed.
#[cfg_attr(not(windows), allow(dead_code))] // used by the Windows runner (and the tests everywhere)
pub fn music_web_uri(query: &str) -> String {
    format!("https://music.youtube.com/search?q={}", encode(query))
}

#[cfg_attr(not(windows), allow(dead_code))] // used by the Windows runner (and the tests everywhere)
pub fn spotify_uri(query: &str) -> String {
    format!("spotify:search:{}", encode(query))
}

// -------------------------------------------------------------------- cards

/// Card text and notes for the model, for the actions whose words differ on a PC. `None`: use `macctl::read_out`.
pub fn read_out(action: &Action, out: &str) -> Option<(String, String)> {
    use Action::*;
    let out = out.trim();
    match action {
        OpenSettings { pane } => Some((format!("{} settings", pane_label(pane)), format!("Done: Windows Settings is open at {}.", pane_label(pane)))),
        MusicPlay { query } if !query.trim().is_empty() => {
            let how = if out == "spotify" { "Spotify" } else { "YouTube Music in the browser" };
            Some((
                format!("Opened in {how}"),
                format!("Done: BYTE opened a search for \"{query}\" in {how}. It doesn't start playing by itself: tell the user to press play on the result."),
            ))
        }
        MusicPlay { .. } | MusicPause | MusicNext | MusicPrevious | NowPlaying => Some(match out {
            "not running" => ("Nothing is playing".into(), "No music or video app is playing right now, so there's nothing to control.".into()),
            "stopped" => ("Nothing playing".into(), if matches!(action, MusicPause) { "Done: the music is paused.".into() } else { "Nothing is playing right now.".into() }),
            o => {
                let (song, artist) = o.split_once(crate::macctl::US).unwrap_or((o, ""));
                let by = if artist.is_empty() { String::new() } else { format!(" by {artist}") };
                let line = if artist.is_empty() { song.to_string() } else { format!("{song} \u{2014} {artist}") };
                if matches!(action, MusicPause) {
                    (format!("Paused \u{b7} {line}"), format!("Done: paused \"{song}\"{by}."))
                } else {
                    (line, format!("Now playing \"{song}\"{by}."))
                }
            }
        }),
        MailDraft { to, name, subject, .. } => {
            let who = if name.is_empty() { to } else { name };
            Some((
                format!("To {who} \u{b7} {subject}"),
                format!("Done: BYTE opened a new email to {who} (\"{subject}\") in the user's mail app. It isn't sent: the user reads it and presses Send."),
            ))
        }
        EventAdd { title, start, minutes, .. } => {
            let (when, length) = (crate::macctl::when_text(*start), crate::macctl::minutes_text(*minutes));
            Some((
                format!("{when} \u{b7} {length}"),
                format!("Done: BYTE opened \"{title}\" on {when} for {length} in the user's calendar app. It isn't in the calendar until the user saves it there: tell them to press Save."),
            ))
        }
        _ => None,
    }
}

/// The approval card's notes, worded for a PC.
pub fn pc_fields(action: &Action, fields: &mut Vec<(String, String)>) {
    match action {
        Action::MailDraft { .. } => {
            fields.retain(|(label, _)| label != "Note");
            fields.push(("Note".into(), "Your mail app opens it for you to read and send. BYTE doesn't send it.".into()));
        }
        Action::EventAdd { .. } => fields.push(("Note".into(), "Your calendar app opens it and asks you to save it.".into())),
        Action::NoteCreate { .. } => fields.push(("Note".into(), "Saved in BYTE's Notes folder. You can undo it from the result.".into())),
        _ => {}
    }
}

// -------------------------------------------------------------------- the runner

/// Runs `Command::Win` operations (the PC's counterpart of `macctl::MacRunner`).
pub struct WinRunner;

impl Runner for WinRunner {
    fn pc(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, cmd: &'a Command) -> BoxFuture<'a, Result<String, RunError>> {
        Box::pin(async move {
            match cmd {
                Command::Win(op) => {
                    let op = op.clone();
                    tokio::task::spawn_blocking(move || run_op(&op)).await.map_err(|e| RunError::Failed(e.to_string()))?
                }
                _ => Err(RunError::Missing),
            }
        })
    }
}

/// Does one operation. Output follows the Mac scripts' conventions (see `macctl::read_out`), so the same card and
/// notes code reads it: dark mode prints `true` when it is now dark, the volume prints the new percent, the media
/// operations print `song<US>artist`, `stopped` or `not running`.
pub fn run_op(op: &WinOp) -> Result<String, RunError> {
    #[cfg(windows)]
    {
        win::run(op)
    }
    #[cfg(not(windows))]
    {
        let _ = op;
        Err(RunError::Missing)
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::macctl::US;
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
    use windows::Media::Control::{GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Playback};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SendMessageTimeoutW, HWND_BROADCAST, SC_MONITORPOWER, SMTO_ABORTIFHUNG, SW_SHOWNORMAL, WM_SETTINGCHANGE, WM_SYSCOMMAND};

    fn fail(e: windows::core::Error) -> RunError {
        RunError::Failed(e.message().trim().to_string())
    }

    /// WinRT and COM need initialising on the calling thread; an error only means the thread already has it.
    fn ensure_winrt() {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    }

    fn ensure_com() {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    pub fn run(op: &WinOp) -> Result<String, RunError> {
        match op {
            WinOp::DarkMode(s) => dark_mode(*s),
            WinOp::Volume(v) => volume(*v),
            WinOp::Mute(m) => mute(*m),
            WinOp::Wifi(on) => wifi(*on),
            WinOp::SleepDisplay => {
                // 2 = off. Any key or mouse move wakes it.
                unsafe { PostMessageW(Some(HWND_BROADCAST), WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER as usize), LPARAM(2)) }.map_err(fail)?;
                Ok(String::new())
            }
            WinOp::Open(target) => open(target).map(|_| String::new()),
            WinOp::AddEvent { title, start, minutes } => add_event(title, *start, *minutes),
            WinOp::PlayMusic(query) => play_music(query),
            WinOp::MediaPlay => control(|s| s.TryPlayAsync().and_then(|o| o.join())),
            WinOp::MediaPause => control(|s| s.TryPauseAsync().and_then(|o| o.join())),
            WinOp::MediaNext => control(|s| s.TrySkipNextAsync().and_then(|o| o.join())),
            WinOp::MediaPrevious => control(|s| s.TrySkipPreviousAsync().and_then(|o| o.join())),
            WinOp::NowPlaying => now_playing(),
            WinOp::Recycle(paths) => Ok(paths
                .iter()
                .map(|p| match crate::recycle::move_to_recycle_bin(p) {
                    Ok(()) => "ok".to_string(),
                    Err(e) => format!("!{e}"),
                })
                .collect::<Vec<_>>()
                .join("\n")),
            WinOp::Restore(paths) => restore(paths),
            WinOp::Reveal(path) => reveal(path),
            WinOp::Snapshot => snapshot(),
            WinOp::Processes => processes(),
            WinOp::Security => security(),
            WinOp::Quit(name) => quit(name),
            WinOp::StartupList => startup_list(),
            WinOp::StartupSet { kind, name, on } => startup_set(kind, name, *on),
            WinOp::Programs => programs(),
            WinOp::Uninstall { id } => uninstall(id),
            WinOp::FindFiles { words, root } => find_files(words, root),
        }
    }

    /// Runs a fixed PowerShell script from a file in the temp folder, with `args` after it, and returns what it printed.
    /// The script text is always one of the constants in this file; nothing a person typed is ever part of it or of the
    /// command line.
    fn run_script(name: &str, text: &str, args: &[&std::path::Path]) -> Result<String, RunError> {
        use std::os::windows::process::CommandExt;
        let dir = std::env::temp_dir().join("BYTE-upkeep");
        std::fs::create_dir_all(&dir).map_err(|e| RunError::Failed(e.to_string()))?;
        let script = dir.join(name);
        std::fs::write(&script, text).map_err(|e| RunError::Failed(e.to_string()))?;
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .args(args)
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { RunError::Missing } else { RunError::Failed(e.to_string()) })?;
        if !out.status.success() {
            return Err(RunError::Failed(String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("PowerShell failed").to_string()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn snapshot() -> Result<String, RunError> {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        let mut lines = vec![
            format!("mem={};{}", sys.total_memory(), sys.available_memory()),
            format!("boot={}", sysinfo::System::boot_time()),
            format!("os={}", sysinfo::System::long_os_version().unwrap_or_default()),
        ];
        let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("C:\\"));
        let (total, free) = crate::system::disk_space_for(&home);
        if total > 0 {
            lines.push(format!("disk={total};{free}"));
        }
        let mut st = SYSTEM_POWER_STATUS::default();
        // BatteryFlag 128 is "no battery" and 255 "unknown"; a percent of 255 is unknown too.
        if unsafe { GetSystemPowerStatus(&mut st) }.is_ok() && st.BatteryFlag != 128 && st.BatteryFlag != 255 && st.BatteryLifePercent <= 100 {
            let left: i64 = if st.BatteryLifeTime == u32::MAX { -1 } else { i64::from(st.BatteryLifeTime) };
            lines.push(format!("power={};{};{left}", st.BatteryLifePercent, u8::from(st.ACLineStatus == 1)));
        }
        Ok(lines.join("\n"))
    }

    /// The process ids that own a window the person can see: visible, with a title, and not a tool window or a child of
    /// another window. These are "apps" in the sense the card means; the rest is Windows and background services.
    pub(super) fn window_pids() -> std::collections::HashSet<u32> {
        windows_of(None).into_iter().map(|(_, pid)| pid).collect()
    }

    /// (window, process id) for each such window; only those of the given processes when a set is given.
    fn windows_of(only: Option<&std::collections::HashSet<u32>>) -> Vec<(windows::Win32::Foundation::HWND, u32)> {
        use windows::core::BOOL;
        use windows::Win32::Foundation::{HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW};
        struct Found(Vec<(HWND, u32)>);
        unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let found = &mut *(lparam.0 as *mut Found);
            let owned = GetWindow(hwnd, GW_OWNER).map(|h| !h.0.is_null()).unwrap_or(false);
            let tool = (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW.0 != 0;
            if IsWindowVisible(hwnd).as_bool() && GetWindowTextLengthW(hwnd) > 0 && !owned && !tool {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                found.0.push((hwnd, pid));
            }
            BOOL(1)
        }
        let mut found = Found(Vec::new());
        // SAFETY: `found` outlives the call, and the callback only pushes to it.
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut found as *mut Found as isize)) };
        found.0.into_iter().filter(|(_, pid)| only.is_none_or(|set| set.contains(pid))).collect()
    }

    fn processes() -> Result<String, RunError> {
        use sysinfo::{ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        // A processor reading needs two samples a moment apart.
        std::thread::sleep(std::time::Duration::from_millis(600));
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let windows = window_pids();
        let mut by: std::collections::HashMap<String, (f32, u64, bool)> = std::collections::HashMap::new();
        for (pid, p) in sys.processes() {
            let name = p.name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case("System Idle Process") || name.is_empty() {
                continue;
            }
            let e = by.entry(name).or_default();
            e.0 += p.cpu_usage();
            e.1 += p.memory();
            e.2 |= windows.contains(&pid.as_u32());
        }
        // The busiest by processor, and the biggest by memory: which of them matters depends on what is short.
        let mut rows: Vec<(String, (f32, u64, bool))> = by.into_iter().collect();
        rows.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0).then(b.1 .1.cmp(&a.1 .1)));
        let mut keep: Vec<(String, (f32, u64, bool))> = rows.iter().take(14).cloned().collect();
        rows.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
        for r in rows.into_iter().take(10) {
            if !keep.iter().any(|k| k.0 == r.0) {
                keep.push(r);
            }
        }
        Ok(keep.into_iter().map(|(n, (cpu, mem, w))| format!("{n}|{cpu:.1}|{mem}|{}", u8::from(w))).collect::<Vec<_>>().join("\n"))
    }

    fn quit(name: &str) -> Result<String, RunError> {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        use sysinfo::{ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let want = name.to_lowercase();
        let want = want.strip_suffix(".exe").unwrap_or(&want).to_string();
        let pids: std::collections::HashSet<u32> = sys
            .processes()
            .iter()
            .filter(|(_, p)| {
                let have = p.name().to_string_lossy().to_lowercase();
                have.strip_suffix(".exe").unwrap_or(&have) == want
            })
            .map(|(pid, _)| pid.as_u32())
            .collect();
        let mut asked = 0;
        // WM_CLOSE is what the window's close button sends: the program may ask to save first, as it would for the person.
        for (hwnd, _) in windows_of(Some(&pids)) {
            if unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok() {
                asked += 1;
            }
        }
        Ok(asked.to_string())
    }

    fn startup_list() -> Result<String, RunError> {
        const SCRIPT: &str = r#"$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$approved = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved'
function Clean($s) { ([string]$s) -replace "[\t\r\n]+", ' ' }
function On($sub, $name) {
  $v = (Get-ItemProperty -LiteralPath "$approved\$sub" -Name $name).$name
  if ($v -is [byte[]] -and $v.Length -gt 0 -and ($v[0] -band 1) -eq 1) { 0 } else { 1 }
}
$runs = @(
  @{ scope = 'user';    key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run';                kind = 'run';   sub = 'Run' },
  @{ scope = 'machine'; key = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run';                kind = 'run';   sub = 'Run' },
  @{ scope = 'machine'; key = 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run';   kind = 'run32'; sub = 'Run32' }
)
foreach ($r in $runs) {
  $k = Get-Item -LiteralPath $r.key
  if ($k) { foreach ($n in $k.GetValueNames()) { if ($n) { "$($r.scope)`t$($r.kind)`t$(Clean $n)`t$(Clean $k.GetValue($n))`t$(On $r.sub $n)" } } }
}
$folders = @(
  @{ scope = 'user';    dir = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup" },
  @{ scope = 'machine'; dir = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\StartUp" }
)
foreach ($f in $folders) {
  Get-ChildItem -LiteralPath $f.dir -File -Force | Where-Object { $_.Name -ne 'desktop.ini' } | ForEach-Object {
    $target = $_.FullName
    if ($_.Extension -eq '.lnk') { try { $target = (New-Object -ComObject WScript.Shell).CreateShortcut($_.FullName).TargetPath } catch {} }
    "$($f.scope)`tfolder`t$(Clean $_.Name)`t$(Clean $target)`t$(On 'StartupFolder' $_.Name)"
  }
}
"#;
        run_script("startup-list.ps1", SCRIPT, &[])
    }

    /// Turns a startup entry on or off through the per-user "approved" flag that Task Manager's Startup tab writes. The kind,
    /// name and wish go in a file, never on a command line; the script only ever writes that one registry value.
    fn startup_set(kind: &str, name: &str, on: bool) -> Result<String, RunError> {
        const SCRIPT: &str = r#"param([string]$Wish)
$l = @(Get-Content -LiteralPath $Wish -Encoding UTF8)
if ($l.Count -lt 3) { Write-Error 'bad request'; exit 2 }
$sub = switch ($l[0]) { 'run' { 'Run' } 'run32' { 'Run32' } 'folder' { 'StartupFolder' } default { '' } }
if (-not $sub -or -not $l[1]) { Write-Error 'bad request'; exit 2 }
$key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\$sub"
if (-not (Test-Path -LiteralPath $key)) { New-Item -Path $key -Force | Out-Null }
if ($l[2] -eq '1') { $bytes = [byte[]](2,0,0,0,0,0,0,0,0,0,0,0) }
else { $bytes = [byte[]](3,0,0,0) + [BitConverter]::GetBytes([int64](Get-Date).ToFileTime()) }
Set-ItemProperty -LiteralPath $key -Name $l[1] -Value $bytes -Type Binary
Write-Output ($l[2])
"#;
        if name.is_empty() || name.contains(['\n', '\r']) || !matches!(kind, "run" | "run32" | "folder") {
            return Err(RunError::Failed("That startup entry can't be changed.".into()));
        }
        let dir = std::env::temp_dir().join("BYTE-upkeep");
        std::fs::create_dir_all(&dir).map_err(|e| RunError::Failed(e.to_string()))?;
        let wish = dir.join(format!("startup-{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&wish, format!("{kind}\n{name}\n{}", u8::from(on))).map_err(|e| RunError::Failed(e.to_string()))?;
        let out = run_script("startup-set.ps1", SCRIPT, &[&wish]);
        let _ = std::fs::remove_file(&wish);
        out
    }

    pub(super) fn known_folder(name: &str) -> Option<std::path::PathBuf> {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        let id = match name {
            "Downloads" => &FOLDERID_Downloads,
            "Desktop" => &FOLDERID_Desktop,
            "Documents" => &FOLDERID_Documents,
            "Pictures" => &FOLDERID_Pictures,
            "Videos" => &FOLDERID_Videos,
            "Music" => &FOLDERID_Music,
            _ => return None,
        };
        // SAFETY: the returned string is copied and then freed exactly once, as the API requires.
        unsafe {
            let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.as_ptr() as *const std::ffi::c_void));
            s.filter(|s| !s.is_empty()).map(std::path::PathBuf::from)
        }
    }

    fn find_files(words: &[String], root: &std::path::Path) -> Result<String, RunError> {
        const SCRIPT: &str = r#"param([string]$Wish)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$l = @(Get-Content -LiteralPath $Wish -Encoding UTF8 | Where-Object { $_ })
if ($l.Count -lt 2) { exit 0 }
$scope = 'file:' + $l[0].Replace("'", "''")
$words = @($l[1..($l.Count - 1)])
$names = ($words | ForEach-Object { "System.FileName LIKE '%$_%'" }) -join ' AND '
$texts = ($words | ForEach-Object { "CONTAINS('""$_*""')" }) -join ' AND '
$exts = (@('.pdf','.doc','.docx','.xls','.xlsx','.ppt','.pptx','.txt','.rtf','.md','.odt','.ods','.odp','.csv','.epub') | ForEach-Object { "System.FileExtension = '$_'" }) -join ' OR '
$conn = New-Object -ComObject ADODB.Connection
$conn.Open("Provider=Search.CollatorDSO;Extended Properties='Application=Windows';")
function Ask($kind, $where, $top) {
  $rs = $conn.Execute("SELECT TOP $top System.ItemPathDisplay FROM SystemIndex WHERE SCOPE='$scope' AND $where ORDER BY System.DateModified DESC")
  while (-not $rs.EOF) { "$kind`t$($rs.Fields.Item(0).Value)"; $rs.MoveNext() }
}
Ask 'name' "($names) AND System.ItemType <> 'Directory'" 300
Ask 'text' "($exts) AND ($texts)" 100
$conn.Close()
"#;
        if words.is_empty() || words.len() > 6 || !words.iter().all(|w| find_word_ok(w)) {
            return Err(RunError::Failed("Those search words can't be used.".into()));
        }
        let root = root.display().to_string();
        if root.contains(['\n', '\r']) {
            return Err(RunError::Failed("That folder can't be searched.".into()));
        }
        let dir = std::env::temp_dir().join("BYTE-upkeep");
        std::fs::create_dir_all(&dir).map_err(|e| RunError::Failed(e.to_string()))?;
        let wish = dir.join(format!("find-{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&wish, format!("{root}\n{}", words.join("\n"))).map_err(|e| RunError::Failed(e.to_string()))?;
        let out = run_script("find-files.ps1", SCRIPT, &[&wish]);
        let _ = std::fs::remove_file(&wish);
        out
    }

    fn programs() -> Result<String, RunError> {
        const SCRIPT: &str = r#"$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
function Clean($s) { ([string]$s) -replace "[\t\r\n|]+", ' ' }
$roots = @(
  @{ hive = 'HKCU';   path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' },
  @{ hive = 'HKLM';   path = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall' },
  @{ hive = 'HKLM32'; path = 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall' }
)
foreach ($r in $roots) {
  foreach ($k in (Get-ChildItem -LiteralPath $r.path)) {
    $p = Get-ItemProperty -LiteralPath $k.PSPath
    if (-not $p.DisplayName) { continue }
    if ($p.SystemComponent -eq 1) { continue }
    if ($p.ParentKeyName -or $p.ParentDisplayName) { continue }
    if (-not ($p.UninstallString -or $p.QuietUninstallString)) { continue }
    if ($k.PSChildName -match '[\\/|"]') { continue }
    "reg`treg|$($r.hive)|$($k.PSChildName)`t$(Clean $p.DisplayName)`t$(Clean $p.DisplayVersion)`t$(Clean $p.Publisher)`t$([int64]$p.EstimatedSize)"
  }
}
foreach ($a in (Get-AppxPackage)) {
  if ($a.IsFramework -or $a.NonRemovable -or $a.SignatureKind -eq 'System' -or -not $a.InstallLocation) { continue }
  "appx`tappx|$($a.PackageFullName)`t$(Clean $a.Name)`t$(Clean $a.Version)`t$(Clean $a.Publisher)`t0"
}
"#;
        run_script("programs.ps1", SCRIPT, &[])
    }

    /// Runs the uninstaller a program registered, found again by its id: the command is read from the registry here, never
    /// taken from the list or from anything a person wrote. A Store app is removed for this user. The id goes to the script in
    /// a file, and only an id `program_id_ok` accepts.
    fn uninstall(id: &str) -> Result<String, RunError> {
        const SCRIPT: &str = r#"param([string]$Wish)
$id = (Get-Content -LiteralPath $Wish -Encoding UTF8 | Select-Object -First 1)
if ($id -match '^appx\|([A-Za-z0-9_.~-]+)$') {
  Remove-AppxPackage -Package $Matches[1] -ErrorAction Stop
  Write-Output 'removed'
  exit 0
}
if ($id -notmatch '^reg\|(HKCU|HKLM|HKLM32)\|([^\\/|"]+)$') { [Console]::Error.WriteLine('That program can''t be uninstalled from here.'); exit 2 }
$root = switch ($Matches[1]) {
  'HKCU' { 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' }
  'HKLM' { 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall' }
  default { 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall' }
}
$key = Join-Path $root $Matches[2]
$p = Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
if (-not $p -or -not $p.DisplayName -or $p.SystemComponent -eq 1) { [Console]::Error.WriteLine('That program isn''t installed any more.'); exit 3 }
$cmd = if ($p.UninstallString) { [string]$p.UninstallString } else { [string]$p.QuietUninstallString }
# Windows Installer entries often say /I (change); uninstalling is /X, as Settings does.
$cmd = $cmd -replace '(?i)^(\s*"?(?:[^"]*\\)?msiexec(?:\.exe)?"?\s+)/I', '$1/X'
if ($cmd -match '^\s*"([^"]+)"\s*(.*)$') { $exe = $Matches[1]; $rest = $Matches[2] }
elseif ($cmd -match '^\s*(.+?\.exe)(?:\s+(.*))?$') { $exe = $Matches[1]; $rest = $Matches[2] }
else { [Console]::Error.WriteLine('This program''s uninstaller can''t be started from here.'); exit 4 }
$present = (Test-Path -LiteralPath $exe -PathType Leaf) -or [bool](Get-Command $exe -CommandType Application -ErrorAction SilentlyContinue)
if (-not $present) { [Console]::Error.WriteLine('The uninstaller isn''t where the program said it was.'); exit 5 }
if ($rest) { Start-Process -FilePath $exe -ArgumentList $rest } else { Start-Process -FilePath $exe }
# A quick uninstaller finishes at once; a wizard waits for the person.
for ($i = 0; $i -lt 30; $i++) {
  Start-Sleep -Milliseconds 500
  if (-not (Test-Path -LiteralPath $key)) { Write-Output 'removed'; exit 0 }
}
Write-Output 'started'
"#;
        if !program_id_ok(id) {
            return Err(RunError::Failed("That program can't be uninstalled from here.".into()));
        }
        let dir = std::env::temp_dir().join("BYTE-upkeep");
        std::fs::create_dir_all(&dir).map_err(|e| RunError::Failed(e.to_string()))?;
        let wish = dir.join(format!("uninstall-{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&wish, id).map_err(|e| RunError::Failed(e.to_string()))?;
        let out = run_script("uninstall.ps1", SCRIPT, &[&wish]);
        let _ = std::fs::remove_file(&wish);
        out
    }

    fn security() -> Result<String, RunError> {
        const SCRIPT: &str = r#"$ErrorActionPreference = 'SilentlyContinue'
try { $mp = Get-MpComputerStatus; if ($mp) { "defender=$($mp.AntivirusEnabled);$($mp.RealTimeProtectionEnabled);$($mp.AMRunningMode);$($mp.AntivirusSignatureAge)" } } catch {}
try { $fw = Get-NetFirewallProfile | ForEach-Object { "$($_.Name):$($_.Enabled)" }; if ($fw) { "firewall=" + ($fw -join ',') } } catch {}
try { $b = (New-Object -ComObject Shell.Application).NameSpace($env:SystemDrive + '').Self.ExtendedProperty('System.Volume.BitLockerProtection'); if ($null -ne $b) { "bitlocker=$b" } } catch {}
try { $r = (New-Object -ComObject Microsoft.Update.AutoUpdate).Results; if ($r.LastInstallationSuccessDate) { "update=" + $r.LastInstallationSuccessDate.ToString('yyyy-MM-dd') } } catch {}
if (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired') { "reboot=1" } else { "reboot=0" }
try { $d = (Get-CimInstance -Namespace root\wmi -ClassName BatteryStaticData).DesignedCapacity; $f = (Get-CimInstance -Namespace root\wmi -ClassName BatteryFullChargedCapacity).FullChargedCapacity; if ($d -and $f) { "battery=$d;$f" } } catch {}
try { $c = (Get-CimInstance -Namespace root\wmi -ClassName BatteryCycleCount).CycleCount; if ($c) { "cycles=$c" } } catch {}
"#;
        run_script("security.ps1", SCRIPT, &[])
    }

    /// Puts files back from the Recycle Bin with a fixed script: the paths go in a file, one per line, and the script
    /// is handed that file's name, so no path is ever part of a command line. The Recycle Bin has no way to be asked
    /// "restore this" from Rust, but the shell can: each item knows where it was deleted from, and "undelete" is the verb of
    /// its Restore command.
    fn restore(paths: &[std::path::PathBuf]) -> Result<String, RunError> {
        const SCRIPT: &str = r#"param([string]$List)
$wanted = @(Get-Content -LiteralPath $List -Encoding UTF8 | Where-Object { $_ })
$shell = New-Object -ComObject Shell.Application
$bin = $shell.NameSpace(10)
$items = @($bin.Items())
$done = 0
foreach ($w in $wanted) {
  $dir = Split-Path -Parent $w
  $leaf = Split-Path -Leaf $w
  $stem = [System.IO.Path]::GetFileNameWithoutExtension($leaf)
  $hit = $null
  foreach ($i in $items) {
    if (($bin.GetDetailsOf($i, 1) -ieq $dir) -and (($i.Name -ieq $leaf) -or ($i.Name -ieq $stem))) { $hit = $i }
  }
  if ($hit) { $hit.InvokeVerb('undelete'); $done++ }
}
Write-Output $done
"#;
        let dir = std::env::temp_dir().join("BYTE-upkeep");
        std::fs::create_dir_all(&dir).map_err(|e| RunError::Failed(e.to_string()))?;
        let list = dir.join(format!("restore-{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&list, paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")).map_err(|e| RunError::Failed(e.to_string()))?;
        let out = run_script("restore.ps1", SCRIPT, &[&list]);
        let _ = std::fs::remove_file(&list);
        out
    }

    /// File Explorer with the file selected. `/select,` takes the path after a comma and wants it quoted as one piece, which
    /// the usual argument quoting would turn into something else, so the argument is written out exactly.
    fn reveal(path: &std::path::Path) -> Result<String, RunError> {
        use std::os::windows::process::CommandExt;
        let text = path.display().to_string();
        if text.contains('"') {
            return Err(RunError::Failed("That path can't be shown.".into()));
        }
        std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{text}\"")).spawn().map_err(|e| RunError::Failed(e.to_string()))?;
        Ok(String::new())
    }

    // ---- sound

    pub(super) fn endpoint() -> Result<IAudioEndpointVolume, RunError> {
        ensure_com();
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(fail)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).map_err(|_| RunError::Missing)?;
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).map_err(fail)
        }
    }

    fn volume(percent: u8) -> Result<String, RunError> {
        let v = endpoint()?;
        unsafe {
            v.SetMasterVolumeLevelScalar(f32::from(percent.min(100)) / 100.0, std::ptr::null()).map_err(fail)?;
            if percent > 0 {
                v.SetMute(false, std::ptr::null()).map_err(fail)?;
            }
            let now = v.GetMasterVolumeLevelScalar().map_err(fail)?;
            Ok(((now * 100.0).round() as u32).to_string())
        }
    }

    fn mute(on: bool) -> Result<String, RunError> {
        let v = endpoint()?;
        unsafe { v.SetMute(on, std::ptr::null()) }.map_err(fail)?;
        Ok(String::new())
    }

    // ---- dark mode

    const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");

    pub(super) fn apps_use_light() -> Option<bool> {
        unsafe {
            let mut key = HKEY::default();
            if RegOpenKeyExW(HKEY_CURRENT_USER, PERSONALIZE, None, KEY_READ, &mut key).is_err() {
                return None;
            }
            let (mut value, mut size) = (1u32, 4u32);
            let ok = RegQueryValueExW(key, w!("AppsUseLightTheme"), None, None, Some(&mut value as *mut u32 as *mut u8), Some(&mut size)).is_ok();
            let _ = RegCloseKey(key);
            ok.then_some(value != 0)
        }
    }

    fn set_light(light: bool) -> Result<(), RunError> {
        unsafe {
            let mut key = HKEY::default();
            RegOpenKeyExW(HKEY_CURRENT_USER, PERSONALIZE, None, KEY_SET_VALUE, &mut key).ok().map_err(fail)?;
            let data = u32::from(light).to_le_bytes();
            let mut result = Ok(());
            for name in [w!("AppsUseLightTheme"), w!("SystemUsesLightTheme")] {
                if let Err(e) = RegSetValueExW(key, name, None, REG_DWORD, Some(&data)).ok() {
                    result = Err(fail(e));
                }
            }
            let _ = RegCloseKey(key);
            result?;
            // Tells running programs the colors changed, so Windows and open apps follow without a sign-out.
            let mut out = 0usize;
            SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(w!("ImmersiveColorSet").as_ptr() as isize), SMTO_ABORTIFHUNG, 2000, Some(&mut out));
        }
        Ok(())
    }

    fn dark_mode(s: Switch) -> Result<String, RunError> {
        let dark = match s {
            Switch::On => true,
            Switch::Off => false,
            Switch::Toggle => apps_use_light().unwrap_or(true),
        };
        set_light(!dark)?;
        Ok(dark.to_string())
    }

    // ---- Wi-Fi

    fn wifi(on: bool) -> Result<String, RunError> {
        ensure_winrt();
        let access = Radio::RequestAccessAsync().and_then(|o| o.join()).map_err(fail)?;
        if access != RadioAccessStatus::Allowed {
            return Err(RunError::NotAllowed);
        }
        let radios = Radio::GetRadiosAsync().and_then(|o| o.join()).map_err(fail)?;
        let mut found = false;
        for radio in radios {
            if radio.Kind().map_err(fail)? == RadioKind::WiFi {
                found = true;
                let state = if on { RadioState::On } else { RadioState::Off };
                if radio.SetStateAsync(state).and_then(|o| o.join()).map_err(fail)? != RadioAccessStatus::Allowed {
                    return Err(RunError::NotAllowed);
                }
            }
        }
        if found {
            Ok(String::new())
        } else {
            Err(RunError::Missing)
        }
    }

    // ---- opening things

    fn open(target: &str) -> Result<(), RunError> {
        let target = HSTRING::from(target);
        // ShellExecute answers with a number above 32 when it worked, and an error code up to 32 when it didn't.
        let r = unsafe { ShellExecuteW(None, w!("open"), &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        if r.0 as isize > 32 {
            Ok(())
        } else {
            Err(RunError::Missing)
        }
    }

    fn add_event(title: &str, start: NaiveDateTime, minutes: u32) -> Result<String, RunError> {
        let dir = std::env::temp_dir().join("BYTE-events");
        std::fs::create_dir_all(&dir).map_err(|e| RunError::Failed(e.to_string()))?;
        let uid = uuid::Uuid::new_v4().simple().to_string();
        let path = dir.join(format!("{uid}.ics"));
        std::fs::write(&path, ics(title, start, minutes, &uid, chrono::Local::now().naive_local())).map_err(|e| RunError::Failed(e.to_string()))?;
        open(&path.to_string_lossy())?;
        Ok(String::new())
    }

    fn spotify_installed() -> bool {
        unsafe {
            let mut key = HKEY::default();
            let found = RegOpenKeyExW(HKEY_CLASSES_ROOT, w!("spotify"), None, KEY_READ, &mut key).is_ok();
            if found {
                let _ = RegCloseKey(key);
            }
            found
        }
    }

    fn play_music(query: &str) -> Result<String, RunError> {
        if spotify_installed() {
            open(&spotify_uri(query))?;
            Ok("spotify".into())
        } else {
            open(&music_web_uri(query))?;
            Ok("web".into())
        }
    }

    // ---- the media session Windows keeps for Spotify, the browser and the rest

    fn current() -> Result<Option<Session>, RunError> {
        ensure_winrt();
        let manager = Manager::RequestAsync().and_then(|o| o.join()).map_err(fail)?;
        Ok(manager.GetCurrentSession().ok())
    }

    /// `song<US>artist`, or `stopped` when the session has no song.
    fn describe(session: &Session) -> Result<String, RunError> {
        let props = session.TryGetMediaPropertiesAsync().and_then(|o| o.join()).map_err(fail)?;
        let title = props.Title().map(|t| t.to_string()).unwrap_or_default();
        let artist = props.Artist().map(|t| t.to_string()).unwrap_or_default();
        if title.trim().is_empty() {
            return Ok("stopped".into());
        }
        Ok(format!("{title}{US}{artist}"))
    }

    fn now_playing() -> Result<String, RunError> {
        let Some(session) = current()? else { return Ok("not running".into()) };
        let status = session.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).unwrap_or(Playback::Stopped);
        if status != Playback::Playing {
            return Ok("stopped".into());
        }
        describe(&session)
    }

    /// Runs one media command (play, pause, next, previous) on the current session; `act` waits for the answer.
    fn control<F>(act: F) -> Result<String, RunError>
    where
        F: FnOnce(&Session) -> windows::core::Result<bool>,
    {
        let Some(session) = current()? else { return Ok("not running".into()) };
        let done = act(&session).map_err(fail)?;
        if !done {
            return Ok("stopped".into());
        }
        // The player takes a moment to move on to the next song.
        std::thread::sleep(std::time::Duration::from_millis(500));
        describe(&session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 9).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn the_system_controls_have_a_pc_operation() {
        assert_eq!(op_for(&Action::DarkMode(Switch::On)), Some(WinOp::DarkMode(Switch::On)));
        assert_eq!(op_for(&Action::Volume(30)), Some(WinOp::Volume(30)));
        assert_eq!(op_for(&Action::Mute(true)), Some(WinOp::Mute(true)));
        assert_eq!(op_for(&Action::Wifi(false)), Some(WinOp::Wifi(false)));
        assert_eq!(op_for(&Action::SleepDisplay), Some(WinOp::SleepDisplay));
        assert_eq!(op_for(&Action::MusicPause), Some(WinOp::MediaPause));
        assert_eq!(op_for(&Action::NowPlaying), Some(WinOp::NowPlaying));
    }

    #[test]
    fn playing_music_resumes_or_searches() {
        assert_eq!(op_for(&Action::MusicPlay { query: "  ".into() }), Some(WinOp::MediaPlay));
        assert_eq!(op_for(&Action::MusicPlay { query: " jazz ".into() }), Some(WinOp::PlayMusic("jazz".into())));
    }

    #[test]
    fn what_a_pc_cannot_do_is_said_not_attempted() {
        for a in [
            Action::ShortcutsList,
            Action::ShortcutRun { name: "x".into(), input: String::new() },
            Action::SafariTab,
            Action::MessageSend { to: "1".into(), name: String::new(), body: "hi".into(), chat: String::new() },
            Action::EventsList { days: 1 },
            Action::MailList { query: String::new(), days: 1 },
        ] {
            assert!(op_for(&a).is_none(), "{a:?}");
            assert!(unsupported(&a).is_some(), "{a:?}");
        }
        assert!(unsupported(&Action::Volume(10)).is_none());
    }

    #[test]
    fn settings_pages_are_found_by_the_words_people_use() {
        assert_eq!(settings_uri("bluetooth"), "ms-settings:bluetooth");
        assert_eq!(settings_uri(pane_for("open windows update settings").unwrap()), "ms-settings:windowsupdate");
        assert_eq!(settings_uri(pane_for("open the night light settings").unwrap()), "ms-settings:nightlight");
        assert_eq!(settings_uri(pane_for("open wi-fi settings").unwrap()), "ms-settings:network-wifi");
        assert_eq!(settings_uri(pane_for("open default apps settings").unwrap()), "ms-settings:defaultapps");
        assert_eq!(pane_label("sound"), "Sound");
        assert_eq!(pane_for("open settings"), None);
        assert_eq!(settings_uri(""), "ms-settings:");
    }

    #[test]
    fn every_settings_page_is_an_ms_settings_address() {
        for (key, label, page) in PANES {
            assert!(!key.is_empty() && !label.is_empty() && !page.is_empty() && !page.contains(':'), "{key}");
            assert!(settings_uri(key).starts_with("ms-settings:"), "{key}");
        }
    }

    #[test]
    fn an_email_link_keeps_every_character_of_the_draft() {
        let link = mailto("sam@example.com", "Friday & lunch?", "Hi Sam,\nSee you at 12:30.\n\nBest");
        assert_eq!(link, "mailto:sam%40example.com?subject=Friday%20%26%20lunch%3F&body=Hi%20Sam%2C%0D%0ASee%20you%20at%2012%3A30.%0D%0A%0D%0ABest");
        // Nothing in a subject or body can end the link or start another parameter.
        let tricky = mailto("a@b.c", "x&bcc=evil@z.z", "y#z\"q");
        assert_eq!(tricky.matches('&').count(), 1);
        assert!(!tricky.contains('#') && !tricky.contains('"'));
        assert!(mailto("a@b.c", "s", &"x".repeat(5000)).len() < 2000 + 40);
    }

    #[test]
    fn a_calendar_file_is_one_event_in_local_time() {
        let text = ics("Lunch, with Sam; noon", at(12, 0), 90, "abc123", at(9, 5));
        assert!(text.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
        assert!(text.contains("DTSTART:20261009T120000\r\n"));
        assert!(text.contains("DTEND:20261009T133000\r\n"));
        assert!(text.contains("DTSTAMP:20261009T090500\r\n"));
        assert!(text.contains("SUMMARY:Lunch\\, with Sam\\; noon\r\n"));
        assert!(text.contains("UID:abc123@byte\r\n"));
        assert!(text.ends_with("END:VEVENT\r\nEND:VCALENDAR\r\n"));
        assert_eq!(text.matches("BEGIN:VEVENT").count(), 1);
    }

    #[test]
    fn a_long_event_crosses_midnight() {
        let text = ics("Overnight", at(23, 0), 180, "u", at(9, 0));
        assert!(text.contains("DTEND:20261010T020000\r\n"));
    }

    #[test]
    fn a_mail_draft_and_an_event_open_through_windows() {
        let draft = Action::MailDraft { to: "sam@x.org".into(), name: "Sam".into(), subject: "Hi".into(), body: "Yo".into() };
        assert_eq!(op_for(&draft), Some(WinOp::Open("mailto:sam%40x.org?subject=Hi&body=Yo".into())));
        let event = Action::EventAdd { title: "Lunch".into(), start: at(12, 0), minutes: 60, calendar: String::new() };
        assert_eq!(op_for(&event), Some(WinOp::AddEvent { title: "Lunch".into(), start: at(12, 0), minutes: 60 }));
    }

    #[test]
    fn music_searches_are_encoded() {
        assert_eq!(spotify_uri("lo-fi beats"), "spotify:search:lo-fi%20beats");
        assert_eq!(music_web_uri("rock & roll"), "https://music.youtube.com/search?q=rock%20%26%20roll");
    }

    #[test]
    fn off_windows_the_runner_says_the_tool_is_missing() {
        if !cfg!(windows) {
            assert_eq!(run_op(&WinOp::Volume(10)), Err(RunError::Missing));
        }
    }

    #[test]
    fn cards_say_what_happened_in_a_pcs_words() {
        let (d, n) = read_out(&Action::OpenSettings { pane: "bluetooth".into() }, "").unwrap();
        assert_eq!(d, "Bluetooth & devices settings");
        assert!(n.contains("Windows Settings"));
        let song = format!("Blue in Green{}Miles Davis", crate::macctl::US);
        let (d, n) = read_out(&Action::NowPlaying, &song).unwrap();
        assert!(d.contains("Blue in Green") && n.contains("by Miles Davis"), "{d} / {n}");
        let (d, n) = read_out(&Action::MusicPause, &song).unwrap();
        assert!(d.starts_with("Paused") && n.starts_with("Done: paused"), "{d} / {n}");
        assert_eq!(read_out(&Action::NowPlaying, "not running").unwrap().0, "Nothing is playing");
        let (d, n) = read_out(&Action::MusicPlay { query: "jazz".into() }, "spotify").unwrap();
        assert!(d.contains("Spotify") && n.contains("doesn't start playing by itself"));
        assert!(read_out(&Action::DarkMode(Switch::On), "true").is_none());
    }

    #[test]
    fn the_approval_card_does_not_promise_what_a_pc_cannot_do() {
        let draft = Action::MailDraft { to: "a@b.c".into(), name: String::new(), subject: "s".into(), body: "b".into() };
        let mut f = draft.fields();
        pc_fields(&draft, &mut f);
        let note = f.iter().find(|(l, _)| l == "Note").map(|(_, v)| v.as_str()).unwrap();
        assert!(note.starts_with("Your mail app") && !note.contains("Mail opens"));
        assert_eq!(f.iter().filter(|(l, _)| l == "Note").count(), 1);
    }
}

/// Real-PC checks. They change the sound, the colors and open a Settings page, then put everything back.
/// Run from a desktop session (not a service or SSH session): `cargo test --lib live_pc -- --ignored --test-threads=1`.
#[cfg(all(test, windows))]
mod live {
    use super::*;

    #[test]
    #[ignore = "changes the volume on the real PC, then restores it"]
    fn live_pc_volume_and_mute_round_trip() {
        let v = win::endpoint().expect("an audio output device");
        let (orig, was_muted) = unsafe { (v.GetMasterVolumeLevelScalar().unwrap(), v.GetMute().unwrap().as_bool()) };
        let set = |p: u8| run_op(&WinOp::Volume(p)).unwrap_or_else(|e| panic!("volume {p}: {e:?}"));
        assert_eq!(set(37), "37");
        assert_eq!(set(12), "12");
        // A volume above zero also unmutes, like pressing the volume key.
        run_op(&WinOp::Mute(true)).unwrap();
        assert!(unsafe { v.GetMute().unwrap().as_bool() });
        assert_eq!(set(20), "20");
        assert!(!unsafe { v.GetMute().unwrap().as_bool() });
        run_op(&WinOp::Mute(true)).unwrap();
        run_op(&WinOp::Mute(false)).unwrap();
        assert!(!unsafe { v.GetMute().unwrap().as_bool() });
        unsafe {
            v.SetMasterVolumeLevelScalar(orig, std::ptr::null()).unwrap();
            v.SetMute(was_muted, std::ptr::null()).unwrap();
        }
    }

    #[test]
    #[ignore = "switches the PC's colors for a moment, then restores them"]
    fn live_pc_dark_mode_round_trip() {
        let was_light = win::apps_use_light().expect("the colors setting");
        assert_eq!(run_op(&WinOp::DarkMode(Switch::On)).unwrap(), "true");
        assert_eq!(win::apps_use_light(), Some(false));
        assert_eq!(run_op(&WinOp::DarkMode(Switch::Off)).unwrap(), "false");
        assert_eq!(win::apps_use_light(), Some(true));
        // Toggle flips whatever it is now.
        assert_eq!(run_op(&WinOp::DarkMode(Switch::Toggle)).unwrap(), "true");
        assert_eq!(run_op(&WinOp::DarkMode(Switch::Toggle)).unwrap(), "false");
        run_op(&WinOp::DarkMode(if was_light { Switch::Off } else { Switch::On })).unwrap();
        assert_eq!(win::apps_use_light(), Some(was_light));
    }

    #[test]
    #[ignore = "reads the real startup list, then turns an entry the test adds itself off and on"]
    fn live_pc_startup_list_and_round_trip() {
        let list = || run_op(&WinOp::StartupList).expect("the startup list can be read");
        let before = list();
        println!("--- startup entries on this PC:\n{before}");
        for l in before.lines() {
            assert_eq!(l.split('\t').count(), 5, "five fields: {l:?}");
        }
        // An entry of the test's own in the user's Run key, removed again whatever happens.
        const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        const APPROVED: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
        const NAME: &str = "BYTE live test entry é";
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for key in [RUN, APPROVED] {
                    let _ = std::process::Command::new("reg").args(["delete", key, "/v", NAME, "/f"]).output();
                }
            }
        }
        let _cleanup = Cleanup;
        let added = std::process::Command::new("reg").args(["add", RUN, "/v", NAME, "/t", "REG_SZ", "/d", r#""C:\Windows\System32\cmd.exe" /c exit"#, "/f"]).output().unwrap();
        assert!(added.status.success());
        let mine = || list().lines().find(|l| l.contains("BYTE live test entry")).map(str::to_string);
        let line = mine().expect("the new entry is listed");
        println!("new entry: {line:?}");
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!((f[0], f[1], f[2], f[4]), ("user", "run", NAME, "1"), "{line:?}");
        assert!(f[3].contains("cmd.exe"));
        let set = |on| run_op(&WinOp::StartupSet { kind: "run".into(), name: NAME.into(), on }).unwrap_or_else(|e| panic!("set {on}: {e:?}"));
        assert_eq!(set(false), "0");
        assert!(mine().unwrap().ends_with("\t0"), "{:?}", mine());
        assert_eq!(set(true), "1");
        assert!(mine().unwrap().ends_with("\t1"), "{:?}", mine());
        // Anything but the three kinds, or an empty name, is refused before PowerShell is asked.
        assert!(run_op(&WinOp::StartupSet { kind: "task".into(), name: "x".into(), on: false }).is_err());
        assert!(run_op(&WinOp::StartupSet { kind: "run".into(), name: "".into(), on: false }).is_err());
        assert!(run_op(&WinOp::StartupSet { kind: "run".into(), name: "a\nb".into(), on: false }).is_err());
    }

    #[test]
    #[ignore = "asks Windows Search and the shell for the real folders"]
    fn live_pc_search_and_known_folders() {
        for name in ["Downloads", "Desktop", "Documents", "Pictures", "Videos", "Music"] {
            let p = known_folder(name).unwrap_or_else(|| panic!("{name}"));
            println!("{name}: {} (exists: {})", p.display(), p.is_dir());
            assert!(p.is_dir(), "{name}");
        }
        assert_eq!(known_folder("Windows"), None);
        let root = known_folder("Desktop").unwrap().parent().unwrap().to_path_buf();
        // The test folder on the owner's desktop is a file that is always there; a name nobody has finds nothing.
        let hits = run_op(&WinOp::FindFiles { words: vec!["zzqxj-nothing-like-this".into()], root: root.clone() }).expect("search");
        assert_eq!(hits, "", "{hits:?}");
        let hits = run_op(&WinOp::FindFiles { words: vec!["readme".into()], root }).expect("search");
        println!("{} lines for readme; first: {:?}", hits.lines().count(), hits.lines().next());
        for l in hits.lines() {
            let (kind, path) = l.split_once('\t').expect("kind and path");
            assert!(matches!(kind, "name" | "text") && !path.is_empty(), "{l:?}");
        }
        assert!(run_op(&WinOp::FindFiles { words: vec!["a'b".into()], root: std::env::temp_dir() }).is_err());
        assert!(run_op(&WinOp::FindFiles { words: vec![], root: std::env::temp_dir() }).is_err());
    }

    #[test]
    #[ignore = "reads the real list of installed programs"]
    fn live_pc_programs_are_listed() {
        let out = run_op(&WinOp::Programs).expect("the programs can be listed");
        let rows: Vec<Vec<&str>> = out.lines().map(|l| l.split('\t').collect()).collect();
        println!("{} programs; the first 12:", rows.len());
        for r in rows.iter().take(12) {
            println!("  {r:?}");
        }
        assert!(rows.len() >= 5, "a PC has programs");
        for r in &rows {
            assert_eq!(r.len(), 6, "six fields: {r:?}");
            assert!(program_id_ok(r[1]), "an id the uninstall accepts: {:?}", r[1]);
            assert!(!r[2].is_empty());
        }
    }

    #[test]
    #[ignore = "adds a program of its own to the registry, uninstalls it through the flow's operation, and makes sure it is gone"]
    fn live_pc_uninstall_runs_the_registered_uninstaller() {
        const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\BYTE live test program";
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::process::Command::new("reg").args(["delete", KEY, "/f"]).output();
            }
        }
        let _cleanup = Cleanup;
        let reg = |args: &[&str]| assert!(std::process::Command::new("reg").args(args).output().unwrap().status.success(), "reg {args:?}");
        reg(&["add", KEY, "/v", "DisplayName", "/t", "REG_SZ", "/d", "BYTE live test program", "/f"]);
        reg(&["add", KEY, "/v", "DisplayVersion", "/t", "REG_SZ", "/d", "1.2.3", "/f"]);
        // The "uninstaller" is a command that removes the program's own entry, like a real one at the end.
        let command = format!(r#""C:\Windows\System32\cmd.exe" /c reg delete "{KEY}" /f"#);
        reg(&["add", KEY, "/v", "UninstallString", "/t", "REG_SZ", "/d", &command, "/f"]);
        let listed = run_op(&WinOp::Programs).unwrap();
        let line = listed.lines().find(|l| l.contains("BYTE live test program")).expect("the program is listed");
        println!("listed as {line:?}");
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!((f[0], f[1], f[3]), ("reg", "reg|HKCU|BYTE live test program", "1.2.3"), "{line:?}");
        assert_eq!(run_op(&WinOp::Uninstall { id: f[1].into() }).unwrap(), "removed");
        assert!(!run_op(&WinOp::Programs).unwrap().contains("BYTE live test program"), "it is gone from the list");
        // Gone is gone: asking again is an error, not a success.
        assert!(run_op(&WinOp::Uninstall { id: f[1].into() }).is_err());
        // Only ids the list prints are accepted.
        for bad in ["", "reg|HKCU|a\\b", "reg|HKCR|x", "reg|HKCU|", "appx|a b", "cmd /c calc", "reg|HKLM|..\\..\\x", "appx|x|y"] {
            assert!(run_op(&WinOp::Uninstall { id: bad.into() }).is_err(), "{bad:?}");
        }
    }

    #[test]
    #[ignore = "opens Notepad on the real PC for a moment and asks it to close"]
    fn live_pc_quit_asks_a_programs_windows_to_close() {
        use sysinfo::{ProcessesToUpdate, System};
        /// Notepad's processes now: (pid, has a visible window).
        fn notepads() -> Vec<(u32, bool)> {
            let mut sys = System::new();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            let windowed = win::window_pids();
            sys.processes()
                .iter()
                .filter(|(_, p)| p.name().to_string_lossy().to_lowercase().trim_end_matches(".exe") == "notepad")
                .map(|(pid, _)| (pid.as_u32(), windowed.contains(&pid.as_u32())))
                .collect()
        }
        /// Whatever happens, the Notepads this test started do not stay open on the person's desktop.
        struct Cleanup {
            before: Vec<u32>,
            dir: std::path::PathBuf,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let mut sys = System::new();
                sys.refresh_processes(ProcessesToUpdate::All, true);
                for (pid, p) in sys.processes() {
                    if p.name().to_string_lossy().to_lowercase().trim_end_matches(".exe") == "notepad" && !self.before.contains(&pid.as_u32()) {
                        p.kill();
                    }
                }
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }
        let dir = std::env::temp_dir().join(format!("byte-quit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("quit-me.txt");
        std::fs::write(&file, "BYTE live test").unwrap();
        let _cleanup = Cleanup { before: notepads().into_iter().map(|(pid, _)| pid).collect(), dir };
        assert!(_cleanup.before.is_empty(), "Notepad is already open on this PC; close it first so the test only touches its own");
        // No inherited output handles: a child holding the test's pipe would keep the whole run waiting for it.
        use std::process::Stdio;
        std::process::Command::new("notepad.exe").arg(&file).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("notepad starts");
        let up = (0..40).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            notepads().iter().any(|(_, w)| *w)
        });
        assert!(up, "Notepad should have a window; its processes: {:?}", notepads());
        let asked: u32 = run_op(&WinOp::Quit("Notepad.exe".into())).expect("quit").trim().parse().unwrap();
        println!("asked {asked} window(s) to close");
        assert!(asked >= 1);
        let gone = (0..40).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            notepads().is_empty()
        });
        assert!(gone, "Notepad should have closed itself when asked; still: {:?}", notepads());
        // Nothing running by that name is not an error, just nothing to close.
        assert_eq!(run_op(&WinOp::Quit("no-such-program-byte".into())).unwrap(), "0");
    }

    #[test]
    #[ignore = "reads the media session of the real PC"]
    fn live_pc_now_playing_answers_whatever_is_playing() {
        let out = run_op(&WinOp::NowPlaying).expect("the media session can be read");
        println!("now playing says: {out:?}");
        assert!(out == "not running" || out == "stopped" || out.contains(crate::macctl::US), "{out:?}");
        // Pausing with nothing playing is harmless and says so.
        let paused = run_op(&WinOp::MediaPause).expect("pause");
        println!("pause says: {paused:?}");
    }

    /// Edge plays a looping tone in a throwaway profile (so nothing of the person's is touched), at a low volume;
    /// BYTE reads what is playing, pauses it, resumes it, then the profile's Edge is closed and the volume restored.
    #[test]
    #[ignore = "plays a quiet tone on the real PC through Edge, then stops it"]
    fn live_pc_media_session_controls_a_real_player() {
        use std::process::Command as P;
        let v = win::endpoint().expect("an audio output device");
        let (orig, was_muted) = unsafe { (v.GetMasterVolumeLevelScalar().unwrap(), v.GetMute().unwrap().as_bool()) };
        run_op(&WinOp::Volume(6)).unwrap();
        let dir = std::env::temp_dir().join("byte-live-media");
        std::fs::create_dir_all(&dir).unwrap();
        let page = dir.join("tone.html");
        std::fs::write(&page, "<title>BYTE test tone</title><audio src=\"file:///C:/Windows/Media/Ring05.wav\" loop autoplay></audio>").unwrap();
        let profile = dir.join("edge-profile");
        let _ = P::new("cmd")
            .args(["/C", "start", "", "msedge", "--autoplay-policy=no-user-gesture-required", "--no-first-run", "--new-window"])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("file:///{}", page.to_string_lossy().replace('\\', "/")))
            .status();
        let mut now = String::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            now = run_op(&WinOp::NowPlaying).unwrap();
            if now.contains(crate::macctl::US) {
                break;
            }
        }
        println!("now playing: {now:?}");
        let playing = now.contains(crate::macctl::US);
        let paused = if playing { run_op(&WinOp::MediaPause).unwrap() } else { String::new() };
        std::thread::sleep(std::time::Duration::from_millis(800));
        let after_pause = run_op(&WinOp::NowPlaying).unwrap();
        let resumed = if playing { run_op(&WinOp::MediaPlay).unwrap() } else { String::new() };
        std::thread::sleep(std::time::Duration::from_millis(800));
        let after_play = run_op(&WinOp::NowPlaying).unwrap();
        println!("pause said {paused:?}; then {after_pause:?}; play said {resumed:?}; then {after_play:?}");
        // Close only the Edge that was opened with the throwaway profile, and put the sound back.
        let _ = P::new("powershell")
            .args(["-NoProfile", "-Command", "Get-CimInstance Win32_Process -Filter \"Name='msedge.exe'\" | Where-Object { $_.CommandLine -like '*byte-live-media*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }"])
            .status();
        unsafe {
            v.SetMasterVolumeLevelScalar(orig, std::ptr::null()).unwrap();
            v.SetMute(was_muted, std::ptr::null()).unwrap();
        }
        assert!(playing, "Edge's tone never showed up as a media session: {now:?}");
        assert!(paused.contains(crate::macctl::US), "pause answers with the song: {paused:?}");
        assert_eq!(after_pause, "stopped", "paused means not playing");
        assert!(after_play.contains(crate::macctl::US), "playing again: {after_play:?}");
    }

    #[test]
    #[ignore = "puts a throwaway file in the real Recycle Bin and takes it back out"]
    fn live_pc_a_file_goes_to_the_recycle_bin_and_comes_back() {
        let dir = std::env::temp_dir().join("byte-live-recycle");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("byte-live-{}.msi", uuid::Uuid::new_v4().simple()));
        std::fs::write(&file, "throwaway contents").unwrap();
        let out = run_op(&WinOp::Recycle(vec![file.clone(), dir.join("not-there.msi")])).expect("the Recycle Bin answers");
        let lines: Vec<&str> = out.lines().collect();
        println!("recycle said {lines:?}");
        assert_eq!(lines[0], "ok");
        assert!(lines[1].starts_with('!'), "a file that is not there is reported, not hidden: {lines:?}");
        assert!(!file.exists(), "the file left its folder");
        let back = run_op(&WinOp::Restore(vec![file.clone()])).expect("restore runs");
        println!("restore said {back:?}");
        assert_eq!(back, "1");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "throwaway contents", "it is back, unchanged");
        // Put it in the bin for good housekeeping and leave nothing in the temp folder.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "reads memory, processes, security settings and battery of the real PC"]
    fn live_pc_health_readers_answer() {
        for (name, op) in [("snapshot", WinOp::Snapshot), ("processes", WinOp::Processes), ("security", WinOp::Security)] {
            let t = std::time::Instant::now();
            let out = run_op(&op).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            println!("--- {name} ({:.1}s)\n{out}", t.elapsed().as_secs_f32());
        }
        // Asking a program that is not running to close does nothing, and says so.
        assert_eq!(run_op(&WinOp::Quit("byte-no-such-program.exe".into())).unwrap(), "0");
    }

    #[test]
    #[ignore = "asks Windows for the Wi-Fi radio and turns it ON (never off)"]
    fn live_pc_wifi_on_finds_the_radio_or_says_there_is_none() {
        match run_op(&WinOp::Wifi(true)) {
            Ok(_) => println!("a Wi-Fi radio exists and is on"),
            Err(RunError::Missing) => println!("this PC has no Wi-Fi radio"),
            Err(e) => panic!("{e:?}"),
        }
    }

    #[test]
    #[ignore = "opens the Sound page of Windows Settings"]
    fn live_pc_settings_page_opens() {
        run_op(&WinOp::Open(settings_uri("sound"))).expect("ShellExecute opened ms-settings:sound");
        std::thread::sleep(std::time::Duration::from_secs(3));
        let found = std::process::Command::new("tasklist").args(["/FI", "IMAGENAME eq SystemSettings.exe"]).output().unwrap();
        let text = String::from_utf8_lossy(&found.stdout).to_string();
        println!("{text}");
        assert!(text.contains("SystemSettings.exe"), "Settings did not start");
        let _ = std::process::Command::new("taskkill").args(["/IM", "SystemSettings.exe", "/F"]).output();
    }
}
