//! A cheap check before whisper runs on a speech burst (wake.rs). Whisper costs a noticeable slice of CPU every time, and a room has
//! many sounds that cannot be "Hey BYTE": a keyboard, a door, a fan, music holding one note, a jingle that plays again and again.
//!
//! The rule that matters: a missed "Hey BYTE" is worse than an extra whisper run, so a burst is skipped only when it clearly cannot be
//! the phrase. Whispered or breathy speech has no pitch and must still pass, so "no pitch" alone never rejects. Everything here is
//! pure (no audio device, no model), measured on synthetic bursts in the tests below. No new model is involved.

use std::sync::atomic::{AtomicU64, Ordering};

const RATE: usize = 16_000;
/// Analysis frame: 30 ms.
const FRAME: usize = RATE * 30 / 1000;
/// "Hey BYTE" is never shorter than this (spoken part, ms); wake.rs already cuts at 2.5 s.
pub const MIN_SPAN_MS: usize = 250;
pub const MAX_SPAN_MS: usize = 2_600;
/// A burst this long or longer whose loudness hardly varies is a tone, a fan or held music (speech rises and falls with every syllable).
pub const STEADY_MIN_MS: usize = 600;
/// Variation of the loudness (standard deviation / mean over the spoken part) below which a long burst is "steady". Speech measures
/// 0.4 and more; this leaves a wide margin.
pub const STEADY_CV: f32 = 0.12;
/// A burst with no pitched sound at all is dropped only when it is also this impulsive (peak / rms): a keyboard or a door, not a whisper.
pub const IMPULSE_CREST: f32 = 6.0;
/// A frame is "pitched" when its autocorrelation at the best lag between 70 and 400 Hz is at least this.
pub const VOICED_AUTOCORR: f32 = 0.35;
/// A burst that came out of whisper as "not the phrase" is not checked again for this long when the next one has the same shape.
pub const COOLDOWN_SECS: f64 = 20.0;
/// How alike (correlation of the loudness curve) two bursts must be to count as the same sound, and how close in loudness.
pub const SAME_SHAPE: f32 = 0.985;
pub const SAME_LEVEL: f32 = 0.08;

static CHECKED: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    TooShort,
    TooLong,
    Steady,
    ImpulseNoVoice,
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn pitched(frame: &[f32]) -> bool {
    let energy: f32 = frame.iter().map(|x| x * x).sum();
    if energy < 1e-6 {
        return false;
    }
    let (lo, hi) = (RATE / 400, (RATE / 70).min(frame.len() - 1));
    (lo..=hi).any(|lag| frame[..frame.len() - lag].iter().zip(&frame[lag..]).map(|(a, b)| a * b).sum::<f32>() / energy >= VOICED_AUTOCORR)
}

/// What the burst looks like: its spoken part (frames well above the burst's peak), how much its loudness varies, whether anything in
/// it is pitched, and how impulsive it is.
struct Look {
    span_ms: usize,
    cv: f32,
    voiced_frames: usize,
    crest: f32,
    /// Frame loudness over the spoken part.
    levels: Vec<f32>,
}

fn look(burst: &[f32]) -> Option<Look> {
    let frames: Vec<&[f32]> = burst.chunks_exact(FRAME).collect();
    let rms_all: Vec<f32> = frames.iter().map(|f| rms(f)).collect();
    let peak = rms_all.iter().copied().fold(0.0, f32::max);
    if peak < 1e-4 {
        return None;
    }
    let loud = |i: usize| rms_all[i] > 0.2 * peak;
    let first = (0..frames.len()).find(|&i| loud(i))?;
    let last = (0..frames.len()).rev().find(|&i| loud(i))?;
    let levels: Vec<f32> = rms_all[first..=last].to_vec();
    let mean = levels.iter().sum::<f32>() / levels.len() as f32;
    let var = levels.iter().map(|l| (l - mean) * (l - mean)).sum::<f32>() / levels.len() as f32;
    let voiced_frames = (first..=last).filter(|&i| loud(i) && pitched(frames[i])).count();
    let spoken = &burst[first * FRAME..(last + 1) * FRAME];
    let top = spoken.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    Some(Look { span_ms: (last + 1 - first) * 30, cv: var.sqrt() / mean.max(1e-9), voiced_frames, crest: top / rms(spoken).max(1e-9), levels })
}

/// Why a burst cannot be the wake phrase, if it cannot. None: worth a whisper run.
pub fn rejection(burst: &[f32]) -> Option<Reason> {
    // Anything that cannot be analysed is passed on: when in doubt, whisper looks.
    let l = look(burst)?;
    if l.span_ms < MIN_SPAN_MS {
        return Some(Reason::TooShort);
    }
    if l.span_ms > MAX_SPAN_MS {
        return Some(Reason::TooLong);
    }
    if l.span_ms >= STEADY_MIN_MS && l.cv < STEADY_CV {
        return Some(Reason::Steady);
    }
    if l.voiced_frames == 0 && l.crest > IMPULSE_CREST {
        return Some(Reason::ImpulseNoVoice);
    }
    None
}

/// Whether whisper should be run on this burst. Counts what it skips (see `counts`) and logs it at debug level only.
pub fn worth_checking(burst: &[f32]) -> bool {
    CHECKED.fetch_add(1, Ordering::Relaxed);
    match rejection(burst) {
        None => true,
        Some(r) => {
            let skipped = SKIPPED.fetch_add(1, Ordering::Relaxed) + 1;
            log::debug!("wake pre-check: skipped a burst ({r:?}); {skipped} of {} skipped so far", CHECKED.load(Ordering::Relaxed));
            false
        }
    }
}

/// (bursts looked at, bursts skipped) since start.
pub fn counts() -> (u64, u64) {
    (CHECKED.load(Ordering::Relaxed), SKIPPED.load(Ordering::Relaxed))
}

// ------------------------------------------------------------------ the same sound again

/// A coarse fingerprint of a burst's loudness curve: 16 points, unit length, and its mean level.
#[derive(Clone)]
struct Shape {
    env: [f32; 16],
    level: f32,
}

fn shape_of(burst: &[f32]) -> Option<Shape> {
    let l = look(burst)?;
    let mut env = [0f32; 16];
    for (i, e) in env.iter_mut().enumerate() {
        let (a, b) = (i * l.levels.len() / 16, ((i + 1) * l.levels.len() / 16).max(i * l.levels.len() / 16 + 1).min(l.levels.len()));
        *e = l.levels[a.min(l.levels.len() - 1)..b].iter().sum::<f32>() / (b - a.min(b - 1)) as f32;
    }
    let level = l.levels.iter().sum::<f32>() / l.levels.len() as f32;
    let n = env.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
    env.iter_mut().for_each(|v| *v /= n);
    Some(Shape { env, level })
}

/// Remembers bursts whisper already said were not the phrase, so a sound that repeats exactly (a ringtone, a jingle, a looping video)
/// is not sent to whisper again. A person repeating "Hey BYTE" after a miss never repeats it that exactly, so retries are never held back.
#[derive(Default)]
pub struct Cooldown {
    seen: Vec<(Shape, f64)>,
}

impl Cooldown {
    /// whisper looked at this burst and it was not the phrase.
    pub fn rejected(&mut self, burst: &[f32], now: f64) {
        if let Some(s) = shape_of(burst) {
            self.seen.retain(|(_, t)| now - t < COOLDOWN_SECS);
            self.seen.push((s, now));
            if self.seen.len() > 4 {
                self.seen.remove(0);
            }
        }
    }

    /// This burst is the same sound as one whisper rejected moments ago.
    pub fn same_as_rejected(&self, burst: &[f32], now: f64) -> bool {
        let Some(s) = shape_of(burst) else { return false };
        self.seen.iter().any(|(o, t)| {
            now - t < COOLDOWN_SECS && o.env.iter().zip(&s.env).map(|(a, b)| a * b).sum::<f32>() >= SAME_SHAPE && (o.level - s.level).abs() <= SAME_LEVEL * o.level.max(s.level)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u32);
    impl Lcg {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
            (self.0 >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        }
    }

    fn quiet(ms: usize) -> Vec<f32> {
        vec![0.0005; RATE * ms / 1000]
    }

    /// Speech-like: a harmonic voice (f0 around 130 Hz, formants) with syllable-rate loudness changes, like "hey BYTE".
    fn speech(ms: usize, f0: f32, rhythm: f32) -> Vec<f32> {
        (0..RATE * ms / 1000)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let env = 0.15 + 0.85 * (0.5 + 0.5 * (2.0 * std::f32::consts::PI * rhythm * t).sin()).powi(2);
                let mut s = 0.0;
                for h in 1..=20 {
                    let f = f0 * (1.0 + 0.03 * (t * 4.0).sin()) * h as f32;
                    let formant = (-((f - 600.0) / 300.0).powi(2)).exp() + 0.6 * (-((f - 1800.0) / 400.0).powi(2)).exp() + 0.05;
                    s += formant * (2.0 * std::f32::consts::PI * f * t).sin();
                }
                0.12 * env * s
            })
            .collect()
    }

    /// Wraps a sound in the quiet the detector leaves around a burst.
    fn burst(sound: Vec<f32>) -> Vec<f32> {
        let mut b = quiet(200);
        b.extend(sound);
        b.extend(quiet(300));
        b
    }

    fn noise(ms: usize, amp: f32, seed: u32) -> Vec<f32> {
        let mut r = Lcg(seed);
        (0..RATE * ms / 1000).map(|_| r.next() * amp).collect()
    }

    #[test]
    fn plausible_speech_is_checked() {
        for (ms, f0, rhythm) in [(600, 130.0, 3.5), (800, 110.0, 4.0), (1200, 200.0, 3.0), (2000, 160.0, 2.5), (450, 250.0, 5.0)] {
            assert_eq!(rejection(&burst(speech(ms, f0, rhythm))), None, "{ms} ms f0 {f0}");
            assert!(worth_checking(&burst(speech(ms, f0, rhythm))));
        }
    }

    #[test]
    fn whispered_and_breathy_speech_is_still_checked() {
        // Noise that rises and falls like syllables: no pitch at all, but not impulsive and not steady.
        let whisper: Vec<f32> = noise(900, 0.1, 7).iter().enumerate().map(|(i, n)| n * (0.2 + 0.8 * (0.5 + 0.5 * (i as f32 * 2.0 * std::f32::consts::PI * 3.0 / RATE as f32).sin()).powi(2))).collect();
        assert_eq!(rejection(&burst(whisper)), None);
    }

    #[test]
    fn a_very_short_or_long_burst_cannot_be_the_phrase() {
        assert_eq!(rejection(&burst(speech(120, 130.0, 3.5))), Some(Reason::TooShort));
        assert_eq!(rejection(&burst(speech(3500, 130.0, 3.5))), Some(Reason::TooLong));
    }

    #[test]
    fn steady_sounds_are_skipped() {
        let tone: Vec<f32> = (0..RATE).map(|i| (i as f32 * 0.07).sin() * 0.2).collect();
        assert_eq!(rejection(&burst(tone)), Some(Reason::Steady));
        // A fan: steady noise.
        assert_eq!(rejection(&burst(noise(1500, 0.05, 3))), Some(Reason::Steady));
        // Music holding a chord.
        let chord: Vec<f32> = (0..RATE * 2).map(|i| 0.1 * ((i as f32 * 0.05).sin() + (i as f32 * 0.063).sin() + (i as f32 * 0.075).sin())).collect();
        assert_eq!(rejection(&burst(chord)), Some(Reason::Steady));
    }

    #[test]
    fn clacks_and_thuds_without_a_voice_are_skipped() {
        // Three keyboard clacks 150 ms apart: short decaying noise.
        let mut keys = vec![];
        let mut r = Lcg(11);
        for _ in 0..3 {
            keys.extend((0..RATE * 25 / 1000).map(|i| r.next() * 0.5 * (-(i as f32) / 120.0).exp()));
            keys.extend(vec![0.0005; RATE * 125 / 1000]);
        }
        assert_eq!(rejection(&burst(keys)), Some(Reason::ImpulseNoVoice));
        // A door: one low thud with a long decay.
        let door: Vec<f32> = (0..RATE * 4 / 10).map(|i| (i as f32 * 0.02).sin() * 0.6 * (-(i as f32) / 1500.0).exp()).collect();
        assert_ne!(rejection(&burst(door)), None);
    }

    #[test]
    fn silence_and_unreadable_audio_are_passed_on() {
        assert_eq!(rejection(&[]), None);
        assert_eq!(rejection(&vec![0.0; RATE]), None);
        // A faint steady hiss is a steady sound like any other.
        assert_eq!(rejection(&quiet(1000)), Some(Reason::Steady));
    }

    #[test]
    fn the_same_sound_again_is_held_back_but_a_retry_is_not() {
        let jingle = burst(speech(900, 140.0, 3.0));
        let mut c = Cooldown::default();
        assert!(!c.same_as_rejected(&jingle, 0.0));
        c.rejected(&jingle, 0.0);
        assert!(c.same_as_rejected(&jingle, 5.0), "an exact repeat");
        assert!(!c.same_as_rejected(&jingle, COOLDOWN_SECS + 1.0), "after the cooldown");
        // A person saying it again: another pace and pitch.
        assert!(!c.same_as_rejected(&burst(speech(800, 150.0, 4.2)), 5.0));
        assert!(!c.same_as_rejected(&burst(speech(1100, 140.0, 2.4)), 5.0));
        // Louder or quieter is not the same sound either.
        let loud: Vec<f32> = jingle.iter().map(|x| x * 2.0).collect();
        assert!(!c.same_as_rejected(&loud, 5.0));
    }

    #[test]
    fn counts_go_up() {
        let (c0, s0) = counts();
        let _ = worth_checking(&burst(noise(1500, 0.05, 5)));
        let (c1, s1) = counts();
        assert!(c1 > c0 && s1 > s0);
    }

    #[test]
    fn it_is_cheap() {
        let b = burst(speech(2500, 130.0, 3.5));
        let t = std::time::Instant::now();
        for _ in 0..5 {
            let _ = rejection(&b);
        }
        // Debug build, five runs: far below one whisper run (hundreds of ms).
        assert!(t.elapsed().as_millis() < 1500, "{:?}", t.elapsed());
    }
}
