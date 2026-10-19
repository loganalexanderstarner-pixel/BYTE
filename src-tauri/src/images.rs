//! Image Studio catalog (Windows builds only for now): the free image models, what each is good at,
//! and, for every quant, whether it fits this PC's video memory, spills into RAM, or won't fit. Downloading and generating come later; this is the
//! list the Models page shows.
//!
//! The data is `catalog/images.json`, built by `scripts/build-image-catalog.py`. Every file carries the size and SHA-256 published by its
//! Hugging Face repository, so a download can be verified like the chat models are.

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

const EMBEDDED: &str = include_str!("../catalog/images.json");
const GB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageFile {
    /// What the engine uses it as: "model", "clip_l", "clip_g", "t5xxl", "llm" or "vae".
    pub role: String,
    pub name: String,
    pub repo: String,
    pub size: u64,
    pub sha256: String,
    pub url: String,
}

/// One quantisation of a model: smaller files fit smaller cards and lose a little quality.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageVariant {
    pub quant: String,
    pub bits: f32,
    /// Quality points this quant loses against the best one (0 for 8-bit and 16-bit).
    pub quality_cost: u32,
    pub size_bytes: u64,
    pub files: Vec<ImageFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageModel {
    pub id: String,
    pub name: String,
    pub family: String,
    /// "generate" for now; "edit", "upscale" and "restore" are added with those features.
    pub kind: String,
    pub tags: Vec<String>,
    /// Real-life looks it does well (portrait, landscape ...); only models tagged "realism" have any.
    #[serde(default)]
    pub looks: Vec<String>,
    pub tagline: String,
    pub strengths: String,
    pub license: String,
    pub commercial_ok: bool,
    /// Made by the community rather than the lab that made the base model.
    pub community: bool,
    /// Adult or uncensored model. Hidden unless the owner turns community models on, and never shown in kids mode.
    #[serde(default)]
    pub mature: bool,
    /// 0-100, how good the pictures are at the best quant. Sorting uses it.
    pub quality: u32,
    pub steps: u32,
    /// Memory the text encoder and working space need on top of the weights, in GB.
    pub overhead_gb: f32,
    #[serde(default)]
    pub downloads: u64,
    pub source: String,
    pub variants: Vec<ImageVariant>,
    /// Text encoders and VAE every quant of this model needs.
    pub companions: Vec<ImageFile>,
}

/// Where a quant would live on this PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    /// All in video memory: fastest.
    Vram,
    /// Part in video memory, the rest in RAM: works, slower.
    Split,
    /// No usable graphics card: runs on the processor from RAM, slowest.
    Ram,
    /// More than video memory and RAM together.
    TooBig,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantFit {
    pub placement: Placement,
    /// Memory it needs while making a picture.
    pub need_bytes: u64,
    /// How much would spill out of video memory into RAM (Split), or fall short (TooBig).
    pub overflow_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageVariantStatus {
    #[serde(flatten)]
    pub variant: ImageVariant,
    pub fit: VariantFit,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageModelStatus {
    #[serde(flatten)]
    pub model: ImageModel,
    pub variants: Vec<ImageVariantStatus>,
    /// Index into `variants` of the version recommended for this PC.
    pub best: usize,
    /// Everything this model needs to download for its recommended version.
    pub total_bytes: u64,
}

/// What the PC offers: video memory (none without a separate graphics card) and RAM.
#[derive(Debug, Clone, Copy)]
pub struct Memory {
    pub vram: Option<u64>,
    pub ram: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ImageCatalog {
    version: u32,
    models: Vec<ImageModel>,
}

fn parse(json: &str) -> AppResult<Vec<ImageModel>> {
    let c: ImageCatalog = serde_json::from_str(json)?;
    if c.version != 2 {
        return Err(crate::error::AppError::msg(format!("unsupported image catalog version {}", c.version)));
    }
    let bad_file = |f: &ImageFile| f.sha256.len() != 64 || f.size == 0 || !f.url.starts_with("https://");
    for m in &c.models {
        if m.variants.is_empty()
            || m.variants.iter().any(|v| v.files.iter().filter(|f| f.role == "model").count() != 1 || v.files.iter().any(bad_file))
            || m.companions.iter().any(bad_file)
        {
            return Err(crate::error::AppError::msg(format!("image catalog entry {} is invalid", m.id)));
        }
    }
    Ok(c.models)
}

/// Whether `v` of `m` fits `mem`, and how much would spill into RAM.
pub fn fit(m: &ImageModel, v: &ImageVariant, mem: Memory) -> VariantFit {
    let need = v.size_bytes + (m.overhead_gb * GB as f32) as u64;
    // Keep some RAM for Windows and the app.
    let ram = mem.ram.saturating_sub(4 * GB);
    let vram = mem.vram.unwrap_or(0).saturating_sub(GB / 2);
    let (placement, overflow_bytes) = if mem.vram.is_some() && need <= vram {
        (Placement::Vram, 0)
    } else if mem.vram.is_some() && need <= vram + ram {
        (Placement::Split, need - vram)
    } else if mem.vram.is_none() && need <= ram {
        (Placement::Ram, 0)
    } else {
        (Placement::TooBig, need - (vram + ram))
    };
    VariantFit { placement, need_bytes: need, overflow_bytes }
}

/// The version to suggest: the best quality that fits video memory, else the smallest that fits
/// with the least spill, else the smallest.
fn best_variant(vs: &[ImageVariantStatus]) -> usize {
    let quality = |v: &ImageVariantStatus| 100 - v.variant.quality_cost as i32;
    if let Some((i, _)) = vs.iter().enumerate().filter(|(_, v)| v.fit.placement == Placement::Vram).max_by_key(|(_, v)| quality(v)) {
        return i;
    }
    vs.iter()
        .enumerate()
        .filter(|(_, v)| matches!(v.fit.placement, Placement::Split | Placement::Ram))
        .min_by_key(|(_, v)| v.fit.overflow_bytes + v.variant.size_bytes / 4)
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Every image model with how each quant fits `mem`.
pub fn list(mem: Memory, installed: &dyn Fn(&ImageModel, &ImageVariant) -> bool) -> AppResult<Vec<ImageModelStatus>> {
    Ok(parse(EMBEDDED)?
        .into_iter()
        .map(|m| {
            let mut vs: Vec<ImageVariantStatus> =
                m.variants.iter().map(|v| ImageVariantStatus { fit: fit(&m, v, mem), installed: installed(&m, v), variant: v.clone() }).collect();
            vs.sort_by_key(|v| v.variant.size_bytes);
            let best = best_variant(&vs);
            let total_bytes = vs[best].variant.size_bytes + m.companions.iter().map(|f| f.size).sum::<u64>();
            ImageModelStatus { variants: vs, best, total_bytes, model: m }
        })
        .collect())
}

/// The image models for the Models page. Windows only until the feature is ready for other platforms.
#[tauri::command]
pub async fn image_catalog(state: tauri::State<'_, crate::state::AppState>) -> AppResult<Vec<ImageModelStatus>> {
    if !cfg!(windows) {
        return Ok(Vec::new());
    }
    let info = crate::system::system_info(&state.paths.data);
    let vram = crate::gpu::best_discrete(crate::gpu::detect()).map(|g| g.dedicated_bytes);
    list(Memory { vram, ram: info.total_ram_bytes }, &|_, _| false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM16: Memory = Memory { vram: Some(16 * GB), ram: 32 * GB };

    #[test]
    fn embedded_catalog_is_valid() {
        let l = list(MEM16, &|_, _| false).unwrap();
        assert!(l.len() >= 100, "{}", l.len());
        let mut ids: Vec<_> = l.iter().map(|m| m.model.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), l.len(), "ids are unique");
        assert!(l.iter().all(|m| !m.variants.is_empty()));
    }

    #[test]
    fn variants_are_ordered_small_to_large() {
        for m in list(MEM16, &|_, _| false).unwrap() {
            assert!(m.variants.windows(2).all(|w| w[0].variant.size_bytes <= w[1].variant.size_bytes), "{}", m.model.id);
        }
    }

    #[test]
    fn every_family_has_its_text_files() {
        for m in list(MEM16, &|_, _| false).unwrap() {
            let has = |r: &str| m.model.companions.iter().any(|f| f.role == r);
            match m.model.family.as_str() {
                "FLUX.1" => assert!(has("clip_l") && has("t5xxl") && has("vae"), "{}", m.model.id),
                "Stable Diffusion 3.5" => assert!(has("clip_l") && has("clip_g") && has("t5xxl"), "{}", m.model.id),
                "Z-Image" | "Qwen-Image 2.1" | "ERNIE-Image" => assert!(has("llm") && has("vae"), "{}", m.model.id),
                _ => {}
            }
        }
    }

    fn sample() -> (ImageModel, ImageVariant) {
        let m = list(MEM16, &|_, _| false).unwrap().into_iter().find(|m| m.model.id == "city96-flux-1-dev-gguf").expect("flux dev");
        let v = m.variants.last().unwrap().variant.clone(); // F16, about 24 GB
        (m.model, v)
    }

    #[test]
    fn says_when_a_quant_overflows_video_memory() {
        let (m, v) = sample();
        let f = fit(&m, &v, MEM16);
        assert_eq!(f.placement, Placement::Split);
        assert!(f.overflow_bytes > 8 * GB);
        // With enough RAM it still fits; with little it does not.
        assert_eq!(fit(&m, &v, Memory { vram: Some(8 * GB), ram: 64 * GB }).placement, Placement::Split);
        assert_eq!(fit(&m, &v, Memory { vram: Some(8 * GB), ram: 16 * GB }).placement, Placement::TooBig);
        // A small quant fits video memory entirely.
        let small = &m.variants[0];
        assert_eq!(fit(&m, small, MEM16).placement, Placement::Vram);
        assert_eq!(fit(&m, small, MEM16).overflow_bytes, 0);
    }

    #[test]
    fn no_graphics_card_runs_from_ram() {
        let (m, _) = sample();
        let small = &m.variants[0];
        assert_eq!(fit(&m, small, Memory { vram: None, ram: 32 * GB }).placement, Placement::Ram);
        assert_eq!(fit(&m, small, Memory { vram: None, ram: 8 * GB }).placement, Placement::TooBig);
    }

    #[test]
    fn recommends_the_best_quant_that_fits_video_memory() {
        let l = list(MEM16, &|_, _| false).unwrap();
        let f = l.iter().find(|m| m.model.id == "city96-flux-1-dev-gguf").unwrap();
        let b = &f.variants[f.best];
        assert_eq!(b.fit.placement, Placement::Vram);
        assert!(f.variants.iter().filter(|v| v.fit.placement == Placement::Vram).all(|v| v.variant.quality_cost >= b.variant.quality_cost));
        // On a tiny card nothing fits video memory, but something still runs.
        let l = list(Memory { vram: Some(4 * GB), ram: 32 * GB }, &|_, _| false).unwrap();
        let f = l.iter().find(|m| m.model.id == "city96-flux-1-dev-gguf").unwrap();
        assert_eq!(f.variants[f.best].fit.placement, Placement::Split);
    }

    #[test]
    fn realism_models_say_which_real_life_looks_they_do() {
        let l = list(MEM16, &|_, _| false).unwrap();
        for m in l.iter().filter(|m| m.model.tags.iter().any(|t| t == "realism")) {
            assert!(!m.model.looks.is_empty(), "{}", m.model.id);
        }
        assert!(l.iter().filter(|m| m.model.looks.iter().any(|k| k == "portrait")).count() > 20);
    }

    #[test]
    fn rejects_bad_entries() {
        assert!(parse(r#"{"version":1,"models":[]}"#).is_err());
        let bad = EMBEDDED.replacen("\"sha256\": \"", "\"sha256\": \"x", 1);
        assert!(parse(&bad).is_err());
    }
}
