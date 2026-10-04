//! The lock: BYTE asks for Touch ID (or the Mac's password) when it opens and
//! after it has been idle for a while. While locked, the commands that read
//! chats, notes and other personal data refuse, so nothing shows behind the
//! lock screen even if the UI were bypassed.
//!
//! The chats are already encrypted on disk (db.rs); the lock keeps someone at
//! an unlocked Mac out of BYTE. Tying the database key to Touch ID in the
//! Keychain would need a paid developer signature, so that isn't done.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const EVENT: &str = "lock://changed";
const LOCKED: &str = "BYTE is locked. Unlock it with Touch ID or your Mac's password.";

/// Whether this system can lock BYTE (Touch ID / the Mac password: macOS only).
pub const AVAILABLE: bool = cfg!(target_os = "macos");

pub struct Lock {
    locked: AtomicBool,
    /// Unix seconds of the last thing the user did in BYTE.
    last_active: AtomicI64,
}

impl Default for Lock {
    fn default() -> Self {
        Lock { locked: AtomicBool::new(false), last_active: AtomicI64::new(now()) }
    }
}

impl Lock {
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    pub fn lock(&self) {
        self.locked.store(true, Ordering::Relaxed);
    }

    pub fn unlock(&self, at: i64) {
        self.last_active.store(at, Ordering::Relaxed);
        self.locked.store(false, Ordering::Relaxed);
    }

    pub fn touch(&self, at: i64) {
        if !self.is_locked() {
            self.last_active.fetch_max(at, Ordering::Relaxed);
        }
    }

    /// Should BYTE lock now? Only when the lock is on, has an idle time, and
    /// nothing happened for that long.
    pub fn due(&self, enabled: bool, after_minutes: u32, at: i64) -> bool {
        enabled && after_minutes > 0 && !self.is_locked() && at - self.last_active.load(Ordering::Relaxed) >= i64::from(after_minutes) * 60
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Err while locked; the first line of every command that shows personal data.
pub fn ensure(state: &AppState) -> AppResult<()> {
    if state.lock.is_locked() {
        Err(AppError::msg(LOCKED))
    } else {
        Ok(())
    }
}

pub fn set_locked(app: &AppHandle, locked: bool) {
    let state = app.state::<AppState>();
    if locked {
        state.lock.lock();
    } else {
        state.lock.unlock(now());
    }
    let _ = app.emit(EVENT, locked);
}

/// At launch: locked from the start when the lock is on. Then a check every
/// 30 seconds for the idle time.
pub fn start(app: AppHandle, enabled: bool) {
    if enabled && AVAILABLE {
        app.state::<AppState>().lock.lock();
    }
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let state = app.state::<AppState>();
            let (on, after) = {
                let s = state.settings.lock().await;
                (s.lock_enabled && AVAILABLE, s.lock_after_minutes)
            };
            if state.lock.due(on, after, now()) {
                set_locked(&app, true);
            }
        }
    });
}

/// Asks the Mac to confirm it's the owner (for erasing everything and the like).
pub async fn verify(reason: &str) -> AppResult<()> {
    authenticate(reason.into()).await
}

/// Asks the Mac to confirm it's the owner: Touch ID, or the password.
async fn authenticate(reason: String) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(move || mac::authenticate(&reason)).await.map_err(|e| AppError::msg(e.to_string()))?.map_err(AppError::msg)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = reason;
        Err(AppError::msg("Locking BYTE needs a Mac for now."))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};

    /// Whether this Mac can confirm the owner (Touch ID or a password is set up).
    pub fn can_check() -> bool {
        let ctx = unsafe { LAContext::new() };
        unsafe { ctx.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) }.is_ok()
    }

    /// Blocks until the person confirms or cancels (the reply comes on another thread).
    pub fn authenticate(reason: &str) -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
        let ctx = unsafe { LAContext::new() };
        if let Err(e) = unsafe { ctx.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) } {
            return Err(format!("This Mac can't confirm it's you right now: {}", e.localizedDescription()));
        }
        let reply = block2::RcBlock::new(move |ok: Bool, err: *mut NSError| {
            let why = if ok.as_bool() {
                None
            } else {
                Some(unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "Not unlocked.".into()))
            };
            let _ = tx.send(why);
        });
        unsafe { ctx.evaluatePolicy_localizedReason_reply(LAPolicy::DeviceOwnerAuthentication, &NSString::from_str(reason), &reply) };
        match rx.recv() {
            Ok(None) => Ok(()),
            Ok(Some(why)) => Err(why),
            Err(_) => Err("Not unlocked.".into()),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockStatus {
    pub available: bool,
    pub enabled: bool,
    pub locked: bool,
}

#[tauri::command]
pub async fn lock_status(state: State<'_, AppState>) -> AppResult<LockStatus> {
    let enabled = state.settings.lock().await.lock_enabled;
    #[cfg(target_os = "macos")]
    let available = mac::can_check();
    #[cfg(not(target_os = "macos"))]
    let available = false;
    Ok(LockStatus { available, enabled: enabled && AVAILABLE, locked: state.lock.is_locked() })
}

/// The user did something; restarts the idle time.
#[tauri::command]
pub fn lock_touch(state: State<'_, AppState>) {
    state.lock.touch(now());
}

#[tauri::command]
pub async fn lock_now(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    if !(AVAILABLE && state.settings.lock().await.lock_enabled) {
        return Err(AppError::msg("Turn on the lock first: Settings → Privacy."));
    }
    set_locked(&app, true);
    Ok(())
}

#[tauri::command]
pub async fn lock_unlock(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    if !state.lock.is_locked() {
        return Ok(());
    }
    authenticate("unlock BYTE".into()).await?;
    set_locked(&app, false);
    Ok(())
}

/// Before the lock is turned on: make sure this Mac can unlock it again.
#[tauri::command]
pub async fn lock_verify() -> AppResult<()> {
    authenticate("turn on BYTE's lock".into()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_the_idle_time_only_when_on() {
        let l = Lock::default();
        l.unlock(1_000);
        assert!(!l.due(false, 5, 10_000), "off");
        assert!(!l.due(true, 0, 10_000), "only at launch");
        assert!(!l.due(true, 5, 1_000 + 299), "not idle long enough");
        assert!(l.due(true, 5, 1_000 + 300));
        l.touch(1_200);
        assert!(!l.due(true, 5, 1_000 + 300), "touching restarts the idle time");
        l.lock();
        assert!(!l.due(true, 5, 99_999), "already locked");
        l.touch(5_000);
        assert!(l.is_locked(), "touching doesn't unlock");
        l.unlock(6_000);
        assert!(!l.is_locked());
        assert!(!l.due(true, 5, 6_100));
    }

    /// On the Mac runner: asking whether Touch ID / a password can be checked
    /// works (the prompt itself can't be automated).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_mac_says_whether_it_can_check() {
        eprintln!("can check the owner: {}", mac::can_check());
    }

    #[test]
    fn touches_never_move_the_clock_back() {
        let l = Lock::default();
        l.unlock(2_000);
        l.touch(1_500);
        assert!(!l.due(true, 1, 2_059));
        assert!(l.due(true, 1, 2_060));
    }
}
