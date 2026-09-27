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
use crate::models::{self, Catalog};
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
    Ready { model: String, context: u32 },
    Error { message: String },
}

/// What to run: model key, file to load, and context size.
#[derive(Debug, Clone)]
struct Launch {
    key: String,
    path: PathBuf,
    context: u32,
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
    ) -> AppResult<()> {
        let (model, variant) = catalog.resolve(key)?;
        if !models::is_installed(&models_dir, variant) {
            self.set_status(app, EngineStatus::NoModel).await;
            return Err(AppError::msg(format!("{} ({}) is not downloaded yet", model.name, variant.quant)));
        }
        let info = system::system_info(&models_dir).minus(reserved);
        let plan = models::plan(model, variant, &info, ctx_override.unwrap_or(DEFAULT_CONTEXT));
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
            inner.loaded = Some(LoadInfo { key: models::key(model, variant), context: plan.context, needed_bytes: plan.needed_bytes });
        }
        let launch = Launch { key: models::key(model, variant), path: models::entry_path(&models_dir, variant), context: plan.context };
        self.spawn(app.clone(), launch).await
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
        let args = server_args(&launch.path, port, &api_key, &launch.key, context);

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
                log::info!("engine ready: {} with {context}-token context", launch.key);
                self.set_status(&app, EngineStatus::Ready { model: launch.key.clone(), context }).await;
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
