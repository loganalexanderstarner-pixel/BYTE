//! Discord, two ways, both the person's own:
//!
//! - `Bot`: their own bot application (Developer Portal). BYTE reads the channels they ticked by polling the REST API with
//!   the bot token (no gateway). The bot needs the "Message Content" intent switched on by the person, or Discord sends
//!   empty texts. It posts as the bot.
//! - `Webhook`: a webhook address pasted from a channel's settings. Post only; it can't read anything.
//!
//! A personal Discord account is never automated (Discord's rules forbid it and it risks the account): only bots and
//! webhooks, which Discord provides for this.

use std::collections::BTreeMap;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::{Channel, Fetched, Msg, Transport, DISCORD, DISCORD_WEBHOOK};
use crate::error::{AppError, AppResult};

pub const API: &str = "https://discord.com/api/v10";

pub struct Bot {
    pub http: reqwest::Client,
    pub base: String,
    pub token: String,
}

impl Bot {
    pub fn new(http: reqwest::Client, token: &str) -> Bot {
        Bot { http, base: API.into(), token: token.trim().to_string() }
    }

    async fn get(&self, path: &str) -> AppResult<Value> {
        crate::offline::guard()?;
        let r = self.http.get(format!("{}{path}", self.base)).header("Authorization", format!("Bot {}", self.token)).send().await?;
        read(r).await
    }
}

/// A Discord answer as JSON, or a plain-words error.
async fn read(r: reqwest::Response) -> AppResult<Value> {
    let status = r.status();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    Err(AppError::msg(match status.as_u16() {
        401 => "Discord didn't accept that token. Copy it again from the bot's page in the Developer Portal (Reset Token makes a new one).".to_string(),
        403 => "The bot isn't allowed to do that there. Check that it was invited to the server and has View Channel (and Read Message History / Send Messages) in that channel.".to_string(),
        404 => "Discord couldn't find that channel or server.".to_string(),
        429 => format!("Discord says to slow down; try again in {} seconds.", body["retry_after"].as_f64().unwrap_or(5.0).ceil() as u64),
        s => format!("Discord answered with an error ({s}){}.", body["message"].as_str().map(|m| format!(": {m}")).unwrap_or_default()),
    }))
}

/// Text channels (0) and announcement channels (5).
pub fn text_channels(guild: &str, list: &Value) -> Vec<Channel> {
    let mut v: Vec<(i64, Channel)> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| matches!(c["type"].as_i64(), Some(0) | Some(5)))
        .filter_map(|c| Some((c["position"].as_i64().unwrap_or(0), Channel { id: c["id"].as_str()?.to_string(), name: format!("#{}", c["name"].as_str()?), group: guild.to_string() })))
        .collect();
    v.sort_by_key(|(p, _)| *p);
    v.into_iter().map(|(_, c)| c).collect()
}

fn snowflake(id: &str) -> u64 {
    id.parse().unwrap_or(0)
}

/// Messages of one channel, oldest first.
pub fn messages(channel: &str, list: &Value) -> Vec<Msg> {
    let mut v: Vec<Msg> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let author = m["author"]["global_name"].as_str().or(m["author"]["username"].as_str()).unwrap_or("someone").to_string();
            Some(Msg {
                id: m["id"].as_str()?.to_string(),
                channel: channel.to_string(),
                author,
                text: m["content"].as_str().unwrap_or("").to_string(),
                ts: m["timestamp"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp()).unwrap_or(0),
                from_bot: m["author"]["bot"].as_bool().unwrap_or(false),
                private: false,
                cursor_key: channel.to_string(), meta: String::new(),
            })
        })
        .collect();
    v.sort_by_key(|m| snowflake(&m.id));
    v
}

impl Transport for Bot {
    fn kind(&self) -> &'static str {
        DISCORD
    }

    fn check(&self) -> BoxFuture<'_, AppResult<String>> {
        Box::pin(async move {
            let me = self.get("/users/@me").await?;
            if me["bot"].as_bool() != Some(true) {
                return Err(AppError::msg("That token isn't a bot's. BYTE only works with a bot you made in the Developer Portal, never with a personal account."));
            }
            Ok(me["username"].as_str().unwrap_or("your bot").to_string())
        })
    }

    fn channels(&self) -> BoxFuture<'_, AppResult<Vec<Channel>>> {
        Box::pin(async move {
            let guilds = self.get("/users/@me/guilds").await?;
            let mut out = vec![];
            for g in guilds.as_array().into_iter().flatten() {
                let (Some(id), name) = (g["id"].as_str(), g["name"].as_str().unwrap_or("a server")) else { continue };
                match self.get(&format!("/guilds/{id}/channels")).await {
                    Ok(list) => out.extend(text_channels(name, &list)),
                    Err(e) => log::warn!("discord: channels of {name}: {e}"),
                }
            }
            Ok(out)
        })
    }

    fn recent<'a>(&'a self, channels: &'a [String], since: &'a BTreeMap<String, String>, limit: usize) -> BoxFuture<'a, AppResult<Fetched>> {
        Box::pin(async move {
            let mut f = Fetched::default();
            for ch in channels {
                let after = since.get(ch).filter(|a| a.as_str() != "0").map(|a| format!("&after={a}")).unwrap_or_default();
                let list = self.get(&format!("/channels/{ch}/messages?limit={}{after}", limit.clamp(1, 100))).await?;
                let msgs = messages(ch, &list);
                if let Some(last) = msgs.last() {
                    f.cursors.insert(ch.clone(), last.id.clone());
                }
                f.msgs.extend(msgs);
            }
            Ok(f)
        })
    }

    fn post<'a>(&'a self, channel: &'a str, text: &'a str) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            crate::offline::guard()?;
            let r = self
                .http
                .post(format!("{}/channels/{channel}/messages", self.base))
                .header("Authorization", format!("Bot {}", self.token))
                .json(&json!({ "content": text, "allowed_mentions": { "parse": [] } }))
                .send()
                .await?;
            read(r).await.map(|_| ())
        })
    }
}

/// A webhook address is `https://discord.com/api/webhooks/<id>/<token>` (the token part is the secret).
pub fn valid_webhook(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url.trim()) else { return false };
    u.scheme() == "https" && matches!(u.host_str(), Some("discord.com") | Some("discordapp.com") | Some("canary.discord.com") | Some("ptb.discord.com")) && u.path().starts_with("/api/webhooks/") && u.path().split('/').count() >= 5
}

pub struct Webhook {
    pub http: reqwest::Client,
    pub url: String,
}

impl Transport for Webhook {
    fn kind(&self) -> &'static str {
        DISCORD_WEBHOOK
    }
    fn check(&self) -> BoxFuture<'_, AppResult<String>> {
        Box::pin(async move {
            crate::offline::guard()?;
            let r = self.http.get(self.url.trim()).send().await?;
            let v = read(r).await?;
            Ok(format!("webhook \"{}\"", v["name"].as_str().unwrap_or("a webhook")))
        })
    }
    fn channels(&self) -> BoxFuture<'_, AppResult<Vec<Channel>>> {
        Box::pin(async { Ok(vec![Channel { id: "webhook".into(), name: "the webhook's channel".into(), group: "Discord".into() }]) })
    }
    fn recent<'a>(&'a self, _c: &'a [String], _s: &'a BTreeMap<String, String>, _l: usize) -> BoxFuture<'a, AppResult<Fetched>> {
        Box::pin(async { Ok(Fetched::default()) })
    }
    fn post<'a>(&'a self, _channel: &'a str, text: &'a str) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            crate::offline::guard()?;
            let r = self.http.post(self.url.trim()).json(&json!({ "content": text, "allowed_mentions": { "parse": [] } })).send().await?;
            read(r).await.map(|_| ())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn bot(server: &MockServer) -> Bot {
        Bot { http: reqwest::Client::new(), base: server.uri(), token: "tok".into() }
    }

    #[tokio::test]
    async fn check_names_the_bot_and_refuses_a_personal_account() {
        let s = MockServer::start().await;
        Mock::given(method("GET")).and(path("/users/@me")).and(header("Authorization", "Bot tok")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "username": "Helper", "bot": true }))).mount(&s).await;
        assert_eq!(bot(&s).check().await.unwrap(), "Helper");
        let s2 = MockServer::start().await;
        Mock::given(method("GET")).and(path("/users/@me")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "username": "sam" }))).mount(&s2).await;
        assert!(bot(&s2).check().await.unwrap_err().to_string().contains("never with a personal account"));
    }

    #[tokio::test]
    async fn a_wrong_token_is_explained() {
        let s = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(401)).mount(&s).await;
        assert!(bot(&s).check().await.unwrap_err().to_string().contains("Reset Token"));
    }

    #[tokio::test]
    async fn channels_list_text_channels_of_each_server() {
        let s = MockServer::start().await;
        Mock::given(method("GET")).and(path("/users/@me/guilds")).respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "id": "g1", "name": "Friends" }]))).mount(&s).await;
        Mock::given(method("GET")).and(path("/guilds/g1/channels")).respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "2", "name": "voice", "type": 2 }, { "id": "3", "name": "memes", "type": 0, "position": 2 }, { "id": "4", "name": "general", "type": 0, "position": 1 }
        ]))).mount(&s).await;
        let c = bot(&s).channels().await.unwrap();
        assert_eq!(c.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["#general", "#memes"]);
        assert_eq!(c[0].group, "Friends");
    }

    #[tokio::test]
    async fn recent_reads_oldest_first_and_moves_the_marker() {
        let s = MockServer::start().await;
        Mock::given(method("GET")).and(path("/channels/9/messages")).and(query_param("after", "100")).respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "103", "content": "c", "author": { "username": "u", "bot": true } },
            { "id": "101", "content": "a", "author": { "username": "u", "global_name": "Una" }, "timestamp": "2026-10-15T10:00:00+00:00" }
        ]))).mount(&s).await;
        let since = BTreeMap::from([("9".to_string(), "100".to_string())]);
        let f = bot(&s).recent(&["9".to_string()], &since, 50).await.unwrap();
        assert_eq!(f.msgs.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["a", "c"]);
        assert_eq!(f.msgs[0].author, "Una");
        assert!(f.msgs[1].from_bot);
        assert_eq!(f.cursors["9"], "103");
    }

    #[tokio::test]
    async fn posting_sends_no_mentions() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/channels/9/messages")).and(body_partial_json(json!({ "content": "hi", "allowed_mentions": { "parse": [] } }))).respond_with(ResponseTemplate::new(200).set_body_json(json!({}))).expect(1).mount(&s).await;
        bot(&s).post("9", "hi").await.unwrap();
    }

    #[test]
    fn webhook_addresses_are_checked() {
        assert!(valid_webhook("https://discord.com/api/webhooks/123/abcDEF"));
        assert!(!valid_webhook("http://discord.com/api/webhooks/123/abc"));
        assert!(!valid_webhook("https://evil.example/api/webhooks/123/abc"));
        assert!(!valid_webhook("https://discord.com/api/webhooks/"));
        assert!(!valid_webhook("not a url"));
    }

    #[tokio::test]
    async fn the_webhook_posts_and_cannot_read() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/webhooks/1/x")).respond_with(ResponseTemplate::new(204)).expect(1).mount(&s).await;
        let w = Webhook { http: reqwest::Client::new(), url: format!("{}/api/webhooks/1/x", s.uri()) };
        w.post("webhook", "hello").await.unwrap();
        assert!(w.recent(&[], &BTreeMap::new(), 10).await.unwrap().msgs.is_empty());
    }
}
