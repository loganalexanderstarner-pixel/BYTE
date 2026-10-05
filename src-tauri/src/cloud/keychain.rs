//! Where the BYTE cloud API key lives: the macOS Keychain, never a file, a
//! setting or the repository (see docs/CLOUD-MODE.md).

use crate::error::{AppError, AppResult};

/// Stores one secret per profile.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> AppResult<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> AppResult<()>;
    fn delete(&self, account: &str) -> AppResult<()>;
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const SERVICE: &str = "com.loganstarner.byte.cloud";

/// The macOS Keychain.
pub struct Keychain;

#[cfg(target_os = "macos")]
impl SecretStore for Keychain {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.get_password()) {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::msg(format!("Couldn't read the Keychain: {e}"))),
        }
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| AppError::msg(format!("Couldn't save to the Keychain: {e}")))
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::msg(format!("Couldn't remove the key from the Keychain: {e}"))),
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

/// Windows and Linux get their own stores in their ports; until then, say so.
#[cfg(not(any(target_os = "macos", target_os = "android")))]
impl SecretStore for Keychain {
    fn get(&self, _account: &str) -> AppResult<Option<String>> {
        Ok(None)
    }

    fn set(&self, _account: &str, _secret: &str) -> AppResult<()> {
        Err(AppError::msg("BYTE cloud keys are stored in the macOS Keychain, so cloud mode needs a Mac."))
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
