//! Counts and budgets with an "Unlimited" choice. Settings keep them as text: a number, a time like "30m", or "unlimited".

/// A count from its setting text. "unlimited" is `None` (no cap); a number is the cap; anything else is `default`.
pub fn count(text: &str, default: Option<u64>) -> Option<u64> {
    let t = text.trim().to_lowercase();
    if t == "unlimited" {
        return None;
    }
    t.parse::<u64>().ok().filter(|n| *n > 0).or(default)
}

/// A time in milliseconds from "90s", "30m", "2h", a bare number of minutes, or "unlimited" (`None`).
pub fn time_ms(text: &str, default: Option<u64>) -> Option<u64> {
    let t = text.trim().to_lowercase();
    if t == "unlimited" {
        return None;
    }
    let (num, unit) = match t.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => t.split_at(i),
        None => (t.as_str(), "m"),
    };
    let n: u64 = match num.parse() {
        Ok(n) if n > 0 => n,
        _ => return default,
    };
    match unit.trim() {
        "s" | "sec" | "secs" => Some(n * 1000),
        "m" | "min" | "mins" => Some(n * 60_000),
        "h" | "hr" | "hrs" => Some(n * 3_600_000),
        _ => default,
    }
}

/// How a cap reads to a person.
pub fn say_count(cap: Option<u64>) -> String {
    cap.map(|n| n.to_string()).unwrap_or_else(|| "no limit".into())
}

pub fn say_time(cap: Option<u64>) -> String {
    match cap {
        None => "no limit".into(),
        Some(ms) if ms % 3_600_000 == 0 => format!("{} h", ms / 3_600_000),
        Some(ms) if ms % 60_000 == 0 => format!("{} min", ms / 60_000),
        Some(ms) => format!("{} s", ms / 1000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_is_no_cap_and_numbers_are_caps() {
        assert_eq!(count("unlimited", Some(5)), None);
        assert_eq!(count("200", None), Some(200));
        assert_eq!(count("0", Some(7)), Some(7));
        assert_eq!(count("lots", Some(7)), Some(7));
        assert_eq!(count(" Unlimited ", Some(1)), None);
    }

    #[test]
    fn times_read_in_seconds_minutes_and_hours() {
        assert_eq!(time_ms("90s", None), Some(90_000));
        assert_eq!(time_ms("30m", None), Some(1_800_000));
        assert_eq!(time_ms("2h", None), Some(7_200_000));
        assert_eq!(time_ms("45", None), Some(45 * 60_000));
        assert_eq!(time_ms("unlimited", Some(1)), None);
        assert_eq!(time_ms("soon", Some(9)), Some(9));
        assert_eq!(time_ms("5x", Some(9)), Some(9));
    }

    #[test]
    fn caps_read_aloud() {
        assert_eq!(say_count(None), "no limit");
        assert_eq!(say_time(Some(90_000)), "90 s");
        assert_eq!(say_time(Some(1_800_000)), "30 min");
        assert_eq!(say_time(Some(7_200_000)), "2 h");
    }
}
