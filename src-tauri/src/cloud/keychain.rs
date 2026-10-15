//! Where the BYTE cloud API key lives: the macOS Keychain, Windows Credential Manager or the Linux
//! desktop's keyring, never a file, a setting or the repository (see docs/CLOUD-MODE.md).

use crate::error::{AppError, AppResult};

/// Stores one secret per profile.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> AppResult<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> AppResult<()>;
    fn delete(&self, account: &str) -> AppResult<()>;
}

#[cfg_attr(not(any(target_os = "macos", windows, target_os = "linux")), allow(dead_code))]
pub const SERVICE: &str = "com.loganstarner.byte.cloud";

/// What the system calls its secret store, for messages.
#[cfg(target_os = "macos")]
const STORE: &str = "Keychain";
#[cfg(windows)]
const STORE: &str = "Credential Manager";
#[cfg(target_os = "linux")]
const STORE: &str = "system keyring";

/// What a failed secret-store call means for the person. Linux has no store of its own: a keyring such as GNOME Keyring
/// or KDE Wallet has to be running in the desktop session, and a minimal install may have none.
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
pub(crate) fn why(e: &keyring::Error) -> String {
    #[cfg(target_os = "linux")]
    if matches!(e, keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_)) {
        return format!("{e}. BYTE keeps secrets in your desktop's keyring (GNOME Keyring or KDE Wallet): install one and sign in to your desktop session, then try again.");
    }
    e.to_string()
}

/// The macOS Keychain, Windows Credential Manager, or the Linux desktop's keyring.
pub struct Keychain;

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
impl SecretStore for Keychain {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.get_password()) {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::msg(format!("Couldn't read the {STORE}: {}", why(&e)))),
        }
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| AppError::msg(format!("Couldn't save to the {STORE}: {}", why(&e))))
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::msg(format!("Couldn't remove the key from the {STORE}: {}", why(&e)))),
        }
    }
}

/// Android: the key sits in a file inside the app's private folder. Android gives
/// every app its own folder that no other app can read and encrypts it at rest, so
/// this is the same protection the Keychain gives on a Mac for an app without a
/// paid signature. The Android Keystore replaces it with the Kotlin plugin (A4,
/// docs/ANDROID.md). The file is 0600, never a setting, never in a backup of the
/// repository, and the folder is set once at startup.
#[cfg(target_os = "android")]
static ANDROID_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
pub fn set_android_dir(dir: std::path::PathBuf) {
    let _ = ANDROID_DIR.set(dir);
}

#[cfg(target_os = "android")]
fn secret_file(account: &str) -> AppResult<std::path::PathBuf> {
    let dir = ANDROID_DIR.get().ok_or_else(|| AppError::msg("BYTE's private folder isn't ready yet."))?;
    // The account is a profile id: keep the file name to safe characters.
    let name: String = account.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    Ok(dir.join(format!("cloud-key-{name}")))
}

#[cfg(target_os = "android")]
impl SecretStore for Keychain {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        match std::fs::read_to_string(secret_file(account)?) {
            Ok(k) => Ok(Some(k.trim().to_string()).filter(|k| !k.is_empty())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AppError::msg(format!("Couldn't read the saved key: {e}"))),
        }
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(secret_file(account)?)
            .map_err(|e| AppError::msg(format!("Couldn't save the key: {e}")))?;
        f.write_all(secret.as_bytes()).map_err(|e| AppError::msg(format!("Couldn't save the key: {e}")))
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        match std::fs::remove_file(secret_file(account)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::msg(format!("Couldn't remove the saved key: {e}"))),
        }
    }
}

/// Systems without a secret store of their own: say so.
#[cfg(not(any(target_os = "macos", windows, target_os = "linux", target_os = "android")))]
impl SecretStore for Keychain {
    fn get(&self, _account: &str) -> AppResult<Option<String>> {
        Ok(None)
    }

    fn set(&self, _account: &str, _secret: &str) -> AppResult<()> {
        Err(AppError::msg("BYTE cloud keys are stored in the system's secret store (the macOS Keychain, Windows Credential Manager or the Linux desktop's keyring), which this system doesn't have yet."))
    }

    fn delete(&self, _account: &str) -> AppResult<()> {
        Ok(())
    }
}

/// In-memory store for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    /// Saves, reads back and removes a throwaway secret in the real Credential Manager.
    #[test]
    #[ignore = "writes to the real Windows Credential Manager, which needs a desktop session: it fails over SSH with ERROR_NO_SUCH_LOGON_SESSION; run with --ignored"]
    fn credential_manager_round_trips() {
        let account = "byte-test-round-trip";
        let secret = "byte_test_not_a_real_key";
        let store = Keychain;
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
        store.set(account, secret).unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some(secret));
        // Writing again replaces the value instead of failing.
        store.set(account, "byte_test_second").unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some("byte_test_second"));
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    /// Saves, reads back and removes a throwaway secret in the real desktop keyring.
    #[test]
    #[ignore = "writes to the desktop's keyring, which needs a running Secret Service (GNOME Keyring or KDE Wallet) in a session; run with --ignored"]
    fn secret_service_round_trips() {
        let account = "byte-test-round-trip";
        let secret = "byte_test_not_a_real_key";
        let store = Keychain;
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
        store.set(account, secret).unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some(secret));
        store.set(account, "byte_test_second").unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some("byte_test_second"));
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
    }

    /// The app calls the secret store from async code. With the wrong runtime feature the keyring panicked here
    /// ("Cannot start a runtime from within a runtime"), which a release build turns into an abort of the whole app.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs a running Secret Service, like secret_service_round_trips"]
    async fn the_keyring_works_from_inside_the_async_runtime() {
        let store = Keychain;
        store.set("byte-test-async", "byte_test_not_a_real_key").unwrap();
        assert_eq!(store.get("byte-test-async").unwrap().as_deref(), Some("byte_test_not_a_real_key"));
        store.delete("byte-test-async").unwrap();
        assert_eq!(store.get("byte-test-async").unwrap(), None);
    }

    #[test]
    fn a_missing_keyring_says_what_to_install() {
        let e = keyring::Error::NoStorageAccess("no secret service".into());
        let text = why(&e);
        assert!(text.contains("GNOME Keyring or KDE Wallet") && text.contains("no secret service"), "{text}");
        assert_eq!(why(&keyring::Error::NoEntry), keyring::Error::NoEntry.to_string());
    }
}
