//! Slack: the person's own Slack app with a bot token (`xoxb-...`). BYTE reads `conversations.history` of the channels
//! ticked (the app must be added to each channel) and posts with `chat.postMessage`.

use std::collections::BTreeMap;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::{Channel, Fetched, Msg, Transport, SLACK};
use crate::error::{AppError, AppResult};

pub const API: &str = "https://slack.com/api";

pub struct App {
    pub http: reqwest::Client,
    pub base: String,
    pub token: String,
}

impl App {
    pub fn new(http: reqwest::Client, token: &str) -> App {
        App { http, base: API.into(), token: token.trim().to_string() }
    }

    async fn call(&self, method: &str, params: &[(&str, String)], body: Option<Value>) -> AppResult<Value> {
        crate::offline::guard()?;
        let url = format!("{}/{method}", self.base);
        let req = match &body {
            Some(b) => self.http.post(&url).json(b),
            None => self.http.get(&url).query(params),
        };
        let r = req.bearer_auth(&self.token).send().await?;
        if r.status().as_u16() == 429 {
            return Err(AppError::msg("Slack says to slow down; BYTE will try again shortly."));
        }
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if v["ok"].as_bool() == Some(true) {
            return Ok(v);
        }
        Err(AppError::msg(match v["error"].as_str().unwrap_or("") {
            "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive" => "Slack didn't accept that token. Use the Bot User OAuth Token (it starts with xoxb-) from your app's OAuth & Permissions page.".to_string(),
            "missing_scope" => format!("The Slack app is missing a permission: {}. Add it under OAuth & Permissions, then reinstall the app.", v["needed"].as_str().unwrap_or("a scope")),
            "not_in_channel" | "channel_not_found" => "The Slack app isn't in that channel. In Slack, open the channel and add the app (Integrations > Add apps).".to_string(),
            "" => "Slack gave an unexpected answer.".to_string(),
            other => format!("Slack said: {other}"),
        }))
    }
}

pub fn messages(channel: &str, list: &Value) -> Vec<Msg> {
    let mut v: Vec<Msg> = list["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["type"].as_str() == Some("message") && m["text"].is_string())
        .filter_map(|m| {
            let ts = m["ts"].as_str()?.to_string();
            Some(Msg {
                id: ts.clone(),
                channel: channel.to_string(),
                author: m["user"].as_str().or(m["username"].as_str()).unwrap_or("someone").to_string(),
                text: m["text"].as_str().unwrap_or("").to_string(),
                ts: ts.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0),
                from_bot: m["bot_id"].is_string() || m["subtype"].as_str() == Some("bot_message"),
                private: false,
                cursor_key: channel.to_string(), meta: String::new(),
            })
        })
        .collect();
    v.sort_by(|a, b| a.id.parse::<f64>().unwrap_or(0.0).total_cmp(&b.id.parse::<f64>().unwrap_or(0.0)));
    v
}

impl Transport for App {
    fn kind(&self) -> &'static str {
        SLACK
    }

    fn check(&self) -> BoxFuture<'_, AppResult<String>> {
        Box::pin(async move {
            let v = self.call("auth.test", &[], Some(json!({}))).await?;
            Ok(format!("{} in {}", v["user"].as_str().unwrap_or("your app"), v["team"].as_str().unwrap_or("your workspace")))
        })
    }

    fn channels(&self) -> BoxFuture<'_, AppResult<Vec<Channel>>> {
        Box::pin(async move {
            let team = self.call("auth.test", &[], Some(json!({}))).await.ok().and_then(|v| v["team"].as_str().map(str::to_string)).unwrap_or_else(|| "Slack".into());
            let v = self.call("conversations.list", &[("types", "public_channel,private_channel".into()), ("exclude_archived", "true".into()), ("limit", "200".into())], None).await?;
            Ok(v["channels"].as_array().into_iter().flatten().filter(|c| c["is_member"].as_bool().unwrap_or(true)).filter_map(|c| Some(Channel { id: c["id"].as_str()?.into(), name: format!("#{}", c["name"].as_str()?), group: team.clone() })).collect())
        })
    }

    fn recent<'a>(&'a self, channels: &'a [String], since: &'a BTreeMap<String, String>, limit: usize) -> BoxFuture<'a, AppResult<Fetched>> {
        Box::pin(async move {
            let mut f = Fetched::default();
            for ch in channels {
                let mut params = vec![("channel", ch.clone()), ("limit", limit.clamp(1, 200).to_string())];
                if let Some(old) = since.get(ch).filter(|s| s.as_str() != "0") {
                    params.push(("oldest", old.clone()));
                }
                let v = self.call("conversations.history", &params, None).await?;
                let msgs: Vec<Msg> = messages(ch, &v).into_iter().filter(|m| since.get(ch).map_or(true, |s| s == "0" || m.id != *s)).collect();
                if let Some(last) = msgs.last() {
                    f.cursors.insert(ch.clone(), last.id.clone());
                }
                f.msgs.extend(msgs);
            }
            Ok(f)
        })
    }

    fn post<'a>(&'a self, channel: &'a str, text: &'a str) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move { self.call("chat.postMessage", &[], Some(json!({ "channel": channel, "text": text, "link_names": false, "parse": "none" }))).await.map(|_| ()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn app(s: &MockServer) -> App {
        App { http: reqwest::Client::new(), base: s.uri(), token: "xoxb-1".into() }
    }

    #[tokio::test]
    async fn check_and_channels() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/auth.test")).and(header("Authorization", "Bearer xoxb-1")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "user": "byte", "team": "Acme" }))).mount(&s).await;
        Mock::given(method("GET")).and(path("/conversations.list")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "channels": [{ "id": "C1", "name": "general", "is_member": true }, { "id": "C2", "name": "other", "is_member": false }] }))).mount(&s).await;
        assert_eq!(app(&s).check().await.unwrap(), "byte in Acme");
        let c = app(&s).channels().await.unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "#general");
    }

    #[tokio::test]
    async fn history_is_oldest_first_and_skips_bots_marker() {
        let s = MockServer::start().await;
        Mock::given(method("GET")).and(path("/conversations.history")).and(query_param("oldest", "1.5")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "messages": [
            { "type": "message", "text": "two", "ts": "3.0", "user": "U2", "bot_id": "B1" },
            { "type": "message", "text": "one", "ts": "2.0", "user": "U1" },
            { "type": "message", "subtype": "channel_join", "ts": "2.5" }
        ] }))).mount(&s).await;
        let since = BTreeMap::from([("C1".to_string(), "1.5".to_string())]);
        let f = app(&s).recent(&["C1".to_string()], &since, 50).await.unwrap();
        assert_eq!(f.msgs.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["one", "two"]);
        assert!(f.msgs[1].from_bot);
        assert_eq!(f.cursors["C1"], "3.0");
    }

    #[tokio::test]
    async fn posting_and_errors_in_plain_words() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/chat.postMessage")).and(body_partial_json(json!({ "channel": "C1", "text": "hi" }))).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true }))).expect(1).mount(&s).await;
        app(&s).post("C1", "hi").await.unwrap();
        let s2 = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": false, "error": "not_in_channel" }))).mount(&s2).await;
        assert!(app(&s2).recent(&["C1".into()], &BTreeMap::new(), 5).await.unwrap_err().to_string().contains("add the app"));
    }
}
