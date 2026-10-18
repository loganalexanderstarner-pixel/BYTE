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

use tauri::{AppHandle, Emitter, Manager};

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
    wake_rest(text).is_some()
}

/// The words said after the wake phrase in the same breath ("Hey BYTE, what's the weather" -> "what's the weather"; empty when only
/// the phrase was said). None when the phrase isn't in the text.
pub fn wake_rest(text: &str) -> Option<String> {
    // --- track:helpers begin --- a wake phrase of the person's own replaces "Hey BYTE" (helpers/wakeword.rs)
    if let Some(custom) = crate::helpers::wakeword::current() {
        return crate::helpers::wakeword::custom_rest(text, &custom);
    }
    // --- track:helpers end ---
    // Words keep their punctuation for the answer; matching looks only at letters and digits.
    let original: Vec<&str> = text.split_whitespace().collect();
    let words: Vec<String> = original.iter().map(|w| w.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()).collect();
    let first = ["hey", "hi", "hay", "a", "okay", "ok"];
    let second = ["byte", "bite", "bait", "bytes", "bites", "bye", "by", "bight"];
    // "A byte" only as the whole short phrase ("a bite of food" isn't a call).
    let at = (0..words.len().saturating_sub(1)).find(|&i| {
        first.contains(&words[i].as_str())
            && second.contains(&words[i + 1].as_str())
            && (words[i] != "a" || (i == 0 && words.len() <= 3 && words[i + 1] != "by"))
    });
    let from = match at {
        Some(i) => i + 2,
        None if words.first().is_some_and(|w| w == "heybyte") => 1,
        None => return None,
    };
    Some(original[from..].join(" ").trim_matches(|c: char| c == ',' || c == '.' || c.is_whitespace()).to_string())
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

/// A 16 kHz mono 16-bit WAV, as bytes.
pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
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
    b
}

/// A 16 kHz mono 16-bit WAV file.
pub fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    std::fs::write(path, wav_bytes(samples))
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

static TEST_LOG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The "Test log" switch (Settings > Voice): while on, what "Hey BYTE" heard is written to the log, with the verdict and the time it took.
/// Off by default, because it records words that were spoken.
pub fn set_test_log(on: bool) {
    TEST_LOG.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn test_log_on() -> bool {
    TEST_LOG.load(std::sync::atomic::Ordering::Relaxed)
}

// --- track:aec begin ---
/// The running capture began with the words that interrupted BYTE (bargein.rs); its audio is checked for the wake phrase when it ends.
static BARGE_TURN: AtomicBool = AtomicBool::new(false);

/// The first seconds of an interrupting turn, checked for the wake phrase. When it is there, the whole turn is read again so that the
/// words after the phrase come out (the phrase itself is dropped). None when the turn does not start with the phrase.
async fn interrupt_turn_wake(app: &AppHandle, models: &Path, audio: &[f32]) -> Option<String> {
    let head = &audio[..audio.len().min(RATE as usize * 2)];
    heard_wake(Some(app), models, head).await?;
    let started = std::time::Instant::now();
    let rest = match transcript(Some(app), models, audio).await {
        Ok(t) => wake_rest(&t),
        Err(_) => None,
    };
    if test_log_on() {
        log::info!("[test] interrupt: the turn that stopped BYTE started with the wake phrase, words after it: {:?} ({} ms)", rest, started.elapsed().as_millis());
    }
    rest
}

/// "Hey BYTE" was said over BYTE: BYTE stops, and the words after the phrase (or the next thing said) are the new turn.
pub async fn on_wake_interrupt(app: &AppHandle, rest: String) {
    crate::tts::stop();
    crate::echo::set_duck(1.0);
    let _ = app.emit(crate::speech::DONE_EVENT, ());
    BARGE_TURN.store(false, Ordering::SeqCst);
    if let Ok(mut c) = CAPTURE.lock() {
        *c = None;
    }
    if rest.trim().len() <= 1 {
        // Only the phrase: the page cuts its own speech and listens (the words that follow are the question).
        let _ = app.emit_to(crate::quick::LABEL, crate::bargein::BARGE_EVENT, ());
    }
    let (show, quiet) = {
        let state = app.state::<crate::state::AppState>();
        let st = state.settings.lock().await;
        (st.wake_show_window, crate::gamepolicy::current(st.game_aware, &st.hey_byte_engine, true).quiet)
    };
    on_wake(app, rest, show, quiet);
}
// --- track:aec end ---

/// Whisper's verdict on one burst.
async fn heard_wake(app: Option<&AppHandle>, models_dir: &Path, burst: &[f32]) -> Option<String> {
    let started = std::time::Instant::now();
    match transcript(app, models_dir, burst).await {
        Ok(text) => {
            let rest = wake_rest(&text);
            if test_log_on() {
                log::info!("[test] hey-byte heard {text:?}: {} ({} ms, burst {:.1} s)", if rest.is_some() { "WAKE PHRASE" } else { "no wake phrase" }, started.elapsed().as_millis(), burst.len() as f32 / 16000.0);
            } else {
                log::debug!("wake burst: {text:?}");
            }
            rest
        }
        Err(e) => {
            log::warn!("wake word: {e}");
            None
        }
    }
}

async fn transcript(app: Option<&AppHandle>, models_dir: &Path, burst: &[f32]) -> AppResult<String> {
    // A path with no open handle: on Windows another program (whisper) cannot open a file this process still holds.
    let tmp = tempfile::Builder::new().prefix("byte-wake").suffix(".wav").tempfile()?.into_temp_path();
    write_wav(&tmp, &padded(burst))?;
    crate::voice::transcribe_short(app, models_dir, &tmp, &crate::helpers::wakeword::prompt()).await // track:helpers
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


// ------------------------------------------------------------------ listening after the phrase
//
// After "Hey BYTE" the same microphone stream keeps going and hears what comes next, so nothing has to be opened (opening a second
// capture in the window cost a noticeable moment). The utterance ends like a Talk-mode turn: about 0.8 s of quiet after a short
// command, 1.1 s after a long sentence, or nothing is heard within the wait.

const UTTER_END_SHORT: usize = RATE as usize * 8 / 10;
const UTTER_END_LONG: usize = RATE as usize * 11 / 10;
const UTTER_SHORT: usize = RATE as usize * 2;
const UTTER_MAX: usize = RATE as usize * 30;
const UTTER_EVENT: &str = "wake://utterance";
const MORE_EVENT: &str = "wake://more";
/// How long to listen for more after a turn that may not be finished (the UI decides from the words; the audio is kept meanwhile).
pub const MORE_WAIT_MS: u64 = 1000;

/// What the next spoken turn needs to be cut out of the stream.
pub struct Capture {
    /// Samples to ignore first (the chime that just played).
    skip: usize,
    wait: usize,
    pending: Vec<f32>,
    buf: Vec<f32>,
    seen: usize,
    floor: f32,
    started: Option<usize>,
    last_loud: usize,
    /// The short extra listen after a turn ("wait until I'm done"): its result is `wake://more`, not the turn.
    more: bool,
}

pub enum Heard {
    Pending,
    /// The turn is over: the audio (empty when nothing was said).
    Done(Vec<f32>),
}

impl Capture {
    pub fn new(wait_ms: u64, skip_ms: u64) -> Self {
        Capture {
            skip: RATE as usize * skip_ms as usize / 1000,
            wait: RATE as usize * wait_ms as usize / 1000,
            pending: vec![],
            buf: vec![],
            seen: 0,
            floor: 0.003,
            started: None,
            last_loud: 0,
            more: false,
        }
    }

    /// The extra second after a turn ended: anything said joins the same turn (see `lib/turnJoin.ts`).
    pub fn extra(wait_ms: u64) -> Self {
        Capture { more: true, ..Capture::new(wait_ms, 0) }
    }

    pub fn feed(&mut self, samples: &[f32]) -> Heard {
        let mut input = samples;
        if self.skip > 0 {
            let n = self.skip.min(input.len());
            self.skip -= n;
            input = &input[n..];
        }
        self.pending.extend_from_slice(input);
        while self.pending.len() >= FRAME {
            let frame: Vec<f32> = self.pending.drain(..FRAME).collect();
            let loud = rms(&frame) > (self.floor * 3.0).max(0.008);
            self.seen += FRAME;
            if loud {
                if self.started.is_none() {
                    self.started = Some(self.seen);
                    // The start of the first word is quiet: keep a little before it.
                    let keep = self.buf.len().saturating_sub(PRE_ROLL);
                    self.buf.drain(..keep);
                }
                self.last_loud = self.seen;
            } else if self.started.is_none() {
                self.floor = self.floor * 0.95 + rms(&frame) * 0.05;
            }
            self.buf.extend_from_slice(&frame);
            if self.started.is_none() {
                let extra = self.buf.len().saturating_sub(PRE_ROLL);
                self.buf.drain(..extra);
            }
            match self.started {
                Some(start) => {
                    let spoke = self.last_loud.saturating_sub(start);
                    let quiet = self.seen - self.last_loud;
                    let need = if spoke < UTTER_SHORT { UTTER_END_SHORT } else { UTTER_END_LONG };
                    if (spoke >= RATE as usize / 4 && quiet >= need) || self.buf.len() >= UTTER_MAX {
                        return Heard::Done(std::mem::take(&mut self.buf));
                    }
                    // A click that never became speech: keep waiting for real words.
                    if spoke < RATE as usize / 4 && quiet >= RATE as usize {
                        self.started = None;
                        self.buf.clear();
                    }
                }
                None => {
                    if self.seen >= self.wait {
                        return Heard::Done(vec![]);
                    }
                }
            }
        }
        Heard::Pending
    }
}

static CAPTURE: std::sync::Mutex<Option<Capture>> = std::sync::Mutex::new(None);

/// The next thing said is the question (sent to the Quick Ask window as `wake://utterance`).
pub fn begin_capture(wait_ms: u64, skip_ms: u64) {
    if let Ok(mut c) = CAPTURE.lock() {
        *c = Some(Capture::new(wait_ms, skip_ms));
    }
}

fn capturing() -> bool {
    CAPTURE.lock().map(|c| c.is_some()).unwrap_or(false)
}

// --- track:voice begin ---
pub use crate::wakecheck::worth_checking;

static COOLDOWN: std::sync::Mutex<Option<crate::wakecheck::Cooldown>> = std::sync::Mutex::new(None);

fn secs_now() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// The same sound whisper turned down moments ago (a ringtone, a looping jingle).
fn cooldown_holds(burst: &[f32]) -> bool {
    COOLDOWN.lock().ok().is_some_and(|c| c.as_ref().is_some_and(|c| c.same_as_rejected(burst, secs_now())))
}

fn cooldown_remember(burst: &[f32]) {
    if let Ok(mut c) = COOLDOWN.lock() {
        c.get_or_insert_with(Default::default).rejected(burst, secs_now());
    }
}

/// A call is going on and "Hey BYTE" is off (the meeting watcher below).
pub fn meeting_quiet() -> bool {
    MEETING_QUIET.load(Ordering::Relaxed)
}
// --- track:voice end ---

/// A turn is being cut out of the stream right now (for the listening indicator).
pub fn capturing_now() -> bool {
    // The short extra listen after a turn ("wait until I'm done") is not listening for a question: the pill stays on "thinking".
    CAPTURE.lock().map(|c| c.as_ref().is_some_and(|cap| !cap.more)).unwrap_or(false)
}

/// Feeds the stream to the running capture; when the turn is over, hands it to the window.
fn capture_feed(app: &AppHandle, chunk: &[f32]) {
    use base64::Engine;
    // (audio, was the extra listen, samples of the wait that were left)
    let finished = match CAPTURE.lock() {
        Ok(mut c) => {
            let (was_more, left) = c.as_ref().map(|cap| (cap.more, cap.wait.saturating_sub(cap.seen))).unwrap_or((false, 0));
            match c.as_mut().map(|cap| cap.feed(chunk)) {
                Some(Heard::Done(audio)) => {
                    *c = None;
                    Some((audio, was_more, left))
                }
                _ => None,
            }
        }
        Err(_) => None,
    };
    let Some((audio, was_more, left)) = finished else { return };
    if test_log_on() {
        log::info!("[test] timing: question captured, {:.1} s of audio{}", audio.len() as f32 / RATE as f32, if was_more { " (the short extra listen)" } else { "" });
    }
    // --- track:aec begin --- a turn that began as an interruption of BYTE may start with "Hey BYTE": check, and strip the phrase.
    if !was_more && !audio.is_empty() && BARGE_TURN.swap(false, Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let models = app.state::<crate::state::AppState>().paths.models.clone();
            match interrupt_turn_wake(&app, &models, &audio).await {
                Some(rest) => on_wake_interrupt(&app, rest).await,
                None => deliver_utterance(&app, audio, was_more, left),
            }
        });
        return;
    }
    // --- track:aec end ---
    deliver_utterance(app, audio, was_more, left);
}

fn deliver_utterance(app: &AppHandle, audio: Vec<f32>, was_more: bool, left: usize) {
    use base64::Engine;
    let b64 = |a: &[f32]| base64::engine::general_purpose::STANDARD.encode(wav_bytes(a));
    // --- track:voice begin --- focus on one voice: another voice's turn is ignored (or, when voices were allowed by name, the page asks the cloud who it was).
    if !audio.is_empty() {
        match crate::voicefocus::check_turn(&audio) {
            crate::voicefocus::Verdict::Accept => {}
            verdict => {
                let left_ms = left as u64 * 1000 / RATE as u64;
                if was_more {
                    let _ = app.emit_to(crate::quick::LABEL, MORE_EVENT, None::<String>);
                } else if verdict == crate::voicefocus::Verdict::CheckName {
                    let _ = app.emit_to(crate::quick::LABEL, "wake://utterance-other", Some(b64(&audio)));
                } else if left_ms >= 1500 {
                    // Keep listening for the person in the rest of the wait.
                    begin_capture(left_ms, 0);
                } else {
                    let _ = app.emit_to(crate::quick::LABEL, UTTER_EVENT, None::<String>);
                }
                return;
            }
        }
    }
    // --- track:voice end ---
    if !audio.is_empty() && !was_more {
        crate::voicectl::set_thinking(true);
        // "Wait until I'm done": right away the same stream listens one more second, so words that come while the first part is still
        // being turned into text are not lost. The page ignores it when the turn was complete.
        if crate::voicectl::wait_more_on() {
            if let Ok(mut c) = CAPTURE.lock() {
                *c = Some(Capture::extra(MORE_WAIT_MS));
            }
        }
    }
    let payload: Option<String> = (!audio.is_empty()).then(|| b64(&audio));
    let _ = app.emit_to(crate::quick::LABEL, if was_more { MORE_EVENT } else { UTTER_EVENT }, payload);
}

/// The talk key was pressed: the same as hearing the phrase with nothing after it (needs the listener, so "Hey BYTE" must be on).
pub fn talk_now(app: &AppHandle, show_window: bool) {
    // The key user is not turned away by an earlier focus on someone else's voice; pressing it again restarts the turn.
    crate::voicefocus::begin(&[]);
    on_wake(app, String::new(), show_window, false);
}

/// A capture that begins with audio already heard (the words that interrupted BYTE, see bargein.rs).
pub fn begin_capture_seeded(wait_ms: u64, seed: &[f32]) {
    BARGE_TURN.store(true, Ordering::SeqCst); // track:aec
    let mut cap = Capture::new(wait_ms, 0);
    let _ = cap.feed(seed);
    if let Ok(mut c) = CAPTURE.lock() {
        *c = Some(cap);
    }
}

/// Follow-up listening after an answer was spoken: `wait_ms` for the first word.
#[tauri::command]
pub fn wake_listen(wait_ms: u64) {
    if wait_ms == 0 {
        // The person closed the listening state: stop waiting for a question.
        if let Ok(mut c) = CAPTURE.lock() {
            *c = None;
        }
    } else {
        begin_capture(wait_ms.clamp(1000, 20_000), 0);
    }
}

/// The wake phrase was heard: Quick Ask opens and starts listening, with a chime.
fn on_wake(app: &AppHandle, rest: String, show_window: bool, quiet: bool) {
    // The chime the person chose (voicectl.rs: the system sound, a soft tone, or none); its length is ignored by the capture.
    // While a game runs (and "game-aware" is on) there is no chime: gamepolicy.rs.
    let skip_ms = if quiet { 0 } else { crate::voicectl::play_chime() };
    // Only the phrase was said: the next thing heard is the question (after the chime, which would otherwise count as speech).
    if rest.trim().len() <= 1 {
        begin_capture(15_000, skip_ms);
    } else {
        crate::voicectl::set_thinking(true);
    }
    // Ready at once, like a speaker: the hidden window's page does the work and BYTE answers out loud. The window only appears when asked for.
    if show_window {
        crate::quick::show(app);
    }
    let _ = app.emit_to(crate::quick::LABEL, HEARD_EVENT, rest);
}

// --- track:lead begin ---
/// True while another program has the microphone open for a call and the person left "meeting-aware quiet" on.
static MEETING_QUIET: AtomicBool = AtomicBool::new(false);

/// Looks every 20 s. It goes quiet only after two readings in a row say "in a call", and comes back at the first clear reading, so one odd
/// reading never silences BYTE for long. Ends when the listener stops.
#[cfg(any(target_os = "macos", windows))]
fn watch_meeting(app: AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let mut previous = false;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            if listen::STOP.lock().map(|s| s.is_none()).unwrap_or(true) {
                MEETING_QUIET.store(false, Ordering::Relaxed);
                return;
            }
            let enabled = app.state::<crate::state::AppState>().settings.lock().await.meeting_quiet;
            let now = enabled && tauri::async_runtime::spawn_blocking(crate::meeting::active).await.unwrap_or(false);
            MEETING_QUIET.store(now && previous, Ordering::Relaxed);
            previous = now;
        }
    });
}
// --- track:lead end ---

#[cfg(any(target_os = "macos", windows))]
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
        super::watch_meeting(app.clone()); // track:lead
        std::thread::spawn(move || {
            // The audio callback only converts and writes into this lock-free ring (aec.rs); this thread reads. No allocation, lock or
            // wait on the audio thread: that was what the "buffer underrun or overrun" warnings came from.
            let ring = crate::aec::Ring::new(RATE as usize * 4); // track:aec
            let failed = std::sync::Arc::new(AtomicBool::new(false)); // track:aec
            // The stream lives on this thread (it isn't Send) until Stop.
            let Some(first) = open_mic(&ring, &failed) else {
                log::warn!("wake word: no microphone");
                if let Ok(mut s) = STOP.lock() {
                    *s = None;
                }
                return;
            };
            let mut stream = Some(first); // track:aec
            let mut last_data = std::time::Instant::now(); // track:aec
            let mut next_retry = std::time::Instant::now(); // track:aec
            let mut loopback = crate::aec::LoopbackControl::default(); // track:aec
            let mut stats_seen = [0u64; 8]; // track:aec
            let mut stats_at = std::time::Instant::now(); // track:aec
            let models = app.state::<crate::state::AppState>().paths.models.clone();
            let mut vad = Vad::default();
            let busy = std::sync::Arc::new(AtomicBool::new(false));
            let mut barge = crate::bargein::Barge::default(); // track:voice
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                // --- track:aec begin ---
                if stats_at.elapsed() > std::time::Duration::from_secs(5) {
                    stats_at = std::time::Instant::now();
                    crate::aec::log_stats_if_changed(&mut stats_seen);
                    loopback.sync(crate::bargein::mode() == crate::bargein::Mode::Speakers);
                }
                let mut chunk: Vec<f32> = Vec::new();
                ring.pop_into(&mut chunk, usize::MAX);
                if chunk.is_empty() {
                    // The stream reported an error, or has delivered nothing for 3 s (a device that was unplugged or switched): rebuild it.
                    let stalled = last_data.elapsed() > std::time::Duration::from_secs(3);
                    if (failed.load(Ordering::Relaxed) || stalled) && std::time::Instant::now() >= next_retry {
                        drop(stream.take());
                        crate::aec::stats::MIC_RESTARTS.fetch_add(1, Ordering::Relaxed);
                        failed.store(false, Ordering::Relaxed);
                        match open_mic(&ring, &failed) {
                            Some(s) => {
                                stream = Some(s);
                                last_data = std::time::Instant::now();
                            }
                            None => {
                                crate::aec::stats::MIC_ERRORS.fetch_add(1, Ordering::Relaxed);
                                next_retry = std::time::Instant::now() + std::time::Duration::from_secs(3);
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(8));
                    continue;
                }
                last_data = std::time::Instant::now();
                crate::aec::stats::MIC_OVERRUNS.store(ring.dropped(), Ordering::Relaxed);
                // --- track:aec end ---
                let mic_pos = crate::echo::advance_mic(chunk.len()); // track:voice
                if capturing() {
                    capture_feed(&app, &chunk);
                    vad = Vad::default();
                    continue;
                }
                // --- track:lead begin ---
                // A call is going on (meeting.rs, checked twice in a row by `watch_meeting`): "Hey BYTE" stays off so BYTE does not answer the room.
                if MEETING_QUIET.load(Ordering::Relaxed) {
                    vad = Vad::default();
                    continue;
                }
                // --- track:lead end ---
                // --- track:voice begin ---
                // BYTE's own voices (tts.rs) can be talked over: the microphone is watched while it speaks (bargein.rs).
                let barge_mode = crate::bargein::mode();
                if !PAUSED.load(Ordering::Relaxed) && barge_mode != crate::bargein::Mode::Off && crate::tts::speaking() {
                    if let Some(seed) = barge.feed(&chunk, mic_pos, barge_mode) {
                        // Only the focused voice may stop BYTE (voicefocus.rs; accepts everyone when the focus is off or has lapsed).
                        if crate::voicefocus::check_barge(&seed) {
                            crate::bargein::interrupted(&app, &seed);
                        }
                    }
                    // --- track:aec begin --- "Hey BYTE" said over BYTE (in the cleaned signal) stops it and starts the new turn.
                    for burst in barge.take_bursts() {
                        check_burst(&app, &models, &busy, burst, true);
                    }
                    // --- track:aec end ---
                    vad = Vad::default();
                    continue;
                }
                barge.idle();
                // --- track:voice end ---
                if PAUSED.load(Ordering::Relaxed) || crate::speech::speaking() {
                    vad = Vad::default();
                    continue;
                }
                for burst in vad.feed(&chunk) {
                    check_burst(&app, &models, &busy, burst, false);
                }
            }
        });
    }

    /// Checks one speech burst for the wake phrase (one check at a time; bursts while whisper is busy are skipped). `over_byte`: the burst
    /// was heard while BYTE spoke, so a hit also stops BYTE.
    fn check_burst(app: &AppHandle, models: &std::path::Path, busy: &std::sync::Arc<AtomicBool>, burst: Vec<f32>, over_byte: bool) {
        // --- track:voice begin --- (cheap pre-check: whisper only runs on bursts that could be the phrase)
        if !worth_checking(&burst) || cooldown_holds(&burst) {
            return;
        }
        // --- track:voice end ---
        // Over BYTE's own voice some of the echo that is left over looks like speech: at most one check every 4 s there.
        if over_byte {
            static LAST_OVER: Mutex<Option<std::time::Instant>> = Mutex::new(None);
            if let Ok(mut last) = LAST_OVER.lock() {
                if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(4)) {
                    return;
                }
                *last = Some(std::time::Instant::now());
            }
        }
        if busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let (app, models, busy) = (app.clone(), models.to_path_buf(), busy.clone());
        tauri::async_runtime::spawn(async move {
            if let Some(rest) = heard_wake(Some(&app), &models, &burst).await {
                crate::voicefocus::begin(&burst); // track:voice (voiceprint of the wake burst, memory only)
                if over_byte {
                    if crate::wake::test_log_on() {
                        log::info!("[test] interrupt: \"Hey BYTE\" heard over BYTE, stopping it");
                    }
                    on_wake_interrupt(&app, rest).await;
                } else {
                    let (show, quiet) = {
                        let state = app.state::<crate::state::AppState>();
                        let st = state.settings.lock().await;
                        (st.wake_show_window, crate::gamepolicy::current(st.game_aware, &st.hey_byte_engine, true).quiet)
                    };
                    on_wake(&app, rest, show, quiet);
                }
            } else {
                cooldown_remember(&burst); // track:voice
            }
            busy.store(false, Ordering::SeqCst);
        });
    }

    /// Opens the default microphone; its callback feeds `ring` (16 kHz mono) and flags `failed` on a stream error.
    fn open_mic(ring: &std::sync::Arc<crate::aec::Ring>, failed: &std::sync::Arc<AtomicBool>) -> Option<cpal::Stream> {
        let dev = cpal::default_host().default_input_device()?;
        let cfg = dev.default_input_config().ok()?;
        let (rate, channels) = (cfg.sample_rate(), cfg.channels() as usize);
        let mut feeder = crate::aec::Feeder::new(rate, channels, ring.clone(), &crate::aec::stats::MIC_DROPPED);
        let failed = failed.clone();
        let s = dev
            .build_input_stream(
                cfg.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| feeder.feed(data),
                move |e| {
                    crate::aec::stats::MIC_ERRORS.fetch_add(1, Ordering::Relaxed);
                    failed.store(true, Ordering::Relaxed);
                    log::warn!("wake word mic: {e}");
                },
                None,
            )
            .ok()?;
        s.play().ok()?;
        Some(s)
    }

    pub fn stop() {
        crate::voicefocus::forget(); // track:voice
        crate::echo::set_duck(1.0); // track:voice
        if let Ok(mut c) = CAPTURE.lock() {
            *c = None;
        }
        if let Some(tx) = STOP.lock().ok().and_then(|mut s| s.take()) {
            let _ = tx.send(());
        }
    }
}

/// Starts or stops listening to match the setting (macOS and Windows; elsewhere it's off).
pub fn apply(app: &AppHandle, on: bool) {
    #[cfg(any(target_os = "macos", windows))]
    {
        if on {
            crate::quick::preload(app);
            crate::voice::ensure_wake_model(app);
            listen::start(app.clone());
        } else {
            listen::stop();
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (app, on, on_wake as fn(&AppHandle, String, bool, bool));
}

/// The UI records or stops recording: the wake listener pauses meanwhile.
#[tauri::command]
pub fn wake_pause(paused: bool) {
    set_paused(paused);
}

/// Whether "Hey BYTE" can work here (a Mac or a Windows PC, with a speech model).
#[tauri::command]
pub async fn wake_ready(state: tauri::State<'_, crate::state::AppState>) -> AppResult<bool> {
    Ok(cfg!(any(target_os = "macos", windows)) && crate::voice::pick(&state.paths.models, "base-en").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(secs: f32) -> Vec<f32> {
        tone(secs, 0.2)
    }

    fn run(cap: &mut Capture, audio: &[f32]) -> Option<Vec<f32>> {
        for chunk in audio.chunks(1600) {
            if let Heard::Done(a) = cap.feed(chunk) {
                return Some(a);
            }
        }
        None
    }

    #[test]
    fn a_short_command_ends_after_under_a_second_of_quiet() {
        let mut cap = Capture::new(10_000, 0);
        let mut audio = vec![0.0; RATE as usize / 2];
        audio.extend(speech(1.0));
        let before_quiet = audio.len();
        audio.extend(vec![0.0; RATE as usize * 3]);
        let got = run(&mut cap, &audio).expect("the turn ended");
        assert!(got.len() >= RATE as usize, "the words are in it");
        assert!(got.len() < before_quiet + RATE as usize, "it did not wait for the long silence: {} samples", got.len());
    }

    #[test]
    fn the_extra_listen_gives_up_after_its_second_and_keeps_late_words() {
        let mut cap = Capture::extra(MORE_WAIT_MS);
        assert!(cap.more);
        let got = run(&mut cap, &vec![0.0; RATE as usize * 3]).expect("gave up");
        assert!(got.is_empty());
        // Words that start inside the second are kept, however long the second part runs.
        let mut cap = Capture::extra(MORE_WAIT_MS);
        let mut audio = vec![0.0; RATE as usize / 2];
        audio.extend(speech(1.5));
        audio.extend(vec![0.0; RATE as usize * 3]);
        assert!(run(&mut cap, &audio).expect("ended").len() >= RATE as usize);
    }

    #[test]
    fn nothing_said_ends_empty_after_the_wait() {
        let mut cap = Capture::new(3000, 0);
        let got = run(&mut cap, &vec![0.0; RATE as usize * 5]).expect("gave up");
        assert!(got.is_empty());
    }

    #[test]
    fn the_chime_at_the_start_is_skipped() {
        let mut cap = Capture::new(2000, 450);
        let mut audio = speech(0.3); // the chime
        audio.extend(vec![0.0; RATE as usize * 3]);
        assert!(run(&mut cap, &audio).expect("gave up").is_empty());
    }

    #[test]
    fn what_follows_the_wake_phrase_is_the_question() {
        assert_eq!(wake_rest("Hey BYTE, what's the weather?").as_deref(), Some("what's the weather?"));
        assert_eq!(wake_rest("Hey Byte.").as_deref(), Some(""));
        assert_eq!(wake_rest("ok bite set a timer for ten minutes").as_deref(), Some("set a timer for ten minutes"));
        assert_eq!(wake_rest("So, hey byte what time is it").as_deref(), Some("what time is it"));
        assert_eq!(wake_rest("a bite of food"), None);
        assert_eq!(wake_rest("good morning"), None);
    }

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
        if !cfg!(any(target_os = "macos", windows)) {
            eprintln!("skipped: speaking the phrase needs macOS's `say` or Windows' speech synthesizer");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::voice::dir(tmp.path())).unwrap();
        std::fs::copy(&model, crate::voice::dir(tmp.path()).join("ggml-base.en.bin")).unwrap();
        // Speaks `text` into a 16 kHz mono WAV with the system's own voice: `say` on a Mac, the speech synthesizer on Windows.
        let say = |text: &str, name: &str| {
            let wav = tmp.path().join(format!("{name}.wav"));
            #[cfg(target_os = "macos")]
            {
                let aiff = tmp.path().join(format!("{name}.aiff"));
                assert!(std::process::Command::new("/usr/bin/say").arg("-o").arg(&aiff).arg(text).status().unwrap().success());
                assert!(std::process::Command::new("/usr/bin/afconvert").args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"]).arg(&aiff).arg(&wav).status().unwrap().success());
            }
            #[cfg(windows)]
            {
                const SCRIPT: &str = "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                    $f = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono); \
                    $s.SetOutputToWaveFile($env:SAY_OUT, $f); $s.Speak($env:SAY_TEXT); $s.Dispose()";
                let ok = std::process::Command::new("powershell").args(["-NoProfile", "-Command", SCRIPT]).env("SAY_OUT", &wav).env("SAY_TEXT", text).status().unwrap().success();
                assert!(ok, "the speech synthesizer failed");
                // A person pauses before and after "Hey BYTE"; the detector learns the room's noise level from that quiet.
                let mut samples = vec![0.0; RATE as usize];
                samples.extend(read_wav(&wav).expect("the synthesized speech is a WAV file"));
                samples.extend(vec![0.0; RATE as usize]);
                write_wav(&wav, &samples).unwrap();
            }
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

