//! The model that plans PC control: which models are good at it, which one runs (the chat model, or a chosen one in its own
//! on-demand engine like the photo helper), where it runs while a game is on (processor only, so the game keeps the card),
//! and the words it is given. Decisions are pure functions with tests; only `ControlBrain` starts a process.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::engine::{Endpoint, Engine, LaunchOpts};
use crate::error::{AppError, AppResult};
use crate::models::{self, Catalog};
use crate::system::Placement;

const IDLE_STOP: Duration = Duration::from_secs(3 * 60);
const CONTEXT: u32 = 8192;

/// Models picked by catalog id for steady JSON and tool calling (all of them have `tools` in the catalog), smallest first,
/// with a note of what each is for. Anything else the user has can still be chosen as the control model by key; this is the
/// list BYTE recommends.
/// The catalog id BYTE recommends for PC control.
pub const RECOMMENDED: &str = "qwen3.5-4b";

pub const CAPABLE: &[(&str, &str)] = &[
    ("qwen3.5-2b", "Fastest. Fine for opening things and simple steps."),
    ("qwen3.5-4b", "Recommended: quick and accurate for most voice control."),
    ("qwen3.5-9b", "More careful with long, multi-step plans."),
    ("gemma-4-12b", "Good at following a strict format."),
    ("qwen3-14b", "Solid all-rounder for tools."),
    ("gpt-oss-20b", "Strong tool use; needs more memory."),
    ("qwen3.6-35b-a3b", "Most capable here; large download, wants a big card or lots of memory."),
];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CapableModel {
    pub id: String,
    pub name: String,
    pub note: String,
    /// The installed version's key ("id:quant"), when downloaded.
    pub installed_key: Option<String>,
    /// What a download would fetch (the smallest ~4-bit version).
    pub download_key: Option<String>,
    pub download_bytes: u64,
    /// This is the control model in Settings.
    pub selected: bool,
}

/// The recommended list with what is downloaded. `selected` is the `controlModel` setting.
pub fn capable_models(catalog: &Catalog, models_dir: &Path, selected: &str) -> Vec<CapableModel> {
    CAPABLE
        .iter()
        .filter_map(|(id, note)| {
            let m = catalog.models.iter().find(|m| m.id == *id)?;
            let installed = m.variants.iter().filter(|v| models::is_installed(models_dir, v)).min_by_key(|v| v.size_bytes);
            let dl = m.variants.iter().filter(|v| v.bits >= 4.0).min_by_key(|v| v.size_bytes).or_else(|| m.variants.iter().min_by_key(|v| v.size_bytes));
            let installed_key = installed.map(|v| models::key(m, v));
            Some(CapableModel {
                id: m.id.clone(),
                name: m.name.clone(),
                note: note.to_string(),
                selected: !selected.is_empty() && (installed_key.as_deref() == Some(selected) || selected.split(':').next() == Some(m.id.as_str())),
                installed_key,
                download_key: dl.map(|v| models::key(m, v)),
                download_bytes: dl.map(|v| v.size_bytes).unwrap_or(0),
            })
        })
        .collect()
}

/// Which model does the controlling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The model already running for chat.
    Chat,
    /// Its own engine with this model key.
    Own(String),
}

/// `setting` is `controlModel`. Empty means chat. A model that was chosen but is not (or no longer) on disk falls back to
/// chat rather than failing, and the reason is returned for the log.
pub fn choose_source(setting: &str, catalog: &Catalog, models_dir: &Path) -> (Source, Option<String>) {
    let s = setting.trim();
    if s.is_empty() {
        return (Source::Chat, None);
    }
    match catalog.resolve(s) {
        Ok((m, v)) if m.role == models::Role::Chat && models::is_installed(models_dir, v) => (Source::Own(models::key(m, v)), None),
        Ok((m, _)) => (Source::Chat, Some(format!("{} is not downloaded, so the chat model is used", m.name))),
        Err(_) => (Source::Chat, Some("the chosen control model is not in the catalog, so the chat model is used".into())),
    }
}

/// Below this many billion parameters a model cannot plan PC control reliably (a 0.6B model wrote a 28-step plan for "change the font").
pub const MIN_PLANNER_B: f32 = 3.0;

/// The recommended control model, in words ("Qwen3.5 4B, about 2.5 GB"), from the catalog.
pub fn recommended_say(catalog: &Catalog) -> String {
    let id = RECOMMENDED;
    match catalog.models.iter().find(|m| m.id == id) {
        Some(m) => {
            let size = m.variants.iter().filter(|v| v.bits >= 4.0).map(|v| v.size_bytes).min().or_else(|| m.variants.iter().map(|v| v.size_bytes).min());
            match size {
                Some(s) => format!("{} (a download of about {:.1} GB)", m.name, s as f64 / 1e9),
                None => m.name.clone(),
            }
        }
        None => "Qwen3.5 4B".to_string(),
    }
}

/// What to tell the person when the model that plans is too small: which model it is, and how to get a better one. `None` when it is big
/// enough (or its size is unknown).
pub fn too_small_say(planner_name: &str, params_b: Option<f32>, catalog: &Catalog) -> Option<String> {
    let b = params_b?;
    (b < MIN_PLANNER_B).then(|| {
        format!(
            "The model that plans PC control is {planner_name} ({b:.1}B), which is too small to plan reliably. Recommended: {}. Download it in Settings → Models, then choose it under Settings → Control and screen → Model that does the controlling.",
            recommended_say(catalog)
        )
    })
}

/// Where the control model runs: with `game_ram` on and a game running, on the processor and system memory only; else normal.
pub fn placement_for(game_ram: bool, game_running: bool) -> Placement {
    if game_ram && game_running {
        Placement::CpuOnly
    } else {
        Placement::Auto
    }
}

/// Whether the engine already running (started with `running_cpu_only`) must restart for the placement wanted now. It
/// restarts when a game started while it holds the card; it does not restart just because the game ended (it is cheap
/// to leave on the processor until it goes idle).
pub fn must_restart(running_cpu_only: bool, want: Placement) -> bool {
    !running_cpu_only && want == Placement::CpuOnly
}

// ------------------------------------------------------------------ the words

pub const SYSTEM: &str = "You operate a Windows PC for the user by planning steps. Reply with ONE JSON object and nothing else: \
{\"steps\":[...]}. At most 12 steps. Each step is one of:\n\
{\"action\":\"open\",\"target\":\"notepad\"}  (a program name, an https: address, or an ms-settings: page)\n\
{\"action\":\"focus\",\"title\":\"part of a window title\"}\n\
{\"action\":\"click\",\"id\":N}  (N is a number from the list of items below; never guess a number)\n\
{\"action\":\"type\",\"text\":\"...\"}  (types into what has the keyboard focus)\n\
{\"action\":\"key\",\"combo\":\"ctrl+s\"}\n\
{\"action\":\"wait\",\"for\":\"window\" or \"element\",\"text\":\"...\",\"timeout_ms\":5000}\n\
{\"action\":\"scroll\",\"direction\":\"up\" or \"down\",\"amount\":3}\n\
{\"action\":\"close\",\"title\":\"part of a window title\"}\n\
{\"action\":\"power\",\"kind\":\"shutdown\" or \"restart\" or \"sign_out\"}\n\
Any step may add \"expect\":{\"window\":\"text in the window title afterwards\",\"element\":\"name of an item that should appear\"}. \
Use it after anything that opens a window or dialog. Use numbers only from the current list; after a step opens or changes a window \
the numbers change, so plan only up to that step, add expect, and you will be shown the new list. \
Prefer keyboard shortcuts and open/focus over clicking when they are reliable. Do not use any other action names or fields.";

/// The user message for a first plan.
pub fn plan_prompt(goal: &str, screen: &str) -> String {
    format!("Goal: {}\n\nWhat is in front now:\n{}\nPlan the steps.", goal.trim(), if screen.trim().is_empty() { "(nothing readable)\n" } else { screen })
}

/// The user message after a plan failed.
pub fn replan_prompt(req: &super::executor::ReplanRequest) -> String {
    format!(
        "Goal: {}\nAlready done: {}\nThis step failed: {} ({}).\n\nWhat is in front now:\n{}\nGive the remaining steps as a new plan.",
        req.goal.trim(),
        if req.done.is_empty() { "nothing".to_string() } else { req.done.join("; ") },
        req.failed,
        req.reason,
        if req.screen.trim().is_empty() { "(nothing readable)\n" } else { &req.screen },
    )
}

/// The user message when the model's reply was not a valid plan.
pub fn fix_prompt(reply_error: &str) -> String {
    format!("That plan was refused: {reply_error}. Reply again with only the corrected JSON object.")
}

pub fn request_body(messages: &[Value]) -> Value {
    json!({
        "messages": messages,
        "temperature": 0.0,
        "max_tokens": 700,
        "stream": false,
        "response_format": { "type": "json_object" },
        "chat_template_kwargs": { "enable_thinking": false },
    })
}

/// Text of the first choice, or an error.
pub fn reply_text(v: &Value) -> Option<String> {
    v["choices"][0]["message"]["content"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ------------------------------------------------------------------ the engine

#[derive(Clone)]
pub struct ControlBrain {
    engine: Engine,
    last_used: Arc<std::sync::Mutex<Instant>>,
    starting: Arc<Mutex<()>>,
    /// The key and placement the running engine was started with.
    running: Arc<std::sync::Mutex<Option<(String, bool)>>>,
    http: reqwest::Client,
}

impl ControlBrain {
    pub fn new(pid_file: std::path::PathBuf) -> Self {
        ControlBrain {
            engine: Engine::helper(pid_file),
            last_used: Arc::new(std::sync::Mutex::new(Instant::now())),
            starting: Arc::new(Mutex::new(())),
            running: Arc::new(std::sync::Mutex::new(None)),
            http: crate::chat::local_client(),
        }
    }

    pub fn reap_stale(&self) {
        self.engine.reap_stale();
    }

    pub fn kill_now(&self) {
        self.engine.kill_now();
    }

    pub async fn is_running(&self) -> bool {
        self.engine.endpoint().await.is_some()
    }

    fn touch(&self) {
        if let Ok(mut t) = self.last_used.lock() {
            *t = Instant::now();
        }
    }

    /// One reply from the model with its own engine on `key`.
    pub async fn complete(&self, app: &AppHandle, models_dir: &Path, catalog: &Catalog, key: &str, cpu_only: bool, messages: &[Value]) -> AppResult<String> {
        let ep = self.ensure_running(app, models_dir, catalog, key, cpu_only).await?;
        self.touch();
        let r = self.complete_at(&ep, messages).await;
        self.touch();
        r
    }

    /// One reply from any running engine (the chat engine's endpoint, or this one's).
    pub async fn complete_at(&self, ep: &Endpoint, messages: &[Value]) -> AppResult<String> {
        let r = self.http.post(format!("{}/v1/chat/completions", ep.base_url)).bearer_auth(&ep.api_key).timeout(Duration::from_secs(120)).json(&request_body(messages)).send().await?;
        if !r.status().is_success() {
            return Err(AppError::msg(format!("the control model failed ({})", r.status())));
        }
        let v: Value = r.json().await?;
        reply_text(&v).ok_or_else(|| AppError::msg("the control model had nothing to say"))
    }

    async fn ensure_running(&self, app: &AppHandle, models_dir: &Path, catalog: &Catalog, key: &str, cpu_only: bool) -> AppResult<Endpoint> {
        let _guard = self.starting.lock().await;
        let current = self.running.lock().ok().and_then(|r| r.clone());
        if let (Some(ep), Some((k, was_cpu))) = (self.engine.endpoint().await, current) {
            if k == key && !must_restart(was_cpu, if cpu_only { Placement::CpuOnly } else { Placement::Auto }) {
                return Ok(ep);
            }
        }
        let opts = LaunchOpts { cpu_only, ..Default::default() };
        self.engine.start(app, models_dir.to_path_buf(), catalog, key, Some(CONTEXT), 0, opts).await?;
        if let Ok(mut r) = self.running.lock() {
            *r = Some((key.to_string(), cpu_only));
        }
        self.touch();
        self.stop_when_idle();
        self.engine.endpoint().await.ok_or_else(|| AppError::msg("the control model stopped while starting"))
    }

    fn stop_when_idle(&self) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if this.engine.endpoint().await.is_none() {
                    return;
                }
                let idle = this.last_used.lock().map(|t| t.elapsed()).unwrap_or_default();
                if idle >= IDLE_STOP {
                    log::info!("control model idle for {} s; stopping it", idle.as_secs());
                    this.engine.stop().await;
                    if let Ok(mut r) = this.running.lock() {
                        *r = None;
                    }
                    return;
                }
            }
        });
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn a_tiny_planner_is_called_out_and_a_good_one_is_not() {
        let cat = Catalog::embedded();
        let say = too_small_say("Qwen3 0.6B", Some(0.6), &cat).unwrap();
        assert!(say.contains("too small") && say.contains("Settings → Models") && say.contains("Model that does the controlling"), "{say}");
        assert!(too_small_say("Qwen3.5 4B", Some(4.0), &cat).is_none());
        assert!(too_small_say("Mystery", None, &cat).is_none(), "an unknown size is not blamed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recommended_model_is_in_the_catalog_and_can_call_tools() {
        let c = Catalog::embedded();
        for (id, note) in CAPABLE {
            let m = c.models.iter().find(|m| m.id == *id).unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert!(m.tools, "{id} should support tools");
            assert_eq!(m.role, models::Role::Chat, "{id}");
            assert!(!note.is_empty());
        }
    }

    #[test]
    fn the_list_marks_nothing_downloaded_in_an_empty_folder_and_offers_a_download() {
        let c = Catalog::embedded();
        let l = capable_models(&c, Path::new("/nowhere"), "");
        assert_eq!(l.len(), CAPABLE.len());
        assert!(l.iter().all(|m| m.installed_key.is_none() && !m.selected));
        assert!(l.iter().all(|m| m.download_key.as_deref().is_some_and(|k| k.starts_with(&m.id)) && m.download_bytes > 0));
        let l = capable_models(&c, Path::new("/nowhere"), "qwen3.5-4b:Q4_K_M");
        assert_eq!(l.iter().filter(|m| m.selected).map(|m| m.id.as_str()).collect::<Vec<_>>(), ["qwen3.5-4b"]);
    }

    #[test]
    fn an_empty_setting_uses_the_chat_model_and_a_missing_one_falls_back_with_a_reason() {
        let c = Catalog::embedded();
        assert_eq!(choose_source("", &c, Path::new("/nowhere")), (Source::Chat, None));
        assert_eq!(choose_source("  ", &c, Path::new("/nowhere")).0, Source::Chat);
        let (s, why) = choose_source("qwen3.5-4b:Q4_K_M", &c, Path::new("/nowhere"));
        assert_eq!(s, Source::Chat);
        assert!(why.unwrap().contains("not downloaded"));
        let (s, why) = choose_source("no-such-model", &c, Path::new("/nowhere"));
        assert_eq!(s, Source::Chat);
        assert!(why.unwrap().contains("not in the catalog"));
    }

    #[test]
    fn a_game_moves_the_control_model_to_the_processor_only_when_the_setting_is_on() {
        assert_eq!(placement_for(true, true), Placement::CpuOnly);
        assert_eq!(placement_for(true, false), Placement::Auto);
        assert_eq!(placement_for(false, true), Placement::Auto);
        assert_eq!(placement_for(false, false), Placement::Auto);
    }

    #[test]
    fn a_running_engine_restarts_only_when_a_game_starts_while_it_holds_the_card() {
        assert!(must_restart(false, Placement::CpuOnly));
        assert!(!must_restart(true, Placement::CpuOnly));
        assert!(!must_restart(true, Placement::Auto), "left on the processor until it goes idle");
        assert!(!must_restart(false, Placement::Auto));
    }

    #[test]
    fn the_launch_option_asks_for_no_graphics_layers() {
        let mut args: Vec<String> = vec!["--n-gpu-layers".into(), "99".into()];
        crate::engine::apply_opts(&mut args, &LaunchOpts { gpu_layers: Some(0), cpu_only: true, ..Default::default() });
        assert_eq!(args, ["--n-gpu-layers", "0"]);
    }

    #[test]
    fn prompts_carry_the_goal_and_the_screen() {
        let p = plan_prompt("save the file", "Window \"A\" (notepad)\n1 button \"Save\"\n");
        assert!(p.contains("save the file") && p.contains("1 button \"Save\""));
        assert!(plan_prompt("x", "").contains("nothing readable"));
        let r = replan_prompt(&crate::control::executor::ReplanRequest { goal: "g".into(), done: vec!["Open notepad".into()], failed: "Click item 9".into(), reason: "gone".into(), screen: String::new() });
        assert!(r.contains("Open notepad") && r.contains("Click item 9") && r.contains("gone"));
        assert!(SYSTEM.contains("never guess a number"));
    }

    #[test]
    fn the_request_is_deterministic_json_without_thinking() {
        let b = request_body(&[json!({"role":"user","content":"x"})]);
        assert_eq!(b["temperature"], 0.0);
        assert_eq!(b["response_format"]["type"], "json_object");
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(reply_text(&json!({"choices":[{"message":{"content":" hi "}}]})).as_deref(), Some("hi"));
        assert_eq!(reply_text(&json!({"choices":[]})), None);
    }
}
