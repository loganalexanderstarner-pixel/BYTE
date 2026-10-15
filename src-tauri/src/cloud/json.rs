//! Structured replies from the BYTE cloud, for BYTE's cards in Cloud mode when no
//! model is loaded on this Mac. The cloud's API is chat only, so each request is a
//! short conversation of its own ("BYTE card helper") that asks for JSON matching a
//! schema; BYTE reads the reply forgivingly and deletes the conversation afterwards.

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use super::{follow, posted_ids, CloudClient, CloudMode};
use crate::error::AppResult;

/// The title the helper conversations get (shown in the cloud's chat list if deleting fails).
pub const HELPER_TITLE: &str = "BYTE card helper";

#[derive(Clone)]
pub struct JsonHelper {
    client: CloudClient,
    mode: String,
}

impl std::fmt::Debug for JsonHelper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JsonHelper({})", self.mode)
    }
}

/// The cloud mode for card requests: Auto when offered (good answers without a long
/// research run), else Fast, else whatever the account has first.
pub fn pick_mode(modes: &[CloudMode]) -> Option<String> {
    ["auto", "fast"].iter().find_map(|w| modes.iter().find(|m| m.id == *w)).or(modes.first()).map(|m| m.id.clone())
}

/// What the cloud is asked: the task, then the exact reply format.
pub fn prompt(system: &str, user: &str, schema: &Value) -> String {
    format!(
        "{system}\n\n{user}\n\n---\nReply with only one JSON object that matches this JSON Schema (same keys, in this order), \
with no other text, no Markdown and no code fences:\n{}",
        serde_json::to_string(schema).unwrap_or_default()
    )
}

impl JsonHelper {
    pub fn new(client: CloudClient, mode: String) -> Self {
        JsonHelper { client, mode }
    }

    /// The cloud's reply to one structured request (the JSON text, as the model wrote it).
    pub async fn complete(&self, system: &str, user: &str, schema: &Value) -> AppResult<String> {
        let cid = self.client.create_conversation(HELPER_TITLE).await?;
        let reply = self.ask(&cid, &prompt(system, user, schema)).await;
        // Keep the cloud's chat list clean; older servers may not allow deleting.
        if let Err(e) = self.client.delete(&format!("/api/conversations/{cid}")).await {
            log::info!("couldn't delete the card helper conversation: {}", crate::error::AppError::from(e));
        }
        reply
    }

    /// The cloud's plain-text reply to a one-off request (the writing studio with no local model).
    pub async fn text(&self, system: &str, user: &str) -> AppResult<String> {
        let cid = self.client.create_conversation(HELPER_TITLE).await?;
        let reply = self.ask(&cid, &format!("{system}\n\n{user}")).await;
        if let Err(e) = self.client.delete(&format!("/api/conversations/{cid}")).await {
            log::info!("couldn't delete the helper conversation: {}", crate::error::AppError::from(e));
        }
        reply
    }

    async fn ask(&self, cid: &str, prompt: &str) -> AppResult<String> {
        let posted = self.client.post(&format!("/api/conversations/{cid}/messages"), &json!({ "content": prompt, "attachment_ids": [], "mode": self.mode })).await?;
        let (user_id, assistant_id) = posted_ids(&posted);
        // Nothing is shown while the cloud writes the JSON: the card appears when it's read.
        let quiet = Channel::new(|_| Ok(()));
        let end = follow(&self.client, cid, user_id, assistant_id, &CancellationToken::new(), &quiet).await?;
        Ok(end.text)
    }
}
