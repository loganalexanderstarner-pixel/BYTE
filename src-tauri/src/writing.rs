//! The writing studio: rewrite, expand, shorten, change the tone of, or fix the
//! grammar of a piece of text. The result streams back as `ChatEvent::Content`;
//! the studio panel shows it next to the original with the changes marked.

use serde::{Deserialize, Serialize};
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
    /// Into the language given as `tone`.
    Translate,
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
        Action::Translate => format!(
            "Translate this text into {}. Translate everything, keep the meaning, tone, names, numbers and formatting.",
            tone.filter(|t| !t.trim().is_empty()).unwrap_or("English")
        ),
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
        // Other scripts can take more tokens than the original.
        Action::Translate => tokens * 2.2 + 300.0,
        _ => tokens * 1.4 + 200.0,
    };
    want.clamp(200.0, 8000.0) as u32
}

/// The request the model sees.
pub fn user_message(action: Action, tone: Option<&str>, text: &str) -> String {
    format!("{}\n\nText:\n<<<\n{}\n>>>", instructions(action, tone), text.trim())
}

/// The engine request for one studio action (what `writing_run` sends; for tests).
#[cfg(test)]
pub fn request_body(action: Action, tone: Option<&str>, text: &str, profile: crate::modelcfg::ModelProfile) -> serde_json::Value {
    let temperature = if action == Action::Grammar { Some(0.1) } else { None };
    text_body(WRITER, &user_message(action, tone, text), max_tokens(action, text), temperature, profile)
}

/// Runs one studio action; the new text streams as `Content`, then `Done`.
/// `like_me`: write in the user's saved style (Settings: "Teach BYTE your style").
#[tauri::command]
pub async fn writing_run(state: State<'_, AppState>, request_id: String, text: String, action: Action, tone: Option<String>, like_me: Option<bool>, on_event: Channel<ChatEvent>) -> AppResult<()> {
    if text.trim().is_empty() {
        return Err(AppError::msg("Type or paste some text first."));
    }
    if text.chars().count() > MAX_CHARS {
        return Err(AppError::msg("That's a lot of text: the writing studio takes up to about 6,000 words at a time. Select a part of it."));
    }
    // Grammar fixes keep the author's own style by definition.
    let style = if like_me.unwrap_or(false) && action != Action::Grammar { style_of(&state).await } else { String::new() };
    let system = format!("{WRITER}{}", style_rules(&style));
    let temperature = if action == Action::Grammar { Some(0.1) } else { None };
    stream_text(&state, &request_id, &system, &user_message(action, tone.as_deref(), &text), max_tokens(action, &text), temperature, &on_event).await
}

/// Streams one request's text (thinking off) from the model on this Mac, or the BYTE
/// cloud when none is loaded; then `Done`.
async fn stream_text(state: &AppState, request_id: &str, system: &str, user: &str, max_tokens: u32, temperature: Option<f64>, on_event: &Channel<ChatEvent>) -> AppResult<()> {
    let Some(ep) = state.engine.endpoint().await else {
        // No model on this Mac: the BYTE cloud writes it (one helper conversation, deleted after).
        let helper = crate::backend::cloud_cards(state).await.and_then(|ep| ep.cloud).ok_or_else(|| AppError::msg("Load a model first (Settings → Models), or connect BYTE Cloud."))?;
        let _ = on_event.send(ChatEvent::Started { thinking: false, model: "BYTE Cloud".into() });
        let out = helper.text(system, user).await?;
        let _ = on_event.send(ChatEvent::Content { delta: out });
        let _ = on_event.send(ChatEvent::Done { finish_reason: "stop".into() });
        return Ok(());
    };
    let catalog = state.catalog.get();
    let profile = catalog.resolve(&ep.model).map(|(m, _)| crate::modelcfg::profile(m)).unwrap_or_default();
    let body = text_body(system, user, max_tokens, temperature, profile);
    let cancel = state.generations.register(request_id).await;
    let _ = on_event.send(ChatEvent::Started { thinking: false, model: ep.model.clone() });
    let mut forward = |e: ChatEvent| -> AppResult<()> {
        // The studio shows only the new text.
        if matches!(e, ChatEvent::Content { .. }) {
            let _ = on_event.send(e);
        }
        Ok(())
    };
    let r = chat::stream_round(&state.local_http, &ep, &body, &cancel, &mut forward).await;
    state.generations.finish(request_id).await;
    let finish = match r {
        Ok(round) => round.finish,
        Err(AppError::Cancelled) => "cancelled".into(),
        Err(e) => return Err(e),
    };
    let _ = on_event.send(ChatEvent::Done { finish_reason: finish });
    Ok(())
}

/// A streaming request with thinking off.
fn text_body(system: &str, user: &str, max_tokens: u32, temperature: Option<f64>, profile: crate::modelcfg::ModelProfile) -> serde_json::Value {
    let mut plan = crate::router::plan_turn(Mode::Fast, crate::settings::ThinkingPref::Off, "").for_model(profile);
    plan.max_tokens = max_tokens;
    let mut body = chat::build_body(chat::base_messages(system, &[ChatMessage::new("user", user)]), plan, None);
    body["stream"] = true.into();
    if let Some(t) = temperature {
        body["temperature"] = t.into();
    }
    body
}

// ---------- your style ("Write like me") ----------

async fn style_of(state: &AppState) -> String {
    state.settings.lock().await.writing_style.clone()
}

/// The saved style as instructions for the writer.
pub fn style_rules(style: &str) -> String {
    let s = style.trim();
    if s.is_empty() {
        return String::new();
    }
    format!("\n\nWrite it the way the user writes. Their style:\n{s}\nMatch their voice and habits; don't copy their example sentences.")
}

fn style_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "voice": { "type": "string" },
            "sentences": { "type": "string" },
            "words": { "type": "string" },
            "habits": { "type": "array", "maxItems": 6, "items": { "type": "string" } },
            "avoid": { "type": "array", "maxItems": 4, "items": { "type": "string" } }
        },
        "required": ["voice", "sentences", "words", "habits", "avoid"]
    })
}

/// Does `text` repeat 6 or more words in a row from one of the samples (a small model
/// copying the sample instead of describing it)?
fn copies(text: &str, samples: &[String]) -> bool {
    let norm = |t: &str| t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let w = norm(text);
    let joined: Vec<String> = samples.iter().map(|s| format!(" {} ", norm(s).join(" "))).collect();
    w.windows(6).any(|win| {
        let needle = format!(" {} ", win.join(" "));
        joined.iter().any(|s| s.contains(&needle))
    })
}

/// A short style profile from the model's reading of the samples (parts that just
/// copy the samples are left out).
pub fn parse_style(reply: &str, samples: &[String]) -> Option<String> {
    let v = crate::research::lenient_json(reply);
    let line = |k: &str| v[k].as_str().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty() && !copies(s, samples));
    let list = |k: &str| {
        v[k].as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).map(str::trim).filter(|x| !x.is_empty() && !copies(x, samples)).map(str::to_string).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for (label, key) in [("Voice", "voice"), ("Sentences", "sentences"), ("Words", "words")] {
        if let Some(t) = line(key) {
            out.push(format!("- {label}: {t}"));
        }
    }
    let habits = list("habits");
    if !habits.is_empty() {
        out.push(format!("- Habits: {}", habits.join("; ")));
    }
    let avoid = list("avoid");
    if !avoid.is_empty() {
        out.push(format!("- Never: {}", avoid.join("; ")));
    }
    (out.len() >= 2).then(|| out.join("\n"))
}

/// Reads 1–3 samples of the user's writing and saves a style profile (returned for the UI).
#[tauri::command]
pub async fn style_learn(state: State<'_, AppState>, samples: Vec<String>) -> AppResult<String> {
    let samples: Vec<String> = samples.into_iter().map(|s| s.trim().chars().take(6000).collect::<String>()).filter(|s| s.split_whitespace().count() >= 30).take(3).collect();
    if samples.is_empty() {
        return Err(AppError::msg("Paste at least one piece you wrote yourself (a few paragraphs: an email, an essay, a post)."));
    }
    let ep = match state.engine.endpoint().await {
        Some(ep) => ep,
        None => crate::backend::cloud_cards(&state).await.ok_or_else(|| AppError::msg("Load a model first (Settings → Models), or connect BYTE Cloud."))?,
    };
    let joined = samples.iter().enumerate().map(|(i, s)| format!("Sample {}:\n<<<\n{s}\n>>>", i + 1)).collect::<Vec<_>>().join("\n\n");
    let user = format!(
        "{joined}\n\nDescribe how this person writes, so a writer could imitate them: their voice (formal? warm? funny?), sentence \
length and rhythm, word choice, habits (how they open and close, punctuation, emoji, lists, contractions), and what they never do. \
Be specific and short; describe the style, not the topics."
    );
    let sys = "You are an expert editor who describes writing styles precisely. Reply only with JSON.";
    let mut profile = None;
    for attempt in 0..2 {
        let ask = if attempt == 0 { user.clone() } else { format!("{user}\nDescribe it in your own words: don't quote the samples.") };
        let reply = chat::complete_json(&state.local_http, &ep, sys, &ask, style_schema(), 500).await?;
        profile = parse_style(&reply, &samples);
        if profile.is_some() {
            break;
        }
    }
    let profile = profile.ok_or_else(|| AppError::msg("BYTE couldn't read a style from that. Try a longer sample, or a bigger model."))?;
    let mut s = state.settings.lock().await;
    let mut next = s.clone();
    next.writing_style = profile.clone();
    next.save(&state.paths.settings_file)?;
    *s = next;
    Ok(profile)
}

// ---------- the long-form writer ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Essay,
    Story,
    Blog,
    Report,
    Speech,
    Poem,
}

impl Kind {
    fn what(self) -> &'static str {
        match self {
            Kind::Essay => "an essay",
            Kind::Story => "a short story",
            Kind::Blog => "a blog post",
            Kind::Report => "a report",
            Kind::Speech => "a speech",
            Kind::Poem => "a poem",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongAsk {
    pub kind: Kind,
    pub topic: String,
    /// Words wanted (not for speeches and poems).
    #[serde(default)]
    pub words: Option<u32>,
    /// Speeches: how long it should take to say.
    #[serde(default)]
    pub minutes: Option<u32>,
    /// Poems: free verse, rhyming, haiku, sonnet, limerick.
    #[serde(default)]
    pub form: Option<String>,
    /// Anything else: audience, points to include, tone.
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub like_me: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub heading: String,
    pub points: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outline {
    pub title: String,
    pub sections: Vec<Section>,
}

/// Speaking pace for speeches.
pub const WORDS_PER_MINUTE: u32 = 130;

/// How many words the whole piece should be.
pub fn target_words(ask: &LongAsk) -> u32 {
    match ask.kind {
        Kind::Speech => ask.minutes.unwrap_or(5).clamp(1, 30) * WORDS_PER_MINUTE,
        Kind::Poem => 0,
        _ => ask.words.unwrap_or(800).clamp(150, 6000),
    }
}

fn brief(ask: &LongAsk) -> String {
    let mut b = format!("Write {} about: {}", ask.kind.what(), ask.topic.trim());
    match ask.kind {
        Kind::Speech => b.push_str(&format!(" (about {} minutes out loud, ~{} words)", ask.minutes.unwrap_or(5).clamp(1, 30), target_words(ask))),
        Kind::Poem => {}
        _ => b.push_str(&format!(" (about {} words)", target_words(ask))),
    }
    if !ask.notes.trim().is_empty() {
        b.push_str(&format!(".\nMore from the user: {}", ask.notes.trim()));
    }
    b
}

fn outline_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "sections": { "type": "array", "minItems": 2, "maxItems": 10, "items": { "type": "object",
                "properties": { "heading": { "type": "string" }, "points": { "type": "array", "maxItems": 5, "items": { "type": "string" } } },
                "required": ["heading", "points"] } }
        },
        "required": ["title", "sections"]
    })
}

/// How many sections suit the length (about 250–400 words each).
pub fn section_count(ask: &LongAsk) -> usize {
    ((target_words(ask) as f64 / 320.0).round() as usize).clamp(2, 10)
}

pub fn outline_prompt(ask: &LongAsk) -> String {
    let parts = match ask.kind {
        Kind::Story => "the story's parts (setup, rising action, turning point, ending)",
        Kind::Speech => "the speech's parts (a strong opening, the main points, a memorable close with a call to action)",
        _ => "sections (an introduction, the main points, a conclusion)",
    };
    format!(
        "{}.\n\nPlan it first: a title and {} {parts}, each with a short heading and 2–4 key points to cover. \
Make the plan specific to this topic.",
        brief(ask),
        section_count(ask)
    )
}

/// The model's plan, cleaned: headings required, 2–10 sections, ≤5 points each.
pub fn parse_outline(reply: &str) -> Option<Outline> {
    let v = crate::research::lenient_json(reply);
    let clean = |x: &serde_json::Value, max: usize| -> String { x.as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect() };
    let sections: Vec<Section> = v["sections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let heading = clean(&s["heading"], 120);
            let points: Vec<String> = s["points"].as_array().into_iter().flatten().map(|p| clean(p, 240)).filter(|p| !p.is_empty()).take(5).collect();
            (!heading.is_empty()).then_some(Section { heading, points })
        })
        .take(10)
        .collect();
    (sections.len() >= 2).then(|| Outline { title: clean(&v["title"], 140), sections })
}

/// The title and heading BYTE puts before part `i` (models garble them when asked to).
pub fn section_heading(ask: &LongAsk, outline: &Outline, i: usize) -> String {
    let mut h = String::new();
    if i == 0 && !outline.title.trim().is_empty() && ask.kind != Kind::Poem {
        h.push_str(&format!("# {}\n\n", outline.title.trim()));
    }
    if matches!(ask.kind, Kind::Essay | Kind::Blog | Kind::Report) {
        if let Some(s) = outline.sections.get(i) {
            h.push_str(&format!("## {}\n\n", s.heading));
        }
    }
    h
}

/// What the writer is asked for section `i` (or the whole poem).
pub fn section_prompt(ask: &LongAsk, outline: &Outline, i: usize, before: &str) -> String {
    if ask.kind == Kind::Poem {
        let form = ask.form.as_deref().filter(|f| !f.trim().is_empty()).unwrap_or("free verse");
        let how = match form.to_lowercase().as_str() {
            "haiku" => "a haiku: three lines of 5, 7 and 5 syllables".to_string(),
            "sonnet" => "a sonnet: 14 lines in iambic pentameter, rhyming ABAB CDCD EFEF GG".to_string(),
            "limerick" => "a limerick: five lines rhyming AABBA with a bouncy rhythm and a funny last line".to_string(),
            "rhyming" => "a rhyming poem of 3–5 stanzas with a steady rhythm".to_string(),
            f => format!("a {f} poem of 3–5 stanzas with vivid, concrete images"),
        };
        return format!("{}\n\nWrite {how}. Give it a title on the first line as a Markdown heading (# Title). Reply with only the poem.", brief(ask));
    }
    let n = outline.sections.len().max(1);
    let words = (target_words(ask) as usize / n).max(80);
    let plan: String = outline.sections.iter().enumerate().map(|(k, s)| format!("{}. {}{}\n", k + 1, s.heading, if k == i { "   <- write this one" } else { "" })).collect();
    let s = &outline.sections[i.min(n - 1)];
    let tail: String = { let c: Vec<char> = before.chars().collect(); c[c.len().saturating_sub(1500)..].iter().collect() };
    let mut p = format!("{}\n\nTitle: {}\nThe plan:\n{plan}\nWrite part {} of {n}, \"{}\", in about {words} words. Cover: {}.", brief(ask), outline.title, i + 1, s.heading, if s.points.is_empty() { "what the heading says".to_string() } else { s.points.join("; ") });
    if !tail.trim().is_empty() {
        p.push_str(&format!("\n\nThe piece so far ends like this; continue straight on from it, in the same voice, without repeating it:\n<<<\n{tail}\n>>>"));
    }
    p.push_str(match ask.kind {
        Kind::Speech => "\nWrite it to be spoken: short sentences, no headings or lists.",
        _ => "\nWrite only the text of this part: no title and no heading (they're added for you).",
    });
    if i + 1 == n && ask.kind == Kind::Speech {
        p.push_str(" This is the close: end with a clear call to action and a line people will remember.");
    }
    p
}

const LONG_WRITER: &str = "You are a skilled writer. Write exactly the part you're asked for, well: clear, specific and vivid, \
with facts you're sure of (no made-up statistics or quotes). Reply with only the text, no notes.";

/// Plans a long piece (edited by the user before writing).
#[tauri::command]
pub async fn writing_outline(state: State<'_, AppState>, ask: LongAsk) -> AppResult<Outline> {
    if ask.topic.trim().is_empty() {
        return Err(AppError::msg("What should it be about?"));
    }
    if ask.kind == Kind::Poem {
        return Ok(Outline { title: String::new(), sections: vec![] });
    }
    let ep = match state.engine.endpoint().await {
        Some(ep) => ep,
        None => crate::backend::cloud_cards(&state).await.ok_or_else(|| AppError::msg("Load a model first (Settings → Models), or connect BYTE Cloud."))?,
    };
    let reply = chat::complete_json(&state.local_http, &ep, "You plan pieces of writing. Reply only with JSON.", &outline_prompt(&ask), outline_schema(), 900).await?;
    parse_outline(&reply).ok_or_else(|| AppError::msg("The plan didn't come out right. Try again, or add a little more about what you want."))
}

/// Writes part `index` of the plan (or the whole poem), streaming.
#[tauri::command]
pub async fn writing_section(state: State<'_, AppState>, request_id: String, ask: LongAsk, outline: Outline, index: usize, before: String, on_event: Channel<ChatEvent>) -> AppResult<()> {
    if ask.kind != Kind::Poem && index >= outline.sections.len() {
        return Err(AppError::msg("That part isn't in the plan."));
    }
    let style = if ask.like_me { style_of(&state).await } else { String::new() };
    let system = format!("{LONG_WRITER}{}", style_rules(&style));
    let words = if ask.kind == Kind::Poem { 250 } else { target_words(&ask) as usize / outline.sections.len().max(1) };
    let max = ((words as f64 * 1.9) as u32 + 200).clamp(300, 3000);
    let heading = section_heading(&ask, &outline, index);
    if !heading.is_empty() {
        let _ = on_event.send(ChatEvent::Content { delta: heading });
    }
    stream_text(&state, &request_id, &system, &section_prompt(&ask, &outline, index, &before), max, Some(0.8), &on_event).await
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
        assert!(instructions(Action::Translate, Some("Japanese")).contains("into Japanese"));
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

    fn ask(kind: Kind) -> LongAsk {
        LongAsk { kind, topic: "why sleep matters".into(), words: Some(1200), minutes: Some(4), form: None, notes: String::new(), like_me: false }
    }

    /// Real engine: a plan, its first part, and a style from a sample.
    #[tokio::test]
    #[ignore]
    async fn e2e_longform_and_style() {
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let http = chat::local_client();
        let a = LongAsk { words: Some(700), ..ask(Kind::Essay) };
        let reply = chat::complete_json(&http, &ep, "You plan pieces of writing. Reply only with JSON.", &outline_prompt(&a), outline_schema(), 900).await.unwrap();
        let o = parse_outline(&reply).expect("an outline");
        eprintln!("outline: {o:?}");
        let body = text_body(LONG_WRITER, &section_prompt(&a, &o, 0, ""), 900, Some(0.8), Default::default());
        let mut out = section_heading(&a, &o, 0);
        let mut f = |e: ChatEvent| {
            if let ChatEvent::Content { delta } = e {
                out.push_str(&delta);
            }
            Ok(())
        };
        chat::stream_round(&http, &ep, &body, &tokio_util::sync::CancellationToken::new(), &mut f).await.unwrap();
        eprintln!("part 1: {out}");
        assert!(out.split_whitespace().count() > 60, "{out}");
        assert!(out.to_lowercase().contains("sleep"), "{out}");
        let sample = "Hey folks! Quick one today - I finally tried the new ramen place on Carson St and honestly? Game changer. \
The broth was rich, the noodles had real bite, and the staff were super chill. Only gripe - the line. Get there early, trust me. \
Anyway, that's my two cents. Go eat some noodles and tell me what you think!";
        let reply = chat::complete_json(&http, &ep, "You are an expert editor who describes writing styles precisely. Reply only with JSON.", &format!("Sample 1:\n<<<\n{sample}\n>>>\n\nDescribe how this person writes."), style_schema(), 500).await.unwrap();
        let style = parse_style(&reply, &[sample.to_string()]).unwrap_or_default();
        eprintln!("style: {style}");
    }

    #[test]
    fn lengths_follow_the_kind() {
        assert_eq!(target_words(&ask(Kind::Essay)), 1200);
        assert_eq!(target_words(&ask(Kind::Speech)), 4 * WORDS_PER_MINUTE);
        assert_eq!(target_words(&LongAsk { words: Some(50), ..ask(Kind::Blog) }), 150, "at least 150 words");
        assert_eq!(section_count(&ask(Kind::Essay)), 4);
        assert_eq!(section_count(&LongAsk { words: Some(20000), ..ask(Kind::Essay) }), 10);
        assert!(outline_prompt(&ask(Kind::Speech)).contains("call to action"));
    }

    #[test]
    fn outlines_are_cleaned() {
        let o = parse_outline(r#"{"title":"Sleep","sections":[{"heading":" Why  it matters ","points":["memory","", "mood"]},{"heading":"","points":["x"]},{"heading":"What to do","points":[]}]}"#).unwrap();
        assert_eq!(o.sections.len(), 2, "sections without a heading are dropped");
        assert_eq!(o.sections[0].heading, "Why it matters");
        assert_eq!(o.sections[0].points, vec!["memory", "mood"]);
        assert!(parse_outline(r#"{"title":"x","sections":[{"heading":"only one","points":[]}]}"#).is_none());
    }

    #[test]
    fn sections_continue_the_piece() {
        let o = Outline { title: "Sleep".into(), sections: vec![Section { heading: "Intro".into(), points: vec!["hook".into()] }, Section { heading: "Close".into(), points: vec![] }] };
        let first = section_prompt(&ask(Kind::Essay), &o, 0, "");
        assert!(first.contains("Write part 1 of 2") && first.contains("no heading") && first.contains("600 words"));
        assert_eq!(section_heading(&ask(Kind::Essay), &o, 0), "# Sleep\n\n## Intro\n\n");
        assert_eq!(section_heading(&ask(Kind::Essay), &o, 1), "## Close\n\n");
        assert_eq!(section_heading(&ask(Kind::Story), &o, 1), "", "stories have no section headings");
        let second = section_prompt(&ask(Kind::Speech), &o, 1, &"x".repeat(3000));
        assert!(second.contains("continue straight on") && second.contains("call to action") && !second.contains("## Close"));
        assert!(second.matches('x').count() <= 1600, "only the end of the piece so far");
        let poem = section_prompt(&LongAsk { form: Some("haiku".into()), ..ask(Kind::Poem) }, &Outline::default(), 0, "");
        assert!(poem.contains("5, 7 and 5 syllables"));
    }

    #[test]
    fn styles_are_read_and_applied() {
        let sample = vec!["Hey folks! Quick one today - I finally tried the new ramen place on Carson St and honestly? Game changer.".to_string()];
        let p = parse_style(r#"{"voice":"warm and direct","sentences":"I finally tried the new ramen place on Carson St","words":"plain, some slang","habits":["starts with Hey","uses dashes"],"avoid":["emoji"]}"#, &sample).unwrap();
        assert!(p.contains("- Voice: warm and direct") && p.contains("- Habits: starts with Hey; uses dashes") && p.contains("- Never: emoji"));
        assert!(!p.contains("Sentences"), "a copied sample isn't a description: {p}");
        assert!(parse_style("{}", &sample).is_none());
        assert_eq!(style_rules("  "), "");
        assert!(style_rules(&p).contains("Write it the way the user writes"));
    }

    #[test]
    fn actions_come_from_the_ui_in_camel_case() {
        assert_eq!(serde_json::from_str::<Action>("\"grammar\"").unwrap(), Action::Grammar);
        assert!(serde_json::from_str::<Action>("\"Grammar\"").is_err());
    }
}
