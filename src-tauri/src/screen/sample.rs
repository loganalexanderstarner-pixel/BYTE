//! Which replay frames go to the model for "where did I go wrong?". The failure is usually at the end, so the last few frames
//! are taken close together and the rest are spread evenly over the earlier part. Pure, so it is tested without a screen.

/// Indices (ascending) of up to `spread + tail` frames out of `n`: the last `tail` ones, and `spread` evenly spaced from the
/// frames before them (the first frame always included when `spread >= 2`).
pub fn pick(n: usize, spread: usize, tail: usize) -> Vec<usize> {
    if n == 0 {
        return vec![];
    }
    if n <= spread + tail {
        return (0..n).collect();
    }
    let tail_start = n - tail;
    let mut out: Vec<usize> = Vec::new();
    match spread {
        0 => {}
        1 => out.push(tail_start - 1),
        _ => {
            // Evenly over 0..tail_start (exclusive), ends included.
            let last = tail_start - 1;
            for k in 0..spread {
                out.push((k * last + (spread - 1) / 2) / (spread - 1));
            }
        }
    }
    out.extend(tail_start..n);
    out.dedup();
    out
}

/// Seconds before the newest frame, for each picked frame (`times` are the frames' own times in ms).
pub fn seconds_before_end(times: &[u64], picked: &[usize]) -> Vec<f32> {
    let end = times.last().copied().unwrap_or(0);
    picked.iter().map(|&i| (end.saturating_sub(times[i])) as f32 / 1000.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_small_buffers_give_everything() {
        assert!(pick(0, 8, 4).is_empty());
        assert_eq!(pick(5, 8, 4), vec![0, 1, 2, 3, 4]);
        assert_eq!(pick(12, 8, 4), (0..12).collect::<Vec<_>>());
    }

    #[test]
    fn a_long_buffer_is_spread_evenly_and_ends_with_the_last_frames() {
        let p = pick(30, 8, 4);
        assert_eq!(p.len(), 12);
        assert_eq!(p[0], 0);
        assert_eq!(&p[8..], &[26, 27, 28, 29]);
        assert!(p.windows(2).all(|w| w[0] < w[1]), "{p:?}");
        // The spread part covers the earlier frames up to just before the tail.
        assert_eq!(p[7], 25);
    }

    #[test]
    fn never_more_than_asked_and_always_sorted_and_unique() {
        for n in 1..200 {
            for (s, t) in [(8, 4), (6, 3), (2, 2), (1, 1), (0, 3), (5, 0)] {
                let p = pick(n, s, t);
                assert!(p.len() <= s + t, "n={n} s={s} t={t} {p:?}");
                assert!(p.windows(2).all(|w| w[0] < w[1]), "n={n} {p:?}");
                assert!(p.iter().all(|&i| i < n));
                if t > 0 {
                    assert_eq!(*p.last().unwrap(), n - 1, "the newest frame is always included");
                }
            }
        }
    }

    #[test]
    fn times_are_counted_back_from_the_newest_frame() {
        let times = [0, 1000, 2000, 3000];
        assert_eq!(seconds_before_end(&times, &[0, 2, 3]), vec![3.0, 1.0, 0.0]);
    }
}
