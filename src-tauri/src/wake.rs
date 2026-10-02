//! "Hey BYTE" (Phase 11, opt-in): listens for the wake phrase and opens Quick
//! Ask with the mic on.
//!
//! How: the Mac's microphone is read natively (cpal/CoreAudio), so it works with
//! BYTE's windows hidden. A small energy-based voice detector cuts out short
//! bursts of speech (0.3–2.5 s); only those are passed to whisper (the smallest
//! installed speech model, prompted with "Hey BYTE"), and the text is matched
//! against the phrase. Nothing is kept: each burst lives in a temp file for the
//! fraction of a second whisper needs. macOS shows its mic indicator while this
//! is on, and it pauses while BYTE is talking or recording.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter};

use crate::error::AppResult;

pub const HEARD_EVENT: &str = "wake://heard";
const RATE: u32 = 16_000;
/// Voice-detector frame: 30 ms.
const FRAME: usize = (RATE as usize) * 30 / 1000;
/// A brisk "hey byte" is about half a second.
const MIN_BURST: usize = RATE as usize * 3 / 10;
const MAX_BURST: usize = RATE as usize * 5 / 2;
/// Silence that ends a burst: 300 ms.
const END_FRAMES: usize = 10;
/// Kept from before the burst started (the start of "hey" is quiet): 200 ms.
const PRE_ROLL: usize = RATE as usize / 5;

/// Paused while the UI records or BYTE speaks (set by the UI and speech.rs).
static PAUSED: AtomicBool = AtomicBool::new(false);

/// Cuts speech bursts out of a 16 kHz stream: frames louder than the (adapting) noise floor start one, 300 ms
/// of quiet ends it, and bursts of 0.3–2.5 s are returned (pre-roll included).
pub struct Vad {
    floor: f32,
    pending: Vec<f32>,
    history: Vec<f32>,
    burst: Vec<f32>,
    quiet: usize,
    /// How much of `burst` is pre-roll (less than PRE_ROLL when speech starts the audio).
    pre: usize,
    in_speech: bool,
    /// Talking went on past the longest burst: ignore it until the next pause.
    discard: bool,
}

impl Default for Vad {
    fn default() -> Self {
        Vad { floor: 0.003, pending: vec![], history: vec![], burst: vec![], quiet: 0, pre: 0, in_speech: false, discard: false }
    }
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

impl Vad {
    /// Feeds 16 kHz samples; returns the bursts that ended in them.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.pending.extend_from_slice(samples);
        let mut out = vec![];
        while self.pending.len() >= FRAME {
            let frame: Vec<f32> = self.pending.drain(..FRAME).collect();
            let e = rms(&frame);
            let loud = e > (self.floor * 3.0).max(0.008);
            if self.in_speech {
                self.quiet = if loud { 0 } else { self.quiet + 1 };
                if !self.discard {
                    self.burst.extend_from_slice(&frame);
                    // Speech only: without the pre-roll and the quiet tail.
                    let spoken = self.burst.len().saturating_sub(self.pre + self.quiet * FRAME);
                    if spoken > MAX_BURST {
                        self.discard = true;
                        self.burst.clear();
                    }
                }
                if self.quiet >= END_FRAMES {
                    let spoken = self.burst.len().saturating_sub(self.pre + self.quiet * FRAME);
                    let b = std::mem::take(&mut self.burst);
                    if !self.discard && spoken >= MIN_BURST {
                        out.push(b);
                    }
                    self.in_speech = false;
                    self.discard = false;
                    self.quiet = 0;
                }
            } else if loud {
                self.in_speech = true;
                self.burst = std::mem::take(&mut self.history);
                self.pre = self.burst.len();
                self.burst.extend_from_slice(&frame);
            } else {
                // The noise floor follows the room while nobody talks.
                self.floor = self.floor * 0.95 + e * 0.05;
                self.history.extend_from_slice(&frame);
                let extra = self.history.len().saturating_sub(PRE_ROLL);
                self.history.drain(..extra);
            }
        }
        out
    }
}

/// Whether whisper heard the wake phrase (and the near-misses it tends to write for it).
pub fn is_wake(text: &str) -> bool {
    let t: String = text.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let words: Vec<&str> = t.split_whitespace().collect();
    let first = ["hey", "hi", "hay", "a", "okay", "ok"];
    let second = ["byte", "bite", "bait", "bytes", "bites", "bye", "by", "bight"];
    // "A byte" only as the whole short phrase ("a bite of food" isn't a call).
    words.windows(2).enumerate().any(|(i, w)| {
        first.contains(&w[0]) && second.contains(&w[1]) && (w[0] != "a" || (i == 0 && words.len() <= 3 && w[1] != "by"))
    }) || words.first().is_some_and(|w| *w == "heybyte")
}

/// Averages blocks down to 16 kHz (input from the mic is usually 48 or 44.1 kHz).
pub fn downsample(input: &[f32], from: u32) -> Vec<f32> {
    if from == RATE {
        return input.to_vec();
    }
    let ratio = from as f64 / RATE as f64;
    let n = (input.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let (a, b) = ((i as f64 * ratio) as usize, (((i + 1) as f64 * ratio) as usize).min(input.len()));
            if b > a { input[a..b].iter().sum::<f32>() / (b - a) as f32 } else { 0.0 }
        })
        .collect()
}

/// A 16 kHz mono 16-bit WAV.
pub fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    let data = (samples.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b)
}

/// Reads a 16 kHz mono 16-bit WAV (for tests and files).
pub fn read_wav(path: &Path) -> Option<Vec<f32>> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 44 || &b[0..4] != b"RIFF" || u32::from_le_bytes([b[24], b[25], b[26], b[27]]) != RATE {
        return None;
    }
    // Find the "data" chunk.
    let mut i = 12;
    while i + 8 <= b.len() {
        let size = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        if &b[i..i + 4] == b"data" {
            let end = (i + 8 + size).min(b.len());
            return Some(b[i + 8..end].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect());
        }
        i += 8 + size + (size & 1);
    }
    None
}

/// Whisper's verdict on one burst.
async fn heard_wake(app: Option<&AppHandle>, models_dir: &Path, burst: &[f32]) -> bool {
    match transcript(app, models_dir, burst).await {
        Ok(text) => {
            log::debug!("wake burst: {text:?}");
            is_wake(&text)
        }
        Err(e) => {
            log::warn!("wake word: {e}");
            false
        }
    }
}

async fn transcript(app: Option<&AppHandle>, models_dir: &Path, burst: &[f32]) -> AppResult<String> {
    let tmp = tempfile::Builder::new().prefix("byte-wake").suffix(".wav").tempfile()?;
    write_wav(tmp.path(), &padded(burst))?;
    crate::voice::transcribe_short(app, models_dir, tmp.path(), "Hey BYTE").await
}

/// Quiet around a burst: whisper.cpp ignores audio shorter than a second, and "Hey BYTE" is about that long.
const PAD: usize = RATE as usize / 2;
const MIN_FOR_WHISPER: usize = RATE as usize * 3 / 2;

/// The burst with half a second of silence on each side, and at least 1.5 s long.
pub fn padded(burst: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0; PAD];
    out.extend_from_slice(burst);
    out.resize((out.len() + PAD).max(MIN_FOR_WHISPER), 0.0);
    out
}

/// Runs the detector over a 16 kHz WAV file; true if the wake phrase is in it (Mac e2e test).
pub async fn detect_in_file(app: Option<&AppHandle>, models_dir: &Path, wav: &Path) -> bool {
    heard_in_file(app, models_dir, wav).await.iter().any(|t| is_wake(t))
}

/// What whisper heard in each speech burst of a 16 kHz WAV file.
pub async fn heard_in_file(app: Option<&AppHandle>, models_dir: &Path, wav: &Path) -> Vec<String> {
    let Some(samples) = read_wav(wav) else { return vec![] };
    let mut vad = Vad::default();
    let mut bursts = vad.feed(&samples);
    bursts.extend(vad.feed(&vec![0.0; RATE as usize]));
    let mut out = vec![];
    for b in bursts {
        out.push(transcript(app, models_dir, &b).await.unwrap_or_else(|e| format!("(error: {e})")));
    }
    out
}

pub fn set_paused(p: bool) {
    PAUSED.store(p, Ordering::Relaxed);
}

/// The wake phrase was heard: Quick Ask opens and starts listening, with a chime.
fn on_wake(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/afplay").arg("/System/Library/Sounds/Tink.aiff").spawn();
    crate::quick::show(app);
    let _ = app.emit_to(crate::quick::LABEL, HEARD_EVENT, ());
}

#[cfg(target_os = "macos")]
mod listen {
    use std::sync::mpsc;
    use std::sync::Mutex;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use tauri::{AppHandle, Manager};

    use super::*;

    /// Stops the running listener when dropped/sent.
    pub static STOP: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);

    pub fn start(app: AppHandle) {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        if let Ok(mut s) = STOP.lock() {
            if s.is_some() {
                return;
            }
            *s = Some(stop_tx);
        }
        std::thread::spawn(move || {
            let (tx, rx) = mpsc::channel::<Vec<f32>>();
            // The stream lives on this thread (it isn't Send) until Stop.
            let stream = (|| {
                let dev = cpal::default_host().default_input_device()?;
                let cfg = dev.default_input_config().ok()?;
                let (rate, channels) = (cfg.sample_rate(), cfg.channels() as usize);
                let s = dev
                    .build_input_stream(
                        cfg.into(),
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let mono: Vec<f32> = data.chunks(channels.max(1)).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
                            let _ = tx.send(downsample(&mono, rate));
                        },
                        |e| log::warn!("wake word mic: {e}"),
                        None,
                    )
                    .ok()?;
                s.play().ok()?;
                Some(s)
            })();
            let Some(_stream) = stream else {
                log::warn!("wake word: no microphone");
                if let Ok(mut s) = STOP.lock() {
                    *s = None;
                }
                return;
            };
            let models = app.state::<crate::state::AppState>().paths.models.clone();
            let mut vad = Vad::default();
            let busy = std::sync::Arc::new(AtomicBool::new(false));
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                let Ok(chunk) = rx.recv_timeout(std::time::Duration::from_millis(200)) else { continue };
                if PAUSED.load(Ordering::Relaxed) || crate::speech::speaking() {
                    vad = Vad::default();
                    continue;
                }
                for burst in vad.feed(&chunk) {
                    // One check at a time; bursts while whisper is busy are skipped.
                    if busy.swap(true, Ordering::SeqCst) {
                        continue;
                    }
                    let (app, models, busy) = (app.clone(), models.clone(), busy.clone());
                    tauri::async_runtime::spawn(async move {
                        if heard_wake(Some(&app), &models, &burst).await {
                            on_wake(&app);
                        }
                        busy.store(false, Ordering::SeqCst);
                    });
                }
            }
        });
    }

    pub fn stop() {
        if let Some(tx) = STOP.lock().ok().and_then(|mut s| s.take()) {
            let _ = tx.send(());
        }
    }
}

/// Starts or stops listening to match the setting (macOS; elsewhere it's off).
pub fn apply(app: &AppHandle, on: bool) {
    #[cfg(target_os = "macos")]
    {
        if on {
            listen::start(app.clone());
        } else {
            listen::stop();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, on, on_wake as fn(&AppHandle));
}

/// The UI records or stops recording: the wake listener pauses meanwhile.
#[tauri::command]
pub fn wake_pause(paused: bool) {
    set_paused(paused);
}

/// Whether "Hey BYTE" can work here (a Mac with a speech model).
#[tauri::command]
pub async fn wake_ready(state: tauri::State<'_, crate::state::AppState>) -> AppResult<bool> {
    Ok(cfg!(target_os = "macos") && crate::voice::pick(&state.paths.models, "base-en").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize).map(|i| (i as f32 * 0.07).sin() * amp).collect()
    }

    #[test]
    fn hears_the_wake_phrase_and_not_others() {
        for yes in ["Hey BYTE", "hey byte.", "Hey, bite!", "Hey bait", "Okay, byte", " A byte?", "Hi Byte, what's up"] {
            assert!(is_wake(yes), "{yes}");
        }
        for no in ["Hey there", "a bite of food", "byte", "I'll stop by", "Goodbye", "[BLANK_AUDIO]", "hey you", "a by-product"] {
            assert!(!is_wake(no), "{no}");
        }
    }

    #[test]
    fn cuts_speech_bursts() {
        let mut vad = Vad::default();
        let mut audio = vec![0.0005f32; RATE as usize]; // quiet room
        audio.extend(tone(1.0, 0.2)); // "hey byte"
        audio.extend(vec![0.0005; RATE as usize / 2]);
        audio.extend(tone(0.1, 0.2)); // a click: too short
        audio.extend(vec![0.0005; RATE as usize / 2]);
        audio.extend(tone(4.0, 0.2)); // someone talking on: too long
        audio.extend(vec![0.0005; RATE as usize]);
        let bursts = vad.feed(&audio);
        assert_eq!(bursts.len(), 1, "{:?}", bursts.iter().map(Vec::len).collect::<Vec<_>>());
        let secs = bursts[0].len() as f32 / RATE as f32;
        assert!((1.0..1.7).contains(&secs), "{secs}");
    }

    #[test]
    fn keeps_a_brisk_phrase_that_starts_the_audio() {
        // `say` starts speaking at the first sample, with no quiet before it to use as pre-roll.
        for secs in [0.35, 0.5] {
            let mut vad = Vad::default();
            let mut bursts = vad.feed(&tone(secs, 0.2));
            bursts.extend(vad.feed(&vec![0.0; RATE as usize]));
            assert_eq!(bursts.len(), 1, "{secs} s");
        }
    }

    #[test]
    fn pads_short_bursts_for_whisper() {
        let p = padded(&tone(0.5, 0.2));
        assert_eq!(p.len(), MIN_FOR_WHISPER);
        assert!(p[..PAD].iter().all(|v| *v == 0.0) && p[PAD + RATE as usize / 4] != 0.0);
        assert_eq!(padded(&tone(2.0, 0.2)).len(), 2 * PAD + 2 * RATE as usize);
    }

    #[test]
    fn resamples_and_round_trips_wav() {
        let d = downsample(&[0.0, 0.3, 0.6, 0.9, 0.9, 0.9], 48_000);
        assert!(d.len() == 2 && (d[0] - 0.3).abs() < 1e-6 && (d[1] - 0.9).abs() < 1e-6, "{d:?}");
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.wav");
        write_wav(&p, &[0.0, 0.5, -0.5]).unwrap();
        let back = read_wav(&p).unwrap();
        assert_eq!(back.len(), 3);
        assert!((back[1] - 0.5).abs() < 0.001 && (back[2] + 0.5).abs() < 0.001);
    }

    /// Real whisper (BYTE_TEST_WHISPER, BYTE_TEST_WHISPER_MODEL = base.en) on "Hey BYTE" spoken by macOS's `say`.
    #[tokio::test]
    #[ignore]
    async fn e2e_wake_word() {
        let (Ok(_), Ok(model)) = (std::env::var("BYTE_TEST_WHISPER"), std::env::var("BYTE_TEST_WHISPER_MODEL")) else {
            eprintln!("skipped: set BYTE_TEST_WHISPER and BYTE_TEST_WHISPER_MODEL");
            return;
        };
        if !cfg!(target_os = "macos") {
            eprintln!("skipped: speaking the phrase needs macOS's `say`");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::voice::dir(tmp.path())).unwrap();
        std::fs::copy(&model, crate::voice::dir(tmp.path()).join("ggml-base.en.bin")).unwrap();
        let say = |text: &str, name: &str| {
            let aiff = tmp.path().join(format!("{name}.aiff"));
            let wav = tmp.path().join(format!("{name}.wav"));
            assert!(std::process::Command::new("/usr/bin/say").arg("-o").arg(&aiff).arg(text).status().unwrap().success());
            assert!(std::process::Command::new("/usr/bin/afconvert").args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"]).arg(&aiff).arg(&wav).status().unwrap().success());
            wav
        };
        let yes = say("Hey BYTE", "yes");
        let no = say("Good morning", "no");
        let heard = heard_in_file(None, tmp.path(), &yes).await;
        eprintln!("Hey BYTE → {heard:?}");
        assert!(heard.iter().any(|t| is_wake(t)), "didn't hear Hey BYTE: {heard:?}");
        let heard = heard_in_file(None, tmp.path(), &no).await;
        eprintln!("Good morning → {heard:?}");
        assert!(!heard.iter().any(|t| is_wake(t)), "heard it in Good morning: {heard:?}");
    }
}

