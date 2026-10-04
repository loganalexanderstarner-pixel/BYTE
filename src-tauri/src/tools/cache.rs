//! Short-lived memory caches for web lookups (searches, papers, places), so
//! follow-up questions, Regenerate and research that repeats a search don't
//! ask the services again. Pages have their own cache in `fetch.rs`. Memory only (nothing is written to disk) and bounded by
//! entry count and size; entries expire after their time-to-live.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use super::academic::Paper;
use super::places::Spot;
use super::search::Searched;

struct Entry<V> {
    value: V,
    stored: Instant,
    used: Instant,
    size: usize,
}

/// A bounded time-to-live cache with least-recently-used eviction.
pub struct TtlCache<V> {
    ttl: Duration,
    max_entries: usize,
    max_bytes: usize,
    inner: Mutex<(HashMap<String, Entry<V>>, usize)>,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(ttl: Duration, max_entries: usize, max_bytes: usize) -> Self {
        TtlCache { ttl, max_entries, max_bytes, inner: Mutex::new((HashMap::new(), 0)) }
    }

    pub fn get(&self, key: &str) -> Option<V> {
        self.get_at(key, Instant::now())
    }

    fn get_at(&self, key: &str, now: Instant) -> Option<V> {
        let mut g = self.inner.lock().ok()?;
        let (map, bytes) = &mut *g;
        match map.get_mut(key) {
            Some(e) if now.duration_since(e.stored) < self.ttl => {
                e.used = now;
                Some(e.value.clone())
            }
            Some(_) => {
                let e = map.remove(key)?;
                *bytes -= e.size;
                None
            }
            None => None,
        }
    }

    /// Stores a value of about `size` bytes; the least recently used entries go first when full.
    pub fn put(&self, key: &str, value: V, size: usize) {
        self.put_at(key, value, size, Instant::now());
    }

    fn put_at(&self, key: &str, value: V, size: usize, now: Instant) {
        if size > self.max_bytes {
            return;
        }
        let Ok(mut g) = self.inner.lock() else { return };
        let (map, bytes) = &mut *g;
        if let Some(old) = map.remove(key) {
            *bytes -= old.size;
        }
        while !map.is_empty() && (map.len() >= self.max_entries || *bytes + size > self.max_bytes) {
            let oldest = map.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                if let Some(e) = map.remove(&k) {
                    *bytes -= e.size;
                }
            }
        }
        *bytes += size;
        map.insert(key.to_string(), Entry { value, stored: now, used: now, size });
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.0.len()).unwrap_or(0)
    }
}

/// Lower-case, single-spaced, so "Rust  release" and "rust release" match.
pub fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Search results by query (and whether the cloud searched): 1 hour.
pub static SEARCHES: Lazy<TtlCache<Searched>> = Lazy::new(|| TtlCache::new(Duration::from_secs(3600), 300, 8 << 20));
/// Paper searches: 24 hours.
pub static PAPERS: Lazy<TtlCache<Vec<Paper>>> = Lazy::new(|| TtlCache::new(Duration::from_secs(24 * 3600), 100, 8 << 20));
/// YouTube transcripts (video details, captions, track): 24 hours.
pub static TRANSCRIPTS: Lazy<TtlCache<(crate::youtube::VideoInfo, Vec<crate::youtube::Cue>, crate::youtube::Track)>> =
    Lazy::new(|| TtlCache::new(Duration::from_secs(24 * 3600), 30, 16 << 20));
/// Places near a point: 1 hour (opening hours change "open now").
pub static PLACES: Lazy<TtlCache<Vec<Spot>>> = Lazy::new(|| TtlCache::new(Duration::from_secs(3600), 100, 4 << 20));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_expire() {
        let c: TtlCache<u32> = TtlCache::new(Duration::from_secs(10), 10, 1000);
        let t0 = Instant::now();
        c.put_at("a", 1, 1, t0);
        assert_eq!(c.get_at("a", t0 + Duration::from_secs(5)), Some(1));
        assert_eq!(c.get_at("a", t0 + Duration::from_secs(11)), None);
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn least_recently_used_goes_first() {
        let c: TtlCache<u32> = TtlCache::new(Duration::from_secs(100), 2, 1000);
        let t0 = Instant::now();
        c.put_at("a", 1, 1, t0);
        c.put_at("b", 2, 1, t0 + Duration::from_secs(1));
        // Touch "a", so "b" is the least recently used.
        c.get_at("a", t0 + Duration::from_secs(2));
        c.put_at("c", 3, 1, t0 + Duration::from_secs(3));
        assert_eq!(c.get_at("b", t0 + Duration::from_secs(4)), None);
        assert_eq!(c.get_at("a", t0 + Duration::from_secs(4)), Some(1));
        assert_eq!(c.get_at("c", t0 + Duration::from_secs(4)), Some(3));
    }

    #[test]
    fn size_is_bounded() {
        let c: TtlCache<u32> = TtlCache::new(Duration::from_secs(100), 100, 10);
        let t0 = Instant::now();
        c.put_at("a", 1, 6, t0);
        c.put_at("b", 2, 6, t0 + Duration::from_secs(1));
        assert_eq!(c.len(), 1);
        assert_eq!(c.get_at("b", t0 + Duration::from_secs(2)), Some(2));
        // Too big to ever fit: not stored.
        c.put_at("huge", 3, 11, t0);
        assert_eq!(c.get_at("huge", t0), None);
        // Replacing a key doesn't double-count it.
        c.put_at("b", 4, 6, t0 + Duration::from_secs(3));
        assert_eq!(c.get_at("b", t0 + Duration::from_secs(3)), Some(4));
        assert_eq!(norm("  Rust   Release "), "rust release");
    }
}
