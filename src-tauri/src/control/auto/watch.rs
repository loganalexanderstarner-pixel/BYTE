//! Two ways the person takes the wheel: the stop key, and (when they switch it on) using the mouse or keyboard while a job runs.

/// The stop key a job listens for unless the person picks another. Guided help already uses F8, F9 and Esc, so none of those.
pub const DEFAULT_STOP_KEY: &str = "Ctrl+Alt+F12";

/// Checks a stop key: a real shortcut, not one BYTE already uses.
pub fn validate_stop_key(keys: &str, others: &[String]) -> Result<(), String> {
    let sc = crate::quick::parse_keys(keys)?;
    let low = keys.to_lowercase();
    let last = low.rsplit('+').next().unwrap_or("").trim().to_string();
    if matches!(last.as_str(), "f8" | "f9" | "escape" | "esc") && !low.contains("ctrl") && !low.contains("alt") {
        return Err("F8, F9 and Esc belong to guided help. Pick another stop key.".into());
    }
    for o in others.iter().filter(|o| !o.trim().is_empty()) {
        if crate::quick::parse_keys(o).map(|x| x.id() == sc.id()).unwrap_or(false) {
            return Err(format!("{} is already another BYTE shortcut.", crate::quick::pretty(o)));
        }
    }
    Ok(())
}

/// Tells BYTE's own mouse and keyboard input from the person's. Windows keeps one "last input" time for both, so BYTE notes
/// when it last sent input itself; input later than that (plus a short grace) is the person's.
#[derive(Debug, Default)]
pub struct Takeover {
    baseline: Option<u64>,
}

pub const GRACE_MS: u64 = 1_200;

impl Takeover {
    /// `last_input`: when the system last saw any input; `own`: when BYTE last sent some (same clock, milliseconds). True when the
    /// person did something since the last check.
    pub fn check(&mut self, last_input: u64, own: u64) -> bool {
        let Some(base) = self.baseline else {
            self.baseline = Some(last_input);
            return false;
        };
        if last_input <= base {
            return false;
        }
        self.baseline = Some(last_input);
        last_input > own.saturating_add(GRACE_MS)
    }

    /// Forgets what was seen (after a pause ends, so the same movement does not pause it again).
    pub fn reset(&mut self) {
        self.baseline = None;
    }
}

/// The system's clock and last-input time, in the same milliseconds.
pub trait Presence: Send + Sync {
    fn now(&self) -> u64;
    fn last_input(&self) -> Option<u64>;
}

pub struct SystemPresence;

impl Presence for SystemPresence {
    fn now(&self) -> u64 {
        #[cfg(windows)]
        {
            unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
        }
        #[cfg(not(windows))]
        {
            0
        }
    }

    fn last_input(&self) -> Option<u64> {
        #[cfg(windows)]
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
            let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
                return None;
            }
            // dwTime is the low 32 bits of the tick count.
            let now = self.now();
            let mut t = (now & !0xFFFF_FFFF) | info.dwTime as u64;
            if t > now {
                t = t.saturating_sub(1 << 32);
            }
            Some(t)
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

#[cfg(test)]
pub struct FakePresence {
    pub now: std::sync::atomic::AtomicU64,
    pub input: std::sync::atomic::AtomicU64,
}

#[cfg(test)]
impl Presence for FakePresence {
    fn now(&self) -> u64 {
        self.now.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn last_input(&self) -> Option<u64> {
        Some(self.input.load(std::sync::atomic::Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_look_only_remembers() {
        let mut t = Takeover::default();
        assert!(!t.check(10_000, 0));
    }

    #[test]
    fn input_that_bytes_own_steps_made_is_not_the_person() {
        let mut t = Takeover::default();
        t.check(10_000, 9_000);
        // BYTE clicked at 11_000 and noted it at 11_400.
        assert!(!t.check(11_000, 11_400));
    }

    #[test]
    fn input_after_the_last_own_step_is_the_person() {
        let mut t = Takeover::default();
        t.check(10_000, 9_000);
        assert!(t.check(15_000, 11_400));
        assert!(!t.check(15_000, 11_400), "the same movement is not counted twice");
    }

    #[test]
    fn a_reset_forgets_the_old_movement() {
        let mut t = Takeover::default();
        t.check(10_000, 0);
        t.reset();
        assert!(!t.check(20_000, 0));
    }

    #[test]
    fn stop_keys_are_checked() {
        assert!(validate_stop_key(DEFAULT_STOP_KEY, &[]).is_ok());
        assert!(validate_stop_key("F12", &[]).is_err(), "needs a modifier");
        assert!(validate_stop_key("F9", &[]).is_err());
        assert!(validate_stop_key("Ctrl+Alt+F12", &["Ctrl+Alt+F12".into()]).is_err(), "already used");
        assert!(validate_stop_key("Ctrl+Alt+F11", &["Ctrl+Alt+F12".into(), String::new()]).is_ok());
        assert!(validate_stop_key("nonsense+key", &[]).is_err());
    }

    #[test]
    fn the_fake_presence_reports_what_it_was_given() {
        let p = FakePresence { now: 5.into(), input: 3.into() };
        assert_eq!((p.now(), p.last_input()), (5, Some(3)));
    }
}
