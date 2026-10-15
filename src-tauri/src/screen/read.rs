//! "Read this page" / "read the selected text": get the words (the selection if there is one, else the window's text read
//! from a picture of it) and say them with BYTE's voice, a few sentences at a time (the speech queue chunks them), so
//! "stop reading" or the stop button ends it at the next sentence.

/// Most characters read in one go; past it BYTE reads the start and says so.
pub const MOST_CHARS: usize = 20_000;

/// Cleans text for speaking: one space between words, paragraphs kept as blank lines, no stray symbols, cut at a sentence end
/// when it is too long. Returns the text and whether it was cut.
pub fn prepare_text(raw: &str, most: usize) -> (String, bool) {
    let mut paras: Vec<String> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !cur.is_empty() {
                paras.push(cur.join(" "));
                cur.clear();
            }
        } else {
            cur.push(t);
        }
    }
    if !cur.is_empty() {
        paras.push(cur.join(" "));
    }
    let text = paras.iter().map(|p| p.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n\n");
    if text.chars().count() <= most {
        return (text, false);
    }
    let head: String = text.chars().take(most).collect();
    let cut = head.rfind(['.', '!', '?', '\n']).filter(|i| *i > most / 2).map(|i| i + 1).unwrap_or(head.len());
    (head[..cut].trim().to_string(), true)
}

/// Windows' own notification state (2 a full-screen program, 3 a full-screen Direct3D game, 4 presentation): the signal `busy.rs` uses.
/// A game in front means no synthetic keys and no reading: BYTE must not press Ctrl+C into a game.
pub fn game_in_front(shell_state: Option<i32>) -> bool {
    matches!(shell_state, Some(2) | Some(3))
}

#[cfg(windows)]
pub fn shell_state() -> Option<i32> {
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    unsafe { SHQueryUserNotificationState().ok().map(|s| s.0) }
}

#[cfg(not(windows))]
pub fn shell_state() -> Option<i32> {
    None
}

pub const GAME_SAY: &str = "A full-screen game or program is in front, so BYTE won't press keys or read it. Switch to the page or text first.";

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_is_tidied_and_paragraphs_kept() {
        let (t, cut) = prepare_text("  Hello   there.\nThis is one\nparagraph.\n\n\n  Next   one.  ", 1000);
        assert_eq!(t, "Hello there. This is one paragraph.\n\nNext one.");
        assert!(!cut);
    }

    #[test]
    fn a_long_text_is_cut_at_a_sentence_end() {
        let s = "One two three. ".repeat(100);
        let (t, cut) = prepare_text(&s, 200);
        assert!(cut);
        assert!(t.chars().count() <= 200);
        assert!(t.ends_with('.'), "{t}");
    }

    #[test]
    fn a_fullscreen_game_or_program_blocks_reading() {
        assert!(game_in_front(Some(3)));
        assert!(game_in_front(Some(2)));
        assert!(!game_in_front(Some(1)));
        assert!(!game_in_front(Some(4)));
        assert!(!game_in_front(None));
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(prepare_text("  \n \n", 100), (String::new(), false));
        assert_eq!(word_count("a b  c"), 3);
    }
}
