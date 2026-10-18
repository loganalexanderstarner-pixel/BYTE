//! The quick "Hey BYTE" detector: a tiny neural network that listens to every audio frame and says "that sounded like the phrase".
//!
//! It is the cheap first stage. Whisper still confirms every candidate (wake.rs), so a wrong guess costs one extra check and a right
//! one lets the listener skip Whisper for all the speech that is not the phrase. The detector alone never wakes BYTE.
//!
//! How it works: 16 kHz audio -> 40 log-mel bands every 10 ms (own FFT, no audio library) -> a causal network of six dilated depthwise
//! blocks (63 k weights, see scripts/wakemodel/train.py, embedded from assets/wakemodel.bin as 16-bit numbers, 123 KB) -> one score per frame. The network is
//! computed one new frame at a time, so the cost is about 6 million multiply-adds per second of audio (2.5 ms of one core per second of audio in a release build). This file uses only `std`.
//!
//! The maths of the features is the same as scripts/wakemodel/feats.py; `golden` in the tests compares both against numbers the
//! training scripts wrote.

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

const RATE: usize = 16_000;
const WIN: usize = 400;
const HOP: usize = 160;
const NFFT: usize = 512;
const NMEL: usize = 40;
const FLOOR: f32 = 1e-7;
const ALPHA: f32 = 0.005;
/// Frames ignored after start-up while the running mean settles (1 s).
const WARMUP: u64 = 100;
/// After a candidate, none for this many frames (1.5 s).
const REFRACTORY: u64 = 150;
/// The decision is on the mean of this many consecutive scores.
const SMOOTH: usize = 3;
/// Audio kept for the confirmation check (seconds).
pub const KEEP_SECS: usize = 3;

static WEIGHTS: &[u8] = include_bytes!("../assets/wakemodel.bin");

// ---- setting ----

const MODE_CAREFUL: u8 = 0;
const MODE_QUICK: u8 = 1;
static MODE: AtomicU8 = AtomicU8::new(MODE_CAREFUL);

/// "quick": the detector picks the bursts Whisper checks. Anything else ("careful"): Whisper checks every burst, as before.
pub fn set_mode(m: &str) {
    MODE.store(if m == "quick" { MODE_QUICK } else { MODE_CAREFUL }, Ordering::Relaxed);
}

pub fn quick_on() -> bool {
    MODE.load(Ordering::Relaxed) == MODE_QUICK
}

// ---- the audio left over after echo cancelling, offered by bargein.rs while BYTE speaks ----

static RESIDUAL: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// While BYTE speaks, the echo canceller hands the microphone audio with BYTE's own voice taken out (16 kHz). `usable` is false until the
/// canceller has found its delay; then the chunk still holds BYTE's voice and is dropped. Cheap when the quick detector is off.
pub fn offer_residual(chunk: &[f32], usable: bool) {
    if !quick_on() {
        return;
    }
    if let Ok(mut r) = RESIDUAL.lock() {
        if !usable {
            r.clear();
            return;
        }
        r.extend_from_slice(chunk);
        let extra = r.len().saturating_sub(RATE);
        r.drain(..extra);
    }
}

/// What `offer_residual` collected since the last call.
pub fn take_residual() -> Vec<f32> {
    RESIDUAL.lock().map(|mut r| std::mem::take(&mut *r)).unwrap_or_default()
}

// ---- features ----

struct Tables {
    window: [f32; WIN],
    cos: [f32; NFFT / 2],
    sin: [f32; NFFT / 2],
    rev: [u16; NFFT],
    /// For each band: first FFT bin and the triangle weights.
    bank: Vec<(usize, Vec<f32>)>,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut window = [0.0; WIN];
        for (i, w) in window.iter_mut().enumerate() {
            *w = 0.5 - 0.5 * (2.0 * PI * i as f32 / WIN as f32).cos();
        }
        let (mut cos, mut sin) = ([0.0; NFFT / 2], [0.0; NFFT / 2]);
        for k in 0..NFFT / 2 {
            let a = -2.0 * std::f64::consts::PI * k as f64 / NFFT as f64;
            cos[k] = a.cos() as f32;
            sin[k] = a.sin() as f32;
        }
        let mut rev = [0u16; NFFT];
        for (i, r) in rev.iter_mut().enumerate() {
            *r = (i as u16).reverse_bits() >> (16 - 9);
        }
        let hz2mel = |f: f64| 2595.0 * (1.0 + f / 700.0).log10();
        let mel2hz = |m: f64| 700.0 * (10f64.powf(m / 2595.0) - 1.0);
        let (lo, hi) = (hz2mel(60.0), hz2mel(7600.0));
        let pts: Vec<f64> = (0..NMEL + 2).map(|i| mel2hz(lo + (hi - lo) * i as f64 / (NMEL + 1) as f64)).collect();
        let bin_hz = |b: usize| b as f64 * RATE as f64 / NFFT as f64;
        let bank = (0..NMEL)
            .map(|i| {
                let (l, c, h) = (pts[i], pts[i + 1], pts[i + 2]);
                let mut first = None;
                let mut w = vec![];
                for b in 0..=NFFT / 2 {
                    let f = bin_hz(b);
                    let v = ((f - l) / (c - l)).min((h - f) / (h - c)).max(0.0);
                    if v > 0.0 {
                        first.get_or_insert(b);
                        w.push(v as f32);
                    }
                }
                (first.unwrap_or(0), w)
            })
            .collect();
        Tables { window, cos, sin, rev, bank }
    })
}

/// Power spectrum (bins 0..=256) of one 400-sample frame.
fn power(frame: &[f32], t: &Tables, re: &mut [f32; NFFT], im: &mut [f32; NFFT]) -> [f32; NFFT / 2 + 1] {
    for i in 0..NFFT {
        let src = t.rev[i] as usize;
        re[i] = if src < WIN { frame[src] * t.window[src] } else { 0.0 };
        im[i] = 0.0;
    }
    let mut len = 2;
    while len <= NFFT {
        let half = len / 2;
        let step = NFFT / len;
        for start in (0..NFFT).step_by(len) {
            for k in 0..half {
                let (c, s) = (t.cos[k * step], t.sin[k * step]);
                let (a, b) = (start + k, start + k + half);
                let (xr, xi) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        len *= 2;
    }
    let mut p = [0.0; NFFT / 2 + 1];
    for (k, v) in p.iter_mut().enumerate() {
        *v = re[k] * re[k] + im[k] * im[k];
    }
    p
}

// ---- the network ----

struct Block {
    d: usize,
    /// Depthwise taps, tap-major: [oldest C][middle C][newest C].
    dw: Vec<f32>,
    pw: Vec<f32>,
    b: Vec<f32>,
}

struct Weights {
    c: usize,
    thr: f32,
    inp_w: Vec<f32>,
    inp_b: Vec<f32>,
    blocks: Vec<Block>,
    head_w: Vec<f32>,
    head_b: f32,
}

/// A 16-bit float (IEEE half) as a 32-bit one.
fn half_to_f32(h: u16) -> f32 {
    let (sign, exp, frac) = ((h >> 15) as u32, ((h >> 10) & 0x1f) as u32, (h & 0x3ff) as u32);
    let bits = if exp == 0 {
        if frac == 0 {
            sign << 31
        } else {
            // A subnormal half: normalise it.
            let mut e = 127 - 15 + 1;
            let mut f = frac;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            (sign << 31) | (e << 23) | ((f & 0x3ff) << 13)
        }
    } else if exp == 31 {
        (sign << 31) | 0x7f80_0000 | (frac << 13)
    } else {
        (sign << 31) | ((exp + 127 - 15) << 23) | (frac << 13)
    };
    f32::from_bits(bits)
}

fn parse(bytes: &[u8]) -> Option<Weights> {
    let u32at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if bytes.get(..4)? != b"BYWM" {
        return None;
    }
    // Version 1: 32-bit float weights. Version 2: 16-bit float weights.
    let width = match u32at(4)? {
        1 => 4,
        2 => 2,
        _ => return None,
    };
    let (c, nb, nmel) = (u32at(8)? as usize, u32at(12)? as usize, u32at(16)? as usize);
    let thr = f32::from_bits(u32at(20)?);
    if nmel != NMEL || c == 0 || c > 512 || nb > 16 {
        return None;
    }
    let mut at = 24;
    let dil: Vec<usize> = (0..nb).map(|i| u32at(at + 4 * i).map(|v| v as usize)).collect::<Option<_>>()?;
    at += 4 * nb;
    let want = c * NMEL + c + nb * (3 * c + c * c + c) + c + 1;
    if bytes.len() - at != want * width {
        return None;
    }
    let mut pos = at;
    let mut take = |n: usize| -> Vec<f32> {
        let v = (0..n)
            .map(|i| {
                let o = pos + width * i;
                if width == 4 {
                    f32::from_bits(u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]))
                } else {
                    half_to_f32(u16::from_le_bytes([bytes[o], bytes[o + 1]]))
                }
            })
            .collect();
        pos += width * n;
        v
    };
    let inp_w = take(c * NMEL);
    let inp_b = take(c);
    let blocks = dil.iter().map(|&d| Block { d, dw: take(3 * c), pw: take(c * c), b: take(c) }).collect();
    let head_w = take(c);
    let head_b = take(1)[0];
    Some(Weights { c, thr, inp_w, inp_b, blocks, head_w, head_b })
}

fn weights() -> Option<&'static Weights> {
    static W: OnceLock<Option<Weights>> = OnceLock::new();
    W.get_or_init(|| parse(WEIGHTS)).as_ref()
}

/// The detector found something that sounded like the phrase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// Mean of the last three scores, 0..1.
    pub score: f32,
    /// Feature frames (10 ms) since the detector started.
    pub frame: u64,
}

/// Streaming detector: feed 16 kHz audio in chunks of any size.
pub struct Detector {
    w: &'static Weights,
    t: &'static Tables,
    pending: Vec<f32>,
    mean: [f32; NMEL],
    frames: u64,
    rings: Vec<Vec<f32>>,
    last: VecDeque<f32>,
    last_fire: Option<u64>,
    audio: VecDeque<f32>,
    /// The highest smoothed score since the last `take_peak`.
    peak: f32,
    thr: f32,
    re: Box<[f32; NFFT]>,
    im: Box<[f32; NFFT]>,
}

impl Detector {
    /// None when the embedded model cannot be read (the listener then keeps using Whisper alone).
    pub fn new() -> Option<Self> {
        let w = weights()?;
        Some(Detector {
            w,
            t: tables(),
            pending: vec![],
            mean: [0.0; NMEL],
            frames: 0,
            rings: w.blocks.iter().map(|b| vec![0.0; (2 * b.d + 1) * w.c]).collect(),
            last: VecDeque::new(),
            last_fire: None,
            audio: VecDeque::new(),
            peak: 0.0,
            thr: w.thr,
            re: Box::new([0.0; NFFT]),
            im: Box::new([0.0; NFFT]),
        })
    }

    /// Forgets the stream (after a gap in the audio, e.g. while the listener was paused).
    pub fn reset(&mut self) {
        let (w, t) = (self.w, self.t);
        *self = Detector { w, t, ..Detector::new().expect("weights parsed before") };
    }

    /// Overrides the built-in threshold (tests, and the lowered value when the person asks for more sensitivity).
    pub fn set_threshold(&mut self, thr: f32) {
        self.thr = thr;
    }

    pub fn threshold(&self) -> f32 {
        self.thr
    }

    /// 10 ms frames handled since the start or the last reset.
    pub fn frames_seen(&self) -> u64 {
        self.frames
    }

    /// The last `secs` seconds of audio fed so far (for the Whisper confirmation of a candidate).
    pub fn recent_audio(&self, secs: f32) -> Vec<f32> {
        let n = ((secs * RATE as f32) as usize).min(self.audio.len());
        self.audio.iter().skip(self.audio.len() - n).copied().collect()
    }

    /// Highest smoothed score seen since the last call, for the Test log.
    pub fn take_peak(&mut self) -> f32 {
        std::mem::take(&mut self.peak)
    }

    /// Feeds audio; returns the candidate when the phrase just ended in it (at most one per call chunk, and none for 1.5 s after one).
    pub fn feed(&mut self, samples: &[f32]) -> Option<Candidate> {
        self.audio.extend(samples.iter().copied());
        let extra = self.audio.len().saturating_sub(KEEP_SECS * RATE);
        self.audio.drain(..extra);
        self.pending.extend_from_slice(samples);
        let mut found = None;
        let mut used = 0;
        while self.pending.len() - used >= WIN {
            let score = self.frame(used);
            used += HOP;
            if let Some(c) = self.decide(score) {
                found = Some(c);
            }
        }
        self.pending.drain(..used);
        found
    }

    fn decide(&mut self, score: f32) -> Option<Candidate> {
        self.last.push_back(score);
        if self.last.len() > SMOOTH {
            self.last.pop_front();
        }
        let s = self.last.iter().sum::<f32>() / SMOOTH as f32;
        let n = self.frames;
        if n < WARMUP || self.last.len() < SMOOTH {
            return None;
        }
        self.peak = self.peak.max(s);
        if s >= self.thr && self.last_fire.map_or(true, |f| n - f >= REFRACTORY) {
            self.last_fire = Some(n);
            return Some(Candidate { score: s, frame: n });
        }
        None
    }

    /// One 10 ms step: features of the frame starting at `pending[at]`, then the network. Returns the score.
    fn frame(&mut self, at: usize) -> f32 {
        let spec = power(&self.pending[at..at + WIN], self.t, &mut self.re, &mut self.im);
        let mut x = [0.0f32; NMEL];
        for (i, (first, w)) in self.t.bank.iter().enumerate() {
            let e: f32 = w.iter().zip(&spec[*first..]).map(|(a, b)| a * b).sum();
            x[i] = (e + FLOOR).ln();
        }
        if self.frames == 0 {
            self.mean = x;
        } else {
            for i in 0..NMEL {
                self.mean[i] += ALPHA * (x[i] - self.mean[i]);
            }
        }
        let mut f = [0.0f32; NMEL];
        for i in 0..NMEL {
            f[i] = x[i] - self.mean[i];
        }
        let score = self.net(&f);
        self.frames += 1;
        score
    }

    fn net(&mut self, f: &[f32; NMEL]) -> f32 {
        let w = self.w;
        let c = w.c;
        let mut h = vec![0.0f32; c];
        for o in 0..c {
            let row = &w.inp_w[o * NMEL..(o + 1) * NMEL];
            let s: f32 = row.iter().zip(f).map(|(a, b)| a * b).sum::<f32>() + w.inp_b[o];
            h[o] = s.max(0.0);
        }
        let t = self.frames as usize;
        let mut y = vec![0.0f32; c];
        for (blk, ring) in w.blocks.iter().zip(self.rings.iter_mut()) {
            let n = 2 * blk.d + 1;
            ring[(t % n) * c..(t % n + 1) * c].copy_from_slice(&h);
            let (old, mid) = (((t + 1) % n) * c, ((t + blk.d + 1) % n) * c);
            for k in 0..c {
                y[k] = blk.dw[k] * ring[old + k] + blk.dw[c + k] * ring[mid + k] + blk.dw[2 * c + k] * h[k];
            }
            for o in 0..c {
                let row = &blk.pw[o * c..(o + 1) * c];
                let s: f32 = row.iter().zip(&y).map(|(a, b)| a * b).sum::<f32>() + blk.b[o];
                h[o] += s.max(0.0);
            }
        }
        let logit: f32 = w.head_w.iter().zip(&h).map(|(a, b)| a * b).sum::<f32>() + w.head_b;
        1.0 / (1.0 + (-logit).exp())
    }
}

/// Scores for every frame of `audio` (tests and the golden comparison).
#[cfg(test)]
fn scores(audio: &[f32]) -> Vec<f32> {
    let mut d = Detector::new().unwrap();
    let mut out = vec![];
    let mut p = 0;
    d.pending.extend_from_slice(audio);
    while d.pending.len() - p >= WIN {
        out.push(d.frame(p));
        p += HOP;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden_signal(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let i = i as f64;
                (0.2 * (0.05 * i).sin() + 0.1 * (0.9 * i).sin() * (0.5 + 0.5 * (0.0003 * i).sin()) + 0.02 * (1.7e-4 * i * i).sin()) as f32
            })
            .collect()
    }

    #[test]
    fn the_embedded_model_loads_and_is_small() {
        let w = weights().expect("model parses");
        assert!(w.blocks.len() >= 6);
        assert!(WEIGHTS.len() < 400_000, "weights are {} bytes", WEIGHTS.len());
        assert!(w.thr > 0.2 && w.thr < 0.99);
    }

    #[test]
    fn half_floats_convert_exactly() {
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0xc000), -2.0);
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3555), 0.333251953125);
        assert_eq!(half_to_f32(0x0001), 2f32.powi(-24)); // smallest subnormal
        assert!(half_to_f32(0x7c00).is_infinite());
    }

    #[test]
    fn matches_the_training_code_on_a_fixed_signal() {
        let g: serde_json::Value = serde_json::from_str(include_str!("../assets/wakemodel_golden.json")).unwrap();
        let x = golden_signal(g["samples"].as_u64().unwrap() as usize);
        let s = scores(&x);
        assert_eq!(s.len() as u64, g["frames"].as_u64().unwrap());
        let idx = g["idx"].as_array().unwrap();
        let want = g["scores"].as_array().unwrap();
        for (i, w) in idx.iter().zip(want) {
            let (i, w) = (i.as_u64().unwrap() as usize, w.as_f64().unwrap() as f32);
            assert!((s[i] - w).abs() < 2e-3, "frame {i}: rust {} vs python {w}", s[i]);
        }
    }

    #[test]
    fn chunk_size_does_not_change_the_scores() {
        let x = golden_signal(32_000);
        let whole = scores(&x);
        let mut d = Detector::new().unwrap();
        d.set_threshold(2.0);
        let mut peaks = vec![];
        for c in x.chunks(777) {
            d.feed(c);
            peaks.push(d.take_peak());
        }
        let top = peaks.iter().cloned().fold(0.0, f32::max);
        let w3 = whole.windows(3).skip(100).map(|w| w.iter().sum::<f32>() / 3.0).fold(0.0, f32::max);
        assert!((top - w3).abs() < 1e-4, "{top} vs {w3}");
    }

    #[test]
    fn silence_and_noise_never_fire() {
        let mut d = Detector::new().unwrap();
        let mut seed = 12345u32;
        let noise: Vec<f32> = (0..16_000 * 30)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                ((seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.1
            })
            .collect();
        for c in noise.chunks(1600) {
            assert!(d.feed(c).is_none());
        }
        for c in vec![0.0f32; 16_000 * 5].chunks(1600) {
            assert!(d.feed(c).is_none());
        }
    }

    #[test]
    fn residual_inbox_only_collects_in_quick_mode() {
        let _g = crate::echo::tests_lock();
        set_mode("careful");
        take_residual();
        offer_residual(&[0.1; 160], true);
        assert!(take_residual().is_empty());
        set_mode("quick");
        offer_residual(&[0.1; 160], true);
        offer_residual(&[0.2; 160], true);
        assert_eq!(take_residual().len(), 320);
        offer_residual(&[0.1; 160], false);
        assert!(take_residual().is_empty());
        set_mode("careful");
    }

    /// Real speech from voices the network never trained on (assets/wakemodel_fixture.*, see scripts/wakemodel/make_fixture.py):
    /// three phrases, one ordinary sentence and "hey siri". Each phrase must lift the score above the gate near its end, the others not.
    #[test]
    fn spots_the_phrase_in_held_out_speech_and_not_other_speech() {
        let meta: serde_json::Value = serde_json::from_str(include_str!("../assets/wakemodel_fixture.json")).unwrap();
        let mu = include_bytes!("../assets/wakemodel_fixture.mu");
        let audio: Vec<f32> = mu
            .iter()
            .map(|&b| {
                let y = (b as f32 - 128.0) / 127.0;
                y.signum() * (256f32.powf(y.abs()) - 1.0) / 255.0
            })
            .collect();
        assert_eq!(audio.len() as u64, meta["samples"].as_u64().unwrap());
        let mut d = Detector::new().unwrap();
        // Highest smoothed score of every 10 ms step.
        let mut peaks = vec![];
        for c in audio.chunks(HOP) {
            d.feed(c);
            peaks.push(d.take_peak());
        }
        let gate = d.threshold() * 0.5;
        let best = |from: usize, to: usize| peaks[(from / HOP).min(peaks.len())..(to / HOP).min(peaks.len())].iter().cloned().fold(0.0f32, f32::max);
        for end in meta["phrase_ends"].as_array().unwrap() {
            let end = end.as_u64().unwrap() as usize;
            let top = best(end.saturating_sub(RATE * 3 / 10), end + RATE * 9 / 10);
            assert!(top >= gate, "phrase ending at {:.2} s scored only {top:.2} (gate {gate:.2})", end as f32 / RATE as f32);
        }
        for span in meta["spans"].as_array().unwrap() {
            if span[0] == "phrase" {
                continue;
            }
            let (from, to) = (span[1].as_u64().unwrap() as usize, span[2].as_u64().unwrap() as usize);
            let top = best(from, to + RATE / 2);
            assert!(top < gate, "{} at {:.2} s scored {top:.2} (gate {gate:.2})", span[0], from as f32 / RATE as f32);
        }
        // And whole-stream: the detector fires (score at or above the threshold) at least once for these three phrases.
        let mut d = Detector::new().unwrap();
        let fired: usize = audio.chunks(1600).filter_map(|c| d.feed(c)).count();
        eprintln!("wakemodel fixture: {fired} candidates for 3 phrases");
        assert!(fired >= 1);
    }

    /// Time per second of audio (debug build: an upper bound; the release build is much faster).
    #[test]
    fn cost_per_second_of_audio() {
        let x = golden_signal(16_000 * 20);
        let mut d = Detector::new().unwrap();
        let t = std::time::Instant::now();
        for c in x.chunks(1600) {
            d.feed(c);
        }
        let ms_per_s = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        eprintln!("wakemodel: {ms_per_s:.2} ms of CPU per second of audio (test build)");
        assert!(ms_per_s < 200.0, "{ms_per_s}");
    }
}
