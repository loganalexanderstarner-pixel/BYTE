//! One polling pass and one post, with every rule applied in one place: the enable switch, the allow-list, the rate
//! limits, the "read up to here" markers, the activity log. The connectors themselves only talk to their service.

use std::path::PathBuf;
use std::sync::Mutex;

use super::activity::Log;
use super::config::{Config, ConnCfg};
use super::limits::{Limiter, HOUR, MINUTE};
use super::{Msg, Transport, DISCORD_WEBHOOK};
use crate::error::{AppError, AppResult};

/// The longest message BYTE posts (Discord's own limit is 2000).
pub const MAX_POST: usize = 1900;

pub struct Hub {
    pub dir: PathBuf,
    /// The profile's id: part of the credential-store key.
    pub profile: String,
    cfg: Mutex<Config>,
    limiter: Mutex<Limiter>,
    pub log: Log,
    /// Drafts and phone requests waiting for a yes.
    pub pending: super::reply::Queue,
    /// Messages kept in memory for the morning digest (only from connectors that opted in); never written to disk.
    pub digest_buf: Mutex<Vec<Msg>>,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Hub {
    pub fn open(dir: PathBuf, profile: &str) -> Hub {
        Hub { cfg: Mutex::new(Config::load(&dir)), limiter: Mutex::new(Limiter::default()), log: Log::new(&dir), pending: super::reply::Queue::open(&dir), digest_buf: Mutex::new(vec![]), profile: profile.to_string(), dir }
    }

    pub fn config(&self) -> Config {
        self.cfg.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Changes the settings and saves them.
    pub fn update(&self, f: impl FnOnce(&mut Config)) -> AppResult<Config> {
        let mut c = self.cfg.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut c);
        c.save(&self.dir)?;
        Ok(c.clone())
    }

    pub fn take(&self, key: &str, limit: u32, window_ms: i64, now: i64) -> bool {
        self.limiter.lock().unwrap_or_else(|p| p.into_inner()).take(key, limit, window_ms, now)
    }

    /// Does this connector's rate limit still have room (without using any)?
    pub fn room(&self, key: &str, limit: u32, window_ms: i64, now: i64) -> bool {
        limit == 0 || self.limiter.lock().unwrap_or_else(|p| p.into_inner()).used(key, window_ms, now) < limit as usize
    }
}

/// Reads the new messages of the allowed channels. Messages from before the connector was first read are never handed out
/// (the first pass only sets the "read up to here" markers), and bots' own messages are dropped.
pub async fn poll(hub: &Hub, t: &dyn Transport, now: i64) -> AppResult<Vec<Msg>> {
    let kind = t.kind();
    let cfg = hub.config().get(kind);
    if !cfg.enabled || cfg.channels.is_empty() {
        return Ok(vec![]);
    }
    if !hub.take(&format!("read:{kind}"), cfg.reads_per_min, MINUTE, now) {
        return Ok(vec![]);
    }
    let ids: Vec<String> = cfg.channels.iter().map(|c| c.id.clone()).collect();
    let fetched = match t.recent(&ids, &cfg.cursors, 50).await {
        Ok(f) => f,
        Err(e) => {
            hub.log.note(now, kind, "error", "", 0, false, &e.to_string());
            return Err(e);
        }
    };
    let mut out = vec![];
    for m in fetched.msgs {
        if !cfg.allows(&m.channel) || cfg.cursors.get(&m.cursor_key).is_none() || m.from_bot {
            continue;
        }
        out.push(m);
    }
    let keys = t.cursor_keys(&ids);
    hub.update(|c| {
        let e = c.entry(kind);
        for k in keys {
            if !e.cursors.contains_key(&k) && !fetched.cursors.contains_key(&k) {
                e.cursors.insert(k, "0".into());
            }
        }
        for (k, v) in &fetched.cursors {
            e.cursors.insert(k.clone(), v.clone());
        }
    })?;
    if !out.is_empty() {
        let mut by: std::collections::BTreeMap<&str, u32> = Default::default();
        for m in &out {
            *by.entry(m.channel.as_str()).or_default() += 1;
        }
        for (ch, n) in by {
            hub.log.note(now, kind, "read", ch, n, true, "");
        }
    }
    Ok(out)
}

/// The recent messages of the allowed channels WITHOUT moving the "read up to here" markers (for the digest of connectors
/// that keep history on their side). Same switch, allow-list and rate limit as a normal read.
pub async fn peek(hub: &Hub, t: &dyn Transport, now: i64) -> AppResult<Vec<Msg>> {
    let kind = t.kind();
    let cfg = hub.config().get(kind);
    if !cfg.enabled || !cfg.in_digest || cfg.channels.is_empty() || !hub.take(&format!("read:{kind}"), cfg.reads_per_min, MINUTE, now) {
        return Ok(vec![]);
    }
    let ids: Vec<String> = cfg.channels.iter().map(|c| c.id.clone()).collect();
    let f = t.recent(&ids, &Default::default(), 30).await?;
    let out: Vec<Msg> = f.msgs.into_iter().filter(|m| cfg.allows(&m.channel) && !m.from_bot && (m.ts == 0 || now / 1000 - m.ts < 86_400)).collect();
    if !out.is_empty() {
        hub.log.note(now, kind, "read", "digest", out.len() as u32, true, "");
    }
    Ok(out)
}

/// Keeps a pass's messages for the digest (only for a connector that opted in; the newest 300).
pub fn remember_for_digest(hub: &Hub, kind: &str, msgs: &[Msg]) {
    if !hub.config().get(kind).in_digest || msgs.is_empty() {
        return;
    }
    let mut b = hub.digest_buf.lock().unwrap_or_else(|p| p.into_inner());
    b.extend(msgs.iter().cloned().map(|mut m| {
        m.meta = format!("{kind}|{}", m.meta);
        m
    }));
    let n = b.len();
    if n > 300 {
        b.drain(..n - 300);
    }
}

fn refuse(hub: &Hub, kind: &str, channel: &str, now: i64, why: &str) -> AppError {
    hub.log.note(now, kind, "refused", channel, 0, false, why);
    AppError::msg(why)
}

/// The checks every post and reply goes through. Returns the cleaned text.
fn prepare(hub: &Hub, kind: &str, cfg: &ConnCfg, channel: &str, text: &str, secret: &str, now: i64, action: &str) -> AppResult<String> {
    if !cfg.enabled {
        return Err(refuse(hub, kind, channel, now, "That connector is turned off."));
    }
    if kind != DISCORD_WEBHOOK && !cfg.allows(channel) {
        return Err(refuse(hub, kind, channel, now, "That channel isn't on the allow-list."));
    }
    let clean = super::guard::clean_outgoing(text, MAX_POST);
    if clean.is_empty() {
        return Err(AppError::msg("There is nothing to post."));
    }
    if !secret.is_empty() && clean.contains(secret) {
        return Err(refuse(hub, kind, channel, now, "That text contains the connector's token, so it was not posted."));
    }
    if !hub.take(&format!("post:{kind}"), cfg.posts_per_hour, HOUR, now) {
        return Err(refuse(hub, kind, channel, now, "Too many posts this hour (the limit is in the connector's settings)."));
    }
    let _ = action;
    Ok(clean)
}

/// Posts to an allowed channel.
pub async fn post(hub: &Hub, t: &dyn Transport, channel: &str, text: &str, secret: &str, now: i64) -> AppResult<()> {
    let kind = t.kind();
    let cfg = hub.config().get(kind);
    let clean = prepare(hub, kind, &cfg, channel, text, secret, now, "post")?;
    match t.post(channel, &clean).await {
        Ok(()) => {
            hub.log.note(now, kind, "post", channel, 1, true, "");
            Ok(())
        }
        Err(e) => {
            hub.log.note(now, kind, "error", channel, 0, false, &e.to_string());
            Err(e)
        }
    }
}

/// Answers a message (a post in its channel; email saves a draft). `auto` marks a reply sent without a yes.
pub async fn reply(hub: &Hub, t: &dyn Transport, to: &Msg, text: &str, secret: &str, auto: bool, now: i64) -> AppResult<String> {
    let kind = t.kind();
    let cfg = hub.config().get(kind);
    let clean = prepare(hub, kind, &cfg, &to.channel, text, secret, now, "reply")?;
    match t.reply(to, &clean).await {
        Ok(what) => {
            hub.log.note(now, kind, if auto { "auto-reply" } else { "post" }, &to.channel, 1, true, "");
            Ok(what)
        }
        Err(e) => {
            hub.log.note(now, kind, "error", &to.channel, 0, false, &e.to_string());
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::config::Allowed;
    use crate::messaging::fake::Fake;

    fn hub_with(kind: &str, enabled: bool, channels: &[&str]) -> (tempfile::TempDir, Hub) {
        let d = tempfile::tempdir().unwrap();
        let hub = Hub::open(d.path().to_path_buf(), "p");
        hub.update(|c| {
            let e = c.entry(kind);
            e.enabled = enabled;
            e.channels = channels.iter().map(|i| Allowed { id: i.to_string(), name: format!("#{i}") }).collect();
        })
        .unwrap();
        (d, hub)
    }

    #[tokio::test]
    async fn off_means_nothing_is_read_or_posted() {
        let (_d, hub) = hub_with("discord", false, &["c1"]);
        let f = Fake::new("discord");
        f.push("c1", 1, "a", "hi");
        assert!(poll(&hub, &f, 0).await.unwrap().is_empty());
        assert_eq!(*f.reads.lock().unwrap(), 0, "the service was never asked");
        assert!(post(&hub, &f, "c1", "x", "", 0).await.is_err());
        assert!(f.sent().is_empty());
    }

    #[tokio::test]
    async fn the_first_pass_only_sets_the_marker_then_new_messages_come_through() {
        let (_d, hub) = hub_with("discord", true, &["c1"]);
        let f = Fake::new("discord");
        f.push("c1", 1, "a", "old");
        assert!(poll(&hub, &f, 0).await.unwrap().is_empty(), "old messages are not handed out");
        f.push("c1", 2, "a", "new");
        let got = poll(&hub, &f, 1).await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "new");
        assert!(poll(&hub, &f, 2).await.unwrap().is_empty(), "read once");
        // The log has counts, not text.
        let log = hub.log.recent(10);
        assert!(log.iter().any(|e| e.action == "read" && e.count == 1));
        assert!(!serde_json::to_string(&log).unwrap().contains("new"));
    }

    #[tokio::test]
    async fn an_empty_channel_still_gets_its_marker() {
        let (_d, hub) = hub_with("discord", true, &["c1"]);
        let f = Fake::new("discord");
        assert!(poll(&hub, &f, 0).await.unwrap().is_empty());
        f.push("c1", 5, "a", "first ever");
        assert_eq!(poll(&hub, &f, 1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn only_allowed_channels_and_never_bots() {
        let (_d, hub) = hub_with("discord", true, &["c1"]);
        let f = Fake::new("discord");
        poll(&hub, &f, 0).await.unwrap();
        f.push("c2", 3, "a", "elsewhere");
        f.push("c1", 4, "bot", "beep");
        f.inbox.lock().unwrap().last_mut().unwrap().from_bot = true;
        assert!(poll(&hub, &f, 1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn reads_are_rate_limited_and_unlimited_is_a_choice() {
        let (_d, hub) = hub_with("discord", true, &["c1"]);
        hub.update(|c| c.entry("discord").reads_per_min = 2).unwrap();
        let f = Fake::new("discord");
        for i in 0..5 {
            poll(&hub, &f, i).await.unwrap();
        }
        assert_eq!(*f.reads.lock().unwrap(), 2);
        hub.update(|c| c.entry("discord").reads_per_min = 0).unwrap();
        for i in 0..5 {
            poll(&hub, &f, 100 + i).await.unwrap();
        }
        assert_eq!(*f.reads.lock().unwrap(), 7);
    }

    #[tokio::test]
    async fn posts_need_the_allow_list_a_limit_and_no_token() {
        let (_d, hub) = hub_with("discord", true, &["c1"]);
        hub.update(|c| c.entry("discord").posts_per_hour = 2).unwrap();
        let f = Fake::new("discord");
        assert!(post(&hub, &f, "other", "hi", "", 0).await.is_err(), "not on the allow-list");
        assert!(post(&hub, &f, "c1", "my token is abc123", "abc123", 0).await.is_err());
        post(&hub, &f, "c1", "one @everyone", "tok", 1).await.unwrap();
        post(&hub, &f, "c1", "two", "tok", 2).await.unwrap();
        assert!(post(&hub, &f, "c1", "three", "tok", 3).await.is_err(), "rate limit");
        assert_eq!(f.sent().len(), 2);
        assert!(!f.sent()[0].1.contains("@everyone"));
        assert!(hub.log.recent(20).iter().filter(|e| e.action == "refused").count() >= 3);
    }

    #[tokio::test]
    async fn peeking_leaves_the_markers_and_needs_the_digest_switch() {
        let (_d, hub) = hub_with("discord", true, &["c1"]);
        let f = Fake::new("discord");
        f.push("c1", 1, "a", "hello");
        assert!(peek(&hub, &f, 5_000).await.unwrap().is_empty(), "not opted in");
        hub.update(|c| c.entry("discord").in_digest = true).unwrap();
        assert_eq!(peek(&hub, &f, 5_000).await.unwrap().len(), 1);
        assert!(hub.config().get("discord").cursors.is_empty());
        remember_for_digest(&hub, "discord", &[Msg { id: "9".into(), channel: "c1".into(), author: "a".into(), text: "x".into(), ts: 0, from_bot: false, private: false, cursor_key: "c1".into(), meta: String::new() }]);
        assert_eq!(hub.digest_buf.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_webhook_has_one_implicit_channel_but_still_a_switch() {
        let (_d, hub) = hub_with("discord-webhook", false, &[]);
        let f = Fake::new("discord-webhook");
        assert!(post(&hub, &f, "webhook", "hi", "", 0).await.is_err());
        hub.update(|c| c.entry("discord-webhook").enabled = true).unwrap();
        post(&hub, &f, "webhook", "hi", "", 1).await.unwrap();
        assert_eq!(f.sent().len(), 1);
    }
}
