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

#[cfg(not(target_os = "macos"))]
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
