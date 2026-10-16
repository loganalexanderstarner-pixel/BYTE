//! Game companion and replay review, on this computer: "Review my last play" looks back over the replay buffer (the screen
//! track keeps the last seconds in memory only, when it is switched on) and says what seems to have gone wrong; "How is my game
//! going?" reads one look at the game and names the likely problems. Both use the local model and the local photo helper, so they
//! work with no internet, and they use the screen track's own rules (excluded windows, private chats, memory only).

use crate::screen::flow::{make_sheets, sheet_labels, PER_SHEET, SPREAD, TAIL};

/// A replay needs at least this many frames to say anything.
pub const MIN_FRAMES: usize = 3;

/// Why the replay can't be reviewed yet, in words (None when it can).
pub fn replay_problem(frames: usize, on: bool, paused: Option<&str>, seconds: u32) -> Option<String> {
    if frames >= MIN_FRAMES {
        return None;
    }
    let why = match (paused, on) {
        (_, false) => "The replay isn't running. Turn on \"Remember the last seconds\" in Settings > Control and screen.".to_string(),
        (Some(p), true) => format!("The replay has almost nothing yet: it is paused because {p}."),
        _ => "The replay has only just started; give it a few seconds of play first.".to_string(),
    };
    Some(format!("{why} It keeps the last {seconds} seconds of your screen in memory only."))
}

/// The user message: each picture's moments and what the photo helper saw in it, then the last frame's words.
pub fn review_message(labels: &[Vec<String>], seen: &[String], last_text: &str) -> String {
    let mut s = String::from("Here is what the screen showed, picture by picture, from the earliest to the latest.\n");
    for (i, l) in labels.iter().enumerate() {
        s.push_str(&format!("\nPicture {} ({}):\n{}\n", i + 1, l.join(", "), seen.get(i).map(|t| t.trim()).filter(|t| !t.is_empty()).unwrap_or("(not described)")));
    }
    if !last_text.trim().is_empty() {
        s.push_str(&format!("\nWords read from the last frame:\n{}\n", last_text.trim()));
    }
    s.push_str("\nWhat went wrong, and what could they try next time?");
    s
}

#[cfg(desktop)]
mod flow {
    use super::*;
    use crate::helpers::popup::{self, Kind, Payload};
    use crate::screen::{capture, frame, prompts, replay, sample};
    use tauri::{AppHandle, Manager};

    fn say(app: &AppHandle, title: &str, body: String, hide: u32) {
        popup::show(app, Kind::Game, Payload { title: title.into(), body, buttons: vec![popup::button("copy", "Copy"), popup::button("close", "Close")], auto_hide: hide, ..Default::default() }, None);
        if let Ok(mut l) = LAST.lock() {
            *l = body_of(app);
        }
    }

    fn body_of(_app: &AppHandle) -> String {
        popup::current_body()
    }

    static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

    pub async fn clicked(app: &AppHandle, id: &str) {
        if id == "copy" {
            let t = LAST.lock().map(|l| l.clone()).unwrap_or_default();
            if !t.is_empty() {
                crate::clipboard::set_quietly(&t);
            }
        }
        popup::hide(app);
    }

    async fn gate(app: &AppHandle) -> Option<crate::screen::config::Config> {
        let state = app.state::<crate::state::AppState>();
        let (on, cfg) = {
            let s = state.settings.lock().await;
            (s.helper_game && !s.kids_mode, crate::screen::config::Config::from_settings(&s))
        };
        if !on {
            return None;
        }
        if let Some(why) = crate::helpers::blocked_now() {
            say(app, "Game companion", why.into(), 6_000);
            return None;
        }
        Some(cfg)
    }

    /// "Review my last play": the replay buffer, looked at with the local photo helper, answered by the local model.
    pub async fn review(app: &AppHandle) {
        let Some(cfg) = gate(app).await else { return };
        let snap = replay::snapshot();
        let st = replay::status();
        if let Some(why) = replay_problem(snap.len(), st.on, st.paused.as_deref(), cfg.seconds) {
            return say(app, "Review my last play", why, 10_000);
        }
        popup::show(app, Kind::Game, Payload { title: "Review my last play".into(), body: "Looking back over the last few seconds...".into(), busy: true, ..Default::default() }, None);
        let times: Vec<u64> = snap.iter().map(|s| s.at_ms).collect();
        let picks = sample::pick(snap.len(), SPREAD, TAIL);
        let before_end = sample::seconds_before_end(&times, &picks);
        let covered = ((times.last().copied().unwrap_or(0) - times.first().copied().unwrap_or(0)) / 1000) as u32;
        let jpegs: Vec<Vec<u8>> = picks.iter().map(|&i| snap[i].jpeg.clone()).collect();
        drop(snap);
        let built = tokio::task::spawn_blocking(move || -> Result<(Vec<capture::Encoded>, String), capture::CaptureError> {
            let frames: Vec<frame::Frame> = jpegs.iter().map(|j| capture::decode(j)).collect::<Result<_, _>>()?;
            let sheets = make_sheets(&frames);
            let enc: Vec<capture::Encoded> = sheets.iter().map(|s| capture::encode(s, 1800)).collect::<Result<_, _>>()?;
            let last = frames.last().map(|f| capture::encode(f, 1600)).transpose()?;
            let text = last.and_then(|e| crate::ocr::image_text(&e.bytes).ok()).unwrap_or_default();
            Ok((enc, text.trim().chars().take(1500).collect()))
        })
        .await;
        let (sheets, last_text) = match built {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return say(app, "Review my last play", e.say(), 10_000),
            Err(_) => return say(app, "Review my last play", "BYTE couldn't read the replay just now.".into(), 10_000),
        };
        let state = app.state::<crate::state::AppState>();
        let catalog = state.catalog.get();
        let labels = sheet_labels(&before_end);
        let mut seen = Vec::new();
        if crate::looker::choose(&catalog, &state.paths.models).is_some() {
            for (i, e) in sheets.iter().enumerate() {
                let q = prompts::frames_question(&[format!("picture {}: {}", i + 1, labels.get(i).map(|l| l.join(", ")).unwrap_or_default())]);
                seen.push(state.looker.describe(app, &state.paths.models, &catalog, &e.data_url(), &q).await.unwrap_or_default());
            }
        }
        if seen.iter().all(|s| s.trim().is_empty()) && last_text.trim().is_empty() {
            return say(app, "Review my last play", "BYTE can't see pictures here: the photo helper isn't set up (Settings > Models), and no words were found on the screen.".into(), 12_000);
        }
        let system = format!("{}{}", crate::helpers::ask::SYSTEM, prompts::coach(&crate::screen::policy::WindowInfo::default(), covered, &before_end, PER_SHEET, false));
        match crate::helpers::ask::run(&state, &system, &review_message(&labels, &seen, &last_text), 500, true).await {
            Ok((answer, place)) => say(app, &format!("Review (answered on {place})"), answer, 0),
            Err(e) => say(app, "Review my last play", e.to_string(), 10_000),
        }
    }

    /// "How is my game going?": one look.
    pub async fn advise(app: &AppHandle) {
        let Some(cfg) = gate(app).await else { return };
        popup::show(app, Kind::Game, Payload { title: "How is my game going?".into(), body: "Taking one look...".into(), busy: true, ..Default::default() }, None);
        let excludes = cfg.excludes.clone();
        // The window in front is whatever the person was in when they chose the tray entry: the game (the tray is not a window of its own).
        let shot = match tokio::task::spawn_blocking(move || crate::screen::flow::grab_and_read(excludes)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return say(app, "How is my game going?", e.say(), 10_000),
            Err(_) => return say(app, "How is my game going?", "BYTE couldn't look just now.".into(), 8_000),
        };
        let state = app.state::<crate::state::AppState>();
        let catalog = state.catalog.get();
        let seen = if crate::looker::choose(&catalog, &state.paths.models).is_some() {
            state.looker.describe(app, &state.paths.models, &catalog, &shot.encoded.data_url(), "Describe this game screen: the game if you can tell, and every number, bar and warning you can read.").await.unwrap_or_default()
        } else {
            String::new()
        };
        if seen.trim().is_empty() && shot.text.trim().is_empty() {
            return say(app, "How is my game going?", "BYTE couldn't see anything to go on.".into(), 8_000);
        }
        let system = format!("{}{}", crate::helpers::ask::SYSTEM, prompts::advisor(&shot.window, false, !shot.text.trim().is_empty()));
        let user = format!("What the screen shows:\n{}\n\nText read from the screen:\n{}\n\nHow is it going, and what is the problem?", seen.trim(), shot.text.trim());
        match crate::helpers::ask::run(&state, &system, &user, 500, true).await {
            Ok((answer, place)) => say(app, &format!("How your game is going (answered on {place})"), answer, 0),
            Err(e) => say(app, "How is my game going?", e.to_string(), 10_000),
        }
    }
}

#[cfg(desktop)]
pub fn review(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { flow::review(&app).await });
}
#[cfg(desktop)]
pub fn advise(app: &tauri::AppHandle) {
    let app = app.clone();
    // Give the tray menu a moment to close so the game is the window in front again.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        flow::advise(&app).await
    });
}
#[cfg(desktop)]
pub async fn clicked(app: &tauri::AppHandle, id: &str) {
    flow::clicked(app, id).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thin_replay_says_why() {
        assert!(replay_problem(5, true, None, 30).is_none());
        assert!(replay_problem(1, false, None, 30).unwrap().contains("isn't running"));
        assert!(replay_problem(1, true, Some("a private chat is open"), 30).unwrap().contains("paused because a private chat is open"));
        assert!(replay_problem(0, true, None, 60).unwrap().contains("only just started") && replay_problem(0, true, None, 60).unwrap().contains("60 seconds"));
    }

    #[test]
    fn the_review_message_lists_each_picture_with_what_was_seen() {
        let labels = vec![vec!["29 s before the end".to_string(), "the end".to_string()], vec!["5 s before the end".to_string()]];
        let m = review_message(&labels, &["A car on a road.".into()], " You died ");
        assert!(m.contains("Picture 1 (29 s before the end, the end):\nA car on a road.") && m.contains("Picture 2 (5 s before the end):\n(not described)"));
        assert!(m.contains("Words read from the last frame:\nYou died") && m.ends_with("next time?"));
        assert!(!review_message(&labels, &[], "").contains("Words read"));
    }
}
