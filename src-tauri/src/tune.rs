//! "Tune for this Mac": measures a few engine settings for one model on this
//! Mac and keeps the fastest. Runs automatically the first time a model loads
//! (about 1–2 minutes), and again from Settings → Engine.
//!
//! Each candidate restarts the engine and measures real speed, because the
//! best settings depend on the chip, memory and model: Speed boost can be 1.5×
//! faster on one Mac and slower on another.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::engine::{EngineStatus, LaunchOpts};
use crate::error::{AppError, AppResult};
use crate::settings::Tuning;
use crate::speed::Speed;
use crate::state::AppState;

pub const TUNE_EVENT: &str = "engine://tune";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuneProgress {
    pub model: String,
    pub step: u32,
    pub total: u32,
    pub label: String,
    /// True once finished (successfully or not).
    pub done: bool,
}

/// Name of this Mac's chip, to know when tuning was done on another Mac.
pub fn chip_id(state: &AppState) -> String {
    let c = crate::system::system_info(&state.paths.data).chip_info;
    format!("{} {}", c.name, c.gpu_cores.map(|g| format!("{g}-core GPU")).unwrap_or_default()).trim().to_string()
}

/// Tuned settings for `key` on this Mac, if measured.
pub async fn saved(state: &AppState, key: &str) -> Option<Tuning> {
    let chip = chip_id(state);
    state.settings.lock().await.tuning.get(key).filter(|t| t.chip == chip).cloned()
}

/// The options to start `key` with: tuned ones if measured, else the defaults.
pub async fn launch_opts(state: &AppState, catalog: &crate::models::Catalog, key: &str) -> LaunchOpts {
    let boost_allowed = state.settings.lock().await.speed_boost;
    let helper = if boost_allowed { helper_path(state, catalog, key) } else { None };
    match saved(state, key).await {
        Some(t) => LaunchOpts { draft: helper.filter(|_| t.boost), kv_f16: t.kv_f16, ubatch: (t.ubatch != 512 && t.ubatch > 0).then_some(t.ubatch) },
        None => LaunchOpts { draft: helper, ..Default::default() },
    }
}

/// The downloaded Speed boost helper for `key`'s model, if any.
pub fn helper_path(state: &AppState, catalog: &crate::models::Catalog, key: &str) -> Option<std::path::PathBuf> {
    let (model, _) = catalog.resolve(key).ok()?;
    let d = crate::models::drafter_for(catalog, model)?;
    let (v, installed) = crate::models::drafter_variant(d, &state.paths.models);
    installed.then(|| crate::models::entry_path(&state.paths.models, v))
}

/// Downloads the Speed boost helper for `model` and waits for it (up to 15 minutes).
async fn fetch_helper(app: &AppHandle, state: &AppState, catalog: &crate::models::Catalog, model: &crate::models::CatalogModel) -> AppResult<()> {
    let d = crate::models::drafter_for(catalog, model).ok_or_else(|| AppError::msg("no helper"))?;
    let (v, installed) = crate::models::drafter_variant(d, &state.paths.models);
    if installed {
        return Ok(());
    }
    let key = crate::models::key(d, v);
    if !state.downloads.active_ids().await.contains(&key) {
        state.downloads.start(app.clone(), state.net.clone(), state.paths.models.clone(), d.repo.clone(), v.clone(), key.clone()).await?;
    }
    for _ in 0..450 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if crate::models::is_installed(&state.paths.models, v) {
            return Ok(());
        }
        if !state.downloads.active_ids().await.contains(&key) {
            break; // paused or failed
        }
    }
    Err(AppError::msg("helper download didn't finish"))
}

/// Keeps a candidate only if it's clearly better.
pub fn better(candidate: Speed, best: Speed, kind: Kind) -> bool {
    match kind {
        Kind::Generate(margin) => candidate.generate > best.generate * (1.0 + margin),
        // Reading speed matters for long prompts; don't trade away writing speed for it.
        Kind::Read => candidate.read > best.read * 1.10 && candidate.generate >= best.generate * 0.97,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Generate(f64),
    Read,
}

/// Measures and saves the fastest settings for the active model.
pub async fn run(app: &AppHandle, state: &AppState) -> AppResult<Tuning> {
    use std::sync::atomic::Ordering;
    if state.tuning.swap(true, Ordering::SeqCst) {
        return Err(AppError::msg("BYTE is already tuning."));
    }
    let result = run_inner(app, state).await;
    state.tuning.store(false, Ordering::SeqCst);
    result
}

async fn run_inner(app: &AppHandle, state: &AppState) -> AppResult<Tuning> {
    let (key, ctx) = {
        let s = state.settings.lock().await;
        (s.active_model.clone(), s.context_size)
    };
    let key = key.ok_or_else(|| AppError::msg("choose a model first"))?;
    let catalog = state.catalog.get();
    let (model, variant) = catalog.resolve(&key)?;
    let key = crate::models::key(model, variant);
    let reserved = state.extras.reserved().await;
    let boost_allowed = state.settings.lock().await.speed_boost;
    let needs_helper = boost_allowed && helper_path(state, &catalog, &key).is_none() && crate::models::drafter_for(&catalog, model).is_some();
    let total = 3 + (boost_allowed && crate::models::drafter_for(&catalog, model).is_some()) as u32 + needs_helper as u32;
    let progress = |step: u32, label: &str, done: bool| {
        let _ = app.emit(TUNE_EVENT, TuneProgress { model: key.clone(), step, total, label: label.into(), done });
    };
    // Get the Speed boost helper first (small; skipped if it can't be downloaded).
    if needs_helper {
        progress(1, "Downloading the Speed boost helper", false);
        if let Err(e) = fetch_helper(app, state, &catalog, model).await {
            log::info!("speed boost helper not downloaded: {e}");
        }
    }
    let helper = if boost_allowed { helper_path(state, &catalog, &key) } else { None };
    let first = 1 + needs_helper as u32;
    let start = |opts: LaunchOpts| {
        let (app, catalog, key) = (app.clone(), catalog.clone(), key.clone());
        async move { state.engine.start(&app, state.paths.models.clone(), &catalog, &key, ctx, reserved, opts).await }
    };
    let measure = || async {
        let ep = state.engine.endpoint().await.ok_or_else(|| AppError::msg("the engine isn't running"))?;
        crate::speed::measure_both(&state.local_http, &ep).await
    };

    let outcome: AppResult<(LaunchOpts, Speed)> = async {
        let mut step = first;
        progress(step, "Measuring the standard settings", false);
        let mut best_opts = LaunchOpts::default();
        start(best_opts.clone()).await?;
        let mut best = measure().await?;
        log::info!("tune {key}: standard {best:?}");

        if let Some(h) = helper.clone() {
            step += 1;
            progress(step, "Trying Speed boost", false);
            let opts = LaunchOpts { draft: Some(h), ..best_opts.clone() };
            start(opts.clone()).await?;
            if matches!(state.engine.status().await, EngineStatus::Ready { boosted: true, .. }) {
                let s = measure().await?;
                log::info!("tune {key}: boost {s:?}");
                if better(s, best, Kind::Generate(0.05)) {
                    (best_opts, best) = (opts, s);
                }
            }
        }

        step += 1;
        progress(step, "Trying full-precision memory", false);
        let opts = LaunchOpts { kv_f16: true, ..best_opts.clone() };
        start(opts.clone()).await?;
        if state.engine.loaded().await.is_some_and(|l| l.kv_f16) {
            let s = measure().await?;
            log::info!("tune {key}: f16 KV {s:?}");
            if better(s, best, Kind::Generate(0.03)) {
                (best_opts, best) = (opts, s);
            }
        }

        step += 1;
        progress(step, "Trying faster prompt reading", false);
        let opts = LaunchOpts { ubatch: Some(1024), ..best_opts.clone() };
        start(opts.clone()).await?;
        let s = measure().await?;
        log::info!("tune {key}: ubatch 1024 {s:?}");
        if better(s, best, Kind::Read) {
            (best_opts, best) = (opts, s);
        }
        Ok((best_opts, best))
    }
    .await;

    let (opts, speed) = match outcome {
        Ok(v) => v,
        Err(e) => {
            progress(total, "Tuning stopped", true);
            // Leave the model running with the usual settings.
            let fallback = launch_opts(state, &catalog, &key).await;
            let _ = start(fallback).await;
            return Err(e);
        }
    };
    // Run with the winner (the last candidate may not be it).
    start(opts.clone()).await?;
    let t = Tuning {
        boost: opts.draft.is_some(),
        kv_f16: opts.kv_f16,
        ubatch: opts.ubatch.unwrap_or(512),
        tokens_per_sec: speed.generate,
        prompt_per_sec: speed.read,
        chip: chip_id(state),
        tested_at: chrono::Utc::now().timestamp_millis(),
    };
    {
        let mut s = state.settings.lock().await;
        let mut next = s.clone();
        next.tuning.insert(key.clone(), t.clone());
        next.save(&state.paths.settings_file)?;
        *s = next;
    }
    progress(total, "Done", true);
    log::info!("tuned {key}: {t:?}");
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_clear_wins() {
        let base = Speed { generate: 13.0, read: 300.0 };
        assert!(better(Speed { generate: 20.0, read: 290.0 }, base, Kind::Generate(0.05)));
        assert!(!better(Speed { generate: 13.4, read: 400.0 }, base, Kind::Generate(0.05)));
        assert!(better(Speed { generate: 12.8, read: 360.0 }, base, Kind::Read));
        // Faster reading isn't worth slower writing.
        assert!(!better(Speed { generate: 12.0, read: 400.0 }, base, Kind::Read));
    }
}
