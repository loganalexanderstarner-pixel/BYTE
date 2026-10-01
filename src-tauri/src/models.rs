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
    /// Total parameters in billions.
    #[serde(default)]
    pub params_b: Option<f32>,
    /// Parameters used per token (mixture-of-experts models only), billions.
    #[serde(default)]
    pub active_b: Option<f32>,
    /// What the model is good for, in plain words.
    #[serde(default)]
    pub used_for: Option<String>,
    pub arch: ModelArch,
    pub variants: Vec<Variant>,
    /// A speed-up head published with the model (see `Helper`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_head: Option<SpeedHead>,
    /// The image adapter of a model that can see photos (`--mmproj`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<Vision>,
    /// Details for the model's dropdown (scripts/enrich-catalog.mjs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ModelDetails>,
}

impl CatalogModel {
    /// A community fine-tune or merge (not from the model's original maker).
    pub fn is_community(&self) -> bool {
        self.tags.iter().any(|t| t == "community") || self.details.as_ref().is_some_and(|d| d.community)
    }
}

/// What the model list shows when a model is opened: a longer description
/// from its model card, who made it, how strong it is at different things
/// (BYTE's estimate from size, family and card), and ideas for using it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelDetails {
    #[serde(default)]
    pub about: String,
    /// Who made the model (the original creator, not the uploader of the GGUF files).
    #[serde(default)]
    pub author: Option<String>,
    /// The original model's page.
    #[serde(default)]
    pub source_url: Option<String>,
    /// Chat, writing, coding, reasoning, math, languages, speed: 1–5 each.
    #[serde(default)]
    pub strengths: std::collections::BTreeMap<String, u8>,
    #[serde(default)]
    pub ideas: Vec<String>,
    /// A community fine-tune or merge (not from the model's original maker).
    #[serde(default)]
    pub community: bool,
    /// Plain notes shown with community models ("fewer refusals: no safety tuning").
    #[serde(default)]
    pub caution: Option<String>,
}

/// How a Speed boost helper guesses ahead (llama.cpp `--spec-type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HelperKind {
    /// A separate small model from the same family.
    #[default]
    Draft,
    /// Multi-token prediction layers trained with the model itself.
    Mtp,
    /// An EAGLE-3 head that reads the model's hidden states.
    Eagle3,
    /// A DSpark head that drafts a whole block at once.
    Dspark,
}

impl HelperKind {
    pub fn spec_type(self) -> &'static str {
        match self {
            HelperKind::Draft => "draft-simple",
            HelperKind::Mtp => "draft-mtp",
            HelperKind::Eagle3 => "draft-eagle3",
            HelperKind::Dspark => "draft-dspark",
        }
    }

    /// How many tokens to guess per step when not tuned. Heads are accurate
    /// for a few tokens; a separate model can run further ahead.
    pub fn default_lookahead(self) -> u32 {
        match self {
            HelperKind::Draft => 16,
            HelperKind::Mtp => 3,
            HelperKind::Eagle3 => 8,
            HelperKind::Dspark => 7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeedHead {
    pub kind: HelperKind,
    pub file: ModelFile,
}

/// A model's image adapter ("mmproj"): lets it see photos.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Vision {
    pub file: ModelFile,
}

/// Version name used in download keys for a model's image adapter
/// (`"gemma-4-12b:vision"`).
pub const VISION_QUANT: &str = "vision";

fn vision_variant(v: &Vision) -> Variant {
    Variant { quant: VISION_QUANT.into(), bits: 16.0, size_bytes: v.file.size, files: vec![v.file.clone()] }
}

/// Folder for downloads of `key`. Image adapters get one folder per model:
/// many repos name theirs just `mmproj-F16.gguf`, and models are stored by
/// file name.
pub fn download_dir(models_dir: &Path, key: &str) -> PathBuf {
    match key.strip_suffix(&format!(":{VISION_QUANT}")) {
        Some(id) => models_dir.join("vision").join(id.replace(['/', '\\', '.'], "_")),
        None => models_dir.to_path_buf(),
    }
}

/// Where `model`'s image adapter lives once downloaded (None if it has none
/// or it isn't downloaded).
pub fn vision_path(models_dir: &Path, model: &CatalogModel) -> Option<PathBuf> {
    let v = vision_variant(model.vision.as_ref()?);
    let dir = download_dir(models_dir, &format!("{}:{VISION_QUANT}", model.id));
    is_installed(&dir, &v).then(|| entry_path(&dir, &v))
}

/// Version name used in download keys for a model's speed-up head
/// (`"gemma-4-12b:speed-head"`).
pub const HEAD_QUANT: &str = "speed-head";

/// The Speed boost helper for a model: its own speed-up head when it ships
/// one (more accurate, smaller), else a small model from the same family.
#[derive(Debug, Clone, PartialEq)]
pub struct Helper {
    pub kind: HelperKind,
    /// Download key.
    pub key: String,
    pub name: String,
    pub repo: String,
    pub variant: Variant,
}

impl Helper {
    pub fn path(&self, models_dir: &Path) -> PathBuf {
        entry_path(models_dir, &self.variant)
    }

    pub fn installed(&self, models_dir: &Path) -> bool {
        is_installed(models_dir, &self.variant)
    }
}

fn head_variant(h: &SpeedHead) -> Variant {
    Variant { quant: HEAD_QUANT.into(), bits: 8.0, size_bytes: h.file.size, files: vec![h.file.clone()] }
}

pub fn helper_for(catalog: &Catalog, model: &CatalogModel, models_dir: &Path) -> Option<Helper> {
    if let Some(h) = &model.speed_head {
        let name = match h.kind {
            HelperKind::Mtp => "built-in multi-token head",
            HelperKind::Eagle3 => "EAGLE-3 head",
            HelperKind::Dspark => "DSpark head",
            HelperKind::Draft => "helper",
        };
        return Some(Helper {
            kind: h.kind,
            key: format!("{}:{HEAD_QUANT}", model.id),
            name: format!("{}'s {name}", model.name),
            repo: model.repo.clone(),
            variant: head_variant(h),
        });
    }
    let d = drafter_for(catalog, model)?;
    let (v, _) = drafter_variant(d, models_dir);
    Some(Helper { kind: HelperKind::Draft, key: key(d, v), name: d.name.clone(), repo: d.repo.clone(), variant: v.clone() })
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
            let extras = m.speed_head.as_ref().map(|h| &h.file).into_iter().chain(m.vision.as_ref().map(|v| &v.file));
            for f in m.variants.iter().flat_map(|v| &v.files).chain(extras) {
                if f.sha256.len() != 64 || f.name.contains("..") || f.name.starts_with('/') {
                    return Err(AppError::msg(format!("catalog entry {} has an invalid file", m.id)));
                }
            }
        }
        Ok(())
    }

    /// What to download for a key: a model version, or a model's speed-up
    /// head (`"<id>:speed-head"`), or its image adapter (`"<id>:vision"`).
    /// Returns (repo, files, canonical key).
    pub fn download_target(&self, key: &str) -> AppResult<(String, Variant, String)> {
        if let Some(id) = key.strip_suffix(&format!(":{HEAD_QUANT}")) {
            let model = self.model(id).ok_or_else(|| AppError::msg(format!("unknown model '{id}'")))?;
            let head = model.speed_head.as_ref().ok_or_else(|| AppError::msg(format!("{} has no speed-up head", model.name)))?;
            return Ok((model.repo.clone(), head_variant(head), key.to_string()));
        }
        if let Some(id) = key.strip_suffix(&format!(":{VISION_QUANT}")) {
            let model = self.model(id).ok_or_else(|| AppError::msg(format!("unknown model '{id}'")))?;
            let v = model.vision.as_ref().ok_or_else(|| AppError::msg(format!("{} can't see images", model.name)))?;
            return Ok((model.repo.clone(), vision_variant(v), key.to_string()));
        }
        let (model, variant) = self.resolve(key)?;
        Ok((model.repo.clone(), variant.clone(), self::key(model, variant)))
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
    /// The embedded or fetched catalog.
    base: RwLock<Arc<Catalog>>,
    /// `base` plus the models the user added (lab.rs); what `get` returns.
    current: RwLock<Arc<Catalog>>,
    added: RwLock<Vec<CatalogModel>>,
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
        let best = Arc::new(best);
        CatalogStore { base: RwLock::new(best.clone()), current: RwLock::new(best), added: RwLock::new(Vec::new()), cache_file }
    }

    /// Sets the models the user added (model lab) and rebuilds the merged list.
    pub fn set_added(&self, models: Vec<CatalogModel>) {
        *self.added.write().expect("catalog lock") = models;
        self.rebuild();
    }

    fn rebuild(&self) {
        let base = self.base.read().expect("catalog lock").clone();
        let added = self.added.read().expect("catalog lock").clone();
        let merged = if added.is_empty() {
            base
        } else {
            let mut c = (*base).clone();
            c.models.retain(|m| !added.iter().any(|a| a.id == m.id));
            c.models.extend(added);
            Arc::new(c)
        };
        *self.current.write().expect("catalog lock") = merged;
    }

    pub fn get(&self) -> Arc<Catalog> {
        self.current.read().expect("catalog lock").clone()
    }

    /// Fetches the catalog from `url`; keeps it if valid and newer.
    pub async fn refresh(&self, client: &reqwest::Client, url: &str) -> AppResult<bool> {
        let text = client.get(url).timeout(Duration::from_secs(15)).send().await?.error_for_status()?.text().await?;
        let fresh = Catalog::parse(&text)?;
        if fresh.generated <= self.base.read().expect("catalog lock").generated {
            return Ok(false);
        }
        let _ = std::fs::write(&self.cache_file, &text);
        *self.base.write().expect("catalog lock") = Arc::new(fresh);
        self.rebuild();
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
    let p = system::plan_fit(v.size_bytes, model.arch, desired_ctx, info.total_ram_bytes, info.gpu_budget_bytes);
    if p.fit != Fit::TooBig {
        return p;
    }
    system::plan_offload(v.size_bytes, model.arch, desired_ctx, info.total_ram_bytes, info.gpu_budget_bytes, expert_share(model)).unwrap_or(p)
}

/// Speed factor when part of the model runs on the CPU: expert layers cost
/// little (few experts per token), dense layers on the CPU cost more.
pub fn offload_slowdown(model: &CatalogModel, p: &FitPlan) -> f64 {
    let layers = model.arch.n_layer.max(1) as f64;
    if p.cpu_moe_layers > 0 {
        1.0 / (1.0 + 0.8 * p.cpu_moe_layers as f64 / layers)
    } else if let Some(on) = p.gpu_layers {
        1.0 / (1.0 + 2.0 * (layers - on as f64).max(0.0) / layers)
    } else {
        1.0
    }
}

/// Rough share of a mixture-of-experts model's weights that are experts
/// (everything not used by every token). 0 for dense models.
pub fn expert_share(model: &CatalogModel) -> f64 {
    match (model.params_b, model.active_b) {
        (Some(t), Some(a)) if t > 0.0 && a > 0.0 && a < t => (1.0 - a as f64 / t as f64).clamp(0.0, 0.95),
        _ => 0.0,
    }
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

/// Quality adjusted for speed on this Mac: answers slower than a target feel
/// sluggish, so slower versions rank lower. The target depends on what the
/// user prefers: ~8 tokens/sec by default, 22 for "faster", 5 for "smarter".
pub fn score(model: &CatalogModel, v: &Variant, info: &SystemInfo) -> i32 {
    use crate::settings::SpeedPref;
    let tps = expected_tps(model, v, info);
    let (target, per_token, cap) = match info.speed_pref {
        SpeedPref::Speed => (22.0, 3.0, 40.0),
        SpeedPref::Balanced => (8.0, 2.5, 20.0),
        SpeedPref::Quality => (5.0, 2.0, 12.0),
    };
    let penalty = if tps >= target { 0.0 } else { ((target - tps) * per_token).min(cap) };
    effective_quality(model, v) - penalty.round() as i32
}

/// Writing speed to plan with: measured by tuning on this Mac when available,
/// else estimated from the chip (a bit higher when Speed boost has a helper
/// for this model; measured boosts were 1.3–2× on code and lists).
pub fn expected_tps(model: &CatalogModel, v: &Variant, info: &SystemInfo) -> f64 {
    if let Some(&m) = info.measured.get(&key(model, v)) {
        return m;
    }
    let raw = |v: &Variant| crate::chip::estimate(&info.chip_info, v.size_bytes, model.params_b, model.active_b).tokens_per_sec;
    // Another version of this model was measured: scale by how far off the
    // estimate was for it (same architecture, same Mac).
    if let Some((other, m)) = model.variants.iter().find_map(|o| info.measured.get(&key(model, o)).map(|m| (o, *m))) {
        return raw(v) * m / raw(other).max(0.1);
    }
    let est = raw(v) * info.calibration.unwrap_or(1.0);
    if info.boost && has_helper(model) {
        est * 1.3
    } else {
        est
    }
}

/// Learns from tuning how far this Mac's real speed is from the estimates
/// (thermals, other apps, the engine build) and applies it to every model.
pub fn calibrate(mut info: SystemInfo, catalog: &Catalog) -> SystemInfo {
    let mut ratios: Vec<f64> = info
        .measured
        .iter()
        .filter_map(|(k, m)| {
            let (model, v) = catalog.resolve(k).ok()?;
            let est = crate::chip::estimate(&info.chip_info, v.size_bytes, model.params_b, model.active_b).tokens_per_sec;
            (est > 0.0).then(|| m / est)
        })
        .collect();
    ratios.sort_by(f64::total_cmp);
    info.calibration = ratios.get(ratios.len() / 2).map(|r| r.clamp(0.2, 3.0));
    info
}

/// Whether Speed boost has a helper for `model` (its own head or a family drafter).
pub fn has_helper(model: &CatalogModel) -> bool {
    model.speed_head.is_some() || (drafter_id(model).is_some() && model.params_b.unwrap_or(0.0) >= 3.2)
}

fn drafter_id(model: &CatalogModel) -> Option<&'static str> {
    let m = model.id.as_str();
    let starts = |p: &[&str]| p.iter().any(|x| m.starts_with(x));
    if starts(&["qwen3.5", "qwen3.6", "qwen3.8-27b", "qwen-agentworld"]) {
        Some("qwen3.5-0.8b")
    } else if starts(&["qwen3-"]) {
        Some("qwen3-0.6b")
    } else if starts(&["gemma-3-"]) {
        Some("gemma-3-270m")
    } else if starts(&["llama-3", "meta-llama-3", "hermes-3-llama-3"]) {
        Some("llama-3.2-1b")
    } else {
        None
    }
}

/// A small model from the same family that can draft tokens for `model`
/// (speculative decoding). It must share the tokenizer and be at most a
/// quarter of the size, or it wouldn't save time.
pub fn drafter_for<'a>(catalog: &'a Catalog, model: &CatalogModel) -> Option<&'a CatalogModel> {
    let d = catalog.model(drafter_id(model)?)?;
    let big = model.params_b.unwrap_or(0.0);
    let small = d.params_b.unwrap_or(f32::MAX);
    (d.id != model.id && small * 4.0 <= big).then_some(d)
}

/// The drafter version to use: the best one already downloaded, else the
/// one to offer (Q8_0 is most accurate; drafters are tiny either way).
pub fn drafter_variant<'a>(d: &'a CatalogModel, models_dir: &Path) -> (&'a Variant, bool) {
    let rank = |v: &Variant| match v.quant.as_str() {
        "Q8_0" => 0,
        "Q4_K_M" => 1,
        _ => 2,
    };
    let mut vs: Vec<&Variant> = d.variants.iter().collect();
    vs.sort_by_key(|v| rank(v));
    match vs.iter().find(|v| is_installed(models_dir, v)) {
        Some(v) => (v, true),
        None => (vs[0], false),
    }
}

/// Orders two choices of equal score: the faster one, then the smaller file.
fn faster(a: (&CatalogModel, &Variant), b: (&CatalogModel, &Variant), info: &SystemInfo) -> std::cmp::Ordering {
    expected_tps(a.0, a.1, info)
        .total_cmp(&expected_tps(b.0, b.1, info))
        .then(b.1.size_bytes.cmp(&a.1.size_bytes))
}

/// The best version of `model` for this Mac: highest score that fits
/// comfortably, else highest that fits at all. Ties go to the faster one.
pub fn best_variant<'a>(model: &'a CatalogModel, info: &SystemInfo, ctx: u32) -> Option<&'a Variant> {
    let pick = |want: Fit| {
        model
            .variants
            .iter()
            // Stretch mode (dense layers on the CPU) is never suggested; people can still pick it.
            .filter(|v| {
                let p = plan(model, v, info, ctx);
                p.fit == want && p.gpu_layers.is_none()
            })
            .max_by(|a, b| score(model, a, info).cmp(&score(model, b, info)).then(faster((model, a), (model, b), info)))
    };
    pick(Fit::Great).or_else(|| pick(Fit::Tight))
}

/// How far below the most capable choice a recommendation may be for the
/// sake of speed, unless the user asked for faster answers.
const QUALITY_FLOOR: i32 = 8;

/// The recommended chat model + version for this Mac.
pub fn recommend<'a>(catalog: &'a Catalog, info: &SystemInfo, ctx: u32) -> Option<(&'a CatalogModel, &'a Variant)> {
    let comfy = |m: &CatalogModel, v: &Variant| plan(m, v, info, ctx).fit == Fit::Great;
    // Community fine-tunes are there for people who go looking; BYTE never picks one for them.
    let options: Vec<(&CatalogModel, &Variant)> = catalog
        .models
        .iter()
        .filter(|m| m.role == Role::Chat && !m.is_community() && !m.tags.iter().any(|t| t == "added"))
        .filter_map(|m| best_variant(m, info, ctx).map(|v| (m, v)))
        .collect();
    // Accuracy first: never trade more than a few quality points for speed
    // unless the user chose "Faster".
    let top = options.iter().filter(|(m, v)| comfy(m, v)).map(|(m, v)| effective_quality(m, v)).max();
    let floor = match (info.speed_pref, top) {
        (crate::settings::SpeedPref::Speed, _) | (_, None) => i32::MIN,
        (_, Some(t)) => t - QUALITY_FLOOR,
    };
    options
        .into_iter()
        .filter(|(m, v)| !comfy(m, v) || effective_quality(m, v) >= floor)
        .max_by(|&(ma, va), &(mb, vb)| {
            comfy(ma, va)
                .cmp(&comfy(mb, vb))
                .then(score(ma, va, info).cmp(&score(mb, vb, info)))
                .then(faster((ma, va), (mb, vb), info))
        })
}

/// A clearly better model this Mac runs comfortably, when the one in use is small:
/// the recommended model if it's at least twice the size (for the "a better model
/// fits" hint). `None` for big models, unknown ones, or when nothing better fits.
pub fn better_model<'a>(catalog: &'a Catalog, info: &SystemInfo, ctx: u32, current: &str) -> Option<&'a CatalogModel> {
    let (m, _) = catalog.resolve(current).ok()?;
    let now = m.params_b.filter(|b| *b <= crate::quality::SMALL_B)?;
    let (best, v) = recommend(catalog, info, ctx)?;
    (best.id != m.id && best.params_b.is_some_and(|b| b >= now * 2.0) && plan(best, v, info, ctx).fit == Fit::Great).then_some(best)
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
    /// Expected speed on this Mac's chip.
    pub speed: crate::chip::SpeedEstimate,
    /// Writing speed measured on this Mac by tuning (tokens/sec).
    pub measured_tps: Option<f64>,
    /// Fits in the memory left next to the models already running.
    pub fits_alongside: bool,
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
    pub params_b: Option<f32>,
    pub active_b: Option<f32>,
    pub used_for: Option<String>,
    pub max_context: u32,
    pub variants: Vec<VariantStatus>,
    /// Best version for this Mac (None if nothing fits).
    pub best: Option<String>,
    /// Smallest Mac memory size that can run any version.
    pub min_ram_gb: u32,
    pub details: Option<ModelDetails>,
    /// The model can see photos once its image adapter is downloaded.
    pub vision: Option<VisionStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionStatus {
    /// Download key (`"<id>:vision"`).
    pub key: String,
    pub size_bytes: u64,
    pub installed: bool,
    pub downloading: bool,
}

pub struct ListContext<'a> {
    pub models_dir: &'a Path,
    pub info: &'a SystemInfo,
    pub ctx: u32,
    pub downloading: &'a [String],
    /// Memory already used by running engines (for "load alongside").
    pub loaded_bytes: u64,
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
                    let alongside = plan(m, v, &lc.info.clone().minus(lc.loaded_bytes), crate::engine::EXTRA_CONTEXT);
                    VariantStatus {
                        measured_tps: lc.info.measured.get(&k).copied(),
                        fits_alongside: lc.loaded_bytes > 0 && alongside.fit != system::Fit::TooBig,
                        downloading: lc.downloading.contains(&k),
                        partial_bytes: if installed { 0 } else { bytes_on_disk(lc.models_dir, v) },
                        key: k,
                        quant: v.quant.clone(),
                        bits: v.bits,
                        size_bytes: v.size_bytes,
                        installed,
                        quality: effective_quality(m, v),
                        min_ram_gb: system::ram_tier_gb(min_plan.needed_bytes),
                        speed: {
                            let mut e = crate::chip::estimate(&lc.info.chip_info, v.size_bytes, m.params_b, m.active_b);
                            let f = offload_slowdown(m, &fit);
                            e.tokens_per_sec *= f;
                            e.reply_secs /= f;
                            e.reply_thinking_secs /= f;
                            e
                        },
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
                params_b: m.params_b,
                active_b: m.active_b,
                used_for: m.used_for.clone(),
                max_context: m.arch.max_ctx,
                details: m.details.clone(),
                vision: m.vision.as_ref().map(|v| {
                    let key = format!("{}:{VISION_QUANT}", m.id);
                    VisionStatus {
                        installed: vision_path(lc.models_dir, m).is_some(),
                        downloading: lc.downloading.contains(&key),
                        size_bytes: v.file.size,
                        key,
                    }
                }),
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
    pub async fn start(&self, app: AppHandle, client: reqwest::Client, models_dir: PathBuf, repo: String, variant: Variant, key: String) -> AppResult<()> {
        self.start_from(app, client, models_dir, repo, variant, key, hf_url).await
    }

    /// Like `start`, with the address of each file from `url_for(repo, file)` (files not on Hugging Face).
    #[allow(clippy::too_many_arguments)]
    pub async fn start_from(
        &self,
        app: AppHandle,
        client: reqwest::Client,
        models_dir: PathBuf,
        repo: String,
        variant: Variant,
        key: String,
        url_for: fn(&str, &str) -> String,
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
            let result = download_variant(&client, &models_dir, &repo, &variant, &key, &token, &emit, &url_for).await;
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
            chip_info: crate::chip::identify("Apple M4", Some(10)),
            speed_pref: Default::default(),
            boost: false,
            measured: Default::default(),
            calibration: None,
        }
    }

    #[test]
    fn embedded_catalog_is_valid_and_small() {
        let c = Catalog::embedded();
        let chat: Vec<_> = c.models.iter().filter(|m| m.role == Role::Chat).collect();
        assert!(chat.len() >= 600, "catalog should offer 600+ chat models");
        assert!(EMBEDDED_CATALOG.len() < 2 * 1024 * 1024, "catalog should stay small (models download separately)");
        // Every chat model has details for its dropdown, most with a description from its card.
        assert!(chat.iter().all(|m| m.details.as_ref().is_some_and(|d| !d.about.is_empty() && d.strengths.len() == 7)));
        assert!(chat.iter().filter(|m| m.details.as_ref().is_some_and(|d| d.author.is_some())).count() * 10 >= chat.len() * 8);
        assert!(chat.iter().filter(|m| m.is_community()).count() >= 50, "community fine-tunes are listed");
        assert!(c.models.iter().filter(|m| m.active_b.is_some()).count() >= 20, "catalog should include MoE models");
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
        // A 16 GB Mac running a 0.6B model is told about a much better one; one running the recommended model isn't.
        let small = c.models.iter().find(|m| m.id == "qwen3-0.6b").map(|m| key(m, &m.variants[0])).unwrap();
        let better = better_model(&c, &mac(16), 16384, &small).expect("a better model fits 16 GB");
        assert!(better.params_b.unwrap() >= 1.2);
        let rec = recommend(&c, &mac(16), 16384).map(|(m, v)| key(m, v)).unwrap();
        assert!(better_model(&c, &mac(16), 16384, &rec).is_none());
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
        let list = list(&c, &ListContext { models_dir: dir.path(), info: &info, ctx: 16384, downloading: &[], loaded_bytes: 0 });
        let big = list.iter().find(|m| m.id == "gpt-oss-120b").unwrap();
        assert!(big.best.is_none());
        assert!(big.min_ram_gb >= 96, "{}", big.min_ram_gb);
        let tiny = list.iter().find(|m| m.id == "qwen3.5-0.8b").unwrap();
        assert_eq!(tiny.min_ram_gb, 8);
        assert!(tiny.best.is_some());
    }

    #[test]
    fn fits_alongside_accounts_for_loaded_models() {
        let c = Catalog::embedded();
        let dir = tempfile::tempdir().unwrap();
        let info = mac(16);
        let status = |loaded_bytes: u64, id: &str, quant: &str| {
            let l = list(&c, &ListContext { models_dir: dir.path(), info: &info, ctx: 16384, downloading: &[], loaded_bytes });
            let m = l.into_iter().find(|m| m.id == id).unwrap();
            m.variants.into_iter().find(|v| v.quant == quant).unwrap().fits_alongside
        };
        // Nothing loaded: "alongside" doesn't apply.
        assert!(!status(0, "qwen3.5-0.8b", "Q8_0"));
        // A 9B Q6_K (~8.5 GB planned) leaves room for a small model on 16 GB, not for another 9B.
        let nine = 8_500_000_000;
        assert!(status(nine, "qwen3.5-0.8b", "Q8_0"));
        assert!(!status(nine, "qwen3.5-9b", "Q6_K"));
    }

    #[test]
    fn drafters_come_from_the_same_family_and_are_small() {
        let c = Catalog::embedded();
        let d = |id: &str| drafter_for(&c, c.model(id).unwrap()).map(|m| m.id.clone());
        assert_eq!(d("qwen3.5-9b").as_deref(), Some("qwen3.5-0.8b"));
        assert_eq!(d("qwen3.8-27b").as_deref(), Some("qwen3.5-0.8b"));
        assert_eq!(d("gemma-3-27b").as_deref(), Some("gemma-3-270m"));
        assert_eq!(d("llama-3.1-8b").as_deref(), Some("llama-3.2-1b"));
        // Too close in size to help, or no same-family helper.
        assert_eq!(d("qwen3.5-2b"), None);
        assert_eq!(d("qwen3.5-0.8b"), None);
        assert_eq!(d("gpt-oss-20b"), None);
        let dir = tempfile::tempdir().unwrap();
        let (v, installed) = drafter_variant(c.model("qwen3.5-0.8b").unwrap(), dir.path());
        assert_eq!((v.quant.as_str(), installed), ("Q8_0", false));
    }

    #[test]
    fn preference_changes_the_pick_on_a_16gb_m4() {
        use crate::settings::SpeedPref;
        let c = Catalog::embedded();
        let pick = |pref| recommend(&c, &mac(16).with_pref(pref), 16384).map(|(m, v)| format!("{}:{}", m.id, v.quant)).unwrap();
        let balanced = pick(SpeedPref::Balanced);
        let fast = pick(SpeedPref::Speed);
        let smart = pick(SpeedPref::Quality);
        let speed = |k: &str| {
            let (m, v) = c.resolve(k).unwrap();
            crate::chip::estimate(&mac(16).chip_info, v.size_bytes, m.params_b, m.active_b).tokens_per_sec
        };
        eprintln!("balanced {balanced} ({:.0} tok/s), fast {fast} ({:.0}), smart {smart} ({:.0})", speed(&balanced), speed(&fast), speed(&smart));
        assert!(speed(&fast) > speed(&balanced) * 1.3, "faster pick should be clearly faster");
        let q = |k: &str| {
            let (m, v) = c.resolve(k).unwrap();
            effective_quality(m, v)
        };
        assert!(q(&smart) >= q(&balanced));
    }

    #[test]
    fn measured_speed_beats_the_estimate() {
        let c = Catalog::embedded();
        let pick = |info: &SystemInfo| recommend(&c, info, 16384).map(|(m, v)| key(m, v)).unwrap();
        assert_eq!(pick(&mac(16)), "qwen3.5-9b:Q6_K");
        // Tuning found the 6-bit version slow on this Mac: the 4-bit one wins.
        let mut slow = mac(16);
        slow.measured.insert("qwen3.5-9b:Q6_K".into(), 6.5);
        let slow = calibrate(slow, &c);
        assert!(slow.calibration.is_some_and(|r| r < 0.6));
        assert_eq!(pick(&slow), "qwen3.5-9b:Q4_K_M");
        // Estimates for other models are corrected by the same factor.
        let (m, v) = c.resolve("qwen3.5-4b:Q6_K").unwrap();
        assert!(expected_tps(m, v, &slow) < expected_tps(m, v, &mac(16)) * 0.6);
        // Speed boost raises estimates only for models that have a helper.
        let mut boosted = mac(16);
        boosted.boost = true;
        let (m, v) = c.resolve("qwen3.5-9b:Q6_K").unwrap();
        assert!(expected_tps(m, v, &boosted) > expected_tps(m, v, &mac(16)));
        let (m, _) = c.resolve("gpt-oss-20b:MXFP4").unwrap();
        assert!(has_helper(m), "gpt-oss ships an EAGLE-3 head");
    }

    #[test]
    fn equal_quality_goes_to_the_faster_model() {
        let c = Catalog::embedded();
        let mut big = mac(128);
        big.chip_info = crate::chip::identify("Apple M4 Max", Some(40));
        let (m, v) = recommend(&c, &big, 16384).unwrap();
        let best_q = c
            .models
            .iter()
            .filter(|m| m.role == Role::Chat)
            .filter_map(|m| best_variant(m, &big, 16384).map(|v| effective_quality(m, v)))
            .max()
            .unwrap();
        assert!(effective_quality(m, v) >= best_q - QUALITY_FLOOR);
        assert!(expected_tps(m, v, &big) > 50.0, "{} is too slow", key(m, v));
    }

    #[test]
    fn image_adapters_download_into_their_own_folder() {
        let c = Catalog::embedded();
        let m = c.models.iter().find(|m| m.vision.is_some()).expect("a model that sees images");
        let key = format!("{}:{VISION_QUANT}", m.id);
        let (repo, v, k) = c.download_target(&key).unwrap();
        assert_eq!((repo.as_str(), k.as_str()), (m.repo.as_str(), key.as_str()));
        assert!(v.files[0].name.to_lowercase().contains("mmproj"));
        let root = Path::new("/models");
        assert_eq!(download_dir(root, &key), root.join("vision").join(m.id.replace(['/', '\\', '.'], "_")));
        assert_eq!(download_dir(root, "qwen3-14b:Q4_K_M"), root);
        // Not downloaded yet.
        assert!(vision_path(Path::new("/nowhere"), m).is_none());
        let no = c.models.iter().find(|m| m.vision.is_none() && m.role == Role::Chat).unwrap();
        assert!(c.download_target(&format!("{}:{VISION_QUANT}", no.id)).is_err());
        assert!(c.models.iter().filter(|m| m.vision.is_some()).count() >= 50);
    }

    #[test]
    fn speed_heads_download_by_key() {
        let c = Catalog::embedded();
        let (repo, v, k) = c.download_target("gemma-4-12b:speed-head").unwrap();
        assert_eq!((repo.as_str(), k.as_str()), ("unsloth/gemma-4-12b-it-GGUF", "gemma-4-12b:speed-head"));
        assert!(v.files[0].name.starts_with("mtp-") && v.size_bytes < 1_000_000_000);
        assert!(c.download_target("qwen3.5-9b:speed-head").is_err());
        assert_eq!(c.download_target("qwen3.5-9b:Q6_K").unwrap().2, "qwen3.5-9b:Q6_K");
        let dir = tempfile::tempdir().unwrap();
        let h = helper_for(&c, c.model("gemma-4-12b").unwrap(), dir.path()).unwrap();
        assert_eq!((h.kind, h.key.as_str()), (HelperKind::Mtp, "gemma-4-12b:speed-head"));
        assert!(!h.installed(dir.path()));
        // Without a head, the family drafter is used.
        assert_eq!(helper_for(&c, c.model("qwen3.5-9b").unwrap(), dir.path()).unwrap().kind, HelperKind::Draft);
    }

    #[test]
    fn big_moe_models_run_partly_on_the_cpu() {
        let c = Catalog::embedded();
        let m16 = mac(16);
        // gpt-oss 20B (12.1 GB) is over a 16 GB Mac's GPU share but fits in RAM:
        // a few expert layers go to the CPU.
        let (m, v) = c.resolve("gpt-oss-20b:MXFP4").unwrap();
        let p = plan(m, v, &m16, 16384);
        assert_eq!(p.fit, Fit::Tight);
        assert!(p.cpu_moe_layers > 0 && p.cpu_moe_layers < m.arch.n_layer / 2 && p.gpu_layers.is_none(), "{p:?}");
        assert!(offload_slowdown(m, &p) > 0.6, "{p:?} {}", offload_slowdown(m, &p));
        // Dense models slightly too big run in stretch mode but are never recommended.
        let stretched: Vec<_> = c
            .models
            .iter()
            .flat_map(|m| m.variants.iter().map(move |v| (m, v)))
            .filter(|(m, v)| plan(m, v, &m16, 16384).gpu_layers.is_some_and(|n| n < m.arch.n_layer))
            .collect();
        assert!(!stretched.is_empty());
        for (m, v) in stretched {
            assert_ne!(best_variant(m, &m16, 16384).map(|b| b.quant.as_str()), Some(v.quant.as_str()), "{}", m.id);
        }
        // Honest about memory: macOS needs its share too (these got killed while loading on a 16 GB Mac).
        for k in ["qwen3.6-35b-a3b:UD-IQ3_XXS", "qwen3.8-27b:UD-IQ3_XXS"] {
            let (m, v) = c.resolve(k).unwrap();
            assert_eq!(plan(m, v, &m16, 16384).fit, Fit::TooBig, "{k}");
        }
        let (m, v) = c.resolve("qwen3.6-35b-a3b:UD-IQ2_M").unwrap();
        let p = plan(m, v, &m16, 16384);
        assert!(p.fit == Fit::Tight && p.cpu_moe_layers > 0, "{p:?}");
        assert!(!recommend(&c, &m16, 16384).map(|(m, v)| plan(m, v, &m16, 16384).offloaded()).unwrap());
        // Far too big stays too big.
        let (m, v) = c.resolve("qwen3.8-27b:Q8_0").unwrap();
        assert_eq!(plan(m, v, &m16, 16384).fit, Fit::TooBig);
    }

    #[test]
    fn community_models_are_never_recommended() {
        let mut c = Catalog::embedded();
        let best = recommend(&c, &mac(16), 16384).map(|(m, _)| m.id.clone()).unwrap();
        // Make the recommended model a community one: something else must be picked.
        c.models.iter_mut().find(|m| m.id == best).unwrap().tags.push("community".into());
        let now = recommend(&c, &mac(16), 16384).map(|(m, _)| m.id.clone()).unwrap();
        assert_ne!(now, best);
        assert!(!c.models.iter().find(|m| m.id == now).unwrap().is_community());
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
        speed_pref: Default::default(),
        boost: false,
        measured: Default::default(),
        calibration: None,
        chip_info: crate::chip::identify(
            &std::env::var("BYTE_DUMP_CHIP").unwrap_or_else(|_| {
                match gb {
                    0..=16 => "Apple M4",
                    17..=48 => "Apple M4 Pro",
                    49..=128 => "Apple M4 Max",
                    _ => "Apple M3 Ultra",
                }
                .into()
            }),
            Some(40),
        ),
    };
    let dir = tempfile::tempdir().unwrap();
    let c = Catalog::embedded();
    let list = list(&c, &ListContext { models_dir: dir.path(), info: &info, ctx: 16384, downloading: &[], loaded_bytes: 0 });
    let rec = recommend(&c, &info, 16384).map(|(m, v)| key(m, v));
    std::fs::write(out, serde_json::to_string(&serde_json::json!({ "models": list, "recommend": rec, "system": info })).unwrap()).unwrap();
}
