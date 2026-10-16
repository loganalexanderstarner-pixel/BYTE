//! Home Assistant (local): "turn on the kitchen light", "turn off the desk plug", "toggle the fan". BYTE talks to the person's own
//! Home Assistant over its REST interface on the local network; the access token lives in the system credential store, never in
//! settings or logs. Only lights, switches (plugs), fans and input switches can be changed here: never locks, alarms, garage doors
//! or anything else. The phrase is claimed only when it names something Home Assistant actually has, so "turn off notifications"
//! and the like carry on to the other routes.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::connectors::Secrets;
use crate::cloud::keychain::SecretStore;
use crate::error::{AppError, AppResult};

pub const ACCOUNT: &str = "homeassistant";
/// What can be changed.
pub const DOMAINS: &[&str] = &["light", "switch", "fan", "input_boolean"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    On,
    Off,
    Toggle,
}

impl Do {
    pub fn service(self) -> &'static str {
        match self {
            Do::On => "turn_on",
            Do::Off => "turn_off",
            Do::Toggle => "toggle",
        }
    }
    fn word(self) -> &'static str {
        match self {
            Do::On => "on",
            Do::Off => "off",
            Do::Toggle => "toggled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub what: Do,
    /// The words naming the thing, without articles ("kitchen", "desk plug").
    pub words: Vec<String>,
}

const FILLER: &[&str] = &["please", "the", "a", "my", "can", "you", "could", "would", "hey", "byte", "for", "me", "now", "to", "lights", "light", "lamp", "lamps", "plug", "plugs", "switch", "outlet", "socket", "fan"];

fn words_of(q: &str) -> Vec<String> {
    q.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// "turn on the kitchen light", "turn the desk plug off", "switch off the fan", "toggle the lamp". None for anything else.
pub fn parse(q: &str) -> Option<Command> {
    let w = words_of(q);
    if w.len() < 2 || w.len() > 14 {
        return None;
    }
    let has = |x: &str| w.iter().any(|t| t == x);
    let what = if has("toggle") {
        Do::Toggle
    } else if (has("turn") || has("switch")) && has("on") && !has("off") {
        Do::On
    } else if (has("turn") || has("switch")) && has("off") && !has("on") {
        Do::Off
    } else {
        return None;
    };
    // The verb must lead the sentence (after polite words): "my light turned on" is not a command.
    let first = w.iter().find(|t| !["please", "can", "you", "could", "would", "hey", "byte"].contains(&t.as_str()))?;
    if !["turn", "switch", "toggle"].contains(&first.as_str()) {
        return None;
    }
    let words: Vec<String> = w.into_iter().filter(|t| !["turn", "switch", "toggle", "on", "off"].contains(&t.as_str())).collect();
    Some(Command { what, words })
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Entity {
    pub entity_id: String,
    #[serde(default)]
    pub name: String,
}

impl Entity {
    pub fn domain(&self) -> &str {
        self.entity_id.split('.').next().unwrap_or("")
    }
    pub fn shown(&self) -> String {
        if self.name.trim().is_empty() { self.entity_id.clone() } else { self.name.trim().to_string() }
    }
    fn tokens(&self) -> BTreeSet<String> {
        let mut t: BTreeSet<String> = words_of(&self.name).into_iter().map(|w| singular(&w)).collect();
        t.extend(words_of(&self.entity_id.replace(['.', '_'], " ")).into_iter().map(|w| singular(&w)));
        t
    }
}

fn singular(w: &str) -> String {
    w.strip_suffix('s').filter(|s| s.len() > 2).unwrap_or(w).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    One(Entity),
    Several(Vec<Entity>),
    None,
}

/// Which entity the words name. Every specific word (not "the", "light", ...) must be in its name, and at least one must be given.
pub fn find(words: &[String], all: &[Entity]) -> Found {
    let specific: Vec<String> = words.iter().filter(|w| !FILLER.contains(&w.as_str())).map(|w| singular(w)).collect();
    if specific.is_empty() {
        return Found::None;
    }
    let mut hits: Vec<&Entity> = all.iter().filter(|e| DOMAINS.contains(&e.domain())).filter(|e| {
        let t = e.tokens();
        specific.iter().all(|w| t.contains(w))
    }).collect();
    // A kind word narrows it: "plug" means switches, "light" or "lamp" means lights, "fan" means fans.
    let kind = |names: &[&str]| words.iter().any(|w| names.contains(&w.as_str()));
    let narrowed: Vec<&Entity> = if kind(&["plug", "plugs", "outlet", "socket"]) {
        hits.iter().copied().filter(|e| e.domain() == "switch").collect()
    } else if kind(&["light", "lights", "lamp", "lamps"]) {
        hits.iter().copied().filter(|e| e.domain() == "light").collect()
    } else if kind(&["fan"]) {
        hits.iter().copied().filter(|e| e.domain() == "fan").collect()
    } else {
        vec![]
    };
    if !narrowed.is_empty() {
        hits = narrowed;
    }
    match hits.len() {
        0 => Found::None,
        1 => Found::One(hits[0].clone()),
        _ => Found::Several(hits.into_iter().take(6).cloned().collect()),
    }
}

/// A Home Assistant address: http on the local network, https anywhere. The token is only ever sent to this address.
pub fn base_url(raw: &str) -> Result<url::Url, String> {
    let u = url::Url::parse(raw.trim()).map_err(|_| "That isn't a web address (try http://homeassistant.local:8123).".to_string())?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() || !u.username().is_empty() || u.password().is_some() {
        return Err("Use an address like http://homeassistant.local:8123 (no user name or password in it).".into());
    }
    let host = u.host_str().unwrap_or("").to_lowercase();
    let local = host == "localhost"
        || host.ends_with(".local")
        || !host.contains('.')
        || host.trim_matches(['[', ']']).parse::<std::net::IpAddr>().is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local(),
            std::net::IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xfe00) == 0xfc00,
        });
    if u.scheme() == "http" && !local {
        return Err("An address outside your home network must start with https, so the token is not sent in the clear.".into());
    }
    Ok(u)
}

fn client() -> reqwest::Client {
    // No redirects: the token must never follow a redirect to another host.
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(8)).build().unwrap_or_default()
}

fn join(base: &url::Url, path: &str) -> String {
    format!("{}/{}", base.as_str().trim_end_matches('/'), path.trim_start_matches('/'))
}

/// Every light, plug, fan and switch Home Assistant has.
pub async fn entities(base: &url::Url, token: &str) -> AppResult<Vec<Entity>> {
    let r = client().get(join(base, "api/states")).bearer_auth(token).send().await.map_err(|e| AppError::msg(format!("Couldn't reach Home Assistant ({e}).")))?;
    if r.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AppError::msg("Home Assistant didn't accept the token. Make a new long-lived access token in your Home Assistant profile."));
    }
    if !r.status().is_success() {
        return Err(AppError::msg(format!("Home Assistant answered {}.", r.status())));
    }
    let v: serde_json::Value = r.json().await.map_err(|e| AppError::msg(format!("Home Assistant's answer wasn't readable ({e}).")))?;
    Ok(entities_from(&v))
}

pub fn entities_from(v: &serde_json::Value) -> Vec<Entity> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let id = s.get("entity_id")?.as_str()?.to_string();
                    let name = s.pointer("/attributes/friendly_name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                    DOMAINS.contains(&id.split('.').next().unwrap_or("")).then_some(Entity { entity_id: id, name })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub async fn call(base: &url::Url, token: &str, e: &Entity, what: Do) -> AppResult<()> {
    if !DOMAINS.contains(&e.domain()) {
        return Err(AppError::msg("BYTE only changes lights, plugs, fans and switches."));
    }
    let url = join(base, &format!("api/services/{}/{}", e.domain(), what.service()));
    let r = client().post(url).bearer_auth(token).json(&serde_json::json!({ "entity_id": e.entity_id })).send().await.map_err(|e| AppError::msg(format!("Couldn't reach Home Assistant ({e}).")))?;
    if r.status().is_success() { Ok(()) } else { Err(AppError::msg(format!("Home Assistant answered {}.", r.status()))) }
}

/// What BYTE says back.
pub fn reply(what: Do, found: &Found) -> String {
    match found {
        Found::One(e) => format!("Done: {} is {}.", e.shown(), what.word()),
        Found::Several(list) => format!("Which one did you mean? {}", list.iter().map(Entity::shown).collect::<Vec<_>>().join(", ")),
        Found::None => "Home Assistant has nothing by that name.".into(),
    }
}

/// The hook in `agent::specialist` (marked `track:helpers`).
pub async fn try_route(turn: &crate::agent::Turn<'_>, q: &str, _cancel: &tokio_util::sync::CancellationToken, send: crate::agent::Emit<'_>) -> AppResult<Option<(crate::tools::SourceBook, String, &'static str)>> {
    use crate::chat::ChatEvent;
    let Some(app) = turn.app else { return Ok(None) };
    let Some(cmd) = parse(crate::router::without_wake_phrase(q)) else { return Ok(None) };
    let state = tauri::Manager::state::<crate::state::AppState>(app);
    let (on, url) = {
        let s = state.settings.lock().await;
        (s.helper_ha && !s.kids_mode, s.helper_ha_url.clone())
    };
    if !on || state.lock.is_locked() || url.trim().is_empty() {
        return Ok(None);
    }
    let Ok(base) = base_url(&url) else { return Ok(None) };
    let Some(token) = Secrets.get(ACCOUNT).ok().flatten() else { return Ok(None) };
    let all = match entities(&base, &token).await {
        Ok(a) => a,
        Err(e) => return Ok(Some((Default::default(), e.to_string(), "house"))),
    };
    let found = find(&cmd.words, &all);
    if found == Found::None {
        return Ok(None);
    }
    if let Found::One(e) = &found {
        let id = format!("byte_ha_{}", uuid::Uuid::new_v4().simple());
        send(ChatEvent::ToolCall { id: id.clone(), name: "home_assistant".into(), args: serde_json::json!({ "what": format!("{} {}", cmd.what.service(), e.shown()), "app": "Home Assistant" }) })?;
        let result = call(&base, &token, e, cmd.what).await;
        send(ChatEvent::ToolResult { id, ok: result.is_ok(), summary: result.as_ref().map(|_| format!("{} {}", cmd.what.service(), e.shown())).unwrap_or_else(|e| e.to_string()) })?;
        if let Err(e) = result {
            return Ok(Some((Default::default(), e.to_string(), "house")));
        }
    }
    Ok(Some((Default::default(), reply(cmd.what, &found), "house")))
}

// ---------------------------------------------------------------- settings commands

#[tauri::command]
pub fn ha_token_saved() -> bool {
    Secrets.get(ACCOUNT).ok().flatten().is_some()
}

/// Saves the access token in the credential store.
#[tauri::command]
pub fn ha_set_token(token: String) -> AppResult<()> {
    let t = token.trim();
    if t.len() < 20 || t.contains(char::is_whitespace) {
        return Err(AppError::msg("That doesn't look like a Home Assistant long-lived access token."));
    }
    Secrets.set(ACCOUNT, t)
}

#[tauri::command]
pub fn ha_clear_token() -> AppResult<()> {
    Secrets.delete(ACCOUNT)
}

/// Checks the address and token, and says what BYTE can control.
#[tauri::command]
pub async fn ha_test(app: tauri::AppHandle) -> AppResult<String> {
    let url = crate::control::settings_now(&app).map(|s| s.helper_ha_url).unwrap_or_default();
    let base = base_url(&url).map_err(AppError::msg)?;
    let token = Secrets.get(ACCOUNT)?.ok_or_else(|| AppError::msg("Save the access token first."))?;
    let all = entities(&base, &token).await?;
    let count = |d: &str| all.iter().filter(|e| e.domain() == d).count();
    Ok(format!("Connected. BYTE can control {} light(s), {} plug/switch(es) and {} fan(s).", count("light"), count("switch") + count("input_boolean"), count("fan")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, name: &str) -> Entity {
        Entity { entity_id: id.into(), name: name.into() }
    }

    fn house() -> Vec<Entity> {
        vec![e("light.kitchen", "Kitchen Light"), e("light.living_room", "Living room lights"), e("switch.desk_plug", "Desk plug"), e("fan.bedroom", "Bedroom fan"), e("switch.kitchen_kettle", "Kitchen kettle"), e("lock.front", "Front door"), e("cover.garage", "Garage door")]
    }

    #[test]
    fn the_sentence_is_read_as_a_command_only_when_the_verb_leads() {
        assert_eq!(parse("Turn on the kitchen light").unwrap(), Command { what: Do::On, words: vec!["the".into(), "kitchen".into(), "light".into()] });
        assert_eq!(parse("turn the desk plug off please").unwrap().what, Do::Off);
        assert_eq!(parse("Hey BYTE, switch off the bedroom fan").unwrap().what, Do::Off);
        assert_eq!(parse("toggle the lamp").unwrap().what, Do::Toggle);
        assert!(parse("my light turned on by itself").is_none());
        assert!(parse("what does turn on mean").is_none());
        assert!(parse("turn it on and off").is_none());
        assert!(parse("hello").is_none());
    }

    #[test]
    fn a_name_finds_one_thing_and_a_vague_name_asks() {
        let all = house();
        let words = |s: &str| words_of(s);
        assert!(matches!(find(&words("the kitchen light"), &all), Found::One(x) if x.entity_id == "light.kitchen"));
        assert!(matches!(find(&words("desk plug"), &all), Found::One(x) if x.entity_id == "switch.desk_plug"));
        assert!(matches!(find(&words("living room lights"), &all), Found::One(x) if x.entity_id == "light.living_room"));
        assert!(matches!(find(&words("bedroom fan"), &all), Found::One(_)));
        // "kitchen" alone fits the light and the kettle: BYTE asks.
        assert!(matches!(find(&words("kitchen"), &all), Found::Several(v) if v.len() == 2));
        assert_eq!(find(&words("the lights"), &all), Found::None);
        assert_eq!(find(&words("notifications"), &all), Found::None);
    }

    #[test]
    fn locks_garage_doors_and_other_things_are_never_matched() {
        let all = house();
        assert_eq!(find(&words_of("front door"), &all), Found::None);
        assert_eq!(find(&words_of("garage door"), &all), Found::None);
        assert!(!DOMAINS.contains(&"lock") && !DOMAINS.contains(&"cover") && !DOMAINS.contains(&"alarm_control_panel"));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let base = url::Url::parse("http://192.168.1.5:8123").unwrap();
        let err = rt.block_on(call(&base, "t", &e("lock.front", "Front door"), Do::Off)).unwrap_err();
        assert!(err.to_string().contains("only changes lights"));
    }

    #[test]
    fn the_address_is_local_http_or_https_and_carries_no_login() {
        assert!(base_url("http://homeassistant.local:8123").is_ok());
        assert!(base_url("http://192.168.1.20:8123").is_ok());
        assert!(base_url("http://homeassistant:8123").is_ok());
        assert!(base_url("https://ha.example.com").is_ok());
        assert!(base_url("http://ha.example.com").unwrap_err().contains("https"));
        assert!(base_url("http://8.8.8.8:8123").is_err());
        assert!(base_url("ftp://192.168.1.2").is_err());
        assert!(base_url("http://user:pass@192.168.1.2").is_err());
        assert!(base_url("nonsense").is_err());
    }

    #[test]
    fn the_state_list_keeps_only_what_can_be_controlled() {
        let v = serde_json::json!([
            { "entity_id": "light.kitchen", "attributes": { "friendly_name": "Kitchen Light" } },
            { "entity_id": "lock.front", "attributes": { "friendly_name": "Front door" } },
            { "entity_id": "sensor.temp", "attributes": {} },
            { "entity_id": "switch.plug" }
        ]);
        let list = entities_from(&v);
        assert_eq!(list.iter().map(|e| e.entity_id.as_str()).collect::<Vec<_>>(), vec!["light.kitchen", "switch.plug"]);
        assert_eq!(list[1].shown(), "switch.plug");
        assert!(entities_from(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn replies_say_what_happened() {
        assert_eq!(reply(Do::On, &Found::One(e("light.k", "Kitchen Light"))), "Done: Kitchen Light is on.");
        assert!(reply(Do::Off, &Found::Several(vec![e("a.b", "One"), e("a.c", "Two")])).contains("One, Two"));
        assert!(reply(Do::On, &Found::None).contains("nothing by that name"));
    }

    #[test]
    fn a_token_must_look_like_one_and_services_are_named() {
        assert!(ha_set_token("short".into()).is_err());
        assert!(ha_set_token("has spaces in it which is not a token at all".into()).is_err());
        assert_eq!((Do::On.service(), Do::Off.service(), Do::Toggle.service()), ("turn_on", "turn_off", "toggle"));
    }
}
