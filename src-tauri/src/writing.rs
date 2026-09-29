//! The writing studio: rewrite, expand, shorten, change the tone of, or fix the
//! grammar of a piece of text. The result streams back as `ChatEvent::Content`;
//! the studio panel shows it next to the original with the changes marked.

use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::State;

use crate::chat::{self, ChatEvent, ChatMessage};
use crate::error::{AppError, AppResult};
use crate::settings::Mode;
use crate::state::AppState;

/// Longest text the studio takes at once (about 6,000 words).
pub const MAX_CHARS: usize = 40_000;

const WRITER: &str = "You are BYTE's writing editor. You change the user's text exactly as asked and reply with only the \
new text: no preface like \"Here is\", no quotes around it, no notes after it, no code fences. Keep the author's meaning, \
facts, names and numbers, the language they wrote in, their paragraph breaks and any Markdown formatting.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Rewrite,
    Expand,
    Shorten,
    Tone,
    Grammar,
}

/// What the model is asked to do.
pub fn instructions(action: Action, tone: Option<&str>) -> String {
    match action {
        Action::Rewrite => "Rewrite this text so it reads more clearly and naturally. Keep the same meaning and roughly the same length.".into(),
        Action::Expand => "Expand this text to about twice its length: add useful detail, examples or explanation in the same voice. \
Don't invent facts about real people, dates, prices or numbers."
            .into(),
        Action::Shorten => "Shorten this text to about half its length. Keep every key point; cut repetition, filler and hedging.".into(),
        Action::Tone => {
            let t = tone.unwrap_or("friendly").to_lowercase();
            let how = match t.as_str() {
                "formal" => "formal and professional: polite, precise, no slang or contractions",
                "confident" => "confident and direct: clear statements, active voice, no hedging",
                "simple" => "simple and plain: short sentences and everyday words a 12-year-old would understand",
                "persuasive" => "persuasive: lead with the benefit, give reasons, end with a clear ask",
                _ => "warm and friendly: conversational, kind, like talking to a friend",
            };
            format!("Rewrite this text so its tone is {how}. Keep the same meaning and roughly the same length.")
        }
        Action::Grammar => "Fix spelling, grammar and punctuation only. Change nothing else: not the wording, the style or the meaning. \
If it's already correct, return it exactly as it is."
            .into(),
    }
}

/// Room for the new text: its length changes with the action.
pub fn max_tokens(action: Action, text: &str) -> u32 {
    let tokens = (text.chars().count() as f64 / 3.5).ceil();
    let want = match action {
        Action::Expand => tokens * 2.6 + 300.0,
        Action::Shorten => tokens * 0.8 + 150.0,
        _ => tokens * 1.4 + 200.0,
    };
    want.clamp(200.0, 8000.0) as u32
}

/// The request the model sees.
pub fn user_message(action: Action, tone: Option<&str>, text: &str) -> String {
    format!("{}\n\nText:\n<<<\n{}\n>>>", instructions(action, tone), text.trim())
}

/// The engine request for one studio action (thinking off, room for the result).
pub fn request_body(action: Action, tone: Option<&str>, text: &str, profile: crate::modelcfg::ModelProfile) -> serde_json::Value {
    let mut plan = crate::router::plan_turn(Mode::Fast, crate::settings::ThinkingPref::Off, "").for_model(profile);
    plan.max_tokens = max_tokens(action, text);
    let messages = chat::base_messages(WRITER, &[ChatMessage::new("user", user_message(action, tone, text))]);
    let mut body = chat::build_body(messages, plan, None);
    body["stream"] = true.into();
    if action == Action::Grammar {
        // Grammar fixes should be the same every time.
        body["temperature"] = 0.1.into();
    }
    body
}

/// Runs one studio action; the new text streams as `Content`, then `Done`.
#[tauri::command]
pub async fn writing_run(state: State<'_, AppState>, request_id: String, text: String, action: Action, tone: Option<String>, on_event: Channel<ChatEvent>) -> AppResult<()> {
    if text.trim().is_empty() {
        return Err(AppError::msg("Type or paste some text first."));
    }
    if text.chars().count() > MAX_CHARS {
        return Err(AppError::msg("That's a lot of text: the writing studio takes up to about 6,000 words at a time. Select a part of it."));
    }
    let Some(ep) = state.engine.endpoint().await else {
        // No model on this Mac: the BYTE cloud writes it (one helper conversation, deleted after).
        let helper = crate::backend::cloud_cards(&state).await.and_then(|ep| ep.cloud).ok_or_else(|| AppError::msg("Load a model first (Settings → Models), or connect BYTE Cloud."))?;
        let _ = on_event.send(ChatEvent::Started { thinking: false, model: "BYTE Cloud".into() });
        let out = helper.text(WRITER, &user_message(action, tone.as_deref(), &text)).await?;
        let _ = on_event.send(ChatEvent::Content { delta: out });
        let _ = on_event.send(ChatEvent::Done { finish_reason: "stop".into() });
        return Ok(());
    };
    let catalog = state.catalog.get();
    let profile = catalog.resolve(&ep.model).map(|(m, _)| crate::modelcfg::profile(m)).unwrap_or_default();
    let body = request_body(action, tone.as_deref(), &text, profile);
    let cancel = state.generations.register(&request_id).await;
    let _ = on_event.send(ChatEvent::Started { thinking: false, model: ep.model.clone() });
    let mut forward = |e: ChatEvent| -> AppResult<()> {
        // The studio shows only the new text.
        if matches!(e, ChatEvent::Content { .. }) {
            let _ = on_event.send(e);
        }
        Ok(())
    };
    let r = chat::stream_round(&state.local_http, &ep, &body, &cancel, &mut forward).await;
    state.generations.finish(&request_id).await;
    let finish = match r {
        Ok(round) => round.finish,
        Err(AppError::Cancelled) => "cancelled".into(),
        Err(e) => return Err(e),
    };
    let _ = on_event.send(ChatEvent::Done { finish_reason: finish });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_action_asks_for_the_right_change() {
        assert!(instructions(Action::Shorten, None).contains("half"));
        assert!(instructions(Action::Expand, None).contains("twice"));
        assert!(instructions(Action::Grammar, None).contains("Change nothing else"));
        assert!(instructions(Action::Tone, Some("Formal")).contains("formal and professional"));
        assert!(instructions(Action::Tone, Some("pirate")).contains("friendly"), "unknown tones fall back to friendly");
        let m = user_message(Action::Rewrite, None, "  hello there  ");
        assert!(m.ends_with("<<<\nhello there\n>>>"));
    }

    #[test]
    fn room_for_the_result_follows_the_action() {
        let text = "word ".repeat(700); // ~1,000 tokens
        let (short, same, long) = (max_tokens(Action::Shorten, &text), max_tokens(Action::Rewrite, &text), max_tokens(Action::Expand, &text));
        assert!(short < same && same < long, "{short} {same} {long}");
        assert!((200..=205).contains(&max_tokens(Action::Grammar, "Hi.")), "short texts get the minimum room");
        assert_eq!(max_tokens(Action::Expand, &"x".repeat(MAX_CHARS)), 8000);
    }

    /// Real engine: shorten makes it shorter, grammar fixes the typos and little else.
    #[tokio::test]
    #[ignore]
    async fn e2e_writing() {
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let http = chat::local_client();
        let run = |action: Action, text: &'static str| {
            let (http, ep) = (http.clone(), ep.clone());
            async move {
                let body = request_body(action, None, text, Default::default());
                let mut out = String::new();
                let mut f = |e: ChatEvent| {
                    if let ChatEvent::Content { delta } = e {
                        out.push_str(&delta);
                    }
                    Ok(())
                };
                chat::stream_round(&http, &ep, &body, &tokio_util::sync::CancellationToken::new(), &mut f).await.unwrap();
                out
            }
        };
        let long = "I am writing to let you know that the meeting that we had planned for Tuesday afternoon has been moved, \
because several people on the team said that they would not be able to attend at that time, so the new time for the meeting \
is now Thursday morning at ten o'clock in the same room as before, and please let me know if that does not work for you.";
        let short = run(Action::Shorten, long).await;
        eprintln!("shorten: {short}");
        assert!(short.len() < long.len() * 3 / 4 && short.to_lowercase().contains("thursday"), "{short}");
        let fixed = run(Action::Grammar, "Their going to the libary tomorow to returns the books.").await;
        eprintln!("grammar: {fixed}");
        let f = fixed.to_lowercase();
        assert!(f.contains("library") && f.contains("tomorrow") && !f.contains("libary"), "{fixed}");
    }

    #[test]
    fn actions_come_from_the_ui_in_camel_case() {
        assert_eq!(serde_json::from_str::<Action>("\"grammar\"").unwrap(), Action::Grammar);
        assert!(serde_json::from_str::<Action>("\"Grammar\"").is_err());
    }
}
