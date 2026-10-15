//! The screen settings as one small value the rest of `screen/` works from (pure, so it is testable).

use crate::settings::Settings;

pub const DEFAULT_SECONDS: u32 = 30;
pub const MIN_SECONDS: u32 = 5;
pub const MAX_SECONDS: u32 = 120;
pub const DEFAULT_FPS: u32 = 1;
pub const MAX_FPS: u32 = 4;
/// Longest side of a stored replay frame, in pixels.
pub const REPLAY_SIDE: u32 = 960;
/// Longest side of a frame sent to a model for one question.
pub const ASK_SIDE: u32 = 1280;
/// JPEG quality for stored and sent frames.
pub const JPEG_QUALITY: u8 = 70;

pub fn default_seconds() -> u32 {
    DEFAULT_SECONDS
}
pub fn default_fps() -> u32 {
    DEFAULT_FPS
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub ask: bool,
    pub excludes: Vec<String>,
    pub replay: bool,
    pub seconds: u32,
    pub fps: u32,
    pub advisor: bool,
    pub read_it: bool,
}

impl Config {
    pub fn from_settings(s: &Settings) -> Config {
        Config {
            ask: s.screen_ask,
            excludes: s.screen_exclude_apps.clone(),
            replay: s.replay_buffer,
            seconds: s.replay_seconds.clamp(MIN_SECONDS, MAX_SECONDS),
            fps: s.replay_fps.clamp(1, MAX_FPS),
            advisor: s.game_advisor,
            read_it: s.read_it,
        }
    }

    /// Milliseconds between replay frames.
    pub fn interval_ms(&self) -> u64 {
        1000 / u64::from(self.fps.clamp(1, MAX_FPS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_that_looks_at_the_screen_is_off_by_default() {
        let c = Config::from_settings(&Settings::default());
        assert!(!c.ask && !c.replay && !c.advisor && !c.read_it);
        assert_eq!((c.seconds, c.fps), (30, 1));
        assert!(!c.excludes.is_empty());
    }

    #[test]
    fn seconds_and_fps_are_kept_in_range() {
        let s = Settings { replay_seconds: 100_000, replay_fps: 60, ..Settings::default() };
        let c = Config::from_settings(&s);
        assert_eq!((c.seconds, c.fps, c.interval_ms()), (120, 4, 250));
        let s = Settings { replay_seconds: 0, replay_fps: 0, ..Settings::default() };
        let c = Config::from_settings(&s);
        assert_eq!((c.seconds, c.fps, c.interval_ms()), (5, 1, 1000));
    }
}
