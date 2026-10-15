//! The region screenshot helper: a hotkey puts a see-through window over the screen, the person drags a box, and BYTE reads
//! only that box. The picture is made in memory, turned into text and dropped at once; it is never saved or sent anywhere.
//! A small popup then offers: what is this / translate / copy the text / make a note / ask a question.
//!
//! The rules in `screen::policy` apply to every window under the box (not only the one in front): a password manager, a bank,
//! a private browser window or anything on the never-look-at list under any part of the box means nothing is captured.

use std::sync::Mutex;

use crate::screen::policy::{self, WindowInfo};

/// A box in screen pixels (the whole desktop, so a second monitor to the left has negative x).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Smallest box worth reading (a stray click is not a box).
pub const MIN_SIDE: i32 = 12;
/// Largest side, so a huge box cannot use a lot of memory.
pub const MAX_SIDE: i32 = 8_000;

pub fn supported() -> bool {
    cfg!(windows)
}

/// A drag from one corner to the other, in any direction, as a box. Too small means None.
pub fn normalise(a: (i32, i32), b: (i32, i32)) -> Option<Region> {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    let (w, h) = ((x1 - x0).min(MAX_SIDE), (y1 - y0).min(MAX_SIDE));
    (w >= MIN_SIDE && h >= MIN_SIDE).then_some(Region { x: x0, y: y0, w, h })
}

/// The points whose windows are checked against the rules: corners (just inside), edge middles and the centre.
pub fn sample_points(r: &Region) -> Vec<(i32, i32)> {
    let (l, t) = (r.x + 1, r.y + 1);
    let (rt, b) = (r.x + r.w - 2, r.y + r.h - 2);
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    vec![(l, t), (rt, t), (l, b), (rt, b), (cx, t), (cx, b), (l, cy), (rt, cy), (cx, cy)]
}

/// May this box be captured, given the windows found under its sample points (None = no window there)?
/// Every window must pass; BYTE's own windows are refused too (the box was drawn over BYTE).
pub fn may_capture_all(windows: &[Option<WindowInfo>], excludes: &[String], private: bool) -> Result<(), String> {
    if private {
        return Err(policy::Refusal::Private.say());
    }
    let mut seen_any = false;
    for w in windows.iter().flatten() {
        seen_any = true;
        policy::may_capture(Some(w), excludes, false).map_err(|r| r.say())?;
    }
    if !seen_any {
        return Err(policy::Refusal::NoWindow.say());
    }
    Ok(())
}

/// What the person has in hand after drawing a box: only text, never the picture.
#[derive(Debug, Clone, Default)]
pub struct Grabbed {
    pub text: String,
    /// The program of the window the box mostly covers.
    pub app: String,
    /// That window (0 when unknown), for guided help.
    pub hwnd: isize,
}

/// What the box is for: the usual choices, or "look at this page and help" (from the tray menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Menu,
    Page,
}

static MODE: Mutex<Mode> = Mutex::new(Mode::Menu);

static HELD: Mutex<Option<Grabbed>> = Mutex::new(None);

fn held() -> Option<Grabbed> {
    HELD.lock().ok()?.clone()
}

/// Forgets the text the popup holds.
pub fn forget() {
    if let Ok(mut h) = HELD.lock() {
        *h = None;
    }
}

// ---------------------------------------------------------------- what the buttons ask

/// The text of the box as content to look at.
fn wrap(text: &str) -> String {
    let (t, _) = super::explain::clip(text);
    format!("<<<\n{t}\n>>>")
}

pub fn what_prompt(text: &str) -> String {
    format!("This text was read from a part of the person's screen. Say what it is and what it means in plain words, and what they might do about it.\n\n{}", wrap(text))
}

pub fn translate_prompt(text: &str) -> String {
    format!("Translate the text between the markers into English. If it is already English, say so in one short sentence and translate it into Spanish instead. Reply with the translation only.\n\n{}", wrap(text))
}

pub fn question_prompt(text: &str, question: &str) -> String {
    format!("This text was read from a part of the person's screen:\n\n{}\n\nAnswer this question about it, using only that text, and say so if the text does not contain the answer.\n\nQuestion: {}", wrap(text), question.trim())
}

/// A short note title from the first words of the text.
pub fn note_title(text: &str) -> String {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("Screen note");
    let words: Vec<&str> = first.split_whitespace().take(7).collect();
    let t = words.join(" ");
    if t.chars().count() > 60 { t.chars().take(60).collect() } else { t }
}

/// The text as the person sees it in the popup's first line: short.
pub fn preview(text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= 220 { one } else { format!("{}...", one.chars().take(220).collect::<String>()) }
}

// ---------------------------------------------------------------- the desktop flow

#[cfg(desktop)]
mod flow {
    use super::*;
    use crate::helpers::popup::{self, Kind, Payload};
    use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

    pub const OVERLAY: &str = "helperregion";

    fn say(app: &AppHandle, body: String) {
        popup::show(app, Kind::Region, Payload { title: "Region screenshot".into(), body, buttons: vec![popup::button("close", "Close")], auto_hide: 10_000, ..Default::default() }, None);
    }

    pub fn start(app: &AppHandle, mode: Mode) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<crate::state::AppState>();
            {
                let s = state.settings.lock().await;
                let allowed = if mode == Mode::Page { s.helper_page } else { s.helper_region };
                if !allowed {
                    return;
                }
            }
            if let Ok(mut m) = MODE.lock() {
                *m = mode;
            }
            if let Some(why) = crate::helpers::blocked_now() {
                return say(&app, why.into());
            }
            if !supported() {
                return say(&app, crate::platform_text::here("Region screenshots aren't built for this system yet."));
            }
            if app.get_webview_window(OVERLAY).is_some() {
                return;
            }
            popup::hide(&app);
            forget();
            open_overlay(&app);
        });
    }

    fn open_overlay(app: &AppHandle) {
        let pointer = app.cursor_position().ok();
        let monitor = pointer.and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten()).or_else(|| app.primary_monitor().ok().flatten());
        let Some(m) = monitor else { return say(app, "BYTE couldn't tell which screen to use.".into()) };
        let built = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("helper-region.html".into()))
            .title("BYTE region")
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build();
        match built {
            Ok(w) => {
                let _ = w.set_position(PhysicalPosition::new(m.position().x, m.position().y));
                let _ = w.set_size(PhysicalSize::new(m.size().width, m.size().height));
                let _ = w.show();
                let _ = w.set_focus();
            }
            Err(e) => say(app, format!("BYTE couldn't open the selection window ({e}).")),
        }
    }

    fn close_overlay(app: &AppHandle) {
        if let Some(w) = app.get_webview_window(OVERLAY) {
            let _ = w.hide();
            let _ = w.close();
        }
    }

    /// The box was drawn: `a` and `b` are corners in CSS pixels inside the overlay.
    pub async fn done(app: AppHandle, a: (f64, f64), b: (f64, f64)) {
        let (origin, scale) = match app.get_webview_window(OVERLAY) {
            Some(w) => (w.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0)), w.scale_factor().unwrap_or(1.0)),
            None => return,
        };
        close_overlay(&app);
        let to_screen = |p: (f64, f64)| (origin.0 + (p.0 * scale).round() as i32, origin.1 + (p.1 * scale).round() as i32);
        let Some(region) = normalise(to_screen(a), to_screen(b)) else { return };
        // Let the overlay disappear before the screen is read.
        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
        let state = app.state::<crate::state::AppState>();
        let excludes = state.settings.lock().await.screen_exclude_apps.clone();
        let result = tokio::task::spawn_blocking(move || read(&region, &excludes)).await.unwrap_or_else(|_| Err("BYTE couldn't read that box.".into()));
        let at = (region.x + region.w, region.y + region.h);
        let mode = MODE.lock().map(|m| *m).unwrap_or(Mode::Menu);
        if mode == Mode::Page {
            return match result {
                Ok(g) => crate::helpers::page::after_region(&app, g.text, g.hwnd, at).await,
                Err(why) => popup::show(&app, Kind::Page, Payload { title: "Look at this page".into(), body: why, buttons: vec![popup::button("close", "Close")], auto_hide: 10_000, ..Default::default() }, Some(at)),
            };
        }
        match result {
            Ok(g) => {
                let none = g.text.trim().is_empty();
                let body = if none { "BYTE couldn't read any words in that box. It reads text, not pictures. Try a box around some writing.".to_string() } else { preview(&g.text) };
                let buttons = if none {
                    vec![popup::button("close", "Close")]
                } else {
                    vec![popup::button("what", "What's this?"), popup::button("translate", "Translate"), popup::button("copy", "Copy the text"), popup::button("note", "Make a note"), popup::button("ask", "Ask a question"), popup::button("close", "Close")]
                };
                if let Ok(mut h) = HELD.lock() {
                    *h = Some(g);
                }
                popup::show(&app, Kind::Region, Payload { title: if none { "Nothing to read".into() } else { "What would you like to do with this?".into() }, body, buttons, ..Default::default() }, Some(at));
            }
            Err(why) => popup::show(&app, Kind::Region, Payload { title: "Region screenshot".into(), body: why, buttons: vec![popup::button("close", "Close")], auto_hide: 10_000, ..Default::default() }, Some(at)),
        }
    }

    pub fn cancel(app: &AppHandle) {
        close_overlay(app);
    }

    #[cfg(windows)]
    fn read(region: &Region, excludes: &[String]) -> Result<Grabbed, String> {
        let windows = super::win::windows_under(&sample_points(region));
        may_capture_all(&windows, excludes, crate::screen::replay::is_private())?;
        let frame = super::win::grab(region)?;
        if frame.is_blank() {
            return Err("That part of the screen came back black. Some apps block screen capture.".into());
        }
        // The pixels live only in this function: they become text and are dropped.
        let jpeg = crate::screen::wgc::encode_jpeg(&frame, 85)?;
        drop(frame);
        let text = crate::ocr::image_text(&jpeg).map_err(|e| e.to_string())?;
        let app = windows.iter().flatten().next().map(|w| w.program.clone()).unwrap_or_default();
        let hwnd = super::win::root_at(((region.x + region.w / 2), (region.y + region.h / 2)));
        Ok(Grabbed { text: text.trim().to_string(), app, hwnd })
    }

    #[cfg(not(windows))]
    fn read(_region: &Region, _excludes: &[String]) -> Result<Grabbed, String> {
        Err(crate::platform_text::here("Region screenshots aren't built for this system yet."))
    }

    pub async fn clicked(app: &AppHandle, id: &str, typed: String) {
        let Some(g) = held() else { return popup::hide(app) };
        let working = |title: &str| popup::update(app, Kind::Region, Payload { title: title.into(), body: "Thinking...".into(), busy: true, ..Default::default() });
        match id {
            "copy" => {
                crate::clipboard::set_quietly(&g.text);
                forget();
                popup::hide(app);
            }
            "note" => {
                let state = app.state::<crate::state::AppState>();
                let msg = match crate::notes::root(app).await {
                    Ok(root) => {
                        let input = crate::notes::NoteInput { title: note_title(&g.text), folder: crate::notes::INBOX.into(), body: g.text.clone(), source: "screen region".into(), ..Default::default() };
                        let _ = &state;
                        match crate::notes::save(&root, &input, chrono::Utc::now().timestamp_millis()) {
                            Ok(n) => format!("Saved as a note: {}", n.title),
                            Err(e) => e.to_string(),
                        }
                    }
                    Err(e) => e.to_string(),
                };
                popup::update(app, Kind::Region, Payload { title: "Make a note".into(), body: msg, buttons: vec![popup::button("close", "Close")], auto_hide: 6_000, ..Default::default() });
            }
            "ask" => popup::update(
                app,
                Kind::Region,
                Payload { title: "Ask a question about this".into(), body: preview(&g.text), input: Some("Type your question".into()), buttons: vec![popup::button("send", "Ask"), popup::button("close", "Close")], ..Default::default() },
            ),
            "what" | "translate" | "send" => {
                if id == "send" && typed.trim().is_empty() {
                    return;
                }
                working(match id { "translate" => "Translate", "send" => "Your question", _ => "What's this?" });
                let prompt = match id {
                    "translate" => translate_prompt(&g.text),
                    "send" => question_prompt(&g.text, &typed),
                    _ => what_prompt(&g.text),
                };
                let state = app.state::<crate::state::AppState>();
                let payload = match crate::helpers::ask::run(&state, crate::helpers::ask::SYSTEM, &prompt, 500, true).await {
                    Ok((answer, place)) => {
                        let mut buttons = vec![popup::button("copyanswer", "Copy answer")];
                        if id == "send" {
                            buttons.push(popup::button("ask", "Another question"));
                        }
                        buttons.push(popup::button("close", "Close"));
                        LAST_ANSWER.lock().map(|mut l| *l = answer.clone()).ok();
                        Payload { title: format!("Answered on {place}"), body: answer, buttons, ..Default::default() }
                    }
                    Err(e) => Payload { title: "Region screenshot".into(), body: e.to_string(), buttons: vec![popup::button("close", "Close")], ..Default::default() },
                };
                popup::update(app, Kind::Region, payload);
            }
            "copyanswer" => {
                let a = LAST_ANSWER.lock().map(|l| l.clone()).unwrap_or_default();
                if !a.is_empty() {
                    crate::clipboard::set_quietly(&a);
                }
                popup::hide(app);
            }
            _ => {}
        }
    }

    static LAST_ANSWER: Mutex<String> = Mutex::new(String::new());
}

#[cfg(desktop)]
pub fn start(app: &tauri::AppHandle) {
    flow::start(app, Mode::Menu);
}
/// "Look at this page and help" from the tray menu.
#[cfg(desktop)]
pub fn start_page(app: &tauri::AppHandle) {
    flow::start(app, Mode::Page);
}
#[cfg(desktop)]
pub async fn clicked(app: &tauri::AppHandle, id: &str, typed: String) {
    flow::clicked(app, id, typed).await;
}
#[cfg(not(desktop))]
pub fn start(_app: &tauri::AppHandle) {}
#[cfg(not(desktop))]
pub async fn clicked(_app: &tauri::AppHandle, _id: &str, _typed: String) {}

/// The overlay page reports the dragged box (CSS pixels inside the overlay).
#[tauri::command]
pub async fn helper_region_done(app: tauri::AppHandle, x0: f64, y0: f64, x1: f64, y1: f64) {
    #[cfg(desktop)]
    flow::done(app, (x0, y0), (x1, y1)).await;
    #[cfg(not(desktop))]
    let _ = (app, x0, y0, x1, y1);
}

#[tauri::command]
pub fn helper_region_cancel(app: tauri::AppHandle) {
    #[cfg(desktop)]
    flow::cancel(&app);
    #[cfg(not(desktop))]
    let _ = app;
}

/// Windows: the windows under points on the screen, and a copy of a rectangle of the screen.
#[cfg(windows)]
mod win {
    use super::*;
    use crate::screen::frame::Frame;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HWND, POINT};
    use windows::Win32::Graphics::Gdi::{BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetWindowTextW, GetWindowThreadProcessId, WindowFromPoint, GA_ROOT};

    fn info(w: HWND) -> WindowInfo {
        let mut title = [0u16; 512];
        let n = unsafe { GetWindowTextW(w, &mut title) }.max(0) as usize;
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(w, Some(&mut pid)) };
        let mut program = String::new();
        if pid != 0 {
            if let Ok(p) = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if unsafe { QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok() {
                    program = std::path::Path::new(&String::from_utf16_lossy(&buf[..len as usize])).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                }
                let _ = unsafe { CloseHandle(p) };
            }
        }
        WindowInfo { program, title: String::from_utf16_lossy(&title[..n]) }
    }

    /// The top-level window at each point (None where there is none).
    pub fn windows_under(points: &[(i32, i32)]) -> Vec<Option<WindowInfo>> {
        points
            .iter()
            .map(|&(x, y)| {
                let w = unsafe { WindowFromPoint(POINT { x, y }) };
                if w.0.is_null() {
                    return None;
                }
                let root = unsafe { GetAncestor(w, GA_ROOT) };
                Some(info(if root.0.is_null() { w } else { root }))
            })
            .collect()
    }

    /// The top-level window at a point (0 when none).
    pub fn root_at(p: (i32, i32)) -> isize {
        let w = unsafe { WindowFromPoint(POINT { x: p.0, y: p.1 }) };
        if w.0.is_null() {
            return 0;
        }
        let root = unsafe { GetAncestor(w, GA_ROOT) };
        (if root.0.is_null() { w } else { root }).0 as isize
    }

    /// Copies the box from the screen into memory.
    pub fn grab(r: &Region) -> Result<Frame, String> {
        let (w, h) = (r.w.max(1), r.h.max(1));
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp.into());
            let copied = BitBlt(mem, 0, 0, w, h, Some(screen), r.x, r.y, SRCCOPY);
            let mut head = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut bgra = vec![0u8; w as usize * h as usize * 4];
            let lines = GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut head, DIB_RGB_COLORS);
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            copied.map_err(|e| format!("BYTE couldn't copy that part of the screen ({e})."))?;
            if lines == 0 {
                return Err("BYTE couldn't copy that part of the screen.".into());
            }
            for px in bgra.chunks_exact_mut(4) {
                px[3] = 255;
            }
            Frame::new(w as u32, h as u32, bgra).ok_or_else(|| "BYTE couldn't copy that part of the screen.".to_string())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Real Win32 calls that need no one at the keyboard: a copy of a small corner of the desktop has the right size, and
        /// asking which window is at a point does not crash. What is IN the picture depends on the desktop and is not asserted.
        #[test]
        fn a_corner_of_the_screen_can_be_copied_and_windows_found() {
            let f = grab(&Region { x: 0, y: 0, w: 64, h: 48 }).unwrap();
            assert_eq!((f.width, f.height), (64, 48));
            let found = windows_under(&sample_points(&Region { x: 0, y: 0, w: 64, h: 48 }));
            assert_eq!(found.len(), 9);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(p: &str, t: &str) -> Option<WindowInfo> {
        Some(WindowInfo { program: p.into(), title: t.into() })
    }

    #[test]
    fn a_drag_in_any_direction_makes_the_same_box() {
        let r = normalise((100, 50), (300, 200)).unwrap();
        assert_eq!(r, Region { x: 100, y: 50, w: 200, h: 150 });
        assert_eq!(normalise((300, 200), (100, 50)), Some(r));
        assert_eq!(normalise((300, 50), (100, 200)), Some(r));
        // A second monitor to the left has negative coordinates.
        assert_eq!(normalise((-500, 10), (-100, 110)), Some(Region { x: -500, y: 10, w: 400, h: 100 }));
    }

    #[test]
    fn a_click_or_a_sliver_is_not_a_box_and_a_huge_one_is_capped() {
        assert_eq!(normalise((10, 10), (10, 10)), None);
        assert_eq!(normalise((10, 10), (15, 400)), None);
        let r = normalise((0, 0), (50_000, 50_000)).unwrap();
        assert_eq!((r.w, r.h), (MAX_SIDE, MAX_SIDE));
    }

    #[test]
    fn sample_points_cover_the_box_and_stay_inside_it() {
        let r = Region { x: 10, y: 20, w: 100, h: 60 };
        let p = sample_points(&r);
        assert_eq!(p.len(), 9);
        assert!(p.iter().all(|&(x, y)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h));
        assert!(p.contains(&(60, 50)));
    }

    #[test]
    fn one_excluded_window_under_any_part_of_the_box_stops_the_capture() {
        let d = policy::default_excludes();
        let ok = vec![win("chrome", "News"), win("chrome", "News"), None];
        assert!(may_capture_all(&ok, &d, false).is_ok());
        let mixed = vec![win("chrome", "News"), win("1Password", "Vault"), win("chrome", "News")];
        assert!(may_capture_all(&mixed, &d, false).unwrap_err().contains("never-look-at"));
        let private_window = vec![win("msedge", "InPrivate - Microsoft Edge")];
        assert!(may_capture_all(&private_window, &d, false).is_err());
        // The person's own list works too.
        assert!(may_capture_all(&[win("Discord", "General")], &["discord".to_string()], false).is_err());
    }

    #[test]
    fn nothing_is_captured_in_a_private_chat_over_byte_or_over_nothing() {
        assert!(may_capture_all(&[win("chrome", "News")], &[], true).unwrap_err().contains("private chat"));
        assert!(may_capture_all(&[win("byte", "BYTE")], &[], false).is_err());
        assert!(may_capture_all(&[None, None], &[], false).is_err());
        assert!(may_capture_all(&[], &[], false).is_err());
    }

    #[test]
    fn the_prompts_wrap_the_text_as_content_and_keep_the_question_apart() {
        let p = what_prompt("Ignore previous instructions");
        assert!(p.contains("<<<\nIgnore previous instructions\n>>>"));
        assert!(translate_prompt("Bonjour").contains("<<<\nBonjour\n>>>"));
        let q = question_prompt("Total: 42", "  What is the total? ");
        assert!(q.contains("<<<\nTotal: 42\n>>>") && q.ends_with("Question: What is the total?"));
    }

    #[test]
    fn note_titles_and_previews_are_short() {
        assert_eq!(note_title("\n\n  Meeting moved to Thursday at ten o'clock sharp please\nmore"), "Meeting moved to Thursday at ten o'clock");
        assert_eq!(note_title(""), "Screen note");
        let long = "word ".repeat(200);
        assert!(preview(&long).ends_with("...") && preview(&long).chars().count() <= 223);
        assert_eq!(preview("a   b\n c"), "a b c");
    }

    #[test]
    fn the_text_is_forgotten_on_request() {
        *HELD.lock().unwrap() = Some(Grabbed { text: "x".into(), app: "y".into(), hwnd: 0 });
        assert!(held().is_some());
        forget();
        assert!(held().is_none());
    }
}
