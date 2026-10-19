//! Image Studio (Windows): downloads image models and makes pictures with the bundled
//! stable-diffusion.cpp engine (`sd-cli`). Everything runs on this PC.
//!
//! The catalog is `images.rs`. This file does the work: verified downloads (the chat-model downloader,
//! one file at a time), building the engine's command line for each model family, running it with
//! progress, the optional second detail pass, enlarging, and the camera-style finish (`finish.rs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::images::{self, ImageFile, ImageModel, ImageVariant};
use crate::models::{DownloadEvent, ModelFile};
use crate::state::AppState;

pub const DOWNLOAD_EVENT: &str = "images://download";
pub const PROGRESS_EVENT: &str = "images://progress";

static ACTIVE: Lazy<Mutex<HashMap<String, CancellationToken>>> = Lazy::new(|| Mutex::new(HashMap::new()));

// ---------- where files live ----------

pub fn images_root(models_dir: &Path) -> PathBuf {
    models_dir.join("images")
}

/// Every file sits in a folder named for its repository, so two repositories can never clash.
pub fn dir_for(models_dir: &Path, f: &ImageFile) -> PathBuf {
    images_root(models_dir).join("files").join(f.repo.replace('/', "__"))
}

fn model_file(f: &ImageFile) -> ModelFile {
    ModelFile { name: f.name.rsplit('/').next().unwrap_or(&f.name).to_string(), size: f.size, sha256: f.sha256.clone() }
}

pub fn path_of(models_dir: &Path, f: &ImageFile) -> PathBuf {
    dir_for(models_dir, f).join(&model_file(f).name)
}

fn is_complete(models_dir: &Path, f: &ImageFile) -> bool {
    std::fs::metadata(path_of(models_dir, f)).map(|m| m.len() == f.size).unwrap_or(false)
}

/// Everything a version needs on disk: its own file plus the model's text encoders and VAE.
pub fn needed<'a>(m: &'a ImageModel, v: &'a ImageVariant) -> Vec<&'a ImageFile> {
    v.files.iter().chain(m.companions.iter()).collect()
}

pub fn installed(models_dir: &Path, m: &ImageModel, v: &ImageVariant) -> bool {
    needed(m, v).into_iter().all(|f| is_complete(models_dir, f))
}

fn bytes_present(models_dir: &Path, m: &ImageModel, v: &ImageVariant) -> u64 {
    needed(m, v)
        .into_iter()
        .map(|f| {
            if is_complete(models_dir, f) {
                f.size
            } else {
                let mut part = path_of(models_dir, f).into_os_string();
                part.push(".part");
                std::fs::metadata(PathBuf::from(part)).map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

fn variant_of<'a>(m: &'a ImageModel, quant: &str) -> AppResult<&'a ImageVariant> {
    m.variants.iter().find(|v| v.quant == quant).ok_or_else(|| AppError::msg("that version is not in the catalog"))
}

// ---------- downloads ----------

fn key(model: &str, quant: &str) -> String {
    format!("{model}:{quant}")
}

#[tauri::command]
pub async fn image_download(app: AppHandle, state: State<'_, AppState>, model: String, quant: String) -> AppResult<()> {
    crate::offline::guard()?;
    let m = images::get(&model).ok_or_else(|| AppError::msg("unknown image model"))?;
    let v = variant_of(m, &quant)?.clone();
    let k = key(&model, &quant);
    let token = CancellationToken::new();
    {
        let mut active = ACTIVE.lock().await;
        if active.contains_key(&k) {
            return Ok(());
        }
        active.insert(k.clone(), token.clone());
    }
    let models_dir = state.paths.models.clone();
    let client = state.net.clone();
    let m = m.clone();
    tauri::async_runtime::spawn(async move {
        let emit = |ev: DownloadEvent| {
            let _ = app.emit(DOWNLOAD_EVENT, ev);
        };
        let result = run_download(&client, &models_dir, &m, &v, &k, &token, &emit).await;
        ACTIVE.lock().await.remove(&k);
        match result {
            Ok(()) => emit(DownloadEvent::Finished { id: k }),
            Err(AppError::Cancelled) => emit(DownloadEvent::Paused { bytes: bytes_present(&models_dir, &m, &v), id: k }),
            Err(e) => {
                log::error!("image download {k} failed: {e}");
                emit(DownloadEvent::Failed { id: k, message: e.to_string() })
            }
        }
    });
    Ok(())
}

async fn run_download(
    client: &reqwest::Client,
    models_dir: &Path,
    m: &ImageModel,
    v: &ImageVariant,
    id: &str,
    cancel: &CancellationToken,
    emit: &(dyn Fn(DownloadEvent) + Sync),
) -> AppResult<()> {
    let files = needed(m, v);
    let total: u64 = files.iter().map(|f| f.size).sum();
    let remaining = total.saturating_sub(bytes_present(models_dir, m, v));
    let free = crate::system::free_disk_for(models_dir);
    if remaining > 0 && free != 0 && free < remaining + 1_000_000_000 {
        return Err(AppError::msg(format!(
            "Not enough free disk space: {:.1} GB needed, {:.1} GB free. Free up space or pick a smaller version.",
            (remaining + 1_000_000_000) as f64 / 1e9,
            free as f64 / 1e9
        )));
    }
    let mut done_before = 0u64;
    for f in files {
        let dir = dir_for(models_dir, f);
        tokio::fs::create_dir_all(&dir).await?;
        let progress = |bytes: u64, rate: f64| emit(DownloadEvent::Progress { id: id.to_string(), bytes: done_before + bytes, total, bytes_per_sec: rate });
        crate::models::download_file(client, &f.url, &dir, &model_file(f), id, cancel, emit, &progress).await?;
        done_before += f.size;
    }
    Ok(())
}

#[tauri::command]
pub async fn image_pause(model: String, quant: String) -> AppResult<()> {
    if let Some(t) = ACTIVE.lock().await.get(&key(&model, &quant)) {
        t.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn image_delete(state: State<'_, AppState>, model: String, quant: String) -> AppResult<()> {
    let m = images::get(&model).ok_or_else(|| AppError::msg("unknown image model"))?;
    let v = variant_of(m, &quant)?;
    // Only this version's own file: the text encoders may be shared with other models.
    for f in &v.files {
        let _ = std::fs::remove_file(path_of(&state.paths.models, f));
    }
    Ok(())
}

// ---------- the engine ----------

// ---------- the picture engine (downloaded on request) ----------

/// stable-diffusion.cpp release `master-975-95b3090`, checked by SHA-256 like every other download.
pub struct EngineZip {
    pub name: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

pub struct EngineSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub zips: &'static [EngineZip],
}

const REL: &str = "https://github.com/leejet/stable-diffusion.cpp/releases/download/master-975-95b3090/";

pub const ENGINES: &[EngineSpec] = &[
    EngineSpec {
        id: "cuda",
        label: "NVIDIA (CUDA)",
        note: "Fastest on NVIDIA graphics cards. Large download.",
        zips: &[
            EngineZip { name: "sd-master-95b3090-bin-win-cuda12-x64.zip", sha256: "b935f5902fbedefe6db0b68c8930ae3e7c0f3cc778bd5d501a3c7f7ee7336338", size: 343_630_973 },
            EngineZip { name: "cudart-sd-bin-win-cu12-x64.zip", sha256: "fe20366827d357c00797eebb58244dddab7fd9a348d70090c3871004c320f38d", size: 563_452_046 },
        ],
    },
    EngineSpec {
        id: "vulkan",
        label: "Any graphics card (Vulkan)",
        note: "Works on NVIDIA, AMD and Intel. Small download; a little slower than CUDA on NVIDIA.",
        zips: &[EngineZip { name: "sd-master-95b3090-bin-win-vulkan-x64.zip", sha256: "17398d07ba89c2fba21b8662db69ecdd917888373e4f82779b7599e97a6b4bd3", size: 29_371_165 }],
    },
    EngineSpec {
        id: "cpu",
        label: "Processor only",
        note: "For PCs without a usable graphics card. Slow.",
        zips: &[EngineZip { name: "sd-master-95b3090-bin-win-cpu-x64.zip", sha256: "945ed74a22a5c6abbae480201312e439f16ad029166dd94e58cef9a2ecb2dba8", size: 17_706_677 }],
    },
];

pub fn engine_dir(models_dir: &Path, id: &str) -> PathBuf {
    images_root(models_dir).join("engine").join(id)
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "sd-cli.exe"
    } else {
        "sd-cli"
    }
}

/// The installed engine kinds, best first.
pub fn installed_engines(models_dir: &Path) -> Vec<&'static str> {
    ["cuda", "vulkan", "cpu"].into_iter().filter(|id| engine_dir(models_dir, id).join(exe_name()).is_file()).collect()
}

/// The engine kind to suggest for this PC.
pub fn recommended_engine(gpus: &[crate::gpu::Gpu]) -> &'static str {
    match crate::gpu::best_discrete(gpus) {
        Some(g) if g.vendor == crate::gpu::Vendor::Nvidia => "cuda",
        Some(_) => "vulkan",
        None => "cpu",
    }
}

/// The engine program: `BYTE_SD_CLI` (for testing), else an installed engine, else one beside the app.
pub fn engine_path(_app: &AppHandle, models_dir: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("BYTE_SD_CLI") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(id) = installed_engines(models_dir).first() {
        return Some(engine_dir(models_dir, id).join(exe_name()));
    }
    std::env::current_exe().ok().and_then(|me| me.parent().map(|d| d.join(exe_name()))).filter(|p| p.is_file())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineOption {
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub size_bytes: u64,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    /// The engine in use ("" when none is installed).
    pub active: String,
    pub recommended: &'static str,
    pub options: Vec<EngineOption>,
}

#[tauri::command]
pub async fn image_engine(app: AppHandle, state: State<'_, AppState>) -> AppResult<EngineStatus> {
    let have = installed_engines(&state.paths.models);
    let active = if engine_path(&app, &state.paths.models).is_some() { have.first().copied().unwrap_or("custom").to_string() } else { String::new() };
    Ok(EngineStatus {
        active,
        recommended: recommended_engine(crate::gpu::detect()),
        options: ENGINES
            .iter()
            .map(|e| EngineOption { id: e.id, label: e.label, note: e.note, size_bytes: e.zips.iter().map(|z| z.size).sum(), installed: have.contains(&e.id) })
            .collect(),
    })
}

#[tauri::command]
pub async fn image_engine_install(app: AppHandle, state: State<'_, AppState>, kind: String) -> AppResult<()> {
    crate::offline::guard()?;
    let spec = ENGINES.iter().find(|e| e.id == kind).ok_or_else(|| AppError::msg("unknown engine"))?;
    let k = format!("engine:{kind}");
    let token = CancellationToken::new();
    {
        let mut active = ACTIVE.lock().await;
        if active.contains_key(&k) {
            return Ok(());
        }
        active.insert(k.clone(), token.clone());
    }
    let models_dir = state.paths.models.clone();
    let client = state.net.clone();
    tauri::async_runtime::spawn(async move {
        let emit = |ev: DownloadEvent| {
            let _ = app.emit(DOWNLOAD_EVENT, ev);
        };
        let result = install_engine(&client, &models_dir, spec, &k, &token, &emit).await;
        ACTIVE.lock().await.remove(&k);
        match result {
            Ok(()) => emit(DownloadEvent::Finished { id: k }),
            Err(AppError::Cancelled) => emit(DownloadEvent::Paused { bytes: 0, id: k }),
            Err(e) => {
                log::error!("picture engine install failed: {e}");
                emit(DownloadEvent::Failed { id: k, message: e.to_string() })
            }
        }
    });
    Ok(())
}

async fn install_engine(
    client: &reqwest::Client,
    models_dir: &Path,
    spec: &'static EngineSpec,
    id: &str,
    cancel: &CancellationToken,
    emit: &(dyn Fn(DownloadEvent) + Sync),
) -> AppResult<()> {
    let total: u64 = spec.zips.iter().map(|z| z.size).sum();
    let free = crate::system::free_disk_for(models_dir);
    if free != 0 && free < total * 2 + 500_000_000 {
        return Err(AppError::msg(format!("Not enough free disk space for the picture engine: about {:.1} GB needed.", (total * 2 + 500_000_000) as f64 / 1e9)));
    }
    let dl_dir = images_root(models_dir).join("engine").join("_download");
    tokio::fs::create_dir_all(&dl_dir).await?;
    let mut done_before = 0u64;
    for z in spec.zips {
        let mf = ModelFile { name: z.name.to_string(), size: z.size, sha256: z.sha256.to_string() };
        let progress = |bytes: u64, rate: f64| emit(DownloadEvent::Progress { id: id.to_string(), bytes: done_before + bytes, total, bytes_per_sec: rate });
        crate::models::download_file(client, &format!("{REL}{}", z.name), &dl_dir, &mf, id, cancel, emit, &progress).await?;
        done_before += z.size;
    }
    emit(DownloadEvent::Verifying { id: id.to_string() });
    let dest = engine_dir(models_dir, spec.id);
    let tmp = dest.with_extension("new");
    let _ = tokio::fs::remove_dir_all(&tmp).await;
    tokio::fs::create_dir_all(&tmp).await?;
    let zips: Vec<PathBuf> = spec.zips.iter().map(|z| dl_dir.join(z.name)).collect();
    let t2 = tmp.clone();
    tokio::task::spawn_blocking(move || -> AppResult<()> {
        for z in &zips {
            unzip_flat(z, &t2)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;
    if !tmp.join(exe_name()).is_file() {
        return Err(AppError::msg("the picture engine download did not contain the engine program"));
    }
    let _ = tokio::fs::remove_dir_all(&dest).await;
    tokio::fs::rename(&tmp, &dest).await?;
    for z in spec.zips {
        let _ = tokio::fs::remove_file(dl_dir.join(z.name)).await;
    }
    Ok(())
}

/// Extracts every file of a zip into `dir`, ignoring folders in the archive (so nothing can land outside `dir`).
pub fn unzip_flat(zip_path: &Path, dir: &Path) -> AppResult<()> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(zip_path)?).map_err(|e| AppError::msg(format!("the engine download is damaged: {e}")))?;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| AppError::msg(e.to_string()))?;
        if f.is_dir() {
            continue;
        }
        let Some(name) = Path::new(f.name()).file_name().map(|n| n.to_owned()) else { continue };
        let mut out = std::fs::File::create(dir.join(name))?;
        std::io::copy(&mut f, &mut out)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub model: String,
    pub quant: String,
    pub prompt: String,
    #[serde(default)]
    pub negative: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub steps: Option<u32>,
    #[serde(default)]
    pub seed: Option<i64>,
}

/// The settings that change how a picture is made.
#[derive(Debug, Clone)]
pub struct Options {
    pub placement: String,
    pub vram_bytes: Option<u64>,
    pub hires: Option<HiresPass>,
}

#[derive(Debug, Clone)]
pub struct HiresPass {
    /// Folder holding upscaler files and the name (without extension) of the one to use; Lanczos when none is installed.
    pub upscaler: Option<(PathBuf, String)>,
}

/// The files of a version, found on disk.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    pub main: PathBuf,
    pub vae: Option<PathBuf>,
    pub clip_l: Option<PathBuf>,
    pub clip_g: Option<PathBuf>,
    pub t5xxl: Option<PathBuf>,
    pub llm: Option<PathBuf>,
}

pub fn resolve(models_dir: &Path, m: &ImageModel, v: &ImageVariant) -> AppResult<Resolved> {
    if !installed(models_dir, m, v) {
        return Err(AppError::msg(format!("{} ({}) is not downloaded yet.", m.name, v.quant)));
    }
    let mut r = Resolved::default();
    for f in needed(m, v) {
        let p = path_of(models_dir, f);
        match f.role.as_str() {
            "model" => r.main = p,
            "vae" => r.vae = Some(p),
            "clip_l" => r.clip_l = Some(p),
            "clip_g" => r.clip_g = Some(p),
            "t5xxl" => r.t5xxl = Some(p),
            "llm" => r.llm = Some(p),
            _ => {}
        }
    }
    Ok(r)
}

/// A distilled model makes a picture in a few steps and wants no guidance.
fn distilled(m: &ImageModel) -> bool {
    let n = format!("{} {}", m.name, m.source).to_lowercase();
    ["turbo", "schnell", "lightning", "flash", "hyper", "viggle"].iter().any(|k| n.contains(k))
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// The engine's command line for one picture.
pub fn build_args(m: &ImageModel, r: &Resolved, req: &Request, seed: i64, opt: &Options, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let mut push = |x: &[&str]| a.extend(x.iter().map(|s| s.to_string()));
    let steps = req.steps.unwrap_or(m.steps).max(1);
    let dist = distilled(m);
    let gguf = r.main.extension().map(|e| e == "gguf").unwrap_or(false);

    match m.family.as_str() {
        "FLUX.1" => {
            push(&["--diffusion-model", &s(&r.main)]);
            push(&["--cfg-scale", "1.0", "--guidance", "3.5", "--sampling-method", "euler", "--clip-on-cpu"]);
        }
        "Chroma" => {
            push(&["--diffusion-model", &s(&r.main)]);
            push(&["--cfg-scale", "4.0", "--sampling-method", "euler", "--model-args", "chroma_use_dit_mask=false", "--clip-on-cpu"]);
        }
        "Z-Image" => {
            push(&["--diffusion-model", &s(&r.main)]);
            push(&["--cfg-scale", if dist { "1.0" } else { "5.0" }]);
        }
        "Qwen-Image 2.1" => {
            push(&["--diffusion-model", &s(&r.main)]);
            push(&["--cfg-scale", if dist { "1.0" } else { "2.5" }, "--sampling-method", "euler", "--flow-shift", "3"]);
        }
        "ERNIE-Image" => {
            push(&["--diffusion-model", &s(&r.main)]);
            push(&["--cfg-scale", if dist { "1.0" } else { "5.0" }]);
        }
        "Stable Diffusion 3.5" => {
            push(&[if gguf { "--diffusion-model" } else { "-m" }, &s(&r.main)]);
            push(&["--cfg-scale", "4.5", "--sampling-method", "euler", "--clip-on-cpu"]);
        }
        // Stable Diffusion XL and 1.5: one file with everything inside.
        _ => {
            push(&["-m", &s(&r.main)]);
            push(&["--cfg-scale", if dist { "1.0" } else { "7.0" }]);
        }
    }
    for (flag, p) in [("--vae", &r.vae), ("--clip_l", &r.clip_l), ("--clip_g", &r.clip_g), ("--t5xxl", &r.t5xxl), ("--llm", &r.llm)] {
        if let Some(p) = p {
            push(&[flag, &s(p)]);
        }
    }
    push(&["-p", &req.prompt]);
    if !req.negative.trim().is_empty() && !dist {
        push(&["-n", &req.negative]);
    }
    push(&["--steps", &steps.to_string(), "-W", &req.width.to_string(), "-H", &req.height.to_string(), "--seed", &seed.to_string()]);

    // Where the model lives. Auto: weights stay in video memory while they fit, else the rest spills into RAM.
    const GB: u64 = 1 << 30;
    match opt.placement.as_str() {
        "ram" => push(&["--offload-to-cpu", "--max-vram", "1"]),
        _ => {
            push(&["--offload-to-cpu"]);
            if let Some(v) = opt.vram_bytes {
                let weights = std::fs::metadata(&r.main).map(|m| m.len()).unwrap_or(0);
                // Keep about 3.5 GB for the desktop, BYTE's chat model and the picture itself.
                let budget = (v / GB).saturating_sub(4).max(2);
                if opt.placement != "vram" && weights / GB >= budget {
                    push(&["--max-vram", &budget.to_string()]);
                }
            }
        }
    }

    if let Some(h) = &opt.hires {
        push(&["--hires", "--hires-scale", "1.5", "--hires-denoising-strength", "0.35", "--hires-steps", &(steps / 2).max(4).to_string()]);
        match &h.upscaler {
            Some((dir, name)) => push(&["--hires-upscalers-dir", &s(dir), "--hires-upscaler", name]),
            None => push(&["--hires-upscaler", "Lanczos"]),
        }
    }
    push(&["-o", &s(out)]);
    a
}

pub fn upscale_args(model: &Path, input: &Path, out: &Path) -> Vec<String> {
    ["-M", "upscale", "--upscale-model", &s(model), "-i", &s(input), "-o", &s(out)].iter().map(|x| x.to_string()).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// "loading", "drawing", "finishing", "enlarging" or "polishing".
    pub phase: String,
    pub step: u32,
    pub total: u32,
}

/// `|####   | 3/8 - 1.20it/s` -> (3, 8).
pub fn parse_progress(line: &str) -> Option<(u32, u32)> {
    if !line.contains('|') {
        return None;
    }
    let tail = line.rsplit('|').next()?.trim();
    let frac = tail.split_whitespace().next()?;
    let (a, b) = frac.split_once('/')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

async fn run_engine(app: &AppHandle, exe: &Path, args: &[String], phase_default: &str, cancel: &CancellationToken) -> AppResult<()> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).kill_on_drop(true);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| AppError::msg(format!("could not start the picture engine: {e}")))?;
    let mut tail: Vec<String> = Vec::new();
    let mut phase = phase_default.to_string();
    let stderr = child.stderr.take().map(BufReader::new);
    let stdout = child.stdout.take().map(BufReader::new);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    for r in [stderr.map(|r| Box::new(r) as Box<dyn tokio::io::AsyncBufRead + Unpin + Send>), stdout.map(|r| Box::new(r) as Box<dyn tokio::io::AsyncBufRead + Unpin + Send>)].into_iter().flatten() {
        let tx = tx.clone();
        tokio::spawn(async move {
            // The engine redraws its progress bar with carriage returns, so split on those too.
            let mut r = r;
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            use tokio::io::AsyncReadExt;
            loop {
                match r.read(&mut byte).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if byte[0] == b'\n' || byte[0] == b'\r' {
                            if !buf.is_empty() {
                                let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
                                buf.clear();
                            }
                        } else {
                            buf.push(byte[0]);
                        }
                    }
                }
            }
        });
    }
    drop(tx);
    let started = Instant::now();
    loop {
        tokio::select! {
            line = rx.recv() => {
                let Some(line) = line else { break };
                let l = line.to_lowercase();
                if l.contains("loading tensors") || l.contains("loading model") { phase = "loading".into(); }
                else if l.contains("sampling") || l.contains("generating") { phase = "drawing".into(); }
                else if l.contains("decoding") { phase = "finishing".into(); }
                if let Some((step, total)) = parse_progress(&line) {
                    // Loading shows its own bar (hundreds of tensors): only report the picture's steps.
                    if phase != "loading" && total > 0 && total < 200 {
                        let _ = app.emit(PROGRESS_EVENT, Progress { phase: phase.clone(), step, total });
                    }
                } else {
                    if tail.len() >= 12 { tail.remove(0); }
                    tail.push(line);
                }
            }
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Err(AppError::Cancelled);
            }
        }
    }
    let status = child.wait().await?;
    log::info!("picture engine finished in {:.1}s ({status})", started.elapsed().as_secs_f32());
    if !status.success() {
        let why = tail.iter().rev().find(|l| l.contains("[E]") || l.to_lowercase().contains("error")).cloned().unwrap_or_else(|| tail.last().cloned().unwrap_or_default());
        return Err(AppError::msg(format!("the picture engine stopped: {why}")));
    }
    Ok(())
}

// ---------- making a picture ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Made {
    /// The saved file (full size).
    pub path: String,
    /// A JPEG preview as a data address, small enough to show in the window.
    pub preview: String,
    pub width: u32,
    pub height: u32,
    pub seed: i64,
    pub seconds: f32,
    /// What was done besides drawing: "Detail pass", "Enlarged 4x with ...", "Camera finish".
    pub notes: Vec<String>,
}

/// Words that mean the prompt wants a realistic picture of a person.
pub fn wants_real_person(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    let photo = ["photo", "photograph", "portrait", "headshot", "selfie", "realistic", "candid", "film still", "dslr"].iter().any(|k| p.contains(k));
    let person = [" man", " woman", "person", "people", " girl", " boy", "face", " lady", "child", "elderly", "grandm", "grandf", " guy", "human"].iter().any(|k| p.contains(k));
    photo && person
}

fn gallery_dir(models_dir: &Path) -> PathBuf {
    images_root(models_dir).join("gallery")
}

#[tauri::command]
pub async fn image_generate(app: AppHandle, state: State<'_, AppState>, req: Request) -> AppResult<Made> {
    crate::lock::ensure(&state)?;
    let settings = state.settings.lock().await.clone();
    if !cfg!(windows) {
        return Err(AppError::msg("Making pictures is in the Windows app for now."));
    }
    if settings.kids_mode {
        return Err(AppError::msg("Picture making is off in kids mode."));
    }
    if !settings.image_studio {
        return Err(AppError::msg("Turn on picture making in Settings → Images first."));
    }
    if req.prompt.trim().is_empty() {
        return Err(AppError::msg("Describe the picture you want."));
    }
    if !settings.image_real_people && wants_real_person(&req.prompt) {
        return Err(AppError::msg("This looks like a request for a realistic picture of a person. Turn on \"Realistic people\" in Settings → Images to allow it."));
    }
    let models_dir = state.paths.models.clone();
    let m = images::get(&req.model).ok_or_else(|| AppError::msg("unknown image model"))?;
    if m.kind != "generate" {
        return Err(AppError::msg("That model does not make pictures."));
    }
    let v = variant_of(m, &req.quant)?;
    let files = resolve(&models_dir, m, v)?;
    let exe = engine_path(&app, &models_dir).ok_or_else(|| AppError::msg("The picture engine is not in this build yet."))?;

    let token = CancellationToken::new();
    ACTIVE.lock().await.insert("generate".into(), token.clone());
    let result = make(&app, &settings, &models_dir, &exe, m, &files, &req, &token).await;
    ACTIVE.lock().await.remove("generate");
    result
}

#[tauri::command]
pub async fn image_cancel() -> AppResult<()> {
    if let Some(t) = ACTIVE.lock().await.get("generate") {
        t.cancel();
    }
    Ok(())
}

/// The installed upscaler to use: the chosen one, else the best installed for photos.
fn pick_upscaler(models_dir: &Path, chosen: &str) -> Option<(PathBuf, String, String)> {
    let order = [chosen, "upscale-4xnomoswebphoto-esrgan", "upscale-realesrgan-x4plus", "upscale-4xultrasharpv10"];
    for id in order.iter().filter(|i| !i.is_empty()) {
        if let Some(m) = images::get(id) {
            if let Some(v) = m.variants.first() {
                if installed(models_dir, m, v) {
                    let f = &v.files[0];
                    return Some((path_of(models_dir, f), m.name.clone(), id.to_string()));
                }
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
async fn make(
    app: &AppHandle,
    settings: &crate::settings::Settings,
    models_dir: &Path,
    exe: &Path,
    m: &ImageModel,
    files: &Resolved,
    req: &Request,
    cancel: &CancellationToken,
) -> AppResult<Made> {
    let started = Instant::now();
    let seed = req.seed.filter(|s| *s >= 0).unwrap_or_else(|| (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1) % 2_000_000_000) as i64);
    let dir = gallery_dir(models_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let base = dir.join(format!("{stamp}-{seed}"));
    let drawn = base.with_extension("png");
    let mut notes: Vec<String> = Vec::new();

    let vram = crate::gpu::best_discrete(crate::gpu::detect()).map(|g| g.dedicated_bytes);
    let upscaler = pick_upscaler(models_dir, &settings.image_upscaler);
    let hires = settings.image_hires.then(|| HiresPass {
        upscaler: upscaler.as_ref().and_then(|(p, _, _)| Some((p.parent()?.to_path_buf(), p.file_stem()?.to_string_lossy().into_owned()))),
    });
    if hires.is_some() {
        notes.push("Detail pass".into());
    }
    let opt = Options { placement: settings.image_placement.clone(), vram_bytes: vram, hires };
    let args = build_args(m, files, req, seed, &opt, &drawn);
    let _ = app.emit(PROGRESS_EVENT, Progress { phase: "loading".into(), step: 0, total: 0 });
    run_engine(app, exe, &args, "loading", cancel).await?;

    let mut current = drawn.clone();
    let mut steps: Vec<PathBuf> = vec![drawn.clone()];
    if settings.image_upscale {
        match &upscaler {
            Some((model, name, _)) => {
                let _ = app.emit(PROGRESS_EVENT, Progress { phase: "enlarging".into(), step: 0, total: 0 });
                let up = base.with_file_name(format!("{stamp}-{seed}-big.png"));
                run_engine(app, exe, &upscale_args(model, &current, &up), "enlarging", cancel).await?;
                notes.push(format!("Enlarged with {name}"));
                steps.push(up.clone());
                current = up;
            }
            None => notes.push("Not enlarged: no upscaler is downloaded (Settings → Images → Upscale)".into()),
        }
    }
    if settings.image_photo_finish {
        let _ = app.emit(PROGRESS_EVENT, Progress { phase: "polishing".into(), step: 0, total: 0 });
        let src = current.clone();
        let out = base.with_file_name(format!("{stamp}-{seed}-finished.jpg"));
        let strength = settings.image_finish_strength.min(100);
        let o2 = out.clone();
        tokio::task::spawn_blocking(move || crate::finish::finish_file(&src, &o2, strength)).await.map_err(|e| AppError::msg(e.to_string()))??;
        notes.push("Camera finish".into());
        steps.push(out.clone());
        current = out;
    }
    if settings.image_strip_info && current.extension().map(|e| e == "png").unwrap_or(false) {
        let p = current.clone();
        tokio::task::spawn_blocking(move || crate::finish::strip_file(&p)).await.map_err(|e| AppError::msg(e.to_string()))??;
        notes.push("Settings removed from the file".into());
    }
    let (preview, width, height) = tokio::task::spawn_blocking({
        let p = current.clone();
        move || crate::finish::preview(&p, 1600)
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;

    // What the gallery keeps: the final file only; every step before it was an intermediate.
    for step in steps.iter().filter(|p| **p != current) {
        let _ = tokio::fs::remove_file(step).await;
    }
    if settings.image_keep == "none" {
        let _ = tokio::fs::remove_file(&current).await;
    }
    Ok(Made { path: current.to_string_lossy().into_owned(), preview, width, height, seed, seconds: started.elapsed().as_secs_f32(), notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> &'static ImageModel {
        images::get(id).unwrap_or_else(|| panic!("{id}"))
    }

    fn res() -> Resolved {
        Resolved {
            main: PathBuf::from("m.gguf"),
            vae: Some("ae.safetensors".into()),
            clip_l: Some("clip_l.safetensors".into()),
            clip_g: None,
            t5xxl: Some("t5.safetensors".into()),
            llm: Some("llm.gguf".into()),
        }
    }

    fn req() -> Request {
        Request { model: "x".into(), quant: "Q8_0".into(), prompt: "a cat".into(), negative: "blur".into(), width: 896, height: 1152, steps: None, seed: None }
    }

    fn opt() -> Options {
        Options { placement: "auto".into(), vram_bytes: Some(16 << 30), hires: None }
    }

    #[test]
    fn parses_the_engines_progress_bar() {
        assert_eq!(parse_progress("  |#####     | 3/8 - 1.20it/s"), Some((3, 8)));
        assert_eq!(parse_progress("[I] sampling completed, taking 18.04s"), None);
    }

    #[test]
    fn flux_gets_its_text_files_and_no_negative_prompt_trouble() {
        let m = model("city96-flux-1-dev-gguf");
        let a = build_args(m, &res(), &req(), 7, &opt(), Path::new("o.png"));
        let j = a.join(" ");
        assert!(j.contains("--diffusion-model m.gguf") && j.contains("--clip_l clip_l.safetensors") && j.contains("--t5xxl t5.safetensors"));
        assert!(j.contains("--guidance 3.5") && j.contains("--seed 7") && j.contains("-W 896 -H 1152"));
        assert!(j.ends_with("-o o.png"));
    }

    #[test]
    fn a_distilled_model_gets_no_guidance_and_no_negative_prompt() {
        let m = model("unsloth-z-image-turbo-gguf");
        let a = build_args(m, &res(), &req(), 1, &opt(), Path::new("o.png"));
        let i = a.iter().position(|x| x == "--cfg-scale").unwrap();
        assert_eq!(a[i + 1], "1.0");
        assert!(!a.contains(&"-n".to_string()));
        assert!(a.contains(&"--llm".to_string()));
    }

    #[test]
    fn the_detail_pass_names_its_upscaler_or_falls_back_to_lanczos() {
        let m = model("unsloth-z-image-turbo-gguf");
        let mut o = opt();
        o.hires = Some(HiresPass { upscaler: Some((PathBuf::from("up"), "Nomos".into())) });
        let j = build_args(m, &res(), &req(), 1, &o, Path::new("o.png")).join(" ");
        assert!(j.contains("--hires --hires-scale 1.5") && j.contains("--hires-upscalers-dir up --hires-upscaler Nomos"));
        o.hires = Some(HiresPass { upscaler: None });
        assert!(build_args(m, &res(), &req(), 1, &o, Path::new("o.png")).join(" ").contains("--hires-upscaler Lanczos"));
    }

    #[test]
    fn ram_only_keeps_the_card_nearly_empty() {
        let m = model("unsloth-z-image-turbo-gguf");
        let mut o = opt();
        o.placement = "ram".into();
        assert!(build_args(m, &res(), &req(), 1, &o, Path::new("o.png")).join(" ").contains("--max-vram 1"));
    }

    #[test]
    fn files_of_different_repositories_never_share_a_folder() {
        let a = images::get("city96-flux-1-dev-gguf").unwrap();
        let q = images::get("unsloth-z-image-turbo-gguf").unwrap();
        let d = Path::new("models");
        assert_ne!(dir_for(d, &a.variants[0].files[0]), dir_for(d, &q.variants[0].files[0]));
        assert!(dir_for(d, &a.variants[0].files[0]).ends_with("city96__FLUX.1-dev-gguf"));
    }

    #[test]
    fn nothing_is_installed_on_a_fresh_disk() {
        let a = images::get("city96-flux-1-dev-gguf").unwrap();
        assert!(!installed(Path::new("/nonexistent"), a, &a.variants[0]));
        assert!(resolve(Path::new("/nonexistent"), a, &a.variants[0]).is_err());
    }

    #[test]
    fn spots_requests_for_realistic_people() {
        assert!(wants_real_person("A candid portrait photograph of a 68-year-old woman"));
        assert!(wants_real_person("realistic photo of a man at a bus stop"));
        assert!(!wants_real_person("a watercolor of a lighthouse at dusk"));
        assert!(!wants_real_person("a cartoon dragon"));
    }

    #[test]
    fn picks_the_engine_kind_for_the_graphics_card() {
        use crate::gpu::{Gpu, Vendor};
        let g = |v| Gpu { name: "x".into(), vendor: v, dedicated_bytes: 8 << 30, shared_bytes: 0, integrated: false };
        assert_eq!(recommended_engine(&[g(Vendor::Nvidia)]), "cuda");
        assert_eq!(recommended_engine(&[g(Vendor::Amd)]), "vulkan");
        assert_eq!(recommended_engine(&[]), "cpu");
    }

    #[test]
    fn the_engine_downloads_are_pinned() {
        for e in ENGINES {
            for z in e.zips {
                assert_eq!(z.sha256.len(), 64, "{}", z.name);
                assert!(z.size > 1_000_000 && z.name.ends_with(".zip"));
            }
        }
    }

    #[test]
    fn unzip_flat_puts_every_file_in_one_folder_and_cannot_escape_it() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("byte-unzip-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("out")).unwrap();
        let zp = dir.join("a.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            w.start_file("bin/sd-cli.exe", o).unwrap();
            w.write_all(b"exe").unwrap();
            w.start_file("../evil.dll", o).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        unzip_flat(&zp, &dir.join("out")).unwrap();
        assert!(dir.join("out/sd-cli.exe").is_file() && dir.join("out/evil.dll").is_file());
        assert!(!dir.join("evil.dll").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_enlarge_command_line() {
        let a = upscale_args(Path::new("u.gguf"), Path::new("i.png"), Path::new("o.png"));
        assert_eq!(a.join(" "), "-M upscale --upscale-model u.gguf -i i.png -o o.png");
    }
}
