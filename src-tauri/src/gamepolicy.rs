//! One place that decides what BYTE does while the PC is busy with a game (setting `gameAware`).
//!
//! While a game runs BYTE answers from BYTE Cloud, stays quiet (no chimes, no spoken notices except timers) and starts
//! nothing heavy. "Hey BYTE" answers used to decide this alone (Settings → Voice → Answers from: Automatic); this
//! generalises it. The caller reads the busy state ([`crate::busy::read`]) and asks [`game_policy`].
//!
//! The quiet flag is for the voice code (wake.rs / tts.rs) to read; the engine choice is what the chat store uses.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// Answer on this PC.
    Local,
    /// Answer from BYTE Cloud (only if an account is connected; the caller checks that).
    Cloud,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub engine: Engine,
    /// No chimes and no spoken notices (a timer still rings).
    pub quiet: bool,
    /// Heavy background work (a model download, indexing, tuning) may start.
    pub heavy_ok: bool,
}

/// `game_aware` is the setting, `engine` the Hey BYTE engine setting ("local" | "cloud" | "auto"), `busy` the PC's state
/// (`None` when it could not be read: then nothing special happens), `local_ready` whether a model is loaded on this PC.
pub fn game_policy(game_aware: bool, engine: &str, busy: Option<bool>, local_ready: bool) -> Policy {
    let busy = busy == Some(true);
    let gaming = game_aware && busy;
    let engine = match engine {
        "cloud" => Engine::Cloud,
        "local" => Engine::Local,
        // Automatic: the cloud while a game runs, or when no model is loaded on this PC.
        _ if busy || !local_ready => Engine::Cloud,
        _ => Engine::Local,
    };
    Policy { engine, quiet: gaming, heavy_ok: !gaming }
}

/// Reads the live state. Blocking (runs `nvidia-smi` and asks Windows); call from a blocking task.
pub fn current(game_aware: bool, engine: &str, local_ready: bool) -> Policy {
    // Nothing is read at all when the setting is off and the engine is not automatic.
    let busy = (game_aware || engine == "auto").then(|| crate::busy::read().busy);
    game_policy(game_aware, engine, busy, local_ready)
}

#[tauri::command]
pub async fn game_policy_now(state: tauri::State<'_, crate::state::AppState>, local_ready: bool) -> Result<Policy, String> {
    let (aware, engine) = {
        let s = state.settings.lock().await;
        (s.game_aware, s.hey_byte_engine.clone())
    };
    tokio::task::spawn_blocking(move || current(aware, &engine, local_ready)).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_means_cloud_quiet_and_nothing_heavy() {
        let p = game_policy(true, "auto", Some(true), true);
        assert_eq!(p, Policy { engine: Engine::Cloud, quiet: true, heavy_ok: false });
    }

    #[test]
    fn no_game_means_this_pc_and_normal_behaviour() {
        assert_eq!(game_policy(true, "auto", Some(false), true), Policy { engine: Engine::Local, quiet: false, heavy_ok: true });
        assert_eq!(game_policy(true, "auto", None, true).quiet, false, "unreadable state is not a game");
    }

    #[test]
    fn explicit_engine_choices_are_kept_but_the_quiet_still_applies() {
        let p = game_policy(true, "local", Some(true), true);
        assert_eq!((p.engine, p.quiet, p.heavy_ok), (Engine::Local, true, false));
        assert_eq!(game_policy(true, "cloud", Some(false), true).engine, Engine::Cloud);
    }

    #[test]
    fn no_local_model_means_cloud_on_automatic() {
        assert_eq!(game_policy(true, "auto", Some(false), false).engine, Engine::Cloud);
        assert_eq!(game_policy(false, "auto", None, false).engine, Engine::Cloud);
    }

    #[test]
    fn with_the_setting_off_nothing_goes_quiet() {
        let p = game_policy(false, "auto", Some(true), true);
        assert_eq!((p.quiet, p.heavy_ok), (false, true));
        assert_eq!(p.engine, Engine::Cloud, "automatic still means the cloud while a game runs, as before");
        assert_eq!(game_policy(false, "local", Some(true), true).engine, Engine::Local);
    }
}
