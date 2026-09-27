//! Measures real generation speed on this Mac, so BYTE can decide from
//! numbers (not guesses) whether Speed boost helps with the current model.

use std::time::Duration;

use serde_json::{json, Value};

use crate::engine::Endpoint;
use crate::error::{AppError, AppResult};

/// Prompts with a typical mix: code (very predictable), prose, and an edit
/// that repeats given text (where repeated-text guessing shines).
const PROMPTS: [&str; 3] = [
    "Write a Python function that checks whether a string is a palindrome, with a docstring and three doctests.",
    "Explain in two short paragraphs why the sky is blue.",
    "Fix the spelling in this paragraph and return the whole corrected paragraph, nothing else:\n\n\
The quick brown fox jumpd over the lazy dog while the farmer watchd from the porch. \
Every mornign the fox came back to the same feild, looking for the chickens that the farmer kept \
behind the old red barn. The farmer never minded, becuase the fox never caught any of them.",
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
            "max_tokens": 120,
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
        let args = crate::engine::draft_args(Some(&crate::engine::Draft::model(draft)), false, None, 0.75);
        let Some((_server, ep)) = crate::chat::e2e_support::start_server_with(&main, &args, None).await else { return };
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

    /// Real engine accepts every setting the tuner can pick, all at once.
    #[tokio::test]
    #[ignore]
    async fn e2e_engine_accepts_all_tuning_options() {
        let (Ok(main), Ok(draft)) = (std::env::var("BYTE_TEST_MAIN_MODEL"), std::env::var("BYTE_TEST_DRAFT_MODEL")) else {
            eprintln!("skipping: set BYTE_TEST_MAIN_MODEL and BYTE_TEST_DRAFT_MODEL");
            return;
        };
        for opts in [
            crate::engine::LaunchOpts { draft: Some(crate::engine::Draft::model(&draft)), ngram: true, kv_f16: true, ubatch: Some(2048), flash_attn_off: true, draft_n_max: Some(24), draft_p_min: Some(0.6) },
            crate::engine::LaunchOpts { draft: Some(crate::engine::Draft::model(&draft)), ngram: false, kv_f16: false, ubatch: Some(256), flash_attn_off: false, draft_n_max: Some(8), draft_p_min: Some(0.9) },
        ] {
            let Some((_server, ep)) = crate::chat::e2e_support::start_server_with(&main, &[], Some(&opts)).await else { return };
            let s = measure_both(&crate::chat::local_client(), &ep).await.unwrap();
            eprintln!("{opts:?}: {:.1} tok/s writing, {:.0} reading", s.generate, s.read);
            assert!(s.generate > 0.0 && s.read > 0.0);
        }
    }

    /// Real engine with a model's own multi-token-prediction head, through
    /// BYTE's exact arguments. Needs BYTE_TEST_HEAD_MAIN and BYTE_TEST_HEAD
    /// (e.g. Gemma 4 E2B + mtp-gemma-4-E2B-it.gguf).
    #[tokio::test]
    #[ignore]
    async fn e2e_speed_head_drafts_tokens() {
        let (Ok(main), Ok(head)) = (std::env::var("BYTE_TEST_HEAD_MAIN"), std::env::var("BYTE_TEST_HEAD")) else {
            eprintln!("skipping: set BYTE_TEST_HEAD_MAIN and BYTE_TEST_HEAD");
            return;
        };
        let opts = crate::engine::LaunchOpts {
            draft: Some(crate::engine::Draft { path: head.into(), kind: crate::models::HelperKind::Mtp }),
            ngram: true,
            ..Default::default()
        };
        let Some((_server, ep)) = crate::chat::e2e_support::start_server_with(&main, &[], Some(&opts)).await else { return };
        let v = request(&crate::chat::local_client(), &ep, PROMPTS[2], 100).await.unwrap();
        let t = &v["timings"];
        eprintln!("drafted {} accepted {} at {:.1} tok/s", t["draft_n"], t["draft_n_accepted"], t["predicted_per_second"].as_f64().unwrap_or(0.0));
        assert!(t["draft_n_accepted"].as_u64().unwrap_or(0) > 0, "no guesses were used: {t}");
    }

    /// Real engine: repeated-text guessing alone (no helper file) speeds up an edit.
    #[tokio::test]
    #[ignore]
    async fn e2e_repeated_text_guessing() {
        let opts = crate::engine::LaunchOpts { ngram: true, ..Default::default() };
        let Some(model) = std::env::var("BYTE_TEST_MODEL").ok() else { return };
        let Some((_server, ep)) = crate::chat::e2e_support::start_server_with(&model, &[], Some(&opts)).await else { return };
        let v = request(&crate::chat::local_client(), &ep, PROMPTS[2], 100).await.unwrap();
        let t = &v["timings"];
        eprintln!("drafted {} accepted {}", t["draft_n"], t["draft_n_accepted"]);
        assert!(v["choices"][0]["message"]["content"].as_str().is_some_and(|c| c.contains("fox")));
        assert!(t["draft_n"].as_u64().unwrap_or(0) > 0, "nothing was guessed: {t}");
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
