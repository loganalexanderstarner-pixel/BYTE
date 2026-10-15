//! The typed list of things BYTE may do to a PC by itself, how a model's JSON is read into it (strictly: anything unknown is
//! an error, never a guess), and how risky each one is. Pure logic: no Windows calls, so it is tested everywhere.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most steps in one plan: a model that wants more should plan again from what it sees.
pub const MAX_STEPS: usize = 12;
/// Longest text typed in one step.
pub const MAX_TYPE_CHARS: usize = 2000;
pub const MIN_WAIT_MS: u64 = 100;
pub const MAX_WAIT_MS: u64 = 30_000;
pub const DEFAULT_WAIT_MS: u64 = 5_000;

/// What a Wait step waits for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitFor {
    /// A window whose title contains the text.
    Window,
    /// An element in the front window whose name contains the text.
    Element,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerKind {
    Shutdown,
    Restart,
    SignOut,
}

/// One step. The names are what the model writes in `"action"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Click (invoke) the numbered element of the front window.
    Click { id: u32 },
    /// Type text into whatever has the keyboard focus.
    Type { text: String },
    /// Press a key or a combination: "ctrl+s", "enter", "alt+tab".
    Key { combo: String },
    /// Bring the window whose title contains this text to the front.
    Focus { title: String },
    /// Open a program by name, a web address, or a `ms-settings:` page.
    Open { target: String },
    /// Wait (no longer than `timeout_ms`) for a window or an element to show up.
    Wait {
        #[serde(rename = "for")]
        what: WaitFor,
        text: String,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    Scroll { direction: Direction, #[serde(default)] amount: Option<u32> },
    /// Ask the window whose title contains this text to close (a program with unsaved work asks the person itself).
    Close { title: String },
    Power { kind: PowerKind },
}

/// What should be true after a step, when the model knows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expect {
    /// The front window's title contains this.
    pub window: Option<String>,
    /// The front window has an element whose name contains this.
    pub element: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub action: Action,
    pub expect: Expect,
}

/// How much trust a step needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    Safe,
    /// The person approves it on a card first.
    Asks,
    /// Refused outright, whatever the model says.
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError(pub String);

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlanError {}

fn err<T>(m: impl Into<String>) -> Result<T, PlanError> {
    Err(PlanError(m.into()))
}

// ------------------------------------------------------------------ keys

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Win,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    F(u8),
    Named(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub mods: Vec<Modifier>,
    pub key: KeyCode,
}

const NAMED: &[&str] = &[
    "enter", "tab", "esc", "space", "backspace", "delete", "home", "end", "pageup", "pagedown", "up", "down", "left", "right", "insert",
    // Media keys: the same ones a keyboard has.
    "playpause", "nexttrack", "prevtrack", "volumeup", "volumedown", "volumemute", "printscreen",
];

/// "Ctrl+Shift+S" -> modifiers and a key. Only keys on a normal keyboard; anything else is an error.
pub fn parse_combo(text: &str) -> Result<Combo, PlanError> {
    let norm = text.trim().to_lowercase();
    if norm.is_empty() {
        return err("an empty key combination");
    }
    // "+" alone or "ctrl++" mean the plus key.
    let parts: Vec<&str> = if norm == "+" { vec!["+"] } else if let Some(p) = norm.strip_suffix("++") { let mut v: Vec<&str> = p.split('+').collect(); v.push("+"); v } else { norm.split('+').collect() };
    let mut mods = Vec::new();
    let mut key: Option<KeyCode> = None;
    for raw in parts {
        let p = raw.trim();
        let m = match p {
            "ctrl" | "control" => Some(Modifier::Ctrl),
            "shift" => Some(Modifier::Shift),
            "alt" => Some(Modifier::Alt),
            "win" | "windows" | "super" | "meta" => Some(Modifier::Win),
            _ => None,
        };
        if let Some(m) = m {
            if mods.contains(&m) {
                return err(format!("\"{text}\" repeats a modifier"));
            }
            mods.push(m);
            continue;
        }
        if key.is_some() {
            return err(format!("\"{text}\" has more than one main key"));
        }
        let k = if let Some(n) = NAMED.iter().find(|n| **n == p) {
            KeyCode::Named(n)
        } else if let Some(num) = p.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=12).contains(n)) {
            KeyCode::F(num)
        } else if p.chars().count() == 1 && p.chars().all(|c| c.is_ascii_alphanumeric() || "+-=,./;'[]`\\".contains(c)) {
            KeyCode::Char(p.chars().next().unwrap_or(' '))
        } else {
            return err(format!("\"{p}\" is not a key BYTE can press"));
        };
        key = Some(k);
    }
    match key {
        Some(key) => Ok(Combo { mods, key }),
        None => err(format!("\"{text}\" has no main key")),
    }
}

// ------------------------------------------------------------------ opening

const NEVER_PROGRAMS: &[&str] = &[
    "cmd", "cmd.exe", "powershell", "powershell.exe", "pwsh", "pwsh.exe", "wscript", "cscript", "mshta", "regedit", "reg", "diskpart", "format", "rundll32", "wmic", "msiexec", "bcdedit", "sc", "net", "schtasks", "certutil", "bitsadmin", "wt", "bash", "wsl", "conhost", "taskkill", "cipher", "vssadmin",
];
const NEVER_EXT: &[&str] = &[
    "exe", "lnk", "url", "appx", "msix", "appxbundle", "msixbundle", "jnlp", "application", "msi", "msp", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "scr", "com", "pif", "reg", "hta", "cpl", "msc", "dll", "jar", "gadget", "inf", "sys", "ocx", "chm",
];
/// Links that are known to the opener at all. Only `http` and `https` are `Safe`; every other one asks first (`steam://run/...` starts programs).
const KNOWN_SCHEMES: &[&str] = &["http", "https", "mailto", "ms-settings", "spotify", "steam", "discord", "tel"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// "notepad", "spotify": a program by the name a person says.
    Program(String),
    Uri(String),
    /// A full path to a file or program.
    Path(String),
}

/// What an `open` target is, or why it is refused. Programs that run commands, scripts and installers never open.
pub fn classify_target(raw: &str) -> Result<Target, PlanError> {
    let t = raw.trim();
    if t.is_empty() {
        return err("nothing to open");
    }
    if t.chars().count() > 500 || t.chars().any(|c| c.is_control()) {
        return err("that is not something BYTE can open");
    }
    let low = t.to_lowercase();
    // A scheme: letters, digits, +-. before a colon, at least two characters (one letter is a drive).
    if let Some((scheme, _)) = low.split_once(':') {
        if scheme.len() >= 2 && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
            return if KNOWN_SCHEMES.contains(&scheme) { Ok(Target::Uri(t.to_string())) } else { err(format!("BYTE won't open {scheme}: links")) };
        }
        let b = low.as_bytes();
        if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
            return path_target(t);
        }
        return err("that is not something BYTE can open");
    }
    if low.starts_with("\\\\") || low.starts_with("//") {
        return err("BYTE won't open network locations");
    }
    if low.contains('\\') || low.contains('/') {
        return path_target(t);
    }
    // A bare name.
    let stem = low.strip_suffix(".exe").unwrap_or(&low);
    if NEVER_PROGRAMS.contains(&stem) || NEVER_PROGRAMS.contains(&low.as_str()) {
        return err(format!("BYTE won't start {t}"));
    }
    if let Some(ext) = low.rsplit_once('.').map(|(_, e)| e) {
        if NEVER_EXT.contains(&ext) {
            return err(format!("BYTE won't open .{ext} files"));
        }
    }
    if !t.chars().all(|c| c.is_alphanumeric() || " .-_&'+".contains(c)) {
        return err("that is not a program name");
    }
    Ok(Target::Program(t.to_string()))
}

fn path_target(t: &str) -> Result<Target, PlanError> {
    let low = t.to_lowercase();
    if low.split(['\\', '/']).any(|p| p == "..") {
        return err("BYTE won't open a path with ..");
    }
    if let Some(ext) = low.rsplit_once('.').map(|(_, e)| e) {
        if NEVER_EXT.contains(&ext) {
            return err(format!("BYTE won't open .{ext} files"));
        }
    }
    let name = low.rsplit(['\\', '/']).next().unwrap_or("");
    let stem = name.strip_suffix(".exe").unwrap_or(name);
    if NEVER_PROGRAMS.contains(&stem) {
        return err(format!("BYTE won't start {name}"));
    }
    Ok(Target::Path(t.to_string()))
}

// ------------------------------------------------------------------ validation and risk

/// Checks one action on its own. Errors are worded for a model to fix its plan.
pub fn validate(a: &Action) -> Result<(), PlanError> {
    match a {
        Action::Click { id } if *id == 0 => err("click ids start at 1"),
        Action::Click { .. } => Ok(()),
        Action::Type { text } => {
            if text.is_empty() {
                err("type needs some text")
            } else if text.chars().count() > MAX_TYPE_CHARS {
                err(format!("type is limited to {MAX_TYPE_CHARS} characters"))
            } else if text.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                err("type can't contain control characters")
            } else {
                Ok(())
            }
        }
        Action::Key { combo } => parse_combo(combo).map(|_| ()),
        Action::Focus { title } | Action::Close { title } if title.trim().is_empty() || title.chars().count() > 200 => err("a window title is needed (up to 200 characters)"),
        Action::Focus { .. } | Action::Close { .. } => Ok(()),
        Action::Open { target } => classify_target(target).map(|_| ()),
        Action::Wait { text, timeout_ms, .. } => {
            if text.trim().is_empty() {
                return err("wait needs the text to wait for");
            }
            match timeout_ms {
                Some(t) if !(MIN_WAIT_MS..=MAX_WAIT_MS).contains(t) => err(format!("timeout_ms must be between {MIN_WAIT_MS} and {MAX_WAIT_MS}")),
                _ => Ok(()),
            }
        }
        Action::Scroll { amount, .. } => match amount {
            Some(n) if !(1..=50).contains(n) => err("scroll amount is 1 to 50"),
            _ => Ok(()),
        },
        Action::Power { .. } => Ok(()),
    }
}

/// Wait time for a Wait step.
pub fn wait_ms(a: &Action) -> u64 {
    match a {
        Action::Wait { timeout_ms, .. } => timeout_ms.unwrap_or(DEFAULT_WAIT_MS).clamp(MIN_WAIT_MS, MAX_WAIT_MS),
        _ => 0,
    }
}

/// Words on a button that make clicking it a decision the person should make.
const RISKY_LABELS: &[&str] = &[
    "send", "buy", "purchase", "pay", "place order", "order now", "checkout", "check out", "confirm", "delete", "remove", "erase", "format", "uninstall", "shut down", "shutdown", "restart", "sign out", "log out", "don't save", "do not save", "discard", "empty recycle", "submit", "transfer", "subscribe", "post", "publish",
];

/// Whether an element's name is one of the decisions above ("Send", "Buy now", "Delete all").
pub fn risky_label(name: &str) -> bool {
    let low = name.to_lowercase();
    let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).collect();
    RISKY_LABELS.iter().any(|l| {
        let lw: Vec<&str> = l.split(' ').collect();
        words.windows(lw.len()).any(|w| w == lw.as_slice())
    })
}

/// How risky the step is by itself. A Click's risk also depends on the element's label (`risky_label`), which only the
/// executor can see.
pub fn risk(a: &Action) -> Risk {
    match a {
        Action::Power { .. } | Action::Close { .. } => Risk::Asks,
        Action::Open { target } => match classify_target(target) {
            Err(_) => Risk::Never,
            Ok(Target::Path(_)) => Risk::Asks,
            Ok(Target::Uri(u)) => {
                let low = u.to_lowercase();
                if low.starts_with("http://") || low.starts_with("https://") {
                    Risk::Safe
                } else {
                    Risk::Asks
                }
            }
            Ok(Target::Program(_)) => Risk::Safe,
        },
        Action::Key { combo } => match parse_combo(combo) {
            Err(_) => Risk::Never,
            Ok(c) => {
                let ctrl_alt = c.mods.contains(&Modifier::Ctrl) && c.mods.contains(&Modifier::Alt);
                if ctrl_alt && c.key == KeyCode::Named("delete") {
                    Risk::Never
                } else if c.mods.contains(&Modifier::Alt) && c.key == KeyCode::F(4) {
                    Risk::Asks
                } else if c.mods.contains(&Modifier::Shift) && c.key == KeyCode::Named("delete") || c.mods.contains(&Modifier::Ctrl) && c.mods.contains(&Modifier::Shift) && c.key == KeyCode::Named("delete") {
                    Risk::Asks
                } else if c.mods.is_empty() && c.key == KeyCode::Named("delete") {
                    Risk::Asks
                } else if c.mods.contains(&Modifier::Win) && c.key == KeyCode::Char('r') {
                    // The Run box takes any command, with any extra modifier too.
                    Risk::Never
                } else if c.mods.contains(&Modifier::Win) && c.key == KeyCode::Char('x') || c.mods.contains(&Modifier::Ctrl) && c.mods.contains(&Modifier::Shift) && c.key == KeyCode::Named("esc") {
                    // The power-user menu and Task Manager.
                    Risk::Asks
                } else {
                    Risk::Safe
                }
            }
        },
        Action::Type { .. } | Action::Click { .. } | Action::Focus { .. } | Action::Wait { .. } | Action::Scroll { .. } => Risk::Safe,
    }
}

/// A short sentence for the approval card and the activity log.
pub fn describe(a: &Action) -> String {
    match a {
        Action::Click { id } => format!("Click item {id}"),
        Action::Type { text } => {
            let shown: String = text.chars().take(40).collect();
            format!("Type \"{shown}{}\"", if text.chars().count() > 40 { "..." } else { "" })
        }
        Action::Key { combo } => format!("Press {combo}"),
        Action::Focus { title } => format!("Switch to \"{title}\""),
        Action::Open { target } => format!("Open {target}"),
        Action::Wait { what, text, .. } => format!("Wait for {} \"{text}\"", if *what == WaitFor::Window { "the window" } else { "the item" }),
        Action::Scroll { direction, .. } => format!("Scroll {}", if *direction == Direction::Up { "up" } else { "down" }),
        Action::Close { title } => format!("Close \"{title}\""),
        Action::Power { kind } => match kind {
            PowerKind::Shutdown => "Shut the PC down".into(),
            PowerKind::Restart => "Restart the PC".into(),
            PowerKind::SignOut => "Sign out".into(),
        },
    }
}

// ------------------------------------------------------------------ reading the model's JSON

/// The first complete JSON object or array in the text (models wrap them in prose or code fences).
pub fn extract_json(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|b| *b == b'{' || *b == b'[')?;
    let (open, close) = if bytes[start] == b'{' { (b'{', b'}') } else { (b'[', b']') };
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        if b == b'"' {
            in_str = true;
        } else if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(&text[start..=i]);
            }
        }
    }
    None
}

fn step_from(v: &Value, n: usize) -> Result<Step, PlanError> {
    let Value::Object(map) = v else { return err(format!("step {n} is not an object")) };
    let mut map = map.clone();
    let expect = match map.remove("expect") {
        None | Some(Value::Null) => Expect::default(),
        Some(e) => {
            if let Value::Object(o) = &e {
                if let Some(k) = o.keys().find(|k| *k != "window" && *k != "element") {
                    return err(format!("step {n}: expect has an unknown field \"{k}\""));
                }
            }
            serde_json::from_value(e).map_err(|e| PlanError(format!("step {n}: expect is wrong ({e})")))?
        }
    };
    let Some(name) = map.get("action").and_then(Value::as_str) else { return err(format!("step {n} has no \"action\"")) };
    let name = name.to_string();
    let action: Action = serde_json::from_value(Value::Object(map)).map_err(|e| PlanError(format!("step {n} (\"{name}\") is not valid: {e}")))?;
    validate(&action).map_err(|e| PlanError(format!("step {n}: {e}")))?;
    if risk(&action) == Risk::Never {
        return err(format!("step {n}: BYTE never does this ({})", describe(&action)));
    }
    Ok(Step { action, expect })
}

/// Reads a plan: `{"steps":[...]}`, a bare array, or one step. Hard errors for anything unknown or refused.
pub fn parse_plan(text: &str) -> Result<Vec<Step>, PlanError> {
    parse_plan_with(text, MAX_STEPS)
}

/// `parse_plan` with the person's own limit on steps per plan (Settings → PC control).
pub fn parse_plan_with(text: &str, max_steps: usize) -> Result<Vec<Step>, PlanError> {
    let Some(json) = extract_json(text) else { return err("no JSON plan found") };
    let v: Value = serde_json::from_str(json).map_err(|e| PlanError(format!("the plan is not valid JSON: {e}")))?;
    let list: Vec<Value> = match v {
        Value::Array(a) => a,
        Value::Object(ref o) if o.contains_key("steps") => match o.get("steps") {
            // Other top-level fields ("reason", "note"…) are only commentary a small model adds; they can carry no action, and
            // every step is still checked strictly below.
            Some(Value::Array(a)) => a.clone(),
            _ => return err("\"steps\" must be a list"),
        },
        Value::Object(_) => vec![v],
        _ => return err("the plan must be an object or a list"),
    };
    if list.is_empty() {
        return err("the plan has no steps");
    }
    if list.len() > max_steps {
        return err(format!("the plan has {} steps; at most {max_steps}", list.len()));
    }
    list.iter().enumerate().map(|(i, s)| step_from(s, i + 1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commentary_next_to_the_steps_is_ignored_but_the_steps_stay_strict() {
        let ok = r#"{"reason": "the font lives in Settings", "steps": [{"action": "open", "target": "notepad"}]}"#;
        assert_eq!(parse_plan_with(ok, 12).unwrap().len(), 1);
        let bad = r#"{"reason": "x", "steps": [{"action": "open", "target": "notepad", "sneaky": true}]}"#;
        assert!(parse_plan_with(bad, 12).is_err(), "an unknown field inside a step is still refused");
    }

    #[test]
    fn a_plan_is_read_from_fenced_or_wrapped_json() {
        let t = "Sure!\n```json\n{\"steps\":[{\"action\":\"focus\",\"title\":\"Notepad\"},{\"action\":\"type\",\"text\":\"hi {there}\"},{\"action\":\"key\",\"combo\":\"Ctrl+S\",\"expect\":{\"window\":\"Save As\"}}]}\n```";
        let p = parse_plan(t).unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].action, Action::Type { text: "hi {there}".into() });
        assert_eq!(p[2].expect.window.as_deref(), Some("Save As"));
    }

    #[test]
    fn a_bare_array_and_a_single_step_work() {
        assert_eq!(parse_plan("[{\"action\":\"click\",\"id\":4}]").unwrap().len(), 1);
        assert_eq!(parse_plan("{\"action\":\"open\",\"target\":\"notepad\"}").unwrap().len(), 1);
    }

    #[test]
    fn anything_unknown_is_a_hard_error() {
        for bad in [
            "{\"steps\":[{\"action\":\"format_disk\"}]}",
            "{\"steps\":[{\"action\":\"click\",\"id\":1,\"extra\":2}]}",
            "{\"steps\":[{\"action\":\"click\"}]}",
            "{\"steps\":[{\"id\":1}]}",
            "{\"steps\":[{\"action\":\"click\",\"id\":0}]}",
            "{\"steps\":[],\"x\":1}",
            "{\"steps\":[]}",
            "{\"steps\":[{\"action\":\"click\",\"id\":1,\"expect\":{\"bogus\":1}}]}",
            "{\"steps\":[{\"action\":\"key\",\"combo\":\"ctrl+banana\"}]}",
            "{\"steps\":[{\"action\":\"type\",\"text\":\"\"}]}",
            "no json at all",
            "{\"steps\":[{\"action\":\"click\",\"id\":-1}]}",
            "{\"steps\":\"click\"}",
            "{\"steps\":[7]}",
        ] {
            assert!(parse_plan(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn too_many_steps_are_refused() {
        let steps = vec!["{\"action\":\"scroll\",\"direction\":\"down\"}"; MAX_STEPS + 1].join(",");
        assert!(parse_plan(&format!("[{steps}]")).is_err());
        let ok = vec!["{\"action\":\"scroll\",\"direction\":\"down\"}"; MAX_STEPS].join(",");
        assert_eq!(parse_plan(&format!("[{ok}]")).unwrap().len(), MAX_STEPS);
    }

    #[test]
    fn combos_parse_and_reject() {
        assert_eq!(parse_combo("Ctrl+Shift+S").unwrap(), Combo { mods: vec![Modifier::Ctrl, Modifier::Shift], key: KeyCode::Char('s') });
        assert_eq!(parse_combo("f5").unwrap().key, KeyCode::F(5));
        assert_eq!(parse_combo("alt+tab").unwrap().key, KeyCode::Named("tab"));
        assert_eq!(parse_combo("ctrl++").unwrap().key, KeyCode::Char('+'));
        assert_eq!(parse_combo("playpause").unwrap().key, KeyCode::Named("playpause"));
        for bad in ["", "ctrl", "ctrl+ctrl+a", "a+b", "f13", "ctrl+", "hyper+x", "é"] {
            assert!(parse_combo(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn dangerous_things_never_open() {
        for t in ["cmd", "cmd.exe", "PowerShell", "powershell.exe", "regedit", "C:\\Windows\\System32\\cmd.exe", "run.bat", "C:\\x\\setup.msi", "script.ps1", "file:///C:/x", "javascript:alert(1)", "ms-msdt:/id", "search-ms:query=x", "\\\\server\\share\\a.exe", "C:\\a\\..\\b.exe", "notepad; calc", "a\nb", "data:text/html,x", ""] {
            assert!(classify_target(t).is_err(), "{t}");
        }
    }

    #[test]
    fn ordinary_things_open() {
        assert_eq!(classify_target("notepad").unwrap(), Target::Program("notepad".into()));
        assert_eq!(classify_target("Visual Studio Code").unwrap(), Target::Program("Visual Studio Code".into()));
        assert_eq!(classify_target("https://example.com/a?b=1").unwrap(), Target::Uri("https://example.com/a?b=1".into()));
        assert_eq!(classify_target("ms-settings:bluetooth").unwrap(), Target::Uri("ms-settings:bluetooth".into()));
        assert_eq!(classify_target("spotify:playlist:abc").unwrap(), Target::Uri("spotify:playlist:abc".into()));
        assert!(matches!(classify_target("C:\\Docs\\report.pdf").unwrap(), Target::Path(_)));
    }

    #[test]
    fn risk_classes() {
        let r = |j: &str| risk(&serde_json::from_str::<Action>(j).unwrap());
        assert_eq!(r("{\"action\":\"click\",\"id\":1}"), Risk::Safe);
        assert_eq!(r("{\"action\":\"type\",\"text\":\"hello\"}"), Risk::Safe);
        assert_eq!(r("{\"action\":\"power\",\"kind\":\"shutdown\"}"), Risk::Asks);
        assert_eq!(r("{\"action\":\"close\",\"title\":\"Notepad\"}"), Risk::Asks);
        assert_eq!(r("{\"action\":\"key\",\"combo\":\"alt+f4\"}"), Risk::Asks);
        assert_eq!(r("{\"action\":\"key\",\"combo\":\"delete\"}"), Risk::Asks);
        assert_eq!(r("{\"action\":\"key\",\"combo\":\"ctrl+alt+delete\"}"), Risk::Never);
        assert_eq!(r("{\"action\":\"key\",\"combo\":\"win+r\"}"), Risk::Never);
        assert_eq!(r("{\"action\":\"key\",\"combo\":\"ctrl+s\"}"), Risk::Safe);
        assert_eq!(r("{\"action\":\"open\",\"target\":\"cmd\"}"), Risk::Never);
        assert_eq!(r("{\"action\":\"open\",\"target\":\"C:\\\\a\\\\b.pdf\"}"), Risk::Asks);
        assert_eq!(r("{\"action\":\"open\",\"target\":\"notepad\"}"), Risk::Safe);
        assert!(Risk::Never > Risk::Asks && Risk::Asks > Risk::Safe);
    }

    #[test]
    fn programs_and_shortcuts_by_path_or_name_are_refused() {
        for ext in ["exe", "lnk", "url", "appx", "msix", "jnlp", "application", "msi", "bat", "cmd", "ps1", "vbs", "js", "jse", "wsf", "scr", "com", "pif", "reg", "hta", "cpl", "msc", "dll", "EXE"] {
            assert!(classify_target(&format!("C:\\x\\a.{ext}")).is_err(), "{ext} by path");
            assert!(classify_target(&format!("a.{ext}")).is_err(), "{ext} by name");
        }
    }

    #[test]
    fn only_web_links_are_safe_other_links_ask() {
        let r = |t: &str| risk(&Action::Open { target: t.into() });
        assert_eq!(r("https://example.com"), Risk::Safe);
        assert_eq!(r("http://example.com"), Risk::Safe);
        for t in ["steam://run/440", "discord://x", "spotify:track:abc", "tel:5551234", "mailto:a@b.c", "ms-settings:sound"] {
            assert_eq!(r(t), Risk::Asks, "{t}");
        }
        assert_eq!(r("notepad"), Risk::Safe);
    }

    #[test]
    fn win_r_is_never_in_any_combination_and_power_menus_ask() {
        for c in ["win+r", "win+shift+r", "ctrl+win+r", "win+alt+r"] {
            assert_eq!(risk(&Action::Key { combo: c.into() }), Risk::Never, "{c}");
        }
        for c in ["win+x", "ctrl+shift+esc", "win+shift+x"] {
            assert_eq!(risk(&Action::Key { combo: c.into() }), Risk::Asks, "{c}");
        }
        assert_eq!(risk(&Action::Key { combo: "win+d".into() }), Risk::Safe);
    }

    #[test]
    fn a_never_step_cannot_get_into_a_plan() {
        assert!(parse_plan("[{\"action\":\"key\",\"combo\":\"win+r\"}]").is_err());
        assert!(parse_plan("[{\"action\":\"open\",\"target\":\"powershell\"}]").is_err());
    }

    #[test]
    fn risky_labels_are_found_as_whole_words() {
        for l in ["Send", "Buy now", "Delete all files", "Don't save", "Place order", "Shut down", "Pay $5"] {
            assert!(risky_label(l), "{l}");
        }
        for l in ["Save", "Cancel", "Sender settings", "Compost", "Open", "Postal code", "Remove" /* removed below */] {
            if l == "Remove" {
                assert!(risky_label(l));
            } else {
                assert!(!risky_label(l), "{l}");
            }
        }
    }

    #[test]
    fn wait_is_clamped_and_checked() {
        let w = |t: Option<u64>| Action::Wait { what: WaitFor::Window, text: "x".into(), timeout_ms: t };
        assert_eq!(wait_ms(&w(None)), DEFAULT_WAIT_MS);
        assert_eq!(wait_ms(&w(Some(1))), MIN_WAIT_MS);
        assert!(validate(&w(Some(999_999))).is_err());
        assert!(validate(&w(Some(2000))).is_ok());
        assert!(parse_plan("[{\"action\":\"wait\",\"for\":\"window\",\"text\":\"Save As\"}]").is_ok());
        assert!(parse_plan("[{\"action\":\"wait\",\"for\":\"tab\",\"text\":\"x\"}]").is_err());
    }

    #[test]
    fn typing_is_limited() {
        assert!(validate(&Action::Type { text: "a\tb\nc".into() }).is_ok());
        assert!(validate(&Action::Type { text: "a\u{7}b".into() }).is_err());
        assert!(validate(&Action::Type { text: "x".repeat(MAX_TYPE_CHARS + 1) }).is_err());
    }

    #[test]
    fn json_is_found_inside_text_with_braces_in_strings() {
        assert_eq!(extract_json("x {\"a\":\"}{\"} y"), Some("{\"a\":\"}{\"}"));
        assert_eq!(extract_json("nothing"), None);
        assert_eq!(extract_json("{\"a\":"), None);
    }
}
