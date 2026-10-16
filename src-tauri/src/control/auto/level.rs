//! The autonomy level on the Control page.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Risky steps show a card first (the default).
    #[default]
    Ask,
    /// Steps inside the run contract go ahead; anything outside it waits for a yes.
    Contract,
    /// No approval cards. Refused things stay refused.
    Never,
}

impl Level {
    pub fn parse(text: &str) -> Level {
        match text.trim() {
            "contract" => Level::Contract,
            "never" => Level::Never,
            _ => Level::Ask,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Ask => "ask",
            Level::Contract => "contract",
            Level::Never => "never",
        }
    }

    /// The level the settings mean. The older "never ask" switch still counts.
    pub fn from_settings(level: &str, never_ask: bool) -> Level {
        if never_ask {
            Level::Never
        } else {
            Level::parse(level)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_read_from_text_and_the_old_switch() {
        assert_eq!(Level::parse("contract"), Level::Contract);
        assert_eq!(Level::parse("nonsense"), Level::Ask);
        assert_eq!(Level::from_settings("ask", true), Level::Never);
        assert_eq!(Level::from_settings("contract", false), Level::Contract);
        assert_eq!(Level::from_settings("", false), Level::Ask);
        assert_eq!(Level::Never.as_str(), "never");
    }
}
