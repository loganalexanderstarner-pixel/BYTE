//! Keys and the mouse. `Input` is the trait the executor uses (a fake in tests); `WinInput` is SendInput on Windows.
//! `virtual_key` is platform-free so the key table is tested everywhere.

use super::actions::{Combo, KeyCode, Modifier};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError(pub String);

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait Input: Send + Sync {
    fn key_combo(&self, combo: &Combo) -> Result<(), InputError>;
    /// Types text as keystrokes (Unicode characters, so any language works).
    fn type_text(&self, text: &str) -> Result<(), InputError>;
    fn click_at(&self, x: i32, y: i32) -> Result<(), InputError>;
    /// Positive `notches` scroll down, negative up.
    fn scroll(&self, notches: i32) -> Result<(), InputError>;
}

pub fn modifier_vk(m: Modifier) -> u16 {
    match m {
        Modifier::Ctrl => 0x11,
        Modifier::Shift => 0x10,
        Modifier::Alt => 0x12,
        Modifier::Win => 0x5B,
    }
}

/// The Windows virtual-key code of the main key.
pub fn virtual_key(k: &KeyCode) -> Option<u16> {
    Some(match k {
        KeyCode::F(n) if (1..=12).contains(n) => 0x70 + (*n as u16 - 1),
        KeyCode::F(_) => return None,
        KeyCode::Char(c) if c.is_ascii_digit() => *c as u16,
        KeyCode::Char(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase() as u16,
        KeyCode::Char(c) => match c {
            ';' => 0xBA,
            '=' | '+' => 0xBB,
            ',' => 0xBC,
            '-' => 0xBD,
            '.' => 0xBE,
            '/' => 0xBF,
            '`' => 0xC0,
            '[' => 0xDB,
            '\\' => 0xDC,
            ']' => 0xDD,
            '\'' => 0xDE,
            _ => return None,
        },
        KeyCode::Named(n) => match *n {
            "enter" => 0x0D,
            "tab" => 0x09,
            "esc" => 0x1B,
            "space" => 0x20,
            "backspace" => 0x08,
            "delete" => 0x2E,
            "insert" => 0x2D,
            "home" => 0x24,
            "end" => 0x23,
            "pageup" => 0x21,
            "pagedown" => 0x22,
            "up" => 0x26,
            "down" => 0x28,
            "left" => 0x25,
            "right" => 0x27,
            "playpause" => 0xB3,
            "nexttrack" => 0xB0,
            "prevtrack" => 0xB1,
            "volumeup" => 0xAF,
            "volumedown" => 0xAE,
            "volumemute" => 0xAD,
            "printscreen" => 0x2C,
            _ => return None,
        },
    })
}

/// Keys that need the "extended" flag in SendInput (arrows, navigation, Delete, Insert).
pub fn is_extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B)
}

#[cfg(windows)]
pub use win::WinInput;

#[cfg(windows)]
mod win {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    pub struct WinInput;

    fn send(inputs: &[INPUT]) -> Result<(), InputError> {
        if inputs.is_empty() {
            return Ok(());
        }
        let n = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if n as usize != inputs.len() {
            // Blocked by a program running as administrator, or by a locked screen.
            return Err(InputError("Windows did not accept the keys (a program running as administrator may be in front)".into()));
        }
        Ok(())
    }

    fn key(vk: u16, up: bool) -> INPUT {
        let mut flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        if is_extended(vk) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    fn unicode(unit: u16, up: bool) -> INPUT {
        let flags = KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: unit, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    fn mouse(flags: MOUSE_EVENT_FLAGS, x: i32, y: i32, data: i32) -> INPUT {
        INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: x, dy: y, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    impl Input for WinInput {
        fn key_combo(&self, combo: &Combo) -> Result<(), InputError> {
            let main = virtual_key(&combo.key).ok_or_else(|| InputError("that key is not available".into()))?;
            let mut seq: Vec<INPUT> = combo.mods.iter().map(|m| key(modifier_vk(*m), false)).collect();
            seq.push(key(main, false));
            seq.push(key(main, true));
            seq.extend(combo.mods.iter().rev().map(|m| key(modifier_vk(*m), true)));
            send(&seq)
        }

        fn type_text(&self, text: &str) -> Result<(), InputError> {
            // Chunks, so a long text is not one huge call.
            let units: Vec<u16> = text.replace("\r\n", "\n").encode_utf16().collect();
            for chunk in units.chunks(100) {
                let mut seq = Vec::with_capacity(chunk.len() * 2);
                for &u in chunk {
                    if u == b'\n' as u16 {
                        seq.push(key(0x0D, false));
                        seq.push(key(0x0D, true));
                    } else if u == b'\t' as u16 {
                        seq.push(key(0x09, false));
                        seq.push(key(0x09, true));
                    } else {
                        seq.push(unicode(u, false));
                        seq.push(unicode(u, true));
                    }
                }
                send(&seq)?;
            }
            Ok(())
        }

        fn click_at(&self, x: i32, y: i32) -> Result<(), InputError> {
            let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN).max(1), GetSystemMetrics(SM_CYSCREEN).max(1)) };
            // Absolute coordinates run 0..65535 across the primary screen.
            let ax = (x as i64 * 65535 / w as i64) as i32;
            let ay = (y as i64 * 65535 / h as i64) as i32;
            send(&[mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, ax, ay, 0), mouse(MOUSEEVENTF_LEFTDOWN, 0, 0, 0), mouse(MOUSEEVENTF_LEFTUP, 0, 0, 0)])
        }

        fn scroll(&self, notches: i32) -> Result<(), InputError> {
            // One notch is 120; a positive wheel value scrolls up, so down is negative.
            send(&[mouse(MOUSEEVENTF_WHEEL, 0, 0, -notches * 120)])
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeInput {
        pub log: Mutex<Vec<String>>,
    }

    impl Input for FakeInput {
        fn key_combo(&self, c: &Combo) -> Result<(), InputError> {
            self.log.lock().unwrap().push(format!("key {:?}+{:?}", c.mods, c.key));
            Ok(())
        }
        fn type_text(&self, t: &str) -> Result<(), InputError> {
            self.log.lock().unwrap().push(format!("type {t}"));
            Ok(())
        }
        fn click_at(&self, x: i32, y: i32) -> Result<(), InputError> {
            self.log.lock().unwrap().push(format!("mouse {x},{y}"));
            Ok(())
        }
        fn scroll(&self, n: i32) -> Result<(), InputError> {
            self.log.lock().unwrap().push(format!("scroll {n}"));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::actions::parse_combo;

    #[test]
    fn letters_digits_and_function_keys_map_to_windows_codes() {
        let vk = |s: &str| virtual_key(&parse_combo(s).unwrap().key);
        assert_eq!(vk("a"), Some(0x41));
        assert_eq!(vk("ctrl+s"), Some(0x53));
        assert_eq!(vk("5"), Some(0x35));
        assert_eq!(vk("f1"), Some(0x70));
        assert_eq!(vk("f12"), Some(0x7B));
        assert_eq!(vk("enter"), Some(0x0D));
        assert_eq!(vk("playpause"), Some(0xB3));
        assert_eq!(vk("volumemute"), Some(0xAD));
        assert_eq!(vk("ctrl+,"), Some(0xBC));
    }

    #[test]
    fn modifiers_and_extended_keys() {
        assert_eq!(modifier_vk(Modifier::Ctrl), 0x11);
        assert_eq!(modifier_vk(Modifier::Win), 0x5B);
        assert!(is_extended(0x26) && is_extended(0x2E));
        assert!(!is_extended(0x41) && !is_extended(0x0D));
    }

    #[test]
    fn every_named_key_has_a_code() {
        for n in ["enter", "tab", "esc", "space", "backspace", "delete", "home", "end", "pageup", "pagedown", "up", "down", "left", "right", "insert", "playpause", "nexttrack", "prevtrack", "volumeup", "volumedown", "volumemute", "printscreen"] {
            assert!(virtual_key(&KeyCode::Named(Box::leak(n.to_string().into_boxed_str()))).is_some(), "{n}");
        }
    }
}
