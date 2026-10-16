//! A wake phrase of your own ("computer", "hey jarvis", "okay robot") in place of "Hey BYTE".
//!
//! How it works: the same speech model that listens for "Hey BYTE" writes down what it heard, and BYTE looks for the chosen words
//! in that text, allowing for the small mis-hearings a speech model makes (one wrong letter in a long word). There is no dedicated
//! keyword model for custom phrases here, so a custom phrase is less sure than "Hey BYTE" (more misses, and now and then a false
//! trigger); a longer, more unusual phrase works better than a short common word.

use std::sync::Mutex;

pub const DEFAULT_PHRASE: &str = "hey byte";
const MIN_LETTERS: usize = 6;
const MAX_LETTERS: usize = 30;

/// Common words that would trigger all the time.
const TOO_COMMON: &[&str] = &["hey", "okay", "hello", "the", "and", "you", "that", "this", "what", "yes", "stop", "play", "start", "computer play", "hey you"];

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(|w| w.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>()).filter(|w| !w.is_empty()).collect()
}

/// A phrase that can be used: 1 to 3 words of letters, 6 to 30 letters in all, not just a common word. Returns it tidied.
pub fn valid(phrase: &str) -> Result<String, String> {
    if phrase.chars().any(|c| !(c.is_alphabetic() || c == ' ' || c == '\'' || c == '-')) {
        return Err("Use only letters and spaces.".into());
    }
    let w = words(phrase);
    let letters: usize = w.iter().map(|x| x.chars().count()).sum();
    if w.is_empty() || w.len() > 3 {
        return Err("Use one to three words.".into());
    }
    if letters < MIN_LETTERS {
        return Err(format!("That is too short ({MIN_LETTERS} letters at least), so it would trigger by accident."));
    }
    if letters > MAX_LETTERS {
        return Err("That is too long to say.".into());
    }
    let joined = w.join(" ");
    if TOO_COMMON.contains(&joined.as_str()) {
        return Err("That is too common a word: it would trigger all the time.".into());
    }
    Ok(joined)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1])));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Does the heard word match the wanted one: the same, or one letter off in a word of five letters or more.
fn near(heard: &str, want: &str) -> bool {
    heard == want || (want.chars().count() >= 5 && heard.chars().count() >= 4 && edit_distance(heard, want) <= 1)
}

/// The words said after the phrase in the same breath, or None when the phrase is not in the text. Like `wake::wake_rest`.
pub fn custom_rest(text: &str, phrase: &str) -> Option<String> {
    let original: Vec<&str> = text.split_whitespace().collect();
    let heard: Vec<String> = original.iter().map(|w| w.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()).collect();
    let want = words(phrase);
    if want.is_empty() || heard.len() < want.len() {
        return None;
    }
    let at = (0..=heard.len() - want.len()).find(|&i| want.iter().enumerate().all(|(k, w)| near(&heard[i + k], w)))?;
    let from = at + want.len();
    Some(original[from..].join(" ").trim_matches(|c: char| c == ',' || c == '.' || c.is_whitespace()).to_string())
}

static PHRASE: Mutex<Option<String>> = Mutex::new(None);

/// Sets the phrase from the setting (empty or unusable means "Hey BYTE").
pub fn set(phrase: &str) {
    if let Ok(mut p) = PHRASE.lock() {
        *p = resolve(phrase);
    }
}

/// The phrase to use for a setting's text: None (Hey BYTE) when it is empty or unusable.
pub fn resolve(phrase: &str) -> Option<String> {
    if phrase.trim().is_empty() { None } else { valid(phrase).ok() }
}

/// The custom phrase in force, if any.
pub fn current() -> Option<String> {
    PHRASE.lock().ok().and_then(|p| p.clone())
}

/// What the speech model is told to expect.
pub fn prompt() -> String {
    prompt_for(current().as_deref())
}

pub fn prompt_for(phrase: Option<&str>) -> String {
    phrase.map(|p| p.split(' ').map(|w| w.chars().next().map(|c| c.to_uppercase().collect::<String>() + &w[c.len_utf8()..]).unwrap_or_default()).collect::<Vec<_>>().join(" ")).unwrap_or_else(|| "Hey BYTE".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phrase_must_be_long_enough_and_not_a_common_word() {
        assert_eq!(valid("Hey Jarvis").as_deref(), Ok("hey jarvis"));
        assert_eq!(valid("  computer ").as_deref(), Ok("computer"));
        assert!(valid("hey").is_err());
        assert!(valid("ok").unwrap_err().contains("too short"));
        assert!(valid("one two three four").unwrap_err().contains("one to three"));
        assert!(valid("hey 123").unwrap_err().contains("letters"));
        assert!(valid("okay").is_err());
        assert!(valid(&"a".repeat(31)).unwrap_err().contains("too long"));
        assert!(valid("").is_err());
    }

    #[test]
    fn the_phrase_is_found_with_the_rest_of_the_sentence() {
        assert_eq!(custom_rest("Hey Jarvis, what's the weather?", "hey jarvis").as_deref(), Some("what's the weather?"));
        assert_eq!(custom_rest("hey jarvis.", "hey jarvis").as_deref(), Some(""));
        assert_eq!(custom_rest("so computer set a timer for ten minutes", "computer").as_deref(), Some("set a timer for ten minutes"));
        assert_eq!(custom_rest("what a nice day", "hey jarvis"), None);
    }

    #[test]
    fn a_small_mishearing_is_forgiven_only_in_long_words() {
        assert!(custom_rest("hey jarvas what time is it", "hey jarvis").is_some());
        assert!(custom_rest("hey jarvus", "hey jarvis").is_some());
        // Short words must be exact: "hey" is not "hex".
        assert!(custom_rest("hex jarvis", "hey jarvis").is_none());
        assert!(custom_rest("hey service", "hey jarvis").is_none());
        // Two wrong letters is a different word.
        assert!(custom_rest("hey jarbas", "hey jarvis").is_none());
        assert!(custom_rest("hey jorbas", "hey jarvis").is_none());
    }

    #[test]
    fn the_setting_is_applied_and_an_unusable_one_falls_back_to_hey_byte() {
        // (The live setting is a global that other tests read, so only the pure parts are checked here.)
        assert_eq!(resolve("okay robot").as_deref(), Some("okay robot"));
        assert_eq!(prompt_for(Some("okay robot")), "Okay Robot");
        assert_eq!(resolve("hey"), None);
        assert_eq!(prompt_for(None), "Hey BYTE");
        assert_eq!(resolve(""), None);
    }
}
