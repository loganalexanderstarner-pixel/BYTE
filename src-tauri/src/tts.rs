//! BYTE's voices (Phase 11): free, open neural voices (Kokoro, Piper, Kitten,
//! Supertonic, Pocket: see `voices.rs` for the catalog), run on this Mac by
//! sherpa-onnx's speech tool (`sherpa-tts` sidecar, built by
//! `scripts/build-sherpa.sh`).
//!
//! Smooth, human-paced speech: an answer is cut into chunks (the first sentence
//! alone, so BYTE starts talking at once; then a few sentences at a time, never
//! across a paragraph). Chunks are made one after another while earlier ones
//! play, trimmed, faded in and out, and followed by a short pause (longer after a
//! paragraph). All audio goes through one continuous output stream, so there's
//! no gap or click between them. Text can be fed while the answer is still being
//! written (`speech_feed`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(target_os = "macos")]
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::voices::{self, Package, Speaker};

const SIDECAR: &str = "sherpa-tts";
pub const SAMPLE_RATE: u32 = 24_000;
/// After the first sentence, chunks of about this many characters (a few sentences).
const CHUNK: usize = 320;

/// The chosen voice can speak here: unpacked, and on a Mac (where the audio output is).
pub fn usable(models_dir: &Path, voice: &str) -> bool {
    cfg!(target_os = "macos") && voices::ready(models_dir, voices::pick(voice).0)
}

// ------------------------------------------------------------------ text → chunks

/// Sentences of already-speakable text (ends kept).
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let next = chars.get(i + 1).copied();
        // A sentence ends at . ! ? followed by a space (not "e.g." / "3.5").
        if matches!(c, '.' | '!' | '?') && next.is_none_or(char::is_whitespace) {
            let prev_word: String = cur.trim_end_matches(['.', '!', '?']).rsplit(' ').next().unwrap_or("").to_lowercase();
            if !["e.g", "i.e", "mr", "mrs", "ms", "dr", "vs", "etc", "st"].contains(&prev_word.as_str()) {
                out.push(cur.trim().to_string());
                cur.clear();
            }
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Sentences with whether each ends a paragraph (paragraphs are separated by a blank line).
pub fn pieces(text: &str) -> Vec<(String, bool)> {
    let mut out = vec![];
    for para in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let s = sentences(para);
        let n = s.len();
        out.extend(s.into_iter().enumerate().map(|(i, x)| (x, i + 1 == n)));
    }
    out
}

/// A chunk to make: its text and whether it ends a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub text: String,
    pub para_end: bool,
}

/// Chunks to make one after another. With `first`, the opening sentence goes alone (a quick start; joined with
/// the next while tiny); then sentences are grouped up to `CHUNK` characters. A chunk never crosses a paragraph.
pub fn chunks(pieces: &[(String, bool)], first: bool) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = vec![];
    for (s, end) in pieces {
        let n = out.len();
        match out.last_mut() {
            Some(last) if !last.para_end && ((first && n == 1 && last.text.len() < 24) || ((!first || n > 1) && last.text.len() + s.len() < CHUNK)) => {
                last.text.push(' ');
                last.text.push_str(s);
                last.para_end = *end;
            }
            _ => out.push(Chunk { text: s.clone(), para_end: *end }),
        }
    }
    out
}

/// Quiet after a chunk: a breath between sentences, longer after a paragraph; Calm pauses longer, Lively shorter.
pub fn pause_ms(para_end: bool, style: &str) -> u32 {
    let base = if para_end { 380.0 } else { 110.0 };
    let k = match style {
        "calm" => 1.4,
        "lively" => 0.7,
        _ => 1.0,
    };
    (base * k) as u32
}

/// Questions a touch slower, exclamations a touch brisker, like people say them.
pub fn chunk_speed(text: &str, speed: f32) -> f32 {
    match text.trim_end().chars().last() {
        Some('?') => speed * 0.97,
        Some('!') => speed * 1.03,
        _ => speed,
    }
}

/// Trims silence at both ends (keeping 20 ms), fades in and out over 8 ms, then adds `pause_ms` of quiet.
pub fn shape(mut s: Vec<f32>, rate: u32, pause_ms: u32) -> Vec<f32> {
    let keep = rate as usize / 50;
    let loud = |x: &f32| x.abs() > 0.004;
    let start = s.iter().position(loud).map(|i| i.saturating_sub(keep)).unwrap_or(0);
    let end = s.iter().rposition(loud).map(|i| (i + keep).min(s.len())).unwrap_or(s.len());
    if start < end {
        s = s[start..end].to_vec();
    }
    let fade = (rate as usize / 125).min(s.len() / 2);
    let n = s.len();
    for i in 0..fade {
        let g = i as f32 / fade as f32;
        s[i] *= g;
        s[n - 1 - i] *= g;
    }
    s.extend(std::iter::repeat_n(0.0, (rate as u64 * pause_ms as u64 / 1000) as usize));
    s
}

/// A WAV file's samples as f32 (16-bit PCM or 32-bit float), plus its sample rate.
pub fn read_wav(path: &Path) -> Option<(Vec<f32>, u32)> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut fmt, mut rate, mut bits, mut channels) = (1u16, 0u32, 16u16, 1u16);
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let size = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = &b[i + 8..(i + 8 + size).min(b.len())];
        if id == b"fmt " && body.len() >= 16 {
            fmt = u16::from_le_bytes([body[0], body[1]]);
            channels = u16::from_le_bytes([body[2], body[3]]).max(1);
            rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            let samples: Vec<f32> = match (fmt, bits) {
                (1, 16) => body.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
                (3, 32) => body.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                _ => return None,
            };
            let mono = samples.chunks(channels as usize).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
            return Some((mono, rate));
        }
        i += 8 + size + (size & 1);
    }
    None
}

/// Linear resampling (24 kHz voice → the speakers' rate).
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let n = (input.len() as u64 * to as u64 / from as u64) as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * from as f64 / to as f64;
            let j = pos as usize;
            let t = (pos - j as f64) as f32;
            let a = input[j.min(input.len() - 1)];
            let b = input[(j + 1).min(input.len() - 1)];
            a + (b - a) * t
        })
        .collect()
}

/// Makes one chunk of speech (24 kHz samples) with voice `speaker` of package `p`, unpacked in `dir`.
pub async fn synthesize(app: Option<&AppHandle>, p: &Package, dir: &Path, speaker: &Speaker, speed: f32, text: &str) -> AppResult<Vec<f32>> {
    let tmp = tempfile::Builder::new().prefix("byte-tts").suffix(".wav").tempfile()?;
    let args = voices::args(p, dir, speaker, speed, voices::threads(), tmp.path(), text)?;
    let ok = match (std::env::var("BYTE_TEST_SHERPA_TTS").ok().filter(|b| !b.is_empty()), app) {
        (Some(bin), _) => tokio::process::Command::new(bin).args(&args).kill_on_drop(true).output().await?.status.success(),
        (None, Some(app)) => {
            let cmd = crate::bundled::tool(&app, SIDECAR).map_err(|e| AppError::msg(format!("BYTE's voice engine is missing from this build: {e}")))?;
            cmd.args(args).output().await.map_err(|e| AppError::msg(format!("BYTE's voice engine didn't start: {e}")))?.status.success()
        }
        (None, None) => return Err(AppError::msg("BYTE's voice engine isn't available here.")),
    };
    if !ok {
        return Err(AppError::msg("BYTE's voice couldn't say that."));
    }
    let (samples, rate) = read_wav(tmp.path()).ok_or_else(|| AppError::msg("BYTE's voice made no audio."))?;
    Ok(resample(&samples, rate, SAMPLE_RATE))
}

// ------------------------------------------------------------------ speaking (sessions + one audio stream)

/// The current speaking session; a new one (or Stop) makes older chunks drop out.
static SESSION: AtomicU64 = AtomicU64::new(0);

struct Feed {
    id: String,
    /// Sentences already handed to the maker.
    sent: usize,
    tx: tokio::sync::mpsc::UnboundedSender<Option<Chunk>>,
}

static FEED: Mutex<Option<Feed>> = Mutex::new(None);
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Audio waiting to play (at `out_rate`), the rate of the output device, and whether the session has more coming.
#[derive(Default)]
pub struct Queue {
    pub samples: VecDeque<f32>,
    pub out_rate: u32,
    pub more: bool,
    pub session: u64,
}

pub fn queue() -> &'static Arc<Mutex<Queue>> {
    static Q: OnceLock<Arc<Mutex<Queue>>> = OnceLock::new();
    Q.get_or_init(|| Arc::new(Mutex::new(Queue { out_rate: SAMPLE_RATE, ..Default::default() })))
}

pub fn speaking() -> bool {
    queue().lock().map(|q| q.more || !q.samples.is_empty()).unwrap_or(false)
}

/// Ends whatever is being said.
pub fn stop() {
    SESSION.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut f) = FEED.lock() {
        *f = None;
    }
    if let Ok(mut q) = queue().lock() {
        q.samples.clear();
        q.more = false;
    }
}

/// Called by the output stream when the queue runs dry with nothing more coming.
pub fn finished(session: u64) {
    if session == SESSION.load(Ordering::SeqCst) {
        if let Some(app) = APP.get() {
            let _ = app.emit(crate::speech::DONE_EVENT, ());
        }
    }
}

/// How BYTE speaks: the voice ("package/speaker"), speed and style, and (optionally) a BYTE Cloud voice.
#[derive(Clone)]
pub struct Delivery {
    pub voice: String,
    pub speed: String,
    pub style: String,
    /// Make the speech on the BYTE cloud with this voice; the Mac's voice takes over if the cloud fails.
    pub cloud: Option<(crate::cloud::CloudClient, String)>,
}

/// Speaks (more of) answer `id`. `text` is the whole answer so far; complete sentences not yet spoken are made
/// and queued. `done`: the answer is finished (the last sentence counts too).
pub fn feed(app: &AppHandle, models_dir: &Path, how: &Delivery, id: &str, text: &str, done: bool) -> AppResult<()> {
    let _ = APP.set(app.clone());
    let mut all = pieces(&crate::speech::speakable(text));
    if !done && !all.is_empty() {
        all.pop(); // may still be growing
    }
    let mut guard = FEED.lock().map_err(|_| AppError::msg("voice busy"))?;
    if guard.as_ref().is_none_or(|f| f.id != id) {
        // A new answer: stop the old one, start a maker for this one.
        drop(guard);
        stop();
        let session = SESSION.load(Ordering::SeqCst);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Option<Chunk>>();
        start_output();
        if let Ok(mut q) = queue().lock() {
            q.more = true;
            q.session = session;
        }
        let (p, s) = voices::pick(&how.voice);
        spawn_maker(app.clone(), p, voices::model_dir(models_dir, p), s.clone(), how.clone(), session, rx);
        guard = FEED.lock().map_err(|_| AppError::msg("voice busy"))?;
        *guard = Some(Feed { id: id.to_string(), sent: 0, tx });
    }
    let f = guard.as_mut().expect("set above");
    let first = f.sent == 0;
    let new: Vec<(String, bool)> = all.iter().skip(f.sent).cloned().collect();
    f.sent = all.len().max(f.sent);
    for c in chunks(&new, first) {
        let _ = f.tx.send(Some(c));
    }
    if done {
        let _ = f.tx.send(None);
    }
    Ok(())
}

fn spawn_maker(app: AppHandle, p: &'static Package, dir: PathBuf, speaker: Speaker, how: Delivery, session: u64, mut rx: tokio::sync::mpsc::UnboundedReceiver<Option<Chunk>>) {
    let speed = voices::speed_factor(&how.speed, &how.style);
    let local_ready = dir.join(".ready").exists();
    tauri::async_runtime::spawn(async move {
        let mut cloud = how.cloud.clone();
        while let Some(next) = rx.recv().await {
            if SESSION.load(Ordering::SeqCst) != session {
                return;
            }
            let Some(chunk) = next else { break };
            let pace = chunk_speed(&chunk.text, speed);
            let made = match &cloud {
                Some((client, voice)) => match crate::cloud::voice::synthesize(client, &chunk.text, voice, &how.style, pace).await {
                    Ok(s) => Ok(s),
                    Err(e) => {
                        // The cloud stopped answering: the rest is spoken on this Mac (when a voice is there).
                        log::warn!("cloud voice: {}", crate::error::AppError::from(e));
                        cloud = None;
                        if local_ready { synthesize(Some(&app), p, &dir, &speaker, pace, &chunk.text).await } else { Err(AppError::msg("no voice on this Mac")) }
                    }
                },
                None => synthesize(Some(&app), p, &dir, &speaker, pace, &chunk.text).await,
            };
            match made {
                Ok(samples) => {
                    if SESSION.load(Ordering::SeqCst) != session {
                        return;
                    }
                    let shaped = shape(samples, SAMPLE_RATE, pause_ms(chunk.para_end, &how.style));
                    if let Ok(mut q) = queue().lock() {
                        let rate = q.out_rate;
                        q.samples.extend(resample(&shaped, SAMPLE_RATE, rate));
                    }
                }
                Err(e) => log::warn!("voice: {e}"),
            }
        }
        if let Ok(mut q) = queue().lock() {
            if q.session == session {
                q.more = false;
                if q.samples.is_empty() {
                    q.session = u64::MAX; // reported here, not again by the output stream
                    drop(q);
                    finished(session);
                }
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn start_output() {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.get().is_some() {
        return;
    }
    let _ = STARTED.set(());
    std::thread::spawn(|| {
        let stream = (|| {
            let dev = cpal::default_host().default_output_device()?;
            let cfg = dev.default_output_config().ok()?;
            let channels = cfg.channels() as usize;
            if let Ok(mut q) = queue().lock() {
                q.out_rate = cfg.sample_rate();
            }
            let q = queue().clone();
            let s = dev
                .build_output_stream(
                    cfg.into(),
                    move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        let mut ended = None;
                        if let Ok(mut q) = q.lock() {
                            for frame in out.chunks_mut(channels.max(1)) {
                                let s = q.samples.pop_front().unwrap_or(0.0);
                                frame.iter_mut().for_each(|x| *x = s);
                            }
                            if q.samples.is_empty() && !q.more && q.session != u64::MAX {
                                ended = Some(q.session);
                                q.session = u64::MAX; // report once
                            }
                        }
                        if let Some(session) = ended {
                            finished(session);
                        }
                    },
                    |e| log::warn!("voice output: {e}"),
                    None,
                )
                .ok()?;
            s.play().ok()?;
            Some(s)
        })();
        let Some(_stream) = stream else {
            log::warn!("voice: no audio output");
            return;
        };
        // The stream lives as long as BYTE does.
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn start_output() {}

// ------------------------------------------------------------------ commands

/// The BYTE cloud's voices (empty when it isn't connected or doesn't offer voices yet).
#[tauri::command]
pub async fn cloud_voices(state: State<'_, AppState>) -> AppResult<Vec<crate::cloud::voice::CloudVoice>> {
    let Ok(client) = state.cloud_client().await else { return Ok(vec![]) };
    Ok(crate::cloud::voice::voices(&client).await.unwrap_or_default())
}

/// The whole voice catalog (it doesn't change while BYTE runs).
#[tauri::command]
pub fn voices_catalog() -> Vec<Package> {
    voices::catalog().to_vec()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoicesStatus {
    /// Unpacked and ready.
    pub ready: Vec<String>,
    /// Downloaded but not unpacked yet.
    pub downloaded: Vec<String>,
    /// Can speak here (a Mac).
    pub can_speak: bool,
}

#[tauri::command]
pub async fn voices_status(state: State<'_, AppState>) -> AppResult<VoicesStatus> {
    let m = &state.paths.models;
    let (mut ready, mut downloaded) = (vec![], vec![]);
    for p in voices::catalog() {
        if voices::ready(m, p) {
            ready.push(p.id.clone());
        } else if voices::downloaded(m, p) {
            downloaded.push(p.id.clone());
        }
    }
    Ok(VoicesStatus { ready, downloaded, can_speak: cfg!(target_os = "macos") })
}

fn known(id: &str) -> AppResult<&'static Package> {
    voices::package(id).ok_or_else(|| AppError::msg("BYTE doesn't know that voice."))
}

#[tauri::command]
pub async fn tts_voice_download(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let p = known(&id)?;
    let d = voices::package_dir(&state.paths.models, p);
    std::fs::create_dir_all(&d)?;
    state.downloads.start_from(app, state.net.clone(), d, String::new(), voices::variant(p), voices::key(p), voices::release_url).await
}

/// Unpacks a voice after its download finished.
#[tauri::command]
pub async fn tts_voice_unpack(state: State<'_, AppState>, id: String) -> AppResult<()> {
    voices::unpack(&state.paths.models, known(&id)?).await
}

#[tauri::command]
pub async fn tts_voice_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let p = known(&id)?;
    state.downloads.pause(&voices::key(p)).await;
    stop();
    let d = voices::package_dir(&state.paths.models, p);
    if d.exists() {
        std::fs::remove_dir_all(d)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sentences_but_not_abbreviations() {
        let s = sentences("Hi there! The M5 is 3.5 times faster, e.g. for video. Is it worth it? Probably not");
        assert_eq!(s, vec!["Hi there!", "The M5 is 3.5 times faster, e.g. for video.", "Is it worth it?", "Probably not"]);
    }

    fn p(list: &[(&str, bool)]) -> Vec<(String, bool)> {
        list.iter().map(|(s, e)| (s.to_string(), *e)).collect()
    }

    #[test]
    fn pieces_know_paragraph_ends() {
        let x = pieces("Sure. Here it is.\n\nNext part! Done.");
        assert_eq!(x, p(&[("Sure.", false), ("Here it is.", true), ("Next part!", false), ("Done.", true)]));
    }

    #[test]
    fn first_chunk_is_quick_then_groups_within_paragraphs() {
        let s = p(&[("Sure.", false), ("The M5 is faster.", false), ("It has a new chip.", false), ("Battery life is the same.", true), ("Most people should wait.", true)]);
        let c = chunks(&s, true);
        // A tiny first sentence takes the next ones until the opening is long enough to be worth making alone;
        // the rest group up to the chunk size, but never across a paragraph.
        assert_eq!(c[0], Chunk { text: "Sure. The M5 is faster. It has a new chip.".into(), para_end: false });
        assert_eq!(c[1], Chunk { text: "Battery life is the same.".into(), para_end: true });
        assert_eq!(c[2], Chunk { text: "Most people should wait.".into(), para_end: true });
        let long: Vec<(String, bool)> = (0..20).map(|i| (format!("This is sentence number {i} about the topic."), false)).collect();
        assert!(chunks(&long, false).iter().all(|c| c.text.len() <= CHUNK + 50));
    }

    #[test]
    fn pauses_and_pace_follow_the_text() {
        assert!(pause_ms(true, "natural") > pause_ms(false, "natural"));
        assert!(pause_ms(true, "calm") > pause_ms(true, "lively"));
        assert!(chunk_speed("Is it?", 1.0) < 1.0 && chunk_speed("Wow!", 1.0) > 1.0 && chunk_speed("Ok.", 1.0) == 1.0);
    }

    #[test]
    fn shapes_chunks_without_clicks() {
        let rate = 24_000;
        let mut s = vec![0.0; 2400]; // 100 ms of silence before
        s.extend((0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5));
        s.extend(vec![0.0; 4800]); // 200 ms after
        let out = shape(s, rate, 300);
        assert_eq!(out[0], 0.0);
        // Trimmed to the speech plus 20 ms each side, then a 300 ms pause.
        let expected = 4800 + 2 * 480 + 7200;
        assert!((out.len() as i64 - expected as i64).abs() < 50, "{} vs {expected}", out.len());
        assert!(out[out.len() - 7200..].iter().all(|x| *x == 0.0));
    }

    #[test]
    fn reads_wavs_and_resamples() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.wav");
        crate::wake::write_wav(&p, &[0.0, 0.5, -0.5, 0.25]).unwrap();
        let (s, rate) = read_wav(&p).unwrap();
        assert_eq!(rate, 16_000);
        assert_eq!(s.len(), 4);
        let up = resample(&[0.0, 1.0], 24_000, 48_000);
        assert_eq!(up.len(), 4);
        assert!((up[1] - 0.5).abs() < 1e-6);
    }

    /// Real speech tool + Kokoro (BYTE_TEST_SHERPA_TTS, BYTE_TEST_KOKORO = the unpacked model folder):
    /// a sentence becomes about the right length of audio, in two different voices.
    #[tokio::test]
    #[ignore]
    async fn e2e_kokoro_speaks() {
        let (Ok(_), Ok(model)) = (std::env::var("BYTE_TEST_SHERPA_TTS"), std::env::var("BYTE_TEST_KOKORO")) else {
            eprintln!("skipped: set BYTE_TEST_SHERPA_TTS and BYTE_TEST_KOKORO");
            return;
        };
        for id in ["kokoro-v1_0/af_heart", "kokoro-v1_0/bm_george"] {
            let (p, s) = voices::pick(id);
            let started = std::time::Instant::now();
            let audio = synthesize(None, p, Path::new(&model), s, 1.0, "Hi, I'm BYTE. Here's what I found about the new MacBook Air.").await.unwrap();
            let secs = audio.len() as f32 / SAMPLE_RATE as f32;
            println!("{id}: {secs:.1} s of audio in {:.1} s", started.elapsed().as_secs_f32());
            assert!((2.0..8.0).contains(&secs), "{secs}");
            assert!(audio.iter().map(|x| x.abs()).fold(0.0, f32::max) > 0.05, "silent");
        }
    }
}
