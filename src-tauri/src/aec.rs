//! Echo reference and microphone hand-over for talking over BYTE.
//!
//! Three small pieces that `echo.rs`/`bargein.rs`/`wake.rs` share:
//!
//! 1. `Ring`: a lock-free single-producer single-consumer ring of samples. The audio callback (microphone or loopback) only writes
//!    into it; the processing thread reads. The callback never allocates, never takes a lock, never waits. When the reader falls
//!    behind, the newest samples are dropped and counted instead of blocking the audio thread (that blocking is what Windows reports as
//!    "buffer underrun or overrun").
//! 2. `ReferenceSource`: where the canceller gets "what the speakers played, aligned with the microphone". The default asks the
//!    loopback of the real output device when it is running (exact, because it is the very stream sent to the speakers, including
//!    other programs' sound) and otherwise the tap in tts.rs. A scripted double serves tests.
//! 3. `stats`: counters for the Test log (overruns, stream errors, restarts, reference gaps).
//!
//! The Windows loopback (`loopback`) captures the default output device through WASAPI via cpal (an input stream built on an
//! output device is a loopback capture there). See STATUS notes in docs/HEY-BYTE-ASSISTANT.md for why this route was picked over the
//! Windows echo-cancelling capture effect and the Voice Capture DSP.

#![cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::echo::RATE;

// ------------------------------------------------------------------ the ring

/// Lock-free ring of f32 samples for exactly one writer thread and one reader thread.
pub struct Ring {
    buf: Box<[AtomicU32]>,
    mask: usize,
    /// Total samples ever written / read (they only grow; the difference is the fill).
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

impl Ring {
    /// A ring holding at least `capacity` samples (rounded up to a power of two).
    pub fn new(capacity: usize) -> Arc<Ring> {
        let cap = capacity.max(2).next_power_of_two();
        Arc::new(Ring { buf: (0..cap).map(|_| AtomicU32::new(0)).collect(), mask: cap - 1, head: AtomicUsize::new(0), tail: AtomicUsize::new(0), dropped: AtomicU64::new(0) })
    }

    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    pub fn len(&self) -> usize {
        self.head.load(Ordering::Acquire).wrapping_sub(self.tail.load(Ordering::Acquire))
    }

    /// Samples that did not fit since the start.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Writer side. Returns how many samples were dropped because the ring was full (0 normally). Wait-free, no allocation.
    pub fn push(&self, data: &[f32]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let free = self.capacity() - head.wrapping_sub(tail);
        let n = data.len().min(free);
        for (i, v) in data[..n].iter().enumerate() {
            self.buf[(head + i) & self.mask].store(v.to_bits(), Ordering::Relaxed);
        }
        self.head.store(head.wrapping_add(n), Ordering::Release);
        let lost = data.len() - n;
        if lost > 0 {
            self.dropped.fetch_add(lost as u64, Ordering::Relaxed);
        }
        lost
    }

    /// Reader side: appends up to `max` samples to `out`; returns how many.
    pub fn pop_into(&self, out: &mut Vec<f32>, max: usize) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let n = head.wrapping_sub(tail).min(max);
        out.reserve(n);
        for i in 0..n {
            out.push(f32::from_bits(self.buf[(tail + i) & self.mask].load(Ordering::Relaxed)));
        }
        self.tail.store(tail.wrapping_add(n), Ordering::Release);
        n
    }

    /// Reader side: throws away the oldest samples so that at most `keep` remain.
    pub fn trim_to(&self, keep: usize) {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        let fill = head.wrapping_sub(tail);
        if fill > keep {
            self.tail.store(tail.wrapping_add(fill - keep), Ordering::Release);
        }
    }
}

// ------------------------------------------------------------------ counters for the Test log

pub mod stats {
    use super::*;

    pub static MIC_OVERRUNS: AtomicU64 = AtomicU64::new(0);
    /// Scratch for the feeder's own drop count (the real total is read from the ring).
    pub static MIC_DROPPED: AtomicU64 = AtomicU64::new(0);
    pub static MIC_ERRORS: AtomicU64 = AtomicU64::new(0);
    pub static MIC_RESTARTS: AtomicU64 = AtomicU64::new(0);
    pub static REF_DROPPED: AtomicU64 = AtomicU64::new(0);
    pub static REF_ERRORS: AtomicU64 = AtomicU64::new(0);
    pub static REF_RESTARTS: AtomicU64 = AtomicU64::new(0);
    /// Microphone chunks that were cleaned with a loopback reference / with the playback tap / not cleaned (no reference).
    pub static CHUNKS_LOOPBACK: AtomicU64 = AtomicU64::new(0);
    pub static CHUNKS_TAP: AtomicU64 = AtomicU64::new(0);

    pub fn snapshot() -> [u64; 8] {
        [
            MIC_OVERRUNS.load(Ordering::Relaxed),
            MIC_ERRORS.load(Ordering::Relaxed),
            MIC_RESTARTS.load(Ordering::Relaxed),
            REF_DROPPED.load(Ordering::Relaxed),
            REF_ERRORS.load(Ordering::Relaxed),
            REF_RESTARTS.load(Ordering::Relaxed),
            CHUNKS_LOOPBACK.load(Ordering::Relaxed),
            CHUNKS_TAP.load(Ordering::Relaxed),
        ]
    }

    pub fn line(s: &[u64; 8]) -> String {
        format!(
            "mic: {} dropped samples (reader too slow), {} stream errors, {} restarts; speaker reference: {} dropped, {} errors, {} restarts; cleaned {} chunks with loopback, {} with the playback tap",
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]
        )
    }
}

/// Writes one counter line to the Test log when something changed since the last call (call it every few seconds from the listener).
pub fn log_stats_if_changed(last: &mut [u64; 8]) {
    if !crate::wake::test_log_on() {
        return;
    }
    let now = stats::snapshot();
    // The two chunk counters grow all the time while BYTE speaks: only the fault counters decide whether a line is worth writing.
    if now[..6] != last[..6] {
        log::info!("[test] {}", stats::line(&now));
    }
    *last = now;
}

// ------------------------------------------------------------------ the reference

/// What the speakers played, for one microphone chunk.
pub trait ReferenceSource: Send {
    /// The reference for the microphone chunk that starts at stream position `mic_pos` and has `len` samples (16 kHz mono; zeros where
    /// nothing was played) and a number that changes when a new stretch of speech starts. None when no reference exists at all.
    fn reference(&mut self, mic_pos: u64, len: usize) -> Option<(Vec<f32>, u64)>;
    /// Short name for the Test log.
    fn name(&self) -> &'static str;
}

/// The default: the loopback of the output device when it runs, else the tap on BYTE's own playback.
#[derive(Default)]
pub struct LiveSource {
    gen: u64,
    was_loopback: bool,
    playing_last: bool,
}

impl ReferenceSource for LiveSource {
    fn reference(&mut self, mic_pos: u64, len: usize) -> Option<(Vec<f32>, u64)> {
        if let Some(r) = loopback_take(len) {
            if !self.was_loopback {
                self.was_loopback = true;
                self.gen += 1;
            }
            stats::CHUNKS_LOOPBACK.fetch_add(1, Ordering::Relaxed);
            let _ = self.playing_last;
            return Some((r, self.gen));
        }
        self.was_loopback = false;
        stats::CHUNKS_TAP.fetch_add(1, Ordering::Relaxed);
        Some(crate::echo::tap_reference(mic_pos, len))
    }

    fn name(&self) -> &'static str {
        if self.was_loopback {
            "loopback"
        } else {
            "playback tap"
        }
    }
}

/// Test double: plays back a prepared reference signal, aligned by position, with an optional lateness (samples the reference arrives
/// behind the microphone, like a loopback packet that came late) so tests can check that the canceller copes.
pub struct ScriptedSource {
    pub signal: Vec<f32>,
    pub late: usize,
    pub generation: u64,
}

impl ScriptedSource {
    pub fn new(signal: Vec<f32>) -> Self {
        ScriptedSource { signal, late: 0, generation: 1 }
    }
}

impl ReferenceSource for ScriptedSource {
    fn reference(&mut self, mic_pos: u64, len: usize) -> Option<(Vec<f32>, u64)> {
        let out = (0..len).map(|k| (mic_pos as usize + k).checked_sub(self.late).and_then(|i| self.signal.get(i)).copied().unwrap_or(0.0)).collect();
        Some((out, self.generation))
    }

    fn name(&self) -> &'static str {
        "scripted"
    }
}

// ------------------------------------------------------------------ the loopback hand-over

/// Loopback samples (16 kHz mono) waiting for the reader. None while no loopback stream runs.
static LOOPBACK_RING: std::sync::Mutex<Option<Arc<Ring>>> = std::sync::Mutex::new(None);
static WANTED: AtomicBool = AtomicBool::new(false);
/// The reader keeps at most this much loopback audio waiting, so the reference never falls far behind the microphone.
const MAX_BACKLOG: usize = RATE * 3 / 10;

/// Settings: whether the speaker reference should come from the output device's loopback.
pub fn set_wanted(on: bool) {
    WANTED.store(on, Ordering::Relaxed);
}

pub fn wanted() -> bool {
    WANTED.load(Ordering::Relaxed)
}

fn set_ring(r: Option<Arc<Ring>>) {
    if let Ok(mut g) = LOOPBACK_RING.lock() {
        *g = r;
    }
}

/// Whether a loopback stream is delivering the reference right now.
pub fn loopback_running() -> bool {
    LOOPBACK_RING.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// The next `len` reference samples: what has arrived since the last call, zero-padded at the end when the output device delivered
/// less (WASAPI loopback sends nothing while the device is silent, so a short read means "silence", not a fault).
pub fn loopback_take(len: usize) -> Option<Vec<f32>> {
    let ring = LOOPBACK_RING.lock().ok()?.clone()?;
    take_from(&ring, len)
}

pub fn take_from(ring: &Ring, len: usize) -> Option<Vec<f32>> {
    ring.trim_to(MAX_BACKLOG + len);
    let mut out = Vec::with_capacity(len);
    ring.pop_into(&mut out, len);
    out.resize(len, 0.0);
    Some(out)
}

/// Converts audio from a device's rate and channel count to 16 kHz mono without allocating once warm, and hands it to a ring.
/// Used by both audio callbacks.
pub struct Feeder {
    dec: crate::echo::Decimator,
    channels: usize,
    scratch: Vec<f32>,
    pub ring: Arc<Ring>,
    dropped_counter: &'static AtomicU64,
}

impl Feeder {
    pub fn new(rate: u32, channels: usize, ring: Arc<Ring>, dropped_counter: &'static AtomicU64) -> Self {
        Feeder { dec: crate::echo::Decimator::new(rate), channels: channels.max(1), scratch: Vec::with_capacity(16_384), ring, dropped_counter }
    }

    /// Called on the audio thread with interleaved samples.
    pub fn feed(&mut self, data: &[f32]) {
        self.scratch.clear();
        let ch = self.channels;
        let mut i = 0;
        while i + ch <= data.len() {
            let mut sum = 0.0;
            for c in 0..ch {
                sum += data[i + c];
            }
            self.dec.push_into(&[sum / ch as f32], &mut self.scratch);
            i += ch;
        }
        let lost = self.ring.push(&self.scratch);
        if lost > 0 {
            self.dropped_counter.fetch_add(lost as u64, Ordering::Relaxed);
        }
    }
}

/// Keeps the loopback stream in step with the setting. Lives on the listener thread (a cpal stream is not Send).
pub struct LoopbackControl {
    #[cfg(windows)]
    stream: Option<cpal::Stream>,
    /// Set by the stream's error callback; the next `sync` rebuilds the stream.
    failed: Arc<AtomicBool>,
    last_try: Option<std::time::Instant>,
}

impl Default for LoopbackControl {
    fn default() -> Self {
        LoopbackControl {
            #[cfg(windows)]
            stream: None,
            failed: Arc::new(AtomicBool::new(false)),
            last_try: None,
        }
    }
}

impl Drop for LoopbackControl {
    fn drop(&mut self) {
        set_ring(None);
    }
}

impl LoopbackControl {
    /// Starts, stops or rebuilds the loopback stream. `need` is true while the speaker reference is wanted (barge-in on speakers, setting on).
    pub fn sync(&mut self, need: bool) {
        #[cfg(windows)]
        {
            if !need || !wanted() {
                if self.stream.take().is_some() {
                    set_ring(None);
                }
                return;
            }
            if self.failed.swap(false, Ordering::Relaxed) && self.stream.take().is_some() {
                set_ring(None);
                stats::REF_RESTARTS.fetch_add(1, Ordering::Relaxed);
            }
            if self.stream.is_none() {
                // A device that cannot be opened is retried every 10 s, not in a tight loop.
                if self.last_try.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(10)) {
                    return;
                }
                self.last_try = Some(std::time::Instant::now());
                if let Some((s, ring)) = loopback::open(self.failed.clone()) {
                    set_ring(Some(ring));
                    self.stream = Some(s);
                } else {
                    stats::REF_ERRORS.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (need, &self.failed, &self.last_try);
        }
    }

    pub fn stop(&mut self) {
        self.sync(false);
    }
}

#[cfg(windows)]
mod loopback {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::*;

    /// Opens the default OUTPUT device as a capture stream. With WASAPI that is a loopback: it delivers exactly what is sent to the
    /// speakers, in the device's own format.
    pub fn open(failed: Arc<AtomicBool>) -> Option<(cpal::Stream, Arc<Ring>)> {
        let dev = cpal::default_host().default_output_device()?;
        let cfg = dev.default_output_config().ok()?;
        let (rate, channels) = (cfg.sample_rate(), cfg.channels() as usize);
        let ring = Ring::new(RATE * 2);
        let mut feeder = Feeder::new(rate, channels, ring.clone(), &stats::REF_DROPPED);
        let s = dev
            .build_input_stream(
                cfg.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| feeder.feed(data),
                move |e| {
                    stats::REF_ERRORS.fetch_add(1, Ordering::Relaxed);
                    failed.store(true, Ordering::Relaxed);
                    log::warn!("speaker reference: {e}");
                },
                None,
            )
            .ok()?;
        s.play().ok()?;
        Some((s, ring))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_round_trips_and_counts_what_does_not_fit() {
        let r = Ring::new(8);
        assert_eq!(r.capacity(), 8);
        assert_eq!(r.push(&[1.0, 2.0, 3.0, 4.0, 5.0]), 0);
        // Only 3 free: the 2 newest are dropped, never blocked on.
        assert_eq!(r.push(&[6.0, 7.0, 8.0, 9.0, 10.0]), 2);
        assert_eq!(r.dropped(), 2);
        let mut out = vec![];
        assert_eq!(r.pop_into(&mut out, 100), 8);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(r.len(), 0);
        // Wraps around many times.
        for k in 0..100 {
            r.push(&[k as f32; 5]);
            let mut o = vec![];
            r.pop_into(&mut o, 5);
            assert_eq!(o, vec![k as f32; 5]);
        }
    }

    #[test]
    fn ring_survives_a_real_writer_and_reader_thread() {
        let r = Ring::new(1 << 12);
        let w = r.clone();
        let total = 200_000usize;
        let t = std::thread::spawn(move || {
            let mut sent = 0usize;
            while sent < total {
                let chunk: Vec<f32> = (sent..(sent + 100).min(total)).map(|i| i as f32).collect();
                let lost = w.push(&chunk);
                if lost > 0 {
                    // The reader is behind: the real callback would drop; here retry the unsent tail to check ordering.
                    let kept = chunk.len() - lost;
                    sent += kept;
                    std::thread::yield_now();
                } else {
                    sent += chunk.len();
                }
            }
        });
        let mut got: Vec<f32> = vec![];
        loop {
            r.pop_into(&mut got, 4096);
            if t.is_finished() && r.len() == 0 {
                break;
            }
        }
        t.join().unwrap();
        // Whatever arrived is in order and unduplicated.
        assert_eq!(got, (0..total).map(|i| i as f32).collect::<Vec<_>>());
    }

    #[test]
    fn take_pads_silence_and_bounds_the_backlog() {
        let r = Ring::new(RATE * 2);
        let out = take_from(&r, 160).unwrap();
        assert_eq!(out, vec![0.0; 160]);
        r.push(&vec![0.5; RATE]);
        let out = take_from(&r, 160).unwrap();
        assert_eq!(out.len(), 160);
        // A second of backlog was cut to the limit: the rest is not a second behind real time.
        assert!(r.len() <= MAX_BACKLOG);
    }

    #[test]
    fn feeder_converts_stereo_48k_without_losing_the_level() {
        let ring = Ring::new(RATE);
        static DROPS: AtomicU64 = AtomicU64::new(0);
        let mut f = Feeder::new(48_000, 2, ring.clone(), &DROPS);
        let stereo: Vec<f32> = (0..4800).flat_map(|_| [0.4, 0.2]).collect(); // 100 ms
        f.feed(&stereo);
        let mut out = vec![];
        ring.pop_into(&mut out, 10_000);
        assert!((out.len() as i64 - 1600).abs() <= 2, "{}", out.len());
        assert!(out.iter().all(|v| (v - 0.3).abs() < 1e-4));
    }

    #[test]
    fn scripted_source_is_aligned_and_can_be_late() {
        let mut s = ScriptedSource::new((0..100).map(|i| i as f32).collect());
        assert_eq!(s.reference(10, 3).unwrap().0, vec![10.0, 11.0, 12.0]);
        s.late = 5;
        assert_eq!(s.reference(3, 4).unwrap().0, vec![0.0, 0.0, 0.0, 1.0]);
        assert_eq!(s.reference(6, 3).unwrap().0, vec![1.0, 2.0, 3.0]);
        assert_eq!(s.reference(98, 4).unwrap().0, vec![93.0, 94.0, 95.0, 96.0]);
    }

    #[test]
    fn no_loopback_means_no_reference_from_it() {
        set_ring(None);
        assert!(loopback_take(160).is_none());
        let mut live = LiveSource::default();
        // Falls back to the playback tap (zeros when nothing was played).
        let (r, _) = live.reference(0, 8).unwrap();
        assert_eq!(r.len(), 8);
    }
}
