//! Reading the front window as a list of things that can be clicked ("set of marks"), instead of looking at pixels.
//! The types and the text the model reads are platform-free; `WinScreen` (Windows UI Automation) is the only part that
//! touches Windows, and sits behind the `Screen` trait so everything above it is tested with a fake.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn center(&self) -> (i32, i32) {
        ((self.left + self.right) / 2, (self.top + self.bottom) / 2)
    }
    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Button,
    Edit,
    CheckBox,
    RadioButton,
    ComboBox,
    ListItem,
    MenuItem,
    TabItem,
    Link,
    TreeItem,
    Slider,
    Other,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Button => "button",
            Kind::Edit => "edit",
            Kind::CheckBox => "checkbox",
            Kind::RadioButton => "radio",
            Kind::ComboBox => "dropdown",
            Kind::ListItem => "item",
            Kind::MenuItem => "menu",
            Kind::TabItem => "tab",
            Kind::Link => "link",
            Kind::TreeItem => "tree",
            Kind::Slider => "slider",
            Kind::Other => "item",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Element {
    /// 1, 2, 3... in reading order of the window's tree. The same window read twice gives the same numbers.
    pub id: u32,
    pub name: String,
    pub kind: Kind,
    pub enabled: bool,
    pub rect: Rect,
    /// A password or card field: never shown to a model, never highlighted, never clicked.
    pub sensitive: bool,
}

/// Names of fields that hold secrets, whatever the program calls them.
pub fn sensitive_name(name: &str) -> bool {
    let l = name.to_lowercase();
    ["password", "passcode", "passphrase", "card number", "credit card", "debit card", "cvv", "cvc", "security code", "expiration", "expiry", "ssn", "social security", "routing number", "account number", "one-time code", "verification code", "secret"].iter().any(|w| l.contains(w))
        || l.split(|c: char| !c.is_alphanumeric()).any(|w| w == "pin")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Snapshot {
    /// The program's name (no path), lower-case: "notepad".
    pub app: String,
    pub title: String,
    pub elements: Vec<Element>,
}

impl Snapshot {
    pub fn find(&self, id: u32) -> Option<&Element> {
        self.elements.iter().find(|e| e.id == id)
    }

    /// An element whose name contains `text` (any case).
    pub fn find_named(&self, text: &str) -> Option<&Element> {
        let t = text.to_lowercase();
        self.elements.iter().find(|e| !e.sensitive && e.name.to_lowercase().contains(&t))
    }
}

/// Most elements shown to a model: more than this is noise that costs time.
pub const MAX_MARKS: usize = 80;
const MAX_NAME: usize = 60;

fn clip(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}...", one.chars().take(n).collect::<String>())
    }
}

/// The compact text a model reads:
/// ```text
/// Window "Untitled - Notepad" (notepad)
/// 1 button "File"
/// 2 edit "Text editor"
/// ```
/// Unnamed elements are kept only when they are fields to type in. Disabled ones say so.
pub fn render(s: &Snapshot) -> String {
    let mut out = format!("Window \"{}\" ({})\n", clip(&s.title, 80), s.app);
    let mut shown = 0usize;
    let mut skipped = 0usize;
    for e in &s.elements {
        if e.sensitive {
            continue;
        }
        let name = clip(&e.name, MAX_NAME);
        if name.is_empty() && e.kind != Kind::Edit {
            continue;
        }
        if shown >= MAX_MARKS {
            skipped += 1;
            continue;
        }
        shown += 1;
        out.push_str(&format!("{} {}{}{}\n", e.id, e.kind.word(), if name.is_empty() { String::new() } else { format!(" \"{name}\"") }, if e.enabled { "" } else { " (disabled)" }));
    }
    if skipped > 0 {
        out.push_str(&format!("... and {skipped} more not shown\n"));
    }
    if shown == 0 {
        out.push_str("(nothing clickable was found in this window)\n");
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenError {
    /// Nothing is in front, or it can't be read (a secure desktop, an elevated window).
    NoWindow,
    NotFound(String),
    Failed(String),
    /// Not on this system.
    Unsupported,
}

impl std::fmt::Display for ScreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScreenError::NoWindow => f.write_str("there is no window BYTE can read in front"),
            ScreenError::NotFound(w) => write!(f, "{w} was not found"),
            ScreenError::Failed(m) => f.write_str(m),
            ScreenError::Unsupported => f.write_str("this needs Windows"),
        }
    }
}

/// One top-level window, as the window list gives it (top of the stack first).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WinRef {
    /// The window handle, as a number.
    pub id: isize,
    pub app: String,
    pub title: String,
    /// The window class ("Shell_TrayWnd" for the taskbar).
    pub class: String,
    pub minimised: bool,
    pub tool: bool,
    pub cloaked: bool,
}

/// Classes of the desktop shell, never a window the person means.
const SHELL_CLASSES: &[&str] = &["Shell_TrayWnd", "Shell_SecondaryTrayWnd", "Progman", "WorkerW", "NotifyIconOverflowWindow", "TopLevelWindowForOverflowXamlIsland", "XamlExplorerHostIslandWindow"];
/// BYTE's own small windows: the guide overlay and the listening pill.
const OWN_TITLES: &[&str] = &["byte guide", "byte listening"];

/// The window a how-to question is about when BYTE's own chat is in front: the first one down the stack that is a real,
/// visible, titled program window, skipping BYTE's own windows, tool windows, the shell, minimised and cloaked ones.
pub fn pick_target(windows: &[WinRef]) -> Option<&WinRef> {
    windows.iter().find(|w| {
        let own = w.app.eq_ignore_ascii_case("byte") || OWN_TITLES.contains(&w.title.trim().to_lowercase().as_str());
        !own && !w.tool && !w.minimised && !w.cloaked && !w.title.trim().is_empty() && !SHELL_CLASSES.contains(&w.class.as_str())
    })
}

/// The window of an app the person named ("notepad", "edge"), wherever it is in the stack (even minimised): the topmost one whose
/// program or title contains the name, never BYTE's own, a tool window or the shell.
pub fn pick_named<'a>(windows: &'a [WinRef], name: &str) -> Option<&'a WinRef> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    windows.iter().find(|w| {
        let own = w.app.eq_ignore_ascii_case("byte") || OWN_TITLES.contains(&w.title.trim().to_lowercase().as_str());
        !own && !w.tool && !w.cloaked && !w.title.trim().is_empty() && !SHELL_CLASSES.contains(&w.class.as_str()) && (w.app.to_lowercase().contains(&n) || w.title.to_lowercase().contains(&n))
    })
}

/// What the executor needs to see and operate. Real: Windows UI Automation. Tests: a fake.
pub trait Screen: Send + Sync {
    fn snapshot(&self) -> Result<Snapshot, ScreenError>;
    /// Presses the element (UI Automation invoke; its middle with the mouse when it can't be invoked).
    fn click(&self, el: &Element) -> Result<(), ScreenError>;
    /// Brings the window whose title contains `title` to the front; returns the title it found.
    fn focus_window(&self, title: &str) -> Result<String, ScreenError>;
    /// Titles of the open top-level windows.
    fn window_titles(&self) -> Vec<String>;
    /// Asks the window to close (like its X button).
    fn close_window(&self, title: &str) -> Result<(), ScreenError>;
    /// The program (lower-case, no .exe) and title of the window in front: a cheap look, no element list.
    fn foreground(&self) -> Result<(String, String), ScreenError> {
        self.snapshot().map(|s| (s.app, s.title))
    }
    /// Top-level windows, top of the stack first.
    fn windows(&self) -> Vec<WinRef> {
        vec![]
    }
    /// That one window as marks, in front or not (so a walk-through keeps reading the window it was made for).
    fn snapshot_window(&self, _id: isize) -> Result<Snapshot, ScreenError> {
        Err(ScreenError::Unsupported)
    }
    /// The window whose title contains `title` (not necessarily in front), as marks. Used to follow a guided walk-through.
    fn snapshot_titled(&self, _title: &str) -> Result<Snapshot, ScreenError> {
        Err(ScreenError::Unsupported)
    }
}

#[cfg(windows)]
pub use win::WinScreen;

#[cfg(windows)]
mod win {
    use super::*;
    use windows::core::BSTR;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::*;

    pub struct WinScreen {
        input: Box<dyn super::super::input::Input>,
    }

    impl WinScreen {
        pub fn new(input: Box<dyn super::super::input::Input>) -> Self {
            WinScreen { input }
        }
    }

    fn fail<E: std::fmt::Display>(e: E) -> ScreenError {
        ScreenError::Failed(e.to_string())
    }

    fn automation() -> Result<IUIAutomation, ScreenError> {
        super::super::com::ensure().map_err(ScreenError::Failed)?;
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(fail)
    }

    fn title_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn process_name(hwnd: HWND) -> String {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let Ok(h) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return String::new() };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(h) };
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let file = path.rsplit('\\').next().unwrap_or("").to_lowercase();
        file.strip_suffix(".exe").unwrap_or(&file).to_string()
    }

    fn kind_of(t: UIA_CONTROLTYPE_ID) -> Option<Kind> {
        Some(match t {
            UIA_ButtonControlTypeId | UIA_SplitButtonControlTypeId => Kind::Button,
            UIA_EditControlTypeId | UIA_DocumentControlTypeId => Kind::Edit,
            UIA_CheckBoxControlTypeId => Kind::CheckBox,
            UIA_RadioButtonControlTypeId => Kind::RadioButton,
            UIA_ComboBoxControlTypeId => Kind::ComboBox,
            UIA_ListItemControlTypeId | UIA_DataItemControlTypeId => Kind::ListItem,
            UIA_MenuItemControlTypeId => Kind::MenuItem,
            UIA_TabItemControlTypeId => Kind::TabItem,
            UIA_HyperlinkControlTypeId => Kind::Link,
            UIA_TreeItemControlTypeId => Kind::TreeItem,
            UIA_SliderControlTypeId => Kind::Slider,
            _ => return None,
        })
    }

    /// Reads the actionable elements of a window in tree order.
    fn read(auto: &IUIAutomation, hwnd: HWND) -> Result<Vec<(Element, IUIAutomationElement)>, ScreenError> {
        let root = unsafe { auto.ElementFromHandle(hwnd) }.map_err(fail)?;
        let cond = unsafe { auto.CreateTrueCondition() }.map_err(fail)?;
        let all = unsafe { root.FindAll(TreeScope_Subtree, &cond) }.map_err(fail)?;
        let n = unsafe { all.Length() }.map_err(fail)?;
        let mut out = Vec::new();
        for i in 0..n.min(4000) {
            let Ok(el) = (unsafe { all.GetElement(i) }) else { continue };
            let Ok(t) = (unsafe { el.CurrentControlType() }) else { continue };
            let Some(kind) = kind_of(t) else { continue };
            if unsafe { el.CurrentIsOffscreen() }.map(|b| b.as_bool()).unwrap_or(false) {
                continue;
            }
            let r = unsafe { el.CurrentBoundingRectangle() }.unwrap_or_default();
            let rect = Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
            if rect.is_empty() {
                continue;
            }
            let name = unsafe { el.CurrentName() }.map(|b: BSTR| b.to_string()).unwrap_or_default();
            let enabled = unsafe { el.CurrentIsEnabled() }.map(|b| b.as_bool()).unwrap_or(true);
            let sensitive = unsafe { el.CurrentIsPassword() }.map(|b| b.as_bool()).unwrap_or(false) || sensitive_name(&name);
            out.push((Element { id: out.len() as u32 + 1, name, kind, enabled, rect, sensitive }, el));
        }
        Ok(out)
    }

    fn front() -> Result<HWND, ScreenError> {
        let h = unsafe { GetForegroundWindow() };
        if h.0.is_null() {
            return Err(ScreenError::NoWindow);
        }
        Ok(h)
    }

    struct Found {
        needle: String,
        hit: Option<(HWND, String)>,
        all: Vec<String>,
        collect_all: bool,
    }

    unsafe extern "system" fn each_window(h: HWND, l: LPARAM) -> BOOL {
        let f = &mut *(l.0 as *mut Found);
        if !IsWindowVisible(h).as_bool() {
            return BOOL(1);
        }
        let t = title_of(h);
        if t.is_empty() {
            return BOOL(1);
        }
        if f.collect_all {
            f.all.push(t.clone());
        }
        if f.hit.is_none() && !f.needle.is_empty() && t.to_lowercase().contains(&f.needle) {
            f.hit = Some((h, t));
        }
        BOOL(1)
    }

    fn find_window(title: &str) -> Option<(HWND, String)> {
        let mut f = Found { needle: title.to_lowercase(), hit: None, all: vec![], collect_all: false };
        let _ = unsafe { EnumWindows(Some(each_window), LPARAM(&mut f as *mut Found as isize)) };
        f.hit
    }

    impl Screen for WinScreen {
        fn snapshot(&self) -> Result<Snapshot, ScreenError> {
            let auto = automation()?;
            let hwnd = front()?;
            let els = read(&auto, hwnd)?;
            Ok(Snapshot { app: process_name(hwnd), title: title_of(hwnd), elements: els.into_iter().map(|(e, _)| e).collect() })
        }

        fn foreground(&self) -> Result<(String, String), ScreenError> {
            let h = front()?;
            Ok((process_name(h), title_of(h)))
        }

        fn windows(&self) -> Vec<WinRef> {
            use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
            let mut out: Vec<WinRef> = Vec::new();
            let mut handles: Vec<HWND> = Vec::new();
            unsafe extern "system" fn grab(h: HWND, l: LPARAM) -> BOOL {
                let v = &mut *(l.0 as *mut Vec<HWND>);
                if IsWindowVisible(h).as_bool() {
                    v.push(h);
                }
                BOOL(1)
            }
            let _ = unsafe { EnumWindows(Some(grab), LPARAM(&mut handles as *mut Vec<HWND> as isize)) };
            for h in handles {
                let mut class = [0u16; 128];
                let n = unsafe { GetClassNameW(h, &mut class) };
                let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
                let mut cloaked = 0u32;
                let _ = unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4) };
                out.push(WinRef {
                    id: h.0 as isize,
                    app: process_name(h),
                    title: title_of(h),
                    class: String::from_utf16_lossy(&class[..n.max(0) as usize]),
                    minimised: unsafe { IsIconic(h) }.as_bool(),
                    tool: ex & WS_EX_TOOLWINDOW.0 != 0,
                    cloaked: cloaked != 0,
                });
            }
            out
        }

        fn snapshot_window(&self, id: isize) -> Result<Snapshot, ScreenError> {
            let auto = automation()?;
            let hwnd = HWND(id as *mut core::ffi::c_void);
            if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                return Err(ScreenError::NotFound("that window".into()));
            }
            let els = read(&auto, hwnd)?;
            Ok(Snapshot { app: process_name(hwnd), title: title_of(hwnd), elements: els.into_iter().map(|(e, _)| e).collect() })
        }

        fn snapshot_titled(&self, title: &str) -> Result<Snapshot, ScreenError> {
            let auto = automation()?;
            let Some((hwnd, t)) = find_window(title) else { return Err(ScreenError::NotFound(format!("a window titled \"{title}\""))) };
            let els = read(&auto, hwnd)?;
            Ok(Snapshot { app: process_name(hwnd), title: t, elements: els.into_iter().map(|(e, _)| e).collect() })
        }

        fn click(&self, el: &Element) -> Result<(), ScreenError> {
            let auto = automation()?;
            let hwnd = front()?;
            let els = read(&auto, hwnd)?;
            // The same numbering again; refuse when the window changed under the model.
            let Some((_, live)) = els.iter().find(|(e, _)| e.id == el.id && e.name == el.name && e.kind == el.kind) else {
                return Err(ScreenError::NotFound(format!("item {} (\"{}\")", el.id, el.name)));
            };
            if let Ok(p) = unsafe { live.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) } {
                if unsafe { p.Invoke() }.is_ok() {
                    return Ok(());
                }
            }
            if let Ok(p) = unsafe { live.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) } {
                if unsafe { p.Select() }.is_ok() {
                    return Ok(());
                }
            }
            if let Ok(p) = unsafe { live.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) } {
                if unsafe { p.Toggle() }.is_ok() {
                    return Ok(());
                }
            }
            if let Ok(p) = unsafe { live.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId) } {
                if unsafe { p.Expand() }.is_ok() {
                    return Ok(());
                }
            }
            if el.kind == Kind::Edit {
                let _ = unsafe { live.SetFocus() };
                return Ok(());
            }
            let (x, y) = el.rect.center();
            self.input.click_at(x, y).map_err(|e| ScreenError::Failed(e.to_string()))
        }

        fn focus_window(&self, title: &str) -> Result<String, ScreenError> {
            let Some((h, t)) = find_window(title) else { return Err(ScreenError::NotFound(format!("a window titled \"{title}\""))) };
            unsafe {
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                // Windows only lets the foreground program take the front; a tap of Alt lets this one.
                let _ = self.input.key_combo(&super::super::actions::parse_combo("alt").unwrap_or(super::super::actions::Combo { mods: vec![], key: super::super::actions::KeyCode::Named("tab") }));
                let _ = SetForegroundWindow(h);
                let _ = SetFocus(Some(h));
            }
            Ok(t)
        }

        fn window_titles(&self) -> Vec<String> {
            let mut f = Found { needle: String::new(), hit: None, all: vec![], collect_all: true };
            let _ = unsafe { EnumWindows(Some(each_window), LPARAM(&mut f as *mut Found as isize)) };
            f.all
        }

        fn close_window(&self, title: &str) -> Result<(), ScreenError> {
            let Some((h, _)) = find_window(title) else { return Err(ScreenError::NotFound(format!("a window titled \"{title}\""))) };
            unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0)) }.map_err(fail)
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;

    /// A scripted screen: each `snapshot()` returns the next entry (the last one repeats). Clicks and focuses are recorded.
    pub struct FakeScreen {
        pub shots: Mutex<Vec<Snapshot>>,
        pub log: Mutex<Vec<String>>,
        pub windows: Mutex<Vec<String>>,
        pub fail_click: Mutex<u32>,
    }

    impl FakeScreen {
        pub fn new(shots: Vec<Snapshot>) -> Self {
            FakeScreen { shots: Mutex::new(shots), log: Mutex::default(), windows: Mutex::default(), fail_click: Mutex::new(0) }
        }
    }

    pub fn el(id: u32, name: &str, kind: Kind) -> Element {
        Element { id, name: name.into(), kind, enabled: true, rect: Rect { left: 0, top: 0, right: 10, bottom: 10 }, sensitive: false }
    }

    pub fn snap(title: &str, els: Vec<Element>) -> Snapshot {
        Snapshot { app: "notepad".into(), title: title.into(), elements: els }
    }

    impl Screen for FakeScreen {
        fn snapshot(&self) -> Result<Snapshot, ScreenError> {
            let mut s = self.shots.lock().unwrap();
            if s.len() > 1 {
                Ok(s.remove(0))
            } else {
                s.first().cloned().ok_or(ScreenError::NoWindow)
            }
        }
        fn click(&self, el: &Element) -> Result<(), ScreenError> {
            let mut f = self.fail_click.lock().unwrap();
            if *f > 0 {
                *f -= 1;
                return Err(ScreenError::Failed("not clickable".into()));
            }
            self.log.lock().unwrap().push(format!("click {}", el.id));
            Ok(())
        }
        fn focus_window(&self, title: &str) -> Result<String, ScreenError> {
            self.log.lock().unwrap().push(format!("focus {title}"));
            Ok(title.to_string())
        }
        fn window_titles(&self) -> Vec<String> {
            self.windows.lock().unwrap().clone()
        }
        fn close_window(&self, title: &str) -> Result<(), ScreenError> {
            self.log.lock().unwrap().push(format!("close {title}"));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    #[test]
    fn the_window_is_rendered_as_numbered_marks() {
        let s = snap("Untitled - Notepad", vec![el(1, "File", Kind::MenuItem), el(2, "", Kind::Edit), el(3, "", Kind::Button), Element { enabled: false, ..el(4, "Undo", Kind::Button) }]);
        let t = render(&s);
        assert_eq!(t, "Window \"Untitled - Notepad\" (notepad)\n1 menu \"File\"\n2 edit\n4 button \"Undo\" (disabled)\n");
    }

    #[test]
    fn long_windows_are_cut_and_say_so() {
        let els: Vec<Element> = (1..=100).map(|i| el(i, &format!("Item {i}"), Kind::ListItem)).collect();
        let t = render(&snap("Big", els));
        assert_eq!(t.lines().count(), 1 + MAX_MARKS + 1);
        assert!(t.ends_with("... and 20 more not shown\n"));
    }

    #[test]
    fn names_are_one_line_and_short() {
        let t = render(&snap("W", vec![el(1, "a\n  very   long\tname that goes on and on and on and on and on and on and on and on", Kind::Button)]));
        let line = t.lines().nth(1).unwrap();
        assert!(line.starts_with("1 button \"a very long name"));
        assert!(line.ends_with("...\""), "{line}");
    }

    #[test]
    fn secret_fields_are_hidden_from_the_marks_and_the_name_search() {
        let mut s = snap("Sign in", vec![el(1, "Email", Kind::Edit), el(2, "Password", Kind::Edit), el(3, "Sign in", Kind::Button)]);
        s.elements[1].sensitive = sensitive_name("Password");
        assert!(s.elements[1].sensitive);
        let t = render(&s);
        assert!(!t.contains("Password") && t.contains("Email") && t.contains("Sign in"), "{t}");
        assert!(s.find_named("password").is_none());
        for n in ["Card number", "CVV", "Security code", "Enter your PIN", "Social Security number"] {
            assert!(sensitive_name(n), "{n}");
        }
        for n in ["Pinterest", "Shipping address", "Save", "Spinner", "Account"] {
            assert!(!sensitive_name(n), "{n}");
        }
    }

    fn win(id: isize, app: &str, title: &str) -> WinRef {
        WinRef { id, app: app.into(), title: title.into(), class: "Chrome_WidgetWin_1".into(), ..Default::default() }
    }

    #[test]
    fn the_target_is_the_first_real_window_below_byte() {
        let list = vec![
            win(1, "byte", "BYTE"),
            WinRef { class: "Shell_TrayWnd".into(), ..win(2, "explorer", "") },
            win(3, "byte", "BYTE guide"),
            WinRef { tool: true, ..win(4, "somepalette", "Tools") },
            WinRef { minimised: true, ..win(5, "notepad", "Notes") },
            WinRef { cloaked: true, ..win(6, "calculator", "Calculator") },
            win(7, "chrome", "  "),
            win(8, "chrome", "Shop - My account"),
            win(9, "code", "main.rs"),
        ];
        assert_eq!(pick_target(&list).map(|w| w.id), Some(8));
        // With the program already in front it is simply that one.
        assert_eq!(pick_target(&list[7..]).map(|w| w.id), Some(8));
    }

    #[test]
    fn a_named_app_is_found_wherever_it_is_in_the_stack() {
        let list = vec![
            win(1, "byte", "BYTE"),
            win(2, "windowsterminal", "C:\\dev\\build"),
            WinRef { minimised: true, ..win(3, "notepad", "server.properties - Notepad") },
            WinRef { cloaked: true, ..win(4, "calculator", "Calculator") },
            win(5, "msedge", "GitHub - Microsoft Edge"),
        ];
        assert_eq!(pick_named(&list, "notepad").map(|w| w.id), Some(3), "a minimised one counts");
        assert_eq!(pick_named(&list, "Edge").map(|w| w.id), Some(5));
        assert!(pick_named(&list, "calculator").is_none(), "a hidden (cloaked) one does not");
        assert!(pick_named(&list, "byte").is_none(), "never BYTE itself");
        assert!(pick_named(&list, "oven").is_none());
        assert!(pick_named(&list, "").is_none());
    }

    #[test]
    fn no_window_to_look_at_is_none() {
        let list = vec![win(1, "byte", "BYTE"), WinRef { class: "Progman".into(), ..win(2, "explorer", "Program Manager") }, WinRef { class: "WorkerW".into(), ..win(3, "explorer", "x") }];
        assert!(pick_target(&list).is_none());
        assert!(pick_target(&[]).is_none());
    }

    #[test]
    fn an_empty_window_says_so() {
        assert!(render(&snap("W", vec![])).contains("nothing clickable"));
    }

    #[test]
    fn finding_by_id_and_name() {
        let s = snap("W", vec![el(1, "Save", Kind::Button), el(2, "Save As...", Kind::Button)]);
        assert_eq!(s.find(2).unwrap().name, "Save As...");
        assert_eq!(s.find_named("SAVE as").unwrap().id, 2);
        assert!(s.find(9).is_none());
    }

    /// Needs an interactive desktop with a window in front, which the PC test harness does not have.
    #[cfg(windows)]
    #[test]
    #[ignore = "needs an interactive desktop: reads the real front window"]
    fn live_reads_the_front_window() {
        let s = WinScreen::new(Box::new(super::super::input::WinInput));
        let shot = s.snapshot().expect("snapshot");
        println!("{}", render(&shot));
        assert!(!shot.title.is_empty());
    }
}
