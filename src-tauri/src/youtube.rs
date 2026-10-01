//! YouTube (Phase 6, v0.6.5): summaries with timestamps, and questions about
//! a video. Paste a link ("summarize https://youtu.be/…") or ask about it.
//!
//! The transcript comes from YouTube's own captions, keyless: the player API
//! (`youtubei/v1/player`, as the Android app calls it) lists the caption
//! tracks; BYTE picks the best one (the user's language, human-made before
//! auto-generated) and reads its timed text. Long videos are summarized in
//! parts, then merged. Timestamps link to that moment in the video.
//!
//! Videos without captions (turned off, private, age-restricted) can't be
//! read yet: transcribing the audio needs the voice model (Phase 11).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::{self, ChatEvent, ChatMessage};
use crate::error::{AppError, AppResult};
use crate::research::{self, Ctx, Emit};
use crate::tools::SourceBook;

// ---------- links ----------

fn valid_id(s: &str) -> Option<String> {
    let id: String = s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    (id.len() == 11).then_some(id)
}

/// The video id of the first YouTube link in `text`.
pub fn video_id(text: &str) -> Option<String> {
    for word in text.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '(' || c == ')' || c == '"') {
        let w = word.trim_matches(|c: char| c == '.' || c == ',' || c == '!' || c == '?');
        let w = w.strip_prefix("https://").or_else(|| w.strip_prefix("http://")).unwrap_or(w);
        let w = w.strip_prefix("www.").or_else(|| w.strip_prefix("m.")).or_else(|| w.strip_prefix("music.")).unwrap_or(w);
        if let Some(rest) = w.strip_prefix("youtu.be/") {
            if let Some(id) = valid_id(rest) {
                return Some(id);
            }
        }
        if let Some(rest) = w.strip_prefix("youtube.com/").or_else(|| w.strip_prefix("youtube-nocookie.com/")) {
            for prefix in ["shorts/", "live/", "embed/", "v/"] {
                if let Some(id) = rest.strip_prefix(prefix).and_then(valid_id) {
                    return Some(id);
                }
            }
            if let Some(q) = rest.strip_prefix("watch").and_then(|r| r.split_once('?')).map(|(_, q)| q).or_else(|| rest.strip_prefix("watch?")) {
                if let Some(id) = q.split('&').find_map(|kv| kv.strip_prefix("v=")).and_then(valid_id) {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// "12:34", or "1:02:03" for an hour or more.
pub fn stamp(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A link to a moment in the video.
pub fn link(id: &str, secs: u64) -> String {
    if secs == 0 {
        format!("https://youtu.be/{id}")
    } else {
        format!("https://youtu.be/{id}?t={secs}")
    }
}

// ---------- the player API and captions ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub lang: String,
    /// Auto-generated (speech recognition).
    pub asr: bool,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub seconds: u64,
    pub thumbnail: String,
    pub tracks: Vec<Track>,
}

/// The video's details and caption tracks from a player reply.
pub fn parse_player(v: &Value) -> AppResult<VideoInfo> {
    let status = v["playabilityStatus"]["status"].as_str().unwrap_or("");
    if status != "OK" {
        let reason = v["playabilityStatus"]["reason"].as_str().unwrap_or("it isn't available");
        return Err(AppError::msg(format!("YouTube won't play this video here: {reason}")));
    }
    let d = &v["videoDetails"];
    let tracks = v["captions"]["playerCaptionsTracklistRenderer"]["captionTracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some(Track { lang: t["languageCode"].as_str()?.to_string(), asr: t["kind"] == "asr", url: t["baseUrl"].as_str()?.to_string() }))
        .collect();
    let thumbnail = d["thumbnail"]["thumbnails"].as_array().and_then(|a| a.last()).and_then(|t| t["url"].as_str()).map(str::to_string).unwrap_or_default();
    Ok(VideoInfo {
        id: d["videoId"].as_str().unwrap_or("").to_string(),
        title: d["title"].as_str().unwrap_or("").to_string(),
        channel: d["author"].as_str().unwrap_or("").to_string(),
        seconds: d["lengthSeconds"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        thumbnail,
        tracks,
    })
}

/// The best track: the wanted language (human-made first), else any
/// human-made track, else auto-generated in the wanted language, else any.
pub fn pick_track<'a>(tracks: &'a [Track], lang: &str) -> Option<&'a Track> {
    let same = |t: &&Track| t.lang == lang || t.lang.split('-').next() == Some(lang);
    tracks.iter().find(|t| same(t) && !t.asr).or_else(|| tracks.iter().find(|t| same(t))).or_else(|| tracks.iter().find(|t| !t.asr)).or_else(|| tracks.first())
}

/// Clients the player API is asked as, in order (versions kept current here).
const CLIENTS: &[(&str, &str)] = &[("ANDROID", "20.10.38"), ("IOS", "20.10.4"), ("WEB", "2.20250312.04.00")];

async fn player(net: &reqwest::Client, id: &str) -> AppResult<VideoInfo> {
    let mut last = AppError::msg("YouTube didn't answer");
    for (name, version) in CLIENTS {
        let mut client = json!({ "clientName": name, "clientVersion": version, "hl": "en" });
        if *name == "ANDROID" {
            client["androidSdkVersion"] = json!(30);
        }
        let r = net
            .post("https://www.youtube.com/youtubei/v1/player")
            .json(&json!({ "context": { "client": client }, "videoId": id }))
            .timeout(Duration::from_secs(15))
            .send()
            .await;
        let v: Value = match r {
            Ok(r) if r.status().is_success() => match r.json().await {
                Ok(v) => v,
                Err(e) => {
                    last = e.into();
                    continue;
                }
            },
            Ok(r) => {
                last = AppError::msg(format!("YouTube answered {}", r.status()));
                continue;
            }
            Err(e) => {
                last = e.into();
                continue;
            }
        };
        match parse_player(&v) {
            Ok(info) if !info.tracks.is_empty() => return Ok(info),
            Ok(info) => last = AppError::msg(format!("“{}” has no captions, so BYTE can't read it yet", info.title)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// One caption line.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: u64,
    pub text: String,
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'")
}

/// Caption lines from YouTube's timed text (format 3 `<p t= d=>` with
/// optional `<s>` words, or the older `<text start= dur=>`). Lines that are
/// only "[Music]" or "♪" are dropped.
pub fn parse_timedtext(xml: &str) -> Vec<Cue> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    let mut cur: Option<(u64, String)> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"p" | b"text" => {
                    let start = e.attributes().flatten().find_map(|a| match a.key.as_ref() {
                        b"t" => String::from_utf8_lossy(&a.value).parse::<u64>().ok(),
                        b"start" => String::from_utf8_lossy(&a.value).parse::<f64>().ok().map(|s| (s * 1000.0) as u64),
                        _ => None,
                    });
                    cur = Some((start.unwrap_or(0), String::new()));
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if let Some((_, text)) = cur.as_mut() {
                    let raw = String::from_utf8_lossy(t.as_ref()).into_owned();
                    text.push_str(&unescape(&raw));
                }
            }
            Ok(Event::End(e)) if matches!(e.name().as_ref(), b"p" | b"text") => {
                if let Some((start, text)) = cur.take() {
                    // Older captions escape twice ("&amp;#39;").
                    let text = unescape(&text).split_whitespace().collect::<Vec<_>>().join(" ");
                    let bare = text.trim_matches(|c: char| c == '♪' || c == '[' || c == ']' || c.is_whitespace()).to_lowercase();
                    if !bare.is_empty() && !["music", "applause", "laughter", "music playing"].contains(&bare.as_str()) {
                        out.push(Cue { start_ms: start, text });
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The transcript in blocks of about `secs` seconds, each labelled "[mm:ss]":
/// (start second, text).
pub fn chunks(cues: &[Cue], secs: u64) -> Vec<(u64, String)> {
    let mut out: Vec<(u64, String)> = Vec::new();
    for c in cues {
        let s = c.start_ms / 1000;
        match out.last_mut() {
            Some((start, text)) if s < *start + secs => {
                text.push(' ');
                text.push_str(&c.text);
            }
            _ => out.push((s, c.text.clone())),
        }
    }
    out
}

/// A video's details and its transcript (cached for a day).
pub async fn transcript(net: &reqwest::Client, id: &str, lang: &str) -> AppResult<(VideoInfo, Vec<Cue>, Track)> {
    let key = format!("{id}|{lang}");
    if let Some(hit) = crate::tools::cache::TRANSCRIPTS.get(&key) {
        return Ok(hit);
    }
    let info = player(net, id).await?;
    let track = pick_track(&info.tracks, lang).cloned().ok_or_else(|| AppError::msg("this video has no captions"))?;
    let xml = net.get(&track.url).timeout(Duration::from_secs(15)).send().await?.text().await?;
    let cues = parse_timedtext(&xml);
    if cues.is_empty() {
        return Err(AppError::msg(format!("the captions for “{}” came back empty", info.title)));
    }
    let size = cues.iter().map(|c| c.text.len() + 16).sum::<usize>();
    let out = (info, cues, track);
    crate::tools::cache::TRANSCRIPTS.put(&key, out.clone(), size);
    Ok(out)
}

/// A video without captions: its audio from the video helper (media.rs), transcribed on this Mac (voice.rs).
async fn spoken_transcript(turn: &Turn<'_>, id: &str, lang: &str) -> AppResult<(VideoInfo, Vec<Cue>, Track)> {
    use tauri::Manager;
    let key = format!("{id}|spoken|{lang}");
    if let Some(hit) = crate::tools::cache::TRANSCRIPTS.get(&key) {
        return Ok(hit);
    }
    let app = turn.app.ok_or_else(|| AppError::msg("transcribing videos needs the app"))?;
    let state = app.state::<crate::state::AppState>();
    let (chosen, language, voice_on) = {
        let s = state.settings.lock().await;
        (s.voice_model.clone(), s.voice_language.clone(), s.voice_enabled)
    };
    let models = &state.paths.models;
    if !voice_on || crate::voice::pick(models, &chosen).is_none() {
        return Err(AppError::msg("transcribing a video needs voice input and a speech model (Settings → Models → Voice)"));
    }
    let tmp = tempfile::tempdir()?;
    let (meta, audio) = crate::media::video_audio(&state.paths.data, id, tmp.path()).await?;
    let wav = crate::voice::wav_16k(&audio, &tmp.path().join("audio16k.wav")).await?;
    // "Detect it" can follow the question's language; an explicit setting wins.
    let lang = if language == "auto" { lang } else { language.as_str() };
    let segments = crate::voice::transcribe_segments(Some(app), models, &chosen, lang, &wav).await?;
    if segments.is_empty() {
        return Err(AppError::msg("no speech was heard in the video"));
    }
    let cues: Vec<Cue> = segments.iter().map(|s| Cue { start_ms: s.start_ms, text: s.text.clone() }).collect();
    let seconds = if meta.seconds > 0 { meta.seconds } else { segments.last().map(|s| s.end_ms / 1000).unwrap_or(0) };
    let info = VideoInfo {
        id: id.to_string(),
        title: if meta.title.is_empty() { "This video".into() } else { meta.title },
        channel: meta.channel,
        seconds,
        thumbnail: meta.thumbnail,
        tracks: vec![],
    };
    // An empty url marks a transcript BYTE made itself.
    let track = Track { lang: lang.to_string(), asr: true, url: String::new() };
    let size = cues.iter().map(|c| c.text.len() + 16).sum::<usize>();
    let out = (info, cues, track);
    crate::tools::cache::TRANSCRIPTS.put(&key, out.clone(), size);
    Ok(out)
}

// ---------- the card ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    /// Second it starts at.
    pub start: u64,
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPoint {
    pub start: u64,
    pub text: String,
}

/// A video summary shown as a card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCard {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub seconds: u64,
    pub thumbnail: String,
    pub language: String,
    /// The captions were auto-generated (may have mistakes).
    pub auto_captions: bool,
    /// There were no captions: BYTE transcribed the audio itself.
    #[serde(default)]
    pub transcribed: bool,
    pub tldr: String,
    pub key_points: Vec<KeyPoint>,
    pub chapters: Vec<Chapter>,
}

fn summary_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tldr": { "type": "string" },
            "keyPoints": { "type": "array", "maxItems": 8, "items": {
                "type": "object", "properties": { "start": { "type": "integer" }, "text": { "type": "string" } }, "required": ["start", "text"] } },
            "chapters": { "type": "array", "minItems": 1, "maxItems": 12, "items": {
                "type": "object", "properties": { "start": { "type": "integer" }, "title": { "type": "string" }, "summary": { "type": "string" } }, "required": ["start", "title", "summary"] } }
        },
        "required": ["tldr", "keyPoints", "chapters"]
    })
}

/// A summary from the model's reply: chapters in order, inside the video,
/// no two at the same second; key points likewise.
pub fn parse_summary(reply: &str, seconds: u64) -> Option<(String, Vec<KeyPoint>, Vec<Chapter>)> {
    let v = research::lenient_json(reply);
    let within = |s: u64| seconds == 0 || s < seconds;
    let text = |x: &Value, n: usize| -> String { x.as_str().unwrap_or("").trim().chars().take(n).collect() };
    let mut chapters: Vec<Chapter> = v["chapters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let start = c["start"].as_u64().or_else(|| c["start"].as_f64().map(|f| f.max(0.0) as u64))?;
            let title = text(&c["title"], 80);
            (!title.is_empty() && within(start)).then(|| Chapter { start, title, summary: text(&c["summary"], 400) })
        })
        .collect();
    chapters.sort_by_key(|c| c.start);
    chapters.dedup_by_key(|c| c.start);
    chapters.truncate(16);
    let mut points: Vec<KeyPoint> = v["keyPoints"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let start = p["start"].as_u64().unwrap_or(0);
            let t = text(&p["text"], 240);
            (!t.is_empty() && within(start)).then_some(KeyPoint { start, text: t })
        })
        .take(8)
        .collect();
    points.sort_by_key(|p| p.start);
    let tldr = text(&v["tldr"], 400);
    (!chapters.is_empty() || !tldr.is_empty()).then_some((tldr, points, chapters))
}

// ---------- when it applies ----------

const SUMMARY_WORDS: &[&str] = &["summar", "tl;dr", "tldr", "recap", "key points", "main points", "takeaways", "chapters", "notes on", "what is this video", "what's this video", "what is it about", "what's it about", "overview", "gist", "break down", "breakdown"];

/// What to do with a video: summarize it, or answer a question about it.
#[derive(Debug, Clone, PartialEq)]
pub enum VideoAsk {
    Summary(String),
    Question(String),
}

/// A video request in this message, or a follow-up about a video linked
/// earlier in the chat (in the last three questions).
pub fn video_ask(question: &str, history: &[ChatMessage]) -> Option<VideoAsk> {
    let lower = question.to_lowercase();
    if let Some(id) = video_id(question) {
        // The link alone, or asking for a summary: summarize.
        let rest: String = question.split_whitespace().filter(|w| video_id(w).is_none()).collect::<Vec<_>>().join(" ");
        let alone = rest.split_whitespace().count() <= 2;
        return Some(if alone || SUMMARY_WORDS.iter().any(|w| lower.contains(w)) { VideoAsk::Summary(id) } else { VideoAsk::Question(id) });
    }
    // A follow-up about "the video" (or he/she/they said…) after a link.
    let padded = format!(" {} ", lower.replace(|c: char| !c.is_alphanumeric() && c != '\'', " "));
    let about_it = [" video ", " he ", " she ", " they ", " mention ", " mentions ", " mentioned ", " talk about ", " talks about ", " at the end ", " in the beginning ", " timestamp ", " minute ", " minutes "].iter().any(|w| padded.contains(w));
    if !about_it {
        return None;
    }
    let earlier = history.iter().rev().filter(|m| m.role == "user").skip(1).take(3).find_map(|m| video_id(&m.content))?;
    Some(if SUMMARY_WORDS.iter().any(|w| lower.contains(w)) { VideoAsk::Summary(earlier) } else { VideoAsk::Question(earlier) })
}

pub fn applies(web: bool, question: &str, history: &[ChatMessage]) -> bool {
    web && video_ask(question, history).is_some()
}

/// The caption language to prefer (from words in the question; English otherwise).
fn wanted_lang(question: &str) -> &'static str {
    let q = question.to_lowercase();
    [("spanish", "es"), ("español", "es"), ("french", "fr"), ("german", "de"), ("portuguese", "pt"), ("italian", "it"), ("japanese", "ja"), ("korean", "ko"), ("chinese", "zh")]
        .iter()
        .find(|(w, _)| q.contains(w))
        .map(|(_, l)| *l)
        .unwrap_or("en")
}

pub const SUMMARY_RULES: &str = "The user sees a card with the video, its chapters and key points. Write the summary \
as a short report: a one-line **TL;DR:** blockquote, then the main ideas as `##` sections or bullets. Put a timestamp \
link after each point, written exactly like [12:34](https://youtu.be/ID?t=754), using only moments from the notes. \
Say briefly what kind of video it is and who it's for. If the captions are auto-generated, names and numbers may be \
misheard: say so if something looks off. Don't invent anything that isn't in the transcript.";

pub const QA_RULES: &str = "Answer the question from these transcript excerpts only. After each point, link the \
moment it comes from, written exactly like [12:34](https://youtu.be/ID?t=754). If the excerpts don't cover the \
question, say the video doesn't seem to address it and what it does talk about instead.";

// ---------- the pipeline ----------

pub async fn run(turn: &Turn<'_>, question: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(ask) = video_ask(question, turn.history) else { return Ok(None) };
    let c = Ctx { turn, cancel, send };
    let id = match &ask {
        VideoAsk::Summary(id) | VideoAsk::Question(id) => id.clone(),
    };
    c.call("byte_yt", "get_transcript", json!({ "video": id }))?;
    let (info, cues, track) = match c.cancellable(transcript(turn.net, &id, wanted_lang(question))).await? {
        Ok(t) => t,
        Err(e) => {
            c.result("byte_yt", false, e.to_string())?;
            // No captions: get the audio with the video helper and transcribe it on this Mac.
            c.call("byte_ytaudio", "transcribe_video", json!({ "video": id }))?;
            match c.cancellable(spoken_transcript(turn, &id, wanted_lang(question))).await? {
                Ok(t) => {
                    c.result("byte_ytaudio", true, format!("Transcribed {} of audio", stamp(t.0.seconds)))?;
                    t
                }
                Err(e2) => {
                    c.result("byte_ytaudio", false, e2.to_string())?;
                    let note = format!("BYTE couldn't read this video's transcript ({e}), and couldn't transcribe its audio either: {e2}. Tell the user plainly, including what would fix it. Don't guess what the video says.");
                    return Ok(Some((SourceBook::default(), note)));
                }
            }
        }
    };
    c.result("byte_yt", true, format!("{} · {} · {} captions", info.title.chars().take(50).collect::<String>(), stamp(info.seconds), if track.asr { "auto" } else { track.lang.as_str() }))?;
    let mut book = SourceBook::default();
    let n = book.add(&format!("{} ({})", info.title, info.channel), &link(&id, 0), "");
    book.mark_read(&link(&id, 0), &format!("{} ({})", info.title, info.channel));
    (c.send)(ChatEvent::Sources { sources: book.sources.clone() })?;
    let captions = if track.url.is_empty() { "none; BYTE transcribed its audio on this Mac (may have small mistakes)" } else if track.asr { "auto-generated" } else { "human-made" };
    let head = format!("Video [{n}]: “{}” by {} ({}), id {id}. Captions: {captions}.", info.title, info.channel, stamp(info.seconds));

    match ask {
        VideoAsk::Question(_) => {
            let blocks = chunks(&cues, 90);
            let texts: Vec<(u32, String)> = blocks.iter().enumerate().map(|(i, (_, t))| (i as u32, t.clone())).collect();
            let budget = research::notes_budget(turn, used_tokens, 0.4);
            c.call("byte_ytrank", "rank_passages", json!({}))?;
            let (mut picks, _) = c.cancellable(research::rank_texts(turn, &texts, question, "", budget, 2)).await?;
            picks.sort_by_key(|p| (p.n, p.order));
            c.result("byte_ytrank", !picks.is_empty(), format!("{} moments in the video", picks.len()))?;
            let excerpts: Vec<String> = picks
                .iter()
                .map(|p| {
                    let start = blocks.get(p.n as usize).map(|b| b.0).unwrap_or(0);
                    format!("[{}]({}) {}", stamp(start), link(&id, start), p.text.trim())
                })
                .collect();
            Ok(Some((book, format!("{head}\nQuestion: {question}\n\nTranscript excerpts:\n{}\n\n{}", excerpts.join("\n\n"), QA_RULES.replace("ID", &id)))))
        }
        VideoAsk::Summary(_) => {
            c.call("byte_ytsum", "summarize_video", json!({}))?;
            let budget = research::notes_budget(turn, used_tokens, 0.45);
            let blocks = chunks(&cues, 60);
            let full: String = blocks.iter().map(|(s, t)| format!("[{}s] {t}\n", s)).collect();
            let ask = |part: &str, from: u64, to: u64| {
                format!(
                    "Video: “{}” by {} ({} long). Transcript{} (each line starts with its time in seconds):\n{part}\n\nSummarize it: a TL;DR (two sentences), up to 8 key points with the second each is said, and chapters (a title and a two-sentence summary each, with the second each starts). Use only times that appear in the transcript.",
                    info.title,
                    info.channel,
                    stamp(info.seconds),
                    if from > 0 || to < info.seconds { format!(" from {} to {}", stamp(from), stamp(to)) } else { String::new() }
                )
            };
            let system = "You summarize videos accurately from their transcripts. Reply only with JSON.";
            let (tldr, points, chapters) = if full.len() <= budget {
                let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, system, &ask(&full, 0, info.seconds), summary_schema(), 1800)).await?.unwrap_or_default();
                parse_summary(&reply, info.seconds).unwrap_or_default()
            } else {
                // Long video: summarize parts, then merge.
                let mut parts: Vec<(u64, String)> = Vec::new();
                for (s, t) in &blocks {
                    match parts.last_mut() {
                        Some((_, text)) if text.len() + t.len() < budget.max(4000) => text.push_str(&format!("[{s}s] {t}\n")),
                        _ => parts.push((*s, format!("[{s}s] {t}\n"))),
                    }
                }
                let mut all_points = Vec::new();
                let mut all_chapters = Vec::new();
                let mut tldrs = Vec::new();
                for (i, (from, text)) in parts.iter().enumerate() {
                    let to = parts.get(i + 1).map(|p| p.0).unwrap_or(info.seconds);
                    let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, system, &ask(text, *from, to), summary_schema(), 1400)).await?.unwrap_or_default();
                    if let Some((t, p, ch)) = parse_summary(&reply, info.seconds) {
                        tldrs.push(t);
                        all_points.extend(p);
                        all_chapters.extend(ch);
                    }
                }
                all_chapters.sort_by_key(|c| c.start);
                all_points.sort_by_key(|p| p.start);
                all_points.truncate(10);
                (tldrs.join(" "), all_points, all_chapters)
            };
            c.result("byte_ytsum", !chapters.is_empty(), format!("{} chapters", chapters.len()))?;
            let card = VideoCard {
                id: id.clone(),
                title: info.title.clone(),
                channel: info.channel.clone(),
                seconds: info.seconds,
                thumbnail: if info.thumbnail.is_empty() { format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg") } else { info.thumbnail.clone() },
                language: track.lang.clone(),
                auto_captions: track.asr,
                transcribed: track.url.is_empty(),
                tldr: tldr.clone(),
                key_points: points.clone(),
                chapters: chapters.clone(),
            };
            if !chapters.is_empty() || !tldr.is_empty() {
                (c.send)(ChatEvent::Video(card))?;
            }
            let mut notes = format!("{head}\nTL;DR: {tldr}\n\nKey points:\n");
            for p in &points {
                notes.push_str(&format!("- [{}]({}) {}\n", stamp(p.start), link(&id, p.start), p.text));
            }
            notes.push_str("\nChapters:\n");
            for ch in &chapters {
                notes.push_str(&format!("- [{}]({}) {}: {}\n", stamp(ch.start), link(&id, ch.start), ch.title, ch.summary));
            }
            if chapters.is_empty() {
                // The summary step failed: give the model the start of the transcript instead.
                notes.push_str(&format!("\nTranscript (start):\n{}\n", full.chars().take(budget).collect::<String>()));
            }
            Ok(Some((book, format!("{notes}\n{}", SUMMARY_RULES.replace("ID", &id)))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn links_are_recognised() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "summarize https://youtu.be/dQw4w9WgXcQ?si=abc please",
            "youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=10",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ.",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=x",
            "(https://www.youtube.com/embed/dQw4w9WgXcQ)",
        ] {
            assert_eq!(video_id(url).as_deref(), Some("dQw4w9WgXcQ"), "{url}");
        }
        assert_eq!(video_id("https://vimeo.com/123456789"), None);
        assert_eq!(video_id("https://youtube.com/watch?v=short"), None);
        assert_eq!(video_id("https://youtube.com/@channel"), None);
        assert_eq!((stamp(754), stamp(3723), stamp(5)), ("12:34".into(), "1:02:03".into(), "0:05".into()));
        assert_eq!(link("abcdefghijk", 754), "https://youtu.be/abcdefghijk?t=754");
        assert_eq!(link("abcdefghijk", 0), "https://youtu.be/abcdefghijk");
    }

    #[test]
    fn player_replies_and_tracks() {
        let v: Value = serde_json::from_str(&fixture("youtube_player.json")).unwrap();
        let info = parse_player(&v).unwrap();
        assert_eq!(info.id, "dQw4w9WgXcQ");
        assert_eq!(info.channel, "Rick Astley");
        assert_eq!(info.seconds, 213);
        assert!(info.thumbnail.contains("hqdefault"));
        assert_eq!(info.tracks.len(), 6);
        let en = pick_track(&info.tracks, "en").unwrap();
        assert!(!en.asr && en.lang == "en");
        assert_eq!(pick_track(&info.tracks, "de").unwrap().lang, "de-DE");
        // No track in that language: a human-made one in another language beats auto-generated.
        assert!(!pick_track(&info.tracks, "ko").unwrap().asr);
        let only_asr = vec![Track { lang: "en".into(), asr: true, url: "u".into() }];
        assert!(pick_track(&only_asr, "fr").unwrap().asr);
        let blocked = json!({"playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Sign in to confirm your age"}});
        assert!(parse_player(&blocked).unwrap_err().to_string().contains("confirm your age"));
    }

    #[test]
    fn captions_are_read() {
        let cues = parse_timedtext(&fixture("timedtext_manual.xml"));
        // "[♪♪♪]" is dropped; entities and line breaks are cleaned.
        assert_eq!(cues[0], Cue { start_ms: 18640, text: "♪ We're no strangers to love ♪".into() });
        assert!(cues.iter().any(|c| c.text == "♪ You know the rules and so do I ♪"));
        let asr = parse_timedtext(&fixture("timedtext_asr.xml"));
        assert_eq!(asr, vec![Cue { start_ms: 18800, text: "We're no strangers to".into() }]);
        let legacy = parse_timedtext(r#"<transcript><text start="1.5" dur="2">Hello &amp;amp; welcome</text><text start="4" dur="1">[Music]</text></transcript>"#);
        assert_eq!(legacy, vec![Cue { start_ms: 1500, text: "Hello & welcome".into() }]);
        let blocks = chunks(&[Cue { start_ms: 0, text: "a".into() }, Cue { start_ms: 50_000, text: "b".into() }, Cue { start_ms: 61_000, text: "c".into() }], 60);
        assert_eq!(blocks, vec![(0, "a b".into()), (61, "c".into())]);
    }

    #[test]
    fn summaries_are_cleaned() {
        let reply = r#"{"tldr":"A classic pop song about commitment.","keyPoints":[{"start":90,"text":"Chorus"},{"start":500,"text":"Past the end"},{"start":19,"text":"Opening line"}],
            "chapters":[{"start":85,"title":"Chorus","summary":"The famous chorus."},{"start":0,"title":"Intro","summary":""},{"start":85,"title":"Duplicate","summary":""},{"start":999,"title":"Too late","summary":""},{"start":30,"title":"","summary":"no title"}]}"#;
        let (tldr, points, chapters) = parse_summary(reply, 213).unwrap();
        assert_eq!(tldr, "A classic pop song about commitment.");
        assert_eq!(points.iter().map(|p| p.start).collect::<Vec<_>>(), vec![19, 90]);
        assert_eq!(chapters.iter().map(|c| (c.start, c.title.as_str())).collect::<Vec<_>>(), vec![(0, "Intro"), (85, "Chorus")]);
        assert!(parse_summary("nope", 100).is_none());
        let v = serde_json::to_value(VideoCard { id: "x".into(), title: "t".into(), channel: "c".into(), seconds: 1, thumbnail: String::new(), language: "en".into(), auto_captions: true, transcribed: false, tldr, key_points: points, chapters }).unwrap();
        assert_eq!(v["autoCaptions"], true);
        assert_eq!(v["keyPoints"][0]["start"], 19);
    }

    #[test]
    fn video_requests_and_follow_ups() {
        let url = "https://youtu.be/dQw4w9WgXcQ";
        assert_eq!(video_ask(url, &[]), Some(VideoAsk::Summary("dQw4w9WgXcQ".into())));
        assert_eq!(video_ask(&format!("Summarize this: {url}"), &[]), Some(VideoAsk::Summary("dQw4w9WgXcQ".into())));
        assert_eq!(video_ask(&format!("What does he say about commitment in {url}?"), &[]), Some(VideoAsk::Question("dQw4w9WgXcQ".into())));
        let history = vec![ChatMessage::new("user", url), ChatMessage::new("assistant", "summary"), ChatMessage::new("user", "What does he say at the end of the video?")];
        assert_eq!(video_ask("What does he say at the end of the video?", &history), Some(VideoAsk::Question("dQw4w9WgXcQ".into())));
        // Unrelated follow-ups aren't hijacked.
        let history = vec![ChatMessage::new("user", url), ChatMessage::new("assistant", "summary"), ChatMessage::new("user", "What's the weather tomorrow?")];
        assert_eq!(video_ask("What's the weather tomorrow?", &history), None);
        assert!(!applies(false, url, &[]));
        assert_eq!(wanted_lang("summarize in spanish"), "es");
    }

    /// Live: `BYTE_TEST_WEB=1 cargo test live_transcript -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_transcript() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let net = crate::tools::fetch::web_client();
        let (info, cues, track) = transcript(&net, "dQw4w9WgXcQ", "en").await.expect("transcript");
        eprintln!("{} by {} ({}s), {} cues, track {} asr={}", info.title, info.channel, info.seconds, cues.len(), track.lang, track.asr);
        assert!(cues.len() > 10);
    }

    /// Real engine + web. Needs BYTE_TEST_LLAMA_SERVER, BYTE_TEST_MODEL and BYTE_TEST_WEB=1.
    #[tokio::test]
    #[ignore]
    async fn e2e_youtube() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let Ok(model) = std::env::var("BYTE_TEST_MODEL") else { return };
        let Some((_server, mut ep)) = crate::chat::e2e_support::start_server_with(&model, &["-c".into(), "16384".into()], None).await else { return };
        ep.context = 16384;
        let dir = tempfile::tempdir().unwrap();
        let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = crate::tools::fetch::web_client();
        let video = std::env::var("BYTE_TEST_VIDEO").unwrap_or_else(|_| "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into());
        let mut history = vec![ChatMessage::new("user", format!("Summarize {video}"))];
        for step in 0..2 {
            let q = history.last().unwrap().content.clone();
            let system = crate::prompt::system_prompt(chrono::Local::now(), crate::settings::Mode::Auto, true, None);
            let plan = crate::router::plan_turn(crate::settings::Mode::Auto, crate::settings::ThinkingPref::Off, &q);
            let (ch, seen) = crate::chat::e2e_support::collecting_channel();
            let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: crate::settings::Mode::Auto, web: true, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false, modules: Default::default() };
            crate::agent::run(turn, CancellationToken::new(), &ch).await.unwrap();
            let ev = seen.lock().unwrap().clone();
            for e in ev.iter().filter(|e| e["kind"] == "toolResult" || e["kind"] == "video") {
                eprintln!("{}", e.to_string().chars().take(900).collect::<String>());
            }
            let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
            eprintln!("--- {content}");
            if step == 0 {
                assert!(ev.iter().any(|e| e["kind"] == "video"), "no video card");
                history.push(ChatMessage::new("assistant", content));
                history.push(ChatMessage::new("user", "What does he promise in the video?"));
            } else {
                assert!(ev.iter().any(|e| e["kind"] == "toolCall" && e["name"] == "get_transcript"));
            }
        }
    }
}
