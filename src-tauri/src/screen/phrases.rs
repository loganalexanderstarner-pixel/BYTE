//! Which spoken or typed requests are about the screen. Plain phrase matching, no model, like the other PC flows.
//!
//! Matching is deliberately narrow: a screen request is short, and the vague ones ("how could I do better") only count when
//! nothing else is being talked about ("... at work"). Whether BYTE may then look is decided elsewhere, by the settings.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// "What do you think about this?", "what's on my screen?", "explain this error".
    Ask,
    /// "How could I do better?", "where did I go wrong?", "what happened there?" (the replay buffer).
    Review,
    /// "How's my city doing?", "what's wrong here?" (game advisor).
    Advisor,
    /// "Read this page", "read the selected text".
    ReadThis,
    /// "Stop reading."
    StopReading,
}

/// Lowercase words only: punctuation and apostrophes removed ("what's" becomes "whats").
fn words(message: &str) -> Vec<String> {
    let text = crate::router::without_wake_phrase(message).to_lowercase();
    text.replace(['\u{2019}', '\''], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// `needle` (space separated words) occurs in `hay` as whole words; returns the words before it and the words after it.
fn find<'a>(hay: &'a [String], needle: &str) -> Option<(&'a [String], &'a [String])> {
    let n: Vec<&str> = needle.split(' ').collect();
    if n.is_empty() || hay.len() < n.len() {
        return None;
    }
    (0..=hay.len() - n.len()).find(|&i| hay[i..i + n.len()].iter().zip(&n).all(|(a, b)| a == b)).map(|i| (&hay[..i], &hay[i + n.len()..]))
}

/// Words that may follow a screen phrase without making it about something else ("explain this error please", "what am I looking at right now").
const TRAILING: &[&str] = &[
    "page", "screen", "window", "error", "message", "popup", "dialog", "please", "right", "now", "here", "to", "me", "mean", "means", "say", "says", "tab", "app", "thanks", "again", "one",
];
/// Words that may come before it ("hey byte can you ...").
const LEADING_MAX: usize = 6;

fn only(rest: &[String], ok: &[&str]) -> bool {
    rest.iter().all(|t| ok.contains(&t.as_str()))
}

/// The phrase counts only when it ends the request (or is followed only by words that keep it about the screen) and is not buried
/// in a long sentence: "what do you think about this song" is about a song, not the screen.
fn ends_like_a_screen_ask(before: &[String], rest: &[String]) -> bool {
    before.len() <= LEADING_MAX && only(rest, TRAILING)
}

/// Words that tie a vague phrase to play.
const PLAY_WORDS: &[&str] = &["there", "fight", "round", "match", "game", "play", "run", "death", "boss", "race", "wave", "battle", "level", "raid", "attempt", "clip", "moment", "try"];
/// Words that may follow a looking-back phrase.
const PLAY_OK: &[&str] = &[
    "there", "here", "just", "now", "that", "this", "last", "in", "the", "my", "a", "at", "again", "so", "badly", "too", "much", "fight", "round", "match", "game", "play", "run", "death", "boss",
    "race", "wave", "battle", "level", "raid", "attempt", "clip", "moment", "try", "back", "time", "please",
];
/// Words that tie a "what should I fix" to a game, a city or this screen.
const GAME_CONTEXT: &[&str] = &["game", "city", "town", "colony", "base", "factory", "here", "empire", "settlement", "kingdom", "village", "island", "railway", "tycoon", "park"];

pub fn detect(message: &str) -> Option<Intent> {
    let w = words(message);
    if w.is_empty() || w.len() > 40 {
        return None;
    }
    let any_ends = |ps: &[&str]| ps.iter().any(|p| find(&w, p).is_some_and(|(b, r)| ends_like_a_screen_ask(b, r)));

    if any_ends(&["stop reading", "stop reading it", "stop reading to me", "stop reading that"]) {
        return Some(Intent::StopReading);
    }
    if any_ends(&[
        "read this page", "read this to me", "read that to me", "read it to me", "read the selected text", "read my selection", "read the selection", "read what is selected", "read whats selected",
        "read this aloud", "read this out loud", "read this out", "read this article", "read the page", "read the screen", "read my screen", "read whats on my screen", "read what is on my screen",
        "read the selected", "read this text", "read the highlighted text", "read what i highlighted",
    ]) {
        return Some(Intent::ReadThis);
    }
    // The game advisor: about a place or system in a game, said with "my".
    const THINGS: &[&str] = &["city", "town", "colony", "base", "factory", "empire", "settlement", "kingdom", "village", "island", "railway", "tycoon"];
    for pre in ["hows my", "how is my", "hows our", "how is our"] {
        if let Some((_, rest)) = find(&w, pre) {
            if rest.first().is_some_and(|t| THINGS.contains(&t.as_str())) && rest.len() <= 4 {
                return Some(Intent::Advisor);
            }
        }
    }
    if any_ends(&["whats wrong here", "what is wrong here", "whats going wrong here", "whats wrong with my city", "whats wrong with my town", "whats wrong with my colony", "whats wrong with my base", "what is wrong with my city"]) {
        return Some(Intent::Advisor);
    }
    for p in ["what should i fix", "what should i fix first", "what should i build next", "what do i need to fix", "what should i do next"] {
        if let Some((_, rest)) = find(&w, p) {
            if rest.iter().any(|t| GAME_CONTEXT.contains(&t.as_str())) {
                return Some(Intent::Advisor);
            }
        }
    }
    // Looking back at play. Specific phrases need only play words after them; vague ones must also name a play word.
    const REVIEW_SPECIFIC: &[&str] = &[
        "how could i do better", "how can i do better", "how could i have done better", "where did i go wrong", "where did i fail", "what happened there", "what happened just now", "what happened back there",
        "why did i die", "why did i lose", "why did we lose", "why did i fail", "what killed me", "how did i die",
    ];
    for p in REVIEW_SPECIFIC {
        if let Some((b, rest)) = find(&w, p) {
            if b.len() <= LEADING_MAX && only(rest, PLAY_OK) {
                return Some(Intent::Review);
            }
        }
    }
    const REVIEW_VAGUE: &[&str] = &["how can i get better", "how can i improve", "how could i improve", "what did i do wrong", "what did i miss", "what went wrong"];
    for p in REVIEW_VAGUE {
        if let Some((b, rest)) = find(&w, p) {
            if b.len() <= LEADING_MAX && only(rest, PLAY_OK) && rest.iter().any(|t| PLAY_WORDS.contains(&t.as_str())) {
                return Some(Intent::Review);
            }
        }
    }
    if let Some((_, rest)) = find(&w, "review my last").or_else(|| find(&w, "review the last")).or_else(|| find(&w, "review that")).or_else(|| find(&w, "review my")) {
        const PLAY: &[&str] = &["play", "fight", "round", "match", "game", "run", "death", "battle", "race", "wave", "attempt", "try", "level", "boss", "raid", "seconds", "clip", "replay", "moment", "minute"];
        if rest.len() <= 3 && rest.iter().any(|t| PLAY.contains(&t.as_str())) {
            return Some(Intent::Review);
        }
    }
    const ASK: &[&str] = &[
        "what do you think about this", "what do you think of this", "what do you think about that", "what do you think of that", "whats on my screen", "what is on my screen",
        "what am i looking at", "look at my screen", "look at the screen", "look at this page", "look at this window", "can you see my screen", "can you see this", "check my screen",
        "explain this page", "explain this error", "explain this screen", "explain this window", "explain this message", "explain this", "what does this error", "what does this page", "what does this say",
        "what does this mean", "summarize this page", "summarise this page", "what is this page", "whats this page", "whats this error", "what is this error", "help me with this page", "help me with this screen",
        "what do you see", "what can you see", "tell me about this page", "tell me about this window", "what is this app", "whats this app",
    ];
    if any_ends(ASK) {
        return Some(Intent::Ask);
    }
    // "How could this do that?", "how can this work?": a short question about the thing on screen.
    for p in ["how could this", "how can this", "how would this"] {
        if let Some((b, rest)) = find(&w, p) {
            if b.len() <= 3 && w.len() <= 8 && rest.first().is_some_and(|t| t == "do" || t == "work") {
                return Some(Intent::Ask);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Option<Intent> {
        detect(s)
    }

    #[test]
    fn asking_about_the_screen() {
        for s in [
            "What do you think about this?",
            "what do you think about this page",
            "What's on my screen?",
            "what is on my screen right now",
            "what am I looking at",
            "what am I looking at here",
            "Hey BYTE, explain this error",
            "explain this error please",
            "explain this page",
            "how could this do that?",
            "how can this work",
            "What does this error mean?",
            "what does this say",
            "summarize this page",
            "can you see this",
            "what do you see",
        ] {
            assert_eq!(d(s), Some(Intent::Ask), "{s}");
        }
    }

    #[test]
    fn a_phrase_inside_a_longer_question_is_not_a_screen_ask() {
        for s in [
            "what do you think about this song",
            "what does this mean for my taxes",
            "what do you see in the news",
            "how could this be better",
            "how would this affect my taxes",
            "what does this say about me",
            "can you see this typo in my sentence",
            "what do you think of this idea",
            "explain this to my boss in simple terms",
            "what am I looking at in terms of a mortgage rate",
        ] {
            assert_eq!(d(s), None, "{s}");
        }
    }

    #[test]
    fn looking_back_at_play() {
        for s in [
            "how could I do better?",
            "Where did I go wrong",
            "what happened there?",
            "why did I die",
            "how can I improve in that fight",
            "what went wrong in that round",
            "review my last fight",
            "review the last round",
            "okay byte where did i go wrong there",
        ] {
            assert_eq!(d(s), Some(Intent::Review), "{s}");
        }
    }

    #[test]
    fn ordinary_questions_are_not_taken_for_the_screen() {
        for s in [
            "how could I do better at work?",
            "where did I go wrong in my essay",
            "how can I improve my resume",
            "what happened in the 1929 stock market crash",
            "what do you think about remote work",
            "how does this work in rust",
            "write a poem about the sea",
            "what is the weather in Dover",
            "review my code for bugs",
            "what's wrong with my sourdough starter",
            "how's my cat doing after the vet, do you think",
            "how is the economy doing",
            "what should I fix for dinner",
            "what should I build next in my side project",
            "what should I fix first in my car",
            "what did I miss in the meeting",
            "what went wrong with my deploy",
            "how can I improve my sleep",
        ] {
            assert_eq!(d(s), None, "{s}");
        }
    }

    #[test]
    fn game_advisor_phrases() {
        for s in ["how's my city doing?", "how is my colony doing", "what's wrong here?", "what should I fix first in my city", "How's my factory?", "whats wrong with my base", "what should i build next in this game"] {
            assert_eq!(d(s), Some(Intent::Advisor), "{s}");
        }
        assert_eq!(d("how's my day going"), None);
        assert_eq!(d("how is my mother doing"), None);
    }

    #[test]
    fn reading_aloud() {
        for s in ["read this page", "read this to me", "read the selected text", "Read the selection", "read my screen", "hey byte, read it to me"] {
            assert_eq!(d(s), Some(Intent::ReadThis), "{s}");
        }
        assert_eq!(d("stop reading"), Some(Intent::StopReading));
        assert_eq!(d("read me a bedtime story"), None);
        assert_eq!(d("can you read a map"), None);
        assert_eq!(d("read this page and tell me what it says about taxes in detail please"), None);
    }

    #[test]
    fn a_long_message_is_never_a_screen_request() {
        let long = "what do you think about this ".repeat(20);
        assert_eq!(d(&long), None);
        assert_eq!(d(""), None);
    }
}
