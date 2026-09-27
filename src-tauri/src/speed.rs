//! Measures real generation speed on this Mac, so BYTE can decide from
//! numbers (not guesses) whether Speed boost helps with the current model.

use std::time::Duration;

use serde_json::{json, Value};

use crate::engine::Endpoint;
use crate::error::{AppError, AppResult};

/// Prompts with a typical mix: code (very predictable) and prose.
const PROMPTS: [&str; 2] = [
    "Write a Python function that checks whether a string is a palindrome, with a docstring and three doctests.",
    "Explain in two short paragraphs why the sky is blue.",
];

/// Generation and prompt-reading speed, tokens per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speed {
    pub generate: f64,
    pub read: f64,
}

/// A ~1,500-token document, like a pasted article, to time prompt reading.
fn long_document() -> String {
    let para = "BYTE runs language models on the Mac's GPU. Reading a long prompt is limited by compute, \
while writing each new word is limited by how fast the model's weights can be read from memory. ";
    format!("Here is a document:\n\n{}\n\nSummarize it in three bullet points.", para.repeat(40))
}

/// Measures both speeds (thinking off, fixed lengths). Takes 10–40 seconds.
pub async fn measure_both(http: &reqwest::Client, ep: &Endpoint) -> AppResult<Speed> {
    let generate = measure(http, ep).await?;
    let v = request(http, ep, &long_document(), 32).await?;
    let read = v["timings"]["prompt_per_second"].as_f64().unwrap_or(0.0);
    Ok(Speed { generate, read })
}

async fn request(http: &reqwest::Client, ep: &Endpoint, prompt: &str, max_tokens: u32) -> AppResult<Value> {
    let body = json!({
        "messages": [{ "role": "user", "content": prompt }],
        "max_tokens": max_tokens,
        "temperature": 0,
        "stream": false,
        "cache_prompt": false,
        "chat_template_kwargs": { "enable_thinking": false },
    });
    Ok(http
        .post(format!("{}/v1/chat/completions", ep.base_url))
        .bearer_auth(&ep.api_key)
        .timeout(Duration::from_secs(300))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Average tokens/sec over the test prompts (thinking off, fixed length).
pub async fn measure(http: &reqwest::Client, ep: &Endpoint) -> AppResult<f64> {
    let mut rates = Vec::new();
    for p in PROMPTS {
        let body = json!({
            "messages": [{ "role": "user", "content": p }],
            "max_tokens": 160,
            "temperature": 0,
            "stream": false,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let v: Value = http
            .post(format!("{}/v1/chat/completions", ep.base_url))
            .bearer_auth(&ep.api_key)
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tps = v["timings"]["predicted_per_second"].as_f64().unwrap_or(0.0);
        if tps > 0.0 {
            rates.push(tps);
        }
    }
    if rates.is_empty() {
        return Err(AppError::msg("the engine didn't report its speed"));
    }
    Ok(rates.iter().sum::<f64>() / rates.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real engine with BYTE's exact Speed boost arguments: the server accepts
    /// them and the helper's guesses are used. Needs BYTE_TEST_MAIN_MODEL and
    /// BYTE_TEST_DRAFT_MODEL (e.g. Qwen3.5 2B + 0.8B).
    #[tokio::test]
    #[ignore]
    async fn e2e_speed_boost_drafts_tokens() {
        let (Ok(main), Ok(draft)) = (std::env::var("BYTE_TEST_MAIN_MODEL"), std::env::var("BYTE_TEST_DRAFT_MODEL")) else {
            eprintln!("skipping: set BYTE_TEST_MAIN_MODEL and BYTE_TEST_DRAFT_MODEL");
            return;
        };
        let args = crate::engine::draft_args(std::path::Path::new(&draft));
        let Some((_server, ep)) = crate::chat::e2e_support::start_server_with(&main, &args).await else { return };
        let body = json!({
            "messages": [{ "role": "user", "content": PROMPTS[0] }],
            "max_tokens": 120, "temperature": 0, "stream": false,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let v: Value = crate::chat::local_client()
            .post(format!("{}/v1/chat/completions", ep.base_url))
            .bearer_auth(&ep.api_key)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let t = &v["timings"];
        eprintln!("drafted {} accepted {} at {:.1} tok/s", t["draft_n"], t["draft_n_accepted"], t["predicted_per_second"].as_f64().unwrap_or(0.0));
        assert!(t["draft_n"].as_u64().unwrap_or(0) > 0, "no tokens were drafted: {t}");
        assert!(t["draft_n_accepted"].as_u64().unwrap_or(0) > 0);
    }

    /// Real engine: reports a positive speed.
    #[tokio::test]
    #[ignore]
    async fn e2e_measures_speed() {
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let s = measure_both(&crate::chat::local_client(), &ep).await.unwrap();
        eprintln!("{:.1} tok/s writing, {:.0} tok/s reading", s.generate, s.read);
        assert!(s.generate > 0.0 && s.read > 0.0);
    }
}
