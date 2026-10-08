//! Text written for a Mac, worded for the machine BYTE is on. The Rust twin of
//! `src/lib/helpText.ts` and `osText()` in `src/lib/platform.ts`: the help articles are shown by
//! the help center (TypeScript) and offered to the model (here), and both must say the same
//! thing, so the rules are the same and each side has tests. Keep them in step.
//!
//! Where an article's steps differ, it has platform blocks, each marker on a line of its own:
//! `<!-- mac -->`, `<!-- win -->`, `<!-- linux -->`, `<!-- pc -->` (Windows and Linux) and `<!-- all -->` (the
//! default at the top of a file).

/// The computer BYTE is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    /// This build's operating system. A Mac when it is neither (the tests on other systems pass an `Os` explicitly).
    pub fn this() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Mac
        }
    }
}

/// The text with only the blocks for this platform, and no markers: `mac`, `win`, `linux`, `pc` (Windows and Linux)
/// and `all`.
pub fn for_platform(text: &str, os: Os) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum Mode {
        All,
        Mac,
        Win,
        Linux,
        Pc,
    }
    let mut mode = Mode::All;
    let mut out: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        match line.trim() {
            "<!-- mac -->" => mode = Mode::Mac,
            "<!-- win -->" => mode = Mode::Win,
            "<!-- linux -->" => mode = Mode::Linux,
            "<!-- pc -->" => mode = Mode::Pc,
            "<!-- all -->" => mode = Mode::All,
            _ => {
                let shown = match mode {
                    Mode::All => true,
                    Mode::Mac => os == Os::Mac,
                    Mode::Win => os == Os::Windows,
                    Mode::Linux => os == Os::Linux,
                    Mode::Pc => os != Os::Mac,
                };
                if shown {
                    out.push(line);
                }
            }
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
    os_text_for(text, Os::Windows)
}

/// `os_text` for any system: the identity on a Mac; on Linux the same swaps, with the file manager, the system
/// keyring and the account password where Windows has File Explorer, Credential Manager and Windows Hello.
/// Keep in step with `osText` in src/lib/platform.ts.
pub fn os_text_for(text: &str, os: Os) -> String {
    if os == Os::Mac {
        return text.to_string();
    }
    let linux = os == Os::Linux;
    let keyring = if linux { "your system keyring" } else { "Windows Credential Manager" };
    let mut s = text.to_string();
    for lead in ["Your", "your", "The", "the", "This", "this"] {
        for mac in ["Mac", "macOS"] {
            s = s.replace(&format!("{lead} {mac}'s Keychain"), keyring);
        }
    }
    s = s.replace("the macOS Keychain", keyring).replace("your Keychain", keyring).replace("Your Keychain", keyring);
    s = replace_word(&s, "Keychain", if linux { "system keyring" } else { "Credential Manager" });
    s = s.replace("Apple Silicon GPU", "graphics card or processor");
    s = s.replace("Show in Finder", if linux { "Show in file manager" } else { "Show in File Explorer" });
    s = replace_word(&s, "Finder", if linux { "file manager" } else { "File Explorer" });
    for (from, to) in [("menu bar", "system tray"), ("menu-bar", "system tray"), ("Menu bar", "System tray"), ("Menu-bar", "System tray")] {
        s = s.replace(from, to);
    }
    s = s.replace("Touch ID", if linux { "your account password" } else { "Windows Hello" }).replace("Mac control", "PC control");
    s = s.replace("System Settings → Accessibility → Display", if linux { "your desktop's accessibility settings" } else { "Settings → Accessibility → Visual effects" });
    s = s.replace("System Settings", "Settings").replace("Privacy & Security", if linux { "Privacy" } else { "Privacy & security" });
    s = replace_word(&s, "macOS", if linux { "Linux" } else { "Windows" });
    s = replace_word(&s, "Macs", "PCs");
    replace_word(&s, "Mac", "PC")
}

/// Text the app wrote for a Mac, worded for the machine it is on: the identity on a Mac,
/// "this PC" for "this Mac" on Windows. Only for the app's own sentences, never for what a
/// person or a model wrote, and for the fixed part of a message rather than the whole of one
/// that has a model or file name in it.
pub fn here(text: impl AsRef<str>) -> String {
    os_text_for(text.as_ref(), Os::this())
}

/// An article as the reader on this machine should see it.
pub fn localize(text: &str, os: Os) -> String {
    let picked = for_platform(text, os);
    if os == Os::Mac {
        picked
    } else {
        os_text_for(&keys(&picked), os)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_pick_the_platform() {
        let t = "before\n<!-- mac -->\nmac only\n<!-- win -->\nwindows only\n<!-- all -->\nafter";
        assert_eq!(for_platform(t, Os::Mac), "before\nmac only\nafter");
        assert_eq!(for_platform(t, Os::Windows), "before\nwindows only\nafter");
        assert_eq!(for_platform(t, Os::Linux), "before\nafter");
        // A block with no end runs to the end of the file.
        assert_eq!(for_platform("a\n<!-- mac -->\nb\nc", Os::Mac), "a\nb\nc");
        assert_eq!(for_platform("a\n<!-- mac -->\nb\nc", Os::Windows), "a");
        // `pc` is Windows and Linux; `linux` is Linux alone.
        let t = "x\n<!-- pc -->\nboth\n<!-- linux -->\nlinux only\n<!-- all -->\ny";
        assert_eq!(for_platform(t, Os::Mac), "x\ny");
        assert_eq!(for_platform(t, Os::Windows), "x\nboth\ny");
        assert_eq!(for_platform(t, Os::Linux), "x\nboth\nlinux only\ny");
    }

    #[test]
    fn a_mac_reader_gets_the_text_unchanged() {
        assert_eq!(localize("Press ⌘K on your Mac.", Os::Mac), "Press ⌘K on your Mac.");
    }

    #[test]
    fn a_windows_reader_gets_windows_words() {
        assert_eq!(localize("Press ⌘K on your Mac.", Os::Windows), "Press Ctrl+K on your PC.");
        assert_eq!(os_text("Open System Settings → Privacy & Security."), "Open Settings → Privacy & security.");
        assert_eq!(os_text("Lock BYTE with Touch ID; it can sit in the menu bar. Menu bar too."), "Lock BYTE with Windows Hello; it can sit in the system tray. System tray too.");
        assert_eq!(os_text("Stored in your Mac's Keychain, or your Keychain, or the macOS Keychain"), "Stored in Windows Credential Manager, or Windows Credential Manager, or Windows Credential Manager");
        assert_eq!(os_text("Show in Finder, Macs and Mac's GPU, macOS"), "Show in File Explorer, PCs and PC's GPU, Windows");
    }

    #[test]
    fn a_linux_reader_gets_linux_words() {
        assert_eq!(localize("Press ⌘K on your Mac.", Os::Linux), "Press Ctrl+K on your PC.");
        assert_eq!(os_text_for("Show in Finder, Finder, macOS, Touch ID", Os::Linux), "Show in file manager, file manager, Linux, your account password");
        assert_eq!(os_text_for("Stored in your Mac's Keychain or the macOS Keychain; Keychain too", Os::Linux), "Stored in your system keyring or your system keyring; system keyring too");
        assert_eq!(os_text_for("Open System Settings → Privacy & Security.", Os::Linux), "Open Settings → Privacy.");
        assert_eq!(os_text_for("Lock in the menu bar", Os::Linux), "Lock in the system tray");
        // A Mac is never reworded.
        assert_eq!(os_text_for("Show in Finder on your Mac", Os::Mac), "Show in Finder on your Mac");
    }

    #[test]
    fn words_that_only_contain_mac_are_left_alone() {
        assert_eq!(os_text("A machine, a macro, Macbeth and the Mac."), "A machine, a macro, Macbeth and the PC.");
        assert_eq!(os_text("Keychains and Finders"), "Keychains and Finders");
    }
}
