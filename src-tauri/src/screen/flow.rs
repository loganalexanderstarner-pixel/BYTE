//! The screen flows that sit in front of a chat turn (`backend::answer` calls `prepare` once per turn):
//! ask about the window in front, review the replay, game advice, and read aloud.
//!
//! How a picture reaches the model, reusing what chat already has:
//! * Local engine that can see: the picture rides along as an attached photo (with the text Windows read from it).
//! * Local engine that can't: the same attachment; the photo helper (`looker.rs`) describes it before the model answers,
//!   and the read text goes along (that is how a photo of a page already works).
//! * BYTE Cloud (only when the person chose it for this question): the picture is NOT uploaded; BYTE describes it here and
//!   sends the words, and says so. The picture never leaves the PC.
//!
//! Nothing here runs in a private chat, and nothing runs unless the matching setting is on.

use serde_json::json;
use tauri::ipc::Channel;

use super::capture::{self, CaptureError};
use super::config::{self, Config};
use super::phrases::{self, Intent};
use super::policy::WindowInfo;
use super::{frame, prompts, read, replay, sample};
use crate::chat::{self, ChatEvent};
use crate::commands::ChatRequest;
use crate::files::{FileKind, Ingested};
use crate::state::AppState;

pub const PRIVATE_SAY: &str = "This is a private chat, so BYTE doesn't look at your screen.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Not a screen turn (or the feature is off): the chat goes on as usual.
    Skip,
    /// A screen request that must not run; say why.
    Refuse(String),
    Go,
}

/// Whether a request of this kind may run now: the one decision every screen flow goes through.
pub fn gate(intent: Intent, cfg: &Config, private: bool, supported: bool) -> Gate {
    let on = match intent {
        Intent::Ask => cfg.ask,
        Intent::Review => cfg.replay,
        Intent::Advisor => cfg.advisor,
        Intent::ReadThis | Intent::StopReading => cfg.read_it,
    };
    if !supported || !on {
        return Gate::Skip;
    }
    if private && intent != Intent::StopReading {
        return Gate::Refuse(PRIVATE_SAY.into());
    }
    Gate::Go
}

/// Frames to ask about: how many evenly spaced and how many from the end.
pub const SPREAD: usize = 8;
pub const TAIL: usize = 4;
/// Frames per picture (2 x 2), each tile this big.
pub const PER_SHEET: usize = 4;
const TILE_W: u32 = 720;
const TILE_H: u32 = 405;

/// The labels ("29 s before the end") of the frames in each sheet, in order.
pub fn sheet_labels(times_before_end: &[f32]) -> Vec<Vec<String>> {
    times_before_end
        .chunks(PER_SHEET)
        .map(|c| c.iter().map(|t| if *t < 0.5 { "the end".to_string() } else { format!("{t:.0} s before the end") }).collect())
        .collect()
}

/// Lays the picked frames out in sheets of four.
pub fn make_sheets(frames: &[frame::Frame]) -> Vec<frame::Frame> {
    frames.chunks(PER_SHEET).filter_map(|c| frame::contact_sheet(c, 2, TILE_W, TILE_H)).collect()
}

fn photo(name: &str, e: &capture::Encoded, text: String) -> Ingested {
    let ocr = !text.trim().is_empty();
    Ingested { name: name.into(), kind: FileKind::Image, pages: None, text, truncated: false, image: Some(e.data_url()), ocr }
}

fn say(on_event: &Channel<ChatEvent>, text: &str) {
    let _ = on_event.send(ChatEvent::Content { delta: text.to_string() });
    let _ = on_event.send(ChatEvent::Done { finish_reason: "stop".into() });
}

/// Requests whose message now carries text read from the screen: nothing from them is saved to memory (local or cloud).
static CARRIED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn carried_mark(request_id: &str) {
    if let Ok(mut v) = CARRIED.lock() {
        if v.len() >= 64 {
            v.remove(0);
        }
        v.push(request_id.to_string());
    }
}

/// This request carried text read from the screen (to the cloud as words): it must not become a memory.
pub fn carried_screen_text(request_id: &str) -> bool {
    CARRIED.lock().map(|v| v.iter().any(|r| r == request_id)).unwrap_or(false)
}

/// What `prepare` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Carry on with the chat (the request may now carry the screen).
    Continue,
    /// BYTE already answered (a refusal, or "reading it now").
    Answered,
}

/// The last thing the person said, without the wake phrase.
fn question_of(request: &ChatRequest) -> String {
    request.messages.iter().rev().find(|m| m.role == "user").map(|m| chat::question_text(&m.content).to_string()).unwrap_or_default()
}

pub async fn prepare(state: &AppState, request: &mut ChatRequest, on_event: &Channel<ChatEvent>) -> Outcome {
    // A private chat forgets what the replay holds, whatever is asked.
    if request.private {
        replay::set_private(true);
    }
    let question = question_of(request);
    let Some(intent) = phrases::detect(&question) else { return Outcome::Continue };
    // A message with files attached is about those files.
    if request.messages.iter().rev().find(|m| m.role == "user").is_some_and(|m| !m.files.is_empty()) {
        return Outcome::Continue;
    }
    let cfg = Config::from_settings(&*state.settings.lock().await);
    match gate(intent, &cfg, request.private, cfg!(windows)) {
        Gate::Skip => return Outcome::Continue,
        Gate::Refuse(why) => {
            say(on_event, &why);
            return Outcome::Answered;
        }
        Gate::Go => {}
    }
    let Some(app) = state.app.get().cloned() else { return Outcome::Continue };
    let cloud = request.cloud.is_some();
    super::looking(&app, true);
    let out = match intent {
        Intent::StopReading => {
            crate::speech::speech_stop();
            say(on_event, "Stopped.");
            Outcome::Answered
        }
        Intent::ReadThis => read_it(state, &app, &cfg, on_event).await,
        Intent::Ask | Intent::Advisor => look_once(state, request, &cfg, intent, cloud, on_event).await,
        Intent::Review => review(state, request, &cfg, cloud, on_event).await,
    };
    super::looking(&app, false);
    out
}

// ------------------------------------------------------------------ ask and advisor

pub(crate) struct Shot {
    pub(crate) window: WindowInfo,
    pub(crate) encoded: capture::Encoded,
    pub(crate) text: String,
}

/// The blocking part: capture, shrink, encode, read the text. Runs on a worker thread (the capture objects stay on it).
pub(crate) fn grab_and_read(excludes: Vec<String>) -> Result<Shot, CaptureError> {
    let mut src = capture::default_source();
    let (window, f) = capture::once(src.as_mut(), &excludes, false)?;
    drop(src);
    // Text is read from a larger picture than the one the model gets: small print survives.
    let for_text = capture::encode(&f, 2200)?;
    let text = crate::ocr::image_text(&for_text.bytes).unwrap_or_default();
    let encoded = capture::encode(&f, config::ASK_SIDE)?;
    Ok(Shot { window, encoded, text: clip(&text, 6000) })
}

fn clip(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= n {
        t.to_string()
    } else {
        t.chars().take(n).collect()
    }
}

async fn look_once(state: &AppState, request: &mut ChatRequest, cfg: &Config, intent: Intent, cloud: bool, on_event: &Channel<ChatEvent>) -> Outcome {
    let id = "byte_screen_0".to_string();
    let _ = on_event.send(ChatEvent::ToolCall { id: id.clone(), name: "look_at_screen".into(), args: json!({}) });
    let excludes = cfg.excludes.clone();
    let shot = match tokio::task::spawn_blocking(move || grab_and_read(excludes)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            let _ = on_event.send(ChatEvent::ToolResult { id, ok: false, summary: "Didn't look".into() });
            say(on_event, &e.say());
            return Outcome::Answered;
        }
        Err(_) => {
            let _ = on_event.send(ChatEvent::ToolResult { id, ok: false, summary: "Didn't look".into() });
            say(on_event, "BYTE couldn't capture the screen just now.");
            return Outcome::Answered;
        }
    };
    let what = if shot.window.program.is_empty() { "the window in front".to_string() } else { shot.window.program.clone() };
    let _ = on_event.send(ChatEvent::ToolResult { id, ok: true, summary: format!("Looked at {what}") });
    let name = if intent == Intent::Advisor { "screen-game.jpg" } else { "screen.jpg" };
    attach(state, request, cloud, on_event, vec![photo(name, &shot.encoded, shot.text.clone())], |picture| {
        let has_text = !shot.text.trim().is_empty();
        if intent == Intent::Advisor {
            prompts::advisor(&shot.window, picture, has_text)
        } else {
            prompts::ask(&shot.window, picture, has_text)
        }
    })
    .await;
    Outcome::Continue
}

// ------------------------------------------------------------------ review the replay

async fn review(state: &AppState, request: &mut ChatRequest, cfg: &Config, cloud: bool, on_event: &Channel<ChatEvent>) -> Outcome {
    let snap = replay::snapshot();
    if snap.len() < 3 {
        let st = replay::status();
        let why = match (&st.paused, st.on) {
            (_, false) => "The replay isn't running.".to_string(),
            (Some(p), true) => format!("The replay has almost nothing yet: it is paused because {p}."),
            _ => "The replay has only just started; give it a few seconds of play first.".to_string(),
        };
        say(on_event, &format!("{why} It keeps the last {} seconds of your screen in memory only, so I have nothing to look back at.", cfg.seconds));
        return Outcome::Answered;
    }
    let id = "byte_screen_0".to_string();
    let _ = on_event.send(ChatEvent::ToolCall { id: id.clone(), name: "look_back".into(), args: json!({ "seconds": cfg.seconds }) });
    let times: Vec<u64> = snap.iter().map(|s| s.at_ms).collect();
    let picks = sample::pick(snap.len(), SPREAD, TAIL);
    let before_end = sample::seconds_before_end(&times, &picks);
    let covered = ((times.last().copied().unwrap_or(0) - times.first().copied().unwrap_or(0)) / 1000) as u32;
    let jpegs: Vec<Vec<u8>> = picks.iter().map(|&i| snap[i].jpeg.clone()).collect();
    drop(snap);
    let built = tokio::task::spawn_blocking(move || -> Result<(Vec<capture::Encoded>, String), CaptureError> {
        let frames: Vec<frame::Frame> = jpegs.iter().map(|j| capture::decode(j)).collect::<Result<_, _>>()?;
        let sheets = make_sheets(&frames);
        let enc: Vec<capture::Encoded> = sheets.iter().map(|s| capture::encode(s, 1800)).collect::<Result<_, _>>()?;
        // The last frame's words (a score, a "you died" banner) read from the frame itself.
        let last = frames.last().map(|f| capture::encode(f, 1600)).transpose()?;
        let text = last.and_then(|e| crate::ocr::image_text(&e.bytes).ok()).unwrap_or_default();
        Ok((enc, clip(&text, 1500)))
    })
    .await;
    let (sheets, last_text) = match built {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            let _ = on_event.send(ChatEvent::ToolResult { id, ok: false, summary: "Couldn't look back".into() });
            say(on_event, &e.say());
            return Outcome::Answered;
        }
        Err(_) => {
            let _ = on_event.send(ChatEvent::ToolResult { id, ok: false, summary: "Couldn't look back".into() });
            say(on_event, "BYTE couldn't read the replay just now.");
            return Outcome::Answered;
        }
    };
    let _ = on_event.send(ChatEvent::ToolResult { id, ok: true, summary: format!("Looked back over {} frames of the last {covered} s", picks.len()) });
    let labels = sheet_labels(&before_end);
    let files: Vec<Ingested> = sheets
        .iter()
        .enumerate()
        .map(|(i, e)| photo(&format!("replay-{}.jpg", i + 1), e, if i + 1 == sheets.len() { last_text.clone() } else { String::new() }))
        .collect();
    let window = WindowInfo::default();
    let flat: Vec<String> = labels.iter().enumerate().map(|(i, l)| format!("picture {}: {}", i + 1, l.join(", "))).collect();
    let question = prompts::frames_question(&flat);
    attach_with(state, request, cloud, on_event, files, Some(question), |picture| prompts::coach(&window, covered, &before_end, PER_SHEET, picture)).await;
    Outcome::Continue
}

// ------------------------------------------------------------------ attaching

async fn attach<F: FnOnce(bool) -> String>(state: &AppState, request: &mut ChatRequest, cloud: bool, on_event: &Channel<ChatEvent>, files: Vec<Ingested>, notes: F) {
    attach_with(state, request, cloud, on_event, files, None, notes).await
}

/// Puts the pictures (and the instructions for answering from them) into the request.
/// `describe_q`: what the photo helper is asked about each picture, for engines that can't see.
async fn attach_with<F: FnOnce(bool) -> String>(state: &AppState, request: &mut ChatRequest, cloud: bool, on_event: &Channel<ChatEvent>, files: Vec<Ingested>, describe_q: Option<String>, notes: F) {
    let Some(i) = request.messages.iter().rposition(|m| m.role == "user") else { return };
    if !cloud {
        // The local engine: photos ride along; `backend::local_turn` lets the photo helper describe them if the model can't see.
        request.style_notes.push_str(&notes(true));
        request.messages[i].files.extend(files);
        return;
    }
    // BYTE Cloud: words only. Describe each picture here, with the helper, and send that.
    let _ = on_event.send(ChatEvent::Notice {
        text: crate::platform_text::here("You chose BYTE Cloud for this question, so BYTE sends it a description of your screen and the text read from it (not the picture itself). It is kept in this chat on the cloud like any message, and BYTE does not remember it in memory."),
    });
    let question = describe_q.unwrap_or_else(|| question_of(request));
    let mut parts: Vec<String> = Vec::new();
    let mut described = false;
    if state.settings.lock().await.photo_helper {
        if let Some(app) = state.app.get() {
            let catalog = state.catalog.get();
            for f in &files {
                if let (Some(url), true) = (f.image.as_deref(), crate::looker::choose(&catalog, &state.paths.models).is_some()) {
                    match state.looker.describe(app, &state.paths.models, &catalog, url, &question).await {
                        Ok(d) => {
                            described = true;
                            parts.push(format!("[What the screen shows, described by BYTE's photo helper on this PC]\n{d}"));
                        }
                        Err(e) => log::warn!("screen: the photo helper failed: {e}"),
                    }
                }
            }
        }
    }
    for f in &files {
        if !f.text.trim().is_empty() {
            parts.push(format!("[Text read from the screen]\n{}", f.text.trim()));
        }
    }
    if parts.is_empty() {
        parts.push("[BYTE could not read or describe the screen: no text was found and the photo helper isn't set up. Say so rather than guessing.]".into());
    }
    request.style_notes.push_str(&notes(described));
    carried_mark(&request.request_id);
    let m = &mut request.messages[i];
    m.content = format!("{}\n\n---\n{}", m.content, parts.join("\n\n"));
}

// ------------------------------------------------------------------ read it to me

#[cfg(windows)]
async fn words_to_read(cfg: &Config) -> Result<(String, String), String> {
    if read::game_in_front(read::shell_state()) {
        return Err(read::GAME_SAY.to_string());
    }
    // The window to read: the one in front, or the one below BYTE when the question was typed in BYTE. The rules apply to THAT window.
    let own = capture::default_source().front_is_own();
    let front = capture::front_window();
    super::policy::may_capture(front.as_ref(), &cfg.excludes, false).map_err(|r| r.say())?;
    let window = front.unwrap_or_default();
    // The selection first (Ctrl+C, clipboard put back), but never by pressing keys in BYTE itself, a terminal or an editor (Ctrl+C would
    // stop a running command): those are read from a picture of the window only.
    if !own && super::policy::may_send_keys(&window) {
        // BYTE's clipboard history sits out the short copy (Windows' own Win+V history may still note it; the clipboard is put back).
        crate::clipboard::pause_history(true);
        let copied = crate::selection::capture_windows().await;
        crate::clipboard::pause_history(false);
        if let Ok(c) = copied {
            if !c.text.trim().is_empty() {
                return Ok((c.text, window.program));
            }
        }
    }
    let excludes = cfg.excludes.clone();
    let shot = tokio::task::spawn_blocking(move || -> Result<String, CaptureError> {
        let mut src = capture::default_source();
        let (_, f) = capture::once(src.as_mut(), &excludes, false)?;
        let e = capture::encode(&f, 2600)?;
        crate::ocr::image_text(&e.bytes).map_err(|e| CaptureError::Failed(e.to_string()))
    })
    .await
    .map_err(|_| "BYTE couldn't capture the screen just now.".to_string())?
    .map_err(|e| e.say())?;
    Ok((shot, window.program))
}

#[cfg(not(windows))]
async fn words_to_read(_: &Config) -> Result<(String, String), String> {
    Err(crate::platform_text::here("Reading the screen aloud isn't built for this system yet."))
}

async fn read_it(state: &AppState, app: &tauri::AppHandle, cfg: &Config, on_event: &Channel<ChatEvent>) -> Outcome {
    let (raw, from) = match words_to_read(cfg).await {
        Ok(v) => v,
        Err(e) => {
            say(on_event, &e);
            return Outcome::Answered;
        }
    };
    let (text, cut) = read::prepare_text(&raw, read::MOST_CHARS);
    if text.is_empty() {
        say(on_event, "BYTE didn't find any text to read there. Select some text, or open the page and ask again.");
        return Outcome::Answered;
    }
    let words = read::word_count(&text);
    let id = format!("byte_read_{}", uuid::Uuid::new_v4().simple());
    let spoken = {
        use tauri::Manager;
        crate::speech::speech_feed(app.clone(), app.state::<AppState>(), id, text, true, Some(true)).await
    };
    let _ = state;
    match spoken {
        Ok(()) => {
            let src = if from.is_empty() { String::new() } else { format!(" from {from}") };
            let more = if cut { " (it was long, so this is the first part)" } else { "" };
            say(on_event, &format!("Reading about {words} words{src}{more}. Say \"stop reading\" to stop."));
        }
        Err(e) => say(on_event, &format!("BYTE couldn't read it aloud: {e}")),
    }
    Outcome::Answered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn cfg(f: impl FnOnce(&mut Settings)) -> Config {
        let mut s = Settings::default();
        f(&mut s);
        Config::from_settings(&s)
    }

    #[test]
    fn nothing_runs_while_its_setting_is_off() {
        let off = cfg(|_| {});
        for i in [Intent::Ask, Intent::Review, Intent::Advisor, Intent::ReadThis, Intent::StopReading] {
            assert_eq!(gate(i, &off, false, true), Gate::Skip, "{i:?}");
        }
    }

    #[test]
    fn each_setting_turns_on_only_its_own_flow() {
        let c = cfg(|s| s.screen_ask = true);
        assert_eq!(gate(Intent::Ask, &c, false, true), Gate::Go);
        assert_eq!(gate(Intent::Review, &c, false, true), Gate::Skip);
        assert_eq!(gate(Intent::Advisor, &c, false, true), Gate::Skip);
        let c = cfg(|s| s.replay_buffer = true);
        assert_eq!(gate(Intent::Review, &c, false, true), Gate::Go);
        assert_eq!(gate(Intent::Ask, &c, false, true), Gate::Skip);
        let c = cfg(|s| s.game_advisor = true);
        assert_eq!(gate(Intent::Advisor, &c, false, true), Gate::Go);
        let c = cfg(|s| s.read_it = true);
        assert_eq!(gate(Intent::ReadThis, &c, false, true), Gate::Go);
        assert_eq!(gate(Intent::StopReading, &c, false, true), Gate::Go);
    }

    #[test]
    fn a_private_chat_is_refused_even_with_everything_on() {
        let c = cfg(|s| {
            s.screen_ask = true;
            s.replay_buffer = true;
            s.game_advisor = true;
            s.read_it = true;
        });
        for i in [Intent::Ask, Intent::Review, Intent::Advisor, Intent::ReadThis] {
            assert!(matches!(gate(i, &c, true, true), Gate::Refuse(_)), "{i:?}");
        }
        // Stopping speech is always fine.
        assert_eq!(gate(Intent::StopReading, &c, true, true), Gate::Go);
    }

    #[test]
    fn an_unsupported_system_leaves_the_chat_alone() {
        let c = cfg(|s| s.screen_ask = true);
        assert_eq!(gate(Intent::Ask, &c, false, false), Gate::Skip);
    }

    #[test]
    fn sheets_hold_four_frames_and_are_labelled_in_order() {
        let frames: Vec<frame::Frame> = (0..10).map(|i| frame::Frame::solid(320, 180, i as u8 * 20, 0, 0)).collect();
        let sheets = make_sheets(&frames);
        assert_eq!(sheets.len(), 3);
        assert_eq!(sheets[0].width, 2 * TILE_W + 3 * 4);
        let labels = sheet_labels(&[29.0, 25.0, 20.0, 15.0, 10.0, 6.0, 3.0, 2.0, 1.0, 0.0]);
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[0], vec!["29 s before the end", "25 s before the end", "20 s before the end", "15 s before the end"]);
        assert_eq!(labels[2], vec!["1 s before the end", "the end"]);
    }

    #[test]
    fn a_photo_carries_its_text_and_a_data_url() {
        let e = capture::encode(&frame::Frame::solid(32, 32, 1, 2, 3), 1280).unwrap();
        let p = photo("screen.jpg", &e, "ERROR 0x80070005".into());
        assert_eq!(p.kind, FileKind::Image);
        assert!(p.ocr && p.text.contains("0x80070005"));
        assert!(p.image.unwrap().starts_with("data:image/"));
        assert!(!photo("a", &e, "  ".into()).ocr);
    }

    #[test]
    fn a_request_that_carried_screen_text_is_remembered_as_such() {
        assert!(!carried_screen_text("req-1"));
        carried_mark("req-1");
        assert!(carried_screen_text("req-1"));
        assert!(!carried_screen_text("req-2"));
    }

    #[test]
    fn clip_cuts_by_characters() {
        assert_eq!(clip("  abc  ", 10), "abc");
        assert_eq!(clip("äöüäöü", 3), "äöü");
    }
}
