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

/// Average tokens/sec over the test prompts (thinking off, fixed length).
pub async fn measure(http: &reqwest::Client, ep: &Endpoint) -> AppResult<f64> {
    let mut rates = Vec::new();
    for p in PROMPTS {
        let body = json!({
            "messages": [{ "role": "user", "content": p }],
            "max_tokens": 200,
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

/// Keep Speed boost only if it's clearly faster (5%+) on this Mac.
pub fn boost_wins(with: Option<f64>, without: f64) -> bool {
    with.is_some_and(|w| w > without * 1.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boost_must_be_clearly_faster() {
        assert!(boost_wins(Some(20.0), 13.0));
        assert!(!boost_wins(Some(13.3), 13.0));
        assert!(!boost_wins(None, 13.0));
    }

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
        let tps = measure(&crate::chat::local_client(), &ep).await.unwrap();
        eprintln!("{tps:.1} tok/s");
        assert!(tps > 0.0);
    }
}
