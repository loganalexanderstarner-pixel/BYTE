//! Dry run: BYTE plans a job and lists what it would click, type and press, without doing any of it. Asked for by the setting
//! "Dry run" (every PC-control job) or by saying "dry run: open Notepad and type hello" in the chat.

use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::AppResult;
use crate::tools::SourceBook;

/// "dry run: open notepad", "Dry-run open notepad", "dryrun - open notepad": the goal after the words, or `None`.
pub fn dry_goal(q: &str) -> Option<String> {
    let t = q.trim();
    let low = t.to_lowercase();
    let rest = ["dry run", "dry-run", "dryrun"].iter().find_map(|p| low.strip_prefix(p).map(|_| &t[p.len()..]))?;
    // The words must stand alone ("dry running" is not it).
    if rest.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    let goal = rest.trim_start_matches(|c: char| c == ':' || c == '-' || c == ',' || c.is_whitespace()).trim();
    (!goal.is_empty()).then(|| goal.to_string())
}

/// The text a dry run shows and the model is told.
pub fn card_text(goal: &str, lines: &[String]) -> (String, String) {
    (format!("Dry run: {goal}"), format!("Nothing was done. BYTE would:\n{}", lines.join("\n")))
}

/// The chat route: "dry run: …" plans from the window in front and shows what would happen.
pub async fn route(turn: &Turn<'_>, q: &str, _cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(app) = turn.app else { return Ok(None) };
    if !turn.modules.control.pc_control {
        return Ok(None);
    }
    let Some(goal) = dry_goal(crate::router::without_wake_phrase(q)) else { return Ok(None) };
    match super::plan_goal(app, &goal).await {
        Ok((steps, snap)) => {
            let lines = super::plancheck::dry_lines(&steps, snap.as_ref());
            let (title, detail) = card_text(&goal, &lines);
            super::activity(app, "Dry run", true, &format!("{} steps for: {goal}", steps.len()));
            let id = format!("byte_pc_{}", uuid::Uuid::new_v4().simple());
            send(ChatEvent::ToolCall { id: id.clone(), name: "pc_control".into(), args: serde_json::json!({ "request": title }) })?;
            send(ChatEvent::ToolResult { id, ok: true, summary: title.clone() })?;
            send(ChatEvent::MacDone(crate::macctl::MacDone { app: "PC".into(), title, detail: detail.clone(), ok: true, undo: None }))?;
            Ok(Some((SourceBook::default(), format!("The user asked for a dry run. BYTE planned these steps and did NOT do any of them; a card shows them. Say in one or two short sentences what BYTE would do, and that nothing was done, and offer to do it for real if they say so.\n{detail}"))))
        }
        Err(e) => {
            super::activity(app, "Dry run", false, &e);
            Ok(Some((SourceBook::default(), format!("The user asked for a dry run, but BYTE could not plan it: {e}. Tell them that in one short sentence."))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dry_run_words_are_read_with_or_without_punctuation() {
        assert_eq!(dry_goal("dry run: open notepad").as_deref(), Some("open notepad"));
        assert_eq!(dry_goal("Dry-run open Notepad and type hi").as_deref(), Some("open Notepad and type hi"));
        assert_eq!(dry_goal("dryrun - change the font").as_deref(), Some("change the font"));
        assert_eq!(dry_goal("dry run"), None);
        assert_eq!(dry_goal("dry running shoes"), None);
        assert_eq!(dry_goal("open notepad"), None);
    }

    #[test]
    fn the_card_says_nothing_was_done() {
        let (t, d) = card_text("open notepad", &["1. Open notepad".into()]);
        assert_eq!(t, "Dry run: open notepad");
        assert!(d.starts_with("Nothing was done") && d.contains("1. Open notepad"));
    }
}
