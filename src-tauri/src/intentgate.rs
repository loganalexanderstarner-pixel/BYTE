//! Command or conversation? The apps used to turn almost anything that matched a phrase into an action. This gate sits in front of the
//! routes that DO things (open, close, set, play, note, find, control) so BYTE can also just talk (Settings → Control → "How BYTE
//! handles commands"). Typed and spoken text go through the same gate. Questions that only want information (the weather, a
//! calculation, a price) are not actions and are never held back here.
//!
//! - `Smart` (default): act on clear commands; explanations, opinions, "how do I…", "why…", "what do you think…" and long
//!   rambling messages are a conversation, even when they mention a thing BYTE could do.
//! - `Always`: the old behaviour; whatever matches a route runs.
//! - `Chat`: BYTE never acts on its own; start a message with `do:` to run one (`do: set the volume to 30`).

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Smart,
    Always,
    Chat,
}

pub fn parse(setting: &str) -> Mode {
    match setting {
        "always" => Mode::Always,
        "chat" => Mode::Chat,
        _ => Mode::Smart,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict<'a> {
    /// Go ahead and let the action routes look at this text (the text to use: a `do:` prefix is removed).
    Act(&'a str),
    /// Keep it a conversation: the action routes are skipped.
    Talk,
}

pub fn judge(mode: Mode, text: &str) -> Verdict<'_> {
    let q = text.trim();
    match mode {
        Mode::Always => Verdict::Act(q),
        Mode::Chat => match strip_do(q) {
            Some(rest) => Verdict::Act(rest),
            None => Verdict::Talk,
        },
        Mode::Smart => {
            if let Some(rest) = strip_do(q) {
                return Verdict::Act(rest); // an explicit do: always means it, in every mode
            }
            if looks_like_conversation(q) {
                Verdict::Talk
            } else {
                Verdict::Act(q)
            }
        }
    }
}

fn strip_do(q: &str) -> Option<&str> {
    let l = q.get(..3)?;
    if l.eq_ignore_ascii_case("do:") {
        let rest = q[3..].trim();
        (!rest.is_empty()).then_some(rest)
    } else {
        None
    }
}

const POLITE: &[&str] = &["hey byte", "okay byte", "ok byte", "byte", "please", "okay", "ok", "can you", "could you", "would you", "will you", "i want you to", "i need you to", "go ahead and", "just"];

const COMMAND_VERBS: &[&str] = &[
    "set", "turn", "open", "close", "play", "pause", "resume", "skip", "next", "previous", "mute", "unmute", "lock", "sleep", "find", "remind", "start",
    "stop", "cancel", "switch", "show", "put", "send", "add", "remove", "delete", "take", "screenshot", "volume", "brightness", "launch", "quit",
    "restart", "shut", "shutdown", "run", "make", "create", "save", "note", "todo", "call", "text", "email", "schedule", "enable", "disable", "toggle",
    "increase", "decrease", "raise", "lower", "dim", "bring", "go", "scroll", "press", "click", "type", "read", "dark", "wifi", "bluetooth",
];

const TALK_OPENERS: &[&str] = &[
    "what do you think", "what did you think", "what would you", "how do ", "how does ", "how did ", "how would ", "how could ", "how can i", "how should",
    "why ", "explain", "tell me about", "tell me why", "tell me how", "describe ", "should i ", "do you ", "did you ", "have you ", "would you recommend",
    "i think", "i feel", "i was ", "i am ", "i'm ", "i've ", "i don't", "i can't", "i just ", "let's talk", "lets talk", "can we talk", "tell me a ", "what's your",
    "whats your", "who are you", "are you ",
];

fn lower(text: &str) -> String {
    text.to_lowercase().replace(['\u{2019}', '\u{2018}'], "'")
}

/// Strips "hey byte", "please", "can you"… from the start (repeatedly) so "hey byte, can you turn it down" is judged by "turn it down".
fn without_polite(l: &str) -> &str {
    let mut s = l.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    loop {
        let mut changed = false;
        for p in POLITE {
            if let Some(rest) = s.strip_prefix(p) {
                if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c == ',') {
                    s = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
                    changed = true;
                    break;
                }
            }
        }
        if !changed {
            return s;
        }
    }
}

fn first_word(s: &str) -> &str {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'')).next().unwrap_or("")
}

pub fn looks_like_conversation(text: &str) -> bool {
    let l = lower(text);
    let core = without_polite(&l);
    if COMMAND_VERBS.contains(&first_word(core)) {
        return false;
    }
    if TALK_OPENERS.iter().any(|o| core.starts_with(o)) {
        return true;
    }
    // A long message with no command in front is somebody talking.
    core.split_whitespace().count() >= 30
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acts(mode: Mode, q: &str) -> bool {
        matches!(judge(mode, q), Verdict::Act(_))
    }

    #[test]
    fn clear_commands_act_in_smart_mode() {
        for q in [
            "set the volume to 30",
            "Turn the volume down a bit",
            "open Spotify",
            "hey byte, can you turn off the wifi",
            "please close the window",
            "could you pause the music?",
            "lock my pc",
            "remind me to call mom at five",
            "note: buy milk",
            "find my tax return",
            "mute",
            "dark mode on",
            "what's my battery",
            "how's my pc",
            "what's the weather",
            "set a timer for ten minutes",
        ] {
            assert!(acts(Mode::Smart, q), "should act: {q}");
        }
    }

    #[test]
    fn conversation_stays_conversation_in_smart_mode() {
        for q in [
            "what do you think about the volume on this song",
            "how do I turn off the wifi on my router",
            "how does the lock screen work",
            "why does my pc go to sleep so fast",
            "explain how notifications work",
            "tell me about opening a restaurant",
            "I think I should close my account but I'm not sure",
            "I was playing a game and it crashed",
            "do you like music",
            "I'm tired today",
            "should I open a savings account",
            "how can I change my shipping address on this site",
        ] {
            assert!(!acts(Mode::Smart, q), "should talk: {q}");
        }
    }

    #[test]
    fn long_rambles_are_conversation_unless_they_start_with_a_command() {
        let long = "so yesterday I was thinking about how the whole thing with my computer started and honestly I could not tell you when it began but there was this noise and then the screen went dark for a while and then it came back";
        assert!(!acts(Mode::Smart, long));
        assert!(acts(Mode::Smart, &format!("open the control panel {long}")));
    }

    #[test]
    fn always_keeps_the_old_behaviour() {
        assert!(acts(Mode::Always, "how do I turn off the wifi"));
        assert!(acts(Mode::Always, "I'm tired today"));
    }

    #[test]
    fn chat_only_acts_after_do() {
        assert!(!acts(Mode::Chat, "set the volume to 30"));
        assert_eq!(judge(Mode::Chat, "do: set the volume to 30"), Verdict::Act("set the volume to 30"));
        assert_eq!(judge(Mode::Chat, "DO: lock my pc"), Verdict::Act("lock my pc"));
        assert!(!acts(Mode::Chat, "do:"));
        assert_eq!(judge(Mode::Smart, "do: how do I x"), Verdict::Act("how do I x"));
    }

    #[test]
    fn the_setting_text_maps_to_a_mode() {
        assert_eq!(parse("always"), Mode::Always);
        assert_eq!(parse("chat"), Mode::Chat);
        assert_eq!(parse("smart"), Mode::Smart);
        assert_eq!(parse(""), Mode::Smart);
    }
}
