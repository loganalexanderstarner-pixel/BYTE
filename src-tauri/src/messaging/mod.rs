//! Messaging connectors: Discord, Telegram, Slack and email, each the person's OWN bot or app.
//!
//! - Nothing is shared and nothing runs on the cluster: each person pastes their own bot token, which goes to the
//!   system credential store (never a file, a setting or a log), keyed by profile and connector.
//! - Every connector is OFF until the person turns it on, and works only on the channels they ticked (the allow-list).
//! - Reading is polling over HTTPS with the person's token (no gateway, no background service); every request goes
//!   through the app's network layer, so the offline switch holds.
//! - Everything read from outside is untrusted: it is fenced like a web page (`guard::wrap`) and never obeyed.
//! - The activity log records what was read or posted (counts, channel ids, times), never message text.
//!
//! Layout: `config` (settings file), `limits` (rate limits), `activity` (the log), `guard` (untrusted text, outgoing
//! text), `service` (one polling pass, one post), `discord` / `telegram` / `slack` / `email` (the connectors),
//! `reply` ("reply for me" drafts), `remote` (ask the PC from the phone), `fake` (the test double), `commands`.

pub mod activity;
pub mod commands;
pub mod config;
pub mod discord;
pub mod email;
pub mod guard;
pub mod limits;
pub mod reply;
pub mod remote;
pub mod service;
pub mod slack;
pub mod telegram;

#[cfg(test)]
pub mod fake;

use std::collections::BTreeMap;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// The connectors, as the stable keys used in the settings file, the credential store and the log.
pub const DISCORD: &str = "discord";
pub const DISCORD_WEBHOOK: &str = "discord-webhook";
pub const TELEGRAM: &str = "telegram";
pub const SLACK: &str = "slack";
pub const EMAIL: &str = "email";
pub const KINDS: &[&str] = &[DISCORD, DISCORD_WEBHOOK, TELEGRAM, SLACK, EMAIL];

pub fn known(kind: &str) -> bool {
    KINDS.contains(&kind)
}

/// A place messages live: a Discord channel, a Telegram chat, a Slack channel, a mail folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    pub name: String,
    /// The server, workspace or account it belongs to (for the picker).
    #[serde(default)]
    pub group: String,
}

/// One message read from outside. Its text is untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Msg {
    /// The service's own id (a snowflake, an update id, a timestamp, a mail UID).
    pub id: String,
    pub channel: String,
    pub author: String,
    pub text: String,
    /// Seconds since 1970 (0 when the service didn't say).
    pub ts: i64,
    /// Written by a bot (including this one): never answered.
    pub from_bot: bool,
    /// A one-to-one chat with the person (Telegram: chat type "private").
    pub private: bool,
    /// Where this message moves the "read up to here" marker (`Fetched::cursors` keys).
    pub cursor_key: String,
    /// What a reply needs that isn't text (email: the Message-ID).
    #[serde(default)]
    pub meta: String,
}

/// What one read returned: the messages (oldest first) and the new "read up to here" markers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fetched {
    pub msgs: Vec<Msg>,
    pub cursors: BTreeMap<String, String>,
}

/// The two things every connector can do. The real ones talk HTTPS (or IMAP); `fake::Fake` is the test double.
pub trait Transport: Send + Sync {
    /// Which connector this is (one of `KINDS`).
    fn kind(&self) -> &'static str;
    /// Checks the token and says who the bot is ("MyBot#1234").
    fn check(&self) -> BoxFuture<'_, AppResult<String>>;
    /// The channels the bot can see (for the allow-list picker).
    fn channels(&self) -> BoxFuture<'_, AppResult<Vec<Channel>>>;
    /// New messages in `channels` since the markers in `since` (a missing marker: the newest messages, and the new marker).
    fn recent<'a>(&'a self, channels: &'a [String], since: &'a BTreeMap<String, String>, limit: usize) -> BoxFuture<'a, AppResult<Fetched>>;
    /// Posts text to a channel.
    fn post<'a>(&'a self, channel: &'a str, text: &'a str) -> BoxFuture<'a, AppResult<()>>;
    /// The keys of the "read up to here" markers this connector uses for `channels` (default: one per channel).
    fn cursor_keys(&self, channels: &[String]) -> Vec<String> {
        channels.to_vec()
    }
    /// Answers `to` with `text`. Most connectors post in the same channel; email saves a draft instead.
    fn reply<'a>(&'a self, to: &'a Msg, text: &'a str) -> BoxFuture<'a, AppResult<String>> {
        Box::pin(async move {
            self.post(&to.channel, text).await?;
            Ok("Sent.".to_string())
        })
    }
}
