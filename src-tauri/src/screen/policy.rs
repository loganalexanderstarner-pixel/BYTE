//! "May I look at this window?" A pure decision, so it is easy to test and hard to get wrong.
//!
//! Never captured: BYTE's own window, password managers, banking, private browser windows, and anything the person adds to
//! the list in Settings (a part of the program name or of the window title, any case). While a private chat is open nothing
//! is captured at all.

/// What is known about the window in front.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowInfo {
    /// The program's file name without ".exe".
    pub program: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Private,
    NoWindow,
    OwnWindow,
    Excluded(String),
}

impl Refusal {
    /// In plain words, for the person.
    pub fn say(&self) -> String {
        match self {
            Refusal::Private => "This is a private chat, so BYTE doesn't look at your screen.".into(),
            Refusal::NoWindow => "BYTE couldn't tell which window is in front.".into(),
            Refusal::OwnWindow => "BYTE's own window is in front. Click the app or page you mean, then ask again.".into(),
            Refusal::Excluded(w) => format!("{w} is on your never-look-at list, so BYTE didn't look."),
        }
    }
}

/// Programs and titles never captured, until the person changes the list.
pub fn default_excludes() -> Vec<String> {
    [
        "1password", "bitwarden", "keepass", "keepassxc", "lastpass", "dashlane", "nordpass", "enpass", "roboform", "proton pass", "keeper",
        "authy", "credential manager", "windows security", "incognito", "inprivate", "private browsing", "private window",
        "bank", "banking", "paypal", "venmo", "chase", "wells fargo", "capital one", "credit union", "turbotax",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// `needle` occurs in `hay` not inside a longer word ("chase" is not in "purchase", but is in "Chase Online").
fn has_word(hay: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let start = from + i;
        let end = start + needle.len();
        let before = hay[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = hay[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = start + hay[start..].chars().next().map_or(1, |c| c.len_utf8());
    }
    false
}

/// Programs that are a terminal or console: a synthetic Ctrl+C there stops the running command, so BYTE never sends keys to them.
const TERMINALS: &[&str] = &[
    "cmd", "powershell", "powershell_ise", "pwsh", "windowsterminal", "wt", "conhost", "openconsole", "ssh", "putty", "kitty", "mintty", "bash", "sh", "zsh", "wsl", "wslhost", "wslg", "git-bash",
    "alacritty", "wezterm", "wezterm-gui", "hyper", "tabby", "termius", "mobaxterm", "cmder", "conemu64", "conemu",
];
/// Editors and IDEs: they have a terminal pane that may have the focus, and BYTE cannot tell which pane it is, so they are read from a picture only.
const EDITORS: &[&str] = &["code", "code - insiders", "cursor", "windsurf", "devenv", "idea64", "pycharm64", "clion64", "rider64", "webstorm64", "goland64", "studio64", "zed", "sublime_text"];
const TERMINAL_TITLES: &[&str] = &["command prompt", "windows powershell", "windows terminal", "powershell", "git bash", "mingw", "ubuntu", "wsl"];

/// May BYTE press keys (Ctrl+C) in this window? Not in a terminal or console (it would interrupt the running program), and not in an
/// editor where a terminal pane may be focused. Unsure means no: the text is then read from a picture instead.
pub fn may_send_keys(w: &WindowInfo) -> bool {
    let program = w.program.trim().to_lowercase();
    let title = w.title.to_lowercase();
    if TERMINALS.contains(&program.as_str()) || EDITORS.contains(&program.as_str()) {
        return false;
    }
    !TERMINAL_TITLES.iter().any(|t| has_word(&title, t))
}

/// One top-level window, as listed top to bottom in the Z-order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Candidate {
    pub id: isize,
    pub info: WindowInfo,
    pub visible: bool,
    pub minimized: bool,
    pub tool_window: bool,
    /// Hidden by Windows without being "not visible" (another virtual desktop, a suspended app).
    pub cloaked: bool,
    /// The desktop, taskbar and other shell windows.
    pub shell: bool,
}

fn is_own(program: &str) -> bool {
    ALWAYS.iter().any(|a| program.trim().eq_ignore_ascii_case(a))
}

/// Which window to look at. `list` is every top-level window from the top of the Z-order down, the foreground one first.
/// If the window in front is not BYTE's own it is the target as it is, even if it is on the never-look-at list: the rules
/// then refuse it, and a lower window is never looked at instead. If BYTE's own window is in front (a question typed in BYTE),
/// the target is the first visible, titled, ordinary window below it: not BYTE's, not a tool window or overlay, not the shell,
/// not minimised. Returns the index in `list`.
pub fn pick_target(list: &[Candidate]) -> Option<usize> {
    let first = list.first()?;
    if !is_own(&first.info.program) {
        return Some(0);
    }
    list.iter().enumerate().skip(1).find(|(_, c)| {
        c.visible && !c.minimized && !c.tool_window && !c.cloaked && !c.shell && !c.info.title.trim().is_empty() && !is_own(&c.info.program)
    }).map(|(i, _)| i)
}

/// Always refused, whatever the list says.
const ALWAYS: &[&str] = &["byte"];

/// Decides whether `win` may be captured. `private`: a private chat is open.
pub fn may_capture(win: Option<&WindowInfo>, excludes: &[String], private: bool) -> Result<(), Refusal> {
    if private {
        return Err(Refusal::Private);
    }
    let Some(w) = win else { return Err(Refusal::NoWindow) };
    if w.program.is_empty() && w.title.is_empty() {
        return Err(Refusal::NoWindow);
    }
    let program = w.program.trim().to_lowercase();
    if ALWAYS.iter().any(|a| program == *a) {
        return Err(Refusal::OwnWindow);
    }
    let title = w.title.to_lowercase();
    for e in excludes.iter().map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()) {
        if has_word(&program, &e) || has_word(&title, &e) {
            return Err(Refusal::Excluded(if w.program.is_empty() { w.title.clone() } else { w.program.clone() }));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(program: &str, title: &str) -> WindowInfo {
        WindowInfo { program: program.into(), title: title.into() }
    }

    #[test]
    fn an_ordinary_window_is_allowed() {
        assert_eq!(may_capture(Some(&win("Code", "main.rs - editor")), &default_excludes(), false), Ok(()));
        assert_eq!(may_capture(Some(&win("CitiesSkylines", "Cities: Skylines")), &default_excludes(), false), Ok(()));
    }

    #[test]
    fn a_private_chat_blocks_everything() {
        assert_eq!(may_capture(Some(&win("Code", "x")), &[], true), Err(Refusal::Private));
    }

    #[test]
    fn password_managers_banks_and_private_windows_are_refused_by_default() {
        let d = default_excludes();
        for (p, t) in [
            ("1Password", "Vault"),
            ("Bitwarden", ""),
            ("KeePassXC", "db"),
            ("chrome", "New Incognito tab - Google Chrome"),
            ("msedge", "InPrivate - Microsoft Edge"),
            ("firefox", "Mozilla Firefox Private Browsing"),
            ("chrome", "Chase Online - Sign in"),
            ("chrome", "Online Banking | My Credit Union"),
        ] {
            assert!(matches!(may_capture(Some(&win(p, t)), &d, false), Err(Refusal::Excluded(_))), "{p} / {t}");
        }
    }

    #[test]
    fn the_persons_own_list_works_in_any_case_and_ignores_blank_entries() {
        let list = vec!["  ".to_string(), "Discord".to_string()];
        assert!(may_capture(Some(&win("discord", "General")), &list, false).is_err());
        assert!(may_capture(Some(&win("Slack", "Discord invite")), &list, false).is_err());
        assert!(may_capture(Some(&win("Slack", "General")), &list, false).is_ok());
    }

    #[test]
    fn the_apps_own_window_and_an_unknown_one_are_refused() {
        assert_eq!(may_capture(Some(&win("byte", "BYTE")), &[], false), Err(Refusal::OwnWindow));
        assert_eq!(may_capture(None, &[], false), Err(Refusal::NoWindow));
        assert_eq!(may_capture(Some(&win("", "")), &[], false), Err(Refusal::NoWindow));
    }

    #[test]
    fn matching_is_by_whole_word_so_purchase_is_not_chase() {
        let d = default_excludes();
        assert!(may_capture(Some(&win("chrome", "Your purchase history - Shop")), &d, false).is_ok());
        assert!(may_capture(Some(&win("chrome", "Bankruptcy law explained")), &d, false).is_ok());
        assert!(may_capture(Some(&win("chrome", "Chase Online - Sign in")), &d, false).is_err());
        assert!(may_capture(Some(&win("KeePassXC", "")), &d, false).is_err());
    }

    fn cand(id: isize, program: &str, title: &str) -> Candidate {
        Candidate { id, info: win(program, title), visible: true, ..Default::default() }
    }

    #[test]
    fn terminals_and_editors_never_get_keys() {
        for p in ["cmd", "powershell", "pwsh", "WindowsTerminal", "wt", "conhost", "ssh", "PuTTY", "mintty", "bash", "wsl", "Code", "devenv", "idea64"] {
            assert!(!may_send_keys(&win(p, "x")), "{p}");
        }
        assert!(!may_send_keys(&win("explorer", "Administrator: Windows PowerShell")));
        assert!(!may_send_keys(&win("somehost", "Command Prompt")));
        assert!(may_send_keys(&win("chrome", "A news page - Chrome")));
        assert!(may_send_keys(&win("notepad", "notes.txt")));
        assert!(may_send_keys(&win("winword", "Ubuntu-review.docx")) == false, "a title naming a terminal is enough to be careful");
    }

    #[test]
    fn a_normal_front_window_is_the_target_even_if_excluded() {
        let l = vec![cand(1, "bitwarden", "Vault"), cand(2, "game", "Cities")];
        assert_eq!(pick_target(&l), Some(0), "never skip a refused window to look at the one below");
    }

    #[test]
    fn with_the_app_in_front_the_next_ordinary_window_below_is_chosen() {
        let mut tool = cand(2, "overlay", "Overlay");
        tool.tool_window = true;
        let mut min = cand(3, "chrome", "Minimised");
        min.minimized = true;
        let mut shell = cand(4, "explorer", "Program Manager");
        shell.shell = true;
        let mut cloaked = cand(5, "calc", "Calculator");
        cloaked.cloaked = true;
        let mut hidden = cand(6, "x", "Hidden");
        hidden.visible = false;
        let l = vec![cand(1, "byte", "BYTE"), cand(7, "BYTE", "BYTE quick ask"), tool, min, shell, cloaked, hidden, cand(8, "", "  "), cand(9, "game", "Cities: Skylines"), cand(10, "chrome", "Other")];
        assert_eq!(pick_target(&l), Some(8));
    }

    #[test]
    fn only_the_app_itself_or_an_empty_list_has_no_target() {
        assert_eq!(pick_target(&[]), None);
        assert_eq!(pick_target(&[cand(1, "byte", "BYTE")]), None);
    }

    #[test]
    fn an_excluded_window_below_byte_is_still_refused_by_the_rules() {
        let l = vec![cand(1, "byte", "BYTE"), cand(2, "1Password", "Vault")];
        let i = pick_target(&l).unwrap();
        assert!(may_capture(Some(&l[i].info), &default_excludes(), false).is_err());
    }

    #[test]
    fn an_empty_list_still_refuses_byte_and_private() {
        assert!(may_capture(Some(&win("notepad", "x")), &[], false).is_ok());
    }
}
