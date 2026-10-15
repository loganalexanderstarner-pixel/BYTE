//! Focus on one voice: when "Hey BYTE" is heard, BYTE takes a voiceprint of that burst and, for the rest of the conversation (until a few
//! quiet minutes pass), only follow-up turns and barge-in from the same voice are accepted. A phone call, the TV or another person in
//! the room is ignored.
//!
//! HONEST NOTE ON THE VOICEPRINT. BYTE has no on-device speaker-embedding model that can be called on a clip: `speakers.rs` ships the
//! sherpa *diarization* tool (it labels speakers in a whole recording, it does not give an embedding for a 1 s clip), and `voiceprints.rs`
//! identifies taught voices on the BYTE cloud, which would mean sending the audio away. This feature promises the voiceprint is kept
//! in memory only and never sent, so it uses a CLEARLY MARKED FALLBACK: simple spectral features (the average shape of the voice's
//! spectrum over 20 bands, plus the typical pitch) compared with cosine similarity. That tells apart clearly different voices (a man and
//! a woman, a TV announcer) far better than similar ones (two brothers), and 1 to 2 s clips make it noisier still. The thresholds are
//! constants below: only the owner's real voices can tune them. If an on-device embedding model is added later, replace `extract` and
//! `similarity` and keep everything else.
//!
//! The voiceprint lives in memory only: no Serialize, no file, no network, no log line with its numbers. It is dropped when the focus
//! ends, when the setting is turned off, and when "Hey BYTE" listening stops.
//!
//! "also listen to <name>" works for voices taught in Settings (voiceprints.rs, identified on the cloud as already happens for every
//! spoken turn): a turn that doesn't match the focused voice is handed to the page, which accepts it when the cloud names the speaker as
//! one of the allowed names.

#![cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]

use std::sync::Mutex;
use std::time::{Duration, Instant};

const RATE: usize = 16_000;
const FRAME: usize = 400; // 25 ms
const HOP: usize = 160; // 10 ms
pub const BANDS: usize = 20;
/// The focus lapses after this long without the focused voice being heard.
pub const QUIET_LAPSE: Duration = Duration::from_secs(3 * 60);
/// Similarity (cosine of the spectral shapes, minus a pitch penalty) a voice needs to count as the focused one. UNTUNED: see the note above.
pub const THRESHOLD_NORMAL: f32 = 0.85;
pub const THRESHOLD_STRICT: f32 = 0.93;
/// Each semitone between the two typical pitches costs this much similarity.
pub const PITCH_PENALTY_PER_SEMITONE: f32 = 0.02;
/// Clips with less voiced sound than this (ms) give no voiceprint.
pub const MIN_VOICED_MS: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Off,
    Normal,
    Strict,
}

pub fn parse_mode(s: &str) -> Mode {
    match s {
        "off" => Mode::Off,
        "strict" => Mode::Strict,
        _ => Mode::Normal,
    }
}

/// What is known about one voice. Deliberately not Serialize and with a Debug that hides the numbers.
#[derive(Clone)]
pub struct Print {
    /// Average log spectrum over `BANDS` bands, with its own mean removed (the shape, not the loudness).
    shape: [f32; BANDS],
    /// Typical pitch in Hz (0 when none could be found).
    pitch: f32,
}

impl std::fmt::Debug for Print {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Print(..)")
    }
}

impl Print {
    /// For tests with synthetic embeddings.
    #[cfg(test)]
    pub fn from_parts(shape: [f32; BANDS], pitch: f32) -> Self {
        Print { shape, pitch }
    }
}

fn band_freqs() -> Vec<Vec<f32>> {
    // Band edges from 120 Hz to 4 kHz, equal ratio; three probe frequencies in each band.
    let (lo, hi) = (120f32, 4000f32);
    let ratio = (hi / lo).powf(1.0 / BANDS as f32);
    (0..BANDS)
        .map(|b| {
            let a = lo * ratio.powi(b as i32);
            let z = a * ratio;
            (0..3).map(|k| a * (z / a).powf((k as f32 + 0.5) / 3.0)).collect()
        })
        .collect()
}

fn goertzel_power(frame: &[f32], window: &[f32], freq: f32) -> f32 {
    let w = 2.0 * std::f32::consts::PI * freq / RATE as f32;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0f32, 0f32);
    for (x, h) in frame.iter().zip(window) {
        let s = x * h + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0)
}

fn pitch_of(frame: &[f32]) -> Option<f32> {
    let (min_lag, max_lag) = (RATE / 400, RATE / 70);
    let energy: f32 = frame.iter().map(|x| x * x).sum();
    if energy < 1e-6 {
        return None;
    }
    let mut best = (0usize, 0f32);
    for lag in min_lag..=max_lag.min(frame.len() - 1) {
        let c: f32 = frame[..frame.len() - lag].iter().zip(&frame[lag..]).map(|(a, b)| a * b).sum::<f32>() / energy;
        if c > best.1 {
            best = (lag, c);
        }
    }
    (best.1 > 0.45).then(|| RATE as f32 / best.0 as f32)
}

/// The fallback voiceprint of a 16 kHz clip: None when it has too little voiced sound.
pub fn extract(samples: &[f32]) -> Option<Print> {
    if samples.len() < FRAME * 2 {
        return None;
    }
    let window: Vec<f32> = (0..FRAME).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FRAME as f32).cos()).collect();
    let frames: Vec<&[f32]> = samples.windows(FRAME).step_by(HOP).collect();
    let rms: Vec<f32> = frames.iter().map(|f| (f.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt()).collect();
    let peak = rms.iter().copied().fold(0.0, f32::max);
    if peak < 0.01 {
        return None;
    }
    // Only the clearly voiced frames (not the quiet before and after the words).
    let keep: Vec<usize> = (0..frames.len()).filter(|&i| rms[i] > 0.3 * peak).collect();
    if keep.len() * HOP * 1000 / RATE < MIN_VOICED_MS {
        return None;
    }
    let freqs = band_freqs();
    let mut shape = [0f32; BANDS];
    let mut pitches = vec![];
    for &i in &keep {
        for (b, fs) in freqs.iter().enumerate() {
            let p: f32 = fs.iter().map(|&f| goertzel_power(frames[i], &window, f)).sum::<f32>() / fs.len() as f32;
            shape[b] += (p + 1e-6).ln();
        }
        if let Some(p) = pitch_of(frames[i]) {
            pitches.push(p);
        }
    }
    shape.iter_mut().for_each(|v| *v /= keep.len() as f32);
    let mean = shape.iter().sum::<f32>() / BANDS as f32;
    shape.iter_mut().for_each(|v| *v -= mean);
    pitches.sort_by(|a, b| a.total_cmp(b));
    let pitch = if pitches.len() >= 3 { pitches[pitches.len() / 2] } else { 0.0 };
    Some(Print { shape, pitch })
}

/// How alike two voices are, about -1 to 1 (1: the same shape and pitch).
pub fn similarity(a: &Print, b: &Print) -> f32 {
    let dot: f32 = a.shape.iter().zip(&b.shape).map(|(x, y)| x * y).sum();
    let (na, nb) = (a.shape.iter().map(|x| x * x).sum::<f32>().sqrt(), b.shape.iter().map(|x| x * x).sum::<f32>().sqrt());
    let cos = if na < 1e-6 || nb < 1e-6 { 0.0 } else { dot / (na * nb) };
    let penalty = if a.pitch > 0.0 && b.pitch > 0.0 { PITCH_PENALTY_PER_SEMITONE * (12.0 * (a.pitch / b.pitch).log2()).abs() } else { 0.0 };
    cos - penalty
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    /// A different voice: ignored.
    Reject,
    /// A different voice, but voices were allowed by name: the page asks the cloud who it is.
    CheckName,
}

/// The conversation's focus. Time is passed in so that the lapse can be tested.
pub struct Focus {
    mode: Mode,
    anchor: Option<Print>,
    /// "Listen to everyone" (or "stop focusing on me"): nothing is turned away until the next "Hey BYTE".
    everyone: bool,
    names: Vec<String>,
    last: Option<Instant>,
}

impl Focus {
    pub const fn new() -> Self {
        Focus { mode: Mode::Normal, anchor: None, everyone: false, names: Vec::new(), last: None }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if mode == Mode::Off {
            self.clear();
        }
    }

    pub fn clear(&mut self) {
        self.anchor = None;
        self.everyone = false;
        self.names.clear();
        self.last = None;
    }

    pub fn active(&self) -> bool {
        self.mode != Mode::Off && self.anchor.is_some() && !self.everyone
    }

    /// "Hey BYTE" was said by this voice: it is the one to listen to now (whoever says the phrase wakes BYTE; the others are ignored after).
    pub fn begin(&mut self, print: Option<Print>, now: Instant) {
        if self.mode == Mode::Off {
            return;
        }
        self.anchor = print;
        self.everyone = false;
        self.names.clear();
        self.last = Some(now);
    }

    fn lapsed(&self, now: Instant) -> bool {
        self.last.is_none_or(|t| now.duration_since(t) > QUIET_LAPSE)
    }

    fn threshold(&self) -> f32 {
        if self.mode == Mode::Strict { THRESHOLD_STRICT } else { THRESHOLD_NORMAL }
    }

    pub fn listen_to_everyone(&mut self) {
        self.everyone = true;
    }

    pub fn allow_name(&mut self, name: &str) {
        let n = name.trim().to_lowercase();
        if !n.is_empty() && !self.names.contains(&n) {
            self.names.push(n);
        }
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Whether a turn (or an interruption) from this voice is accepted. `print`: None when the clip had too little voiced sound.
    pub fn check(&mut self, print: Option<&Print>, now: Instant) -> Verdict {
        if !self.active() {
            return Verdict::Accept;
        }
        if self.lapsed(now) {
            // A few quiet minutes passed: the conversation is over, anyone may speak again.
            self.clear();
            return Verdict::Accept;
        }
        let anchor = self.anchor.as_ref().expect("active");
        let ok = match print {
            Some(p) => similarity(anchor, p) >= self.threshold(),
            // Nothing to compare: normal gives the benefit of the doubt, strict does not.
            None => self.mode == Mode::Normal,
        };
        if ok {
            self.last = Some(now);
            Verdict::Accept
        } else if self.names.is_empty() {
            Verdict::Reject
        } else {
            Verdict::CheckName
        }
    }

    /// The page confirmed (through the cloud's voice check) that an allowed name spoke: the conversation counts as active.
    pub fn confirmed(&mut self, now: Instant) {
        self.last = Some(now);
    }
}

static FOCUS: Mutex<Focus> = Mutex::new(Focus::new());

pub fn set_mode(m: &str) {
    if let Ok(mut f) = FOCUS.lock() {
        f.set_mode(parse_mode(m));
    }
}

/// Listening for the phrase stopped: nothing about anyone's voice is kept.
pub fn forget() {
    if let Ok(mut f) = FOCUS.lock() {
        f.clear();
    }
}

/// The wake phrase was heard in this burst.
pub fn begin(burst: &[f32]) {
    if parse_active() {
        let print = extract(burst);
        if let Ok(mut f) = FOCUS.lock() {
            f.begin(print, Instant::now());
        }
    }
}

fn parse_active() -> bool {
    FOCUS.lock().map(|f| f.mode != Mode::Off).unwrap_or(false)
}

fn active() -> bool {
    FOCUS.lock().map(|f| f.active()).unwrap_or(false)
}

/// A spoken turn (the audio is about to go to the page): is it the focused voice?
pub fn check_turn(audio: &[f32]) -> Verdict {
    if !active() {
        return Verdict::Accept;
    }
    let print = extract(audio);
    FOCUS.lock().map(|mut f| f.check(print.as_ref(), Instant::now())).unwrap_or(Verdict::Accept)
}

/// Someone talks over BYTE: only the focused voice may stop it.
pub fn check_barge(seed: &[f32]) -> bool {
    check_turn(seed) == Verdict::Accept
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Command {
    Everyone,
    AlsoListenTo { name: String },
}

/// A spoken focus command ("listen to everyone", "also listen to Anna", "stop focusing on me") reached the backend.
#[tauri::command]
pub fn voice_focus_command(command: Command) {
    if let Ok(mut f) = FOCUS.lock() {
        match command {
            Command::Everyone => f.listen_to_everyone(),
            Command::AlsoListenTo { name } => f.allow_name(&name),
        }
    }
}

/// The page confirmed an allowed (taught) voice spoke.
#[tauri::command]
pub fn voice_focus_confirmed() {
    if let Ok(mut f) = FOCUS.lock() {
        f.confirmed(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic voice: harmonics of `f0` shaped by two formant bumps.
    fn voice(f0: f32, f1: f32, f2: f32, secs: f32, seed: f32) -> Vec<f32> {
        let n = (secs * RATE as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                // A little vibrato and drift, so no two clips are identical.
                let f = f0 * (1.0 + 0.02 * (t * 5.0 + seed).sin());
                let mut s = 0.0;
                for h in 1..=30 {
                    let hf = f * h as f32;
                    if hf > 4000.0 {
                        break;
                    }
                    let gain = (-((hf - f1) / 250.0).powi(2)).exp() + 0.7 * (-((hf - f2) / 400.0).powi(2)).exp() + 0.03;
                    s += gain * (2.0 * std::f32::consts::PI * hf * t + h as f32 * seed).sin();
                }
                0.08 * s
            })
            .collect()
    }

    fn a(secs: f32, seed: f32) -> Vec<f32> {
        voice(115.0, 600.0, 1400.0, secs, seed)
    }
    fn b(secs: f32, seed: f32) -> Vec<f32> {
        voice(215.0, 800.0, 2400.0, secs, seed)
    }

    #[test]
    fn the_same_synthetic_voice_matches_and_a_different_one_does_not() {
        let a1 = extract(&a(1.5, 0.3)).expect("print");
        let a2 = extract(&a(1.2, 1.9)).expect("print");
        let b1 = extract(&b(1.5, 0.7)).expect("print");
        let same = similarity(&a1, &a2);
        let other = similarity(&a1, &b1);
        assert!(same >= THRESHOLD_NORMAL, "same voice {same}");
        assert!(other < THRESHOLD_NORMAL, "different voice {other}");
        assert!(same - other > 0.2);
    }

    #[test]
    fn silence_and_tiny_clips_have_no_voiceprint() {
        assert!(extract(&vec![0.0; RATE]).is_none());
        assert!(extract(&a(0.02, 0.0)).is_none());
        assert!(extract(&vec![0.001; RATE]).is_none());
    }

    #[test]
    fn debug_output_hides_the_numbers() {
        let p = extract(&a(1.0, 0.0)).unwrap();
        assert_eq!(format!("{p:?}"), "Print(..)");
    }

    // Synthetic embeddings: the decision logic without audio.
    fn p(shape_seed: usize, pitch: f32) -> Print {
        let mut shape = [0f32; BANDS];
        for (i, v) in shape.iter_mut().enumerate() {
            *v = ((i * shape_seed) as f32 * 0.7).sin();
        }
        let mean = shape.iter().sum::<f32>() / BANDS as f32;
        shape.iter_mut().for_each(|v| *v -= mean);
        Print::from_parts(shape, pitch)
    }

    #[test]
    fn similarity_is_one_for_itself_and_pays_for_pitch() {
        let x = p(3, 120.0);
        assert!((similarity(&x, &x) - 1.0).abs() < 1e-5);
        let higher = p(3, 240.0); // an octave up
        assert!((similarity(&x, &higher) - (1.0 - 12.0 * PITCH_PENALTY_PER_SEMITONE)).abs() < 1e-4);
        assert!(similarity(&p(3, 120.0), &p(5, 120.0)) < 0.5);
        // No pitch on one side: no penalty.
        assert!((similarity(&x, &p(3, 0.0)) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn follow_ups_from_other_voices_are_ignored_and_the_focus_lapses() {
        let t0 = Instant::now();
        let mut f = Focus::new();
        f.begin(Some(p(3, 120.0)), t0);
        assert_eq!(f.check(Some(&p(3, 122.0)), t0 + Duration::from_secs(5)), Verdict::Accept);
        assert_eq!(f.check(Some(&p(7, 210.0)), t0 + Duration::from_secs(8)), Verdict::Reject);
        // A few quiet minutes later anyone is accepted again.
        let mut f = Focus::new();
        f.begin(Some(p(3, 120.0)), t0);
        assert_eq!(f.check(Some(&p(7, 210.0)), t0 + QUIET_LAPSE + Duration::from_secs(1)), Verdict::Accept);
        assert!(!f.active());
    }

    #[test]
    fn hearing_the_focused_voice_keeps_the_focus_alive() {
        let t0 = Instant::now();
        let mut f = Focus::new();
        f.begin(Some(p(3, 120.0)), t0);
        let mut t = t0;
        for _ in 0..5 {
            t += Duration::from_secs(150); // each gap is under the lapse, the total is far over it
            assert_eq!(f.check(Some(&p(3, 120.0)), t), Verdict::Accept);
        }
        assert_eq!(f.check(Some(&p(7, 210.0)), t + Duration::from_secs(1)), Verdict::Reject);
    }

    #[test]
    fn strict_is_stricter_and_unclear_clips_depend_on_the_mode() {
        let t0 = Instant::now();
        let anchor = p(3, 120.0);
        // Something in between: slightly different shape.
        let mut mid = anchor.clone();
        mid.shape[2] += 0.9;
        mid.shape[9] -= 0.9;
        let s = similarity(&anchor, &mid);
        assert!(s > THRESHOLD_NORMAL - 0.12, "{s}");
        let mut normal = Focus::new();
        normal.begin(Some(anchor.clone()), t0);
        assert_eq!(normal.check(None, t0), Verdict::Accept);
        let mut strict = Focus::new();
        strict.set_mode(Mode::Strict);
        strict.begin(Some(anchor), t0);
        assert_eq!(strict.check(None, t0), Verdict::Reject);
        assert!(THRESHOLD_STRICT > THRESHOLD_NORMAL);
    }

    #[test]
    fn off_accepts_everything_and_keeps_nothing() {
        let t0 = Instant::now();
        let mut f = Focus::new();
        f.set_mode(Mode::Off);
        f.begin(Some(p(3, 120.0)), t0);
        assert!(!f.active());
        assert_eq!(f.check(Some(&p(9, 300.0)), t0), Verdict::Accept);
    }

    #[test]
    fn everyone_and_named_voices() {
        let t0 = Instant::now();
        let mut f = Focus::new();
        f.begin(Some(p(3, 120.0)), t0);
        f.allow_name("  Anna ");
        f.allow_name("anna");
        assert_eq!(f.names(), ["anna".to_string()]);
        assert_eq!(f.check(Some(&p(7, 210.0)), t0), Verdict::CheckName);
        f.listen_to_everyone();
        assert_eq!(f.check(Some(&p(7, 210.0)), t0), Verdict::Accept);
        // The next phrase starts over.
        f.begin(Some(p(5, 140.0)), t0);
        assert!(f.active() && f.names().is_empty());
    }

    #[test]
    fn commands_parse_from_the_page() {
        let c: Command = serde_json::from_str(r#"{"alsoListenTo":{"name":"Anna"}}"#).unwrap();
        assert_eq!(c, Command::AlsoListenTo { name: "Anna".into() });
        let c: Command = serde_json::from_str(r#""everyone""#).unwrap();
        assert_eq!(c, Command::Everyone);
    }
}
