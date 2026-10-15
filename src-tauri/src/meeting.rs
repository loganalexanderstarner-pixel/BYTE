//! Is another program on a call, holding the microphone? "Hey BYTE" stays off then (setting `meetingQuiet`), so it neither
//! hears the meeting nor answers it.
//!
//! Windows lists, per microphone, the programs that have an audio session open on it and whether each one is active right
//! now (the audio session manager, the same list the Windows sound mixer shows). A program that is not BYTE and has an
//! active capture session counts as a call. Nothing is recorded or listened to: only who holds the microphone is asked,
//! once each time the voice code checks. Other systems cannot tell, so they say "no meeting".

/// One program's audio session on a microphone, as read from the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSession {
    pub pid: u32,
    /// Capturing right now (not just open).
    pub active: bool,
    /// The system sounds session (never a call).
    pub system_sounds: bool,
}

/// True when some other program is capturing from a microphone. `own_pid` is BYTE itself.
pub fn meeting_active(sessions: &[CaptureSession], own_pid: u32) -> bool {
    sessions.iter().any(|s| s.active && !s.system_sounds && s.pid != 0 && s.pid != own_pid)
}

/// Whether a call is going on right now. Always false when the platform cannot tell.
pub fn active() -> bool {
    meeting_active(&read_sessions(), std::process::id())
}

/// Same, but only asks when `enabled` (the `meetingQuiet` setting), so a person who turned it off is never checked.
pub fn active_if(enabled: bool) -> bool {
    enabled && active()
}

#[cfg(not(windows))]
fn read_sessions() -> Vec<CaptureSession> {
    Vec::new()
}

#[cfg(windows)]
fn read_sessions() -> Vec<CaptureSession> {
    win::read().unwrap_or_default()
}

#[cfg(windows)]
mod win {
    use super::CaptureSession;
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{eCapture, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};

    /// COM is started once per thread and stopped when the thread ends. A thread that already runs COM in another mode
    /// (RPC_E_CHANGED_MODE) can use it as it is, so that case is tolerated and not paired with an uninitialize.
    struct ComGuard(bool);
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }
    thread_local! {
        static COM: ComGuard = ComGuard(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() });
    }

    pub fn read() -> windows::core::Result<Vec<CaptureSession>> {
        let mut out = Vec::new();
        unsafe {
            COM.with(|_| ());
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
            for d in 0..devices.GetCount()? {
                let Ok(device) = devices.Item(d) else { continue };
                let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                let Ok(list) = manager.GetSessionEnumerator() else { continue };
                for i in 0..list.GetCount().unwrap_or(0) {
                    let Ok(control) = list.GetSession(i) else { continue };
                    let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                    let active = control.GetState().map(|s| s == AudioSessionStateActive).unwrap_or(false);
                    let pid = control2.GetProcessId().unwrap_or(0);
                    // S_OK (0) means "this is the system sounds session".
                    let system_sounds = control2.IsSystemSoundsSession().0 == 0;
                    out.push(CaptureSession { pid, active, system_sounds });
                }
            }
        }
        Ok(out)
    }
}

#[tauri::command]
pub async fn meeting_now() -> bool {
    tokio::task::spawn_blocking(active).await.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pid: u32, active: bool, system_sounds: bool) -> CaptureSession {
        CaptureSession { pid, active, system_sounds }
    }

    #[test]
    fn another_program_capturing_is_a_meeting() {
        assert!(meeting_active(&[s(4242, true, false)], 100));
    }

    #[test]
    fn byte_itself_is_not_a_meeting() {
        assert!(!meeting_active(&[s(100, true, false)], 100));
    }

    #[test]
    fn an_open_but_idle_session_is_not_a_meeting() {
        assert!(!meeting_active(&[s(4242, false, false)], 100));
    }

    #[test]
    fn system_sounds_and_unknown_processes_never_count() {
        assert!(!meeting_active(&[s(7, true, true), s(0, true, false)], 100));
    }

    #[test]
    fn any_one_active_session_among_many_is_enough() {
        let all = [s(100, true, false), s(5, false, false), s(9000, true, false)];
        assert!(meeting_active(&all, 100));
        assert!(!meeting_active(&[], 100));
    }

    #[test]
    fn a_turned_off_setting_never_asks() {
        assert!(!active_if(false));
    }

    #[test]
    #[ignore = "reads this machine's real microphone sessions; run with --ignored --nocapture"]
    fn live_reads_this_machine() {
        println!("sessions: {:?}  meeting: {}", read_sessions(), active());
    }
}
