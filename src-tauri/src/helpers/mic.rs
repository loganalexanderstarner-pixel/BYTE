//! Audio taken into memory for the helpers that listen only when asked: dictation (the microphone while a key is held), meeting
//! notes (the microphone and the sound the computer plays) and live captions (the sound the computer plays, the last seconds
//! only). Audio is kept as 16 kHz mono 16-bit numbers in memory, never saved unless a helper writes a file on purpose (meeting
//! notes write a temporary file for the speech model and delete it).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const RATE: usize = 16_000;

/// Floats in -1..1 to 16-bit numbers.
pub fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect()
}

pub fn to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| f32::from(s) / 32768.0).collect()
}

/// Two recordings made at the same time, added together (the shorter one is padded with silence), so one file holds both sides
/// of a call.
pub fn mix(a: &[i16], b: &[i16]) -> Vec<i16> {
    let n = a.len().max(b.len());
    (0..n).map(|i| (i32::from(a.get(i).copied().unwrap_or(0)) + i32::from(b.get(i).copied().unwrap_or(0))).clamp(-32768, 32767) as i16).collect()
}

/// A 16 kHz mono 16-bit WAV file's bytes.
pub fn wav16(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&(RATE as u32).to_le_bytes());
    b.extend_from_slice(&(RATE as u32 * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// What a recording keeps: everything up to `cap` samples (the oldest are dropped past it; None keeps all).
#[derive(Debug, Default)]
pub struct Buffer {
    data: VecDeque<i16>,
    cap: Option<usize>,
    /// Samples dropped from the front because of the cap.
    dropped: usize,
}

impl Buffer {
    pub fn new(cap: Option<usize>) -> Buffer {
        Buffer { data: VecDeque::new(), cap, dropped: 0 }
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.data.extend(samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16));
        if let Some(cap) = self.cap {
            if self.data.len() > cap {
                let extra = self.data.len() - cap;
                self.data.drain(..extra);
                self.dropped += extra;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn seconds(&self) -> f32 {
        self.data.len() as f32 / RATE as f32
    }

    pub fn copy(&self) -> Vec<i16> {
        self.data.iter().copied().collect()
    }

    /// The newest `n` samples.
    pub fn tail(&self, n: usize) -> Vec<i16> {
        let skip = self.data.len().saturating_sub(n);
        self.data.iter().skip(skip).copied().collect()
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }
}

/// Where the sound comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The microphone.
    Mic,
    /// What the computer is playing (WASAPI loopback on Windows).
    Loopback,
}

/// An open audio stream filling a buffer. Dropping or finishing it closes the stream.
pub struct Recording {
    stop: std::sync::mpsc::Sender<()>,
    buf: Arc<Mutex<Buffer>>,
    done: std::sync::mpsc::Receiver<()>,
}

impl Recording {
    /// Opens the source. None when there is no such device or it cannot be opened.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn start(source: Source, cap_seconds: Option<usize>) -> Option<Recording> {
        use std::sync::mpsc;
        let buf = Arc::new(Mutex::new(Buffer::new(cap_seconds.map(|s| s * RATE))));
        let (stop, stop_rx) = mpsc::channel::<()>();
        let (done_tx, done) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<bool>();
        let fill = buf.clone();
        std::thread::spawn(move || {
            use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
            let stream = (|| {
                let host = cpal::default_host();
                let (dev, cfg) = match source {
                    Source::Mic => {
                        let d = host.default_input_device()?;
                        let c = d.default_input_config().ok()?;
                        (d, c)
                    }
                    Source::Loopback => {
                        let d = host.default_output_device()?;
                        let c = d.default_output_config().ok()?;
                        (d, c)
                    }
                };
                let (rate, channels) = (cfg.sample_rate(), cfg.channels() as usize);
                let s = dev
                    .build_input_stream(
                        cfg.into(),
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let mono: Vec<f32> = data.chunks(channels.max(1)).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
                            let down = crate::wake::downsample(&mono, rate);
                            if let Ok(mut b) = fill.lock() {
                                b.push(&down);
                            }
                        },
                        |e| log::warn!("helper audio: {e}"),
                        None,
                    )
                    .ok()?;
                s.play().ok()?;
                Some(s)
            })();
            let _ = ready_tx.send(stream.is_some());
            let Some(_stream) = stream else { return };
            let _ = stop_rx.recv();
            drop(_stream);
            let _ = done_tx.send(());
        });
        ready_rx.recv_timeout(std::time::Duration::from_secs(3)).ok().filter(|ok| *ok).map(|_| Recording { stop, buf, done })
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn start(_source: Source, _cap_seconds: Option<usize>) -> Option<Recording> {
        None
    }

    pub fn seconds(&self) -> f32 {
        self.buf.lock().map(|b| b.seconds()).unwrap_or(0.0)
    }

    /// The newest `seconds` of sound (a copy; the recording goes on).
    pub fn last(&self, seconds: usize) -> Vec<i16> {
        self.buf.lock().map(|b| b.tail(seconds * RATE)).unwrap_or_default()
    }

    /// Closes the stream and returns everything kept.
    pub fn finish(self) -> Vec<i16> {
        let _ = self.stop.send(());
        let _ = self.done.recv_timeout(std::time::Duration::from_secs(3));
        self.buf.lock().map(|b| b.copy()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_keeps_only_its_newest_seconds() {
        let mut b = Buffer::new(Some(RATE * 2));
        b.push(&vec![0.5; RATE]);
        b.push(&vec![-0.5; RATE]);
        b.push(&vec![0.25; RATE]);
        assert_eq!(b.len(), RATE * 2);
        assert!((b.seconds() - 2.0).abs() < 0.001);
        let t = b.tail(RATE);
        assert!(t.iter().all(|&s| s == 8191));
        assert_eq!(b.copy().len(), RATE * 2);
        assert_eq!(b.dropped, RATE);
    }

    #[test]
    fn an_uncapped_buffer_keeps_everything_and_clears() {
        let mut b = Buffer::new(None);
        b.push(&vec![0.1; 1000]);
        b.push(&vec![0.1; 1000]);
        assert_eq!(b.len(), 2000);
        b.clear();
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn mixing_adds_both_sides_pads_the_short_one_and_never_wraps() {
        assert_eq!(mix(&[100, 200], &[1, 2, 3]), vec![101, 202, 3]);
        assert_eq!(mix(&[32000], &[32000]), vec![32767]);
        assert_eq!(mix(&[-32000], &[-32000]), vec![-32768]);
        assert!(mix(&[], &[]).is_empty());
    }

    #[test]
    fn samples_convert_both_ways() {
        let f = to_f32(&to_i16(&[0.0, 0.5, -0.5, 1.0, 2.0]));
        assert!((f[1] - 0.5).abs() < 0.001 && (f[2] + 0.5).abs() < 0.001 && f[3] > 0.99 && f[4] > 0.99);
    }

    #[test]
    fn a_wav_has_a_right_header_and_size() {
        let w = wav16(&[1, -1, 2]);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 16_000);
        assert_eq!(u32::from_le_bytes([w[40], w[41], w[42], w[43]]), 6);
    }

    #[test]
    fn there_is_no_recording_where_there_is_no_audio_backend_or_device() {
        // On a machine with no sound device this is None; with one it is Some and must close cleanly.
        if let Some(r) = Recording::start(Source::Mic, Some(2)) {
            let _ = r.finish();
        }
    }
}
