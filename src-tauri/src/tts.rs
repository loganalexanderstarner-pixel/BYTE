//! BYTE's voices (Phase 11): Kokoro, a free, open neural voice model, run on
//! this Mac by sherpa-onnx's speech tool (`sherpa-tts` sidecar, built by
//! `scripts/build-sherpa.sh`). 28 English voices, downloaded once (~350 MB)
//! from sherpa-onnx's release and checked against a pinned SHA-256.
//!
//! Smooth speech: an answer is cut into chunks (the first sentence alone, so
//! BYTE starts talking at once; then a few sentences at a time). Chunks are
//! made one after another while earlier ones play, and all audio goes through
//! one continuous output stream, so there's no gap between them. Text can be
//! fed while the answer is still being written (`speech_feed`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(target_os = "macos")]
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::models::{self, ModelFile, Variant};
use crate::state::AppState;

const SIDECAR: &str = "sherpa-tts";
pub const KEY: &str = "voice:kokoro";
const RELEASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models";
const ARCHIVE: &str = "kokoro-multi-lang-v1_0.tar.bz2";
const ARCHIVE_SIZE: u64 = 349_906_910;
const ARCHIVE_SHA: &str = "c5f7e2d2caf082bc1d20fb70334a61d99d20b484500aad32e7cf84c128ea3298";
const FOLDER: &str = "kokoro-multi-lang-v1_0";
pub const SAMPLE_RATE: u32 = 24_000;
/// After the first sentence, chunks of about this many characters (a few sentences).
const CHUNK: usize = 320;

/// One of BYTE's voices: Kokoro speaker id, a friendly name and its accent.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: &'static str,
    pub sid: u32,
    pub name: &'static str,
    /// "American" or "British".
    pub accent: &'static str,
    /// "female" or "male".
    pub gender: &'static str,
}

const fn v(id: &'static str, sid: u32, name: &'static str, accent: &'static str, gender: &'static str) -> Voice {
    Voice { id, sid, name, accent, gender }
}

/// The English voices in Kokoro v1.0 (speaker ids from the model's own metadata).
pub const VOICES: &[Voice] = &[
    v("af_heart", 3, "Heart", "American", "female"),
    v("af_bella", 2, "Bella", "American", "female"),
    v("af_nicole", 6, "Nicole", "American", "female"),
    v("af_sarah", 9, "Sarah", "American", "female"),
    v("af_nova", 7, "Nova", "American", "female"),
    v("af_sky", 10, "Sky", "American", "female"),
    v("af_alloy", 0, "Alloy", "American", "female"),
    v("af_aoede", 1, "Aoede", "American", "female"),
    v("af_jessica", 4, "Jessica", "American", "female"),
    v("af_kore", 5, "Kore", "American", "female"),
    v("af_river", 8, "River", "American", "female"),
    v("am_michael", 16, "Michael", "American", "male"),
    v("am_fenrir", 14, "Fenrir", "American", "male"),
    v("am_puck", 18, "Puck", "American", "male"),
    v("am_echo", 12, "Echo", "American", "male"),
    v("am_eric", 13, "Eric", "American", "male"),
    v("am_liam", 15, "Liam", "American", "male"),
    v("am_onyx", 17, "Onyx", "American", "male"),
    v("am_adam", 11, "Adam", "American", "male"),
    v("am_santa", 19, "Santa", "American", "male"),
    v("bf_emma", 21, "Emma", "British", "female"),
    v("bf_isabella", 22, "Isabella", "British", "female"),
    v("bf_alice", 20, "Alice", "British", "female"),
    v("bf_lily", 23, "Lily", "British", "female"),
    v("bm_george", 26, "George", "British", "male"),
    v("bm_fable", 25, "Fable", "British", "male"),
    v("bm_lewis", 27, "Lewis", "British", "male"),
    v("bm_daniel", 24, "Daniel", "British", "male"),
];

pub const DEFAULT_VOICE: &str = "af_heart";

pub fn voice(id: &str) -> &'static Voice {
    VOICES.iter().find(|v| v.id == id).unwrap_or(&VOICES[0])
}

// ------------------------------------------------------------------ the model

fn dir(models_dir: &Path) -> PathBuf {
    crate::voice::dir(models_dir).join("kokoro")
}

fn archive_variant() -> Variant {
    Variant { quant: "kokoro".into(), bits: 0.0, size_bytes: ARCHIVE_SIZE, files: vec![ModelFile { name: ARCHIVE.into(), size: ARCHIVE_SIZE, sha256: ARCHIVE_SHA.into() }] }
}

fn release_url(_repo: &str, file: &str) -> String {
    format!("{RELEASE}/{file}")
}

fn model_dir(models_dir: &Path) -> PathBuf {
    dir(models_dir).join(FOLDER)
}

/// The voices are downloaded and unpacked.
pub fn ready(models_dir: &Path) -> bool {
    model_dir(models_dir).join(".ready").exists()
}

/// BYTE's voices can speak here: unpacked, and on a Mac (where the audio output is).
pub fn usable(models_dir: &Path) -> bool {
    cfg!(target_os = "macos") && ready(models_dir)
}

/// Unpacks the downloaded archive (macOS's and Linux's `tar` read .tar.bz2), then removes it.
pub async fn unpack(models_dir: &Path) -> AppResult<()> {
    if ready(models_dir) {
        return Ok(());
    }
    let d = dir(models_dir);
    let archive = models::entry_path(&d, &archive_variant());
    if !models::is_installed(&d, &archive_variant()) {
        return Err(AppError::msg("The voices haven't finished downloading."));
    }
    let out = tokio::process::Command::new("tar").arg("-xjf").arg(&archive).arg("-C").arg(&d).output().await?;
    if !out.status.success() || !model_dir(models_dir).join("model.onnx").exists() {
        return Err(AppError::msg("BYTE couldn't unpack the voices; deleting and downloading them again usually fixes it."));
    }
    let _ = std::fs::remove_file(&archive);
    std::fs::write(model_dir(models_dir).join(".ready"), b"ok")?;
    Ok(())
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

/// Chunks to make one after another: the first sentence alone (a quick start; joined with the next if tiny),
/// then sentences grouped up to `CHUNK` characters.
pub fn chunks(sentences: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for s in sentences {
        let n = out.len();
        match out.last_mut() {
            Some(last) if (n == 1 && last.len() < 24) || (n > 1 && last.len() + s.len() < CHUNK) => {
                last.push(' ');
                last.push_str(s);
            }
            _ => out.push(s.clone()),
        }
    }
    out
}

/// The speech tool's arguments. The text is the last argument, never mistaken for an option.
pub fn tts_args(model: &Path, sid: u32, speed: &str, out: &Path, text: &str, british: bool) -> Vec<String> {
    let length_scale = match speed {
        "slow" => "1.15",
        "fast" => "0.87",
        _ => "1.0",
    };
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
    let lexicon = if british { "lexicon-gb-en.txt" } else { "lexicon-us-en.txt" };
    vec![
        format!("--kokoro-model={}", model.join("model.onnx").display()),
        format!("--kokoro-voices={}", model.join("voices.bin").display()),
        format!("--kokoro-tokens={}", model.join("tokens.txt").display()),
        format!("--kokoro-data-dir={}", model.join("espeak-ng-data").display()),
        format!("--kokoro-lexicon={}", model.join(lexicon).display()),
        format!("--kokoro-length-scale={length_scale}"),
        format!("--num-threads={threads}"),
        format!("--sid={sid}"),
        format!("--output-filename={}", out.display()),
        text.trim_start_matches(['-', ' ']).to_string(),
    ]
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

/// Makes one chunk of speech (24 kHz samples).
pub async fn synthesize(app: Option<&AppHandle>, model: &Path, voice: &Voice, speed: &str, text: &str) -> AppResult<Vec<f32>> {
    let tmp = tempfile::Builder::new().prefix("byte-tts").suffix(".wav").tempfile()?;
    let args = tts_args(model, voice.sid, speed, tmp.path(), text, voice.accent == "British");
    let ok = match (std::env::var("BYTE_TEST_SHERPA_TTS").ok().filter(|b| !b.is_empty()), app) {
        (Some(bin), _) => tokio::process::Command::new(bin).args(&args).kill_on_drop(true).output().await?.status.success(),
        (None, Some(app)) => {
            use tauri_plugin_shell::ShellExt;
            let cmd = app.shell().sidecar(SIDECAR).map_err(|e| AppError::msg(format!("BYTE's voice engine is missing from this build: {e}")))?;
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
    tx: tokio::sync::mpsc::UnboundedSender<Option<String>>,
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

/// Speaks (more of) answer `id`. `text` is the whole answer so far; complete sentences not yet spoken are made
/// and queued. `done`: the answer is finished (the last sentence counts too).
pub fn feed(app: &AppHandle, models_dir: &Path, voice_id: &str, speed: &str, id: &str, text: &str, done: bool) -> AppResult<()> {
    let _ = APP.set(app.clone());
    let mut all = sentences(&crate::speech::speakable(text));
    if !done && !all.is_empty() {
        all.pop(); // may still be growing
    }
    let mut guard = FEED.lock().map_err(|_| AppError::msg("voice busy"))?;
    if guard.as_ref().is_none_or(|f| f.id != id) {
        // A new answer: stop the old one, start a maker for this one.
        drop(guard);
        stop();
        let session = SESSION.load(Ordering::SeqCst);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Option<String>>();
        start_output();
        if let Ok(mut q) = queue().lock() {
            q.more = true;
            q.session = session;
        }
        spawn_maker(app.clone(), model_dir(models_dir), voice(voice_id).clone(), speed.to_string(), session, rx);
        guard = FEED.lock().map_err(|_| AppError::msg("voice busy"))?;
        *guard = Some(Feed { id: id.to_string(), sent: 0, tx });
    }
    let f = guard.as_mut().expect("set above");
    let new: Vec<String> = all.iter().skip(f.sent).cloned().collect();
    f.sent = all.len().max(f.sent);
    let first = f.sent == new.len();
    // The first sentence goes alone (a quick start); later ones in groups.
    let parts = if first { chunks(&new) } else { chunks_after_first(&new) };
    for p in parts {
        let _ = f.tx.send(Some(p));
    }
    if done {
        let _ = f.tx.send(None);
    }
    Ok(())
}

fn chunks_after_first(sentences: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for s in sentences {
        match out.last_mut() {
            Some(last) if last.len() + s.len() < CHUNK => {
                last.push(' ');
                last.push_str(s);
            }
            _ => out.push(s.clone()),
        }
    }
    out
}

fn spawn_maker(app: AppHandle, model: PathBuf, voice: Voice, speed: String, session: u64, mut rx: tokio::sync::mpsc::UnboundedReceiver<Option<String>>) {
    tauri::async_runtime::spawn(async move {
        while let Some(next) = rx.recv().await {
            if SESSION.load(Ordering::SeqCst) != session {
                return;
            }
            let Some(text) = next else { break };
            match synthesize(Some(&app), &model, &voice, &speed, &text).await {
                Ok(samples) => {
                    if SESSION.load(Ordering::SeqCst) != session {
                        return;
                    }
                    if let Ok(mut q) = queue().lock() {
                        let rate = q.out_rate;
                        q.samples.extend(resample(&samples, SAMPLE_RATE, rate));
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsStatus {
    pub ready: bool,
    /// Downloaded but not unpacked yet.
    pub downloaded: bool,
    pub size_bytes: u64,
    pub key: String,
    pub voices: Vec<Voice>,
}

#[tauri::command]
pub async fn tts_status(state: State<'_, AppState>) -> AppResult<TtsStatus> {
    let m = &state.paths.models;
    Ok(TtsStatus { ready: ready(m), downloaded: models::is_installed(&dir(m), &archive_variant()), size_bytes: ARCHIVE_SIZE, key: KEY.into(), voices: VOICES.to_vec() })
}

#[tauri::command]
pub async fn tts_download(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let d = dir(&state.paths.models);
    std::fs::create_dir_all(&d)?;
    state.downloads.start_from(app, state.net.clone(), d, String::new(), archive_variant(), KEY.into(), release_url).await
}

/// Unpacks the voices after their download finished.
#[tauri::command]
pub async fn tts_unpack(state: State<'_, AppState>) -> AppResult<()> {
    unpack(&state.paths.models).await
}

#[tauri::command]
pub async fn tts_delete(state: State<'_, AppState>) -> AppResult<()> {
    state.downloads.pause(KEY).await;
    stop();
    let d = dir(&state.paths.models);
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

    #[test]
    fn first_chunk_is_quick_then_groups() {
        let s: Vec<String> = ["Sure.", "The M5 is faster.", "It has a new chip.", "Battery life is the same.", "Most people should wait."].iter().map(|x| x.to_string()).collect();
        let c = chunks(&s);
        // A tiny first sentence takes the next ones until the opening is long enough to be worth making alone;
        // the rest group up to the chunk size.
        assert_eq!(c[0], "Sure. The M5 is faster. It has a new chip.");
        assert_eq!(c[1], "Battery life is the same. Most people should wait.");
        let long: Vec<String> = (0..20).map(|i| format!("This is sentence number {i} about the topic.")).collect();
        assert!(chunks(&long).iter().skip(1).all(|c| c.len() <= CHUNK + 50));
    }

    #[test]
    fn arguments_keep_text_last_and_safe() {
        let a = tts_args(Path::new("/m"), 3, "fast", Path::new("/tmp/o.wav"), "--sid=99 hello", false);
        assert_eq!(a.last().unwrap(), "sid=99 hello");
        assert!(a.contains(&"--sid=3".to_string()) && a.contains(&"--kokoro-length-scale=0.87".to_string()));
        assert!(tts_args(Path::new("/m"), 26, "", Path::new("o"), "Hi", true).iter().any(|x| x.ends_with("lexicon-gb-en.txt")));
    }

    #[test]
    fn voices_are_known_and_unique() {
        let mut sids: Vec<u32> = VOICES.iter().map(|v| v.sid).collect();
        sids.sort();
        sids.dedup();
        assert_eq!(sids.len(), VOICES.len());
        assert_eq!(voice("bm_george").sid, 26);
        assert_eq!(voice("nope").id, DEFAULT_VOICE);
        assert_eq!(VOICES.len(), 28);
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
        for id in ["af_heart", "bm_george"] {
            let started = std::time::Instant::now();
            let audio = synthesize(None, Path::new(&model), voice(id), "normal", "Hi, I'm BYTE. Here's what I found about the new MacBook Air.").await.unwrap();
            let secs = audio.len() as f32 / SAMPLE_RATE as f32;
            println!("{id}: {secs:.1} s of audio in {:.1} s", started.elapsed().as_secs_f32());
            assert!((2.0..8.0).contains(&secs), "{secs}");
            assert!(audio.iter().map(|x| x.abs()).fold(0.0, f32::max) > 0.05, "silent");
        }
    }
}
