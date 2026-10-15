//! The replay buffer's storage: encoded frames kept ONLY in memory, newest last. It forgets frames older than the window,
//! and drops the oldest when a memory cap would be passed. Nothing here touches the disk or the log.

use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    /// When the frame was taken (milliseconds on any steady clock).
    pub at_ms: u64,
    pub jpeg: Vec<u8>,
}

#[derive(Debug)]
pub struct Ring {
    frames: VecDeque<Stored>,
    window_ms: u64,
    max_bytes: usize,
    bytes: usize,
}

/// Most memory the buffer may use (the frames' encoded bytes): 64 MB is about 3 minutes of 960 px frames.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

impl Ring {
    pub fn new(window_secs: u32, max_bytes: usize) -> Ring {
        Ring { frames: VecDeque::new(), window_ms: u64::from(window_secs) * 1000, max_bytes, bytes: 0 }
    }

    pub fn set_window(&mut self, window_secs: u32, now_ms: u64) {
        self.window_ms = u64::from(window_secs) * 1000;
        self.evict(now_ms);
    }

    pub fn push(&mut self, at_ms: u64, jpeg: Vec<u8>) {
        // A single frame bigger than the whole cap is not kept.
        if jpeg.len() > self.max_bytes {
            return;
        }
        self.bytes += jpeg.len();
        self.frames.push_back(Stored { at_ms, jpeg });
        self.evict(at_ms);
    }

    fn evict(&mut self, now_ms: u64) {
        while let Some(f) = self.frames.front() {
            let too_old = now_ms.saturating_sub(f.at_ms) > self.window_ms;
            if too_old || self.bytes > self.max_bytes {
                self.bytes -= f.jpeg.len();
                self.frames.pop_front();
            } else {
                break;
            }
        }
    }

    /// Forgets everything (and gives the memory back).
    pub fn clear(&mut self) {
        self.frames = VecDeque::new();
        self.bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Copies of the frames, oldest first.
    pub fn snapshot(&self) -> Vec<Stored> {
        self.frames.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(r: &mut Ring, n: u64, step_ms: u64, size: usize) {
        for i in 0..n {
            r.push(i * step_ms, vec![i as u8; size]);
        }
    }

    #[test]
    fn keeps_only_the_window() {
        let mut r = Ring::new(30, MAX_BYTES);
        fill(&mut r, 100, 1000, 10); // 100 seconds at 1 fps
        // The newest frame is at 99 s; frames older than 30 s before it are gone.
        assert_eq!(r.len(), 31);
        assert_eq!(r.snapshot().first().unwrap().at_ms, 69_000);
        assert_eq!(r.snapshot().last().unwrap().at_ms, 99_000);
    }

    #[test]
    fn a_memory_cap_drops_the_oldest_first() {
        let mut r = Ring::new(3600, 100);
        fill(&mut r, 10, 1000, 30);
        assert!(r.bytes() <= 100, "{}", r.bytes());
        assert_eq!(r.len(), 3);
        assert_eq!(r.snapshot().last().unwrap().at_ms, 9000);
    }

    #[test]
    fn a_frame_larger_than_the_cap_is_not_kept() {
        let mut r = Ring::new(30, 10);
        r.push(0, vec![0; 11]);
        assert!(r.is_empty());
        assert_eq!(r.bytes(), 0);
    }

    #[test]
    fn clear_forgets_everything() {
        let mut r = Ring::new(30, MAX_BYTES);
        fill(&mut r, 5, 1000, 100);
        r.clear();
        assert!(r.is_empty());
        assert_eq!(r.bytes(), 0);
        assert!(r.snapshot().is_empty());
    }

    #[test]
    fn shrinking_the_window_drops_frames_at_once() {
        let mut r = Ring::new(60, MAX_BYTES);
        fill(&mut r, 60, 1000, 10);
        assert_eq!(r.len(), 60);
        r.set_window(10, 59_000);
        assert_eq!(r.len(), 11);
    }

    #[test]
    fn the_size_total_stays_exact_through_eviction() {
        let mut r = Ring::new(5, MAX_BYTES);
        fill(&mut r, 50, 1000, 7);
        assert_eq!(r.bytes(), r.len() * 7);
    }
}
