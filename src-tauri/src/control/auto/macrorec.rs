//! The watcher behind "watch me do this once" and its commands. The watcher polls the mouse button and the keys on its own thread
//! (no hooks, nothing injected), notes clicks as UI Automation names and key combinations, and hands them to `Recorder`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use super::live::LiveView;
use super::recorder::{Recorder, Recording};
use crate::control;
use crate::error::{AppError, AppResult};

struct Session {
    stop: Arc<AtomicBool>,
    rec: Arc<Mutex<Recorder>>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn recording() -> bool {
    SESSION.lock().map(|s| s.is_some()).unwrap_or(false)
}

fn badge(app: &AppHandle, steps: usize) {
    let v = LiveView { title: "Recording your steps".into(), status: "recording".into(), now: format!("{steps} steps noted. Typed text is never kept."), ..Default::default() };
    super::live::show(app, &v);
}

#[tauri::command]
pub fn macro_start(app: AppHandle) -> AppResult<()> {
    let s = control::settings_now(&app).ok_or_else(|| AppError::msg("settings are busy, try again"))?;
    if !cfg!(windows) {
        return Err(AppError::msg("Watch me do this once needs Windows."));
    }
    if s.kids_mode || !s.pc_control || !s.macro_recorder {
        return Err(AppError::msg("Turn on PC control and \"Watch me do this once\" in Settings, Control first."));
    }
    if super::real::is_running() {
        return Err(AppError::msg("A job is running. Stop it first."));
    }
    if recording() {
        return Err(AppError::msg("Already watching."));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let rec = Arc::new(Mutex::new(Recorder::new(control::never_touch_apps(&s))));
    if let Ok(mut g) = SESSION.lock() {
        *g = Some(Session { stop: stop.clone(), rec: rec.clone() });
    }
    badge(&app, 0);
    let app2 = app.clone();
    std::thread::Builder::new()
        .name("byte-macro-watch".into())
        .spawn(move || {
            watch(&stop, &rec, &|n| badge(&app2, n));
        })
        .map_err(|e| AppError::msg(e.to_string()))?;
    Ok(())
}

/// Ends the recording and returns what was noted (also sent as the event `macro://done`, for the badge's own button).
#[tauri::command]
pub fn macro_stop(app: AppHandle) -> AppResult<Recording> {
    let session = SESSION.lock().ok().and_then(|mut g| g.take()).ok_or_else(|| AppError::msg("Nothing is being recorded."))?;
    session.stop.store(true, Ordering::SeqCst);
    super::live::hide(&app);
    let rec = match Arc::try_unwrap(session.rec) {
        Ok(m) => m.into_inner().unwrap_or_default(),
        Err(arc) => std::mem::take(&mut *arc.lock().unwrap_or_else(|p| p.into_inner())),
    };
    let out = rec.finish();
    let _ = app.emit("macro://done", &out);
    Ok(out)
}

#[tauri::command]
pub fn macro_cancel(app: AppHandle) {
    if let Some(s) = SESSION.lock().ok().and_then(|mut g| g.take()) {
        s.stop.store(true, Ordering::SeqCst);
    }
    super::live::hide(&app);
}

#[tauri::command]
pub fn macro_active() -> bool {
    recording()
}

#[cfg(windows)]
fn watch(stop: &AtomicBool, rec: &Mutex<Recorder>, on_count: &dyn Fn(usize)) {
    use super::recorder::{element_at, Raw};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let down = |vk: i32| unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0;
    let screen = control::real_screen();
    let mut mouse_was = down(VK_LBUTTON.0 as i32);
    let mut keys_was = [false; 256];
    // Special keys that count as steps on their own, and the letters, digits and F-keys that count with Ctrl, Alt or Win.
    let special: &[(i32, &str)] = &[(0x0D, "enter"), (0x09, "tab"), (0x1B, "esc"), (0x2E, "delete"), (0x08, "backspace"), (0x26, "up"), (0x28, "down"), (0x25, "left"), (0x27, "right")];
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(30));
        let (app, title) = screen.foreground().unwrap_or_default();
        let mine = app == "byte" || app.is_empty();
        let m = down(VK_LBUTTON.0 as i32);
        if m && !mouse_was && !mine {
            let mut p = POINT::default();
            if unsafe { GetCursorPos(&mut p) }.is_ok() {
                if let Ok(shot) = screen.snapshot() {
                    if let Some(e) = element_at(&shot, p.x, p.y) {
                        rec.lock().unwrap_or_else(|x| x.into_inner()).feed(Raw::Click { app: shot.app.clone(), window: shot.title.clone(), name: e.name.clone(), sensitive: e.sensitive });
                        on_count(rec.lock().unwrap_or_else(|x| x.into_inner()).len());
                    }
                }
            }
        }
        mouse_was = m;
        let (ctrl, alt, shift, win) = (down(VK_CONTROL.0 as i32), down(VK_MENU.0 as i32), down(VK_SHIFT.0 as i32), down(VK_LWIN.0 as i32) || down(VK_RWIN.0 as i32));
        let mut candidates: Vec<(i32, String)> = special.iter().map(|(vk, n)| (*vk, n.to_string())).collect();
        candidates.extend((0x41..=0x5A).map(|vk| (vk, ((vk as u8) as char).to_ascii_lowercase().to_string())));
        candidates.extend((0x30..=0x39).map(|vk| (vk, ((vk as u8) as char).to_string())));
        candidates.extend((0x70..=0x7B).map(|vk| (vk, format!("f{}", vk - 0x6F))));
        for (vk, name) in candidates {
            let k = down(vk);
            let was = keys_was[vk as usize];
            keys_was[vk as usize] = k;
            if !k || was || mine {
                continue;
            }
            let is_special = name.len() > 1;
            if ctrl || alt || win || is_special {
                let mut combo = String::new();
                for (on, n) in [(ctrl, "ctrl"), (alt, "alt"), (shift, "shift"), (win, "win")] {
                    if on {
                        combo.push_str(n);
                        combo.push('+');
                    }
                }
                combo.push_str(&name);
                let _ = &title;
                rec.lock().unwrap_or_else(|x| x.into_inner()).feed(Raw::Keys { app: app.clone(), combo });
                on_count(rec.lock().unwrap_or_else(|x| x.into_inner()).len());
            } else {
                rec.lock().unwrap_or_else(|x| x.into_inner()).feed(Raw::Typing { app: app.clone(), count: 1 });
            }
        }
    }
}

#[cfg(not(windows))]
fn watch(_stop: &AtomicBool, _rec: &Mutex<Recorder>, _on_count: &dyn Fn(usize)) {}
