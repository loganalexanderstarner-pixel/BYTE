//! Automatic chat titles, one-line summaries and tags, written by the local
//! model after the first answer. One short non-streaming request with thinking
//! off; the output is forced to JSON by llama-server's grammar support.


use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::engine::Endpoint;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The example in the instructions; a reply that copies it is rejected.
const EXAMPLE_TITLE: &str = "Fixing a slipping bike chain";
/// Small models sometimes copy these from the example; kept only if the chat mentions them.
const EXAMPLE_TAGS: [&str; 2] = ["cycling", "repair"];

const INSTRUCTIONS: &str = "You label conversations for a chat list. Reply with JSON only, in this shape: \
{\"title\": \"Fixing a slipping bike chain\", \"summary\": \"How to diagnose and fix a bike chain that slips when pedaling.\", \"tags\": [\"cycling\", \"repair\"]}. \
Describe the conversation below, not this example. \
title: 2 to 6 words, no quotes or trailing period. summary: one sentence, under 20 words, about what the user wanted. \
tags: 1 to 3 short lowercase topic words.";

/// The start of a chat as plain text (enough to label it), capped in length.
pub fn transcript(messages: &[Value], max_chars: usize) -> String {
    let mut out = String::new();
    for m in messages {
        let who = if m.get("role").and_then(Value::as_str) == Some("user") { "User" } else { "Assistant" };
        let text = m.get("content").and_then(Value::as_str).unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        let room = max_chars.saturating_sub(out.len());
        if room < 40 {
            break;
        }
        let clipped: String = text.chars().take(room.min(1200)).collect();
        out.push_str(&format!("{who}: {clipped}\n\n"));
    }
    out
}

/// Pulls the JSON object out of the model's reply and tidies it up.
pub fn parse(reply: &str) -> Option<ChatSummary> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let mut s: ChatSummary = serde_json::from_str(reply.get(start..=end)?).ok()?;
    s.title = short_title(s.title.trim().trim_matches(|c| c == '"' || c == '.'));
    s.summary = s.summary.trim().chars().take(200).collect();
    s.tags = s
        .tags
        .iter()
        .map(|t| t.trim().trim_start_matches('#').to_lowercase())
        .filter(|t| !t.is_empty() && t.len() <= 24)
        .take(3)
        .collect();
    (!s.title.is_empty() && !s.summary.is_empty() && !s.title.eq_ignore_ascii_case(EXAMPLE_TITLE)).then_some(s)
}

/// At most 8 words and 60 characters, not ending on a small word ("… taxes as a").
fn short_title(t: &str) -> String {
    let mut words: Vec<&str> = t.split_whitespace().take(8).collect();
    while words.len() > 2 && words.iter().map(|w| w.len() + 1).sum::<usize>() > 61 {
        words.pop();
    }
    let small = ["a", "an", "the", "as", "to", "of", "for", "and", "or", "in", "on", "with", "at", "by", "from"];
    while words.len() > 2 && words.last().is_some_and(|w| small.contains(&w.to_lowercase().as_str())) {
        words.pop();
    }
    words.join(" ").trim_end_matches([',', ':', ';', '-']).to_string()
}

pub async fn summarize(http: &reqwest::Client, ep: &Endpoint, transcript: &str) -> AppResult<ChatSummary> {
    let schema = json!({
        "type": "object",
        "properties": { "title": { "type": "string" }, "summary": { "type": "string" }, "tags": { "type": "array", "maxItems": 3, "items": { "type": "string" } } },
        "required": ["title", "summary", "tags"]
    });
    let text = crate::chat::complete_json(http, ep, INSTRUCTIONS, &format!("Conversation:\n\n{transcript}\nLabel it."), schema, 160).await?;
    let text = text.as_str();
    let mut s = parse(text).ok_or_else(|| AppError::msg("the model didn't return a usable title"))?;
    let lower = transcript.to_lowercase();
    s.tags.retain(|t| !EXAMPLE_TAGS.contains(&t.as_str()) || lower.contains(t.as_str()));
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_tidies_replies() {
        let s = parse("Sure! {\"title\": \"\\\"Lisbon weekend plan.\\\"\", \"summary\": \" A 3-day Lisbon plan. \", \"tags\": [\"#Travel\", \"Portugal\", \"food\", \"extra\"]}").unwrap();
        assert_eq!(s.title, "Lisbon weekend plan");
        assert_eq!(short_title("How to file quarterly estimated taxes as a freelancer"), "How to file quarterly estimated taxes");
        assert_eq!(s.summary, "A 3-day Lisbon plan.");
        assert_eq!(s.tags, vec!["travel", "portugal", "food"]);
        assert!(parse("no json here").is_none());
        assert!(parse("{\"title\": \"\", \"summary\": \"x\"}").is_none());
        assert_eq!(parse("{\"title\": \"T\", \"summary\": \"S\"}").unwrap().tags.len(), 0);
        // Copying the example from the instructions isn't a real label.
        assert!(parse("{\"title\": \"Fixing a slipping bike chain\", \"summary\": \"x\"}").is_none());
    }

    #[test]
    fn transcript_is_capped() {
        let msgs = vec![
            json!({ "role": "user", "content": "Plan a weekend in Lisbon" }),
            json!({ "role": "assistant", "content": "x".repeat(5000) }),
            json!({ "role": "user", "content": "" }),
        ];
        let t = transcript(&msgs, 800);
        assert!(t.starts_with("User: Plan a weekend in Lisbon"));
        assert!(t.len() <= 820, "{}", t.len());
    }

    /// Real engine: returns a usable title and summary.
    #[tokio::test]
    #[ignore]
    async fn e2e_summarizes_a_chat() {
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let chats = [
            ("Can you plan a relaxed three-day weekend in Lisbon with good food?", "Day 1: Alfama, tram 28 and dinner in Bairro Alto. Day 2: Belém and pastéis de nata. Day 3: Sintra.", ["lisbon", "travel", "trip", "weekend", "food"]),
            ("How do quarterly estimated taxes work for a freelancer?", "You pay four times a year using Form 1040-ES, based on expected income; missing payments can mean penalties.", ["tax", "taxes", "freelanc", "estimated", "quarterly"]),
        ];
        for (q, a, words) in chats {
            let msgs = vec![json!({ "role": "user", "content": q }), json!({ "role": "assistant", "content": a })];
            let s = summarize(&crate::chat::local_client(), &ep, &transcript(&msgs, 3000)).await.unwrap();
            eprintln!("{s:?}");
            assert!(!s.title.is_empty() && s.title.split_whitespace().count() <= 8);
            // The label is about this chat.
            let all = format!("{} {} {}", s.title, s.summary, s.tags.join(" ")).to_lowercase();
            assert!(words.iter().any(|w| all.contains(w)), "{s:?}");
            assert!(!s.tags.iter().any(|t| EXAMPLE_TAGS.contains(&t.as_str())), "{s:?}");
        }
    }
}
