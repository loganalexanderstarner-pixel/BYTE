//! Embeddings: turns text into vectors so BYTE can find passages by meaning,
//! not only by exact words (knowledge base search, instant-answer cache).
//!
//! A second, tiny llama-server runs the catalog's embedding model
//! (nomic-embed-text v1.5, ~146 MB) in `--embedding` mode. It starts the first
//! time it's needed and stops after `IDLE_STOP` without use.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::engine::{Endpoint, Engine, EngineStatus, LaunchOpts};
use crate::error::{AppError, AppResult};
use crate::models::{self, Catalog, Role};

/// Stop the embedding engine after this long without use.
const IDLE_STOP: Duration = Duration::from_secs(5 * 60);
/// Texts sent per request.
const BATCH: usize = 32;
/// Each text must fit the model's 2,048-token window; cut longer ones.
const MAX_CHARS: usize = 5_000;
/// Context (and batch) size for the embedding engine.
const CONTEXT: u32 = 2048;

/// What a text is, for models that are trained with task prefixes (nomic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// A passage to be found later (knowledge base chunks).
    Document,
    /// Something being searched for (the user's question).
    Query,
}

impl Purpose {
    fn prefix(self) -> &'static str {
        match self {
            Purpose::Document => "search_document: ",
            Purpose::Query => "search_query: ",
        }
    }
}

#[derive(Clone)]
pub struct Embedder {
    engine: Engine,
    last_used: Arc<std::sync::Mutex<Instant>>,
    /// Held while starting, so two searches don't start two engines.
    starting: Arc<Mutex<()>>,
    http: reqwest::Client,
}

impl Embedder {
    pub fn new(pid_file: std::path::PathBuf) -> Self {
        Embedder {
            engine: Engine::helper(pid_file),
            last_used: Arc::new(std::sync::Mutex::new(Instant::now())),
            starting: Arc::new(Mutex::new(())),
            http: crate::chat::local_client(),
        }
    }

    /// Download key of the embedding model ("id:quant").
    pub fn model_key(catalog: &Catalog) -> Option<String> {
        let m = catalog.models.iter().find(|m| m.role == Role::Embed)?;
        m.variants.first().map(|v| models::key(m, v))
    }

    /// Whether the embedding model is downloaded.
    pub fn installed(catalog: &Catalog, models_dir: &Path) -> bool {
        Self::model_key(catalog)
            .and_then(|k| catalog.resolve(&k).ok().map(|(_, v)| models::is_installed(models_dir, v)))
            .unwrap_or(false)
    }

    pub async fn running(&self) -> bool {
        matches!(self.engine.status().await, EngineStatus::Ready { .. })
    }

    pub fn reap_stale(&self) {
        self.engine.reap_stale();
    }

    pub fn kill_now(&self) {
        self.engine.kill_now();
    }

    /// Vectors for `texts` (unit length, so a dot product is the cosine).
    pub async fn embed(&self, app: &AppHandle, models_dir: &Path, catalog: &Catalog, texts: &[String], purpose: Purpose) -> AppResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let ep = self.ensure_running(app, models_dir, catalog).await?;
        self.touch();
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            out.extend(embed_at(&self.http, &ep, batch, purpose).await?);
            self.touch();
        }
        Ok(out)
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
        let key = Self::model_key(catalog).ok_or_else(|| AppError::msg("this model list has no embedding model"))?;
        if !Self::installed(catalog, models_dir) {
            return Err(AppError::msg("the search model for your files isn't downloaded yet (Settings → Knowledge base)"));
        }
        let opts = LaunchOpts { embedding: true, ubatch: Some(CONTEXT), ..Default::default() };
        self.engine.start(app, models_dir.to_path_buf(), catalog, &key, Some(CONTEXT), 0, opts).await?;
        self.touch();
        self.stop_when_idle();
        self.engine.endpoint().await.ok_or_else(|| AppError::msg("the embedding engine stopped while starting"))
    }

    /// Checks once a minute and stops the engine after `IDLE_STOP` unused.
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
                    log::info!("embedding engine idle for {} s; stopping it", idle.as_secs());
                    this.engine.stop().await;
                    return;
                }
            }
        });
    }
}

/// One `/v1/embeddings` request, results in input order and normalized.
async fn embed_at(http: &reqwest::Client, ep: &Endpoint, texts: &[String], purpose: Purpose) -> AppResult<Vec<Vec<f32>>> {
    let input: Vec<String> = texts.iter().map(|t| format!("{}{}", purpose.prefix(), clip(t))).collect();
    let body = serde_json::json!({ "input": input, "model": ep.model });
    let r = http
        .post(format!("{}/v1/embeddings", ep.base_url))
        .bearer_auth(&ep.api_key)
        .timeout(Duration::from_secs(120))
        .json(&body)
        .send()
        .await?;
    if !r.status().is_success() {
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        return Err(AppError::msg(format!("embedding request failed ({status}): {}", text.chars().take(200).collect::<String>())));
    }
    let v: serde_json::Value = r.json().await?;
    let mut rows: Vec<(usize, Vec<f32>)> = v["data"]
        .as_array()
        .ok_or_else(|| AppError::msg("embedding reply had no data"))?
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let idx = d["index"].as_u64().map(|x| x as usize).unwrap_or(i);
            let vec = d["embedding"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default();
            (idx, normalize(vec))
        })
        .collect();
    if rows.len() != texts.len() {
        return Err(AppError::msg(format!("embedding reply had {} vectors for {} texts", rows.len(), texts.len())));
    }
    rows.sort_by_key(|(i, _)| *i);
    Ok(rows.into_iter().map(|(_, v)| v).collect())
}

fn clip(t: &str) -> &str {
    match t.char_indices().nth(MAX_CHARS) {
        Some((i, _)) => &t[..i],
        None => t,
    }
}

pub fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// Cosine similarity of two unit vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Vectors as little-endian bytes (for the database) and back.
pub fn to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_bytes(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn ep(base: String) -> Endpoint {
        Endpoint { base_url: base, api_key: "k".into(), model: "nomic-embed-v1.5:Q8_0".into(), context: CONTEXT, vision: false }
    }

    #[tokio::test]
    async fn embeddings_come_back_in_order_normalized_and_prefixed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(header("authorization", "Bearer k"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let input = body["input"].as_array().unwrap();
                assert!(input.iter().all(|t| t.as_str().unwrap().starts_with("search_query: ")));
                // Reply out of order to check sorting.
                let data: Vec<_> = (0..input.len()).rev().map(|i| serde_json::json!({ "index": i, "embedding": [3.0 * (i + 1) as f64, 4.0 * (i + 1) as f64] })).collect();
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": data }))
            })
            .mount(&server)
            .await;
        let texts = vec!["a".to_string(), "b".to_string()];
        let v = embed_at(&crate::chat::local_client(), &ep(server.uri()), &texts, Purpose::Query).await.unwrap();
        assert_eq!(v.len(), 2);
        assert!((v[0][0] - 0.6).abs() < 1e-6 && (v[0][1] - 0.8).abs() < 1e-6);
        assert!((cosine(&v[0], &v[1]) - 1.0).abs() < 1e-6);
    }

    /// Real engine + nomic-embed (BYTE_TEST_LLAMA_SERVER, BYTE_TEST_EMBED_MODEL):
    /// BYTE's embedding flags load, and a question lands nearest its answer.
    #[tokio::test]
    #[ignore]
    async fn e2e_embeddings_find_the_passage_that_answers() {
        let Ok(model) = std::env::var("BYTE_TEST_EMBED_MODEL") else {
            eprintln!("skipping: set BYTE_TEST_EMBED_MODEL");
            return;
        };
        let opts = LaunchOpts { embedding: true, ubatch: Some(CONTEXT), ..Default::default() };
        let Some((_server, ep)) = crate::chat::e2e_support::start_server_with(&model, &[], Some(&opts)).await else { return };
        let http = crate::chat::local_client();
        let docs: Vec<String> = [
            "The lease says the tenant pays for damage caused by leaks they didn't report within 48 hours.",
            "Sourdough needs a starter fed with flour and water every day.",
            "The Rust compiler checks borrowing rules at compile time.",
        ]
        .map(String::from)
        .to_vec();
        let d = embed_at(&http, &ep, &docs, Purpose::Document).await.unwrap();
        let q = embed_at(&http, &ep, &["who pays if my apartment has water damage?".to_string()], Purpose::Query).await.unwrap();
        let scores: Vec<f32> = d.iter().map(|v| cosine(&q[0], v)).collect();
        eprintln!("scores: {scores:?}");
        assert!(scores[0] > scores[1] && scores[0] > scores[2], "{scores:?}");
    }

    #[test]
    fn vectors_round_trip_through_bytes_and_long_texts_are_cut() {
        let v = normalize(vec![1.0, 2.0, 2.0]);
        assert_eq!(from_bytes(&to_bytes(&v)), v);
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
        let long = "é".repeat(MAX_CHARS + 10);
        assert_eq!(clip(&long).chars().count(), MAX_CHARS);
    }
}
