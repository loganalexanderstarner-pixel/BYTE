//! The video helper (Phase 11): yt-dlp, downloaded on request, fetches the audio
//! of a YouTube video that has no captions, so BYTE can transcribe it on this
//! Mac (voice.rs) and summarize it like any other video (youtube.rs).
//!
//! yt-dlp is the official single-file build from its GitHub releases, checked
//! against the release's `SHA2-256SUMS`, kept in `<data>/tools/`. It's never
//! downloaded or updated by itself. The video id travels as one argument after
//! `--`, always as a youtube.com watch link.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::models::{DownloadEvent, DOWNLOAD_EVENT};
use crate::state::AppState;

const RELEASES: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
pub const KEY: &str = "tool:yt-dlp";
/// About how big the download is (shown before it starts).
const APPROX_SIZE: u64 = 36_000_000;
/// Longest video BYTE fetches the audio of.
const MAX_SECONDS: u64 = 3 * 3600;
const TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The release file for this computer.
fn asset() -> &'static str {
    if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp_linux"
    }
}

pub fn tool_path(data: &Path) -> PathBuf {
    data.join("tools").join(if cfg!(target_os = "windows") { "yt-dlp.exe" } else { "yt-dlp" })
}

/// The checksum of `asset` in a `SHA2-256SUMS` file ("<hex>  <name>" lines).
pub fn checksum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (hash, name) = (it.next()?, it.next()?);
        (name.trim_start_matches('*') == asset && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())).then(|| hash.to_lowercase())
    })
}

/// The installed yt-dlp's version (None when it isn't there or won't run).
pub async fn version(data: &Path) -> Option<String> {
    let p = tool_path(data);
    if !p.is_file() {
        return None;
    }
    let out = tokio::time::timeout(Duration::from_secs(20), tokio::process::Command::new(&p).arg("--version").kill_on_drop(true).output()).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|v| !v.is_empty())
}

/// Downloads (or updates) yt-dlp, with progress as `models://download` events under `KEY`.
async fn download(app: &AppHandle, net: &reqwest::Client, data: &Path) -> AppResult<()> {
    let emit = |ev: DownloadEvent| {
        let _ = app.emit(DOWNLOAD_EVENT, ev);
    };
    let sums = net.get(format!("{RELEASES}/SHA2-256SUMS")).timeout(Duration::from_secs(30)).send().await?.error_for_status()?.text().await?;
    let want = checksum_for(&sums, asset()).ok_or_else(|| AppError::msg("yt-dlp's release has no checksum for this computer."))?;
    let resp = net.get(format!("{RELEASES}/{}", asset())).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(APPROX_SIZE);
    let dest = tool_path(data);
    std::fs::create_dir_all(dest.parent().unwrap_or(data))?;
    let part = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hasher = Sha256::new();
    let (mut bytes, started, mut shown) = (0u64, Instant::now(), Instant::now());
    let mut body = resp.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        bytes += chunk.len() as u64;
        if shown.elapsed() > Duration::from_millis(250) {
            shown = Instant::now();
            emit(DownloadEvent::Progress { id: KEY.into(), bytes, total, bytes_per_sec: bytes as f64 / started.elapsed().as_secs_f64().max(0.001) });
        }
    }
    file.flush().await?;
    drop(file);
    emit(DownloadEvent::Verifying { id: KEY.into() });
    let got = format!("{:x}", hasher.finalize());
    if got != want {
        let _ = std::fs::remove_file(&part);
        return Err(AppError::msg("The yt-dlp download didn't match its checksum, so BYTE threw it away."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&part, &dest)?;
    Ok(())
}

/// What yt-dlp says about a video, printed before it downloads the audio.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VideoMeta {
    pub title: String,
    pub channel: String,
    pub seconds: u64,
    pub thumbnail: String,
}

/// yt-dlp's arguments for one video's audio (m4a when there is one).
pub fn audio_args(id: &str, out_dir: &Path) -> Vec<String> {
    vec![
        "--no-playlist".into(),
        "--no-warnings".into(),
        "--no-progress".into(),
        "--no-simulate".into(),
        "--match-filter".into(),
        format!("duration <= {MAX_SECONDS}"),
        "-f".into(),
        "bestaudio[ext=m4a]/bestaudio".into(),
        "--print".into(),
        "before_dl:%(title)s\t%(channel)s\t%(duration)s\t%(thumbnail)s".into(),
        "-o".into(),
        out_dir.join("audio.%(ext)s").to_string_lossy().into_owned(),
        "--".into(),
        format!("https://www.youtube.com/watch?v={id}"),
    ]
}

/// The tab-separated line `audio_args` asks yt-dlp to print.
pub fn parse_meta(stdout: &str) -> Option<VideoMeta> {
    let line = stdout.lines().find(|l| l.matches('\t').count() >= 3)?;
    let mut f = line.split('\t');
    let mut val = || f.next().map(str::trim).filter(|v| *v != "NA").unwrap_or("").to_string();
    let (title, channel, seconds, thumbnail) = (val(), val(), val(), val());
    Some(VideoMeta { title, channel, seconds: seconds.parse::<f64>().map(|s| s as u64).unwrap_or(0), thumbnail })
}

/// Downloads a video's audio into `out_dir`; returns what yt-dlp said about it and the file.
pub async fn video_audio(data: &Path, id: &str, out_dir: &Path) -> AppResult<(VideoMeta, PathBuf)> {
    if !(id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        return Err(AppError::msg("That isn't a YouTube video id."));
    }
    let bin = tool_path(data);
    if !bin.is_file() {
        return Err(AppError::msg("BYTE needs its video helper for videos without captions: Settings → Models → Voice → Video helper."));
    }
    let fut = tokio::process::Command::new(&bin).args(audio_args(id, out_dir)).kill_on_drop(true).output();
    let out = tokio::time::timeout(TIMEOUT, fut).await.map_err(|_| AppError::msg("Getting the video's audio took too long."))??;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let meta = parse_meta(&stdout).unwrap_or_default();
    let file = std::fs::read_dir(out_dir)?.flatten().map(|e| e.path()).find(|p| p.file_stem().is_some_and(|s| s == "audio") && p.extension().is_some_and(|e| e != "part"));
    match file {
        Some(f) if out.status.success() => Ok((meta, f)),
        _ if meta.seconds > MAX_SECONDS => Err(AppError::msg("That video is over 3 hours, which is longer than BYTE transcribes.")),
        _ => {
            let err = String::from_utf8_lossy(&out.stderr);
            let last = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("it didn't say why").trim().to_string();
            Err(AppError::msg(format!("The video helper couldn't get the audio ({last}). Updating it in Settings → Models → Voice often fixes this.")))
        }
    }
}

// ------------------------------------------------------------------ commands

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaStatus {
    /// The installed yt-dlp version, if any.
    pub version: Option<String>,
    pub approx_bytes: u64,
    pub key: String,
}

#[tauri::command]
pub async fn media_status(state: State<'_, AppState>) -> AppResult<MediaStatus> {
    Ok(MediaStatus { version: version(&state.paths.data).await, approx_bytes: APPROX_SIZE, key: KEY.into() })
}

/// Downloads or updates the video helper (in the background; progress as download events).
#[tauri::command]
pub async fn media_download(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let (net, data) = (state.net.clone(), state.paths.data.clone());
    tauri::async_runtime::spawn(async move {
        let ev = match download(&app, &net, &data).await {
            Ok(()) => DownloadEvent::Finished { id: KEY.into() },
            Err(e) => DownloadEvent::Failed { id: KEY.into(), message: e.to_string() },
        };
        let _ = app.emit(DOWNLOAD_EVENT, ev);
    });
    Ok(())
}

#[tauri::command]
pub async fn media_delete(state: State<'_, AppState>) -> AppResult<()> {
    let p = tool_path(&state.paths.data);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_checksum_for_this_computer() {
        let sums = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  yt-dlp_linux\nABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789  yt-dlp_macos\nnot-a-hash  yt-dlp.exe\n";
        assert_eq!(checksum_for(sums, "yt-dlp_macos").unwrap(), "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789");
        assert!(checksum_for(sums, "yt-dlp.exe").is_none());
        assert!(checksum_for(sums, "yt-dlp_macos_legacy").is_none());
    }

    #[test]
    fn video_arguments_are_fixed() {
        let a = audio_args("dQw4w9WgXcQ", Path::new("/tmp/v"));
        assert_eq!(a[a.len() - 2], "--");
        assert_eq!(a.last().unwrap(), "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert!(a.contains(&"duration <= 10800".to_string()));
        assert!(a.contains(&"/tmp/v/audio.%(ext)s".to_string()));
    }

    #[test]
    fn reads_what_yt_dlp_printed() {
        let m = parse_meta("[youtube] dQw4: Downloading\nMe at the zoo\tjawed\t19\thttps://i.ytimg.com/vi/x/hq.jpg\n").unwrap();
        assert_eq!(m, VideoMeta { title: "Me at the zoo".into(), channel: "jawed".into(), seconds: 19, thumbnail: "https://i.ytimg.com/vi/x/hq.jpg".into() });
        assert_eq!(parse_meta("A\tNA\t12.5\tNA").unwrap().channel, "");
        assert!(parse_meta("nothing here").is_none());
    }

    #[tokio::test]
    async fn refuses_odd_ids_and_a_missing_helper() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(video_audio(tmp.path(), "x; rm -rf ~", tmp.path()).await.unwrap_err().to_string().contains("isn't a YouTube video id"));
        assert!(video_audio(tmp.path(), "dQw4w9WgXcQ", tmp.path()).await.unwrap_err().to_string().contains("video helper"));
    }
}
