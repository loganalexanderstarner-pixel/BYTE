//! Model catalog and the resumable, verified downloader.
//!
//! The catalog (`catalog/models.json`, ~13 KB) lists models and, for each, a
//! few quantized sizes ("variants"). It is compiled into the app and can be
//! refreshed from a URL, so new models appear without an app update. Model
//! files themselves are downloaded from Hugging Face only when chosen.
//!
//! A model is addressed by a key `"<model id>:<quant>"`, e.g.
//! `"qwen3.8-27b:UD-IQ3_XXS"`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::system::{self, Fit, FitPlan, ModelArch, SystemInfo};

// ---------- catalog data ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// A chat model the user talks to.
    Chat,
    /// Small helper for speculative decoding and routing.
    Draft,
    /// Embedding model for search and the knowledge base.
    Embed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    /// Path inside the Hugging Face repo (may include a folder).
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub quant: String,
    /// Approximate bits per weight; lower means smaller but less accurate.
    pub bits: f32,
    pub size_bytes: u64,
    /// One file, or several parts ("-00001-of-00003") for big models.
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub released: Option<String>,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub thinking: bool,
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub license: Option<String>,
    /// Curated overall capability (0–100) used for recommendations.
    #[serde(default)]
    pub quality: u32,
    pub repo: String,
    pub role: Role,
    #[serde(default)]
    pub size_label: Option<String>,
    pub arch: ModelArch,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub version: u32,
    pub generated: String,
    pub models: Vec<CatalogModel>,
}

/// The catalog compiled into this build.
pub const EMBEDDED_CATALOG: &str = include_str!("../catalog/models.json");

/// Where fresh catalogs are published.
pub const DEFAULT_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/loganalexanderstarner-pixel/BYTE/main/src-tauri/catalog/models.json";

impl Catalog {
    pub fn embedded() -> Catalog {
        Catalog::parse(EMBEDDED_CATALOG).expect("embedded catalog is valid")
    }

    pub fn parse(text: &str) -> AppResult<Catalog> {
        let c: Catalog = serde_json::from_str(text)?;
        c.validate()?;
        Ok(c)
    }

    fn validate(&self) -> AppResult<()> {
        if self.version != 1 {
            return Err(AppError::msg(format!("unsupported catalog version {}", self.version)));
        }
        if self.models.is_empty() {
            return Err(AppError::msg("catalog has no models"));
        }
        for m in &self.models {
            if m.variants.is_empty() || m.variants.iter().any(|v| v.files.is_empty()) {
                return Err(AppError::msg(format!("catalog entry {} has no files", m.id)));
            }
            for f in m.variants.iter().flat_map(|v| &v.files) {
                if f.sha256.len() != 64 || f.name.contains("..") || f.name.starts_with('/') {
                    return Err(AppError::msg(format!("catalog entry {} has an invalid file", m.id)));
                }
            }
        }
        Ok(())
    }

    pub fn model(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Resolves `"model:quant"`. Keys from older builds (`"qwen3-14b"`) map to
    /// the variant those builds downloaded.
    pub fn resolve(&self, key: &str) -> AppResult<(&CatalogModel, &Variant)> {
        let (id, quant) = match key.split_once(':') {
            Some((id, q)) => (id, Some(q)),
            None => (key, None),
        };
        let model = self.model(id).ok_or_else(|| AppError::msg(format!("unknown model '{id}'")))?;
        let variant = match quant {
            Some(q) => model.variants.iter().find(|v| v.quant == q),
            None => model.variants.iter().find(|v| v.quant == "Q4_K_M").or_else(|| model.variants.first()),
        }
        .ok_or_else(|| AppError::msg(format!("{} has no '{}' version", model.name, quant.unwrap_or("default"))))?;
        Ok((model, variant))
    }
}

pub fn key(model: &CatalogModel, variant: &Variant) -> String {
    format!("{}:{}", model.id, variant.quant)
}

/// Holds the current catalog; replaced when a newer one is fetched.
pub struct CatalogStore {
    current: RwLock<Arc<Catalog>>,
    cache_file: PathBuf,
}

impl CatalogStore {
    /// Starts from the cached remote catalog if it's newer than the embedded one.
    pub fn load(cache_file: PathBuf) -> Self {
        let embedded = Catalog::embedded();
        let cached = std::fs::read_to_string(&cache_file).ok().and_then(|t| Catalog::parse(&t).ok());
        let best = match cached {
            Some(c) if c.generated > embedded.generated => c,
            _ => embedded,
        };
        CatalogStore { current: RwLock::new(Arc::new(best)), cache_file }
    }

    pub fn get(&self) -> Arc<Catalog> {
        self.current.read().expect("catalog lock").clone()
    }

    /// Fetches the catalog from `url`; keeps it if valid and newer.
    pub async fn refresh(&self, client: &reqwest::Client, url: &str) -> AppResult<bool> {
        let text = client.get(url).timeout(Duration::from_secs(15)).send().await?.error_for_status()?.text().await?;
        let fresh = Catalog::parse(&text)?;
        if fresh.generated <= self.get().generated {
            return Ok(false);
        }
        let _ = std::fs::write(&self.cache_file, &text);
        *self.current.write().expect("catalog lock") = Arc::new(fresh);
        Ok(true)
    }
}

// ---------- files on disk ----------

/// Local path of a model file. Files are stored by their file name so
/// downloads from older builds are reused.
pub fn file_path(models_dir: &Path, f: &ModelFile) -> PathBuf {
    models_dir.join(Path::new(&f.name).file_name().unwrap_or_default())
}

fn part_path(models_dir: &Path, f: &ModelFile) -> PathBuf {
    let mut p = file_path(models_dir, f).into_os_string();
    p.push(".part");
    PathBuf::from(p)
}

/// The file llama-server is pointed at (the first part of a split model).
pub fn entry_path(models_dir: &Path, v: &Variant) -> PathBuf {
    file_path(models_dir, &v.files[0])
}

pub fn is_installed(models_dir: &Path, v: &Variant) -> bool {
    v.files
        .iter()
        .all(|f| std::fs::metadata(file_path(models_dir, f)).map(|m| m.len() == f.size).unwrap_or(false))
}

/// Bytes already on disk for a variant (finished files plus partial ones).
pub fn bytes_on_disk(models_dir: &Path, v: &Variant) -> u64 {
    v.files
        .iter()
        .map(|f| {
            let done = std::fs::metadata(file_path(models_dir, f)).map(|m| m.len()).unwrap_or(0);
            if done == f.size {
                done
            } else {
                std::fs::metadata(part_path(models_dir, f)).map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

pub fn delete(models_dir: &Path, v: &Variant) -> AppResult<()> {
    for f in &v.files {
        for p in [file_path(models_dir, f), part_path(models_dir, f)] {
            if p.exists() {
                std::fs::remove_file(p)?;
            }
        }
    }
    Ok(())
}

// ---------- fit & recommendations ----------

pub fn plan(model: &CatalogModel, v: &Variant, info: &SystemInfo, desired_ctx: u32) -> FitPlan {
    system::plan_fit(v.size_bytes, model.arch, desired_ctx, info.total_ram_bytes, info.gpu_budget_bytes)
}

/// Quality after quantization: very low-bit versions lose accuracy.
pub fn effective_quality(model: &CatalogModel, v: &Variant) -> i32 {
    let penalty = match v.bits {
        b if b < 2.5 => 22,
        b if b < 3.0 => 15,
        b if b < 3.5 => 9,
        b if b < 4.2 => 5,
        b if b < 5.0 => 2,
        b if b < 6.0 => 1,
        _ => 0,
    };
    model.quality as i32 - penalty
}

/// The best version of `model` for this Mac: highest quality that fits
/// comfortably, else highest that fits at all. Ties go to the smaller file
/// (faster, same quality).
pub fn best_variant<'a>(model: &'a CatalogModel, info: &SystemInfo, ctx: u32) -> Option<&'a Variant> {
    let pick = |want: Fit| {
        model
            .variants
            .iter()
            .filter(|v| plan(model, v, info, ctx).fit == want)
            .max_by(|a, b| {
                effective_quality(model, a)
                    .cmp(&effective_quality(model, b))
                    .then(b.size_bytes.cmp(&a.size_bytes))
            })
    };
    pick(Fit::Great).or_else(|| pick(Fit::Tight))
}

/// The recommended chat model + version for this Mac.
pub fn recommend<'a>(catalog: &'a Catalog, info: &SystemInfo, ctx: u32) -> Option<(&'a CatalogModel, &'a Variant)> {
    catalog
        .models
        .iter()
        .filter(|m| m.role == Role::Chat)
        .filter_map(|m| best_variant(m, info, ctx).map(|v| (m, v)))
        .max_by(|(ma, va), (mb, vb)| {
            let comfy = |m: &CatalogModel, v: &Variant| plan(m, v, info, ctx).fit == Fit::Great;
            comfy(ma, va)
                .cmp(&comfy(mb, vb))
                .then(effective_quality(ma, va).cmp(&effective_quality(mb, vb)))
                .then(vb.size_bytes.cmp(&va.size_bytes))
        })
}

// ---------- status for the UI ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantStatus {
    pub key: String,
    pub quant: String,
    pub bits: f32,
    pub size_bytes: u64,
    pub installed: bool,
    pub partial_bytes: u64,
    pub downloading: bool,
    pub quality: i32,
    pub fit: FitPlan,
    /// Smallest standard Mac memory size this version needs.
    pub min_ram_gb: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub family: Option<String>,
    pub released: Option<String>,
    pub tagline: String,
    pub tags: Vec<String>,
    pub thinking: bool,
    pub tools: bool,
    pub license: Option<String>,
    pub quality: u32,
    pub repo: String,
    pub role: Role,
    pub size_label: Option<String>,
    pub max_context: u32,
    pub variants: Vec<VariantStatus>,
    /// Best version for this Mac (None if nothing fits).
    pub best: Option<String>,
    /// Smallest Mac memory size that can run any version.
    pub min_ram_gb: u32,
}

pub struct ListContext<'a> {
    pub models_dir: &'a Path,
    pub info: &'a SystemInfo,
    pub ctx: u32,
    pub downloading: &'a [String],
}

pub fn list(catalog: &Catalog, lc: &ListContext<'_>) -> Vec<ModelStatus> {
    catalog
        .models
        .iter()
        .map(|m| {
            let variants: Vec<VariantStatus> = m
                .variants
                .iter()
                .map(|v| {
                    let fit = plan(m, v, lc.info, lc.ctx);
                    let min_plan = system::plan_fit(v.size_bytes, m.arch, 4096, u64::MAX / 4, u64::MAX / 4);
                    let installed = is_installed(lc.models_dir, v);
                    let k = key(m, v);
                    VariantStatus {
                        downloading: lc.downloading.contains(&k),
                        partial_bytes: if installed { 0 } else { bytes_on_disk(lc.models_dir, v) },
                        key: k,
                        quant: v.quant.clone(),
                        bits: v.bits,
                        size_bytes: v.size_bytes,
                        installed,
                        quality: effective_quality(m, v),
                        min_ram_gb: system::ram_tier_gb(min_plan.needed_bytes),
                        fit,
                    }
                })
                .collect();
            ModelStatus {
                best: best_variant(m, lc.info, lc.ctx).map(|v| key(m, v)),
                min_ram_gb: variants.iter().map(|v| v.min_ram_gb).min().unwrap_or(512),
                id: m.id.clone(),
                name: m.name.clone(),
                family: m.family.clone(),
                released: m.released.clone(),
                tagline: m.tagline.clone(),
                tags: m.tags.clone(),
                thinking: m.thinking,
                tools: m.tools,
                license: m.license.clone(),
                quality: m.quality,
                repo: m.repo.clone(),
                role: m.role,
                size_label: m.size_label.clone(),
                max_context: m.arch.max_ctx,
                variants,
            }
        })
        .collect()
}

// ---------- downloads ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum DownloadEvent {
    /// Hashing bytes already on disk from an earlier, interrupted download.
    Resuming { id: String, bytes: u64 },
    Progress { id: String, bytes: u64, total: u64, bytes_per_sec: f64 },
    Verifying { id: String },
    Finished { id: String },
    Paused { id: String, bytes: u64 },
    Failed { id: String, message: String },
}

pub const DOWNLOAD_EVENT: &str = "models://download";

/// Tracks in-flight downloads so they can be paused and never start twice.
#[derive(Default, Clone)]
pub struct Downloads {
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl Downloads {
    pub async fn active_ids(&self) -> Vec<String> {
        self.active.lock().await.keys().cloned().collect()
    }

    pub async fn pause(&self, key: &str) {
        if let Some(t) = self.active.lock().await.get(key) {
            t.cancel();
        }
    }

    /// Starts downloading every file of a variant in the background. Progress
    /// arrives as `models://download` events keyed by the model key.
    pub async fn start(
        &self,
        app: AppHandle,
        client: reqwest::Client,
        models_dir: PathBuf,
        repo: String,
        variant: Variant,
        key: String,
    ) -> AppResult<()> {
        let token = CancellationToken::new();
        {
            let mut active = self.active.lock().await;
            if active.contains_key(&key) {
                return Ok(());
            }
            active.insert(key.clone(), token.clone());
        }
        let active = self.active.clone();
        tauri::async_runtime::spawn(async move {
            let emit = |ev: DownloadEvent| {
                let _ = app.emit(DOWNLOAD_EVENT, ev);
            };
            let result = download_variant(&client, &models_dir, &repo, &variant, &key, &token, &emit, &hf_url).await;
            active.lock().await.remove(&key);
            match result {
                Ok(()) => emit(DownloadEvent::Finished { id: key }),
                Err(AppError::Cancelled) => emit(DownloadEvent::Paused { bytes: bytes_on_disk(&models_dir, &variant), id: key }),
                Err(e) => {
                    log::error!("download of {key} failed: {e}");
                    emit(DownloadEvent::Failed { id: key, message: e.to_string() })
                }
            }
        });
        Ok(())
    }
}

pub fn hf_url(repo: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{file}?download=true")
}

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// Extra free space to leave on disk beyond the download.
const DISK_MARGIN: u64 = 1_000_000_000;

/// Downloads all files of a variant, resuming and verifying each.
#[allow(clippy::too_many_arguments)]
pub async fn download_variant(
    client: &reqwest::Client,
    models_dir: &Path,
    repo: &str,
    v: &Variant,
    id: &str,
    cancel: &CancellationToken,
    emit: &(dyn Fn(DownloadEvent) + Sync),
    url_for: &(dyn Fn(&str, &str) -> String + Sync),
) -> AppResult<()> {
    let remaining = v.size_bytes.saturating_sub(bytes_on_disk(models_dir, v));
    let free = system::free_disk_for(models_dir);
    if remaining > 0 && free != 0 && free < remaining + DISK_MARGIN {
        return Err(AppError::msg(format!(
            "Not enough disk space: need {:.1} GB free, {:.1} GB available.",
            (remaining + DISK_MARGIN) as f64 / 1e9,
            free as f64 / 1e9
        )));
    }
    let mut done_before = 0u64;
    for f in &v.files {
        let progress = |bytes: u64, rate: f64| {
            emit(DownloadEvent::Progress { id: id.to_string(), bytes: done_before + bytes, total: v.size_bytes, bytes_per_sec: rate })
        };
        download_file(client, &url_for(repo, &f.name), models_dir, f, id, cancel, emit, &progress).await?;
        done_before += f.size;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn download_file(
    client: &reqwest::Client,
    url: &str,
    models_dir: &Path,
    f: &ModelFile,
    id: &str,
    cancel: &CancellationToken,
    emit: &(dyn Fn(DownloadEvent) + Sync),
    progress: &(dyn Fn(u64, f64) + Sync),
) -> AppResult<()> {
    let dest = file_path(models_dir, f);
    if std::fs::metadata(&dest).map(|m| m.len() == f.size).unwrap_or(false) {
        return Ok(());
    }
    let part = part_path(models_dir, f);
    let mut hasher = Sha256::new();
    let mut have: u64 = 0;

    if let Ok(meta) = tokio::fs::metadata(&part).await {
        if meta.len() > f.size {
            tokio::fs::remove_file(&part).await?;
        } else if meta.len() > 0 {
            emit(DownloadEvent::Resuming { id: id.to_string(), bytes: meta.len() });
            have = hash_existing(&part, &mut hasher, cancel).await?;
        }
    }

    let mut req = client.get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let resp = tokio::select! {
        r = req.send() => r?,
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
    };
    let status = resp.status();
    let mut file = if have > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
        tokio::fs::OpenOptions::new().append(true).open(&part).await?
    } else if status.is_success() {
        // Server ignored the range: start over.
        have = 0;
        hasher = Sha256::new();
        tokio::fs::File::create(&part).await?
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && have == f.size {
        return finish(&part, &dest, hasher, f, id, emit).await;
    } else {
        return Err(AppError::msg(format!("download failed: server returned {status}")));
    };

    let mut stream = resp.bytes_stream();
    let mut last_emit = Instant::now();
    let mut window_start = Instant::now();
    let mut window_bytes: u64 = 0;
    let mut rate: f64 = 0.0;
    loop {
        let chunk = tokio::select! {
            c = stream.next() => c,
            _ = cancel.cancelled() => {
                file.flush().await?;
                return Err(AppError::Cancelled);
            }
        };
        let Some(chunk) = chunk else { break };
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                // Keep what we have so the next attempt resumes from here.
                file.flush().await?;
                return Err(AppError::msg(format!(
                    "the connection was interrupted at {:.1} of {:.1} GB ({e}). Press Download to resume.",
                    have as f64 / 1e9,
                    f.size as f64 / 1e9
                )));
            }
        };
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        have += chunk.len() as u64;
        window_bytes += chunk.len() as u64;
        if have > f.size {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Err(AppError::msg("download is larger than expected; discarded"));
        }
        if last_emit.elapsed() >= PROGRESS_INTERVAL {
            let secs = window_start.elapsed().as_secs_f64();
            if secs >= 1.0 {
                let inst = window_bytes as f64 / secs;
                rate = if rate == 0.0 { inst } else { rate * 0.7 + inst * 0.3 };
                window_start = Instant::now();
                window_bytes = 0;
            }
            progress(have, rate);
            last_emit = Instant::now();
        }
    }
    file.flush().await?;
    drop(file);
    if have != f.size {
        return Err(AppError::msg(format!(
            "connection closed early ({:.1} of {:.1} GB). Press Download to resume.",
            have as f64 / 1e9,
            f.size as f64 / 1e9
        )));
    }
    finish(&part, &dest, hasher, f, id, emit).await
}

async fn finish(
    part: &Path,
    dest: &Path,
    hasher: Sha256,
    f: &ModelFile,
    id: &str,
    emit: &(dyn Fn(DownloadEvent) + Sync),
) -> AppResult<()> {
    emit(DownloadEvent::Verifying { id: id.to_string() });
    let digest = hex(&hasher.finalize());
    if digest != f.sha256 {
        let _ = tokio::fs::remove_file(part).await;
        return Err(AppError::msg("the downloaded file was corrupted (checksum mismatch) and was removed; please download again"));
    }
    tokio::fs::rename(part, dest).await?;
    Ok(())
}

async fn hash_existing(path: &Path, hasher: &mut Sha256, cancel: &CancellationToken) -> AppResult<u64> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    let mut total = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok(total)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn mac(ram_gib: u64) -> SystemInfo {
        let total = ram_gib * GIB;
        SystemInfo {
            chip: "Apple M4".into(),
            total_ram_bytes: total,
            gpu_budget_bytes: system::gpu_budget(total, None),
            free_disk_bytes: 500_000_000_000,
            os_version: String::new(),
            cpu_cores: 10,
            apple_silicon: true,
        }
    }

    #[test]
    fn embedded_catalog_is_valid_and_small() {
        let c = Catalog::embedded();
        assert!(c.models.len() >= 10);
        assert!(EMBEDDED_CATALOG.len() < 64 * 1024, "catalog should stay tiny");
        assert!(c.models.iter().any(|m| m.role == Role::Draft));
        assert!(c.models.iter().any(|m| m.role == Role::Embed));
        let mut ids: Vec<_> = c.models.iter().map(|m| &m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), c.models.len(), "duplicate ids");
        for m in &c.models {
            for v in &m.variants {
                assert_eq!(v.size_bytes, v.files.iter().map(|f| f.size).sum::<u64>(), "{} {}", m.id, v.quant);
            }
        }
    }

    #[test]
    fn resolves_keys_and_legacy_ids() {
        let c = Catalog::embedded();
        let (m, v) = c.resolve("qwen3.8-27b:UD-IQ3_XXS").unwrap();
        assert_eq!((m.id.as_str(), v.quant.as_str()), ("qwen3.8-27b", "UD-IQ3_XXS"));
        // Builds before the catalog stored "qwen3-14b" and downloaded Q4_K_M.
        let (_, v) = c.resolve("qwen3-14b").unwrap();
        assert_eq!(v.quant, "Q4_K_M");
        assert_eq!(v.files[0].name, "Qwen3-14B-Q4_K_M.gguf");
        assert!(c.resolve("nope").is_err());
        assert!(c.resolve("qwen3-14b:Q1").is_err());
    }

    #[test]
    fn recommendations_scale_with_memory() {
        let c = Catalog::embedded();
        let pick = |gb| recommend(&c, &mac(gb), 16384).map(|(m, v)| format!("{}:{}", m.id, v.quant));
        let r8 = pick(8).unwrap();
        let r16 = pick(16).unwrap();
        let r32 = pick(32).unwrap();
        let r128 = pick(128).unwrap();
        eprintln!("8GB → {r8}\n16GB → {r16}\n32GB → {r32}\n128GB → {r128}");
        let q = |k: &str| {
            let (m, v) = c.resolve(k).unwrap();
            effective_quality(m, v)
        };
        assert!(q(&r8) <= q(&r16) && q(&r16) <= q(&r32) && q(&r32) <= q(&r128));
        // Whatever is recommended must actually fit.
        for (gb, k) in [(8, &r8), (16, &r16), (32, &r32)] {
            let (m, v) = c.resolve(k).unwrap();
            assert_ne!(plan(m, v, &mac(gb), 16384).fit, Fit::TooBig, "{k} on {gb} GB");
        }
    }

    #[test]
    fn best_variant_prefers_quality_then_size() {
        let c = Catalog::embedded();
        let m = c.model("qwen3.5-9b").unwrap();
        // 64 GB fits everything; Q6_K and Q8_0 have equal quality, so the smaller Q6_K wins.
        assert_eq!(best_variant(m, &mac(64), 16384).unwrap().quant, "Q6_K");
        let big = c.model("gpt-oss-120b").unwrap();
        assert!(best_variant(big, &mac(16), 16384).is_none());
    }

    #[test]
    fn list_reports_fit_and_min_ram() {
        let c = Catalog::embedded();
        let dir = tempfile::tempdir().unwrap();
        let info = mac(16);
        let list = list(&c, &ListContext { models_dir: dir.path(), info: &info, ctx: 16384, downloading: &[] });
        let big = list.iter().find(|m| m.id == "gpt-oss-120b").unwrap();
        assert!(big.best.is_none());
        assert!(big.min_ram_gb >= 96, "{}", big.min_ram_gb);
        let tiny = list.iter().find(|m| m.id == "qwen3.5-0.8b").unwrap();
        assert_eq!(tiny.min_ram_gb, 8);
        assert!(tiny.best.is_some());
    }

    #[test]
    fn rejects_bad_catalogs() {
        assert!(Catalog::parse("{}").is_err());
        let mut c = Catalog::embedded();
        c.models[0].variants[0].files[0].name = "../../etc/passwd".into();
        assert!(Catalog::parse(&serde_json::to_string(&c).unwrap()).is_err());
        let mut c = Catalog::embedded();
        c.version = 99;
        assert!(Catalog::parse(&serde_json::to_string(&c).unwrap()).is_err());
    }

    #[test]
    fn store_prefers_newer_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("catalog.json");
        let mut newer = Catalog::embedded();
        newer.generated = "2999-01-01".into();
        newer.models.truncate(3);
        std::fs::write(&cache, serde_json::to_string(&newer).unwrap()).unwrap();
        assert_eq!(CatalogStore::load(cache.clone()).get().models.len(), 3);
        let mut older = Catalog::embedded();
        older.generated = "2000-01-01".into();
        std::fs::write(&cache, serde_json::to_string(&older).unwrap()).unwrap();
        assert_eq!(CatalogStore::load(cache).get().generated, Catalog::embedded().generated);
    }

    #[test]
    fn hex_encodes() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
    }

    // ---- downloader against a local test server ----

    /// Minimal HTTP/1.1 server that honours `Range: bytes=N-` and can cut the
    /// connection after `cut_after` bytes to simulate a dropped download.
    async fn serve(files: HashMap<String, Arc<Vec<u8>>>, cut_after: Option<usize>, ignore_range: bool) -> String {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let files = Arc::new(files);
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { return };
                let files = files.clone();
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    let mut start = 0usize;
                    let mut path = String::new();
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if path.is_empty() {
                            path = line.split_whitespace().nth(1).unwrap_or("").trim_start_matches('/').to_string();
                        }
                        let l = line.to_ascii_lowercase();
                        if let Some(v) = l.strip_prefix("range: bytes=") {
                            start = v.trim().trim_end_matches('-').parse().unwrap_or(0);
                        }
                    }
                    let Some(data) = files.get(&path) else {
                        let _ = w.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
                        return;
                    };
                    if ignore_range {
                        start = 0;
                    }
                    let body = &data[start..];
                    let status = if start > 0 { "206 Partial Content" } else { "200 OK" };
                    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = w.write_all(head.as_bytes()).await;
                    let n = cut_after.map(|c| c.min(body.len())).unwrap_or(body.len());
                    let _ = w.write_all(&body[..n]).await;
                    let _ = w.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn file_entry(name: &str, data: &[u8]) -> ModelFile {
        ModelFile { name: name.into(), size: data.len() as u64, sha256: hex(&Sha256::digest(data)) }
    }

    fn variant(files: Vec<ModelFile>) -> Variant {
        Variant { quant: "Q4".into(), bits: 4.0, size_bytes: files.iter().map(|f| f.size).sum(), files }
    }

    fn noop(_: DownloadEvent) {}

    async fn run(base: &str, dir: &Path, v: &Variant) -> AppResult<()> {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = base.to_string();
        let url = move |_repo: &str, file: &str| format!("{base}/{file}");
        download_variant(&client, dir, "repo", v, "t:Q4", &CancellationToken::new(), &noop, &url).await
    }

    #[tokio::test]
    async fn resumes_after_dropped_connection_and_verifies() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let v = variant(vec![file_entry("m.gguf", &data)]);
        let dir = tempfile::tempdir().unwrap();
        let files = HashMap::from([("m.gguf".to_string(), Arc::new(data.clone()))]);

        let err = run(&serve(files.clone(), Some(120_000), false).await, dir.path(), &v).await.unwrap_err();
        assert!(err.to_string().contains("resume"), "{err}");
        assert_eq!(bytes_on_disk(dir.path(), &v), 120_000);

        run(&serve(files, None, false).await, dir.path(), &v).await.unwrap();
        assert!(is_installed(dir.path(), &v));
        assert_eq!(std::fs::read(entry_path(dir.path(), &v)).unwrap(), data);
    }

    #[tokio::test]
    async fn downloads_multi_part_models() {
        let a: Vec<u8> = vec![1; 50_000];
        let b: Vec<u8> = vec![2; 30_000];
        let v = variant(vec![file_entry("big-00001-of-00002.gguf", &a), file_entry("sub/big-00002-of-00002.gguf", &b)]);
        let dir = tempfile::tempdir().unwrap();
        let files = HashMap::from([
            ("big-00001-of-00002.gguf".to_string(), Arc::new(a.clone())),
            ("sub/big-00002-of-00002.gguf".to_string(), Arc::new(b.clone())),
        ]);
        run(&serve(files, None, false).await, dir.path(), &v).await.unwrap();
        assert!(is_installed(dir.path(), &v));
        // Parts land side by side so llama-server finds them from the first.
        assert!(dir.path().join("big-00002-of-00002.gguf").exists());
        assert_eq!(entry_path(dir.path(), &v), dir.path().join("big-00001-of-00002.gguf"));
        delete(dir.path(), &v).unwrap();
        assert_eq!(bytes_on_disk(dir.path(), &v), 0);
    }

    #[tokio::test]
    async fn restarts_cleanly_when_server_ignores_range() {
        let data: Vec<u8> = (0..50_000u32).map(|i| (i % 13) as u8).collect();
        let f = file_entry("m.gguf", &data);
        let v = variant(vec![f.clone()]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(part_path(dir.path(), &f), &data[..10_000]).unwrap();
        let files = HashMap::from([("m.gguf".to_string(), Arc::new(data.clone()))]);
        run(&serve(files, None, true).await, dir.path(), &v).await.unwrap();
        assert_eq!(std::fs::read(entry_path(dir.path(), &v)).unwrap(), data);
    }

    #[tokio::test]
    async fn rejects_corrupted_download() {
        let good: Vec<u8> = vec![1; 40_000];
        let v = variant(vec![file_entry("m.gguf", &good)]);
        let mut bad = good.clone();
        bad[20_000] = 9;
        let dir = tempfile::tempdir().unwrap();
        let files = HashMap::from([("m.gguf".to_string(), Arc::new(bad))]);
        let err = run(&serve(files, None, false).await, dir.path(), &v).await.unwrap_err();
        assert!(err.to_string().contains("checksum"), "{err}");
        assert!(!is_installed(dir.path(), &v));
        assert_eq!(bytes_on_disk(dir.path(), &v), 0);
    }
}

/// Writes the UI's model list for a simulated Mac, for screenshots:
/// `BYTE_DUMP_MODELS=/tmp/m.json BYTE_DUMP_RAM_GB=16 cargo test dump_models_for_ui -- --ignored`
#[cfg(test)]
#[test]
#[ignore]
fn dump_models_for_ui() {
    let Ok(out) = std::env::var("BYTE_DUMP_MODELS") else { return };
    let gb: u64 = std::env::var("BYTE_DUMP_RAM_GB").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
    let total = gb << 30;
    let info = SystemInfo {
        chip: "Apple M4".into(),
        total_ram_bytes: total,
        gpu_budget_bytes: system::gpu_budget(total, None),
        free_disk_bytes: 212_000_000_000,
        os_version: "macOS 15".into(),
        cpu_cores: 10,
        apple_silicon: true,
    };
    let dir = tempfile::tempdir().unwrap();
    let c = Catalog::embedded();
    let list = list(&c, &ListContext { models_dir: dir.path(), info: &info, ctx: 16384, downloading: &[] });
    let rec = recommend(&c, &info, 16384).map(|(m, v)| key(m, v));
    std::fs::write(out, serde_json::to_string(&serde_json::json!({ "models": list, "recommend": rec, "system": info })).unwrap()).unwrap();
}
