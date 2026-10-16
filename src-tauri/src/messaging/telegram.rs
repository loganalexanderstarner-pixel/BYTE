//! Telegram: the person's own bot from BotFather. BYTE polls `getUpdates` (one marker for the whole bot) and answers with
//! `sendMessage`. Only chats on the allow-list are used; everything else is skipped (and still moves the marker).

use std::collections::BTreeMap;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::{Channel, Fetched, Msg, Transport, TELEGRAM};
use crate::error::{AppError, AppResult};

pub const API: &str = "https://api.telegram.org";
/// The one marker key: Telegram numbers updates for the whole bot, not per chat.
pub const KEY: &str = "_";

pub struct Bot {
    pub http: reqwest::Client,
    pub base: String,
    pub token: String,
}

impl Bot {
    pub fn new(http: reqwest::Client, token: &str) -> Bot {
        Bot { http, base: API.into(), token: token.trim().to_string() }
    }

    async fn call(&self, method: &str, body: Value) -> AppResult<Value> {
        crate::offline::guard()?;
        let r = self.http.post(format!("{}/bot{}/{method}", self.base, self.token)).json(&body).send().await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if v["ok"].as_bool() == Some(true) {
            return Ok(v["result"].clone());
        }
        Err(AppError::msg(match (status.as_u16(), v["description"].as_str().unwrap_or("")) {
            (401, _) | (404, _) => "Telegram didn't accept that token. Copy it again from BotFather (it looks like 123456:ABC-DEF...).".to_string(),
            (409, _) => "This bot has a webhook set, so BYTE can't poll it. Remove the webhook (open https://api.telegram.org/bot<token>/deleteWebhook once) or use another bot.".to_string(),
            (429, _) => "Telegram says to slow down; BYTE will try again shortly.".to_string(),
            (_, d) if !d.is_empty() => format!("Telegram said: {d}"),
            (s, _) => format!("Telegram answered with an error ({s})."),
        }))
    }
}

pub fn chat_name(chat: &Value) -> String {
    if let Some(t) = chat["title"].as_str() {
        return t.to_string();
    }
    let first = chat["first_name"].as_str().unwrap_or("");
    let last = chat["last_name"].as_str().unwrap_or("");
    let name = format!("{first} {last}").trim().to_string();
    if !name.is_empty() {
        name
    } else {
        chat["username"].as_str().map(|u| format!("@{u}")).unwrap_or_else(|| "a chat".into())
    }
}

/// Text messages from `updates`, oldest first, plus the highest update id seen.
pub fn parse(updates: &Value) -> (Vec<Msg>, Option<i64>) {
    let mut msgs = vec![];
    let mut top: Option<i64> = None;
    for u in updates.as_array().into_iter().flatten() {
        let Some(id) = u["update_id"].as_i64() else { continue };
        top = Some(top.map_or(id, |t| t.max(id)));
        let m = &u["message"];
        let (Some(text), Some(chat)) = (m["text"].as_str(), m["chat"]["id"].as_i64()) else { continue };
        let from = &m["from"];
        let author = from["first_name"].as_str().or(from["username"].as_str()).unwrap_or("someone").to_string();
        msgs.push(Msg {
            id: id.to_string(),
            channel: chat.to_string(),
            author,
            text: text.to_string(),
            ts: m["date"].as_i64().unwrap_or(0),
            from_bot: from["is_bot"].as_bool().unwrap_or(false),
            private: m["chat"]["type"].as_str() == Some("private"),
            cursor_key: KEY.into(), meta: String::new(),
        });
    }
    msgs.sort_by_key(|m| m.id.parse::<i64>().unwrap_or(0));
    (msgs, top)
}

impl Transport for Bot {
    fn kind(&self) -> &'static str {
        TELEGRAM
    }

    fn check(&self) -> BoxFuture<'_, AppResult<String>> {
        Box::pin(async move {
            let me = self.call("getMe", json!({})).await?;
            Ok(format!("@{}", me["username"].as_str().unwrap_or("your bot")))
        })
    }

    /// The chats that have written to the bot lately (the person sends it a message first, then ticks the chat).
    fn channels(&self) -> BoxFuture<'_, AppResult<Vec<Channel>>> {
        Box::pin(async move {
            let updates = self.call("getUpdates", json!({ "limit": 100, "timeout": 0 })).await?;
            let mut seen: BTreeMap<String, Channel> = BTreeMap::new();
            for u in updates.as_array().into_iter().flatten() {
                let chat = &u["message"]["chat"];
                if let Some(id) = chat["id"].as_i64() {
                    let kind = chat["type"].as_str().unwrap_or("private");
                    seen.insert(id.to_string(), Channel { id: id.to_string(), name: chat_name(chat), group: if kind == "private" { "Private chats".into() } else { "Groups".into() } });
                }
            }
            Ok(seen.into_values().collect())
        })
    }

    fn cursor_keys(&self, _channels: &[String]) -> Vec<String> {
        vec![KEY.into()]
    }

    fn recent<'a>(&'a self, channels: &'a [String], since: &'a BTreeMap<String, String>, limit: usize) -> BoxFuture<'a, AppResult<Fetched>> {
        Box::pin(async move {
            let after: i64 = since.get(KEY).and_then(|s| s.parse().ok()).unwrap_or(0);
            let mut body = json!({ "limit": limit.clamp(1, 100), "timeout": 0, "allowed_updates": ["message"] });
            if after > 0 {
                body["offset"] = json!(after + 1);
            }
            let updates = self.call("getUpdates", body).await?;
            let (msgs, top) = parse(&updates);
            let mut f = Fetched::default();
            if let Some(t) = top {
                f.cursors.insert(KEY.into(), t.to_string());
            }
            f.msgs = msgs.into_iter().filter(|m| channels.contains(&m.channel)).collect();
            Ok(f)
        })
    }

    fn post<'a>(&'a self, channel: &'a str, text: &'a str) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            let chat: Value = channel.parse::<i64>().map(|n| json!(n)).unwrap_or_else(|_| json!(channel));
            self.call("sendMessage", json!({ "chat_id": chat, "text": text, "disable_web_page_preview": true })).await.map(|_| ())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn bot(s: &MockServer) -> Bot {
        Bot { http: reqwest::Client::new(), base: s.uri(), token: "123:abc".into() }
    }

    fn updates() -> Value {
        json!({ "ok": true, "result": [
            { "update_id": 11, "message": { "text": "hello", "date": 5, "chat": { "id": 42, "type": "private", "first_name": "Sam" }, "from": { "first_name": "Sam", "is_bot": false } } },
            { "update_id": 12, "message": { "text": "spam", "chat": { "id": 99, "type": "group", "title": "Stranger group" }, "from": { "first_name": "X", "is_bot": false } } },
            { "update_id": 13, "edited_message": {} }
        ] })
    }

    #[tokio::test]
    async fn check_names_the_bot() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/bot123:abc/getMe")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "result": { "username": "my_byte_bot" } }))).mount(&s).await;
        assert_eq!(bot(&s).check().await.unwrap(), "@my_byte_bot");
    }

    #[tokio::test]
    async fn chats_are_found_from_what_was_sent_to_the_bot() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/bot123:abc/getUpdates")).respond_with(ResponseTemplate::new(200).set_body_json(updates())).mount(&s).await;
        let c = bot(&s).channels().await.unwrap();
        assert_eq!(c.len(), 2);
        assert!(c.iter().any(|c| c.id == "42" && c.name == "Sam" && c.group == "Private chats"));
    }

    #[tokio::test]
    async fn only_allowed_chats_come_back_and_the_marker_passes_the_rest() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/bot123:abc/getUpdates")).and(body_partial_json(json!({ "offset": 11 }))).respond_with(ResponseTemplate::new(200).set_body_json(updates())).mount(&s).await;
        let since = BTreeMap::from([(KEY.to_string(), "10".to_string())]);
        let f = bot(&s).recent(&["42".to_string()], &since, 50).await.unwrap();
        assert_eq!(f.msgs.len(), 1);
        assert_eq!(f.msgs[0].text, "hello");
        assert!(f.msgs[0].private);
        assert_eq!(f.cursors[KEY], "13");
    }

    #[tokio::test]
    async fn sending_and_errors() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/bot123:abc/sendMessage")).and(body_partial_json(json!({ "chat_id": 42, "text": "hi" }))).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "result": {} }))).expect(1).mount(&s).await;
        bot(&s).post("42", "hi").await.unwrap();
        let s2 = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(409).set_body_json(json!({ "ok": false, "description": "Conflict" }))).mount(&s2).await;
        assert!(bot(&s2).recent(&[], &BTreeMap::new(), 5).await.unwrap_err().to_string().contains("webhook"));
    }
}
