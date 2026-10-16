//! Test doubles: a connector that records what it was asked, and a secret store in memory.

use std::collections::BTreeMap;
use std::sync::Mutex;

use futures_util::future::BoxFuture;

use super::{Channel, Fetched, Msg, Transport};
use crate::cloud::keychain::SecretStore;
use crate::error::{AppError, AppResult};

#[derive(Default)]
pub struct MemStore(Mutex<BTreeMap<String, String>>);

impl SecretStore for MemStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> AppResult<()> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

/// Serves a fixed list of messages and records every post.
pub struct Fake {
    pub kind: &'static str,
    pub inbox: Mutex<Vec<Msg>>,
    pub sent: Mutex<Vec<(String, String)>>,
    pub fail_posts: bool,
    pub reads: Mutex<u32>,
}

impl Fake {
    pub fn new(kind: &'static str) -> Fake {
        Fake { kind, inbox: Mutex::new(vec![]), sent: Mutex::new(vec![]), fail_posts: false, reads: Mutex::new(0) }
    }
    pub fn push(&self, channel: &str, id: u64, author: &str, text: &str) {
        self.inbox.lock().unwrap().push(Msg { id: id.to_string(), channel: channel.into(), author: author.into(), text: text.into(), ts: id as i64, from_bot: false, private: true, cursor_key: channel.into(), meta: String::new() });
    }
    pub fn sent(&self) -> Vec<(String, String)> {
        self.sent.lock().unwrap().clone()
    }
}

impl Transport for Fake {
    fn kind(&self) -> &'static str {
        self.kind
    }
    fn check(&self) -> BoxFuture<'_, AppResult<String>> {
        Box::pin(async { Ok("FakeBot".to_string()) })
    }
    fn channels(&self) -> BoxFuture<'_, AppResult<Vec<Channel>>> {
        Box::pin(async { Ok(vec![Channel { id: "c1".into(), name: "general".into(), group: "Test".into() }]) })
    }
    fn recent<'a>(&'a self, channels: &'a [String], since: &'a BTreeMap<String, String>, _limit: usize) -> BoxFuture<'a, AppResult<Fetched>> {
        Box::pin(async move {
            *self.reads.lock().unwrap() += 1;
            let mut f = Fetched::default();
            for m in self.inbox.lock().unwrap().iter() {
                if !channels.contains(&m.channel) {
                    continue;
                }
                let after: u64 = since.get(&m.cursor_key).and_then(|s| s.parse().ok()).unwrap_or(0);
                let id: u64 = m.id.parse().unwrap_or(0);
                if since.get(&m.cursor_key).is_none() || id > after {
                    f.msgs.push(m.clone());
                    let e = f.cursors.entry(m.cursor_key.clone()).or_insert_with(|| "0".into());
                    if id > e.parse::<u64>().unwrap_or(0) {
                        *e = m.id.clone();
                    }
                }
            }
            Ok(f)
        })
    }
    fn post<'a>(&'a self, channel: &'a str, text: &'a str) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            if self.fail_posts {
                return Err(AppError::msg("the service said no"));
            }
            self.sent.lock().unwrap().push((channel.into(), text.into()));
            Ok(())
        })
    }
}
