//! The account's notes (`/api/memories`), so what BYTE knows about someone follows them to every device. With BYTE Cloud
//! connected the Memory list in Settings and the facts given to the on-device model come from here; without it, from this PC.

use std::time::Duration;

use serde_json::{json, Value};

use crate::db::Memory;

/// The notes in the cloud's shape (`id`, `fact`, `created_at` in seconds) as the app's own `Memory`.
pub fn parse(v: &Value) -> Vec<Memory> {
    let Some(list) = v.as_array() else { return Vec::new() };
    list.iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|i| i.as_i64().map(|n| n.to_string()).or_else(|| i.as_str().map(String::from)))?;
            let text = m.get("fact")?.as_str()?.trim().to_string();
            if text.is_empty() {
                return None;
            }
            let created = m.get("created_at").and_then(Value::as_f64).unwrap_or(0.0) as i64;
            Some(Memory { id, text, source: "cloud".into(), created_at: created })
        })
        .collect()
}

pub async fn list(client: &crate::cloud::CloudClient) -> Result<Vec<Memory>, String> {
    client.get("/api/memories").await.map(|v| parse(&v)).map_err(|e| crate::error::AppError::from(e).to_string())
}

/// For the prompt: the notes, or nothing if the cloud doesn't answer within a few seconds (the local ones still apply).
pub async fn facts_quickly(client: &crate::cloud::CloudClient) -> Vec<String> {
    match tokio::time::timeout(Duration::from_secs(3), list(client)).await {
        Ok(Ok(v)) => v.into_iter().map(|m| m.text).collect(),
        _ => Vec::new(),
    }
}

pub async fn add(client: &crate::cloud::CloudClient, text: &str) -> Result<Memory, String> {
    let v = client.post("/api/memories", &json!({ "fact": text })).await.map_err(|e| crate::error::AppError::from(e).to_string())?;
    parse(&Value::Array(vec![v])).into_iter().next().ok_or_else(|| "The cloud didn't save that note.".to_string())
}

/// The cloud has no edit call, so a changed note is added again and the old one removed: the same text, a new id.
pub async fn replace(client: &crate::cloud::CloudClient, id: &str, text: &str) -> Result<(), String> {
    add(client, text).await?;
    remove(client, id).await
}

pub async fn remove(client: &crate::cloud::CloudClient, id: &str) -> Result<(), String> {
    let path = format!("/api/memories/{}", id.chars().filter(char::is_ascii_digit).collect::<String>());
    client.delete(&path).await.map(|_| ()).map_err(|e| crate::error::AppError::from(e).to_string())
}

/// Facts for the prompt: the account's notes first, then this PC's that say something else.
pub fn merge(cloud: Vec<String>, local: Vec<String>) -> Vec<String> {
    let mut out = cloud;
    for l in local {
        if !out.iter().any(|c| c.eq_ignore_ascii_case(&l)) {
            out.push(l);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_cloud_notes() {
        let v = json!([{"id": 7, "fact": "Logan's favorite color is yellow.", "category": null, "created_at": 1760000000.5}, {"id": 8, "fact": "  ", "category": null, "created_at": 1.0}, {"fact": "no id"}]);
        let m = parse(&v);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "7");
        assert_eq!(m[0].source, "cloud");
        assert_eq!(m[0].created_at, 1_760_000_000);
        assert!(parse(&json!({"detail": "nope"})).is_empty());
    }

    #[test]
    fn merging_keeps_the_account_first_and_drops_repeats() {
        let out = merge(vec!["Likes tea.".into()], vec!["likes tea.".into(), "Has a cat.".into()]);
        assert_eq!(out, vec!["Likes tea.".to_string(), "Has a cat.".to_string()]);
    }
}
