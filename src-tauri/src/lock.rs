//! The lock: BYTE asks for Touch ID (or the Mac's password) -- or Windows Hello on
//! a PC -- when it opens and after it has been idle for a while. While locked, the commands that read
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

/// Whether this build has a way to lock BYTE at all (Touch ID or the Mac
/// password; Windows Hello). Whether THIS machine can use it is a separate
/// question, answered at run time by `can_check`.
pub const AVAILABLE: bool = cfg!(any(target_os = "macos", windows));

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
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(move || win::authenticate(&reason)).await.map_err(|e| AppError::msg(e.to_string()))?.map_err(AppError::msg)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = reason;
        Err(AppError::msg("Locking BYTE isn't available on this system yet."))
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

/// Windows Hello: PIN, fingerprint or face. This is the app-lock gate, not an
/// encryption key, which is how the macOS side uses Touch ID too.
///
/// Honest difference from the Mac: macOS accepts the account password as a
/// fallback, Windows Hello needs a PIN, fingerprint or face to be set up. An
/// account that only has a password cannot use the lock, and says so rather than
/// failing mysteriously.
#[cfg(windows)]
mod win {
    use windows::core::HSTRING;
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult as R, UserConsentVerifier, UserConsentVerifierAvailability as A,
    };
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    /// WinRT needs COM on the calling thread; an error here only means the thread
    /// already has it. Same reasoning as ocr.rs.
    fn ensure_winrt() {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    }

    /// Whether Windows Hello is set up for this account.
    pub fn can_check() -> bool {
        ensure_winrt();
        UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.get()).map(|a| a == A::Available).unwrap_or(false)
    }

    /// Blocks until the person confirms or cancels.
    pub fn authenticate(reason: &str) -> Result<(), String> {
        ensure_winrt();
        let availability = UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.get()).map_err(|e| e.message())?;
        if availability != A::Available {
            return Err(match availability {
                A::DeviceBusy => "Windows Hello is busy right now. Try again in a moment.".into(),
                A::DisabledByPolicy => "Windows Hello is turned off by your organisation's policy.".into(),
                _ => "Windows Hello isn't set up on this PC. Add a PIN, fingerprint or face in Settings \u{2192} Accounts \u{2192} Sign-in options.".into(),
            });
        }
        let result = UserConsentVerifier::RequestVerificationAsync(&HSTRING::from(reason))
            .and_then(|op| op.get())
            .map_err(|e| e.message())?;
        match result {
            R::Verified => Ok(()),
            R::Canceled => Err("Not unlocked.".into()),
            R::RetriesExhausted => Err("Too many wrong tries. Wait a moment and try again.".into()),
            R::DeviceBusy => Err("Windows Hello is busy right now. Try again in a moment.".into()),
            _ => Err("Windows couldn't confirm it's you.".into()),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockStatus {
    pub available: bool,
    pub enabled: bool,
    pub locked: bool,
    /// What the person is asked for, in the platform's own words. The frontend
    /// shows these rather than hard-coding "Touch ID", which is wrong on a PC.
    pub method: &'static str,
    /// "BYTE asks for ___ when it opens".
    pub asks: &'static str,
    /// The small line under the unlock button.
    pub hint: &'static str,
    /// Shown instead of the toggle when this machine cannot confirm the owner.
    pub unavailable: &'static str,
}

#[cfg(windows)]
const WORDING: (&str, &str, &str, &str) = (
    "Windows Hello",
    "Windows Hello (your PIN, fingerprint or face)",
    "No fingerprint or camera? Windows asks for your PIN instead.",
    "Windows Hello isn't set up on this PC, so BYTE can't be locked here. Add a PIN, fingerprint or face in Settings \u{2192} Accounts \u{2192} Sign-in options, then come back.",
);
#[cfg(not(windows))]
const WORDING: (&str, &str, &str, &str) = (
    "Touch ID",
    "Touch ID (or your Mac's password)",
    "No Touch ID? macOS asks for your password instead.",
    "This Mac can't confirm it's you (no Touch ID and no password set up), so BYTE can't be locked here.",
);

#[tauri::command]
pub async fn lock_status(state: State<'_, AppState>) -> AppResult<LockStatus> {
    let enabled = state.settings.lock().await.lock_enabled;
    #[cfg(target_os = "macos")]
    let available = mac::can_check();
    #[cfg(windows)]
    let available = tauri::async_runtime::spawn_blocking(win::can_check).await.unwrap_or(false);
    #[cfg(not(any(target_os = "macos", windows)))]
    let available = false;
    let (method, asks, hint, unavailable) = WORDING;
    Ok(LockStatus { available, enabled: enabled && AVAILABLE, locked: state.lock.is_locked(), method, asks, hint, unavailable })
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

    /// On Windows: asking whether Hello is set up works and does not panic (the
    /// prompt itself cannot be automated). Both answers are valid: a CI runner has
    /// no Hello, a client PC usually does.
    #[cfg(windows)]
    #[test]
    fn windows_says_whether_it_can_check() {
        eprintln!("Windows Hello available: {}", win::can_check());
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
