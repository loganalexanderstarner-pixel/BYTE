//! Text written for a Mac, worded for the machine BYTE is on. The Rust twin of
//! `src/lib/helpText.ts` and `osText()` in `src/lib/platform.ts`: the help articles are shown by
//! the help center (TypeScript) and offered to the model (here), and both must say the same
//! thing, so the rules are the same and each side has tests. Keep them in step.
//!
//! Where an article's steps differ, it has platform blocks, each marker on a line of its own:
//! `<!-- mac -->`, `<!-- win -->` and `<!-- all -->` (the default at the top of a file).

/// The text with only the blocks for this platform, and no markers.
pub fn for_platform(text: &str, windows: bool) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum Mode {
        All,
        Mac,
        Win,
    }
    let mut mode = Mode::All;
    let mut out: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        match line.trim() {
            "<!-- mac -->" => mode = Mode::Mac,
            "<!-- win -->" => mode = Mode::Win,
            "<!-- all -->" => mode = Mode::All,
            _ if mode == Mode::All || (mode == Mode::Win) == windows => out.push(line),
            _ => {}
        }
    }
    out.join("\n")
}

/// Mac key glyphs as Windows writes them: ⌘K → Ctrl+K.
pub fn keys(text: &str) -> String {
    text.replace('⌘', "Ctrl+").replace('⌃', "Ctrl+").replace('⌥', "Alt+").replace('⇧', "Shift+")
}

/// `word` replaced wherever it stands alone ("Mac" in "your Mac's GPU", not in "Machine").
fn replace_word(text: &str, word: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let word_char = |c: char| c.is_alphanumeric() || c == '_';
    while let Some(i) = rest.find(word) {
        let before_ok = !rest[..i].chars().next_back().is_some_and(word_char);
        let after = &rest[i + word.len()..];
        let after_ok = !after.chars().next().is_some_and(word_char);
        out.push_str(&rest[..i]);
        out.push_str(if before_ok && after_ok { to } else { word });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Reword Mac text for Windows: "this Mac" becomes "this PC", Finder File Explorer, the menu bar
/// the system tray, Keychain Windows' Credential Manager, Touch ID Windows Hello. Only for text
/// the app itself wrote. The rules apply in order, because "your Mac's Keychain" has to become
/// one phrase.
pub fn os_text(text: &str) -> String {
    let mut s = text.to_string();
    for lead in ["Your", "your", "The", "the", "This", "this"] {
        for mac in ["Mac", "macOS"] {
            s = s.replace(&format!("{lead} {mac}'s Keychain"), "Windows Credential Manager");
        }
    }
    s = s.replace("the macOS Keychain", "Windows Credential Manager").replace("your Keychain", "Windows Credential Manager").replace("Your Keychain", "Windows Credential Manager");
    s = replace_word(&s, "Keychain", "Credential Manager");
    s = s.replace("Apple Silicon GPU", "graphics card or processor");
    s = s.replace("Show in Finder", "Show in File Explorer");
    s = replace_word(&s, "Finder", "File Explorer");
    for (from, to) in [("menu bar", "system tray"), ("menu-bar", "system tray"), ("Menu bar", "System tray"), ("Menu-bar", "System tray")] {
        s = s.replace(from, to);
    }
    s = s.replace("Touch ID", "Windows Hello").replace("Mac control", "PC control");
    s = s.replace("System Settings → Accessibility → Display", "Settings → Accessibility → Visual effects");
    s = s.replace("System Settings", "Settings").replace("Privacy & Security", "Privacy & security");
    s = replace_word(&s, "macOS", "Windows");
    s = replace_word(&s, "Macs", "PCs");
    replace_word(&s, "Mac", "PC")
}

/// Text the app wrote for a Mac, worded for the machine it is on: the identity on a Mac,
/// "this PC" for "this Mac" on Windows. Only for the app's own sentences, never for what a
/// person or a model wrote, and for the fixed part of a message rather than the whole of one
/// that has a model or file name in it.
pub fn here(text: impl AsRef<str>) -> String {
    if cfg!(windows) {
        os_text(text.as_ref())
    } else {
        text.as_ref().to_string()
    }
}

/// An article as the reader on this machine should see it.
pub fn localize(text: &str, windows: bool) -> String {
    let picked = for_platform(text, windows);
    if windows {
        os_text(&keys(&picked))
    } else {
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_pick_the_platform() {
        let t = "before\n<!-- mac -->\nmac only\n<!-- win -->\nwindows only\n<!-- all -->\nafter";
        assert_eq!(for_platform(t, false), "before\nmac only\nafter");
        assert_eq!(for_platform(t, true), "before\nwindows only\nafter");
        // A block with no end runs to the end of the file.
        assert_eq!(for_platform("a\n<!-- mac -->\nb\nc", false), "a\nb\nc");
        assert_eq!(for_platform("a\n<!-- mac -->\nb\nc", true), "a");
    }

    #[test]
    fn a_mac_reader_gets_the_text_unchanged() {
        assert_eq!(localize("Press ⌘K on your Mac.", false), "Press ⌘K on your Mac.");
    }

    #[test]
    fn a_windows_reader_gets_windows_words() {
        assert_eq!(localize("Press ⌘K on your Mac.", true), "Press Ctrl+K on your PC.");
        assert_eq!(os_text("Open System Settings → Privacy & Security."), "Open Settings → Privacy & security.");
        assert_eq!(os_text("Lock BYTE with Touch ID; it can sit in the menu bar. Menu bar too."), "Lock BYTE with Windows Hello; it can sit in the system tray. System tray too.");
        assert_eq!(os_text("Stored in your Mac's Keychain, or your Keychain, or the macOS Keychain"), "Stored in Windows Credential Manager, or Windows Credential Manager, or Windows Credential Manager");
        assert_eq!(os_text("Show in Finder, Macs and Mac's GPU, macOS"), "Show in File Explorer, PCs and PC's GPU, Windows");
    }

    #[test]
    fn words_that_only_contain_mac_are_left_alone() {
        assert_eq!(os_text("A machine, a macro, Macbeth and the Mac."), "A machine, a macro, Macbeth and the PC.");
        assert_eq!(os_text("Keychains and Finders"), "Keychains and Finders");
    }
}
