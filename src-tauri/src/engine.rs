//! Supervises the bundled `llama-server` process: picks a port, starts it with
//! the right model and context, waits until it is healthy, warms it up, and
//! restarts it if it crashes.
//!
//! BYTE can run several models at once: the *main* engine serves the active
//! model and reports on `engine://status`; `Extras` holds up to three more
//! engines ("loaded alongside") that each run their own llama-server. The RAM
//! planner counts memory already used by the others before starting one.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::models::{self, Catalog, HelperKind};
use crate::system;

pub const STATUS_EVENT: &str = "engine://status";
/// Emitted (no payload) whenever an extra engine changes state.
pub const EXTRAS_EVENT: &str = "engine://extras";
/// Context for models loaded alongside the main one: enough for normal chats
/// while keeping their memory use modest.
pub const EXTRA_CONTEXT: u32 = 8192;
pub const MAX_EXTRAS: usize = 3;
const SIDECAR: &str = "llama-server";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_RESTARTS: u32 = 3;
const LOG_LINES: usize = 400;
pub const DEFAULT_CONTEXT: u32 = 16384;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum EngineStatus {
    /// No chat model downloaded yet.
    NoModel,
    Stopped,
    Starting { model: String },
    /// `boosted`: a helper model is speeding up generation (speculative decoding).
    Ready { model: String, context: u32, boosted: bool },
    Error { message: String },
}

/// What to run: model key, file to load, and context size.
#[derive(Debug, Clone)]
struct Launch {
    key: String,
    path: PathBuf,
    context: u32,
    opts: LaunchOpts,
}

/// The Speed boost helper for one engine start: a small same-family model or
/// the model's own speed-up head (see `models::Helper`).
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub path: PathBuf,
    pub kind: HelperKind,
}

#[cfg(test)]
impl Draft {
    pub fn model(path: impl Into<PathBuf>) -> Self {
        Draft { path: path.into(), kind: HelperKind::Draft }
    }
}

/// Performance options for one engine start (see `tune.rs`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LaunchOpts {
    /// Helper for speculative decoding ("Speed boost").
    pub draft: Option<Draft>,
    /// Also guess from text already in the conversation (llama.cpp `ngram-mod`):
    /// no download, big wins when answers repeat code or quoted text.
    pub ngram: bool,
    /// Full-precision KV cache instead of 8-bit (faster on some Macs, uses more memory).
    pub kv_f16: bool,
    /// Physical batch size for prompt processing (llama.cpp default 512).
    pub ubatch: Option<u32>,
    /// Turn flash attention off (only possible with a full-precision KV cache).
    pub flash_attn_off: bool,
    /// Speed boost look-ahead: how many words the helper drafts, and how sure it must be.
    pub draft_n_max: Option<u32>,
    pub draft_p_min: Option<f32>,
    /// Set by the memory planner, not tuning: expert layers kept for the CPU
    /// and, for dense models in stretch mode, how many layers go on the GPU.
    pub cpu_moe_layers: u32,
    pub gpu_layers: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub context: u32,
}

/// What an engine is running and how much memory it was planned to use.
#[derive(Debug, Clone)]
pub struct LoadInfo {
    pub key: String,
    pub context: u32,
    pub needed_bytes: u64,
    /// Whether the full-precision KV cache was actually used (memory allowing).
    pub kv_f16: bool,
}

struct Inner {
    status: EngineStatus,
    loaded: Option<LoadInfo>,
    child: Option<CommandChild>,
    endpoint: Option<Endpoint>,
    /// Bumped on every start/stop so stale exit events are ignored.
    generation: u64,
    restarts: u32,
    log: VecDeque<String>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Mutex<Inner>>,
    http: reqwest::Client,
    /// Records the running engine's PID so a crash can be cleaned up on the next launch.
    pid_file: PathBuf,
    /// The main engine reports status on `engine://status`; extras on `engine://extras`.
    primary: bool,
}

impl Engine {
    pub fn new(pid_file: PathBuf) -> Self {
        Self::with_role(pid_file, true)
    }

    fn with_role(pid_file: PathBuf, primary: bool) -> Self {
        Engine {
            pid_file,
            primary,
            inner: Arc::new(Mutex::new(Inner {
                loaded: None,
                status: EngineStatus::Stopped,
                child: None,
                endpoint: None,
                generation: 0,
                restarts: 0,
                log: VecDeque::with_capacity(LOG_LINES),
            })),
            http: crate::chat::local_client(),
        }
    }

    pub async fn status(&self) -> EngineStatus {
        self.inner.lock().await.status.clone()
    }

    pub async fn endpoint(&self) -> Option<Endpoint> {
        self.inner.lock().await.endpoint.clone()
    }

    pub async fn log_tail(&self) -> Vec<String> {
        self.inner.lock().await.log.iter().cloned().collect()
    }

    /// The model this engine runs (or is starting) and its planned memory use.
    pub async fn loaded(&self) -> Option<LoadInfo> {
        self.inner.lock().await.loaded.clone()
    }

    async fn set_status(&self, app: &AppHandle, status: EngineStatus) {
        self.inner.lock().await.status = status.clone();
        if self.primary {
            let _ = app.emit(STATUS_EVENT, status);
        } else {
            let _ = app.emit(EXTRAS_EVENT, ());
        }
    }

    /// Starts (or restarts) the engine with a catalog model key ("id:quant").
    /// `reserved` is memory already used by other running engines.
    pub async fn start(
        &self,
        app: &AppHandle,
        models_dir: PathBuf,
        catalog: &Catalog,
        key: &str,
        ctx_override: Option<u32>,
        reserved: u64,
        opts: LaunchOpts,
    ) -> AppResult<()> {
        let LaunchOpts { draft, ngram, mut kv_f16, ubatch, mut flash_attn_off, draft_n_max, draft_p_min, .. } = opts;
        let (model, variant) = catalog.resolve(key)?;
        if !models::is_installed(&models_dir, variant) {
            self.set_status(app, EngineStatus::NoModel).await;
            return Err(AppError::msg(format!("{} ({}) is not downloaded yet", model.name, variant.quant)));
        }
        let info = system::system_info(&models_dir).minus(reserved);
        let desired_ctx = ctx_override.unwrap_or(DEFAULT_CONTEXT);
        // The helper model needs its own memory; use it only if everything still fits comfortably.
        let draft = draft.filter(|d| {
            let extra = std::fs::metadata(&d.path).map(|m| m.len()).unwrap_or(u64::MAX / 4) + DRAFT_OVERHEAD;
            let with = models::plan(model, variant, &info.clone().minus(extra), desired_ctx);
            let without = models::plan(model, variant, &info, desired_ctx);
            let ok = with.fit != system::Fit::TooBig && with.context >= without.context.min(DEFAULT_CONTEXT);
            if !ok {
                log::info!("speed boost skipped: not enough memory next to {}", model.name);
            }
            ok
        });
        let draft_extra = draft.as_ref().and_then(|d| std::fs::metadata(&d.path).ok()).map(|m| m.len() + DRAFT_OVERHEAD).unwrap_or(0);
        let plan = models::plan(model, variant, &info.clone().minus(draft_extra), desired_ctx);
        // A full-precision KV cache takes about twice the memory: only when it still fits the same context.
        let kv_extra = model.arch.kv_bytes_per_token() * plan.context as u64;
        if kv_f16 {
            let bigger = models::plan(model, variant, &info.clone().minus(draft_extra + kv_extra), desired_ctx);
            if bigger.fit == system::Fit::TooBig || bigger.context < plan.context {
                log::info!("full-precision KV cache skipped: not enough memory");
                kv_f16 = false;
            }
        }
        // An 8-bit KV cache needs flash attention.
        if !kv_f16 {
            flash_attn_off = false;
        }
        let extra = draft_extra + if kv_f16 { kv_extra } else { 0 };
        if plan.offloaded() {
            log::info!("{} runs partly on the CPU: {}", model.name, plan.note);
        }
        if plan.fit == system::Fit::TooBig {
            let message = if reserved > 0 {
                format!("{} doesn't fit next to the models already loaded. Unload one first, or pick a smaller version.", model.name)
            } else {
                format!("{} can't run on this Mac. {}", model.name, plan.note)
            };
            self.set_status(app, EngineStatus::Error { message: message.clone() }).await;
            return Err(AppError::msg(message));
        }
        self.stop().await;
        {
            let mut inner = self.inner.lock().await;
            inner.restarts = 0;
            inner.loaded = Some(LoadInfo { key: models::key(model, variant), context: plan.context, needed_bytes: plan.needed_bytes + extra, kv_f16 });
        }
        let launch = Launch {
            key: models::key(model, variant),
            path: models::entry_path(&models_dir, variant),
            context: plan.context,
            opts: LaunchOpts {
                draft,
                ngram,
                kv_f16,
                ubatch,
                flash_attn_off,
                draft_n_max,
                draft_p_min,
                cpu_moe_layers: plan.cpu_moe_layers,
                gpu_layers: plan.gpu_layers,
            },
        };
        let result = self.spawn(app.clone(), launch.clone()).await;
        // A helper that doesn't work with this engine build must never leave
        // the user without a model: start again without it.
        if result.is_err() && (launch.opts.draft.is_some() || launch.opts.ngram) {
            log::warn!("engine didn't start with Speed boost; starting {} without it", launch.key);
            let mut plain = launch;
            plain.opts.draft = None;
            plain.opts.ngram = false;
            return self.spawn(app.clone(), plain).await;
        }
        result
    }

    /// Boxed with an explicit type so the crash-restart path (which calls back
    /// into `spawn`) doesn't create a recursive `impl Future` type.
    fn spawn(&self, app: AppHandle, launch: Launch) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<()>> + Send + '_>> {
        Box::pin(self.spawn_inner(app, launch))
    }

    async fn spawn_inner(&self, app: AppHandle, launch: Launch) -> AppResult<()> {
        let context = launch.context;
        self.set_status(&app, EngineStatus::Starting { model: launch.key.clone() }).await;
        let port = free_port()?;
        let api_key = uuid::Uuid::new_v4().simple().to_string();
        let mut args = server_args(&launch.path, port, &api_key, &launch.key, context);
        apply_opts(&mut args, &launch.opts);

        let command = match app.shell().sidecar(SIDECAR) {
            Ok(c) => c,
            Err(e) => {
                let message = format!("The AI engine is missing from this build ({e}).");
                self.set_status(&app, EngineStatus::Error { message: message.clone() }).await;
                return Err(AppError::msg(message));
            }
        };
        let (mut rx, child) = match command.args(args).spawn() {
            Ok(v) => v,
            Err(e) => {
                let message = format!("Couldn't start the AI engine: {e}");
                self.set_status(&app, EngineStatus::Error { message: message.clone() }).await;
                return Err(AppError::msg(message));
            }
        };
        let _ = std::fs::write(&self.pid_file, child.pid().to_string());
        let generation = {
            let mut inner = self.inner.lock().await;
            inner.generation += 1;
            inner.child = Some(child);
            inner.endpoint = None;
            inner.generation
        };
        log::info!("llama-server starting on port {port} with {} (ctx {context})", launch.key);

        // Pump process output into the ring buffer and watch for exits.
        let this = self.clone();
        let app_for_events = app.clone();
        let relaunch = launch.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(ev) = rx.recv().await {
                match ev {
                    CommandEvent::Stdout(line) | CommandEvent::Stderr(line) => {
                        let text = String::from_utf8_lossy(&line).trim_end().to_string();
                        let mut inner = this.inner.lock().await;
                        if inner.log.len() == LOG_LINES {
                            inner.log.pop_front();
                        }
                        inner.log.push_back(text);
                    }
                    CommandEvent::Terminated(payload) => {
                        this.on_exit(&app_for_events, relaunch, generation, payload.code).await;
                        break;
                    }
                    CommandEvent::Error(err) => log::warn!("llama-server: {err}"),
                    _ => {}
                }
            }
        });

        let base_url = format!("http://127.0.0.1:{port}");
        match self.wait_healthy(&base_url, generation).await {
            Ok(()) => {
                let endpoint = Endpoint { base_url, api_key, model: launch.key.clone(), context };
                self.warm_up(&endpoint).await;
                {
                    let mut inner = self.inner.lock().await;
                    if inner.generation != generation {
                        return Ok(());
                    }
                    inner.endpoint = Some(endpoint);
                }
                // llama-server keeps running without speculation if the helper doesn't match.
                let boosted = (launch.opts.draft.is_some() && !speculation_failed(&self.log_tail().await)) || launch.opts.ngram;
                log::info!("engine ready: {} with {context}-token context (speed boost: {boosted})", launch.key);
                self.set_status(&app, EngineStatus::Ready { model: launch.key.clone(), context, boosted }).await;
                Ok(())
            }
            Err(e) => {
                let still_current = self.inner.lock().await.generation == generation;
                if still_current {
                    let tail = self.log_tail().await;
                    let hint = diagnose(&tail);
                    let message = format!("The AI engine didn't start: {e}.{hint}");
                    self.set_status(&app, EngineStatus::Error { message: message.clone() }).await;
                    self.stop().await;
                    return Err(AppError::msg(message));
                }
                Ok(())
            }
        }
    }

    async fn on_exit(&self, app: &AppHandle, launch: Launch, generation: u64, code: Option<i32>) {
        let restart = {
            let mut inner = self.inner.lock().await;
            if inner.generation != generation {
                return; // Intentional stop or already replaced.
            }
            inner.child = None;
            inner.endpoint = None;
            // Only crashes after a successful start are retried; load failures
            // are reported by `wait_healthy` with a diagnosis instead.
            if !matches!(inner.status, EngineStatus::Ready { .. }) {
                return;
            }
            inner.restarts += 1;
            inner.restarts <= MAX_RESTARTS
        };
        log::warn!("llama-server exited unexpectedly (code {code:?})");
        if restart {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let _ = self.spawn(app.clone(), launch).await;
        } else {
            let hint = diagnose(&self.log_tail().await);
            self.set_status(app, EngineStatus::Error { message: format!("The AI engine keeps stopping (exit code {code:?}).{hint}") }).await;
        }
    }

    async fn wait_healthy(&self, base_url: &str, generation: u64) -> AppResult<()> {
        let deadline = tokio::time::Instant::now() + HEALTH_TIMEOUT;
        loop {
            if self.inner.lock().await.generation != generation {
                return Err(AppError::msg("replaced by a newer start"));
            }
            if self.inner.lock().await.child.is_none() {
                return Err(AppError::msg("process exited while loading"));
            }
            if let Ok(r) = self.http.get(format!("{base_url}/health")).send().await {
                if r.status().is_success() {
                    return Ok(());
                }
            }
            if tokio::time::Instant::now() > deadline {
                return Err(AppError::msg("timed out while loading the model"));
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    /// One tiny request so Metal kernels are compiled and the first real
    /// answer starts immediately.
    async fn warm_up(&self, ep: &Endpoint) {
        let body = serde_json::json!({
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1,
            "chat_template_kwargs": {"enable_thinking": false},
        });
        let _ = self
            .http
            .post(format!("{}/v1/chat/completions", ep.base_url))
            .bearer_auth(&ep.api_key)
            .timeout(Duration::from_secs(60))
            .json(&body)
            .send()
            .await;
    }

    pub async fn stop(&self) {
        let child = {
            let mut inner = self.inner.lock().await;
            inner.generation += 1;
            inner.endpoint = None;
            inner.loaded = None;
            inner.child.take()
        };
        if let Some(c) = child {
            let _ = c.kill();
        }
        let _ = std::fs::remove_file(&self.pid_file);
    }

    /// Synchronous kill used while the app is exiting.
    pub fn kill_now(&self) {
        if let Ok(mut inner) = self.inner.try_lock() {
            inner.generation += 1;
            if let Some(c) = inner.child.take() {
                let _ = c.kill();
            }
        } else if let Some(pid) = read_pid(&self.pid_file) {
            kill_if_engine(pid);
        }
        let _ = std::fs::remove_file(&self.pid_file);
    }

    /// Kills an engine left running by a previous BYTE that crashed or was
    /// force-quit, so it doesn't keep gigabytes of RAM busy.
    pub fn reap_stale(&self) {
        if let Some(pid) = read_pid(&self.pid_file) {
            if kill_if_engine(pid) {
                log::warn!("stopped a leftover engine process ({pid}) from a previous session");
            }
        }
        let _ = std::fs::remove_file(&self.pid_file);
    }
}

/// A model running in memory, for the "Loaded models" list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModel {
    pub key: String,
    /// True for the main (active) model.
    pub primary: bool,
    pub status: EngineStatus,
    pub context: u32,
    pub needed_bytes: u64,
}

impl Engine {
    pub async fn describe(&self) -> Option<LoadedModel> {
        let inner = self.inner.lock().await;
        let l = inner.loaded.clone()?;
        Some(LoadedModel { key: l.key, primary: self.primary, status: inner.status.clone(), context: l.context, needed_bytes: l.needed_bytes })
    }
}

/// Engines for models loaded alongside the main one, keyed by model key.
pub struct Extras {
    dir: PathBuf,
    slots: Mutex<Vec<(String, Engine)>>,
}

impl Extras {
    pub fn new(dir: PathBuf) -> Self {
        Extras { dir, slots: Mutex::new(Vec::new()) }
    }

    pub async fn get(&self, key: &str) -> Option<Engine> {
        self.slots.lock().await.iter().find(|(k, _)| k == key).map(|(_, e)| e.clone())
    }

    pub async fn all(&self) -> Vec<Engine> {
        self.slots.lock().await.iter().map(|(_, e)| e.clone()).collect()
    }

    /// Memory planned for all extra engines.
    pub async fn reserved(&self) -> u64 {
        let mut total = 0;
        for e in self.all().await {
            total += e.loaded().await.map(|l| l.needed_bytes).unwrap_or(0);
        }
        total
    }

    /// Adds an engine slot for `key` (not started yet).
    pub async fn add(&self, key: &str) -> AppResult<Engine> {
        let mut slots = self.slots.lock().await;
        if let Some((_, e)) = slots.iter().find(|(k, _)| k == key) {
            return Ok(e.clone());
        }
        if slots.len() >= MAX_EXTRAS {
            return Err(AppError::msg(format!("Up to {} extra models can be loaded at once. Unload one first.", MAX_EXTRAS)));
        }
        // Reuse the lowest free PID-file number so stale ones are overwritten.
        let n = (1..).find(|n| !slots.iter().any(|(_, e)| e.pid_file == self.pid_file(*n))).unwrap_or(1);
        let engine = Engine::with_role(self.pid_file(n), false);
        slots.push((key.to_string(), engine.clone()));
        Ok(engine)
    }

    /// Stops and forgets the engine for `key`. Returns true if one was loaded.
    pub async fn remove(&self, app: &AppHandle, key: &str) -> bool {
        let engine = {
            let mut slots = self.slots.lock().await;
            let idx = slots.iter().position(|(k, _)| k == key);
            idx.map(|i| slots.remove(i).1)
        };
        match engine {
            Some(e) => {
                e.stop().await;
                let _ = app.emit(EXTRAS_EVENT, ());
                true
            }
            None => false,
        }
    }

    fn pid_file(&self, n: usize) -> PathBuf {
        self.dir.join(format!("engine-extra-{n}.pid"))
    }

    pub fn kill_all_now(&self) {
        if let Ok(slots) = self.slots.try_lock() {
            for (_, e) in slots.iter() {
                e.kill_now();
            }
        } else {
            self.reap_stale();
        }
    }

    /// Kills extra engines left over from a previous session.
    pub fn reap_stale(&self) {
        for n in 1..=MAX_EXTRAS {
            Engine::with_role(self.pid_file(n), false).reap_stale();
        }
    }
}

fn read_pid(path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Kills `pid` only if it is really a llama-server (PIDs get reused).
fn kill_if_engine(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    match sys.process(pid) {
        Some(p) if p.name().to_string_lossy().starts_with(SIDECAR) => p.kill(),
        _ => false,
    }
}

pub fn server_args(model: &std::path::Path, port: u16, api_key: &str, alias: &str, context: u32) -> Vec<String> {
    vec![
        "--model".into(),
        model.to_string_lossy().into_owned(),
        "--alias".into(),
        alias.into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--api-key".into(),
        api_key.into(),
        "--ctx-size".into(),
        context.to_string(),
        "--n-gpu-layers".into(),
        "999".into(),
        "--flash-attn".into(),
        "on".into(),
        "--cache-type-k".into(),
        "q8_0".into(),
        "--cache-type-v".into(),
        "q8_0".into(),
        "--parallel".into(),
        "1".into(),
        "--jinja".into(),
        "--reasoning-format".into(),
        "deepseek".into(),
        "--cache-reuse".into(),
        "256".into(),
        "--no-webui".into(),
    ]
}

/// Memory for the helper model's context and buffers, on top of its file.
const DRAFT_OVERHEAD: u64 = 400 * 1_000_000;

/// Speculative decoding: the helper guesses a few tokens ahead and the main
/// model checks them all in one pass, so answers don't change. A separate
/// model only proposes guesses it is at least `p_min` sure of (kept 70–90% in
/// tests); heads trained with the model are accurate for a few tokens.
/// `ngram` adds guessing from text already in the conversation.
pub fn draft_args(draft: Option<&Draft>, ngram: bool, n_max: Option<u32>, p_min: f32) -> Vec<String> {
    let mut types: Vec<&str> = draft.iter().map(|d| d.kind.spec_type()).collect();
    if ngram {
        types.push("ngram-mod");
    }
    if types.is_empty() {
        return vec![];
    }
    let mut a = vec!["--spec-type".to_string(), types.join(",")];
    if ngram {
        // llama.cpp's defaults (24-token match) suit long code files; chat
        // repeats shorter runs. A 4-token match made edits ~20% faster in tests.
        a.extend(["--spec-ngram-mod-n-match", "4", "--spec-ngram-mod-n-min", "2", "--spec-ngram-mod-n-max", "16"].map(String::from));
    }
    if let Some(d) = draft {
        let n = n_max.unwrap_or(d.kind.default_lookahead());
        a.extend(["--model-draft".into(), d.path.to_string_lossy().into_owned(), "--spec-draft-n-max".into(), n.to_string()]);
        if d.kind == HelperKind::Draft {
            a.extend(["--spec-draft-p-min".into(), format!("{p_min:.2}")]);
        }
        a.extend(["--n-gpu-layers-draft", "999", "--cache-type-k-draft", "q8_0", "--cache-type-v-draft", "q8_0"].map(String::from));
    }
    a
}

/// Adds tuned performance options to the base arguments.
pub fn apply_opts(args: &mut Vec<String>, opts: &LaunchOpts) {
    for i in 0..args.len().saturating_sub(1) {
        if opts.kv_f16 && (args[i] == "--cache-type-k" || args[i] == "--cache-type-v") {
            args[i + 1] = "f16".into();
        }
        if opts.flash_attn_off && opts.kv_f16 && args[i] == "--flash-attn" {
            args[i + 1] = "off".into();
        }
    }
    if let Some(n) = opts.gpu_layers {
        if let Some(i) = args.iter().position(|a| a == "--n-gpu-layers") {
            args[i + 1] = n.to_string();
        }
    }
    if opts.cpu_moe_layers > 0 {
        args.extend(["--n-cpu-moe".into(), opts.cpu_moe_layers.to_string()]);
    }
    if let Some(ub) = opts.ubatch {
        args.extend(["--ubatch-size".into(), ub.to_string(), "--batch-size".into(), ub.max(2048).to_string()]);
    }
    if opts.draft.is_some() || opts.ngram {
        let mut a = draft_args(opts.draft.as_ref(), opts.ngram, opts.draft_n_max, opts.draft_p_min.unwrap_or(0.75));
        // Without flash attention the helper can't use an 8-bit cache either
        // (llama-server exits with "failed to create MTP context").
        if opts.flash_attn_off && opts.kv_f16 {
            for i in 0..a.len().saturating_sub(1) {
                if a[i] == "--cache-type-k-draft" || a[i] == "--cache-type-v-draft" {
                    a[i + 1] = "f16".into();
                }
            }
        }
        args.extend(a);
    }
}

fn speculation_failed(log: &[String]) -> bool {
    log.iter().any(|l| l.contains("failed to initialize speculative") || l.contains("vocabs are not compatible"))
}

fn free_port() -> AppResult<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// Turns common llama.cpp failures into advice a person can act on.
pub fn diagnose(log: &[String]) -> String {
    let text = log.join("\n").to_lowercase();
    if text.contains("failed to allocate") || text.contains("out of memory") || text.contains("insufficient memory") {
        " Your Mac ran out of memory — quit other apps or choose a smaller model in Settings → Models.".into()
    } else if text.contains("failed to load model") || text.contains("invalid magic") || text.contains("gguf") && text.contains("error") {
        " The model file looks damaged — delete it in Settings → Models and download it again.".into()
    } else if text.contains("address already in use") {
        " A network port was busy — BYTE will pick another one if you restart the engine.".into()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_contain_core_flags() {
        let a = server_args(std::path::Path::new("/m/q.gguf"), 5555, "k", "qwen3-14b", 16384);
        let joined = a.join(" ");
        for needle in ["--model /m/q.gguf", "--port 5555", "--ctx-size 16384", "--jinja", "--reasoning-format deepseek", "--host 127.0.0.1", "--api-key k"] {
            assert!(joined.contains(needle), "missing {needle}: {joined}");
        }
    }

    #[test]
    fn draft_args_and_failure_detection() {
        let a = draft_args(Some(&Draft::model("/m/d.gguf")), false, None, 0.75).join(" ");
        assert!(a.contains("--spec-type draft-simple") && a.contains("--model-draft /m/d.gguf"), "{a}");
        assert!(a.contains("--spec-draft-n-max 16") && a.contains("--spec-draft-p-min 0.75"), "{a}");
        // A model's own head: its spec type, a short look-ahead, no confidence cut-off.
        let head = Draft { path: "/m/mtp.gguf".into(), kind: HelperKind::Mtp };
        let h = draft_args(Some(&head), true, None, 0.75).join(" ");
        assert!(h.contains("--spec-type draft-mtp,ngram-mod") && h.contains("--spec-draft-n-max 3"), "{h}");
        assert!(!h.contains("p-min"), "{h}");
        let e = draft_args(Some(&Draft { path: "/m/e.gguf".into(), kind: HelperKind::Eagle3 }), false, Some(5), 0.75).join(" ");
        assert!(e.contains("--spec-type draft-eagle3") && e.contains("--spec-draft-n-max 5"), "{e}");
        // Repeated-text guessing alone needs no helper file.
        let n = draft_args(None, true, None, 0.75).join(" ");
        assert!(n.starts_with("--spec-type ngram-mod --spec-ngram-mod-n-match 4") && !n.contains("--model-draft"), "{n}");
        assert!(draft_args(None, false, None, 0.75).is_empty());
        assert!(speculation_failed(&["E srv load_model: failed to initialize speculative decoding context".into()]));
        assert!(!speculation_failed(&["srv loaded".into()]));
    }

    #[test]
    fn tuned_options_change_the_arguments() {
        let mut a = server_args(std::path::Path::new("/m/q.gguf"), 1, "k", "m", 4096);
        apply_opts(&mut a, &LaunchOpts { draft: Some(Draft::model("/m/d.gguf")), kv_f16: true, ubatch: Some(1024), flash_attn_off: true, draft_n_max: Some(8), draft_p_min: Some(0.6), ..Default::default() });
        let j = a.join(" ");
        assert!(j.contains("--cache-type-k f16") && j.contains("--cache-type-v f16"), "{j}");
        assert!(j.contains("--flash-attn off") && j.contains("--spec-draft-n-max 8") && j.contains("--spec-draft-p-min 0.60"), "{j}");
        assert!(j.contains("--cache-type-k-draft f16") && j.contains("--cache-type-v-draft f16"), "{j}");
        // Flash attention stays on with an 8-bit cache (llama.cpp requires it).
        let mut c = server_args(std::path::Path::new("/m/q.gguf"), 1, "k", "m", 4096);
        apply_opts(&mut c, &LaunchOpts { flash_attn_off: true, ..Default::default() });
        assert!(c.join(" ").contains("--flash-attn on"));
        assert!(j.contains("--ubatch-size 1024") && j.contains("--batch-size 2048"), "{j}");
        assert!(j.contains("--model-draft /m/d.gguf"));
        // Planner offload: expert layers for the CPU, or fewer GPU layers.
        let mut m = server_args(std::path::Path::new("/m/q.gguf"), 1, "k", "m", 4096);
        apply_opts(&mut m, &LaunchOpts { cpu_moe_layers: 6, ..Default::default() });
        assert!(m.join(" ").contains("--n-cpu-moe 6") && m.join(" ").contains("--n-gpu-layers 999"));
        let mut st = server_args(std::path::Path::new("/m/q.gguf"), 1, "k", "m", 4096);
        apply_opts(&mut st, &LaunchOpts { gpu_layers: Some(30), ..Default::default() });
        assert!(st.join(" ").contains("--n-gpu-layers 30") && !st.join(" ").contains("n-cpu-moe"));
        let mut b = server_args(std::path::Path::new("/m/q.gguf"), 1, "k", "m", 4096);
        apply_opts(&mut b, &LaunchOpts::default());
        assert_eq!(b, server_args(std::path::Path::new("/m/q.gguf"), 1, "k", "m", 4096));
    }

    #[test]
    fn diagnose_memory_errors() {
        let log = vec!["ggml_metal: failed to allocate buffer".to_string()];
        assert!(diagnose(&log).contains("memory"));
        assert_eq!(diagnose(&["all good".to_string()]), "");
    }

    #[test]
    fn reaps_only_real_engine_processes() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("engine.pid");
        // A PID that isn't llama-server (this test process) must survive.
        std::fs::write(&pid_file, std::process::id().to_string()).unwrap();
        Engine::new(pid_file.clone()).reap_stale();
        assert!(!pid_file.exists());
        // Garbage and missing files are ignored.
        std::fs::write(&pid_file, "not a pid").unwrap();
        Engine::new(pid_file.clone()).reap_stale();
        Engine::new(pid_file).reap_stale();
    }

    #[tokio::test]
    async fn extras_have_a_limit_and_separate_pid_files() {
        let dir = tempfile::tempdir().unwrap();
        let x = Extras::new(dir.path().to_path_buf());
        for i in 0..MAX_EXTRAS {
            x.add(&format!("m{i}:Q4_K_M")).await.unwrap();
        }
        assert!(x.add("one-more:Q4_K_M").await.is_err());
        // Adding an already-loaded key returns it instead of failing.
        assert!(x.add("m0:Q4_K_M").await.is_ok());
        let pids: std::collections::HashSet<_> = x.all().await.iter().map(|e| e.pid_file.clone()).collect();
        assert_eq!(pids.len(), MAX_EXTRAS);
        assert!(x.all().await.iter().all(|e| !e.primary));
        // Nothing started yet, so nothing is reserved.
        assert_eq!(x.reserved().await, 0);
        assert!(x.get("m1:Q4_K_M").await.is_some());
        assert!(x.get("nope").await.is_none());
    }

    #[test]
    fn free_port_is_nonzero() {
        assert!(free_port().unwrap() > 0);
    }
}
