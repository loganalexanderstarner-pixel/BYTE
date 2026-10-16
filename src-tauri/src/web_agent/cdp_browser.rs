//! The real-browser mode of the web agent (Settings → Browser → "Use my browser program"): Edge or Chrome,
//! started by BYTE with its own profile folder and a debugging port that only listens on this computer.
//!
//! - never the person's own profile: the folder is `byte-browser-profile` inside BYTE's data folder, and
//!   `is_safe_profile_dir` refuses anything that looks like a normal Edge/Chrome profile;
//! - a random free port bound to 127.0.0.1 (`--remote-debugging-address`), used once; the page address the
//!   browser reports for it contains a random id, and BYTE connects only to the address its own child reported;
//! - the browser is closed when the answer ends (the child is killed when the session is dropped).
//!
//! `CdpBrowser` implements the same `Browser` trait as the hidden window, so `Session` (approvals, private
//! fields, public sites, step limit) runs unchanged on top of it.

use std::path::{Path, PathBuf};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use url::Url;

use super::cdp::{Driver, Timing, Transport};
use super::{Browser, Capture};
use crate::error::{AppError, AppResult};

/// A page BYTE drives through the DevTools protocol. `guard` keeps the browser process alive (killed on drop).
pub struct CdpBrowser<T: Transport> {
    d: Driver<T>,
    _guard: Option<Box<dyn Send + Sync>>,
}

impl<T: Transport> CdpBrowser<T> {
    pub fn new(t: T, timing: Timing, guard: Option<Box<dyn Send + Sync>>) -> CdpBrowser<T> {
        CdpBrowser { d: Driver::new(t, timing), _guard: guard }
    }

    pub async fn init(&self) {
        self.d.init().await;
    }

    pub fn driver(&self) -> &Driver<T> {
        &self.d
    }
}

impl<T: Transport> Browser for CdpBrowser<T> {
    fn open<'a>(&'a self, url: &'a Url) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move { self.d.open(url.as_str()).await.map(|_| ()) })
    }

    fn call<'a>(&'a self, method: &'a str, args: Value) -> BoxFuture<'a, AppResult<Value>> {
        Box::pin(async move { Ok(self.d.bridge_call(method, &args).await) })
    }

    fn settle(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move {
            self.d.wait_loaded().await;
            self.d.enforce_site_rules().await
        })
    }

    fn back(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move { self.d.back().await.map(|_| ()) })
    }

    fn stopped(&self) -> bool {
        self.d.t.closed()
    }

    fn capture(&self, kind: Capture) -> BoxFuture<'_, AppResult<Vec<u8>>> {
        Box::pin(async move {
            use base64::Engine;
            let (method, params, field) = match kind {
                Capture::Pdf => ("Page.printToPDF", json!({ "printBackground": true }), "data"),
                Capture::Png => ("Page.captureScreenshot", json!({ "format": "png", "captureBeyondViewport": true }), "data"),
                Capture::Archive => ("Page.captureSnapshot", json!({ "format": "mhtml" }), "data"),
            };
            let r = self.d.t.send(method, params).await?;
            let data = r.get(field).and_then(Value::as_str).ok_or_else(|| AppError::msg("the browser gave no data for the page"))?;
            if kind == Capture::Archive {
                return Ok(data.as_bytes().to_vec());
            }
            base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| AppError::msg(format!("the page picture wasn't readable: {e}")))
        })
    }
}

// ------------------------------------------------------------------ starting it

/// Where Edge or Chrome usually is, best first (Edge first on Windows: it is always there).
pub fn browser_candidates() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles(x86)", "ProgramFiles", "LocalAppData"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                v.push(base.join("Microsoft/Edge/Application/msedge.exe"));
                v.push(base.join("Google/Chrome/Application/chrome.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        v.push("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into());
        v.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into());
    } else {
        for name in ["microsoft-edge", "microsoft-edge-stable", "google-chrome", "google-chrome-stable", "chromium", "chromium-browser"] {
            for dir in ["/usr/bin", "/usr/local/bin", "/snap/bin", "/opt/microsoft/msedge", "/opt/google/chrome"] {
                v.push(Path::new(dir).join(name));
            }
        }
    }
    v
}

pub fn find_browser() -> Option<PathBuf> {
    browser_candidates().into_iter().find(|p| p.is_file())
}

/// BYTE's own profile folder inside its data folder.
pub const PROFILE_NAME: &str = "byte-browser-profile";

/// True only for a folder named like BYTE's own, and not inside a normal browser's profile area.
pub fn is_safe_profile_dir(dir: &Path) -> bool {
    // Windows paths too, whatever system this runs on.
    let text = dir.to_string_lossy().to_lowercase().replace('\\', "/");
    let name_ok = text.trim_end_matches('/').rsplit('/').next() == Some(PROFILE_NAME);
    let theirs = ["user data", "/google/chrome", "/microsoft/edge", "/.config/google-chrome", "/.config/chromium", "/.config/microsoft-edge", "application support/google", "application support/microsoft edge", "/default"];
    name_ok && !theirs.iter().any(|t| text.contains(t))
}

/// The command line: a profile folder of BYTE's own and a debugging port on this computer only.
pub fn launch_args(port: u16, profile: &Path) -> Vec<String> {
    vec![
        format!("--remote-debugging-port={port}"),
        "--remote-debugging-address=127.0.0.1".into(),
        format!("--user-data-dir={}", profile.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-sync".into(),
        "--disable-background-mode".into(),
        "--window-size=1280,900".into(),
        "about:blank".into(),
    ]
}

/// A free port on 127.0.0.1, chosen by the system (so it is random), used straight away.
pub fn free_port() -> AppResult<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(l.local_addr()?.port())
}

/// The page to drive, from `/json/list`: the first normal tab, with an address on our own port only.
pub fn pick_page(list: &Value, port: u16) -> Option<String> {
    list.as_array()?.iter().filter(|t| t["type"] == "page").find_map(|t| {
        let ws = t["webSocketDebuggerUrl"].as_str()?;
        let u = Url::parse(ws).ok()?;
        let local = matches!(u.host_str(), Some("127.0.0.1" | "localhost"));
        (u.scheme() == "ws" && local && u.port() == Some(port) && u.path().starts_with("/devtools/page/")).then(|| u.path().to_string())
    })
}

/// Keeps the browser process alive and closes it when dropped.
struct ChildGuard(#[allow(dead_code)] tokio::process::Child);

/// Starts Edge/Chrome with BYTE's own profile and connects to its first tab.
pub async fn launch(profile: &Path) -> AppResult<CdpBrowser<super::cdp_ws::WsTransport>> {
    launch_in(profile, false).await
}

// --- track:found begin ---
/// Edge's own profile folder (what the person uses every day), for the opt-in "use my own Edge profile". None off Windows.
pub fn own_profile_dir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    std::env::var_os("LocalAppData").map(|b| PathBuf::from(b).join("Microsoft/Edge/User Data"))
}
// --- track:found end ---

/// `own`: the folder is the person's own browser profile (never without their explicit opt-in). Newer Edge versions refuse remote control of
/// their everyday profile; then this fails with the usual "didn't open its debugging port" message and BYTE falls back to its own profile.
pub async fn launch_in(profile: &Path, own: bool) -> AppResult<CdpBrowser<super::cdp_ws::WsTransport>> {
    if !own && !is_safe_profile_dir(profile) {
        return Err(AppError::msg("BYTE will only use its own browser profile folder"));
    }
    let exe = find_browser().ok_or_else(|| AppError::msg("Edge or Chrome isn't installed (looked in the usual places)"))?;
    std::fs::create_dir_all(profile)?;
    let port = free_port()?;
    let child = tokio::process::Command::new(&exe)
        .args(launch_args(port, profile))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| AppError::msg(format!("couldn't start the browser: {e}")))?;
    let mut child = child;
    // Wait until it answers on its debugging port, and only trust a Chromium that says so.
    let mut path = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // A browser that ends at once handed the job to an instance already running (or crashed).
        if let Ok(Some(status)) = child.try_wait() {
            return Err(AppError::msg(format!("the browser closed right after starting ({status}); another window may already be using BYTE's browser profile. Close it and try again")));
        }
        if let Ok(list) = super::cdp_ws::http_get_json(port, "/json/list").await {
            if let Some(p) = pick_page(&list, port) {
                path = Some(p);
                break;
            }
        }
    }
    let path = path.ok_or_else(|| AppError::msg("the browser didn't open its debugging port in time (a copy that is already open on BYTE's profile folder can swallow the start)"))?;
    let guard = ChildGuard(child);
    let version = super::cdp_ws::http_get_json(port, "/json/version").await?;
    let product = version["Browser"].as_str().unwrap_or("");
    if !(product.starts_with("Chrome/") || product.starts_with("Edg/") || product.starts_with("HeadlessChrome/")) {
        return Err(AppError::msg("what answered on the debugging port isn't Edge or Chrome"));
    }
    let ws = super::cdp_ws::WsTransport::connect(port, &path).await?;
    let b = CdpBrowser::new(ws, Timing::REAL, Some(Box::new(guard)));
    b.init().await;
    Ok(b)
}

#[cfg(test)]
#[path = "cdp_browser_tests.rs"]
mod tests;
