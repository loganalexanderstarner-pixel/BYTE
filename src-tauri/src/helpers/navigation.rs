//! Voice navigation (an accessibility mode): "scroll down", "go back", "next tab", "click Send", said to BYTE, move around the
//! program in front without touching the mouse. It uses PC control's own executor, so every step is logged, Stop works, and a
//! click on a risky label (pay, buy, delete...) still asks first. It needs PC control to be on too.

use crate::control::actions::{self, Action, Direction, Expect, Step};

/// One thing to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nav {
    Keys(&'static str),
    Scroll(Direction, u32),
    /// Click the control with this name in the front window.
    Click(String),
}

fn clean(q: &str) -> String {
    q.trim().trim_end_matches(['.', '!', '?']).to_lowercase().replace(',', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The phrase without the polite words that start it.
fn strip(l: &str) -> &str {
    let mut s = l;
    loop {
        let before = s;
        for p in ["please ", "hey byte ", "byte ", "can you ", "could you ", "go ahead and "] {
            if let Some(r) = s.strip_prefix(p) {
                s = r;
            }
        }
        if s == before {
            return s.trim_end_matches(" please");
        }
    }
}

/// What the sentence asks for, if it is a navigation command. Plain "scroll down" / "page down" are PC control's own phrases and are
/// left to it; this adds the rest.
pub fn parse(q: &str) -> Option<Nav> {
    let l = clean(q);
    let l = strip(&l);
    let keys = match l {
        "go back" | "back" | "navigate back" | "go to the previous page" => Some("alt+left"),
        "go forward" | "forward" | "navigate forward" | "go to the next page" => Some("alt+right"),
        "scroll to the top" | "go to the top" | "top of the page" | "scroll to top" => Some("ctrl+home"),
        "scroll to the bottom" | "go to the bottom" | "bottom of the page" | "scroll to bottom" => Some("ctrl+end"),
        "next tab" | "switch to the next tab" => Some("ctrl+tab"),
        "previous tab" | "last tab" | "switch to the previous tab" => Some("ctrl+shift+tab"),
        "new tab" | "open a new tab" => Some("ctrl+t"),
        "close tab" | "close this tab" | "close the tab" => Some("ctrl+w"),
        "refresh" | "reload" | "refresh the page" | "reload the page" | "refresh this page" => Some("f5"),
        "zoom in" => Some("ctrl+="),
        "zoom out" => Some("ctrl+-"),
        "select all" => Some("ctrl+a"),
        "copy" | "copy that" | "copy this" => Some("ctrl+c"),
        "paste" | "paste it" | "paste that" => Some("ctrl+v"),
        "undo" | "undo that" => Some("ctrl+z"),
        _ => None,
    };
    if let Some(k) = keys {
        return Some(Nav::Keys(k));
    }
    for (prefix, dir) in [("scroll down ", Direction::Down), ("scroll up ", Direction::Up)] {
        if let Some(rest) = l.strip_prefix(prefix) {
            let n = match rest {
                "a little" | "a bit" | "slightly" => 2,
                "a lot" | "far" | "a long way" => 14,
                r => r.trim_end_matches(" times").trim_end_matches(" pages").trim_end_matches(" page").parse::<u32>().ok().filter(|n| (1..=50).contains(n))?,
            };
            return Some(Nav::Scroll(dir, n));
        }
    }
    for p in ["click on ", "click ", "tap on ", "tap ", "press the ", "hit the "] {
        if let Some(rest) = l.strip_prefix(p) {
            let name = rest.trim().trim_start_matches("the ").trim_end_matches(" button").trim_end_matches(" link").trim_end_matches(" tab").trim();
            // "press the X" must name a button; "press ctrl s" is a key press for PC control.
            if name.is_empty() || name.split_whitespace().count() > 5 || (p == "press the " && !rest.ends_with(" button")) {
                return None;
            }
            return Some(Nav::Click(name.to_string()));
        }
    }
    None
}

/// The executor step for a navigation that needs no lookup.
pub fn step_for(n: &Nav) -> Option<Step> {
    let action = match n {
        Nav::Keys(c) => Action::Key { combo: (*c).to_string() },
        Nav::Scroll(d, amount) => Action::Scroll { direction: *d, amount: Some(*amount) },
        Nav::Click(_) => return None,
    };
    Some(Step { action, expect: Expect::default() })
}

/// The element a click names: the one whose name is exactly it, else the one that starts with it, else contains it (never a password
/// or card field). None when nothing fits or several fit equally and none is exact.
pub fn pick_element(snap: &crate::control::uia::Snapshot, name: &str) -> Result<u32, String> {
    let want = name.trim().to_lowercase();
    let live: Vec<&crate::control::uia::Element> = snap.elements.iter().filter(|e| !e.sensitive && e.enabled && !e.name.trim().is_empty()).collect();
    let exact: Vec<_> = live.iter().filter(|e| e.name.trim().to_lowercase() == want).collect();
    if exact.len() == 1 {
        return Ok(exact[0].id);
    }
    if exact.len() > 1 {
        return Err(format!("There is more than one \"{name}\" in that window."));
    }
    let starts: Vec<_> = live.iter().filter(|e| e.name.to_lowercase().starts_with(&want)).collect();
    if starts.len() == 1 {
        return Ok(starts[0].id);
    }
    let has: Vec<_> = live.iter().filter(|e| e.name.to_lowercase().contains(&want)).collect();
    match (starts.len(), has.len()) {
        (0, 1) => Ok(has[0].id),
        (0, 0) => Err(format!("I can't see \"{name}\" in that window.")),
        _ => Err(format!("More than one thing there matches \"{name}\". Say more of its name.")),
    }
}

/// The hook in `agent::specialist` (marked `track:helpers`).
pub async fn try_route(turn: &crate::agent::Turn<'_>, q: &str, cancel: &tokio_util::sync::CancellationToken, send: crate::agent::Emit<'_>) -> crate::error::AppResult<Option<(crate::tools::SourceBook, String, &'static str)>> {
    use crate::control::uia::Screen;
    let Some(app) = turn.app else { return Ok(None) };
    let Some(nav) = parse(crate::router::without_wake_phrase(q)) else { return Ok(None) };
    let Some(settings) = crate::control::settings_now(app) else { return Ok(None) };
    if !settings.helper_nav || !settings.pc_control || settings.kids_mode || !cfg!(windows) {
        return Ok(None);
    }
    let said = |t: String| Ok(Some((Default::default(), t, "house")));
    let step = match step_for(&nav) {
        Some(s) => s,
        None => {
            let Nav::Click(name) = &nav else { return Ok(None) };
            let screen: std::sync::Arc<dyn Screen> = crate::control::real_screen().into();
            let wins = { let s = screen.clone(); tokio::task::spawn_blocking(move || s.windows()).await.unwrap_or_default() };
            let Some(target) = crate::control::uia::pick_target(&wins).cloned() else { return said("There is no window to click in.".into()) };
            if let Err(why) = crate::control::guide::allowed(&target.app, &target.title, &settings.screen_exclude_apps) {
                return said(why);
            }
            let id = target.id;
            let snap = match tokio::task::spawn_blocking(move || screen.snapshot_window(id)).await {
                Ok(Ok(s)) => s,
                _ => return said("BYTE couldn't read that window.".into()),
            };
            match pick_element(&snap, name) {
                Ok(n) => Step { action: Action::Click { id: n }, expect: Expect::default() },
                Err(why) => return said(why),
            }
        }
    };
    let report = crate::control::run_plan_steps(app, vec![step], &format!("Voice navigation: {q}"), cancel, send).await;
    said(if report.summary.trim().is_empty() { "Done.".to_string() } else { report.summary })
}

#[allow(dead_code)]
fn _uses(_: &Action) -> bool {
    matches!(actions::risk(&Action::Key { combo: "f5".into() }), actions::Risk::Safe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::uia::{Element, Kind, Rect, Snapshot};

    fn el(id: u32, name: &str) -> Element {
        Element { id, name: name.into(), kind: Kind::Button, enabled: true, rect: Rect { left: 0, top: 0, right: 10, bottom: 10 }, sensitive: false }
    }

    #[test]
    fn navigation_phrases_become_keys_scrolls_and_clicks() {
        assert_eq!(parse("Go back."), Some(Nav::Keys("alt+left")));
        assert_eq!(parse("hey byte, please go forward"), Some(Nav::Keys("alt+right")));
        assert_eq!(parse("scroll to the bottom"), Some(Nav::Keys("ctrl+end")));
        assert_eq!(parse("next tab"), Some(Nav::Keys("ctrl+tab")));
        assert_eq!(parse("reload the page"), Some(Nav::Keys("f5")));
        assert_eq!(parse("scroll down 5"), Some(Nav::Scroll(Direction::Down, 5)));
        assert_eq!(parse("scroll up a lot"), Some(Nav::Scroll(Direction::Up, 14)));
        assert_eq!(parse("click Send"), Some(Nav::Click("send".into())));
        assert_eq!(parse("Click on the Sign in button"), Some(Nav::Click("sign in".into())));
        assert_eq!(parse("press the Save button"), Some(Nav::Click("save".into())));
    }

    #[test]
    fn ordinary_sentences_and_pc_controls_own_phrases_are_not_taken() {
        for q in ["scroll down", "scroll up", "press ctrl s", "press the", "what is the best way to go back in time", "click", "scroll down 99", "I will click the thing next week when I have time to try it", "go back to notepad"] {
            assert_eq!(parse(q), None, "{q}");
        }
    }

    #[test]
    fn every_key_a_phrase_can_press_passes_pc_controls_own_checks() {
        for q in ["go back", "go forward", "scroll to the top", "scroll to the bottom", "next tab", "previous tab", "new tab", "close tab", "refresh", "zoom in", "zoom out", "select all", "copy", "paste", "undo"] {
            let n = parse(q).unwrap_or_else(|| panic!("{q}"));
            let s = step_for(&n).unwrap();
            assert!(actions::validate(&s.action).is_ok(), "{q}: {:?}", actions::validate(&s.action));
            assert_ne!(actions::risk(&s.action), actions::Risk::Never, "{q}");
        }
    }

    #[test]
    fn a_click_finds_one_clear_control_and_says_when_it_cannot() {
        let snap = Snapshot { app: "mail".into(), title: "Inbox".into(), elements: vec![el(1, "Send"), el(2, "Send later"), el(3, "Discard"), el(4, "Reply")] };
        assert_eq!(pick_element(&snap, "send"), Ok(1));
        assert_eq!(pick_element(&snap, "disc"), Ok(3));
        assert_eq!(pick_element(&snap, "rep"), Ok(4));
        assert!(pick_element(&snap, "forward").unwrap_err().contains("can't see"));
        let two = Snapshot { elements: vec![el(1, "Save as"), el(2, "Save all")], ..snap.clone() };
        assert!(pick_element(&two, "save").unwrap_err().contains("More than one"));
        let dup = Snapshot { elements: vec![el(1, "OK"), el(2, "OK")], ..snap.clone() };
        assert!(pick_element(&dup, "ok").unwrap_err().contains("more than one"));
    }

    #[test]
    fn password_fields_and_disabled_controls_are_never_clicked() {
        let mut secret = el(1, "Password");
        secret.sensitive = true;
        let mut off = el(2, "Continue");
        off.enabled = false;
        let snap = Snapshot { app: "x".into(), title: "y".into(), elements: vec![secret, off] };
        assert!(pick_element(&snap, "password").is_err());
        assert!(pick_element(&snap, "continue").is_err());
    }
}
