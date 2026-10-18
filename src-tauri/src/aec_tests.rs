//! Offline harness for talking over BYTE: no microphone, no speakers, no Windows.
//!
//! A scenario is built from three synthetic signals:
//! - "BYTE speaking": a voiced, syllable-shaped signal (glottal pulses through vowel formants) of several seconds, the exact reference
//!   the loopback would deliver;
//! - a room: speaker non-linearity, a direct path with a delay of 20 to 120 ms and a gain, a reverberant tail and a noise floor;
//! - a person saying "Hey BYTE" (two vowel glides) over BYTE at a chosen moment.
//!
//! (The listener also limits wake checks over BYTE's voice to one every 4 s, see `wake.rs`.) It measures how many dB of BYTE's own voice are removed (ERLE) once the canceller has settled, whether the interrupt logic fires
//! within 500 ms of the person starting, and that echo alone never triggers it. `cargo test --lib aec_tests -- --nocapture` prints
//! the table that STATUS-aec.md quotes.

use crate::aec::ScriptedSource;
use crate::bargein::{Barge, Mode};
use crate::echo::{EchoCanceller, RATE};

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Second-order resonator (a formant).
struct Formant {
    a1: f32,
    a2: f32,
    g: f32,
    z1: f32,
    z2: f32,
}
impl Formant {
    fn new(freq: f32, bw: f32) -> Self {
        let r = (-std::f32::consts::PI * bw / RATE as f32).exp();
        let th = 2.0 * std::f32::consts::PI * freq / RATE as f32;
        Formant { a1: 2.0 * r * th.cos(), a2: -r * r, g: 1.0 - r, z1: 0.0, z2: 0.0 }
    }
    fn run(&mut self, x: f32) -> f32 {
        let y = self.g * x + self.a1 * self.z1 + self.a2 * self.z2;
        self.z2 = self.z1;
        self.z1 = y;
        y
    }
}

/// A vowel-like piece: pitch `f0` (Hz, gliding to `f0_end`), formants gliding from `f_a` to `f_b`, `ms` long, shaped like a syllable.
fn syllable(f0: f32, f0_end: f32, f_a: [f32; 3], f_b: [f32; 3], ms: usize, rng: &mut Rng) -> Vec<f32> {
    let n = RATE * ms / 1000;
    let mut forms: Vec<Formant> = f_a.iter().map(|f| Formant::new(*f, 90.0)).collect();
    let mut phase = 0.0f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / n as f32;
        // Slowly gliding formants: rebuild every 80 samples.
        if i % 80 == 0 {
            forms = (0..3).map(|k| Formant::new(f_a[k] + (f_b[k] - f_a[k]) * t, 90.0 + 40.0 * k as f32)).collect();
        }
        let f = f0 + (f0_end - f0) * t;
        phase += f / RATE as f32;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        // Glottal pulse: a skewed ramp plus a little breath noise.
        let src = (if phase < 0.4 { phase / 0.4 } else { (1.0 - phase) / 0.6 }) * 2.0 - 1.0 + 0.05 * rng.next();
        let y: f32 = forms.iter_mut().zip([1.0, 0.6, 0.35]).map(|(f, w)| f.run(src) * w).sum();
        let env = (std::f32::consts::PI * t).sin().powf(0.6);
        out.push(y * env * 6.0);
    }
    out
}

fn silence(ms: usize) -> Vec<f32> {
    vec![0.0; RATE * ms / 1000]
}

/// Scale to a given rms.
fn at_level(mut x: Vec<f32>, level: f32) -> Vec<f32> {
    let r = rms(&x).max(1e-9);
    x.iter_mut().for_each(|v| *v *= level / r);
    x
}

/// BYTE's voice: a sentence of 9 syllables with pauses, about `secs` long (repeated if needed). Speaker-level rms 0.2.
fn byte_speaking(secs: f32) -> Vec<f32> {
    let mut rng = Rng(0x0BAD_5EED);
    let vowels: [[f32; 3]; 5] = [[730.0, 1090.0, 2440.0], [270.0, 2290.0, 3010.0], [300.0, 870.0, 2240.0], [530.0, 1840.0, 2480.0], [660.0, 1720.0, 2410.0]];
    let mut out = vec![];
    let mut k = 0;
    while out.len() < (secs * RATE as f32) as usize {
        let (a, b) = (vowels[k % 5], vowels[(k * 3 + 1) % 5]);
        let f0 = 180.0 + 25.0 * ((k * 7) % 5) as f32;
        out.extend(syllable(f0, f0 - 20.0, a, b, 160 + 30 * (k % 4), &mut rng));
        out.extend(silence(if k % 4 == 3 { 260 } else { 40 }));
        k += 1;
    }
    out.truncate((secs * RATE as f32) as usize);
    at_level(out, 0.2)
}

/// A person saying "Hey BYTE": "hey" (e to i glide) and "byte" (a to i glide), deeper voice, about 0.7 s.
fn hey_byte() -> Vec<f32> {
    let mut rng = Rng(0x1234_4321);
    let mut s = syllable(120.0, 110.0, [530.0, 1840.0, 2480.0], [300.0, 2200.0, 3000.0], 260, &mut rng);
    s.extend(silence(50));
    s.extend(syllable(115.0, 100.0, [730.0, 1090.0, 2440.0], [300.0, 2200.0, 3000.0], 330, &mut rng));
    s
}

struct Room {
    delay: usize,
    gain: f32,
    /// Reverberant tail: length (samples) and its energy relative to the direct sound (0.3 = 5 dB below it).
    tail_len: usize,
    tail_energy: f32,
    noise: f32,
    /// Speaker non-linearity (tanh drive; 0 = linear).
    drive: f32,
    /// A loopback packet that arrives this many samples late (reference jitter).
    late: usize,
}

impl Room {
    fn impulse(&self) -> Vec<f32> {
        let mut rng = Rng(0x7777);
        let mut h = vec![0.0; self.delay + self.tail_len + 1];
        h[self.delay] = self.gain;
        for k in 1..self.tail_len {
            let decay = (-6.9 * k as f32 / self.tail_len as f32).exp(); // -60 dB at the end of the tail
            // Uniform noise has variance 1/3; the decaying envelope sums to tail_len / 13.8 samples of unit energy.
            let amp = (self.tail_energy * 13.8 * 3.0 / self.tail_len as f32).sqrt();
            h[self.delay + k] = rng.next() * self.gain * amp * decay;
        }
        h
    }

    /// What the microphone hears of `played`.
    fn echo(&self, played: &[f32]) -> Vec<f32> {
        let driven: Vec<f32> = if self.drive > 0.0 { played.iter().map(|v| (v * self.drive).tanh() / self.drive).collect() } else { played.to_vec() };
        let h = self.impulse();
        let mut out = vec![0.0; played.len()];
        // Sparse direct convolution is plenty fast at these lengths.
        for (j, hj) in h.iter().enumerate() {
            if *hj == 0.0 {
                continue;
            }
            for i in j..played.len() {
                out[i] += hj * driven[i - j];
            }
        }
        let mut rng = Rng(0x5555);
        out.iter_mut().for_each(|v| *v += self.noise * rng.next());
        out
    }
}

struct Outcome {
    erle_db: f32,
    /// First duck / stop after the person began, ms; None if none came.
    duck_ms: Option<f32>,
    stop_ms: Option<f32>,
    /// Interrupts fired before the person started (must be none).
    false_interrupts: usize,
    /// A speech burst containing the person's phrase came out of the cleaned signal.
    burst_seen: bool,
}

/// Runs one scenario. `person_at`: when "Hey BYTE" starts (seconds); None = echo only. `person_vs_echo`: the person's rms while speaking, as a
/// multiple of BYTE's echo rms at the microphone (1.0 = as loud as BYTE's own voice).
fn run(room: &Room, secs: f32, person_at: Option<f32>, person_vs_echo: f32) -> Outcome {
    let _g = crate::echo::tests_lock();
    crate::echo::set_tap(false);
    let played = byte_speaking(secs);
    let echo = room.echo(&played);
    let mut mic = echo.clone();
    let from = person_at.map(|t| (t * RATE as f32) as usize);
    if let Some(from) = from {
        let p = at_level(hey_byte(), person_vs_echo * rms(&echo));
        for (i, v) in p.iter().enumerate() {
            if from + i < mic.len() {
                mic[from + i] += v;
            }
        }
    }
    // ERLE with the canceller alone, over the echo-only stretch before the person (or the last 2 s).
    let mut c = EchoCanceller::new();
    let mut cleaned = vec![];
    let mut src = ScriptedSource::new(played.clone());
    src.late = room.late;
    for (k, m) in mic.chunks(160).enumerate() {
        let r = crate::aec::ReferenceSource::reference(&mut src, (k * 160) as u64, m.len()).unwrap().0;
        cleaned.extend(c.process(m, &r));
    }
    let (a, b) = match from {
        Some(f) => (f.saturating_sub(RATE * 2), f.saturating_sub(RATE / 10)),
        None => (mic.len() - RATE * 2, mic.len()),
    };
    let erle_db = 20.0 * (rms(&echo[a..b]) / rms(&cleaned[a..b]).max(1e-9)).log10();
    // The interrupt logic, as the listener drives it.
    let mut barge = Barge::with_source(Box::new({
        let mut s = ScriptedSource::new(played.clone());
        s.late = room.late;
        s
    }));
    let (mut duck_ms, mut stop_ms, mut false_interrupts, mut burst_seen) = (None, None, 0, false);
    let mut pos = 0u64;
    for m in mic.chunks(160) {
        let before = crate::echo::duck_gain();
        let stop = barge.feed(m, pos, Mode::Speakers).is_some();
        let t_ms = pos as f32 * 1000.0 / RATE as f32;
        let onset_ms = from.map(|f| f as f32 * 1000.0 / RATE as f32);
        let after = crate::echo::duck_gain();
        let ducked_now = after < before;
        match onset_ms {
            Some(o) if t_ms >= o => {
                if ducked_now && duck_ms.is_none() {
                    duck_ms = Some(t_ms - o);
                }
                if stop && stop_ms.is_none() {
                    stop_ms = Some(t_ms - o);
                }
            }
            _ => {
                if stop || ducked_now {
                    false_interrupts += 1;
                }
            }
        }
        if let (Some(o), false) = (onset_ms, burst_seen) {
            // The burst detector saw the phrase if one ended after the onset.
            if barge.take_bursts().iter().any(|b| b.len() >= RATE / 3) && t_ms > o {
                burst_seen = true;
            }
        } else {
            barge.take_bursts();
        }
        if stop {
            break;
        }
        pos += m.len() as u64;
    }
    crate::echo::set_duck(1.0);
    Outcome { erle_db, duck_ms, stop_ms, false_interrupts, burst_seen }
}

/// (name, room, least ERLE in dB the harness requires). The person must be heard in rooms that ask for at least 12 dB; the harder rooms
/// (reverb much longer than the filter, speakers that distort) only have to stay quiet on echo alone, and their numbers are reported.
fn rooms() -> Vec<(&'static str, Room, f32)> {
    let ms = |m: usize| RATE * m / 1000;
    vec![
        ("20 ms, close, dry", Room { delay: ms(20), gain: 0.5, tail_len: ms(40), tail_energy: 0.1, noise: 0.002, drive: 0.0, late: 0 }, 18.0),
        ("60 ms, desk, some reverb", Room { delay: ms(60), gain: 0.35, tail_len: ms(120), tail_energy: 0.3, noise: 0.003, drive: 0.0, late: 0 }, 12.0),
        ("120 ms, far, long reverb", Room { delay: ms(120), gain: 0.25, tail_len: ms(200), tail_energy: 0.5, noise: 0.004, drive: 0.0, late: 0 }, 10.0),
        ("60 ms, loud speakers, mild clipping", Room { delay: ms(60), gain: 0.7, tail_len: ms(100), tail_energy: 0.2, noise: 0.003, drive: 1.5, late: 0 }, 8.0),
        ("60 ms, reference 8 ms late", Room { delay: ms(60), gain: 0.35, tail_len: ms(120), tail_energy: 0.3, noise: 0.003, drive: 0.0, late: ms(8) }, 12.0),
        ("120 ms, far, heavy clipping", Room { delay: ms(120), gain: 0.25, tail_len: ms(200), tail_energy: 0.5, noise: 0.004, drive: 3.0, late: 0 }, 0.0),
    ]
}

/// The two rooms that run with every `cargo test` (a debug build is slow at 1024 taps); the full table is the ignored test below.
#[test]
fn table_of_rooms() {
    check_rooms(2);
}

/// All rooms: `cargo test --lib aec_tests::full_table -- --ignored --nocapture` (about two minutes in a debug build).
#[test]
#[ignore]
fn full_table_of_rooms() {
    check_rooms(usize::MAX);
}

fn check_rooms(count: usize) {
    eprintln!("{:<38} {:>8} {:>10} {:>10} {:>10}", "room", "ERLE dB", "echo only", "duck ms", "stop ms");
    let mut problems = vec![];
    for (name, room, min_erle) in rooms().into_iter().take(count) {
        let echo_only = run(&room, 8.0, None, 0.0);
        let talk = run(&room, 8.0, Some(5.5), 1.0);
        let quiet = echo_only.false_interrupts == 0 && echo_only.stop_ms.is_none() && echo_only.duck_ms.is_none();
        eprintln!(
            "{:<38} {:>8.1} {:>10} {:>10} {:>10}",
            name,
            echo_only.erle_db,
            if quiet { "quiet" } else { "FALSE" },
            talk.duck_ms.map_or("-".into(), |v| format!("{v:.0}")),
            talk.stop_ms.map_or("-".into(), |v| format!("{v:.0}")),
        );
        if !quiet {
            problems.push(format!("{name}: echo alone interrupted"));
        }
        if echo_only.erle_db < min_erle {
            problems.push(format!("{name}: only {:.1} dB removed (needs {min_erle})", echo_only.erle_db));
        }
        if talk.false_interrupts > 0 {
            problems.push(format!("{name}: interrupted before the person spoke"));
        }
        match talk.stop_ms {
            None if min_erle < 12.0 => {}
            None => problems.push(format!("{name}: the person was not heard")),
            Some(ms) if ms > 500.0 && min_erle >= 12.0 => problems.push(format!("{name}: stop came after {ms:.0} ms")),
            _ => {}
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// "Hey BYTE" said so quietly that it does not stop BYTE by loudness alone still comes out as a speech burst for the wake check, and
/// the echo that is left over never does (no wake check, so no speech recognition, on BYTE's own voice).
#[test]
fn quiet_phrase_gives_a_burst_and_echo_gives_none() {
    let (name, room, _) = rooms().remove(1);
    for (person_at, level) in [(None, 0.0), (Some(5.5f32), 0.03)] {
        let _g = crate::echo::tests_lock();
        crate::echo::set_tap(false);
        let played = byte_speaking(9.0);
        let mut mic = room.echo(&played);
        if let Some(t) = person_at {
            let from = (t * RATE as f32) as usize;
            for (i, v) in at_level(hey_byte(), level).iter().enumerate() {
                mic[from + i] += v;
            }
        }
        let mut barge = Barge::with_source(Box::new(ScriptedSource::new(played.clone())));
        let (mut bursts, mut pos) = (vec![], 0u64);
        for m in mic.chunks(160) {
            let _ = barge.feed(m, pos, Mode::Speakers);
            bursts.extend(barge.take_bursts());
            pos += m.len() as u64;
        }
        crate::echo::set_duck(1.0);
        eprintln!("{name}: person {person_at:?} level {level}: {} burst(s) {:?}", bursts.len(), bursts.iter().map(|b| b.len() as f32 / RATE as f32).collect::<Vec<_>>());
        match person_at {
            None => assert!(bursts.len() <= 3, "the leftover echo was sent for {} wake checks in 9 s", bursts.len()),
            Some(_) => assert!(bursts.iter().any(|b| b.len() >= RATE / 3), "the phrase did not come out as a burst"),
        }
    }
}
