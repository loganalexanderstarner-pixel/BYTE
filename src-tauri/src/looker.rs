//! The photo helper: lets a model that can't see images answer about a photo. A small
//! vision model (from the catalog, with its image adapter) runs in a second, on-demand
//! llama-server like the embedder (`embed.rs`), describes the photo in words, and the
//! description goes to the main model with the rest of the message. It stops after
//! `IDLE_STOP` without use.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::engine::{Endpoint, Engine, LaunchOpts};
use crate::error::{AppError, AppResult};
use crate::models::{self, Catalog, CatalogModel};

const IDLE_STOP: Duration = Duration::from_secs(5 * 60);
const CONTEXT: u32 = 4096;
/// The helper BYTE offers to download (small, sees images, reads text in photos).
pub const DEFAULT_HELPER: &str = "qwen3.5-0.8b";
/// Largest model used as a helper (billions of parameters): it runs beside the main model.
const MAX_HELPER_B: f32 = 4.5;

/// Starts a photo's text once the helper has described it (see `chat::with_files`).
pub const DESCRIBED: &str = "[What the photo shows, described by BYTE's photo helper]\n";

/// What the helper is asked, before the user's question (if any).
pub const LOOK: &str = "Describe this photo in detail for someone who can't see it: what it shows, the setting, people and \
objects, colours, and any text in it (copy text exactly). Plain sentences, no preamble.";

#[derive(Clone)]
pub struct Looker {
    engine: Engine,
    last_used: Arc<std::sync::Mutex<Instant>>,
    starting: Arc<Mutex<()>>,
    http: reqwest::Client,
}

fn usable(m: &CatalogModel) -> bool {
    m.vision.is_some() && !m.id.starts_with("community") && m.params_b.is_some_and(|b| b <= MAX_HELPER_B)
}

/// The helper model's key ("id:quant"): a downloaded vision model with its image adapter,
/// the default helper first, else the smallest. `None` if none is downloaded.
pub fn choose(catalog: &Catalog, models_dir: &Path) -> Option<String> {
    let mut found: Vec<(bool, f32, String)> = catalog
        .models
        .iter()
        .filter(|m| usable(m) && models::vision_path(models_dir, m).is_some())
        .filter_map(|m| {
            let v = m.variants.iter().filter(|v| models::is_installed(models_dir, v)).min_by_key(|v| v.size_bytes)?;
            Some((m.id != DEFAULT_HELPER, m.params_b.unwrap_or(99.0), models::key(m, v)))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    found.into_iter().next().map(|(_, _, k)| k)
}

/// What to download for the default helper: the model (its smallest ~4-bit version)
/// and its image adapter.
pub fn downloads(catalog: &Catalog) -> Vec<String> {
    let Some(m) = catalog.models.iter().find(|m| m.id == DEFAULT_HELPER && m.vision.is_some()) else { return vec![] };
    let pick = m.variants.iter().filter(|v| v.bits >= 4.0).min_by_key(|v| v.size_bytes).or_else(|| m.variants.iter().min_by_key(|v| v.size_bytes));
    pick.map(|v| vec![models::key(m, v), format!("{}:{}", m.id, models::VISION_QUANT)]).unwrap_or_default()
}

/// Download size of the default helper (model + adapter), for the Settings offer.
pub fn download_bytes(catalog: &Catalog) -> u64 {
    downloads(catalog).iter().filter_map(|k| catalog.download_target(k).ok()).map(|(_, v, _)| v.size_bytes).sum()
}

/// The request that asks the helper about one photo.
pub fn request(image_url: &str, question: &str) -> serde_json::Value {
    let ask = if question.trim().is_empty() { LOOK.to_string() } else { format!("{LOOK}\nThe user asks about it: \"{}\"; include what's needed to answer that.", question.trim()) };
    serde_json::json!({
        "messages": [{ "role": "user", "content": [
            { "type": "text", "text": ask },
            { "type": "image_url", "image_url": { "url": image_url } }
        ]}],
        "max_tokens": 450,
        // Small vision models loop ("the red square is… the red square is…") without these.
        "temperature": 0.7,
        "top_p": 0.8,
        "presence_penalty": 1.5,
        "repeat_penalty": 1.05,
        "stream": false,
        "chat_template_kwargs": { "enable_thinking": false },
    })
}

impl Looker {
    pub fn new(pid_file: std::path::PathBuf) -> Self {
        Looker {
            engine: Engine::helper(pid_file),
            last_used: Arc::new(std::sync::Mutex::new(Instant::now())),
            starting: Arc::new(Mutex::new(())),
            http: crate::chat::local_client(),
        }
    }

    pub fn reap_stale(&self) {
        self.engine.reap_stale();
    }

    pub fn kill_now(&self) {
        self.engine.kill_now();
    }

    /// The photo in words (and any text in it).
    pub async fn describe(&self, app: &AppHandle, models_dir: &Path, catalog: &Catalog, image_url: &str, question: &str) -> AppResult<String> {
        let ep = self.ensure_running(app, models_dir, catalog).await?;
        self.touch();
        let r = self
            .http
            .post(format!("{}/v1/chat/completions", ep.base_url))
            .bearer_auth(&ep.api_key)
            .timeout(Duration::from_secs(180))
            .json(&request(image_url, question))
            .send()
            .await?;
        self.touch();
        if !r.status().is_success() {
            return Err(AppError::msg(format!("the photo helper failed ({})", r.status())));
        }
        let v: serde_json::Value = r.json().await?;
        let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            return Err(AppError::msg("the photo helper had nothing to say"));
        }
        Ok(text)
    }

    fn touch(&self) {
        if let Ok(mut t) = self.last_used.lock() {
            *t = Instant::now();
        }
    }

    async fn ensure_running(&self, app: &AppHandle, models_dir: &Path, catalog: &Catalog) -> AppResult<Endpoint> {
        if let Some(ep) = self.engine.endpoint().await {
            return Ok(ep);
        }
        let _guard = self.starting.lock().await;
        if let Some(ep) = self.engine.endpoint().await {
            return Ok(ep);
        }
        let key = choose(catalog, models_dir).ok_or_else(|| AppError::msg("the photo helper isn't downloaded yet (Settings → Features → Photo helper)"))?;
        let (m, _) = catalog.resolve(&key)?;
        let opts = LaunchOpts { mmproj: models::vision_path(models_dir, m), ..Default::default() };
        self.engine.start(app, models_dir.to_path_buf(), catalog, &key, Some(CONTEXT), 0, opts).await?;
        self.touch();
        self.stop_when_idle();
        self.engine.endpoint().await.ok_or_else(|| AppError::msg("the photo helper stopped while starting"))
    }

    fn stop_when_idle(&self) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                if this.engine.endpoint().await.is_none() {
                    return;
                }
                let idle = this.last_used.lock().map(|t| t.elapsed()).unwrap_or_default();
                if idle >= IDLE_STOP {
                    log::info!("photo helper idle for {} s; stopping it", idle.as_secs());
                    this.engine.stop().await;
                    return;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_carries_the_photo_and_the_question() {
        let r = request("data:image/png;base64,AAAA", "What breed is this dog?");
        let parts = r["messages"][0]["content"].as_array().unwrap();
        assert!(parts[0]["text"].as_str().unwrap().contains("What breed is this dog?"));
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,AAAA");
        assert_eq!(r["chat_template_kwargs"]["enable_thinking"], false);
    }

    #[test]
    fn a_described_photo_is_passed_on_as_words() {
        let photo = crate::files::Ingested {
            name: "dog.jpg".into(),
            kind: crate::files::FileKind::Image,
            pages: None,
            text: format!("{DESCRIBED}A brown dog on a beach."),
            truncated: false,
            image: Some("data:image/jpeg;base64,AAAA".into()),
            ocr: false,
        };
        let m = crate::chat::ChatMessage { role: "user".into(), content: "What breed is this?".into(), files: vec![photo], images: vec![] };
        let out = crate::chat::with_files(&[m], 8192, false);
        assert!(out[0].images.is_empty(), "no image for a model that can't see");
        assert!(out[0].content.contains("A brown dog on a beach."));
        assert!(out[0].content.contains("described it for you"));
    }

    /// Real engine: the helper describes a red square. Needs BYTE_TEST_LLAMA_SERVER,
    /// BYTE_TEST_VISION_MODEL and BYTE_TEST_VISION_MMPROJ.
    #[tokio::test]
    #[ignore]
    async fn e2e_looker_describes_a_photo() {
        use base64::Engine as _;
        let (Ok(model), Ok(mmproj)) = (std::env::var("BYTE_TEST_VISION_MODEL"), std::env::var("BYTE_TEST_VISION_MMPROJ")) else { return };
        let opts = LaunchOpts { mmproj: Some(mmproj.into()), ..Default::default() };
        let Some((_server, ep)) = crate::chat::e2e_support::start_server_with(&model, &[], Some(&opts)).await else { return };
        // A white picture with a red square and "BYTE 1234" in black.
        let png = include_bytes!("../tests/fixtures/photo_red_square.png");
        let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png));
        let r = crate::chat::local_client()
            .post(format!("{}/v1/chat/completions", ep.base_url))
            .bearer_auth(&ep.api_key)
            .json(&request(&url, "What colour is the shape?"))
            .send()
            .await
            .unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_lowercase();
        eprintln!("described: {text}");
        assert!(text.contains("red"), "{text}");
        assert!(text.contains("byte"), "the text in the photo: {text}");
    }

    #[test]
    fn the_default_helper_is_offered_with_its_adapter() {
        let catalog = Catalog::embedded();
        let d = downloads(&catalog);
        assert_eq!(d.len(), 2, "{d:?}");
        assert!(d[0].starts_with(&format!("{DEFAULT_HELPER}:")));
        assert_eq!(d[1], format!("{DEFAULT_HELPER}:{}", models::VISION_QUANT));
        let mb = download_bytes(&catalog) / 1_000_000;
        assert!((300..2000).contains(&mb), "{mb} MB");
        // Nothing is downloaded in a fresh folder.
        assert!(choose(&catalog, Path::new("/nowhere")).is_none());
    }
}
