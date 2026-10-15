//! COM for this thread, started once and ended when the thread ends. UI Automation and the audio endpoints need it; a thread
//! that already has COM in the other mode is tolerated (the calls still work there).

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    struct Guard {
        ok: bool,
        /// This guard started COM (or added a count to it), so it ends it.
        owns: bool,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.owns {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn start() -> Guard {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            Guard { ok: true, owns: true }
        } else if hr == RPC_E_CHANGED_MODE {
            Guard { ok: true, owns: false }
        } else {
            Guard { ok: false, owns: false }
        }
    }

    thread_local! {
        static COM: Guard = start();
    }

    pub fn ensure() -> Result<(), String> {
        COM.with(|g| if g.ok { Ok(()) } else { Err("Windows could not start the service BYTE needs".to_string()) })
    }
}

#[cfg(windows)]
pub use imp::ensure;

#[cfg(not(windows))]
pub fn ensure() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn ensure_is_repeatable_on_one_thread() {
        assert!(super::ensure().is_ok());
        assert!(super::ensure().is_ok());
    }
}
