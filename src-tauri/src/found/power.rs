//! Laptop mode (off until chosen): while the computer runs on its battery, "Hey BYTE" listening and the screen replay are paused
//! (their settings are not changed), and the screen shows a small badge that also suggests a smaller model. The state of the power
//! comes from a [`PowerSource`]; on Windows that is `GetSystemPowerStatus`, and tests use a fake.
//!
//! Following the rule for anything that acts by itself: a change is acted on only after two readings, 30 seconds apart, agree.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::Settings;
use crate::state::AppState;

pub const EVENT: &str = "power://state";
const POLL: Duration = Duration::from_secs(30);

/// Where the power state comes from.
pub trait PowerSource: Send + Sync {
    /// `Some(true)` on battery, `Some(false)` on mains power, `None` when unknown or there is no battery.
    fn on_battery(&self) -> Option<bool>;
}

/// The real thing.
pub struct SystemPower;

impl PowerSource for SystemPower {
    fn on_battery(&self) -> Option<bool> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
            let mut st = SYSTEM_POWER_STATUS::default();
            // SAFETY: the struct is plain data and is only written to.
            unsafe { GetSystemPowerStatus(&mut st).ok()? };
            from_windows(st.ACLineStatus, st.BatteryFlag)
        }
        #[cfg(not(windows))]
        {
            crate::system::battery().map(|(_, charging)| !charging)
        }
    }
}

/// `ACLineStatus` (0 battery, 1 mains, 255 unknown) and `BatteryFlag` (128 no battery, 255 unknown) of `SYSTEM_POWER_STATUS`.
pub fn from_windows(ac_line: u8, battery_flag: u8) -> Option<bool> {
    if battery_flag == 128 {
        return None; // a desktop
    }
    match ac_line {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

/// Turns single readings into confirmed changes.
#[derive(Debug, Default)]
pub struct Debounce {
    state: bool,
    pending: Option<bool>,
}

impl Debounce {
    pub fn state(&self) -> bool {
        self.state
    }

    /// Feed one reading; returns the new state when two in a row differ from the current one. An unknown reading changes nothing.
    pub fn feed(&mut self, reading: Option<bool>) -> Option<bool> {
        let Some(r) = reading else {
            self.pending = None;
            return None;
        };
        if r == self.state {
            self.pending = None;
            return None;
        }
        if self.pending == Some(r) {
            self.pending = None;
            self.state = r;
            return Some(r);
        }
        self.pending = Some(r);
        None
    }
}

static ON_BATTERY: AtomicBool = AtomicBool::new(false);

/// Laptop mode is on and the computer is on battery.
pub fn saving(s: &Settings) -> bool {
    s.laptop_mode && ON_BATTERY.load(Ordering::Relaxed)
}

/// The settings as BYTE should act on them right now: the listening and replay switches held off while saving power.
pub fn effective(s: &Settings) -> Settings {
    let mut e = s.clone();
    if saving(s) {
        e.wake_word = false;
        e.replay_buffer = false;
    }
    e
}

/// Applies the listening and replay settings (held off when saving power).
pub fn reapply(app: &AppHandle, s: &Settings) {
    let e = effective(s);
    crate::wake::apply(app, e.wake_word);
    crate::screen::apply(app, &e);
    crate::voicectl::apply(app, &e);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerStatus {
    pub on_battery: bool,
    pub laptop_mode: bool,
    /// What is paused right now, in words.
    pub paused: Vec<String>,
}

pub fn status_for(on_battery: bool, s: &Settings) -> PowerStatus {
    let mut paused = vec![];
    if s.laptop_mode && on_battery {
        if s.wake_word {
            paused.push("Hey BYTE listening".to_string());
        }
        if s.replay_buffer {
            paused.push("the screen replay".to_string());
        }
    }
    PowerStatus { on_battery, laptop_mode: s.laptop_mode, paused }
}

#[tauri::command]
pub async fn power_status(state: tauri::State<'_, AppState>) -> Result<PowerStatus, String> {
    let s = state.settings.lock().await.clone();
    Ok(status_for(ON_BATTERY.load(Ordering::Relaxed), &s))
}

/// Starts the poller (desktop only; a phone manages its own battery).
pub fn start(app: &AppHandle) {
    #[cfg(desktop)]
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut d = Debounce::default();
            loop {
                tokio::time::sleep(POLL).await;
                let reading = tokio::task::spawn_blocking(src_read).await.ok().flatten();
                let Some(now) = reading else {
                    d.feed(None);
                    continue;
                };
                if let Some(changed) = d.feed(Some(now)) {
                    ON_BATTERY.store(changed, Ordering::Relaxed);
                    let s = app.state::<AppState>().settings.lock().await.clone();
                    log::info!("power: {}", if changed { "on battery" } else { "on mains power" });
                    if s.laptop_mode {
                        reapply(&app, &s);
                    }
                    let _ = app.emit(EVENT, status_for(changed, &s));
                }
            }
        });
    }
    #[cfg(not(desktop))]
    let _ = app;
}

fn src_read() -> Option<bool> {
    SystemPower.on_battery()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Vec<Option<bool>>);
    impl PowerSource for Fake {
        fn on_battery(&self) -> Option<bool> {
            self.0.first().copied().flatten()
        }
    }

    #[test]
    fn windows_codes_map_to_battery_mains_or_unknown() {
        assert_eq!(from_windows(0, 1), Some(true));
        assert_eq!(from_windows(1, 8), Some(false));
        assert_eq!(from_windows(255, 1), None);
        assert_eq!(from_windows(0, 128), None, "no battery means a desktop");
    }

    #[test]
    fn a_change_needs_two_readings_in_a_row() {
        let mut d = Debounce::default();
        assert_eq!(d.feed(Some(true)), None);
        assert_eq!(d.feed(Some(true)), Some(true));
        assert!(d.state());
        assert_eq!(d.feed(Some(true)), None, "no change, nothing to do");
        assert_eq!(d.feed(Some(false)), None);
        assert_eq!(d.feed(Some(true)), None, "a single odd reading is ignored");
        assert_eq!(d.feed(Some(false)), None);
        assert_eq!(d.feed(Some(false)), Some(false));
    }

    #[test]
    fn an_unknown_reading_never_changes_anything_and_resets_the_wait() {
        let mut d = Debounce::default();
        assert_eq!(d.feed(Some(true)), None);
        assert_eq!(d.feed(None), None);
        assert_eq!(d.feed(Some(true)), None);
        assert!(!d.state());
    }

    #[test]
    fn the_fake_source_reports_what_it_was_given() {
        assert_eq!(Fake(vec![Some(true)]).on_battery(), Some(true));
        assert_eq!(Fake(vec![None]).on_battery(), None);
    }

    #[test]
    fn saving_power_holds_listening_and_replay_off_without_changing_the_setting() {
        let s = Settings { laptop_mode: true, wake_word: true, replay_buffer: true, ..Settings::default() };
        ON_BATTERY.store(true, Ordering::Relaxed);
        let e = effective(&s);
        assert!(!e.wake_word && !e.replay_buffer);
        assert!(s.wake_word && s.replay_buffer);
        assert_eq!(status_for(true, &s).paused, ["Hey BYTE listening", "the screen replay"]);
        let off = Settings { laptop_mode: false, ..s.clone() };
        assert!(effective(&off).wake_word, "the mode is off, so nothing is paused");
        assert!(status_for(true, &off).paused.is_empty());
        ON_BATTERY.store(false, Ordering::Relaxed);
        assert!(effective(&s).wake_word);
    }
}
