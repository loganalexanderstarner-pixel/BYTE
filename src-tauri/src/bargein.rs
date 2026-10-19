//! Barge-in: talking over BYTE. While BYTE speaks, the microphone stream keeps running; what the person says is told apart from BYTE's
//! own voice, the volume ducks at once, and if the speech goes on BYTE stops and the next turn is captured with the words that
//! interrupted it (a new question, or "stop").
//!
//! Two modes (setting `bargeIn`):
//! - `headphones`: nothing of BYTE's voice can reach the microphone, so any sustained speech counts. No echo canceller.
//! - `speakers`: BYTE's voice is in the microphone. `echo.rs` subtracts what BYTE is playing first, and the leftover has to be clearly
//!   louder than the echo that is expected, so the thresholds are stricter.
//!
//! The thresholds below are constants on purpose: they decide how easy BYTE is to interrupt, and only the owner's room can say what is
//! right (speaker volume, distance to the microphone, how quiet the room is). docs/HEY-BYTE-ASSISTANT.md says how to tune them.

#![cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]

use std::collections::VecDeque;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter};

use crate::echo::{self, EchoCanceller};

/// The default of the setting is off: on speakers, "headphones" mode would hear BYTE's own voice and cut itself off. The owner turns on
/// `headphones` (any speech stops BYTE) or `speakers` (echo cancelled) on purpose.
pub const DEFAULT_MODE: &str = "off";

pub const RATE: usize = 16_000;
/// Analysis frame: 30 ms.
pub const FRAME: usize = RATE * 30 / 1000;
/// Sustained speech this long ducks BYTE's volume at once.
pub const DUCK_MS: u32 = 300;
/// Sustained speech this long stops BYTE.
pub const STOP_MS: u32 = 390;
/// Quiet this long inside a stretch of speech (between words) does not end it.
pub const GAP_MS: u32 = 90;
/// BYTE's volume while ducked (share of normal).
pub const DUCK_GAIN: f32 = 0.2;
/// Loudness (rms, 0..1) a frame needs to count as speech in headphones mode: the same as the wake-word detector.
pub const HEADPHONES_MIN: f32 = 0.008;
pub const HEADPHONES_FLOOR_X: f32 = 3.0;
/// Speakers mode: stricter. The frame must also be this share of the echo level the canceller predicts.
pub const SPEAKERS_MIN: f32 = 0.02;
pub const SPEAKERS_FLOOR_X: f32 = 2.5;
pub const SPEAKERS_ECHO_SHARE: f32 = 0.5;
/// The speech threshold sits this many times above the echo the room measurably leaves behind (never below 0.2 or above `SPEAKERS_ECHO_SHARE` of the predicted echo).
pub const ECHO_SAFETY: f32 = 1.8;
/// How much audio before the stop is handed to the next turn (so that its first words are not lost): 1.5 s.
pub const SEED_MS: usize = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Headphones,
    Speakers,
}

pub fn parse_mode(s: &str) -> Mode {
    match s {
        "off" => Mode::Off,
        "speakers" => Mode::Speakers,
        "headphones" => Mode::Headphones,
        _ => Mode::Off,
    }
}

static MODE: Mutex<Mode> = Mutex::new(Mode::Off);

pub fn set_mode(m: &str) {
    let mode = parse_mode(m);
    if let Ok(mut x) = MODE.lock() {
        *x = mode;
    }
    echo::set_tap(mode == Mode::Speakers);
    if mode != Mode::Speakers {
        echo::set_duck(1.0);
    }
}

pub fn mode() -> Mode {
    MODE.lock().map(|m| *m).unwrap_or(Mode::Off)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Speech started over BYTE: lower its volume.
    Duck,
    /// The speech stopped before BYTE was stopped: the volume comes back.
    Restore,
    /// The person is interrupting: stop BYTE and listen.
    Stop,
}

/// Decides, frame by frame, whether the person is talking over BYTE. Pure: it only sees the residual and the expected echo level.
pub struct Detector {
    mode: Mode,
    floor: f32,
    pending: Vec<f32>,
    run_ms: u32,
    gap_ms: u32,
    ducked: bool,
}

impl Detector {
    pub fn new(mode: Mode) -> Self {
        Detector { mode, floor: 0.003, pending: vec![], run_ms: 0, gap_ms: 0, ducked: false }
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.run_ms = 0;
        self.gap_ms = 0;
        self.ducked = false;
    }

    pub fn ducked(&self) -> bool {
        self.ducked
    }

    /// The loudness a frame needs to count as speech.
    pub fn threshold(&self, echo_rms: f32) -> f32 {
        match self.mode {
            Mode::Speakers => (self.floor * SPEAKERS_FLOOR_X).max(SPEAKERS_MIN).max(SPEAKERS_ECHO_SHARE * echo_rms),
            _ => (self.floor * HEADPHONES_FLOOR_X).max(HEADPHONES_MIN),
        }
    }

    /// Feeds 16 kHz samples (the residual after echo cancellation); `echo_rms` is the level of BYTE's own voice expected in them.
    pub fn feed(&mut self, residual: &[f32], echo_rms: f32) -> Vec<Action> {
        let mut out = vec![];
        self.pending.extend_from_slice(residual);
        while self.pending.len() >= FRAME {
            let frame: Vec<f32> = self.pending.drain(..FRAME).collect();
            let e = (frame.iter().map(|v| v * v).sum::<f32>() / FRAME as f32).sqrt();
            if e > self.threshold(echo_rms) {
                self.run_ms += 30;
                self.gap_ms = 0;
                if self.run_ms >= STOP_MS {
                    out.push(Action::Stop);
                    self.reset();
                } else if self.run_ms >= DUCK_MS && !self.ducked {
                    self.ducked = true;
                    out.push(Action::Duck);
                }
            } else {
                self.gap_ms += 30;
                if self.gap_ms > GAP_MS {
                    if self.ducked {
                        out.push(Action::Restore);
                    }
                    self.run_ms = 0;
                    self.ducked = false;
                    // The floor follows the room (and, in headphones mode, the quiet of the headset).
                    self.floor = self.floor * 0.95 + e * 0.05;
                }
            }
        }
        out
    }
}

/// The running side: the canceller, the detector and a short memory of what was said, fed by the wake listener's loop.
pub struct Barge {
    source: Box<dyn crate::aec::ReferenceSource>,
    /// Speech bursts in the cleaned signal while BYTE talks: the listener checks them for the wake phrase (a spoken "Hey BYTE" over BYTE).
    wake_vad: crate::wake::Vad,
    bursts: Vec<Vec<f32>>,
    canceller: EchoCanceller,
    det: Detector,
    ring: VecDeque<f32>,
    generation: u64,
    active: bool,
    mode: Mode,
}

impl Default for Barge {
    fn default() -> Self {
        Barge::with_source(Box::new(crate::aec::LiveSource::default()))
    }
}

impl Barge {
    pub fn with_source(source: Box<dyn crate::aec::ReferenceSource>) -> Self {
        Barge {
            source,
            wake_vad: crate::wake::Vad::default(),
            bursts: vec![],
            canceller: EchoCanceller::new(),
            det: Detector::new(Mode::Headphones),
            ring: VecDeque::new(),
            generation: 0,
            active: false,
            mode: Mode::Headphones,
        }
    }

    /// The speech bursts heard in the cleaned signal since the last call (to be checked for the wake phrase).
    pub fn take_bursts(&mut self) -> Vec<Vec<f32>> {
        std::mem::take(&mut self.bursts)
    }

    /// BYTE is not speaking (any more): everything goes back to normal.
    pub fn idle(&mut self) {
        if self.active {
            self.active = false;
            self.det.reset();
            self.ring.clear();
            self.bursts.clear();
            self.wake_vad = crate::wake::Vad::default();
            echo::set_duck(1.0);
        }
    }

    /// One microphone chunk while BYTE speaks. `mic_pos` is where the chunk starts on the microphone stream. Returns the audio to
    /// start the next turn with when the person has interrupted (BYTE is stopped by the caller).
    pub fn feed(&mut self, mic: &[f32], mic_pos: u64, mode: Mode) -> Option<Vec<f32>> {
        if mode == Mode::Off {
            self.idle();
            return None;
        }
        if mode != self.mode || !self.active {
            self.mode = mode;
            self.det = Detector::new(mode);
            self.active = true;
        }
        let (residual, echo_rms, usable) = if mode == Mode::Speakers {
            // A reference that stopped being recorded cannot be subtracted: no decisions from the raw microphone.
            if echo::tap_overflowed() {
                tlog(|| "no decision: the speaker reference was not recorded in time".into());
                return None;
            }
            let Some((reference, gen)) = self.source.reference(mic_pos, mic.len()) else {
                tlog(|| "no decision: there is no speaker reference".into());
                return None;
            };
            if gen != self.generation {
                self.generation = gen;
                self.canceller.begin_session();
            }
            let r = self.canceller.process(mic, &reference);
            // The leftover echo is judged by what this room really leaves (measured), not by a fixed share of the predicted echo.
            let share = (ECHO_SAFETY * self.canceller.leftover_share()).clamp(0.2, SPEAKERS_ECHO_SHARE);
            (r, self.canceller.echo_rms() * share / SPEAKERS_ECHO_SHARE, self.canceller.settled())
        } else {
            (mic.to_vec(), 0.0, true)
        };
        crate::wakemodel::offer_residual(&residual, usable); // track:wakemodel (the quick "Hey BYTE" detector also listens while BYTE speaks)
        self.ring.extend(residual.iter().copied());
        let keep = RATE * SEED_MS / 1000;
        while self.ring.len() > keep {
            self.ring.pop_front();
        }
        // Until the canceller knows the delay its output is still the raw microphone, full of BYTE's own voice: no decisions yet.
        if !usable {
            tlog(|| "no decision yet: the echo canceller is still finding the delay and learning the room".into());
            return None;
        }
        tlog(|| {
            let r = (residual.iter().map(|v| v * v).sum::<f32>() / residual.len().max(1) as f32).sqrt();
            format!("mode {:?}: voice level {:.4}, BYTE's own echo {:.4}, needs above {:.4}", self.mode, r, echo_rms, self.det.threshold(echo_rms))
        });
        // The same burst detector as the idle listener, on the cleaned signal: "Hey BYTE" said over BYTE ends in a burst the listener checks.
        // Only chunks that stand out from the leftover echo go in, so the echo that is left over is never sent for a wake check.
        let level = (residual.iter().map(|v| v * v).sum::<f32>() / residual.len().max(1) as f32).sqrt();
        if level > self.det.threshold(echo_rms) {
            self.bursts.extend(self.wake_vad.feed(&residual));
        } else {
            self.bursts.extend(self.wake_vad.feed(&vec![0.0; residual.len()]));
        }
        for a in self.det.feed(&residual, echo_rms) {
            match a {
                Action::Duck => {
                    if crate::wake::test_log_on() {
                        log::info!("[test] interrupt: someone is talking over BYTE, turning it down");
                    }
                    echo::set_duck(DUCK_GAIN)
                }
                Action::Restore => echo::set_duck(1.0),
                Action::Stop => {
                    if crate::wake::test_log_on() {
                        log::info!("[test] interrupt: stopping BYTE, listening");
                    }
                    echo::set_duck(1.0);
                    let seed: Vec<f32> = self.ring.iter().copied().collect();
                    self.ring.clear();
                    return Some(seed);
                }
            }
        }
        None
    }
}

pub const BARGE_EVENT: &str = "wake://barge";

/// A Test-log line about the interrupt logic, at most one a second (the logic runs many times a second while BYTE talks).
fn tlog(msg: impl FnOnce() -> String) {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    if !crate::wake::test_log_on() {
        return;
    }
    if let Ok(mut last) = LAST.lock() {
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return;
        }
        *last = Some(Instant::now());
        log::info!("[test] interrupt: {}", msg());
    }
}

/// The person interrupted: BYTE stops, the UI is told, and the next turn is cut out of the stream starting with the interruption.
pub fn interrupted(app: &AppHandle, seed: &[f32]) {
    crate::tts::stop();
    echo::set_duck(1.0);
    // The page forgets the speaking state (and does not start the answer again) when it hears this.
    let _ = app.emit(crate::speech::DONE_EVENT, ());
    crate::wake::begin_capture_seeded(6_000, seed);
    let _ = app.emit_to(crate::quick::LABEL, BARGE_EVENT, ());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: usize, amp: f32) -> Vec<f32> {
        (0..RATE * ms / 1000).map(|i| (i as f32 * 0.07).sin() * amp).collect()
    }
    fn quiet(ms: usize) -> Vec<f32> {
        vec![0.0005; RATE * ms / 1000]
    }
    fn run(det: &mut Detector, audio: &[f32], echo: f32) -> Vec<Action> {
        audio.chunks(1600).flat_map(|c| det.feed(c, echo)).collect()
    }

    #[test]
    fn sustained_speech_ducks_then_stops() {
        let mut d = Detector::new(Mode::Headphones);
        let mut audio = quiet(300);
        audio.extend(tone(600, 0.2));
        assert_eq!(run(&mut d, &audio, 0.0), vec![Action::Duck, Action::Stop]);
    }

    #[test]
    fn timing_is_about_300_and_390_ms() {
        let mut d = Detector::new(Mode::Headphones);
        // 240 ms of speech: nothing yet.
        assert!(run(&mut d, &tone(240, 0.2), 0.0).is_empty());
        let mut d = Detector::new(Mode::Headphones);
        assert_eq!(run(&mut d, &tone(330, 0.2), 0.0), vec![Action::Duck]);
        let mut d = Detector::new(Mode::Headphones);
        assert_eq!(run(&mut d, &tone(480, 0.2), 0.0), vec![Action::Duck, Action::Stop]);
    }

    #[test]
    fn a_cough_does_nothing_and_a_short_burst_restores_the_volume() {
        let mut d = Detector::new(Mode::Headphones);
        let mut audio = tone(150, 0.2);
        audio.extend(quiet(500));
        assert!(run(&mut d, &audio, 0.0).is_empty());
        // 330 ms of speech ducks, then it stops: the volume comes back.
        let mut audio = tone(330, 0.2);
        audio.extend(quiet(400));
        assert_eq!(run(&mut d, &audio, 0.0), vec![Action::Duck, Action::Restore]);
    }

    #[test]
    fn a_pause_between_words_does_not_restart_the_count() {
        let mut d = Detector::new(Mode::Headphones);
        let mut audio = tone(200, 0.2);
        audio.extend(quiet(60));
        audio.extend(tone(300, 0.2));
        assert!(run(&mut d, &audio, 0.0).contains(&Action::Stop));
    }

    #[test]
    fn speakers_mode_ignores_leftover_echo_but_hears_a_person() {
        // The leftover echo is as loud as expected: nothing.
        let mut d = Detector::new(Mode::Speakers);
        assert!(run(&mut d, &tone(1500, 0.03), 0.1).is_empty());
        // A person clearly above the expected echo: ducks and stops.
        let mut d = Detector::new(Mode::Speakers);
        assert_eq!(run(&mut d, &tone(600, 0.2), 0.05), vec![Action::Duck, Action::Stop]);
        // Headphones would have heard the quiet one.
        let mut d = Detector::new(Mode::Headphones);
        assert!(run(&mut d, &tone(600, 0.03), 0.0).contains(&Action::Stop));
        assert!(Detector::new(Mode::Speakers).threshold(0.0) > Detector::new(Mode::Headphones).threshold(0.0));
    }

    #[test]
    fn modes_parse() {
        assert_eq!(parse_mode("off"), Mode::Off);
        assert_eq!(parse_mode("speakers"), Mode::Speakers);
        assert_eq!(parse_mode("headphones"), Mode::Headphones);
        assert_eq!(parse_mode("junk"), Mode::Off);
        assert_eq!(DEFAULT_MODE, "off");
    }

    #[test]
    fn off_changes_nothing_and_headphones_returns_the_seed() {
        let _g = crate::echo::tests_lock();
        let mut b = Barge::default();
        assert!(b.feed(&tone(1000, 0.3), 0, Mode::Off).is_none());
        let mut got = None;
        let audio = tone(800, 0.3);
        for (i, c) in audio.chunks(1600).enumerate() {
            if let Some(s) = b.feed(c, (i * 1600) as u64, Mode::Headphones) {
                got = Some(s);
                break;
            }
        }
        let seed = got.expect("interrupted");
        assert!(seed.len() >= RATE * 4 / 10 && seed.len() <= RATE * SEED_MS / 1000);
        crate::echo::set_duck(1.0);
    }

    #[test]
    fn speakers_mode_with_a_played_voice_does_not_interrupt_itself() {
        let _g = crate::echo::tests_lock();
        // What BYTE plays is also what the microphone hears (delayed and quieter): the canceller must not let it through as speech.
        crate::echo::set_tap(true);
        let mut noise = 0x2468_ace1u32;
        let mut lp = 0.0f32;
        let played: Vec<f32> = (0..RATE * 6)
            .map(|_| {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                lp = 0.6 * lp + 0.4 * ((noise as f32 / u32::MAX as f32) * 2.0 - 1.0);
                lp * 1.2
            })
            .collect();
        let delay = 800;
        let mic: Vec<f32> = (0..played.len()).map(|i| if i >= delay { played[i - delay] * 0.4 } else { 0.0 }).collect();
        let mut b = Barge::default();
        let start = crate::echo::advance_mic(0);
        crate::echo::tap_push(&played.iter().flat_map(|s| [*s, *s, *s]).collect::<Vec<_>>(), 48_000, true);
        let mut pos = start;
        let mut stopped = false;
        for c in mic.chunks(320) {
            if b.feed(c, pos, Mode::Speakers).is_some() {
                stopped = true;
            }
            pos += c.len() as u64;
        }
        crate::echo::set_tap(false);
        assert!(!stopped, "BYTE interrupted itself");
    }

    #[test]
    fn speakers_mode_hears_a_person_talking_over_the_played_voice() {
        let _g = crate::echo::tests_lock();
        crate::echo::set_tap(true);
        let mut noise = 0x1357_9bdfu32;
        let mut lp = 0.0f32;
        let played: Vec<f32> = (0..RATE * 8)
            .map(|_| {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                lp = 0.6 * lp + 0.4 * ((noise as f32 / u32::MAX as f32) * 2.0 - 1.0);
                lp * 1.2
            })
            .collect();
        let talk_from = RATE * 5;
        let mic: Vec<f32> = (0..played.len())
            .map(|i| {
                let echo = if i >= 800 { played[i - 800] * 0.4 } else { 0.0 };
                let person = if i >= talk_from { 0.12 * (i as f32 * 0.09).sin() } else { 0.0 };
                echo + person
            })
            .collect();
        let mut b = Barge::default();
        let start = crate::echo::advance_mic(0);
        crate::echo::tap_push(&played.iter().flat_map(|s| [*s, *s, *s]).collect::<Vec<_>>(), 48_000, true);
        let mut pos = start;
        let mut stopped_at = None;
        for c in mic.chunks(320) {
            if b.feed(c, pos, Mode::Speakers).is_some() {
                stopped_at.get_or_insert((pos - start) as usize);
            }
            pos += c.len() as u64;
        }
        crate::echo::set_tap(false);
        let at = stopped_at.expect("the person was not heard");
        assert!(at >= talk_from && at < talk_from + RATE, "stopped at {at}");
    }
}
