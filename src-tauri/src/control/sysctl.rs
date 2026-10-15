//! Fixed PC controls that pcctl.rs lacked: lock, sleep, screenshot, brightness, microphone mute, media keys, and the power
//! actions the executor uses. Each is one function; the words a person says are only ever a number or a choice from a list.
//! Which of them ask first is `risk`.

use super::actions::{parse_combo, PowerKind, Risk};
use super::input::Input;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysOp {
    Lock,
    Sleep,
    /// Win+PrintScreen: Windows saves a picture in Pictures, Screenshots.
    Screenshot,
    /// 0 to 100.
    Brightness(u8),
    /// Brighter (positive) or dimmer by this many points.
    BrightnessStep(i8),
    /// Some = set, None = flip.
    MicMute(Option<bool>),
    Media(MediaKey),
}

pub fn risk(op: &SysOp) -> Risk {
    match op {
        // Sleep stops downloads and anything running; the person decides.
        SysOp::Sleep => Risk::Asks,
        _ => Risk::Safe,
    }
}

pub fn describe(op: &SysOp) -> String {
    match op {
        SysOp::Lock => "Lock the PC".into(),
        SysOp::Sleep => "Put the PC to sleep".into(),
        SysOp::Screenshot => "Take a screenshot".into(),
        SysOp::Brightness(n) => format!("Set the brightness to {n}%"),
        SysOp::BrightnessStep(n) if *n >= 0 => "Turn the brightness up".into(),
        SysOp::BrightnessStep(_) => "Turn the brightness down".into(),
        SysOp::MicMute(Some(true)) => "Mute the microphone".into(),
        SysOp::MicMute(Some(false)) => "Unmute the microphone".into(),
        SysOp::MicMute(None) => "Mute or unmute the microphone".into(),
        SysOp::Media(MediaKey::PlayPause) => "Play or pause the media".into(),
        SysOp::Media(MediaKey::Next) => "Skip to the next track".into(),
        SysOp::Media(MediaKey::Previous) => "Go back a track".into(),
    }
}

/// The spoken answer, short. `out` is what `run` returned.
pub fn say(op: &SysOp, out: &str) -> String {
    match op {
        SysOp::Lock => "Locked.".into(),
        SysOp::Sleep => "Going to sleep.".into(),
        SysOp::Screenshot => "Screenshot saved in your Pictures, in the Screenshots folder.".into(),
        SysOp::Brightness(n) => format!("Brightness {n} percent."),
        SysOp::BrightnessStep(_) => match out.trim().parse::<u8>() {
            Ok(n) => format!("Brightness {n} percent."),
            Err(_) => "Done.".into(),
        },
        SysOp::MicMute(_) => if out.trim() == "muted" { "Microphone muted." } else { "Microphone on." }.into(),
        SysOp::Media(MediaKey::PlayPause) => "Done.".into(),
        SysOp::Media(MediaKey::Next) => "Next track.".into(),
        SysOp::Media(MediaKey::Previous) => "Previous track.".into(),
    }
}

/// The new brightness after a step, kept within 0 to 100.
pub fn step_brightness(now: u8, step: i8) -> u8 {
    (now as i16 + step as i16).clamp(0, 100) as u8
}

/// The command for each power choice: a program and its arguments, no shell. A short delay lets the spoken answer finish.
pub fn power_command(kind: PowerKind) -> (&'static str, Vec<&'static str>) {
    match kind {
        PowerKind::Shutdown => ("shutdown.exe", vec!["/s", "/t", "10"]),
        PowerKind::Restart => ("shutdown.exe", vec!["/r", "/t", "10"]),
        PowerKind::SignOut => ("shutdown.exe", vec!["/l"]),
    }
}

pub fn power(kind: PowerKind) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let (prog, args) = power_command(kind);
        std::process::Command::new(prog).args(args).creation_flags(0x0800_0000).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = kind;
        Err("this needs Windows".into())
    }
}

/// Does the operation. Returns what `say` needs (the new level, or "muted"/"on").
pub fn run(op: &SysOp, input: &dyn Input) -> Result<String, String> {
    match op {
        SysOp::Screenshot => key(input, "win+printscreen").map(|_| String::new()),
        SysOp::Media(k) => key(input, match k {
            MediaKey::PlayPause => "playpause",
            MediaKey::Next => "nexttrack",
            MediaKey::Previous => "prevtrack",
        })
        .map(|_| String::new()),
        other => platform(other),
    }
}

fn key(input: &dyn Input, combo: &str) -> Result<(), String> {
    let c = parse_combo(combo).map_err(|e| e.to_string())?;
    input.key_combo(&c).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn platform(_: &SysOp) -> Result<String, String> {
    Err("this needs Windows".into())
}

#[cfg(windows)]
fn platform(op: &SysOp) -> Result<String, String> {
    match op {
        SysOp::Lock => unsafe { windows::Win32::System::Shutdown::LockWorkStation() }.map(|_| String::new()).map_err(|e| e.to_string()),
        SysOp::Sleep => {
            // Sleep, not hibernate; not forced, so a program that refuses is respected.
            if unsafe { windows::Win32::System::Power::SetSuspendState(false, false, false) } {
                Ok(String::new())
            } else {
                Err("Windows would not go to sleep".into())
            }
        }
        SysOp::Brightness(n) => brightness_set((*n).min(100)).map(|n| n.to_string()),
        SysOp::BrightnessStep(s) => {
            let now = brightness_now()?;
            brightness_set(step_brightness(now, *s)).map(|n| n.to_string())
        }
        SysOp::MicMute(m) => mic_mute(*m),
        SysOp::Screenshot | SysOp::Media(_) => Err("not here".into()),
    }
}

#[cfg(windows)]
fn powershell(script: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("Windows would not change the brightness of this display".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(windows)]
fn brightness_now() -> Result<u8, String> {
    powershell("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1).CurrentBrightness")?
        .parse::<u8>()
        .map_err(|_| "This display doesn't let Windows change its brightness (most desktop monitors don't; use the buttons on the monitor)".to_string())
}

#[cfg(windows)]
fn brightness_set(n: u8) -> Result<u8, String> {
    // `n` is a number: the only thing that goes into the script.
    powershell(&format!("Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=1; Brightness={n}}} | Out-Null"))
        .map_err(|_| "This display doesn't let Windows change its brightness (most desktop monitors don't; use the buttons on the monitor)".to_string())?;
    Ok(n)
}

#[cfg(windows)]
fn mic_mute(set: Option<bool>) -> Result<String, String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eCapture, eMultimedia, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    super::com::ensure()?;
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let device = enumerator.GetDefaultAudioEndpoint(eCapture, eMultimedia).map_err(|_| "There is no microphone".to_string())?;
        let v: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())?;
        let want = match set {
            Some(m) => m,
            None => !v.GetMute().map_err(|e| e.to_string())?.as_bool(),
        };
        v.SetMute(want, std::ptr::null()).map_err(|e| e.to_string())?;
        Ok(if want { "muted" } else { "on" }.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::input::fake::FakeInput;

    #[test]
    fn brightness_steps_stay_in_range() {
        assert_eq!(step_brightness(50, 10), 60);
        assert_eq!(step_brightness(95, 10), 100);
        assert_eq!(step_brightness(5, -10), 0);
    }

    #[test]
    fn power_commands_are_fixed_programs_with_a_delay() {
        assert_eq!(power_command(PowerKind::Shutdown), ("shutdown.exe", vec!["/s", "/t", "10"]));
        assert_eq!(power_command(PowerKind::Restart).1[0], "/r");
        assert_eq!(power_command(PowerKind::SignOut).1, vec!["/l"]);
    }

    #[test]
    fn only_sleep_asks() {
        assert_eq!(risk(&SysOp::Sleep), Risk::Asks);
        for op in [SysOp::Lock, SysOp::Screenshot, SysOp::Brightness(5), SysOp::MicMute(None), SysOp::Media(MediaKey::Next)] {
            assert_eq!(risk(&op), Risk::Safe);
        }
    }

    #[test]
    fn keys_go_through_the_input_and_the_words_are_short() {
        let i = FakeInput::default();
        assert!(run(&SysOp::Media(MediaKey::PlayPause), &i).is_ok());
        assert!(run(&SysOp::Screenshot, &i).is_ok());
        let log = i.log.lock().unwrap();
        assert!(log[0].contains("playpause"));
        assert!(log[1].contains("Win") && log[1].contains("printscreen"));
        assert_eq!(say(&SysOp::BrightnessStep(10), "70"), "Brightness 70 percent.");
        assert_eq!(say(&SysOp::MicMute(None), "muted"), "Microphone muted.");
        assert_eq!(say(&SysOp::MicMute(None), "on"), "Microphone on.");
        assert!(say(&SysOp::Screenshot, "").contains("Screenshots"));
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_the_real_ones_say_so() {
        assert!(run(&SysOp::Lock, &FakeInput::default()).is_err());
        assert!(power(PowerKind::Shutdown).is_err());
    }
}
