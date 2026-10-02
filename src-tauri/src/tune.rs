//! "Tune for this Mac": measures engine settings for a model on this Mac and
//! keeps the fastest. The quick tune runs automatically the first time a model
//! loads (1–2 minutes); the thorough tune (about 5 minutes) and "tune all
//! downloaded models" run from Settings → Engine.
//!
//! Each candidate restarts the engine and measures real speed, because the
//! best settings depend on the chip, memory and model: Speed boost can be 1.5×
//! faster on one Mac and slower on another. The search is greedy: each step
//! changes one setting from the best so far and keeps it only if it's clearly
//! better.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::engine::{Draft, EngineStatus, LaunchOpts};
use crate::models::HelperKind;
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
    /// True once everything finished (successfully or not).
    pub done: bool,
    /// When tuning several models: which one (1-based) of how many.
    pub model_index: u32,
    pub model_count: u32,
}

/// Name of this Mac's chip, to know when tuning was done on another Mac.
pub fn chip_id(state: &AppState) -> String {
    crate::system::system_info(&state.paths.data).chip_id()
}

/// Tuned settings for `key` on this Mac, if measured.
pub async fn saved(state: &AppState, key: &str) -> Option<Tuning> {
    let chip = chip_id(state);
    state.settings.lock().await.tuning.get(key).filter(|t| t.chip == chip).cloned()
}

/// The options to start `key` with: tuned ones if measured, else the defaults.
pub async fn launch_opts(state: &AppState, catalog: &crate::models::Catalog, key: &str) -> LaunchOpts {
    let boost_allowed = state.settings.lock().await.speed_boost;
    let helper = if boost_allowed { helper(state, catalog, key) } else { None };
    let mut opts = match saved(state, key).await {
        Some(t) => to_opts(&t, helper),
        None => LaunchOpts { draft: helper, ..Default::default() },
    };
    // Models that can see load their image adapter when it's downloaded.
    opts.mmproj = catalog.resolve(key).ok().and_then(|(m, _)| crate::models::vision_path(&state.paths.models, m));
    opts
}

fn to_opts(t: &Tuning, helper: Option<Draft>) -> LaunchOpts {
    // A look-ahead tuned for another kind of helper (say the catalog added a
    // speed-up head since) doesn't carry over.
    let same_kind = helper.as_ref().is_some_and(|h| h.kind == t.helper_kind);
    let n_default = t.helper_kind.default_lookahead();
    LaunchOpts {
        draft: helper.filter(|_| t.boost),
        ngram: t.ngram,
        kv_f16: t.kv_f16,
        ubatch: (t.ubatch != 512 && t.ubatch > 0).then_some(t.ubatch),
        flash_attn_off: !t.flash_attn,
        draft_n_max: (same_kind && t.draft_n_max > 0 && t.draft_n_max != n_default).then_some(t.draft_n_max),
        draft_p_min: (same_kind && (t.draft_p_min - 0.75).abs() > 0.001).then_some(t.draft_p_min),
        ..Default::default()
    }
}

/// The downloaded Speed boost helper for `key`'s model, if any: its own
/// speed-up head when it has one, else a small model from the same family.
pub fn helper(state: &AppState, catalog: &crate::models::Catalog, key: &str) -> Option<Draft> {
    let (model, _) = catalog.resolve(key).ok()?;
    let h = crate::models::helper_for(catalog, model, &state.paths.models)?;
    h.installed(&state.paths.models).then(|| Draft { path: h.path(&state.paths.models), kind: h.kind })
}

/// Shorter and longer look-ahead to try for each kind of helper.
fn lookaheads(kind: HelperKind) -> (u32, Option<u32>) {
    match kind {
        HelperKind::Draft => (8, Some(24)),
        HelperKind::Mtp => (2, Some(5)),
        HelperKind::Eagle3 => (5, Some(12)),
        // DSpark drafts a fixed-size block; only shorter makes sense.
        HelperKind::Dspark => (4, None),
    }
}

/// Keeps a candidate only if it's clearly better.
pub fn better(candidate: Speed, best: Speed, kind: Kind) -> bool {
    match kind {
        Kind::Generate(margin) => candidate.generate > best.generate * (1.0 + margin),
        // Reading speed matters for long prompts; don't trade away writing speed for it.
        Kind::Read => candidate.read > best.read * 1.10 && candidate.generate >= best.generate * 0.97,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Generate(f64),
    Read,
}

/// One setting to try: a label, how to change the best options so far (None =
/// not applicable), and what "better" means for it.
struct Candidate {
    label: &'static str,
    change: fn(&LaunchOpts, &Option<Draft>) -> Option<LaunchOpts>,
    kind: Kind,
}

fn plan(thorough: bool) -> Vec<Candidate> {
    let mut c = vec![Candidate {
        label: "Trying Speed boost",
        change: |o, h| h.clone().map(|d| LaunchOpts { draft: Some(d), ..o.clone() }),
        kind: Kind::Generate(0.05),
    }];
    if thorough {
        c.push(Candidate {
            label: "Trying a shorter Speed boost look-ahead",
            change: |o, _| o.draft.as_ref().map(|d| LaunchOpts { draft_n_max: Some(lookaheads(d.kind).0), ..o.clone() }),
            kind: Kind::Generate(0.03),
        });
        c.push(Candidate {
            label: "Trying a longer Speed boost look-ahead",
            change: |o, _| o.draft.as_ref().and_then(|d| lookaheads(d.kind).1).map(|n| LaunchOpts { draft_n_max: Some(n), ..o.clone() }),
            kind: Kind::Generate(0.03),
        });
        c.push(Candidate {
            label: "Letting the helper guess more boldly",
            change: |o, _| o.draft.as_ref().filter(|d| d.kind == HelperKind::Draft).map(|_| LaunchOpts { draft_p_min: Some(0.6), ..o.clone() }),
            kind: Kind::Generate(0.03),
        });
        c.push(Candidate {
            label: "Letting the helper guess more carefully",
            change: |o, _| o.draft.as_ref().filter(|d| d.kind == HelperKind::Draft).map(|_| LaunchOpts { draft_p_min: Some(0.9), ..o.clone() }),
            kind: Kind::Generate(0.03),
        });
    }
    // Lossless and free: reuse text already in the conversation (code, quotes, edits).
    c.push(Candidate { label: "Trying repeated-text guessing", change: |o, _| Some(LaunchOpts { ngram: true, ..o.clone() }), kind: Kind::Generate(0.03) });
    c.push(Candidate { label: "Trying full-precision memory", change: |o, _| Some(LaunchOpts { kv_f16: true, ..o.clone() }), kind: Kind::Generate(0.03) });
    if thorough {
        c.push(Candidate {
            label: "Trying without flash attention",
            change: |o, _| Some(LaunchOpts { kv_f16: true, flash_attn_off: true, ..o.clone() }),
            kind: Kind::Generate(0.03),
        });
        c.push(Candidate { label: "Trying a small reading batch", change: |o, _| Some(LaunchOpts { ubatch: Some(256), ..o.clone() }), kind: Kind::Read });
    }
    c.push(Candidate { label: "Trying faster prompt reading", change: |o, _| Some(LaunchOpts { ubatch: Some(1024), ..o.clone() }), kind: Kind::Read });
    if thorough {
        c.push(Candidate { label: "Trying the largest reading batch", change: |o, _| Some(LaunchOpts { ubatch: Some(2048), ..o.clone() }), kind: Kind::Read });
    }
    c
}

/// Tunes one model (`key`, or the active one). Chat waits while it runs.
pub async fn run(app: &AppHandle, state: &AppState, key: Option<String>, thorough: bool) -> AppResult<Tuning> {
    let _guard = Busy::start(state)?;
    let active = state.settings.lock().await.active_model.clone();
    let key = key.or(active).ok_or_else(|| AppError::msg("choose a model first"))?;
    let result = tune_model(app, state, &key, thorough, (1, 1)).await;
    let _ = app.emit(TUNE_EVENT, done_event(&key));
    result
}

/// Tunes every downloaded chat model that fits, one after another, then goes
/// back to the model that was in use. Returns how many were tuned.
pub async fn run_all(app: &AppHandle, state: &AppState, thorough: bool) -> AppResult<usize> {
    let _guard = Busy::start(state)?;
    let catalog = state.catalog.get();
    let info = crate::system::system_info(&state.paths.data);
    let ctx = state.settings.lock().await.context_size.unwrap_or(crate::engine::DEFAULT_CONTEXT);
    let keys: Vec<String> = catalog
        .models
        .iter()
        .filter(|m| m.role == crate::models::Role::Chat)
        .flat_map(|m| m.variants.iter().map(move |v| (m, v)))
        .filter(|(m, v)| crate::models::is_installed(&state.paths.models, v) && crate::models::plan(m, v, &info, ctx).fit != crate::system::Fit::TooBig)
        .map(|(m, v)| crate::models::key(m, v))
        .collect();
    let active = state.settings.lock().await.active_model.clone();
    let mut tuned = 0;
    for (i, key) in keys.iter().enumerate() {
        match tune_model(app, state, key, thorough, (i as u32 + 1, keys.len() as u32)).await {
            Ok(_) => tuned += 1,
            Err(e) => log::warn!("couldn't tune {key}: {e}"),
        }
    }
    // Back to the model that was in use, with its tuned settings.
    if let Some(a) = active {
        let reserved = state.extras.reserved().await;
        let opts = launch_opts(state, &catalog, &a).await;
        let _ = state.engine.start(app, state.paths.models.clone(), &catalog, &a, ctx.into(), reserved, opts).await;
    }
    let _ = app.emit(TUNE_EVENT, done_event(""));
    Ok(tuned)
}

fn done_event(key: &str) -> TuneProgress {
    TuneProgress { model: key.into(), step: 0, total: 0, label: "Done".into(), done: true, model_index: 0, model_count: 0 }
}

/// Marks BYTE as tuning (chat waits) until dropped.
struct Busy<'a>(&'a AppState);

impl<'a> Busy<'a> {
    fn start(state: &'a AppState) -> AppResult<Self> {
        if state.tuning.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::msg("BYTE is already tuning."));
        }
        Ok(Busy(state))
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.tuning.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

async fn tune_model(app: &AppHandle, state: &AppState, key: &str, thorough: bool, (index, count): (u32, u32)) -> AppResult<Tuning> {
    let ctx = state.settings.lock().await.context_size;
    let catalog = state.catalog.get();
    let (model, variant) = catalog.resolve(key)?;
    let key = crate::models::key(model, variant);
    let reserved = state.extras.reserved().await;
    let boost_allowed = state.settings.lock().await.speed_boost;
    let wanted = if boost_allowed { crate::models::helper_for(&catalog, model, &state.paths.models) } else { None };
    let needs_helper = wanted.as_ref().is_some_and(|h| !h.installed(&state.paths.models));
    let candidates = plan(thorough);
    let total = 1 + needs_helper as u32 + candidates.len() as u32;
    let progress = |step: u32, label: &str| {
        let _ = app.emit(
            TUNE_EVENT,
            TuneProgress { model: key.clone(), step, total, label: label.into(), done: false, model_index: index, model_count: count },
        );
    };
    let start = |opts: LaunchOpts| {
        let (app, catalog, key) = (app.clone(), catalog.clone(), key.clone());
        async move { state.engine.start(&app, state.paths.models.clone(), &catalog, &key, ctx, reserved, opts).await }
    };
    let measure = || async {
        let ep = state.engine.endpoint().await.ok_or_else(|| AppError::msg("the engine isn't running"))?;
        crate::speed::measure_both(&state.local_http, &ep).await
    };

    let mut step = 1;
    // Get the Speed boost helper first (small; skipped if it can't be downloaded).
    if needs_helper {
        progress(step, "Downloading the Speed boost helper");
        if let Err(e) = fetch_helper(app, state, wanted.as_ref().expect("checked above")).await {
            log::info!("speed boost helper not downloaded: {e}");
        }
        step += 1;
    }
    let helper = if boost_allowed { helper(state, &catalog, &key) } else { None };

    let outcome: AppResult<(LaunchOpts, Speed)> = async {
        progress(step, "Measuring the standard settings");
        let mut best_opts = LaunchOpts::default();
        start(best_opts.clone()).await?;
        let mut best = measure().await?;
        log::info!("tune {key}: standard {best:?}");
        for c in &candidates {
            step += 1;
            let Some(opts) = (c.change)(&best_opts, &helper) else { continue };
            if opts == best_opts {
                continue;
            }
            progress(step, c.label);
            if start(opts.clone()).await.is_err() {
                continue; // this setting doesn't work here
            }
            // Settings the engine had to drop (not enough memory, helper mismatch) aren't tested.
            let loaded = state.engine.loaded().await;
            if opts.kv_f16 && !loaded.as_ref().is_some_and(|l| l.kv_f16) {
                continue;
            }
            if opts.draft.is_some() && !matches!(state.engine.status().await, EngineStatus::Ready { boosted: true, .. }) {
                continue;
            }
            let s = measure().await?;
            log::info!("tune {key}: {} {s:?}", c.label);
            if better(s, best, c.kind) {
                (best_opts, best) = (opts, s);
            }
        }
        Ok((best_opts, best))
    }
    .await;

    let (opts, speed) = match outcome {
        Ok(v) => v,
        Err(e) => {
            // Leave the model running with the usual settings.
            let fallback = launch_opts(state, &catalog, &key).await;
            let _ = start(fallback).await;
            return Err(e);
        }
    };
    // Run with the winner (the last candidate may not be it).
    start(opts.clone()).await?;
    let helper_kind = opts.draft.as_ref().or(helper.as_ref()).map(|d| d.kind).unwrap_or_default();
    let t = Tuning {
        boost: opts.draft.is_some(),
        ngram: opts.ngram,
        helper_kind,
        kv_f16: opts.kv_f16,
        ubatch: opts.ubatch.unwrap_or(512),
        tokens_per_sec: speed.generate,
        prompt_per_sec: speed.read,
        chip: chip_id(state),
        tested_at: chrono::Utc::now().timestamp_millis(),
        flash_attn: !opts.flash_attn_off,
        draft_n_max: opts.draft_n_max.unwrap_or(helper_kind.default_lookahead()),
        draft_p_min: opts.draft_p_min.unwrap_or(0.75),
        thorough,
    };
    {
        let mut s = state.settings.lock().await;
        let mut next = s.clone();
        next.tuning.insert(key.clone(), t.clone());
        next.save(&state.paths.settings_file)?;
        *s = next;
    }
    log::info!("tuned {key}: {t:?}");
    Ok(t)
}

/// Downloads the Speed boost helper and waits for it (up to 15 minutes).
async fn fetch_helper(app: &AppHandle, state: &AppState, h: &crate::models::Helper) -> AppResult<()> {
    if h.installed(&state.paths.models) {
        return Ok(());
    }
    if !state.downloads.active_ids().await.contains(&h.key) {
        state.downloads.start(app.clone(), state.net.clone(), state.paths.models.clone(), h.repo.clone(), h.variant.clone(), h.key.clone()).await?;
    }
    for _ in 0..450 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if h.installed(&state.paths.models) {
            return Ok(());
        }
        if !state.downloads.active_ids().await.contains(&h.key) {
            break; // paused or failed
        }
    }
    Err(AppError::msg("helper download didn't finish"))
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

    #[test]
    fn plans_try_the_right_settings() {
        let helper = Some(Draft::model("/m/d.gguf"));
        let base = LaunchOpts::default();
        let quick = plan(false);
        assert_eq!(quick.len(), 4);
        assert!(quick.iter().any(|c| (c.change)(&base, &None).is_some_and(|o| o.ngram)));
        let thorough = plan(true);
        assert!(thorough.len() >= 9);
        // Look-ahead tweaks only apply once Speed boost is on.
        let lookahead = thorough.iter().find(|c| c.label.contains("shorter")).unwrap();
        assert!((lookahead.change)(&base, &helper).is_none());
        let boosted = LaunchOpts { draft: helper.clone(), ..Default::default() };
        assert_eq!((lookahead.change)(&boosted, &helper).unwrap().draft_n_max, Some(8));
        // A model's own head gets its own look-ahead range and no confidence tweaks.
        let head = Some(Draft { path: "/m/mtp.gguf".into(), kind: HelperKind::Mtp });
        let with_head = LaunchOpts { draft: head.clone(), ..Default::default() };
        assert_eq!((lookahead.change)(&with_head, &head).unwrap().draft_n_max, Some(2));
        let bold = thorough.iter().find(|c| c.label.contains("boldly")).unwrap();
        assert!((bold.change)(&with_head, &head).is_none());
        assert!((bold.change)(&boosted, &helper).is_some());
        // Without a helper, Speed boost isn't tried.
        assert!((quick[0].change)(&base, &None).is_none());
        // Turning flash attention off always comes with a full-precision cache.
        let fa = thorough.iter().find(|c| c.label.contains("flash")).unwrap();
        let o = (fa.change)(&base, &helper).unwrap();
        assert!(o.flash_attn_off && o.kv_f16);
    }

    #[test]
    fn tuned_settings_round_trip_to_launch_options() {
        let t = Tuning { boost: true, kv_f16: true, ubatch: 1024, flash_attn: false, draft_n_max: 24, draft_p_min: 0.6, ..Default::default() };
        let o = to_opts(&t, Some(Draft::model("/m/d.gguf")));
        assert_eq!(o.draft, Some(Draft::model("/m/d.gguf")));
        assert!(o.kv_f16 && o.flash_attn_off);
        assert_eq!((o.ubatch, o.draft_n_max, o.draft_p_min), (Some(1024), Some(24), Some(0.6)));
        // Defaults map to "no override", and a missing helper means no boost.
        let d = to_opts(&Tuning::default(), None);
        assert_eq!(d, LaunchOpts::default());
        // A head's default look-ahead (3) is "no override"; a draft-model tuning doesn't carry over to a head.
        let head = Draft { path: "/m/mtp.gguf".into(), kind: HelperKind::Mtp };
        let tm = Tuning { boost: true, helper_kind: HelperKind::Mtp, draft_n_max: 3, ngram: true, ..Default::default() };
        let om = to_opts(&tm, Some(head.clone()));
        assert_eq!((om.draft_n_max, om.ngram), (None, true));
        assert_eq!(to_opts(&t, Some(head)).draft_n_max, None);
    }
}
