//! Which microphone and which speakers BYTE uses for its voice. Nothing chosen means the system's own default, which is what almost
//! everyone wants; the choice only exists for a PC with several (a headset and a webcam, speakers and a monitor).

use std::sync::Mutex;

use serde::Serialize;

static CHOSEN: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

/// Remembers the choice (names as `devices()` lists them; empty is "the system default"). True when it changed.
pub fn set(input: &str, output: &str) -> bool {
    let Ok(mut c) = CHOSEN.lock() else { return false };
    let changed = c.0 != input || c.1 != output;
    *c = (input.to_string(), output.to_string());
    changed
}

pub fn chosen_input() -> String {
    CHOSEN.lock().map(|c| c.0.clone()).unwrap_or_default()
}

pub fn chosen_output() -> String {
    CHOSEN.lock().map(|c| c.1.clone()).unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Devices {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub default_input: String,
    pub default_output: String,
}

#[cfg(any(target_os = "macos", windows))]
mod real {
    use cpal::traits::{DeviceTrait, HostTrait};

    fn name(d: &cpal::Device) -> String {
        d.description().map(|x| x.name().to_string()).unwrap_or_default()
    }

    /// The microphone to use: the chosen one if it is still plugged in, else the default.
    pub fn input() -> Option<cpal::Device> {
        let host = cpal::default_host();
        let want = super::chosen_input();
        if !want.is_empty() {
            if let Some(d) = host.input_devices().ok()?.find(|d| name(d) == want) {
                return Some(d);
            }
        }
        host.default_input_device()
    }

    /// The speakers to use: the chosen ones if still there, else the default.
    pub fn output() -> Option<cpal::Device> {
        let host = cpal::default_host();
        let want = super::chosen_output();
        if !want.is_empty() {
            if let Some(d) = host.output_devices().ok()?.find(|d| name(d) == want) {
                return Some(d);
            }
        }
        host.default_output_device()
    }

    pub fn devices() -> super::Devices {
        let host = cpal::default_host();
        let list = |it: Option<Vec<cpal::Device>>| {
            let mut v: Vec<String> = it.unwrap_or_default().iter().map(name).filter(|n| !n.is_empty()).collect();
            v.sort();
            v.dedup();
            v
        };
        super::Devices {
            inputs: list(host.input_devices().ok().map(|i| i.collect())),
            outputs: list(host.output_devices().ok().map(|i| i.collect())),
            default_input: host.default_input_device().map(|d| name(&d)).unwrap_or_default(),
            default_output: host.default_output_device().map(|d| name(&d)).unwrap_or_default(),
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
pub use real::{input, output};

/// The microphones and speakers the system has (empty where BYTE has no voice of its own).
#[tauri::command]
pub fn audio_devices() -> Devices {
    #[cfg(any(target_os = "macos", windows))]
    {
        real::devices()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Devices::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_is_remembered_and_a_change_is_reported() {
        assert!(set("Headset mic", ""));
        assert!(!set("Headset mic", ""));
        assert_eq!(chosen_input(), "Headset mic");
        assert!(set("", ""));
        assert_eq!(chosen_input(), "");
    }
}
