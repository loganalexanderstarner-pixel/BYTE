//! Model lab: add any GGUF model to BYTE, from a file on this Mac or a Hugging Face
//! link. BYTE reads the file's header (gguf.rs), checks it against this Mac's memory
//! with the same planner as the catalog, and lists it as a model like any other
//! (`CatalogStore::set_added`). Added models are kept in `<data>/added_models.json`;
//! a file on this Mac is linked into the models folder, not copied.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::gguf::{self, ReadError};
use crate::models::{CatalogModel, ModelFile, Role, Variant};
use crate::state::AppState;
use crate::system::{Fit, SystemInfo};

/// Header bytes read first, then at most this much (tokenizers make headers big).
const FIRST_READ: usize = 8 * 1024 * 1024;
const MAX_READ: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LabModel {
    pub id: String,
    pub name: String,
    /// "file" or "huggingface".
    pub source: String,
    pub path: String,
    pub repo: String,
    pub file: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub architecture: String,
    pub params_b: Option<f32>,
    pub quant: String,
    pub layers: u32,
    pub context_max: u32,
    pub thinking: bool,
    /// "great", "tight" or "no".
    pub fit: String,
    pub fit_note: String,
    pub context: u32,
    pub added: bool,
    /// The planner's numbers from the header.
    pub kv_layers: u32,
    pub n_head_kv: u32,
    pub head_dim: u32,
}

impl LabModel {
    pub fn key(&self) -> String {
        format!("{}:{}", self.id, self.quant)
    }

    fn arch(&self) -> crate::system::ModelArch {
        crate::system::ModelArch { n_layer: self.layers, kv_layers: self.kv_layers, n_head_kv: self.n_head_kv, head_dim: self.head_dim, max_ctx: self.context_max }
    }

    /// The model as a catalog entry (never recommended; tagged "added").
    pub fn to_catalog(&self) -> CatalogModel {
        // Files are stored by their file name (a Hugging Face path can have folders).
        let file = if self.source == "huggingface" { self.file.clone() } else { file_name(&self.file) };
        let entry = serde_json::json!({
            "id": self.id,
            "name": self.name,
            "repo": if self.source == "huggingface" { self.repo.as_str() } else { "local" },
            "role": "chat",
            "tags": ["added"],
            "thinking": self.thinking,
            "paramsB": self.params_b,
            "tagline": format!("Added by you · {}", if self.source == "huggingface" { self.repo.clone() } else { crate::platform_text::here("a file on this Mac") }),
            "arch": self.arch(),
            "variants": [Variant {
                quant: self.quant.clone(),
                bits: gguf::bits_of(&self.quant) as f32,
                size_bytes: self.size_bytes,
                files: vec![ModelFile { name: file, size: self.size_bytes, sha256: self.sha256.clone() }],
            }],
        });
        let mut m: CatalogModel = serde_json::from_value(entry).expect("a valid catalog entry");
        m.role = Role::Chat;
        m
    }
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "model.gguf".into())
}

/// "Qwen3-8B-Q4_K_M.gguf" → "qwen3-8b-q4-k-m": lowercase letters, digits and dashes.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(48).collect()
}

/// The model from its header, checked against this Mac.
pub fn from_header(meta: &gguf::Meta, source: &str, file: &str, size: u64, info: &SystemInfo, want_ctx: u32) -> LabModel {
    let arch = meta.arch();
    let stem = file.trim_end_matches(".gguf");
    let quant = meta.quant().map(str::to_string).unwrap_or_else(|| stem.rsplit(['-', '.']).next().unwrap_or("custom").to_uppercase());
    let prefix = if source == "huggingface" { "hf" } else { "local" };
    let mut m = LabModel {
        id: format!("{prefix}-{}", slug(stem)),
        name: meta.name().unwrap_or_else(|| stem.to_string()),
        source: source.into(),
        file: file.to_string(),
        size_bytes: size,
        architecture: meta.architecture(),
        params_b: meta.params_b(size),
        quant: slug(&quant).replace('-', "_").to_uppercase(),
        layers: arch.n_layer,
        context_max: arch.max_ctx,
        thinking: meta.thinks(),
        kv_layers: arch.kv_layers,
        n_head_kv: arch.n_head_kv,
        head_dim: arch.head_dim,
        ..Default::default()
    };
    // The same memory check as the catalog's models.
    let cm = m.to_catalog();
    let p = crate::models::plan(&cm, &cm.variants[0], info, want_ctx);
    m.fit = match p.fit {
        Fit::Great => "great",
        Fit::Tight => "tight",
        Fit::TooBig => "no",
    }
    .into();
    m.fit_note = p.note.clone();
    m.context = p.context;
    m
}

/// Reads a file's header (8 MB first, more if the header is bigger).
fn read_file_header(path: &Path) -> AppResult<gguf::Meta> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| AppError::msg(format!("Couldn't open that file: {e}")))?;
    let mut buf = Vec::new();
    let mut want = FIRST_READ;
    loop {
        let have = buf.len();
        (&mut f).take((want - have) as u64).read_to_end(&mut buf)?;
        match gguf::parse(&buf) {
            Ok(m) => return Ok(m),
            Err(ReadError::Short) if buf.len() == want && want < MAX_READ => want = MAX_READ,
            Err(ReadError::Short) => return Err(AppError::msg("That file ends too early to be a whole model.")),
            Err(ReadError::Bad(e)) => return Err(AppError::msg(format!("That isn't a model BYTE can use: {e}."))),
        }
    }
}

/// "https://huggingface.co/<owner>/<repo>/(resolve|blob)/<rev>/<path>.gguf" → (repo, path).
pub fn hf_parts(url: &str) -> Option<(String, String)> {
    let u = url::Url::parse(url.trim()).ok()?;
    if !matches!(u.host_str()?, "huggingface.co" | "www.huggingface.co" | "hf.co") {
        return None;
    }
    let segs: Vec<&str> = u.path_segments()?.collect();
    if segs.len() < 5 || !matches!(segs[2], "resolve" | "blob") || segs[3] != "main" {
        return None;
    }
    let file = segs[4..].join("/");
    file.to_lowercase().ends_with(".gguf").then(|| (format!("{}/{}", segs[0], segs[1]), file))
}

async fn inspect_hf(net: &reqwest::Client, url: &str, info: &SystemInfo, ctx: u32) -> AppResult<LabModel> {
    let (repo, file) = hf_parts(url).ok_or_else(|| {
        AppError::msg("Paste a link to one .gguf file on Hugging Face, like https://huggingface.co/owner/repo/blob/main/model-Q4_K_M.gguf")
    })?;
    let resolve = format!("https://huggingface.co/{repo}/resolve/main/{file}");
    // Size and checksum without downloading: the redirect's headers carry them.
    let head_client = crate::offline::client_builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(20)).build()?;
    let head = head_client.head(&resolve).send().await?;
    if head.status() == 404 {
        return Err(AppError::msg("That file isn't on Hugging Face (or the repo is private)."));
    }
    let h = |k: &str| head.headers().get(k).and_then(|v| v.to_str().ok()).map(|s| s.trim_matches('"').to_string());
    let size: u64 = h("x-linked-size").and_then(|s| s.parse().ok()).ok_or_else(|| AppError::msg("Hugging Face didn't say how big that file is."))?;
    let sha = h("x-linked-etag").filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())).ok_or_else(|| AppError::msg("Hugging Face didn't give that file's checksum."))?;
    let mut want = FIRST_READ;
    let meta = loop {
        let bytes = net.get(&resolve).header("Range", format!("bytes=0-{}", want - 1)).timeout(Duration::from_secs(120)).send().await?.error_for_status()?.bytes().await?;
        match gguf::parse(&bytes) {
            Ok(m) => break m,
            Err(ReadError::Short) if want < MAX_READ => want = MAX_READ,
            Err(ReadError::Short) => return Err(AppError::msg("That model's header is too big to read.")),
            Err(ReadError::Bad(e)) => return Err(AppError::msg(format!("That isn't a model BYTE can use: {e}."))),
        }
    };
    let name = file.rsplit('/').next().unwrap_or(&file).to_string();
    let mut m = from_header(&meta, "huggingface", &name, size, info, ctx);
    m.repo = repo;
    m.file = file;
    m.sha256 = sha;
    Ok(m)
}

fn store_path(state: &AppState) -> PathBuf {
    state.paths.data.join("added_models.json")
}

pub fn load(path: &Path) -> Vec<LabModel> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(state: &AppState, list: &[LabModel]) -> AppResult<()> {
    std::fs::write(store_path(state), serde_json::to_string_pretty(list)?)?;
    state.catalog.set_added(list.iter().map(LabModel::to_catalog).collect());
    Ok(())
}

async fn context_pref(state: &AppState) -> (SystemInfo, u32) {
    let settings = state.settings.lock().await.clone();
    let info = crate::system::system_info(&state.paths.data).with_settings(&settings);
    (info, settings.context_size.unwrap_or(crate::engine::DEFAULT_CONTEXT))
}

#[tauri::command]
pub async fn lab_inspect(state: State<'_, AppState>, path: String) -> AppResult<LabModel> {
    let p = PathBuf::from(&path);
    let size = std::fs::metadata(&p).map_err(|e| AppError::msg(format!("Couldn't open that file: {e}")))?.len();
    let meta = tokio::task::spawn_blocking(move || read_file_header(&p)).await.map_err(|e| AppError::msg(e.to_string()))??;
    let (info, ctx) = context_pref(&state).await;
    let mut m = from_header(&meta, "file", &file_name(&path), size, &info, ctx);
    m.path = path;
    m.added = load(&store_path(&state)).iter().any(|x| x.id == m.id);
    Ok(m)
}

#[tauri::command]
pub async fn lab_inspect_url(state: State<'_, AppState>, url: String) -> AppResult<LabModel> {
    let (info, ctx) = context_pref(&state).await;
    let mut m = inspect_hf(&state.net, &url, &info, ctx).await?;
    m.added = load(&store_path(&state)).iter().any(|x| x.id == m.id);
    Ok(m)
}

#[tauri::command]
pub fn lab_list(state: State<'_, AppState>) -> Vec<LabModel> {
    load(&store_path(&state))
}

/// Adds the model to the list (a file on this Mac is linked into the models folder).
/// Returns its key; a Hugging Face model is then downloaded like any other.
#[tauri::command]
pub fn lab_add(state: State<'_, AppState>, model: LabModel) -> AppResult<String> {
    if model.fit == "no" {
        return Err(AppError::msg(crate::platform_text::here("That model is too big for this Mac.")));
    }
    if model.id.is_empty() || model.quant.is_empty() {
        return Err(AppError::msg("Check the model first."));
    }
    if state.catalog.get().models.iter().any(|m| m.id == model.id && !m.tags.iter().any(|t| t == "added")) {
        return Err(AppError::msg("That model is already in BYTE's list."));
    }
    let mut m = model;
    if m.source == "file" {
        let src = PathBuf::from(&m.path);
        if !src.is_file() {
            return Err(AppError::msg("That file isn't there any more."));
        }
        let link = state.paths.models.join(file_name(&m.path));
        if !link.exists() {
            link_file(&src, &link)?;
        }
        // Not downloaded, so no checksum: a placeholder keeps the entry valid.
        m.sha256 = "0".repeat(64);
    }
    m.added = true;
    let mut list: Vec<LabModel> = load(&store_path(&state)).into_iter().filter(|x| x.id != m.id).collect();
    list.push(m.clone());
    save(&state, &list)?;
    Ok(m.key())
}

#[cfg(unix)]
fn link_file(src: &Path, link: &Path) -> AppResult<()> {
    std::os::unix::fs::symlink(src, link).map_err(|e| AppError::msg(format!("Couldn't add that file: {e}")))
}

#[cfg(not(unix))]
fn link_file(src: &Path, link: &Path) -> AppResult<()> {
    std::fs::hard_link(src, link).or_else(|_| std::fs::copy(src, link).map(|_| ())).map_err(|e| AppError::msg(format!("Couldn't add that file: {e}")))
}

/// Removes an added model (its link, or its downloaded file).
#[tauri::command]
pub fn lab_remove(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let list = load(&store_path(&state));
    if let Some(m) = list.iter().find(|m| m.id == id) {
        let file = state.paths.models.join(if m.source == "file" { file_name(&m.path) } else { file_name(&m.file) });
        let is_link = std::fs::symlink_metadata(&file).map(|md| md.file_type().is_symlink()).unwrap_or(false);
        if m.source == "huggingface" || is_link {
            let _ = std::fs::remove_file(&file);
        }
    }
    save(&state, &list.into_iter().filter(|m| m.id != id).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::{test_header, Val};

    fn meta() -> gguf::Meta {
        gguf::parse(&test_header(&[
            ("general.architecture", Val::Str("llama".into())),
            ("general.name", Val::Str("My Llama".into())),
            ("general.file_type", Val::Int(15)),
            ("llama.block_count", Val::Int(32)),
            ("llama.context_length", Val::Int(131072)),
            ("llama.embedding_length", Val::Int(4096)),
            ("llama.attention.head_count", Val::Int(32)),
            ("llama.attention.head_count_kv", Val::Int(8)),
        ]))
        .unwrap()
    }

    fn mac(gb: u64) -> SystemInfo {
        let total = gb * 1024 * 1024 * 1024;
        let dir = tempfile::tempdir().unwrap();
        let mut i = crate::system::system_info(dir.path());
        i.total_ram_bytes = total;
        i.gpu_budget_bytes = crate::system::gpu_budget(total, None);
        i
    }

    #[test]
    fn a_header_becomes_a_checked_model() {
        let m = from_header(&meta(), "file", "My-Llama-8B-Q4_K_M.gguf", 4_900_000_000, &mac(16), 16384);
        assert_eq!((m.id.as_str(), m.name.as_str(), m.quant.as_str(), m.architecture.as_str()), ("local-my-llama-8b-q4-k-m", "My Llama", "Q4_K_M", "llama"));
        assert_eq!(m.fit, "great");
        assert!(m.context >= 8192 && m.params_b.is_some_and(|b| (7.0..9.0).contains(&b)), "{m:?}");
        let big = from_header(&meta(), "file", "huge.gguf", 40_000_000_000, &mac(16), 16384);
        assert_eq!(big.fit, "no");
        let c = m.to_catalog();
        assert!(c.tags.iter().any(|t| t == "added") && c.variants[0].files[0].name == "My-Llama-8B-Q4_K_M.gguf");
        assert_eq!(m.key(), "local-my-llama-8b-q4-k-m:Q4_K_M");
    }

    #[test]
    fn hugging_face_links_are_read() {
        assert_eq!(
            hf_parts("https://huggingface.co/unsloth/Qwen3-8B-GGUF/blob/main/Qwen3-8B-Q4_K_M.gguf"),
            Some(("unsloth/Qwen3-8B-GGUF".into(), "Qwen3-8B-Q4_K_M.gguf".into()))
        );
        assert_eq!(hf_parts("https://hf.co/a/b/resolve/main/sub/x.gguf?download=true").map(|p| p.1), Some("sub/x.gguf".into()));
        assert!(hf_parts("https://huggingface.co/a/b/blob/dev/x.gguf").is_none(), "only the main branch");
        assert!(hf_parts("https://huggingface.co/a/b").is_none());
        assert!(hf_parts("https://example.com/a/b/resolve/main/x.gguf").is_none());
    }

    /// Live: a Hugging Face link is read from its first bytes (with size and checksum).
    #[tokio::test]
    #[ignore]
    async fn live_hf_header() {
        let net = crate::tools::fetch::web_client();
        let m = inspect_hf(&net, "https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/blob/main/Qwen3-0.6B-Q4_K_M.gguf", &mac(16), 16384).await.unwrap();
        eprintln!("{m:?}");
        assert_eq!((m.architecture.as_str(), m.quant.as_str(), m.sha256.len()), ("qwen3", "Q4_K_M", 64));
        assert!(m.size_bytes > 300_000_000 && m.fit == "great" && m.id.starts_with("hf-"));
    }

    /// Real files: BYTE_TEST_MODEL's header reads as the right model.
    #[test]
    #[ignore]
    fn e2e_reads_a_real_gguf() {
        let Ok(path) = std::env::var("BYTE_TEST_MODEL") else { return };
        let meta = read_file_header(Path::new(&path)).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        let m = from_header(&meta, "file", &file_name(&path), size, &mac(16), 16384);
        eprintln!("{m:?}");
        assert!(m.layers > 0 && m.context_max >= 2048 && !m.quant.is_empty() && m.fit != "no");
    }
}
