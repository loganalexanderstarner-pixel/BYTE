//! The app's side of the connectors: building the real connector from the stored token, the background pass (poll, reply
//! drafts, phone requests), and the commands the settings screen calls. Every command is for grown-ups only (kids mode refuses).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use super::config::{Allowed, ConnCfg};
use super::remote::Executor;
use super::reply::{Drafter, Pending};
use super::service::{self, now_ms, Hub};
use super::{discord, email, guard, known, slack, telegram, Channel, Msg, Transport, DISCORD, DISCORD_WEBHOOK, EMAIL, KINDS, SLACK, TELEGRAM};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub const PENDING_EVENT: &str = "connect://pending";

/// Creates the connectors' state for this profile (called once at startup, after the app state exists).
pub fn init(app: &AppHandle) {
    let state = app.state::<AppState>();
    let profile = state.paths.data.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    app.manage(Arc::new(Hub::open(state.paths.data.clone(), &profile)));
}

pub fn hub(app: &AppHandle) -> Arc<Hub> {
    app.state::<Arc<Hub>>().inner().clone()
}

fn secret(app: &AppHandle, kind: &str) -> AppResult<Option<String>> {
    let h = hub(app);
    guard::token(app.state::<AppState>().secrets.as_ref(), &h.profile, kind)
}

/// The connector for `kind` with this token (the password for email, the address for a webhook).
pub fn make(http: &reqwest::Client, kind: &str, token: &str, cfg: &ConnCfg) -> AppResult<Box<dyn Transport>> {
    Ok(match kind {
        DISCORD => Box::new(discord::Bot::new(http.clone(), token)),
        DISCORD_WEBHOOK => {
            if !discord::valid_webhook(token) {
                return Err(AppError::msg("That doesn't look like a Discord webhook address. It starts with https://discord.com/api/webhooks/"));
            }
            Box::new(discord::Webhook { http: http.clone(), url: token.trim().to_string() })
        }
        TELEGRAM => Box::new(telegram::Bot::new(http.clone(), token)),
        SLACK => Box::new(slack::App::new(http.clone(), token)),
        EMAIL => Box::new(email::Mail::new(&cfg.host, &cfg.user, token, &cfg.drafts_folder)),
        _ => return Err(AppError::msg("Unknown connector.")),
    })
}

async fn transport(app: &AppHandle, kind: &str) -> AppResult<(Box<dyn Transport>, String)> {
    let token = secret(app, kind)?.ok_or_else(|| AppError::msg("Set this connector up first (paste its token)."))?;
    let cfg = hub(app).config().get(kind);
    let state = app.state::<AppState>();
    Ok((make(&state.net, kind, &token, &cfg)?, token))
}

// ------------------------------------------------------------ the real helpers

struct AppDrafter {
    app: AppHandle,
}

impl Drafter for AppDrafter {
    fn draft<'a>(&'a self, prompt: &'a str) -> BoxFuture<'a, AppResult<String>> {
        Box::pin(async move {
            let state = self.app.state::<AppState>();
            // No tools at all: the message is untrusted, so the draft can only be words.
            let a = crate::triggers::toolbox::with_groups(vec![], crate::scheduler::ask_unattended(&state, prompt)).await;
            match a.error {
                Some(e) => Err(AppError::msg(e)),
                None => Ok(a.content),
            }
        })
    }
}

struct AppExecutor {
    app: AppHandle,
}

impl Executor for AppExecutor {
    fn routines(&self) -> Vec<String> {
        let state = self.app.state::<AppState>();
        crate::automations::list(&state.db).unwrap_or_default().into_iter().filter(|a| a.enabled).map(|a| a.name).collect()
    }

    fn is_pc_action(&self, q: &str) -> bool {
        crate::agent::is_pc_action(q)
    }

    fn run_routine<'a>(&'a self, name: &'a str) -> BoxFuture<'a, AppResult<String>> {
        Box::pin(async move {
            let state = self.app.state::<AppState>();
            let a = crate::automations::list(&state.db)?.into_iter().find(|a| a.name == name).ok_or_else(|| AppError::msg("That routine is gone."))?;
            let run = crate::automations::start(&self.app, a, 0)?;
            for _ in 0..200 {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                if let Some(v) = crate::automations::run_view(&state.db, run)? {
                    if v.finished {
                        return Ok(if v.ok { "done".to_string() } else { v.steps.iter().find(|s| s.status == "failed").map(|s| format!("stopped: {}", s.detail)).unwrap_or_else(|| "stopped".into()) });
                    }
                }
            }
            Ok("still running on the PC".into())
        })
    }

    fn ask<'a>(&'a self, q: &'a str, control: bool) -> BoxFuture<'a, AppResult<String>> {
        Box::pin(async move {
            let state = self.app.state::<AppState>();
            let groups: Vec<String> = if control { vec!["web".into(), "browser".into(), "pc".into()] } else { vec!["web".into()] };
            let a = crate::triggers::toolbox::with_groups(groups, crate::scheduler::ask_unattended(&state, q)).await;
            match a.error {
                Some(e) => Err(AppError::msg(e)),
                None => Ok(a.content),
            }
        })
    }
}

// ------------------------------------------------------------ the background pass

fn interval_ms(kind: &str) -> i64 {
    match kind {
        TELEGRAM => 8_000,
        EMAIL => 120_000,
        _ => 30_000,
    }
}

fn last_polls() -> &'static Mutex<BTreeMap<String, i64>> {
    static L: OnceLock<Mutex<BTreeMap<String, i64>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

/// One pass over every enabled connector. Returns the new messages per connector (for jobs triggered by a message).
pub async fn tick(app: &AppHandle) -> Vec<(String, Vec<Msg>)> {
    let mut all = vec![];
    if crate::kids::is_on() || crate::offline::is_offline() {
        return all;
    }
    let h = hub(app);
    let now = now_ms();
    for kind in KINDS.iter().filter(|k| **k != DISCORD_WEBHOOK) {
        let cfg = h.config().get(kind);
        if !cfg.enabled || cfg.channels.is_empty() {
            continue;
        }
        {
            let mut l = last_polls().lock().unwrap_or_else(|p| p.into_inner());
            if l.get(*kind).is_some_and(|t| now - *t < interval_ms(kind)) {
                continue;
            }
            l.insert(kind.to_string(), now);
        }
        let Ok((t, token)) = transport(app, kind).await else { continue };
        let msgs = match service::poll(&h, t.as_ref(), now).await {
            Ok(m) => m,
            Err(e) => {
                log::info!("{kind}: {e}");
                continue;
            }
        };
        if msgs.is_empty() {
            continue;
        }
        service::remember_for_digest(&h, kind, &msgs);
        let app2 = app.clone();
        let msgs2 = msgs.clone();
        let kind2 = kind.to_string();
        tauri::async_runtime::spawn(async move {
            let h = hub(&app2);
            let Ok((t, token)) = transport(&app2, &kind2).await else { return };
            let before = h.pending.list().len();
            if kind2 == TELEGRAM {
                super::remote::handle(&h, t.as_ref(), &AppExecutor { app: app2.clone() }, &token, &msgs2, now_ms()).await;
            }
            super::reply::handle(&h, t.as_ref(), &AppDrafter { app: app2.clone() }, &token, &msgs2, now_ms()).await;
            if h.pending.list().len() > before {
                crate::scheduler::notify(&app2, "BYTE has something waiting for your yes", "Open Settings, Control and screen, Connectors to see it.");
                let _ = app2.emit(PENDING_EVENT, ());
            }
        });
        let _ = token;
        all.push((kind.to_string(), msgs));
    }
    all
}

/// Recent messages for the morning digest, grouped "Discord #general": what the pass remembered, or (for services that keep
/// history) a look at the channels now.
pub async fn digest_messages(app: &AppHandle) -> Vec<(String, Vec<Msg>)> {
    let h = hub(app);
    let now = now_ms();
    let mut out: Vec<(String, Vec<Msg>)> = vec![];
    if crate::kids::is_on() || crate::offline::is_offline() {
        return out;
    }
    let cutoff = now / 1000 - 86_400;
    let remembered: Vec<Msg> = h.digest_buf.lock().unwrap_or_else(|p| p.into_inner()).iter().filter(|m| m.ts == 0 || m.ts >= cutoff).cloned().collect();
    for kind in KINDS.iter().filter(|k| **k != DISCORD_WEBHOOK) {
        let cfg = h.config().get(kind);
        if !cfg.enabled || !cfg.in_digest {
            continue;
        }
        let mut msgs: Vec<Msg> = remembered.iter().filter(|m| m.meta.starts_with(&format!("{kind}|"))).cloned().collect();
        if msgs.is_empty() && *kind != TELEGRAM {
            if let Ok((t, _)) = transport(app, kind).await {
                msgs = service::peek(&h, t.as_ref(), now).await.unwrap_or_default();
            }
        }
        for m in msgs {
            let label = format!("{} {}", display(kind), cfg.label(&m.channel));
            match out.iter_mut().find(|(l, _)| *l == label) {
                Some((_, v)) => v.push(m),
                None => out.push((label, vec![m])),
            }
        }
    }
    out
}

pub fn display(kind: &str) -> &'static str {
    match kind {
        DISCORD => "Discord",
        DISCORD_WEBHOOK => "Discord webhook",
        TELEGRAM => "Telegram",
        SLACK => "Slack",
        EMAIL => "Email",
        _ => "Connector",
    }
}

/// Posts text (a job's result) to an allowed channel, through every rule.
pub async fn post_from_app(app: &AppHandle, kind: &str, channel: &str, text: &str) -> AppResult<()> {
    crate::kids::grownups_only()?;
    crate::offline::guard()?;
    let (t, token) = transport(app, kind).await?;
    let channel = if kind == DISCORD_WEBHOOK { "webhook" } else { channel };
    service::post(&hub(app), t.as_ref(), channel, text, &token, now_ms()).await
}

// ------------------------------------------------------------------ commands

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub kind: String,
    pub name: String,
    pub has_token: bool,
    pub enabled: bool,
    pub channels: Vec<Allowed>,
    pub reads_per_min: u32,
    pub posts_per_hour: u32,
    pub reply_for_me: bool,
    pub auto_reply_channel: Option<String>,
    pub auto_reply_per_hour: u32,
    pub phone_ask: bool,
    pub allow_control: bool,
    pub confirm_each: bool,
    pub in_digest: bool,
    pub host: String,
    pub user: String,
    pub folder: String,
    pub drafts_folder: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub rows: Vec<Row>,
    pub pending: Vec<Pending>,
    pub jobs_paused: bool,
}

fn row(kind: &str, has_token: bool, c: &ConnCfg) -> Row {
    Row { kind: kind.into(), name: display(kind).into(), has_token, enabled: c.enabled, channels: c.channels.clone(), reads_per_min: c.reads_per_min, posts_per_hour: c.posts_per_hour, reply_for_me: c.reply_for_me, auto_reply_channel: c.auto_reply_channel.clone(), auto_reply_per_hour: c.auto_reply_per_hour, phone_ask: c.phone_ask, allow_control: c.allow_control, confirm_each: c.confirm_each, in_digest: c.in_digest, host: c.host.clone(), user: c.user.clone(), folder: c.folder.clone(), drafts_folder: c.drafts_folder.clone() }
}

fn status_now(app: &AppHandle) -> AppResult<Status> {
    let h = hub(app);
    let cfg = h.config();
    let rows = KINDS.iter().map(|k| row(k, secret(app, k).ok().flatten().is_some(), &cfg.get(k))).collect();
    Ok(Status { rows, pending: h.pending.list(), jobs_paused: cfg.jobs_paused })
}

#[tauri::command]
pub fn connect_status(app: AppHandle) -> AppResult<Status> {
    crate::kids::grownups_only()?;
    status_now(&app)
}

/// Checks a pasted token with the service and, only if it works, keeps it in the credential store. Returns who the bot is.
#[tauri::command]
pub async fn connect_token_set(app: AppHandle, kind: String, token: String) -> AppResult<String> {
    crate::kids::grownups_only()?;
    if !known(&kind) || kind == EMAIL {
        return Err(AppError::msg("Unknown connector."));
    }
    let state = app.state::<AppState>();
    let cfg = hub(&app).config().get(&kind);
    let t = make(&state.net, &kind, token.trim(), &cfg)?;
    let who = t.check().await?;
    guard::set_token(state.secrets.as_ref(), &hub(&app).profile, &kind, token.trim())?;
    Ok(who)
}

/// Email: the server, the account and an app password; checked by logging in, then kept.
#[tauri::command]
pub async fn connect_email_set(app: AppHandle, host: String, user: String, password: String, folder: String, drafts: String) -> AppResult<String> {
    crate::kids::grownups_only()?;
    let state = app.state::<AppState>();
    let mut cfg = ConnCfg { host: host.trim().into(), user: user.trim().into(), drafts_folder: drafts.trim().into(), ..Default::default() };
    if cfg.drafts_folder.is_empty() {
        cfg.drafts_folder = "Drafts".into();
    }
    let t = make(&state.net, EMAIL, password.trim(), &cfg)?;
    let who = t.check().await?;
    guard::set_token(state.secrets.as_ref(), &hub(&app).profile, EMAIL, password.trim())?;
    hub(&app).update(|c| {
        let e = c.entry(EMAIL);
        e.host = cfg.host.clone();
        e.user = cfg.user.clone();
        e.drafts_folder = cfg.drafts_folder.clone();
        e.folder = if folder.trim().is_empty() { "INBOX".into() } else { folder.trim().to_string() };
    })?;
    Ok(who)
}

/// The channels the bot can see (what the person ticks from).
#[tauri::command]
pub async fn connect_channels(app: AppHandle, kind: String) -> AppResult<Vec<Channel>> {
    crate::kids::grownups_only()?;
    crate::offline::guard()?;
    let (t, _) = transport(&app, &kind).await?;
    t.channels().await
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Patch {
    pub enabled: Option<bool>,
    pub channels: Option<Vec<Allowed>>,
    pub reads_per_min: Option<u32>,
    pub posts_per_hour: Option<u32>,
    pub reply_for_me: Option<bool>,
    pub auto_reply_channel: Option<Option<String>>,
    pub auto_reply_per_hour: Option<u32>,
    pub phone_ask: Option<bool>,
    pub allow_control: Option<bool>,
    pub confirm_each: Option<bool>,
    pub in_digest: Option<bool>,
}

/// Applies a change to one connector's settings. Switching one on needs its token first.
pub fn apply(c: &mut ConnCfg, p: Patch) {
    if let Some(v) = p.enabled {
        c.enabled = v;
    }
    if let Some(v) = p.channels {
        c.channels = v;
        // Auto-reply only ever names a channel that is still allowed.
        if c.auto_reply_channel.as_ref().is_some_and(|a| !c.allows(a)) {
            c.auto_reply_channel = None;
        }
        c.cursors.retain(|k, _| k == "_" || c.channels.iter().any(|a| &a.id == k) || k == &c.folder);
    }
    if let Some(v) = p.reads_per_min {
        c.reads_per_min = v;
    }
    if let Some(v) = p.posts_per_hour {
        c.posts_per_hour = v;
    }
    if let Some(v) = p.reply_for_me {
        c.reply_for_me = v;
        if !v {
            c.auto_reply_channel = None;
        }
    }
    if let Some(v) = p.auto_reply_channel {
        c.auto_reply_channel = v.filter(|a| c.allows(a) && c.reply_for_me);
    }
    if let Some(v) = p.auto_reply_per_hour {
        c.auto_reply_per_hour = v;
    }
    if let Some(v) = p.phone_ask {
        c.phone_ask = v;
        if !v {
            c.allow_control = false;
        }
    }
    if let Some(v) = p.allow_control {
        c.allow_control = v && c.phone_ask;
    }
    if let Some(v) = p.confirm_each {
        c.confirm_each = v;
    }
    if let Some(v) = p.in_digest {
        c.in_digest = v;
    }
}

#[tauri::command]
pub fn connect_set(app: AppHandle, kind: String, patch: Patch) -> AppResult<Status> {
    crate::kids::grownups_only()?;
    if !known(&kind) {
        return Err(AppError::msg("Unknown connector."));
    }
    if patch.enabled == Some(true) && secret(&app, &kind)?.is_none() {
        return Err(AppError::msg("Paste this connector's token first (steps 1 to 4 of its setup)."));
    }
    hub(&app).update(|c| apply(c.entry(&kind), patch))?;
    status_now(&app)
}

/// Removes the token and every setting of one connector.
#[tauri::command]
pub fn connect_forget(app: AppHandle, kind: String) -> AppResult<Status> {
    crate::kids::grownups_only()?;
    let h = hub(&app);
    guard::forget_token(app.state::<AppState>().secrets.as_ref(), &h.profile, &kind)?;
    h.update(|c| {
        c.connectors.remove(&kind);
    })?;
    status_now(&app)
}

#[tauri::command]
pub fn connect_log(app: AppHandle, limit: usize) -> AppResult<Vec<super::activity::Entry>> {
    crate::kids::grownups_only()?;
    Ok(hub(&app).log.recent(limit.clamp(1, 500)))
}

#[tauri::command]
pub fn connect_log_clear(app: AppHandle) -> AppResult<()> {
    crate::kids::grownups_only()?;
    hub(&app).log.clear();
    Ok(())
}

/// A test message to one allowed channel, so the person sees the connection works.
#[tauri::command]
pub async fn connect_post_test(app: AppHandle, kind: String, channel: String) -> AppResult<()> {
    post_from_app(&app, &kind, &channel, "Hello from BYTE. This is a test message.").await
}

#[tauri::command]
pub fn connect_pending(app: AppHandle) -> AppResult<Vec<Pending>> {
    crate::kids::grownups_only()?;
    Ok(hub(&app).pending.list())
}

/// Yes (maybe after editing): a drafted reply is sent; a phone request runs and its result goes back to the phone.
#[tauri::command]
pub async fn connect_pending_yes(app: AppHandle, id: String, text: Option<String>) -> AppResult<String> {
    crate::kids::grownups_only()?;
    let h = hub(&app);
    let p = h.pending.list().into_iter().find(|p| p.id == id).ok_or_else(|| AppError::msg("That is no longer waiting."))?;
    let (t, token) = transport(&app, &p.connector).await?;
    let out = if p.kind == "phone" { super::remote::approve(&h, t.as_ref(), &AppExecutor { app: app.clone() }, &token, &id, now_ms()).await } else { super::reply::approve(&h, t.as_ref(), &token, &id, text.as_deref(), now_ms()).await };
    let _ = app.emit(PENDING_EVENT, ());
    out
}

#[tauri::command]
pub fn connect_pending_no(app: AppHandle, id: String) -> AppResult<()> {
    crate::kids::grownups_only()?;
    super::reply::reject(&hub(&app), &id);
    let _ = app.emit(PENDING_EVENT, ());
    Ok(())
}

/// For an email draft: a `mailto:` link that opens the reply in the person's mail app.
#[tauri::command]
pub fn connect_mailto(app: AppHandle, id: String, text: Option<String>) -> AppResult<String> {
    crate::kids::grownups_only()?;
    let p = hub(&app).pending.list().into_iter().find(|p| p.id == id).ok_or_else(|| AppError::msg("That is no longer waiting."))?;
    let m = p.msg.ok_or_else(|| AppError::msg("That isn't an email."))?;
    Ok(email::mailto(&m, text.as_deref().unwrap_or(&p.draft)))
}

#[tauri::command]
pub fn jobs_pause(app: AppHandle, paused: bool) -> AppResult<Status> {
    crate::kids::grownups_only()?;
    hub(&app).update(|c| c.jobs_paused = paused)?;
    status_now(&app)
}

#[allow(dead_code)]
fn _keep(_: State<'_, AppState>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ConnCfg {
        ConnCfg { channels: vec![Allowed { id: "a".into(), name: "#a".into() }, Allowed { id: "b".into(), name: "#b".into() }], ..Default::default() }
    }

    #[test]
    fn control_needs_phone_ask_and_auto_reply_needs_reply_for_me_and_an_allowed_channel() {
        let mut c = cfg();
        apply(&mut c, Patch { allow_control: Some(true), ..Default::default() });
        assert!(!c.allow_control, "control can't be on without phone requests");
        apply(&mut c, Patch { phone_ask: Some(true), allow_control: Some(true), ..Default::default() });
        assert!(c.allow_control);
        apply(&mut c, Patch { phone_ask: Some(false), ..Default::default() });
        assert!(!c.allow_control, "turning phone requests off turns control off too");
        apply(&mut c, Patch { auto_reply_channel: Some(Some("a".into())), ..Default::default() });
        assert!(c.auto_reply_channel.is_none(), "reply for me is off");
        apply(&mut c, Patch { reply_for_me: Some(true), auto_reply_channel: Some(Some("a".into())), ..Default::default() });
        assert_eq!(c.auto_reply_channel.as_deref(), Some("a"));
        apply(&mut c, Patch { channels: Some(vec![Allowed { id: "b".into(), name: "#b".into() }]), ..Default::default() });
        assert!(c.auto_reply_channel.is_none(), "an unticked channel can't auto-reply");
        apply(&mut c, Patch { reply_for_me: Some(false), ..Default::default() });
        assert!(!c.reply_for_me);
    }

    #[test]
    fn making_a_connector_validates_what_it_was_given() {
        let http = reqwest::Client::new();
        assert!(make(&http, DISCORD_WEBHOOK, "https://example.com/x", &ConnCfg::default()).is_err());
        assert!(make(&http, DISCORD_WEBHOOK, "https://discord.com/api/webhooks/1/abc", &ConnCfg::default()).is_ok());
        assert_eq!(make(&http, TELEGRAM, "1:a", &ConnCfg::default()).unwrap().kind(), TELEGRAM);
        assert_eq!(make(&http, SLACK, "xoxb-1", &ConnCfg::default()).unwrap().kind(), SLACK);
        assert!(make(&http, "nope", "x", &ConnCfg::default()).is_err());
    }
}
