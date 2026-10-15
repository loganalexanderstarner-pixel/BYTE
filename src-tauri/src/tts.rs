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
#[cfg(any(target_os = "macos", windows))]
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

/// The chosen voice can speak here: unpacked, and on a system with an audio output
/// backend (a Mac, or Windows through WASAPI).
pub fn usable(models_dir: &Path, voice: &str) -> bool {
    cfg!(any(target_os = "macos", windows)) && voices::ready(models_dir, voices::pick(voice).0)
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
    let tmp = tempfile::Builder::new().prefix("byte-tts").suffix(".wav").tempfile()?.into_temp_path();
    let args = voices::args(p, dir, speaker, speed, voices::threads(), &tmp, text)?;
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
    let (samples, rate) = read_wav(&tmp).ok_or_else(|| AppError::msg("BYTE's voice made no audio."))?;
    Ok(resample(&samples, rate, SAMPLE_RATE))
}

// ------------------------------------------------------------------ streamed speech (the cluster's expressive voices)

/// How the person's voice sounded when they asked (set at the start of a spoken answer): shapes the tone BYTE answers in, quietly.
static MOOD: Mutex<Option<String>> = Mutex::new(None);

pub fn set_mood(mood: Option<String>) {
    if let Ok(mut m) = MOOD.lock() {
        *m = mood;
    }
}

/// The direction for the voice, in plain words, for a mood. Never about what BYTE heard: only how to say things.
pub fn tone_for(mood: Option<&str>) -> &'static str {
    match mood {
        Some("sad") | Some("fearful") => "The person sounds down, so be gentle and warm.",
        Some("angry") | Some("disgusted") => "The person sounds upset, so stay calm and steady.",
        Some("happy") | Some("surprised") => "The person sounds happy, so be a little warmer and brighter.",
        _ => "",
    }
}

/// Turns streamed 16-bit little-endian PCM into samples, keeping a half sample for the next piece.
#[derive(Default)]
pub struct PcmDecoder {
    odd: Option<u8>,
}

impl PcmDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<f32> {
        let mut all: Vec<u8> = Vec::with_capacity(bytes.len() + 1);
        all.extend(self.odd.take());
        all.extend_from_slice(bytes);
        if all.len() % 2 == 1 {
            self.odd = all.pop();
        }
        all.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect()
    }
}

/// Linear resampling piece by piece with no seam between pieces (resampling each piece alone would put a small jump at every join).
pub struct StreamResampler {
    step: f64,
    pos: f64,
    last: Option<f32>,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32) -> Self {
        StreamResampler { step: from as f64 / to as f64, pos: 0.0, last: None }
    }

    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        if (self.step - 1.0).abs() < f64::EPSILON {
            return input.to_vec();
        }
        let mut buf: Vec<f32> = Vec::with_capacity(input.len() + 1);
        buf.extend(self.last);
        buf.extend_from_slice(input);
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 2);
        while self.pos + 1.0 < buf.len() as f64 {
            let i = self.pos as usize;
            let t = (self.pos - i as f64) as f32;
            out.push(buf[i] + (buf[i + 1] - buf[i]) * t);
            self.pos += self.step;
        }
        if let Some(&l) = buf.last() {
            self.pos -= (buf.len() - 1) as f64;
            self.last = Some(l);
        }
        out
    }
}

/// Shapes a stream of speech like `shape` does a whole chunk: leading quiet is dropped and the start fades in; the last few
/// milliseconds are held back until the end so they can fade out; a pause follows. Everything else passes straight through.
pub struct StreamShaper {
    rate: u32,
    started: bool,
    lead: Vec<f32>,
    hold: VecDeque<f32>,
}

impl StreamShaper {
    pub fn new(rate: u32) -> Self {
        StreamShaper { rate, started: false, lead: vec![], hold: VecDeque::new() }
    }

    fn hold_len(&self) -> usize {
        self.rate as usize / 50
    }

    /// Samples ready to play now.
    pub fn push(&mut self, new: Vec<f32>) -> Vec<f32> {
        let mut ready = Vec::new();
        let mut incoming = new;
        if !self.started {
            self.lead.append(&mut incoming);
            let Some(first) = self.lead.iter().position(|x| x.abs() > 0.004) else { return ready };
            self.started = true;
            let keep = self.rate as usize / 50;
            let mut body: Vec<f32> = self.lead.split_off(first.saturating_sub(keep));
            self.lead.clear();
            let fade = (self.rate as usize / 125).min(body.len() / 2);
            for (i, s) in body.iter_mut().take(fade).enumerate() {
                *s *= i as f32 / fade as f32;
            }
            incoming = std::mem::take(&mut body);
        }
        self.hold.extend(incoming);
        let keep = self.hold_len();
        while self.hold.len() > keep {
            ready.extend(self.hold.pop_front());
        }
        ready
    }

    /// The rest at the end: the held samples faded out, then a pause.
    pub fn finish(&mut self, pause_ms: u32) -> Vec<f32> {
        let n = self.hold.len();
        let mut out: Vec<f32> = self.hold.drain(..).collect();
        for (i, s) in out.iter_mut().enumerate() {
            *s *= (n - 1 - i) as f32 / n.max(1) as f32;
        }
        if self.started {
            out.extend(std::iter::repeat_n(0.0, (self.rate as u64 * pause_ms as u64 / 1000) as usize));
        }
        out
    }
}

/// Speaks one chunk with a cluster voice that streams: the audio is queued as it arrives, so the first sound starts after a fraction of
/// a second instead of when the whole chunk is made. `Err` only if nothing was said (the caller then uses the voice on this PC).
async fn speak_streaming(client: &crate::cloud::CloudClient, text: &str, voice: &str, style: &str, speed: f32, pause_ms: u32, session: u64) -> Result<(), String> {
    let out_rate = queue().lock().map(|q| q.out_rate).unwrap_or(SAMPLE_RATE);
    let mut dec = PcmDecoder::default();
    let mut shaper = StreamShaper::new(SAMPLE_RATE);
    let mut rs = StreamResampler::new(SAMPLE_RATE, out_rate);
    let mut spoke = false;
    let play = |samples: Vec<f32>, rs: &mut StreamResampler| -> bool {
        if samples.is_empty() {
            return true;
        }
        if SESSION.load(Ordering::SeqCst) != session {
            return false;
        }
        let out = rs.push(&samples);
        if let Ok(mut q) = queue().lock() {
            q.samples.extend(out);
        }
        true
    };
    let tone = tone_for(MOOD.lock().ok().and_then(|m| m.clone()).as_deref());
    let result = crate::cloud::voice::stream(client, text, voice, style, speed, tone, |bytes| {
        let ready = shaper.push(dec.push(bytes));
        spoke |= !ready.is_empty();
        play(ready, &mut rs)
    })
    .await;
    if SESSION.load(Ordering::SeqCst) != session {
        return Ok(());
    }
    match result {
        Ok(()) => {
            play(shaper.finish(pause_ms), &mut rs);
            Ok(())
        }
        // Part-way through: what was said stays said; nothing was said: the voice on this PC does it.
        Err(e) if spoke => {
            log::warn!("streamed voice stopped part-way: {}", crate::error::AppError::from(e));
            play(shaper.finish(pause_ms), &mut rs);
            Ok(())
        }
        Err(e) => Err(crate::error::AppError::from(e).to_string()),
    }
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
            if let Some((client, voice)) = &cloud {
                if crate::cloud::voice::streams(voice) {
                    match speak_streaming(client, &chunk.text, voice, &how.style, pace, pause_ms(chunk.para_end, &how.style), session).await {
                        Ok(()) => continue,
                        Err(e) => {
                            // Nothing was said: the rest is spoken on this PC (when a voice is there).
                            log::warn!("cloud voice: {e}");
                            cloud = None;
                        }
                    }
                }
            }
            let made = match &cloud {
                Some((client, voice)) => match crate::cloud::voice::synthesize(client, &chunk.text, voice, &how.style, pace).await {
                    Ok(s) => Ok(s),
                    Err(e) => {
                        // The cloud stopped answering: the rest is spoken on this Mac (when a voice is there).
                        log::warn!("cloud voice: {}", crate::error::AppError::from(e));
                        cloud = None;
                        if local_ready { synthesize(Some(&app), p, &dir, &speaker, pace, &chunk.text).await } else { Err(AppError::msg(crate::platform_text::here("no voice on this Mac"))) }
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

#[cfg(any(target_os = "macos", windows))]
fn start_output() {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::AtomicBool;
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.get().is_some() {
        return;
    }
    let _ = STARTED.set(());
    std::thread::spawn(|| {
        // Windows invalidates a stream when the sound device changes (a headset plugged in, the default output switched, the
        // format changed): "The stream configuration is no longer valid and must be rebuilt". Then it plays nothing for good,
        // so the stream is watched and built again, and a changed default device is followed.
        let broken = Arc::new(AtomicBool::new(false));
        let build = |broken: &Arc<AtomicBool>| {
            let dev = cpal::default_host().default_output_device()?;
            let cfg = dev.default_output_config().ok()?;
            let channels = cfg.channels() as usize;
            if let Ok(mut q) = queue().lock() {
                q.out_rate = cfg.sample_rate();
            }
            let q = queue().clone();
            let flag = broken.clone();
            let mut scratch: Vec<f32> = Vec::with_capacity(16_384); // track:voice
            let s = dev
                .build_output_stream(
                    cfg.into(),
                    move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        let mut ended = None;
                        if let Ok(mut q) = q.lock() {
                            // --- track:voice begin --- (barge-in: lower the volume while the person talks, and note what is played for echo cancellation)
                            let gain = crate::echo::duck_gain();
                            let tap = crate::echo::tap_wanted();
                            scratch.clear();
                            let mut any = q.more;
                            for frame in out.chunks_mut(channels.max(1)) {
                                let real = q.samples.pop_front();
                                any |= real.is_some();
                                let s = real.unwrap_or(0.0) * gain;
                                frame.iter_mut().for_each(|x| *x = s);
                                if tap {
                                    scratch.push(s); // reserved up front: no allocation here for normal block sizes
                                }
                            }
                            if tap {
                                crate::echo::tap_push(&scratch, q.out_rate, any);
                            }
                            // --- track:voice end ---
                            if q.samples.is_empty() && !q.more && q.session != u64::MAX {
                                ended = Some(q.session);
                                q.session = u64::MAX; // report once
                            }
                        }
                        if let Some(session) = ended {
                            finished(session);
                        }
                    },
                    move |e| {
                        log::warn!("voice output: {e}");
                        flag.store(true, Ordering::SeqCst);
                    },
                    None,
                )
                .ok()?;
            s.play().ok()?;
            let name = dev.description().map(|d| d.name().to_string()).unwrap_or_default();
            Some((s, name))
        };
        let current_default = || {
            cpal::default_host().default_output_device().and_then(|d| d.description().map(|x| x.name().to_string()).ok()).unwrap_or_default()
        };
        let mut stream: Option<(cpal::Stream, String)> = None;
        loop {
            if stream.is_none() || broken.load(Ordering::SeqCst) || current_default() != stream.as_ref().map(|s| s.1.clone()).unwrap_or_default() {
                stream = None; // closes the old one first
                broken.store(false, Ordering::SeqCst);
                stream = build(&broken);
                if stream.is_none() {
                    log::warn!("voice: no audio output");
                } else {
                    log::info!("voice output: stream (re)built on {}", stream.as_ref().map(|s| s.1.as_str()).unwrap_or(""));
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(not(any(target_os = "macos", windows)))]
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
    Ok(VoicesStatus { ready, downloaded, can_speak: cfg!(any(target_os = "macos", windows)) })
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

    /// Windows audio through cpal (WASAPI): what the default output and input are, and
    /// the format each wants. Informational and tolerant: a machine with no sound
    /// device (a CI runner) legitimately has neither, so this reports and passes
    /// rather than failing on a fact about the machine. The point is to show, on a
    /// real PC, that the backend that was not even compiled before now works.
    #[cfg(windows)]
    #[test]
    fn windows_audio_devices_are_visible() {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let out = host.default_output_device();
        let inp = host.default_input_device();
        eprintln!("audio host: {:?}", host.id());
        match &out {
            Some(d) => eprintln!("output: {:?} -> {:?}", d.description().map(|x| x.name().to_string()).ok(), d.default_output_config().map(|c| (c.sample_rate(), c.channels())).ok()),
            None => eprintln!("output: none"),
        }
        match &inp {
            Some(d) => eprintln!("input:  {:?} -> {:?}", d.description().map(|x| x.name().to_string()).ok(), d.default_input_config().map(|c| (c.sample_rate(), c.channels())).ok()),
            None => eprintln!("input:  none"),
        }
    }

    #[test]
    fn streamed_pcm_is_decoded_even_when_a_piece_ends_mid_sample() {
        let samples: Vec<i16> = vec![0, 16384, -16384, 32767, -32768, 100];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut d = PcmDecoder::default();
        let mut out = d.push(&bytes[..3]);
        out.extend(d.push(&bytes[3..8]));
        out.extend(d.push(&bytes[8..]));
        assert_eq!(out.len(), 6);
        assert!((out[1] - 0.5).abs() < 1e-4 && (out[2] + 0.5).abs() < 1e-4 && out[4] <= -0.9999);
        assert!(d.push(&[]).is_empty());
    }

    #[test]
    fn resampling_in_pieces_matches_resampling_whole_with_no_seams() {
        let sine: Vec<f32> = (0..24_000).map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / 24_000.0).sin() * 0.5).collect();
        for to in [24_000u32, 48_000, 44_100, 16_000] {
            let mut whole = StreamResampler::new(24_000, to);
            let all = whole.push(&sine);
            let mut pieces = StreamResampler::new(24_000, to);
            let mut joined = vec![];
            for p in sine.chunks(1777) {
                joined.extend(pieces.push(p));
            }
            assert!((all.len() as i64 - joined.len() as i64).abs() <= 1, "{to}: {} vs {}", all.len(), joined.len());
            let n = all.len().min(joined.len());
            let worst = (0..n).map(|i| (all[i] - joined[i]).abs()).fold(0.0f32, f32::max);
            assert!(worst < 1e-4, "{to}: seams of {worst}");
            // The tone is still a smooth sine: no sample jumps by more than a sine of that pitch can.
            let step = (0..n - 1).map(|i| (joined[i + 1] - joined[i]).abs()).fold(0.0f32, f32::max);
            assert!(step < 0.05, "{to}: a jump of {step}");
        }
    }

    #[test]
    fn a_streamed_chunk_starts_soft_ends_soft_and_keeps_every_sound_between() {
        let rate = 24_000;
        let mut shaper = StreamShaper::new(rate);
        // 0.3 s of quiet, then 1 s of tone, delivered in odd-sized pieces.
        let mut input: Vec<f32> = vec![0.0; 7200];
        input.extend((0..24_000).map(|i| (i as f32 * 0.1).sin() * 0.4));
        let mut played = vec![];
        for p in input.chunks(1234) {
            played.extend(shaper.push(p.to_vec()));
        }
        played.extend(shaper.finish(150));
        // The quiet at the start is gone (only a little lead-in kept), the tone is all there, and a 150 ms pause follows.
        assert!(played.len() < input.len() + 3600 && played.len() > 24_000 + 3600, "{}", played.len());
        assert!(played[0].abs() < 0.01, "starts silent: {}", played[0]);
        let tone_end = played.len() - (rate as usize * 150 / 1000);
        assert!(played[tone_end - 1].abs() < 0.01, "ends soft: {}", played[tone_end - 1]);
        assert!(played[played.len() - 1] == 0.0);
        let mid = played[played.len() / 2 - 100..played.len() / 2 + 100].iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(mid > 0.3, "the middle is untouched: {mid}");
        // Pure silence says nothing at all, not even a pause.
        let mut quiet = StreamShaper::new(rate);
        assert!(quiet.push(vec![0.0; 5000]).is_empty());
        assert!(quiet.finish(150).is_empty());
    }

    #[test]
    fn the_mood_of_the_voice_only_shapes_how_things_are_said() {
        assert_eq!(tone_for(None), "");
        assert_eq!(tone_for(Some("neutral")), "");
        assert!(tone_for(Some("sad")).contains("gentle") && tone_for(Some("angry")).contains("calm") && tone_for(Some("happy")).contains("brighter"));
        assert!(!tone_for(Some("sad")).to_lowercase().contains("hear") || tone_for(Some("sad")).contains("sounds"), "it never claims to have heard anything");
    }
}
