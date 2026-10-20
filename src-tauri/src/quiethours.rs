//! Quiet hours: a window of the day (for example 22:00 to 07:00) when "Hey BYTE" is switched off and BYTE speaks nothing on its own.
//! Off until the owner turns it on in Settings → Voice and Hey BYTE. Typed questions and the talk key still work.

/// "22:30" -> minutes since midnight. Anything else is None.
pub fn parse(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Whether `now` (minutes since midnight) is inside the window, which may run past midnight. An empty window (same start and end) is never.
pub fn inside(now: u32, from: u32, to: u32) -> bool {
    if from == to {
        false
    } else if from < to {
        now >= from && now < to
    } else {
        now >= from || now < to
    }
}

/// Whether the clock on this PC is inside the owner's quiet hours right now.
pub fn active_now(enabled: bool, from: &str, to: &str) -> bool {
    if !enabled {
        return false;
    }
    let (Some(f), Some(t)) = (parse(from), parse(to)) else { return false };
    use chrono::Timelike;
    let n = chrono::Local::now();
    inside(n.hour() * 60 + n.minute(), f, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_clock_times() {
        assert_eq!(parse("22:30"), Some(1350));
        assert_eq!(parse(" 07:05 "), Some(425));
        assert_eq!(parse("24:00"), None);
        assert_eq!(parse("7pm"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn a_window_past_midnight_covers_both_ends_of_the_night() {
        let (f, t) = (parse("22:00").unwrap(), parse("07:00").unwrap());
        assert!(inside(parse("23:30").unwrap(), f, t));
        assert!(inside(parse("00:10").unwrap(), f, t));
        assert!(inside(parse("06:59").unwrap(), f, t));
        assert!(!inside(parse("07:00").unwrap(), f, t));
        assert!(!inside(parse("12:00").unwrap(), f, t));
        assert!(inside(parse("22:00").unwrap(), f, t));
    }

    #[test]
    fn a_daytime_window_and_an_empty_one() {
        let (f, t) = (parse("13:00").unwrap(), parse("15:00").unwrap());
        assert!(inside(parse("14:00").unwrap(), f, t) && !inside(parse("16:00").unwrap(), f, t));
        assert!(!inside(parse("14:00").unwrap(), f, f));
    }

    #[test]
    fn off_or_unreadable_means_never_quiet() {
        assert!(!active_now(false, "00:00", "23:59"));
        assert!(!active_now(true, "soon", "later"));
    }
}
