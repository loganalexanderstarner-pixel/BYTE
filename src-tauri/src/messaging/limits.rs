//! Rate limits: at most `limit` events inside a sliding window; a limit of 0 means Unlimited.

use std::collections::{HashMap, VecDeque};

#[derive(Default)]
pub struct Limiter {
    events: HashMap<String, VecDeque<i64>>,
}

impl Limiter {
    /// Records an event and says yes, unless `limit` events already happened in the last `window_ms`.
    pub fn take(&mut self, key: &str, limit: u32, window_ms: i64, now_ms: i64) -> bool {
        let q = self.events.entry(key.to_string()).or_default();
        while q.front().is_some_and(|t| now_ms - *t >= window_ms) {
            q.pop_front();
        }
        if limit != 0 && q.len() >= limit as usize {
            return false;
        }
        q.push_back(now_ms);
        true
    }

    /// How many events are inside the window now.
    pub fn used(&mut self, key: &str, window_ms: i64, now_ms: i64) -> usize {
        let q = self.events.entry(key.to_string()).or_default();
        while q.front().is_some_and(|t| now_ms - *t >= window_ms) {
            q.pop_front();
        }
        q.len()
    }
}

pub const MINUTE: i64 = 60_000;
pub const HOUR: i64 = 3_600_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_slides() {
        let mut l = Limiter::default();
        assert!(l.take("a", 2, MINUTE, 0));
        assert!(l.take("a", 2, MINUTE, 10));
        assert!(!l.take("a", 2, MINUTE, 20));
        assert!(l.take("b", 2, MINUTE, 20), "keys are separate");
        assert!(l.take("a", 2, MINUTE, MINUTE + 1), "the oldest left the window");
    }

    #[test]
    fn zero_is_unlimited() {
        let mut l = Limiter::default();
        for i in 0..1000 {
            assert!(l.take("a", 0, MINUTE, i));
        }
    }
}
