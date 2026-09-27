//! Model catalog and the resumable, verified downloader.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::system::{self, FitPlan, ModelArch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// A chat model the user talks to.
    Chat,
    /// Small helper for speculative decoding and routing.
    Draft,
    /// Embedding model for search and knowledge base.
    Embed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub tagline: &'static str,
    pub repo: &'static str,
    pub file: &'static str,
    pub size_bytes: u64,
    pub sha256: &'static str,
    pub arch: ModelArch,
    pub role: Role,
    pub recommended: bool,
    pub thinking: bool,
    /// Rough generation speed on a base M4, tokens per second.
    pub speed_hint: &'static str,
}

impl CatalogEntry {
    pub fn url(&self) -> String {
        format!("https://huggingface.co/{}/resolve/main/{}?download=true", self.repo, self.file)
    }
}

const QWEN3_CTX: u32 = 32768;

pub static CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "qwen3-14b",
        name: "Qwen3 14B",
        tagline: "Smart — best reasoning that fits 16 GB",
        repo: "Qwen/Qwen3-14B-GGUF",
        file: "Qwen3-14B-Q4_K_M.gguf",
        size_bytes: 9_001_752_960,
        sha256: "500a8806e85ee9c83f3ae08420295592451379b4f8cf2d0f41c15dffeb6b81f0",
        arch: ModelArch { n_layer: 40, n_head_kv: 8, head_dim: 128, max_ctx: QWEN3_CTX },
        role: Role::Chat,
        recommended: true,
        thinking: true,
        speed_hint: "12–15 tok/s",
    },
    CatalogEntry {
        id: "qwen3-8b",
        name: "Qwen3 8B",
        tagline: "Fast — quicker replies, a little less capable",
        repo: "Qwen/Qwen3-8B-GGUF",
        file: "Qwen3-8B-Q4_K_M.gguf",
        size_bytes: 5_027_783_488,
        sha256: "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785",
        arch: ModelArch { n_layer: 36, n_head_kv: 8, head_dim: 128, max_ctx: QWEN3_CTX },
        role: Role::Chat,
        recommended: false,
        thinking: true,
        speed_hint: "22–28 tok/s",
    },
    CatalogEntry {
        id: "qwen3-30b-a3b",
        name: "Qwen3 30B-A3B",
        tagline: "Power — mixture-of-experts, needs 24 GB+",
        repo: "unsloth/Qwen3-30B-A3B-GGUF",
        file: "Qwen3-30B-A3B-Q3_K_M.gguf",
        size_bytes: 14_711_847_488,
        sha256: "a63f06070fccd2e3a329f3bfe5e996709a7f305d04c6acf2c8ae900c4723fffa",
        arch: ModelArch { n_layer: 48, n_head_kv: 4, head_dim: 128, max_ctx: QWEN3_CTX },
        role: Role::Chat,
        recommended: false,
        thinking: true,
        speed_hint: "30–40 tok/s",
    },
    CatalogEntry {
        id: "qwen3-0.6b",
        name: "Qwen3 0.6B",
        tagline: "Helper — speeds up the big model and routes requests",
        repo: "Qwen/Qwen3-0.6B-GGUF",
        file: "Qwen3-0.6B-Q8_0.gguf",
        size_bytes: 639_446_688,
        sha256: "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
        arch: ModelArch { n_layer: 28, n_head_kv: 8, head_dim: 128, max_ctx: QWEN3_CTX },
        role: Role::Draft,
        recommended: false,
        thinking: false,
        speed_hint: "150+ tok/s",
    },
    CatalogEntry {
        id: "nomic-embed-v1.5",
        name: "Nomic Embed v1.5",
        tagline: "Search — powers your knowledge base and answer cache",
        repo: "nomic-ai/nomic-embed-text-v1.5-GGUF",
        file: "nomic-embed-text-v1.5.Q8_0.gguf",
        size_bytes: 146_146_432,
        sha256: "3e24342164b3d94991ba9692fdc0dd08e3fd7362e0aacc396a9a5c54a544c3b7",
        arch: ModelArch { n_layer: 12, n_head_kv: 12, head_dim: 64, max_ctx: 8192 },
        role: Role::Embed,
        recommended: false,
        thinking: false,
        speed_hint: "—",
    },
];

pub fn find(id: &str) -> AppResult<&'static CatalogEntry> {
    CATALOG
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::msg(format!("unknown model '{id}'")))
}

pub fn model_path(models_dir: &Path, entry: &CatalogEntry) -> PathBuf {
    models_dir.join(entry.file)
}

fn part_path(models_dir: &Path, entry: &CatalogEntry) -> PathBuf {
    models_dir.join(format!("{}.part", entry.file))
}

pub fn is_installed(models_dir: &Path, entry: &CatalogEntry) -> bool {
    std::fs::metadata(model_path(models_dir, entry))
        .map(|m| m.len() == entry.size_bytes)
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    pub installed: bool,
    pub partial_bytes: u64,
    pub downloading: bool,
    pub fit: FitPlan,
}

pub fn list(models_dir: &Path, desired_ctx: u32, downloading: &[String]) -> Vec<ModelStatus> {
    let info = system::system_info(models_dir);
    CATALOG
        .iter()
        .map(|e| ModelStatus {
            installed: is_installed(models_dir, e),
            partial_bytes: std::fs::metadata(part_path(models_dir, e)).map(|m| m.len()).unwrap_or(0),
            downloading: downloading.iter().any(|d| d == e.id),
            fit: system::plan_fit(e.size_bytes, e.arch, desired_ctx, info.total_ram_bytes, info.gpu_budget_bytes),
            entry: e.clone(),
        })
        .collect()
}

pub fn delete(models_dir: &Path, id: &str) -> AppResult<()> {
    let e = find(id)?;
    for p in [model_path(models_dir, e), part_path(models_dir, e)] {
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

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

/// Tracks in-flight downloads so they can be paused and are never started twice.
#[derive(Default, Clone)]
pub struct Downloads {
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl Downloads {
    pub async fn active_ids(&self) -> Vec<String> {
        self.active.lock().await.keys().cloned().collect()
    }

    pub async fn pause(&self, id: &str) {
        if let Some(t) = self.active.lock().await.get(id) {
            t.cancel();
        }
    }

    /// Starts a download in the background. Progress arrives as
    /// `models://download` events.
    pub async fn start(&self, app: AppHandle, client: reqwest::Client, models_dir: PathBuf, id: String) -> AppResult<()> {
        let entry = find(&id)?;
        let token = CancellationToken::new();
        {
            let mut active = self.active.lock().await;
            if active.contains_key(&id) {
                return Ok(());
            }
            active.insert(id.clone(), token.clone());
        }
        let active = self.active.clone();
        tauri::async_runtime::spawn(async move {
            let emit = |ev: DownloadEvent| {
                let _ = app.emit(DOWNLOAD_EVENT, ev);
            };
            let result = download(&client, &models_dir, entry, &token, &emit).await;
            active.lock().await.remove(&id);
            match result {
                Ok(()) => emit(DownloadEvent::Finished { id }),
                Err(AppError::Cancelled) => {
                    let bytes = std::fs::metadata(part_path(&models_dir, entry)).map(|m| m.len()).unwrap_or(0);
                    emit(DownloadEvent::Paused { id, bytes })
                }
                Err(e) => {
                    log::error!("download of {id} failed: {e}");
                    emit(DownloadEvent::Failed { id, message: e.to_string() })
                }
            }
        });
        Ok(())
    }
}

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// Extra free space to leave on disk beyond the file itself.
const DISK_MARGIN: u64 = 1_000_000_000;

pub async fn download(
    client: &reqwest::Client,
    models_dir: &Path,
    entry: &CatalogEntry,
    cancel: &CancellationToken,
    emit: &(dyn Fn(DownloadEvent) + Sync),
) -> AppResult<()> {
    download_from(client, &entry.url(), models_dir, entry, cancel, emit).await
}

pub async fn download_from(
    client: &reqwest::Client,
    url: &str,
    models_dir: &Path,
    entry: &CatalogEntry,
    cancel: &CancellationToken,
    emit: &(dyn Fn(DownloadEvent) + Sync),
) -> AppResult<()> {
    let id = entry.id.to_string();
    let dest = model_path(models_dir, entry);
    if is_installed(models_dir, entry) {
        return Ok(());
    }
    let part = part_path(models_dir, entry);
    let mut hasher = Sha256::new();
    let mut have: u64 = 0;

    if let Ok(meta) = tokio::fs::metadata(&part).await {
        if meta.len() > entry.size_bytes {
            tokio::fs::remove_file(&part).await?;
        } else if meta.len() > 0 {
            emit(DownloadEvent::Resuming { id: id.clone(), bytes: meta.len() });
            have = hash_existing(&part, &mut hasher, cancel).await?;
        }
    }

    let free = system::free_disk_for(models_dir);
    let remaining = entry.size_bytes - have;
    if free != 0 && free < remaining + DISK_MARGIN {
        return Err(AppError::msg(format!(
            "Not enough disk space: need {:.1} GB free, {:.1} GB available.",
            (remaining + DISK_MARGIN) as f64 / 1e9,
            free as f64 / 1e9
        )));
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
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && have == entry.size_bytes {
        return finish(&part, &dest, hasher, entry, emit).await;
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
                    entry.size_bytes as f64 / 1e9
                )));
            }
        };
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        have += chunk.len() as u64;
        window_bytes += chunk.len() as u64;
        if have > entry.size_bytes {
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
            emit(DownloadEvent::Progress { id: id.clone(), bytes: have, total: entry.size_bytes, bytes_per_sec: rate });
            last_emit = Instant::now();
        }
    }
    file.flush().await?;
    drop(file);
    if have != entry.size_bytes {
        return Err(AppError::msg(format!(
            "connection closed early ({:.1} of {:.1} GB). Press Download to resume.",
            have as f64 / 1e9,
            entry.size_bytes as f64 / 1e9
        )));
    }
    finish(&part, &dest, hasher, entry, emit).await
}

async fn finish(
    part: &Path,
    dest: &Path,
    hasher: Sha256,
    entry: &CatalogEntry,
    emit: &(dyn Fn(DownloadEvent) + Sync),
) -> AppResult<()> {
    emit(DownloadEvent::Verifying { id: entry.id.to_string() });
    let digest = hex(&hasher.finalize());
    if digest != entry.sha256 {
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

    #[test]
    fn catalog_ids_are_unique_and_hashes_look_valid() {
        let mut ids: Vec<_> = CATALOG.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        for m in CATALOG {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(m.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(m.url().starts_with("https://huggingface.co/"));
        }
        assert_eq!(CATALOG.iter().filter(|m| m.recommended).count(), 1);
    }

    #[test]
    fn hex_encodes() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
    }

    /// Minimal HTTP/1.1 server that honours `Range: bytes=N-` and can cut the
    /// connection after `cut_after` bytes to simulate a dropped download.
    async fn serve(data: Arc<Vec<u8>>, cut_after: Option<usize>, ignore_range: bool) -> String {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { return };
                let data = data.clone();
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    let mut start = 0usize;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        let l = line.to_ascii_lowercase();
                        if let Some(v) = l.strip_prefix("range: bytes=") {
                            start = v.trim().trim_end_matches('-').parse().unwrap_or(0);
                        }
                    }
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
        format!("http://{addr}/model.gguf")
    }

    fn test_entry(data: &[u8]) -> &'static CatalogEntry {
        let sha: &'static str = Box::leak(hex(&Sha256::digest(data)).into_boxed_str());
        Box::leak(Box::new(CatalogEntry {
            id: "test-model",
            name: "Test",
            tagline: "",
            repo: "",
            file: "test-model.gguf",
            size_bytes: data.len() as u64,
            sha256: sha,
            arch: ModelArch { n_layer: 1, n_head_kv: 1, head_dim: 1, max_ctx: 1 },
            role: Role::Chat,
            recommended: false,
            thinking: false,
            speed_hint: "",
        }))
    }

    fn noop(_: DownloadEvent) {}

    #[tokio::test]
    async fn resumes_after_dropped_connection_and_verifies() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let entry = test_entry(&data);
        let dir = tempfile::tempdir().unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let cancel = CancellationToken::new();

        let url = serve(Arc::new(data.clone()), Some(120_000), false).await;
        let err = download_from(&client, &url, dir.path(), entry, &cancel, &noop).await.unwrap_err();
        assert!(err.to_string().contains("resume"), "{err}");
        assert_eq!(std::fs::metadata(part_path(dir.path(), entry)).unwrap().len(), 120_000);

        let url = serve(Arc::new(data.clone()), None, false).await;
        download_from(&client, &url, dir.path(), entry, &cancel, &noop).await.unwrap();
        assert!(is_installed(dir.path(), entry));
        assert_eq!(std::fs::read(model_path(dir.path(), entry)).unwrap(), data);
        assert!(!part_path(dir.path(), entry).exists());
    }

    #[tokio::test]
    async fn restarts_cleanly_when_server_ignores_range() {
        let data: Vec<u8> = (0..50_000u32).map(|i| (i % 13) as u8).collect();
        let entry = test_entry(&data);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(part_path(dir.path(), entry), &data[..10_000]).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = serve(Arc::new(data.clone()), None, true).await;
        download_from(&client, &url, dir.path(), entry, &CancellationToken::new(), &noop).await.unwrap();
        assert_eq!(std::fs::read(model_path(dir.path(), entry)).unwrap(), data);
    }

    #[tokio::test]
    async fn rejects_corrupted_download() {
        let good: Vec<u8> = vec![1; 40_000];
        let entry = test_entry(&good);
        let mut bad = good.clone();
        bad[20_000] = 9;
        let dir = tempfile::tempdir().unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = serve(Arc::new(bad), None, false).await;
        let err = download_from(&client, &url, dir.path(), entry, &CancellationToken::new(), &noop).await.unwrap_err();
        assert!(err.to_string().contains("checksum"), "{err}");
        assert!(!model_path(dir.path(), entry).exists());
        assert!(!part_path(dir.path(), entry).exists());
    }
}
