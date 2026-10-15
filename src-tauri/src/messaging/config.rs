//! The connectors' settings file (`connect.json` in the profile's data folder): per connector an enable switch (off), the
//! allow-list of channels, rate limits and the "reply for me" choices. Device-local, per profile, never part of the profile
//! sync. Tokens are NOT here: they live in the credential store (`guard::account`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

pub const FILE: &str = "connect.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Allowed {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConnCfg {
    /// Off until the person turns it on.
    pub enabled: bool,
    /// The only channels BYTE reads from or posts to.
    pub channels: Vec<Allowed>,
    /// Reads per minute (0 = Unlimited).
    pub reads_per_min: u32,
    /// Posts per hour (0 = Unlimited).
    pub posts_per_hour: u32,
    /// BYTE drafts replies to new messages and waits for a yes.
    pub reply_for_me: bool,
    /// The one named channel where BYTE may reply without a yes (None: never).
    pub auto_reply_channel: Option<String>,
    /// Auto-replies per hour in that channel (0 = Unlimited).
    pub auto_reply_per_hour: u32,
    /// Telegram: messages from an allowed private chat can start a saved routine or ask a plain question.
    pub phone_ask: bool,
    /// Telegram: "allow control from Telegram": the question may use PC control. Off.
    pub allow_control: bool,
    /// Telegram: every message waits for a yes on the PC first.
    pub confirm_each: bool,
    /// Include this connector's allowed channels in the morning digest.
    pub in_digest: bool,
    /// Email: the server ("imap.gmail.com"), the account, the folder to read and the folder for drafts.
    pub host: String,
    pub user: String,
    pub folder: String,
    pub drafts_folder: String,
    /// "Read up to here" markers per channel.
    pub cursors: BTreeMap<String, String>,
}

impl Default for ConnCfg {
    fn default() -> Self {
        ConnCfg {
            enabled: false,
            channels: vec![],
            reads_per_min: 12,
            posts_per_hour: 20,
            reply_for_me: false,
            auto_reply_channel: None,
            auto_reply_per_hour: 6,
            phone_ask: false,
            allow_control: false,
            confirm_each: true,
            in_digest: false,
            host: String::new(),
            user: String::new(),
            folder: "INBOX".into(),
            drafts_folder: "Drafts".into(),
            cursors: BTreeMap::new(),
        }
    }
}

impl ConnCfg {
    pub fn allows(&self, channel: &str) -> bool {
        self.channels.iter().any(|c| c.id == channel)
    }
    pub fn label(&self, channel: &str) -> String {
        self.channels.iter().find(|c| c.id == channel).map(|c| c.name.clone()).unwrap_or_else(|| channel.to_string())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub connectors: BTreeMap<String, ConnCfg>,
    /// The master switch: while on, no job runs (schedules and triggers wait).
    pub jobs_paused: bool,
}

impl Config {
    pub fn load(dir: &Path) -> Config {
        std::fs::read_to_string(dir.join(FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> AppResult<()> {
        let tmp = dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, dir.join(FILE))?;
        Ok(())
    }

    /// The connector's settings (the defaults, switched off, when it was never set up).
    pub fn get(&self, kind: &str) -> ConnCfg {
        self.connectors.get(kind).cloned().unwrap_or_default()
    }

    pub fn entry(&mut self, kind: &str) -> &mut ConnCfg {
        self.connectors.entry(kind.to_string()).or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_starts_off() {
        let c = Config::default();
        assert!(!c.get("discord").enabled);
        assert!(!c.get("telegram").allow_control);
        assert!(!c.get("telegram").phone_ask);
        assert!(!c.get("slack").reply_for_me);
        assert!(c.get("slack").auto_reply_channel.is_none());
        assert!(!c.jobs_paused);
    }

    #[test]
    fn saved_and_read_back_without_tokens() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Config::default();
        c.entry("discord").enabled = true;
        c.entry("discord").channels.push(Allowed { id: "1".into(), name: "general".into() });
        c.save(d.path()).unwrap();
        let back = Config::load(d.path());
        assert_eq!(back, c);
        assert!(back.get("discord").allows("1") && !back.get("discord").allows("2"));
        let raw = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(!raw.to_lowercase().contains("token"));
    }

    #[test]
    fn a_broken_file_means_defaults() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(FILE), "{nope").unwrap();
        assert_eq!(Config::load(d.path()), Config::default());
    }
}
