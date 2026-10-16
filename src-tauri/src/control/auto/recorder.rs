//! "Watch me do this once": with the person's permission and a visible badge, BYTE notes the buttons they press and the key
//! combinations they use in an app, as UI Automation names (never pixels, never typed text), and offers them as a routine to replay
//! or edit. Password and card fields are never noted; programs on the never-touch list are not watched at all.

use crate::control::routines::{RStep, MAX_STEPS};
use crate::control::uia::{Element, Snapshot};

/// What the watcher saw, before it is turned into steps.
#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
    /// A click on a named item in a window.
    Click { app: String, window: String, name: String, sensitive: bool },
    /// A key combination with Ctrl, Alt or Win, or a special key (Enter, Tab, Esc, F-keys).
    Keys { app: String, combo: String },
    /// The person typed ordinary characters (how many; never which).
    Typing { app: String, count: u32 },
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub steps: Vec<RStep>,
    /// Parts the person typed that were left out (text is never kept).
    pub typed_parts: u32,
    /// Items left out because they were password or card fields.
    pub secret_skipped: u32,
    /// Steps beyond what a routine holds.
    pub cut: usize,
}

#[derive(Default)]
pub struct Recorder {
    never_touch: Vec<String>,
    steps: Vec<RStep>,
    typed_parts: u32,
    secret_skipped: u32,
    last_typing: bool,
}

fn key(s: &str) -> String {
    let l = s.trim().to_lowercase();
    l.strip_suffix(".exe").unwrap_or(&l).to_string()
}

impl Recorder {
    pub fn new(never_touch: Vec<String>) -> Recorder {
        Recorder { never_touch: never_touch.iter().map(|a| key(a)).collect(), ..Default::default() }
    }

    pub fn feed(&mut self, raw: Raw) {
        let app = match &raw {
            Raw::Click { app, .. } | Raw::Keys { app, .. } | Raw::Typing { app, .. } => key(app),
        };
        if self.never_touch.contains(&app) && !app.is_empty() {
            return;
        }
        match raw {
            Raw::Click { window, name, sensitive, .. } => {
                self.last_typing = false;
                if sensitive || crate::control::uia::sensitive_name(&name) {
                    self.secret_skipped += 1;
                } else if !name.trim().is_empty() && !window.trim().is_empty() {
                    self.steps.push(RStep::ClickElement { window: window.trim().to_string(), name: name.trim().to_string() });
                }
            }
            Raw::Keys { combo, .. } => {
                self.last_typing = false;
                let step = RStep::PressKeys { combo: combo.to_lowercase() };
                if step.check().is_ok() {
                    self.steps.push(step);
                }
            }
            Raw::Typing { .. } => {
                if !self.last_typing {
                    self.typed_parts += 1;
                }
                self.last_typing = true;
            }
        }
    }

    pub fn finish(self) -> Recording {
        let cut = self.steps.len().saturating_sub(MAX_STEPS);
        let mut steps = self.steps;
        steps.truncate(MAX_STEPS);
        Recording { steps, typed_parts: self.typed_parts, secret_skipped: self.secret_skipped, cut }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }
}

/// The named item under a point: the smallest one that contains it.
pub fn element_at(shot: &Snapshot, x: i32, y: i32) -> Option<&Element> {
    shot.elements.iter().filter(|e| !e.name.trim().is_empty() && e.rect.left <= x && x <= e.rect.right && e.rect.top <= y && y <= e.rect.bottom).min_by_key(|e| (e.rect.right - e.rect.left).max(0) as i64 * (e.rect.bottom - e.rect.top).max(0) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::uia::{Kind, Rect};

    fn click(app: &str, name: &str) -> Raw {
        Raw::Click { app: app.into(), window: "Untitled - Notepad".into(), name: name.into(), sensitive: false }
    }

    #[test]
    fn clicks_and_key_combinations_become_steps_in_order() {
        let mut r = Recorder::new(vec![]);
        r.feed(click("notepad", "File"));
        r.feed(click("notepad", "Save as"));
        r.feed(Raw::Keys { app: "notepad".into(), combo: "Ctrl+S".into() });
        let rec = r.finish();
        assert_eq!(rec.steps, vec![RStep::ClickElement { window: "Untitled - Notepad".into(), name: "File".into() }, RStep::ClickElement { window: "Untitled - Notepad".into(), name: "Save as".into() }, RStep::PressKeys { combo: "ctrl+s".into() }]);
    }

    #[test]
    fn typed_text_is_counted_but_never_kept() {
        let mut r = Recorder::new(vec![]);
        r.feed(Raw::Typing { app: "notepad".into(), count: 5 });
        r.feed(Raw::Typing { app: "notepad".into(), count: 3 });
        r.feed(click("notepad", "Save"));
        r.feed(Raw::Typing { app: "notepad".into(), count: 2 });
        let rec = r.finish();
        assert_eq!(rec.typed_parts, 2);
        assert_eq!(rec.steps.len(), 1);
        assert!(!format!("{:?}", rec).contains("count"));
    }

    #[test]
    fn password_and_card_fields_are_never_noted() {
        let mut r = Recorder::new(vec![]);
        r.feed(Raw::Click { app: "chrome".into(), window: "Sign in".into(), name: "Username".into(), sensitive: true });
        r.feed(click("chrome", "Password"));
        r.feed(click("chrome", "Card number"));
        let rec = r.finish();
        assert!(rec.steps.is_empty());
        assert_eq!(rec.secret_skipped, 3);
    }

    #[test]
    fn programs_on_the_never_touch_list_are_not_watched() {
        let mut r = Recorder::new(vec!["Bank.exe".into()]);
        r.feed(click("bank", "Transfer history"));
        r.feed(Raw::Keys { app: "bank".into(), combo: "ctrl+p".into() });
        assert!(r.finish().steps.is_empty());
    }

    #[test]
    fn dangerous_key_combinations_are_not_recorded() {
        let mut r = Recorder::new(vec![]);
        r.feed(Raw::Keys { app: "x".into(), combo: "win+r".into() });
        r.feed(Raw::Keys { app: "x".into(), combo: "ctrl+alt+delete".into() });
        assert!(r.finish().steps.is_empty());
    }

    #[test]
    fn a_long_recording_is_cut_to_what_a_routine_holds() {
        let mut r = Recorder::new(vec![]);
        for i in 0..20 {
            r.feed(click("notepad", &format!("Item {i}")));
        }
        let rec = r.finish();
        assert_eq!((rec.steps.len(), rec.cut), (MAX_STEPS, 20 - MAX_STEPS));
    }

    #[test]
    fn the_smallest_named_item_under_the_pointer_is_the_one_clicked() {
        let el = |id, name: &str, l, t, r, b| Element { id, name: name.into(), kind: Kind::Button, enabled: true, rect: Rect { left: l, top: t, right: r, bottom: b }, sensitive: false };
        let shot = Snapshot { app: "x".into(), title: "w".into(), elements: vec![el(1, "Toolbar", 0, 0, 500, 50), el(2, "Save", 10, 10, 60, 40), el(3, "", 12, 12, 30, 30)] };
        assert_eq!(element_at(&shot, 20, 20).unwrap().name, "Save");
        assert_eq!(element_at(&shot, 400, 20).unwrap().name, "Toolbar");
        assert!(element_at(&shot, 900, 900).is_none());
    }

    #[test]
    fn a_recorded_step_passes_the_routine_checks_and_a_secret_one_does_not() {
        assert!(RStep::ClickElement { window: "w".into(), name: "Save".into() }.check().is_ok());
        assert!(RStep::ClickElement { window: "w".into(), name: "Password".into() }.check().is_err());
        assert!(RStep::PressKeys { combo: "win+r".into() }.check().is_err());
        assert!(RStep::TypeText { text: String::new() }.check().is_err());
    }
}
