//! Is this PC busy with something that wants the graphics card, like a game? "Hey BYTE" can answer from BYTE Cloud then, so
//! the answer is not fought over with the game for the card and the game does not stutter (Settings → Voice → Answers from:
//! Automatic).
//!
//! Windows tells a program whether another one is running full screen (`SHQueryUserNotificationState`, the same signal that
//! holds back notifications during a game); an NVIDIA card also reports how hard it is working. Nothing is watched in the
//! background: this is read once when a spoken question is asked.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Busy {
    pub busy: bool,
    /// In plain words, for the Settings page and the log.
    pub reason: String,
}

/// The card works this hard (percent) before BYTE counts it as busy.
const GPU_BUSY_PERCENT: u32 = 60;

/// `shell` is Windows' notification state (2 busy full screen, 3 full-screen Direct3D game, 4 presentation), `gpu` the
/// busiest card's load in percent. The reason, if the PC counts as busy.
pub fn decide(shell: Option<i32>, gpu: Option<u32>) -> Option<String> {
    match shell {
        Some(3) => return Some("a full-screen game is running".into()),
        Some(2) => return Some("a full-screen program is in front".into()),
        _ => {}
    }
    gpu.filter(|g| *g >= GPU_BUSY_PERCENT).map(|g| format!("the graphics card is {g}% busy"))
}

/// The busiest NVIDIA card's load from `nvidia-smi` output (one percent per line).
pub fn busiest(smi_output: &str) -> Option<u32> {
    smi_output.lines().filter_map(|l| l.trim().trim_end_matches('%').trim().parse::<u32>().ok()).max()
}

#[cfg(windows)]
fn shell_state() -> Option<i32> {
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    unsafe { SHQueryUserNotificationState().ok().map(|s| s.0) }
}

#[cfg(not(windows))]
fn shell_state() -> Option<i32> {
    None
}

fn gpu_load() -> Option<u32> {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args(["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"]);
    // No console window flashing up over a game.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    busiest(&String::from_utf8_lossy(&out.stdout))
}

pub fn read() -> Busy {
    match decide(shell_state(), gpu_load()) {
        Some(reason) => Busy { busy: true, reason },
        None => Busy { busy: false, reason: "nothing is using the graphics card heavily".into() },
    }
}

#[tauri::command]
pub async fn pc_busy() -> Busy {
    tokio::task::spawn_blocking(read).await.unwrap_or(Busy { busy: false, reason: "could not tell".into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_or_a_hard_working_card_makes_the_pc_busy() {
        assert_eq!(decide(Some(3), None).as_deref(), Some("a full-screen game is running"));
        assert_eq!(decide(Some(2), Some(5)).as_deref(), Some("a full-screen program is in front"));
        assert_eq!(decide(Some(5), Some(87)).as_deref(), Some("the graphics card is 87% busy"));
        assert_eq!(decide(Some(5), Some(59)), None);
        assert_eq!(decide(Some(7), None), None, "an ordinary app in front is not a game");
        assert_eq!(decide(None, None), None, "when nothing can be read, the PC is not called busy");
    }

    #[test]
    fn nvidia_smi_output_is_read_for_the_busiest_card() {
        assert_eq!(busiest("12\n 97 \n"), Some(97));
        assert_eq!(busiest("45 %\r\n"), Some(45));
        assert_eq!(busiest(""), None);
        assert_eq!(busiest("No devices were found"), None);
    }

    #[test]
    #[ignore = "reads this machine's real full-screen state and graphics load; run with --ignored --nocapture"]
    fn live_reads_this_machine() {
        let b = read();
        println!("busy: {b:?}  (shell state {:?}, gpu load {:?})", shell_state(), gpu_load());
        assert!(!b.reason.is_empty());
    }
}
