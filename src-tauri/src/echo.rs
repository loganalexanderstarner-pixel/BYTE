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
pub const TAPS: usize = 512;
/// The filter starts this far (samples) before the estimated delay, so a delay that is a little off still falls inside it.
pub const PRE: usize = 128;
/// The longest delay between "played" and "heard" that is searched for: 300 ms.
pub const MAX_DELAY: usize = RATE * 3 / 10;
/// Block-NLMS step size (one step per 10 ms block, normalized by the block's input energy): larger converges faster and leaves more
/// noise behind; much above 60 becomes unstable for a coloured reference.
pub const MU: f32 = 30.0;
/// How much audio the delay estimate looks at: 0.5 s.
pub const ESTIMATE_WINDOW: usize = RATE / 2;
/// The delay is re-checked this often while it runs: 1 s.
pub const REESTIMATE_EVERY: usize = RATE;
/// A new delay estimate replaces the old one only when it differs by more than this (samples).
pub const DELAY_JUMP: usize = 48;
/// The correlation peak (0..1) a delay estimate needs to be trusted.
pub const MIN_CORRELATION: f32 = 0.15;
/// Double talk before the filter has converged: the microphone is this many times louder than the reference (rms) over a block. Loose on
/// purpose: a real microphone often hears the echo louder than the digital reference, so a tight ratio would never let the filter learn.
pub const GEIGEL: f32 = 3.0;
/// Double talk, once the filter has converged: the microphone is this many times louder than the echo the filter predicts.
pub const NEAR_END_OVER_ECHO: f32 = 1.8;
/// After a block that looked like double talk the filter stays frozen this long (samples).
pub const FREEZE_HANG: usize = RATE * 15 / 100;
/// Double talk, learned: the leftover is this many times (plus `ERR_ABS`) what echo-only blocks left.
pub const ERR_JUMP: f32 = 3.0;
pub const ERR_ABS: f32 = 0.004;
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

fn decimate4(x: &[f32]) -> Vec<f32> {
    x.chunks_exact(4).map(|c| c.iter().sum::<f32>() / 4.0).collect()
}

/// How many samples later the reference shows up in the microphone, and how strong that match is (0..1). `reference[i]` and `mic[i]` are
/// the same moment on the two streams. None when nothing matches well enough (silence, or no echo).
pub fn estimate_delay(reference: &[f32], mic: &[f32], max_delay: usize) -> Option<(usize, f32)> {
    let len = reference.len().min(mic.len());
    if len < 2 * max_delay.min(len / 2).max(64) {
        return None;
    }
    let (reference, mic) = (&reference[reference.len() - len..], &mic[mic.len() - len..]);
    if rms(reference) < 1e-3 || rms(mic) < 1e-4 {
        return None;
    }
    let corr = |r: &[f32], m: &[f32], d: usize| -> f32 {
        if d >= m.len() {
            return 0.0;
        }
        let (mm, rr) = (&m[d..], &r[..m.len() - d]);
        let (mut xy, mut xx, mut yy) = (0f32, 0f32, 0f32);
        for (a, b) in mm.iter().zip(rr) {
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        xy / (xx * yy).sqrt().max(1e-9)
    };
    // Coarse: every 4th sample's average.
    let (r4, m4) = (decimate4(reference), decimate4(mic));
    let mut best = (0usize, f32::MIN);
    for d in 0..=(max_delay / 4) {
        let c = corr(&r4, &m4, d);
        if c > best.1 {
            best = (d, c);
        }
    }
    // Fine: around the coarse peak, at the full rate.
    let centre = best.0 * 4;
    let (lo, hi) = (centre.saturating_sub(6), (centre + 6).min(max_delay));
    let mut fine = (centre, f32::MIN);
    for d in lo..=hi {
        let c = corr(reference, mic, d);
        if c > fine.1 {
            fine = (d, c);
        }
    }
    (fine.1 >= MIN_CORRELATION).then_some(fine)
}

/// Subtracts what BYTE plays from what the microphone hears.
pub struct EchoCanceller {
    w: Vec<f32>,
    /// Reference and microphone since the session began, with `PAD` zeros in front so every filter position is valid.
    refs: Vec<f32>,
    mics: Vec<f32>,
    /// The delay in use (samples), once found.
    delay: Option<usize>,
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
        EchoCanceller { w: vec![0.0; TAPS], refs: vec![0.0; PAD], mics: vec![0.0; PAD], delay: None, since_estimate: 0, frozen: 0, mic_level: 0.0, err_level: 0.0, echo_level: 0.0, err_floor: 0.0, last_err: 0.0 }
    }
}

impl EchoCanceller {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new stretch of speech starts. The learned filter is kept (the room did not change); the stream positions begin again.
    pub fn begin_session(&mut self) {
        self.refs = vec![0.0; PAD];
        self.mics = vec![0.0; PAD];
        self.since_estimate = 0;
        self.frozen = 0;
    }

    /// The delay in use, in samples (None until it has been found).
    pub fn delay(&self) -> Option<usize> {
        self.delay
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
        self.refs.extend_from_slice(&reference[..n]);
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
                self.delay = Some(d);
                self.w.iter_mut().for_each(|x| *x = 0.0);
                self.err_floor = 0.0;
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
                // w[i] belongs to the reference i samples before p.
                let y: f32 = self.w.iter().zip(x.iter().rev()).map(|(w, x)| w * x).sum();
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
                || (converged && mic_block_rms > NEAR_END_OVER_ECHO * echo_now.max(1e-4) + 2e-3);
            let odd = converged && self.err_floor > 0.0 && block_err > ERR_JUMP * self.err_floor + ERR_ABS;
            if near_end || odd {
                self.frozen = FREEZE_HANG;
            }
            let adapt = self.frozen == 0 && ref_block_rms > 1e-3;
            self.frozen = self.frozen.saturating_sub(len);
            // Pass 2: one normalized step for the whole block.
            if adapt {
                let mut grad = vec![0f32; TAPS];
                for k in 0..len {
                    let p = start + k - lag;
                    let x = &self.refs[p + 1 - TAPS..=p];
                    for (g, x) in grad.iter_mut().zip(x.iter().rev()) {
                        *g += es[k] * x;
                    }
                }
                let step = MU / (energy_sum + EPS * TAPS as f32 * len as f32);
                for (w, g) in self.w.iter_mut().zip(&grad) {
                    *w += step * g;
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
                    block_err
                } else if self.frozen == 0 && !odd {
                    self.err_floor * 0.95 + block_err * 0.05
                } else {
                    // Frozen for long: slowly accept the new level, so a changed room does not freeze the filter for good.
                    self.err_floor + FLOOR_CREEP * (block_err - self.err_floor)
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
