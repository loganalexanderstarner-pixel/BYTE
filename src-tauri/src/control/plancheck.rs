//! Checks a plan against the window it was made for, before anything runs: every click must point at an item that is in the list,
//! a switch must name a window that is open (or one the plan opens first), and a plan that says it cannot is read as that.
//! Pure functions; the errors are worded for the model to fix its plan on the next try.

use serde_json::Value;

use super::actions::{self, Action, PlanError, Step};
use super::uia::{Kind, Snapshot};

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The items a model was shown (the same ones `uia::render` lists): not secret fields, and named ones (or fields to type in).
pub fn listed(snap: &Snapshot) -> Vec<(u32, String, Kind)> {
    snap.elements
        .iter()
        .filter(|e| !e.sensitive && (!e.name.trim().is_empty() || e.kind == Kind::Edit))
        .take(super::uia::MAX_MARKS)
        .map(|e| (e.id, one_line(&e.name), e.kind))
        .collect()
}

/// A few of the real items, for an error message: `1 button "File", 2 edit "Text editor"`.
fn some_items(snap: &Snapshot, n: usize) -> String {
    let items = listed(snap);
    let mut out: Vec<String> = items.iter().take(n).map(|(id, name, k)| if name.is_empty() { format!("{id} {}", k.word()) } else { format!("{id} {} \"{name}\"", k.word()) }).collect();
    if items.len() > n {
        out.push(format!("... {} more", items.len() - n));
    }
    if out.is_empty() {
        "(none)".into()
    } else {
        out.join(", ")
    }
}

/// Whether a step may change what the window shows, so the numbers after it belong to a list the model has not seen yet.
pub(crate) fn changes_view(s: &Step) -> bool {
    match &s.action {
        Action::Open { .. } | Action::Focus { .. } | Action::Close { .. } | Action::Key { .. } | Action::Wait { .. } | Action::Power { .. } => true,
        Action::Click { .. } => s.expect.window.is_some() || s.expect.element.is_some(),
        Action::Type { .. } | Action::Scroll { .. } => false,
    }
}

/// A window title in `windows` that `want` plainly means (the same text in other words, such as "the Notepad window"): one word of
/// the wanted title that is in exactly one open window's title.
fn repair_title(want: &str, windows: &[String]) -> Option<String> {
    let low = want.to_lowercase();
    if let Some(w) = windows.iter().find(|w| w.to_lowercase().contains(&low)) {
        return Some(w.clone());
    }
    let mut hit: Option<&String> = None;
    for word in low.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 4 && !matches!(*w, "window" | "the" | "this" | "that" | "program" | "app")) {
        let found: Vec<&String> = windows.iter().filter(|t| t.to_lowercase().contains(word)).collect();
        if found.len() == 1 {
            match hit {
                Some(h) if h != found[0] => return None,
                _ => hit = Some(found[0]),
            }
        }
    }
    hit.cloned()
}

/// Checks `steps` against `snap` (the window in front when the plan was made) and `windows` (titles open now; empty when they
/// could not be read, which skips the switch check). Returns the steps, with a switch repaired to the real window title when the
/// model's words plainly mean one window, or the first problem.
pub fn check_plan(steps: Vec<Step>, snap: &Snapshot, windows: &[String]) -> Result<Vec<Step>, PlanError> {
    let mut fresh = true;
    let mut opened_here = false;
    let mut out = Vec::with_capacity(steps.len());
    for (i, mut s) in steps.into_iter().enumerate() {
        let n = i + 1;
        match &s.action {
            Action::Click { id } if fresh => match snap.find(*id) {
                None => {
                    let max = snap.elements.iter().map(|e| e.id).max().unwrap_or(0);
                    return Err(PlanError(format!(
                        "step {n}: item {id} is not in the list (the numbers go up to {max}). Items now: {}. Use only a number from that list; if the button you need is not there, plan only up to the step that shows it",
                        some_items(snap, 12)
                    )));
                }
                Some(e) if e.sensitive => return Err(PlanError(format!("step {n}: item {id} is a private field and is never used"))),
                Some(e) if !e.enabled => return Err(PlanError(format!("step {n}: item {id} (\"{}\") is disabled right now; pick another way", one_line(&e.name)))),
                Some(_) => {}
            },
            Action::Focus { title } if !opened_here && !windows.is_empty() => match repair_title(title, windows) {
                Some(real) => {
                    if real != *title {
                        s.action = Action::Focus { title: real };
                    }
                }
                None => {
                    let shown: Vec<String> = windows.iter().take(8).map(|w| format!("\"{}\"", one_line(w))).collect();
                    return Err(PlanError(format!("step {n}: no open window matches \"{title}\". Open windows: {}. Open the program first, or use one of these titles", shown.join(", "))));
                }
            },
            Action::Open { .. } => opened_here = true,
            _ => {}
        }
        if changes_view(&s) {
            fresh = false;
        }
        out.push(s);
    }
    Ok(out)
}

/// A reply of `{"steps":[],"cannot":"why"}`: the model says the goal cannot be done. The reason, trimmed.
pub fn cannot_reason(text: &str) -> Option<String> {
    let json = actions::extract_json(text)?;
    let v: Value = serde_json::from_str(json).ok()?;
    let o = v.as_object()?;
    let empty = o.get("steps").and_then(Value::as_array).map(|a| a.is_empty()).unwrap_or(true);
    let why = one_line(o.get("cannot")?.as_str()?);
    (empty && !why.is_empty()).then(|| why.chars().take(200).collect())
}

/// What to say to the model when its reply was refused: the exact error, and for a plan that was too long, how to shorten it.
pub fn fix_text(error: &str, limit: usize) -> String {
    if error.contains("steps; at most") {
        let cap = if limit == usize::MAX { String::new() } else { format!(" to at most {limit} steps") };
        format!(
            "That plan was refused: {error}. Shorten it{cap}: plan only up to the first step that opens a window or changes what is on screen, add \"expect\" to that step, and you will be shown the new list to plan the rest. Reply again with only the corrected JSON object."
        )
    } else {
        format!("That plan was refused: {error}. Reply again with only the corrected JSON object.")
    }
}

/// One line per step for a dry run: what BYTE would click (by name), type, press, open or close. Typed text is shown whole.
pub fn dry_lines(steps: &[Step], snap: Option<&Snapshot>) -> Vec<String> {
    let mut fresh = true;
    let mut lines = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        let text = match &s.action {
            Action::Click { id } => match snap.filter(|_| fresh).and_then(|sn| sn.find(*id)) {
                Some(e) if !e.name.trim().is_empty() => format!("Click the {} \"{}\"", e.kind.word(), one_line(&e.name)),
                _ => format!("Click item {id}"),
            },
            Action::Type { text } => {
                let shown: String = text.chars().take(120).collect();
                format!("Type: {shown}{}", if text.chars().count() > 120 { "…" } else { "" })
            }
            other => actions::describe(other),
        };
        if changes_view(s) {
            fresh = false;
        }
        lines.push(format!("{}. {text}", i + 1));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::uia::fake::{el, snap, FakeScreen};
    use crate::control::uia::Screen;

    fn click(id: u32) -> Step {
        Step { action: Action::Click { id }, expect: Default::default() }
    }

    fn screen() -> Snapshot {
        let mut disabled = el(4, "Paste", Kind::Button);
        disabled.enabled = false;
        let mut secret = el(5, "Password", Kind::Edit);
        secret.sensitive = true;
        FakeScreen::new(vec![snap("Untitled - Notepad", vec![el(1, "File", Kind::MenuItem), el(2, "Edit", Kind::MenuItem), el(3, "Text editor", Kind::Edit), disabled, secret])]).snapshot().unwrap()
    }

    #[test]
    fn a_click_on_an_item_that_is_there_passes() {
        assert_eq!(check_plan(vec![click(1), click(2)], &screen(), &[]).unwrap().len(), 2);
    }

    #[test]
    fn a_click_on_an_item_that_is_not_there_is_refused_with_the_real_list() {
        let e = check_plan(vec![click(1), click(17)], &screen(), &[]).unwrap_err().0;
        assert!(e.starts_with("step 2: item 17 is not in the list"), "{e}");
        assert!(e.contains("1 menu \"File\"") && e.contains("3 edit \"Text editor\""), "{e}");
        assert!(!e.contains("Password"), "a secret field is never listed: {e}");
    }

    #[test]
    fn disabled_and_secret_items_are_refused() {
        assert!(check_plan(vec![click(4)], &screen(), &[]).unwrap_err().0.contains("disabled"));
        assert!(check_plan(vec![click(5)], &screen(), &[]).unwrap_err().0.contains("private"));
    }

    #[test]
    fn numbers_after_a_step_that_changes_the_window_are_not_judged_by_the_old_list() {
        let open = Step { action: Action::Open { target: "calc".into() }, expect: Default::default() };
        assert!(check_plan(vec![open, click(99)], &screen(), &[]).is_ok());
        let mut c = click(1);
        c.expect.window = Some("Save".into());
        assert!(check_plan(vec![c, click(99)], &screen(), &[]).is_ok(), "a click that expects a new window ends the list");
        assert!(check_plan(vec![click(1), click(99)], &screen(), &[]).is_err(), "a plain click keeps the list");
    }

    #[test]
    fn a_switch_to_a_window_that_is_not_open_is_refused_unless_the_plan_opens_it() {
        let windows = vec!["Untitled - Notepad".to_string(), "Google Chrome".to_string()];
        let focus = |t: &str| Step { action: Action::Focus { title: t.into() }, expect: Default::default() };
        assert!(check_plan(vec![focus("chrome")], &screen(), &windows).is_ok());
        let e = check_plan(vec![focus("Spotify")], &screen(), &windows).unwrap_err().0;
        assert!(e.contains("no open window matches \"Spotify\"") && e.contains("Google Chrome"), "{e}");
        let open = Step { action: Action::Open { target: "spotify".into() }, expect: Default::default() };
        assert!(check_plan(vec![open, focus("Spotify")], &screen(), &windows).is_ok());
        assert!(check_plan(vec![focus("Spotify")], &screen(), &[]).is_ok(), "windows that could not be read skip the check");
    }

    #[test]
    fn a_switch_in_other_words_is_repaired_to_the_real_title() {
        let windows = vec!["Untitled - Notepad".to_string(), "Google Chrome".to_string()];
        let s = Step { action: Action::Focus { title: "the Notepad window".into() }, expect: Default::default() };
        let fixed = check_plan(vec![s], &screen(), &windows).unwrap();
        assert_eq!(fixed[0].action, Action::Focus { title: "Untitled - Notepad".into() });
    }

    #[test]
    fn a_plan_that_says_it_cannot_is_read_as_that() {
        assert_eq!(cannot_reason(r#"{"steps":[],"cannot":"I should not type your bank password"}"#).as_deref(), Some("I should not type your bank password"));
        assert_eq!(cannot_reason(r#"{"steps":[{"action":"open","target":"notepad"}],"cannot":"x"}"#), None);
        assert_eq!(cannot_reason(r#"{"steps":[]}"#), None);
        assert_eq!(cannot_reason("no json"), None);
    }

    #[test]
    fn the_fix_text_carries_the_exact_error_and_how_to_shorten_a_long_plan() {
        let f = fix_text("the plan has 20 steps; at most 12", 12);
        assert!(f.contains("the plan has 20 steps; at most 12") && f.contains("at most 12 steps") && f.contains("expect"), "{f}");
        assert!(!fix_text("the plan has 20 steps; at most 12", usize::MAX).contains("at most 18446"));
        let g = fix_text("step 2: item 17 is not in the list", 12);
        assert!(g.contains("step 2: item 17 is not in the list") && !g.contains("Shorten"));
    }

    #[test]
    fn a_dry_run_lists_names_for_clicks_and_whole_text_for_typing() {
        let steps = vec![click(1), Step { action: Action::Type { text: "hello".into() }, expect: Default::default() }];
        let l = dry_lines(&steps, Some(&screen()));
        assert_eq!(l[0], "1. Click the menu \"File\"");
        assert_eq!(l[1], "2. Type: hello");
    }
}
