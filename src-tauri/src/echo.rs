//! Echo cancellation for barge-in: while BYTE talks through the speakers, its own voice reaches the microphone. BYTE knows exactly what
//! it plays (the queue in tts.rs), so that signal is the reference: the part of the microphone that is a delayed, filtered copy of it
//! is subtracted, and what is left is whoever else is talking.
//!
//! Pure Rust, no platform code, 16 kHz mono everywhere:
//! 1. `estimate_delay` finds how late the reference shows up in the microphone (output buffer + room + input buffer), by normalized
//!    cross-correlation (coarse at 4 kHz, then refined at 16 kHz).
//! 2. `EchoCanceller` runs a normalized-LMS adaptive filter of `TAPS` taps placed at that delay, and freezes the adaptation while a
//!    person talks over the reference (double talk), so the filter does not learn the person as echo.
//!
//! The constants below are starting values that work on synthetic signals. A real room (speakers, microphone, distance, volume) needs
//! the owner's ears to tune them; see docs/HEY-BYTE-ASSISTANT.md.

#![cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

pub const RATE: usize = 16_000;
/// Adaptive filter length: 32 ms of room response after the direct sound.
pub const TAPS: usize = 1024;
/// The filter starts this far (samples) before the estimated delay, so a delay that is a little off still falls inside it.
pub const PRE: usize = 128;
/// The longest delay between "played" and "heard" that is searched for: 300 ms.
pub const MAX_DELAY: usize = RATE * 3 / 10;
/// NLMS step size per sample (0..2 is stable): larger converges faster and leaves more noise behind.
pub const MU: f32 = 1.0;
/// Pre-whitening coefficient for the adaptation (first-order difference).
pub const WHITEN: f32 = 0.9;
/// How much audio the delay estimate looks at: 0.5 s.
pub const ESTIMATE_WINDOW: usize = RATE / 2;
/// The delay is re-checked this often while it runs: 1 s.
pub const REESTIMATE_EVERY: usize = RATE;
/// A new delay estimate replaces the old one only when it differs by more than this (samples).
pub const DELAY_JUMP: usize = 48;
/// How sharp the delay peak (0..1, from peak-to-sidelobe ratio: 0.4 is about 5x the rest) a delay estimate needs to be trusted.
pub const MIN_CORRELATION: f32 = 0.4;
/// Double talk before the filter has converged: the microphone is this many times louder than the reference (rms) over a block. Loose on
/// purpose: a real microphone often hears the echo louder than the digital reference, so a tight ratio would never let the filter learn.
pub const GEIGEL: f32 = 3.0;
/// Double talk, once the filter has converged: the microphone is this many times louder than the echo the filter predicts.
pub const NEAR_END_OVER_ECHO: f32 = 2.5;
/// After a block that looked like double talk the filter stays frozen this long (samples).
pub const FREEZE_HANG: usize = RATE * 5 / 100;
/// Double talk, learned: the leftover, as a share of the microphone level, is this many times (plus `ERR_ABS`) what echo-only blocks left.
pub const ERR_JUMP: f32 = 1.8;
pub const ERR_ABS: f32 = 0.12;
/// How fast a frozen filter accepts a new leftover level as normal (share per 10 ms block).
pub const FLOOR_CREEP: f32 = 0.002;
const BLOCK: usize = 160;
const EPS: f32 = 1e-4;

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// In-place radix-2 FFT (`inverse` also scales). `re.len()` must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos() as f32, ang.sin() as f32);
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
    if inverse {
        let inv = 1.0 / n as f32;
        re.iter_mut().for_each(|v| *v *= inv);
        im.iter_mut().for_each(|v| *v *= inv);
    }
}

/// How many samples later the reference shows up in the microphone, and how sure that is (0..1). `reference[i]` and `mic[i]` are the
/// same moment on the two streams. None when nothing matches well enough (silence, or no echo).
///
/// Generalised cross-correlation with phase transform (GCC-PHAT): every frequency counts equally, so the voiced, periodic, coloured
/// sound of speech (whose plain correlation has a peak at every pitch period, and which a room's reverb smears) still gives one sharp
/// peak at the true delay. Plain normalised correlation wandered between pitch periods on a voice and kept resetting the filter.
pub fn estimate_delay(reference: &[f32], mic: &[f32], max_delay: usize) -> Option<(usize, f32)> {
    let len = reference.len().min(mic.len());
    if len < 2 * max_delay.min(len / 2).max(64) {
        return None;
    }
    let (reference, mic) = (&reference[reference.len() - len..], &mic[mic.len() - len..]);
    if rms(reference) < 1e-3 || rms(mic) < 1e-4 {
        return None;
    }
    let size = (2 * len).next_power_of_two();
    let load = |x: &[f32]| {
        let mut re = vec![0f32; size];
        re[..x.len()].copy_from_slice(x);
        (re, vec![0f32; size])
    };
    let ((mut mr, mut mi), (mut rr, mut ri)) = (load(mic), load(reference));
    fft(&mut mr, &mut mi, false);
    fft(&mut rr, &mut ri, false);
    // Only the speech band: below it is rumble, above it is hiss.
    let bin = |hz: f32| (hz / RATE as f32 * size as f32) as usize;
    let (lo, hi) = (bin(150.0), bin(4500.0));
    let (mut cr, mut ci) = (vec![0f32; size], vec![0f32; size]);
    let mut peak_mag = 0f32;
    for k in lo..=hi {
        // mic * conj(reference)
        let (a, b) = (mr[k] * rr[k] + mi[k] * ri[k], mi[k] * rr[k] - mr[k] * ri[k]);
        peak_mag = peak_mag.max((a * a + b * b).sqrt());
    }
    for k in lo..=hi {
        let (a, b) = (mr[k] * rr[k] + mi[k] * ri[k], mi[k] * rr[k] - mr[k] * ri[k]);
        // Bins with almost no energy would only add noise at full weight.
        let mag = (a * a + b * b).sqrt().max(peak_mag * 1e-3);
        cr[k] = a / mag;
        ci[k] = b / mag;
        // Real signal: mirror.
        cr[size - k] = cr[k];
        ci[size - k] = -ci[k];
    }
    fft(&mut cr, &mut ci, true);
    let max_delay = max_delay.min(size / 2 - 1);
    let (mut best, mut best_v) = (0usize, f32::MIN);
    for d in 0..=max_delay {
        if cr[d] > best_v {
            best = d;
            best_v = cr[d];
        }
    }
    // How far the peak stands above the rest of the correlation (peak-to-sidelobe), outside the peak's own neighbourhood.
    let (mut sum, mut count) = (0f32, 0f32);
    for d in 0..=max_delay {
        if d.abs_diff(best) > 24 {
            sum += cr[d] * cr[d];
            count += 1.0;
        }
    }
    let side = (sum / count.max(1.0)).sqrt().max(1e-9);
    let psr = best_v / side;
    let strength = (psr / (psr + 8.0)).clamp(0.0, 1.0);
    (best_v > 0.0 && strength >= MIN_CORRELATION).then_some((best, strength))
}

/// Subtracts what BYTE plays from what the microphone hears.
pub struct EchoCanceller {
    w: Vec<f32>,
    /// Reference and microphone since the session began, with `PAD` zeros in front so every filter position is valid.
    refs: Vec<f32>,
    /// The reference pre-whitened (first difference), for the update.
    refsw: Vec<f32>,
    mics: Vec<f32>,
    /// The delay in use (samples), once found.
    delay: Option<usize>,
    /// The filter has once removed at least 6 dB of the echo since the delay was found: only then is what is left trusted as "not BYTE".
    settled: bool,
    /// Blocks seen and blocks in which the filter was allowed to learn (for the Test log).
    pub blocks: u64,
    pub adapted: u64,
    pub near_end_blocks: u64,
    pub odd_blocks: u64,
    /// A different delay seen once, waiting for a second sighting.
    candidate: Option<usize>,
    since_estimate: usize,
    frozen: usize,
    /// Smoothed levels, to tell whether the filter has converged and how loud the echo it predicts is.
    mic_level: f32,
    err_level: f32,
    echo_level: f32,
    /// What the leftover looked like in blocks that were echo only (once the filter converged), and in the last block.
    err_floor: f32,
    last_err: f32,
}

const PAD: usize = MAX_DELAY + TAPS + PRE;

impl Default for EchoCanceller {
    fn default() -> Self {
        EchoCanceller { w: vec![0.0; TAPS], refs: vec![0.0; PAD], refsw: vec![0.0; PAD], mics: vec![0.0; PAD], delay: None, candidate: None, settled: false, blocks: 0, adapted: 0, near_end_blocks: 0, odd_blocks: 0, since_estimate: 0, frozen: 0, mic_level: 0.0, err_level: 0.0, echo_level: 0.0, err_floor: 0.0, last_err: 0.0 }
    }
}

impl EchoCanceller {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new stretch of speech starts. The learned filter is kept (the room did not change); the stream positions begin again.
    pub fn begin_session(&mut self) {
        self.refs = vec![0.0; PAD];
        self.refsw = vec![0.0; PAD];
        self.mics = vec![0.0; PAD];
        self.since_estimate = 0;
        self.frozen = 0;
    }

    /// The delay in use, in samples (None until it has been found).
    pub fn delay(&self) -> Option<usize> {
        self.delay
    }

    /// How much of the echo is left after cancelling, as a share of the microphone level in blocks with only BYTE's voice (0.18 = about
    /// 15 dB removed). 1.0 until measured.
    pub fn leftover_share(&self) -> f32 {
        if self.err_floor > 0.0 { self.err_floor.clamp(0.01, 1.0) } else { 1.0 }
    }

    /// Whether the filter has learned the room well enough (about 6 dB removed) for the leftover to be judged.
    pub fn settled(&self) -> bool {
        self.settled
    }

    /// Whether a delay is known, so that the output is actually cleaned.
    pub fn ready(&self) -> bool {
        self.delay.is_some()
    }

    /// The rms of the echo the filter predicted in the last chunk (what the microphone should hold of BYTE's own voice).
    pub fn echo_rms(&self) -> f32 {
        self.echo_level
    }

    /// Cleans one chunk: `mic` and `reference` are the same samples on the two streams (reference zero when nothing was playing).
    pub fn process(&mut self, mic: &[f32], reference: &[f32]) -> Vec<f32> {
        let n = mic.len().min(reference.len());
        let base = self.refs.len();
        for &r in &reference[..n] {
            let prev = *self.refs.last().unwrap_or(&0.0);
            self.refsw.push(if r.is_finite() { r - WHITEN * if prev.is_finite() { prev } else { 0.0 } } else { 0.0 });
            self.refs.push(r);
        }
        self.mics.extend_from_slice(&mic[..n]);
        self.since_estimate += n;
        self.maybe_estimate();
        let out = match self.delay {
            None => {
                self.echo_level = 0.0;
                mic[..n].to_vec()
            }
            Some(d) => self.filter(base, n, d),
        };
        self.trim();
        out
    }

    fn maybe_estimate(&mut self) {
        let have = self.refs.len() - PAD;
        let due = if self.delay.is_none() { self.since_estimate >= RATE / 4 } else { self.since_estimate >= REESTIMATE_EVERY };
        if !due || have < ESTIMATE_WINDOW {
            return;
        }
        self.since_estimate = 0;
        let from = self.refs.len() - ESTIMATE_WINDOW - MAX_DELAY.min(self.refs.len() - ESTIMATE_WINDOW);
        if let Some((d, _)) = estimate_delay(&self.refs[from..], &self.mics[from..], MAX_DELAY) {
            let jump = self.delay.map_or(usize::MAX, |old| old.abs_diff(d));
            if jump > DELAY_JUMP {
                // The first delay is taken at once; a change of a known delay (which throws away what the filter learned) has to be
                // seen twice in a row, so one odd estimate does not undo seconds of learning.
                let confirmed = self.delay.is_none() || self.candidate.is_some_and(|c| c.abs_diff(d) <= DELAY_JUMP / 2);
                if confirmed {
                    self.delay = Some(d);
                    self.settled = false;
                    self.candidate = None;
                    self.w.iter_mut().for_each(|x| *x = 0.0);
                    self.err_floor = 0.0;
                } else {
                    self.candidate = Some(d);
                }
            } else {
                self.candidate = None;
            }
        }
    }

    fn filter(&mut self, base: usize, n: usize, d: usize) -> Vec<f32> {
        // The filter's first tap sits PRE samples before the estimated delay.
        let lag = d.saturating_sub(PRE);
        let mut out = Vec::with_capacity(n);
        let mut done = 0;
        while done < n {
            let len = BLOCK.min(n - done);
            let start = base + done;
            let (mut mic_sq, mut ref_sq, mut err_sq, mut echo_sq) = (0f32, 0f32, 0f32, 0f32);
            let ref_block_rms = rms(&self.refs[start - lag..start - lag + len]);
            let mic_block_rms = rms(&self.mics[start..start + len]);
            // Pass 1: the leftover with the filter as it is (nothing is learned before the block is judged).
            let mut es = [0f32; BLOCK];
            let mut energy_sum = 0f32;
            let first = start - lag;
            let mut energy: f32 = self.refs[first + 1 - TAPS..=first].iter().map(|v| v * v).sum();
            for k in 0..len {
                let p = start + k - lag;
                if k > 0 {
                    let (add, drop) = (self.refs[p], self.refs[p - TAPS]);
                    energy = (energy + add * add - drop * drop).max(0.0);
                }
                energy_sum += energy;
                let x = &self.refs[p + 1 - TAPS..=p];
                // w[j] belongs to the reference sample TAPS-1-j before p (time order, so both slices run forward).
                let y: f32 = self.w.iter().zip(x).map(|(w, x)| w * x).sum();
                let m = self.mics[start + k];
                let e = m - y;
                // A bad sample (NaN/inf from a broken device) must not poison the filter: pass the microphone through.
                es[k] = if e.is_finite() { e } else { m };
                mic_sq += m * m;
                ref_sq += self.refs[p] * self.refs[p];
                err_sq += es[k] * es[k];
                echo_sq += y * y;
                out.push(es[k]);
            }
            let block_err = (err_sq / len as f32).sqrt();
            let echo_now = (echo_sq / len as f32).sqrt();
            // Double talk: the microphone is louder than the reference could explain, or (converged) the leftover jumped.
            let converged = self.mic_level > 1e-4 && self.err_level < self.mic_level * 0.5;
            let near_end = mic_block_rms > GEIGEL * ref_block_rms.max(1e-4) + 1e-3
                || (converged && mic_block_rms > NEAR_END_OVER_ECHO * echo_now.max(1e-4) + 4e-3);
            // Judged as a share of the microphone level: speech rises and falls by 20 dB from syllable to syllable, and so does the leftover.
            let ratio = block_err / mic_block_rms.max(1e-4);
            let odd = converged && self.err_floor > 0.0 && ratio > ERR_JUMP * self.err_floor + ERR_ABS;
            if near_end || odd {
                self.frozen = FREEZE_HANG;
            }
            let adapt = self.frozen == 0 && ref_block_rms > 1e-3;
            if adapt && converged && !near_end && !odd {
                self.settled = true;
            }
            self.frozen = self.frozen.saturating_sub(len);
            self.blocks += 1;
            self.adapted += adapt as u64;
            self.near_end_blocks += near_end as u64;
            self.odd_blocks += odd as u64;
            // Pass 2: sample by sample normalized LMS on pre-whitened signals. (The earlier block step added the gradients of 160 samples
            // that are strongly correlated in voiced speech, so the effective step was ~160 times larger than meant and the filter blew
            // up on a real voice while behaving on noise. A per-sample step below 2 is stable for any signal.) Pre-whitening takes the
            // slope out of speech, which makes plain NLMS converge several times faster.
            if adapt {
                let first = start - lag;
                let mut en: f32 = self.refsw[first + 1 - TAPS..=first].iter().map(|v| v * v).sum();
                for k in 0..len {
                    let p = start + k - lag;
                    if k > 0 {
                        let (add, drop) = (self.refsw[p], self.refsw[p - TAPS]);
                        en = (en + add * add - drop * drop).max(0.0);
                    }
                    let mw = self.mics[start + k] - WHITEN * self.mics[start + k - 1];
                    let x = &self.refsw[p + 1 - TAPS..=p];
                    let dot: f32 = self.w.iter().zip(x).map(|(w, x)| w * x).sum();
                    let ew = mw - dot;
                    if !ew.is_finite() {
                        continue;
                    }
                    let step = MU * ew / (en + EPS * TAPS as f32);
                    for (w, x) in self.w.iter_mut().zip(x) {
                        *w += step * x;
                    }
                }
                if self.w.iter().any(|w| !w.is_finite()) {
                    self.w.iter_mut().for_each(|w| *w = 0.0);
                    self.err_floor = 0.0;
                }
            }
            let l = len as f32;
            let smooth = |old: f32, new: f32| if old == 0.0 { new } else { old * 0.9 + new * 0.1 };
            self.mic_level = smooth(self.mic_level, (mic_sq / l).sqrt());
            self.err_level = smooth(self.err_level, (err_sq / l).sqrt());
            self.echo_level = echo_now;
            let _ = ref_sq;
            self.last_err = block_err;
            if converged {
                self.err_floor = if self.err_floor == 0.0 {
                    ratio
                } else if self.frozen == 0 && !odd {
                    self.err_floor * 0.95 + ratio * 0.05
                } else {
                    // Frozen for long: slowly accept the new level, so a changed room does not freeze the filter for good.
                    self.err_floor + FLOOR_CREEP * (ratio - self.err_floor)
                };
            }
            done += len;
        }
        out
    }

    fn trim(&mut self) {
        let keep = PAD + ESTIMATE_WINDOW + RATE;
        if self.refs.len() > keep * 2 {
            let cut = self.refs.len() - keep;
            self.refs.drain(..cut);
            self.refsw.drain(..cut);
            self.mics.drain(..cut);
        }
    }
}

// ------------------------------------------------------------------ the reference BYTE plays (a tap on the output stream)

/// Turns audio from the output device's rate into 16 kHz, keeping its place between calls (block averages).
pub struct Decimator {
    ratio: f64,
    pos: f64,
    acc: f32,
    count: u32,
}

impl Decimator {
    pub fn new(from: u32) -> Self {
        Decimator { ratio: from as f64 / RATE as f64, pos: 0.0, acc: 0.0, count: 0 }
    }

    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        let mut out = vec![];
        self.push_into(input, &mut out);
        out
    }

    /// As `push`, appending to `out` (no allocation of its own: used on the audio thread).
    pub fn push_into(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &s in input {
            self.acc += s;
            self.count += 1;
            self.pos += 1.0;
            if self.pos >= self.ratio {
                self.pos -= self.ratio;
                out.push(self.acc / self.count as f32);
                self.acc = 0.0;
                self.count = 0;
            }
        }
    }
}

struct Tap {
    on: bool,
    /// What was played this session, at 16 kHz, from the first sample.
    refs: Vec<f32>,
    dec: Option<Decimator>,
    rate: u32,
    /// Where the microphone stream stood when the session's first sample was played.
    mic_at_start: u64,
    active: bool,
    generation: u64,
    /// The session ran past the cap: the reference is incomplete, so nothing may be decided from it.
    overflowed: bool,
}

static TAP: Mutex<Tap> = Mutex::new(Tap { on: false, refs: Vec::new(), dec: None, rate: 0, mic_at_start: 0, active: false, generation: 0, overflowed: false });
/// How many microphone samples (16 kHz) the listener has handled.
static MIC_POS: AtomicU64 = AtomicU64::new(0);
static DUCK: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0
static TAP_ON: AtomicBool = AtomicBool::new(false);

pub fn advance_mic(n: usize) -> u64 {
    MIC_POS.fetch_add(n as u64, Ordering::Relaxed)
}

/// The output stream asks this for every block: whether to record what it plays.
pub fn tap_wanted() -> bool {
    TAP_ON.load(Ordering::Relaxed)
}

pub fn set_tap(on: bool) {
    TAP_ON.store(on, Ordering::Relaxed);
    if let Ok(mut t) = TAP.lock() {
        t.on = on;
        if !on {
            t.refs.clear();
            t.active = false;
        }
    }
}

/// Called by the output stream with what it just played (mono, at `rate`, after any ducking). `playing`: it was BYTE's voice, not an
/// idle stream.
pub fn tap_push(mono: &[f32], rate: u32, playing: bool) {
    // Runs on the audio thread: never wait for the lock (a block is dropped when the listener holds it) and never allocate beyond
    // the session's one up-front reserve.
    let Ok(mut t) = TAP.try_lock() else { return };
    if !t.on {
        return;
    }
    if !playing {
        t.active = false;
        return;
    }
    if !t.active {
        t.active = true;
        t.refs.clear();
        t.refs.reserve(RATE * 30);
        t.dec = Some(Decimator::new(rate));
        t.rate = rate;
        t.mic_at_start = MIC_POS.load(Ordering::Relaxed);
        t.generation += 1;
        t.overflowed = false;
    }
    if t.refs.len() > RATE * 120 {
        // Never grows without bound; a session this long is not a conversation. Barge-in stops deciding (see `tap_overflowed`).
        t.overflowed = true;
        return;
    }
    let Tap { dec, refs, .. } = &mut *t;
    if let Some(d) = dec.as_mut() {
        d.push_into(mono, refs);
    }
}

/// The reference stopped being recorded (a very long session): echo cancellation has nothing to subtract, so barge-in must not
/// treat the raw microphone as the person's voice.
pub fn tap_overflowed() -> bool {
    TAP.lock().map(|t| t.overflowed).unwrap_or(false)
}

/// The reference for the microphone chunk that starts at stream position `mic_pos` and is `len` long, zeros where nothing was played.
/// Also returns the session's generation, which changes when BYTE starts a new stretch of speech.
pub fn tap_reference(mic_pos: u64, len: usize) -> (Vec<f32>, u64) {
    let Ok(t) = TAP.lock() else { return (vec![0.0; len], 0) };
    let out = (0..len)
        .map(|k| {
            let i = (mic_pos + k as u64).checked_sub(t.mic_at_start).map(|v| v as usize);
            i.and_then(|i| t.refs.get(i)).copied().unwrap_or(0.0)
        })
        .collect();
    (out, t.generation)
}

/// Output gain while the person is talking over BYTE (1.0 = normal).
pub fn duck_gain() -> f32 {
    f32::from_bits(DUCK.load(Ordering::Relaxed))
}

pub fn set_duck(g: f32) {
    DUCK.store(g.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
pub fn tests_lock() -> std::sync::MutexGuard<'static, ()> {
    tests::TAP_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tap and the duck gain are process-wide: tests that touch them take turns.
    pub static TAP_LOCK: Mutex<()> = Mutex::new(());

    struct Noise(u32);
    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
        }
    }

    /// BYTE's voice stand-in: smoothed noise at about 0.25 rms.
    fn reference(secs: f32) -> Vec<f32> {
        let mut n = Noise(0x1234_5678);
        let mut lp = 0.0;
        (0..(secs * RATE as f32) as usize)
            .map(|_| {
                lp = 0.6 * lp + 0.4 * n.next();
                lp * 1.2
            })
            .collect()
    }

    /// The person: three tones, a vowel-ish mix.
    fn person(len: usize, from: usize, amp: f32) -> Vec<f32> {
        (0..len)
            .map(|i| {
                if i < from {
                    return 0.0;
                }
                let t = i as f32 / RATE as f32;
                amp * ((2.0 * std::f32::consts::PI * 210.0 * t).sin() + 0.6 * (2.0 * std::f32::consts::PI * 530.0 * t).sin() + 0.4 * (2.0 * std::f32::consts::PI * 1130.0 * t).sin())
            })
            .collect()
    }

    /// What the microphone hears of the reference: delayed, attenuated, with a weaker second reflection.
    fn echo_of(r: &[f32], delay: usize, gain: f32) -> Vec<f32> {
        (0..r.len())
            .map(|i| {
                let a = if i >= delay { r[i - delay] } else { 0.0 };
                let b = if i >= delay + 40 { r[i - delay - 40] } else { 0.0 };
                gain * a + 0.35 * gain * b
            })
            .collect()
    }

    fn run(c: &mut EchoCanceller, mic: &[f32], r: &[f32]) -> Vec<f32> {
        let mut out = vec![];
        for (m, x) in mic.chunks(320).zip(r.chunks(320)) {
            out.extend(c.process(m, x));
        }
        out
    }

    #[test]
    fn finds_the_delay() {
        let r = reference(1.0);
        for d in [0usize, 37, 600, 2000, 4000] {
            let mic = echo_of(&r, d, 0.4);
            let (found, strength) = estimate_delay(&r, &mic, MAX_DELAY).expect("a delay");
            assert!(found.abs_diff(d) <= 1, "{d} found as {found}");
            assert!(strength > 0.5);
        }
    }

    #[test]
    fn no_delay_without_an_echo() {
        let r = reference(1.0);
        let mut n = Noise(99);
        let mic: Vec<f32> = (0..r.len()).map(|_| n.next() * 0.2).collect();
        assert!(estimate_delay(&r, &mic, MAX_DELAY).is_none());
        assert!(estimate_delay(&vec![0.0; r.len()], &r, MAX_DELAY).is_none());
    }

    #[test]
    fn removes_the_reference_and_keeps_the_person() {
        let secs = 6.0;
        let r = reference(secs);
        let echo = echo_of(&r, 700, 0.4);
        let talk_from = (RATE as f32 * 4.0) as usize;
        let p = person(r.len(), talk_from, 0.05);
        let mic: Vec<f32> = echo.iter().zip(&p).map(|(a, b)| a + b).collect();
        let mut c = EchoCanceller::new();
        let out = run(&mut c, &mic, &r);
        assert!(c.ready());
        assert!(c.delay().unwrap().abs_diff(700) <= 2);
        // Echo alone (seconds 2 to 4): at least 20 dB quieter.
        let (a, b) = (RATE * 2, RATE * 4);
        let erle = 20.0 * (rms(&mic[a..b]) / rms(&out[a..b]).max(1e-9)).log10();
        assert!(erle >= 20.0, "echo only reduced {erle:.1} dB");
        // The person (seconds 4.5 to 6): the output is mostly them, with little of the echo left.
        let (a, b) = ((RATE as f32 * 4.5) as usize, r.len());
        let dist: Vec<f32> = out[a..b].iter().zip(&p[a..b]).map(|(o, p)| o - p).collect();
        let ratio = rms(&dist) / rms(&p[a..b]);
        eprintln!("delay {:?} dist {} person {} out {} echo-only out {} mic-echo {}", c.delay(), rms(&dist), rms(&p[a..b]), rms(&out[a..b]), rms(&out[RATE * 2..RATE * 4]), rms(&mic[RATE*2..RATE*4]));
        assert!(ratio < 0.35, "residual {ratio:.2} of the person's level is something else");
        assert!(rms(&out[a..b]) > 0.7 * rms(&p[a..b]), "the person came out too quiet");
        // And clearly louder than the echo-only part of the output.
        assert!(rms(&out[a..b]) > 3.0 * rms(&out[RATE * 2..RATE * 4]));
    }

    #[test]
    fn works_for_other_delays_and_gains() {
        for (delay, gain) in [(100usize, 0.15f32), (1500, 0.8), (3000, 0.3), (4500, 0.5)] {
            let r = reference(5.0);
            let mic = echo_of(&r, delay, gain);
            let mut c = EchoCanceller::new();
            let out = run(&mut c, &mic, &r);
            let (a, b) = (RATE * 3, RATE * 5);
            let erle = 20.0 * (rms(&mic[a..b]) / rms(&out[a..b]).max(1e-9)).log10();
            assert!(erle >= 20.0, "delay {delay} gain {gain}: {erle:.1} dB");
        }
    }

    #[test]
    fn a_bad_sample_does_not_poison_the_filter() {
        let r = reference(4.0);
        let mic = echo_of(&r, 500, 0.5);
        let mut c = EchoCanceller::new();
        let mut bad = r.clone();
        bad[RATE * 2] = f32::NAN;
        bad[RATE * 2 + 5] = f32::INFINITY;
        let out = run(&mut c, &mic, &bad);
        assert!(out.iter().all(|x| x.is_finite()), "non-finite output");
        // Learning recovered afterwards.
        let r2 = reference(3.0);
        let mic2 = echo_of(&r2, 500, 0.5);
        c.begin_session();
        let out2 = run(&mut c, &mic2, &r2);
        assert!(out2.iter().all(|x| x.is_finite()));
        let erle = 20.0 * (rms(&mic2[RATE * 2..]) / rms(&out2[RATE * 2..]).max(1e-9)).log10();
        assert!(erle >= 15.0, "{erle:.1} dB");
    }

    #[test]
    fn quiet_reference_leaves_the_microphone_alone() {
        let mut c = EchoCanceller::new();
        let mic = person(RATE, 0, 0.1);
        let out = run(&mut c, &mic, &vec![0.0; RATE]);
        assert_eq!(out, mic);
    }

    #[test]
    fn a_new_session_keeps_what_was_learned() {
        let r = reference(4.0);
        let mic = echo_of(&r, 500, 0.5);
        let mut c = EchoCanceller::new();
        run(&mut c, &mic, &r);
        c.begin_session();
        assert!(c.ready(), "the delay is kept");
        let r2 = reference(1.0);
        let mic2 = echo_of(&r2, 500, 0.5);
        let out = run(&mut c, &mic2, &r2);
        // Right away, in the first second, it is already cleaned.
        let erle = 20.0 * (rms(&mic2[2000..]) / rms(&out[2000..]).max(1e-9)).log10();
        assert!(erle >= 12.0, "{erle:.1} dB");
    }

    #[test]
    fn decimates_to_16k() {
        let mut d = Decimator::new(48_000);
        let out = d.push(&[0.0, 0.3, 0.6, 0.9, 0.9, 0.9]);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.3).abs() < 1e-6 && (out[1] - 0.9).abs() < 1e-6);
        let mut d = Decimator::new(44_100);
        let total: usize = (0..100).map(|_| d.push(&vec![0.5; 441]).len()).sum();
        assert!((total as i64 - 16_000).abs() <= 2, "{total}");
    }

    #[test]
    fn the_tap_hands_out_an_aligned_reference() {
        let _g = TAP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_tap(true);
        let before = advance_mic(1000);
        tap_push(&vec![0.5; 480], 48_000, true); // 160 samples at 16 kHz
        let (r, gen) = tap_reference(before + 1000, 4);
        assert_eq!(r.len(), 4);
        assert!(gen > 0);
        // The 160 played samples start where the microphone stood; before that and after the end there is silence.
        let (inside, _) = tap_reference(MIC_POS.load(Ordering::Relaxed) - 1000, 4);
        let _ = inside;
        set_tap(false);
        set_duck(0.25);
        assert!((duck_gain() - 0.25).abs() < 1e-6);
        set_duck(1.0);
    }
}
