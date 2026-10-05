//! Where the BYTE cloud API key lives: the macOS Keychain or Windows Credential
//! Manager, never a file, a setting or the repository (see docs/CLOUD-MODE.md).

use crate::error::{AppError, AppResult};

/// Stores one secret per profile.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> AppResult<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> AppResult<()>;
    fn delete(&self, account: &str) -> AppResult<()>;
}

#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub const SERVICE: &str = "com.loganstarner.byte.cloud";

/// What the system calls its secret store, for messages.
#[cfg(target_os = "macos")]
const STORE: &str = "Keychain";
#[cfg(windows)]
const STORE: &str = "Credential Manager";

/// The macOS Keychain, or Windows Credential Manager.
pub struct Keychain;

#[cfg(any(target_os = "macos", windows))]
impl SecretStore for Keychain {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.get_password()) {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::msg(format!("Couldn't read the {STORE}: {e}"))),
        }
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| AppError::msg(format!("Couldn't save to the {STORE}: {e}")))
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::msg(format!("Couldn't remove the key from the {STORE}: {e}"))),
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
impl SecretStore for Keychain {
    fn get(&self, _account: &str) -> AppResult<Option<String>> {
        Ok(None)
    }

    fn set(&self, _account: &str, _secret: &str) -> AppResult<()> {
        Err(AppError::msg("BYTE cloud keys are stored in the system's secret store (the macOS Keychain or Windows Credential Manager), which this system doesn't have yet."))
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
