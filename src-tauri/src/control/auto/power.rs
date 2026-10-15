//! Keeps the PC awake while a job runs, and lets it sleep again afterwards. Windows' `SetThreadExecutionState` is per thread, so
//! one small thread holds it and sets it back when told. Everything else talks to the trait, so tests use a double.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;

pub trait KeepAwake: Send + Sync {
    fn hold(&self) -> Result<(), String>;
    fn release(&self);
}

/// Holds while it lives, releases when dropped (also if the run ends in an error).
pub struct AwakeGuard<'a> {
    inner: &'a dyn KeepAwake,
    held: bool,
}

impl<'a> AwakeGuard<'a> {
    pub fn new(inner: &'a dyn KeepAwake, wanted: bool) -> AwakeGuard<'a> {
        let held = wanted && inner.hold().is_ok();
        AwakeGuard { inner, held }
    }

    pub fn held(&self) -> bool {
        self.held
    }
}

impl Drop for AwakeGuard<'_> {
    fn drop(&mut self) {
        if self.held {
            self.inner.release();
        }
    }
}

/// A test double: counts holds and releases.
#[derive(Default)]
pub struct FakeAwake {
    pub holds: AtomicI32,
    pub releases: AtomicI32,
}

impl KeepAwake for FakeAwake {
    fn hold(&self) -> Result<(), String> {
        self.holds.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn release(&self) {
        self.releases.fetch_add(1, Ordering::SeqCst);
    }
}

/// The real one: Windows asks for the system and the screen to stay on; elsewhere it says it can't.
#[derive(Default)]
pub struct SystemAwake {
    stop: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl KeepAwake for SystemAwake {
    fn hold(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED};
            let mut slot = self.stop.lock().map_err(|_| "busy")?;
            if slot.is_some() {
                return Ok(());
            }
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();
            std::thread::Builder::new()
                .name("byte-keep-awake".into())
                .spawn(move || {
                    let prev = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) };
                    let _ = ready_tx.send(prev.0 != 0);
                    let _ = rx.recv();
                    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
                })
                .map_err(|e| e.to_string())?;
            let ok = ready_rx.recv().unwrap_or(false);
            if !ok {
                let _ = tx.send(());
                return Err("Windows would not keep the PC awake".into());
            }
            *slot = Some(tx);
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Err("keeping the PC awake needs Windows".into())
        }
    }

    fn release(&self) {
        if let Ok(mut slot) = self.stop.lock() {
            if let Some(tx) = slot.take() {
                let _ = tx.send(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_holds_while_it_lives_and_releases_when_dropped() {
        let a = FakeAwake::default();
        {
            let g = AwakeGuard::new(&a, true);
            assert!(g.held());
            assert_eq!(a.holds.load(Ordering::SeqCst), 1);
            assert_eq!(a.releases.load(Ordering::SeqCst), 0);
        }
        assert_eq!(a.releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn it_releases_even_when_the_run_ends_in_a_panic() {
        let a = FakeAwake::default();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = AwakeGuard::new(&a, true);
            panic!("run failed");
        }));
        assert!(r.is_err());
        assert_eq!(a.releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn when_it_is_not_wanted_nothing_is_held() {
        let a = FakeAwake::default();
        let g = AwakeGuard::new(&a, false);
        assert!(!g.held());
        drop(g);
        assert_eq!((a.holds.load(Ordering::SeqCst), a.releases.load(Ordering::SeqCst)), (0, 0));
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_the_real_one_says_it_cannot() {
        let s = SystemAwake::default();
        assert!(s.hold().is_err());
        s.release();
    }
}
