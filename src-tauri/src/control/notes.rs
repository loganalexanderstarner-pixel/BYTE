//! Per-program notes: "in Notepad, for change the font: click Format, click Font". Saved only when the person turned notes on,
//! after a run that worked (or from words they typed themselves), kept in one small file on this PC. A note holds the program, a
//! short goal phrase and the NAMES of the buttons used; never typed text, never the contents of a window. Matching notes are
//! given to the planner as hints.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::actions::{Action, Step};
use super::uia::{Kind, Snapshot};

pub const FILE: &str = "control_notes.json";
pub const MAX_NOTES: usize = 200;
const MAX_HINTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteStep {
    /// "open", "focus", "click", "key", "type" (no text kept), "wait", "scroll" or "close".
    pub kind: String,
    /// The name of the button or window, the key combination, or the program: never typed text.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    /// Lower-case program name ("notepad").
    pub app: String,
    /// What was asked, in a few plain words.
    pub goal: String,
    pub steps: Vec<NoteStep>,
    /// Words the person wrote themselves ("the font is under the gear").
    #[serde(default)]
    pub text: String,
    pub at: String,
    #[serde(default)]
    pub uses: u32,
}

pub fn load(path: &Path) -> Vec<Note> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(path: &Path, notes: &[Note]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(notes).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn words(s: &str) -> Vec<String> {
    let mut w: Vec<String> = s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3 && !matches!(*w, "the" | "and" | "for" | "please" | "can" | "you" | "into" | "with" | "that" | "this")).map(String::from).collect();
    w.sort();
    w.dedup();
    w
}

/// The goal as a short phrase that holds no typed content: quoted text, numbers and any word the person had BYTE type are dropped.
pub fn clean_goal(goal: &str, typed: &[String]) -> String {
    let mut unquoted = String::new();
    let mut inside = false;
    for c in goal.chars() {
        if c == '"' || c == '“' || c == '”' {
            inside = !inside;
        } else if !inside {
            unquoted.push(c);
        }
    }
    let typed_words: Vec<String> = typed.iter().flat_map(|t| words(t)).collect();
    let keep: Vec<&str> = unquoted.split_whitespace().filter(|w| !w.chars().any(|c| c.is_ascii_digit()) && !w.contains('@') && !typed_words.contains(&w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())).take(8).collect();
    keep.join(" ").to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

/// Buttons whose names are the program's own words; a list item or tree item is often the person's own content (a file, a contact).
fn nameable(kind: Kind) -> bool {
    matches!(kind, Kind::Button | Kind::MenuItem | Kind::TabItem | Kind::CheckBox | Kind::RadioButton | Kind::Link | Kind::ComboBox | Kind::Slider)
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect()
}

/// A note from a plan that just worked in `snap`'s window. `None` when it cannot be written down safely: a click on something that
/// is not a plain button (or after the window changed, when the names are unknown), a power step, or nothing to remember.
pub fn from_run(steps: &[Step], snap: &Snapshot, goal: &str) -> Option<Note> {
    let mut fresh = true;
    let mut out = Vec::new();
    let mut typed = Vec::new();
    for s in steps {
        let (kind, name) = match &s.action {
            Action::Click { id } => {
                let e = snap.find(*id).filter(|_| fresh)?;
                if e.sensitive || !nameable(e.kind) || e.name.trim().is_empty() || super::uia::sensitive_name(&e.name) {
                    return None;
                }
                ("click", one_line(&e.name))
            }
            Action::Open { target } => ("open", one_line(target)),
            Action::Focus { title } => ("focus", one_line(title)),
            Action::Key { combo } => ("key", combo.to_lowercase()),
            Action::Type { text } => {
                typed.push(text.clone());
                ("type", String::new())
            }
            Action::Wait { text, .. } => ("wait", one_line(text)),
            Action::Scroll { direction, .. } => ("scroll", format!("{direction:?}").to_lowercase()),
            Action::Close { title } => ("close", one_line(title)),
            Action::Power { .. } => return None,
        };
        if super::plancheck::changes_view(s) {
            fresh = false;
        }
        out.push(NoteStep { kind: kind.into(), name });
    }
    // The program: the one a plan starts by opening or switching to, else the window it was made in.
    let app = match steps.first().map(|s| &s.action) {
        Some(Action::Open { target }) => target.to_lowercase(),
        _ => snap.app.to_lowercase(),
    };
    let goal = clean_goal(goal, &typed);
    if out.is_empty() || app.trim().is_empty() || goal.is_empty() {
        return None;
    }
    Some(Note { id: uuid::Uuid::new_v4().simple().to_string(), app: app.trim().trim_end_matches(".exe").to_string(), goal, steps: out, text: String::new(), at: chrono::Local::now().format("%Y-%m-%d").to_string(), uses: 0 })
}

/// A note the person wrote in their own words.
pub fn from_words(app: &str, goal: &str, text: &str) -> Option<Note> {
    let (app, goal, text) = (app.trim().to_lowercase(), clean_goal(goal, &[]), one_line_long(text));
    if app.is_empty() || text.is_empty() {
        return None;
    }
    Some(Note { id: uuid::Uuid::new_v4().simple().to_string(), app: app.trim_end_matches(".exe").to_string(), goal, steps: vec![], text, at: chrono::Local::now().format("%Y-%m-%d").to_string(), uses: 0 })
}

fn one_line_long(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect()
}

/// Adds a note; the same program and goal replace the older one (and count another use). The oldest go first past the cap.
pub fn upsert(list: &mut Vec<Note>, mut n: Note) {
    if let Some(i) = list.iter().position(|o| o.app == n.app && o.goal == n.goal && o.text.is_empty() == n.text.is_empty()) {
        n.uses = list[i].uses + 1;
        list.remove(i);
    }
    list.push(n);
    while list.len() > MAX_NOTES {
        list.remove(0);
    }
}

/// How well a note's goal matches what is asked now: shared words over all words, 0 to 1.
fn similarity(a: &str, b: &str) -> f32 {
    let (x, y) = (words(a), words(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    let shared = x.iter().filter(|w| y.contains(w)).count() as f32;
    shared / (x.len() + y.len()) as f32 * 2.0
}

fn step_words(s: &NoteStep) -> String {
    match s.kind.as_str() {
        "click" => format!("click \"{}\"", s.name),
        "open" => format!("open {}", s.name),
        "focus" => format!("switch to \"{}\"", s.name),
        "key" => format!("press {}", s.name),
        "type" => "type the text".into(),
        "wait" => format!("wait for \"{}\"", s.name),
        "scroll" => format!("scroll {}", s.name),
        "close" => format!("close \"{}\"", s.name),
        other => other.to_string(),
    }
}

/// The notes that fit `app` and `goal`, as lines for the planner (empty when none): `In notepad, for "change font": click "Format" then click "Font"`.
pub fn hints(list: &[Note], app: &str, goal: &str) -> Vec<(String, String)> {
    let app = app.to_lowercase();
    let app = app.trim_end_matches(".exe");
    let mut hits: Vec<(f32, &Note)> = list.iter().filter(|n| n.app == app).map(|n| (if n.goal.is_empty() { 0.5 } else { similarity(&n.goal, goal) }, n)).filter(|(s, _)| *s >= 0.5).collect();
    hits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.uses.cmp(&a.1.uses)));
    hits.into_iter()
        .take(MAX_HINTS)
        .map(|(_, n)| {
            let how = if n.steps.is_empty() { n.text.clone() } else { n.steps.iter().map(step_words).collect::<Vec<_>>().join(" then ") };
            let extra = if !n.steps.is_empty() && !n.text.is_empty() { format!(" ({})", n.text) } else { String::new() };
            let lead = if n.goal.is_empty() { format!("In {}: ", n.app) } else { format!("In {}, for \"{}\": ", n.app, n.goal) };
            (n.id.clone(), format!("{lead}{how}{extra}"))
        })
        .collect()
}

/// The text added to the planner's question: the matching notes, or empty.
pub fn hint_text(list: &[Note], app: &str, goal: &str) -> (String, Vec<String>) {
    let h = hints(list, app, goal);
    if h.is_empty() {
        return (String::new(), vec![]);
    }
    let lines: Vec<String> = h.iter().map(|(_, l)| format!("- {l}")).collect();
    (format!("\n\nWhat worked before in this program (names of buttons, not numbers; use the numbers from the list above):\n{}\n", lines.join("\n")), h.into_iter().map(|(id, _)| id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::actions::Expect;
    use crate::control::uia::fake::{el, snap};

    fn st(a: Action) -> Step {
        Step { action: a, expect: Expect::default() }
    }

    fn pad() -> Snapshot {
        snap("Untitled - Notepad", vec![el(1, "Format", Kind::MenuItem), el(2, "Font", Kind::MenuItem), el(3, "Text editor", Kind::Edit), el(4, "quarterly-report.docx", Kind::ListItem)])
    }

    #[test]
    fn a_run_that_worked_becomes_a_note_of_names_only() {
        let steps = vec![st(Action::Click { id: 1 }), st(Action::Click { id: 2 })];
        let n = from_run(&steps, &pad(), "change the font").unwrap();
        assert_eq!((n.app.as_str(), n.goal.as_str()), ("notepad", "change the font"));
        assert_eq!(n.steps, vec![NoteStep { kind: "click".into(), name: "Format".into() }, NoteStep { kind: "click".into(), name: "Font".into() }]);
    }

    #[test]
    fn typed_text_is_never_kept_and_its_words_are_taken_out_of_the_goal() {
        let steps = vec![st(Action::Open { target: "Notepad".into() }), st(Action::Type { text: "dear Alice, the invoice is late".into() })];
        let n = from_run(&steps, &pad(), "write \"dear Alice\" and the invoice is late in notepad 42").unwrap();
        let json = serde_json::to_string(&n).unwrap();
        assert!(!json.to_lowercase().contains("alice") && !json.contains("invoice") && !json.contains("42"), "{json}");
        assert_eq!(n.steps[1], NoteStep { kind: "type".into(), name: String::new() });
        assert_eq!(n.app, "notepad");
    }

    #[test]
    fn clicks_on_a_file_or_after_the_window_changed_are_not_noted() {
        assert!(from_run(&[st(Action::Click { id: 4 })], &pad(), "open the report").is_none(), "a list item can be the person's own file");
        let after = vec![st(Action::Key { combo: "ctrl+o".into() }), st(Action::Click { id: 1 })];
        assert!(from_run(&after, &pad(), "open a file").is_none(), "after the dialog opened the names are unknown");
        assert!(from_run(&[st(Action::Power { kind: crate::control::actions::PowerKind::Restart })], &pad(), "restart").is_none());
    }

    #[test]
    fn the_same_program_and_goal_replace_the_old_note() {
        let mut l = vec![];
        let a = from_run(&[st(Action::Click { id: 1 })], &pad(), "change the font").unwrap();
        upsert(&mut l, a.clone());
        let mut b = a;
        b.id = "other".into();
        upsert(&mut l, b);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].id.as_str(), l[0].uses), ("other", 1));
        let base = l[0].clone();
        for i in 0..(MAX_NOTES + 5) {
            upsert(&mut l, Note { goal: format!("goal {i}"), ..base.clone() });
        }
        assert_eq!(l.len(), MAX_NOTES);
    }

    #[test]
    fn only_notes_for_the_same_program_and_a_similar_goal_become_hints() {
        let mut l = vec![];
        upsert(&mut l, from_run(&[st(Action::Click { id: 1 }), st(Action::Click { id: 2 })], &pad(), "change the font").unwrap());
        upsert(&mut l, from_words("Notepad", "", "the font is under the gear").unwrap());
        let (text, ids) = hint_text(&l, "notepad", "change the font size");
        assert!(text.contains("click \"Format\" then click \"Font\"") && text.contains("the font is under the gear"), "{text}");
        assert_eq!(ids.len(), 2);
        assert!(hint_text(&l, "chrome", "change the font").0.is_empty());
        let (other, _) = hint_text(&l, "notepad", "save the file");
        assert!(!other.contains("Format"), "an unrelated goal gets only the always-shown words: {other}");
    }

    #[test]
    fn notes_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!("byte-notes-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(FILE);
        assert!(load(&f).is_empty());
        let n = from_words("notepad", "change the font", "it is under the gear").unwrap();
        save(&f, std::slice::from_ref(&n)).unwrap();
        assert_eq!(load(&f), vec![n]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_note_needs_a_program_and_some_words() {
        assert!(from_words("", "x", "y").is_none());
        assert!(from_words("notepad", "x", "   ").is_none());
    }
}
