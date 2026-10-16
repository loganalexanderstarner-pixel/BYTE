//! Live research in real browser tabs ("Keep researching until I'm back"): Edge (or Chrome) is started visibly with a separate profile
//! folder and a debugging port on 127.0.0.1 only, each page BYTE reads opens in a NEW tab (`Target.createTarget`) so the person
//! watches them appear, and the same safety as everywhere else applies, because every tab is driven by the same `Driver`: site rules
//! before and after every load, private fields, approval cards, untrusted-page wrapping, size caps (the `Session` on top does those).
//!
//! The debugging port exists only while the job runs. When it ends (the person says "I'm back", presses Stop, or the time limit is
//! reached) BYTE detaches, closes that browser process, and unless "close the tabs when I'm back" is on, reopens the pages as tabs of a
//! normal window of the same program, which has no debugging port. [`TabsBrowser`] is generic over a [`TabSource`] so all of this is
//! tested with the scripted fake browser; [`EdgeTabs`] is the real source.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::cdp::{Timing, Transport};
use super::cdp_browser::CdpBrowser;
use super::{Browser, Capture};
use crate::error::{AppError, AppResult};

/// Most pages handed back to the person's window at the end.
pub const MAX_HANDOVER: usize = 20;

/// Where tabs come from.
pub trait TabSource: Send + Sync {
    /// A tab to read one page in: the first call may give the blank tab the browser started with, later calls open a new one.
    /// Returns its id (to close it again) and the page driver.
    fn new_tab(&self) -> BoxFuture<'_, AppResult<(String, Box<dyn Browser>)>>;
    fn close_tab<'a>(&'a self, id: &'a str) -> BoxFuture<'a, ()>;
    /// The browser went away (the person closed all its windows).
    fn gone(&self) -> bool;
    /// Detach and end the browser process. `reopen`: addresses to show the person afterwards in a normal window (none = close all).
    fn finish<'a>(&'a self, reopen: Vec<String>) -> BoxFuture<'a, ()>;
}

/// The browser BYTE drives for a research job: the current tab is the last one opened.
pub struct TabsBrowser<S: TabSource> {
    src: S,
    tabs: Mutex<Vec<Arc<dyn Browser>>>,
    visited: Mutex<Vec<String>>,
    done: AtomicBool,
}

impl<S: TabSource> TabsBrowser<S> {
    pub fn new(src: S) -> Self {
        TabsBrowser { src, tabs: Mutex::new(Vec::new()), visited: Mutex::new(Vec::new()), done: AtomicBool::new(false) }
    }

    fn current(&self) -> AppResult<Arc<dyn Browser>> {
        if self.done.load(Ordering::SeqCst) {
            return Err(AppError::msg("the research browser has been closed"));
        }
        self.tabs.lock().ok().and_then(|t| t.last().cloned()).ok_or_else(|| AppError::msg("no page is open yet"))
    }

    /// The pages read so far, once each, in order.
    pub fn visited(&self) -> Vec<String> {
        self.visited.lock().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn source(&self) -> &S {
        &self.src
    }
}

/// The addresses worth showing the person: web pages only, once each, at most [`MAX_HANDOVER`].
pub fn handover_list(visited: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for u in visited {
        if url::Url::parse(u).is_ok_and(|p| matches!(p.scheme(), "http" | "https")) && !out.contains(u) {
            out.push(u.clone());
        }
        if out.len() >= MAX_HANDOVER {
            break;
        }
    }
    out
}

impl<S: TabSource> Browser for TabsBrowser<S> {
    fn open<'a>(&'a self, url: &'a url::Url) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            if self.done.load(Ordering::SeqCst) {
                return Err(AppError::msg("the research browser has been closed"));
            }
            let (id, tab) = self.src.new_tab().await?;
            let tab: Arc<dyn Browser> = Arc::from(tab);
            match tab.open(url).await {
                Ok(()) => {
                    if let Ok(mut t) = self.tabs.lock() {
                        t.push(tab);
                    }
                    if let Ok(mut v) = self.visited.lock() {
                        v.push(url.to_string());
                    }
                    Ok(())
                }
                Err(e) => {
                    // A page that would not open leaves no empty tab behind.
                    self.src.close_tab(&id).await;
                    Err(e)
                }
            }
        })
    }

    fn call<'a>(&'a self, method: &'a str, args: Value) -> BoxFuture<'a, AppResult<Value>> {
        Box::pin(async move { self.current()?.call(method, args).await })
    }

    fn settle(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move { self.current()?.settle().await })
    }

    fn back(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move { self.current()?.back().await })
    }

    fn stopped(&self) -> bool {
        self.src.gone()
    }

    fn live_tabs(&self) -> bool {
        true
    }

    fn finish_job(&self, close_tabs: bool) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if self.done.swap(true, Ordering::SeqCst) {
                return;
            }
            let reopen = if close_tabs { vec![] } else { handover_list(&self.visited()) };
            if let Ok(mut t) = self.tabs.lock() {
                t.clear();
            }
            self.src.finish(reopen).await;
        })
    }

    fn capture(&self, kind: Capture) -> BoxFuture<'_, AppResult<Vec<u8>>> {
        Box::pin(async move { self.current()?.capture(kind).await })
    }
}

// ------------------------------------------------------------------ the real browser

/// Edge or Chrome started for this job.
pub struct EdgeTabs {
    port: u16,
    exe: std::path::PathBuf,
    /// The profile folder to reuse for the normal window afterwards (None: the program's default profile).
    profile: Option<std::path::PathBuf>,
    browser: super::cdp_ws::WsTransport,
    first: Mutex<Option<String>>,
    child: tokio::sync::Mutex<Option<tokio::process::Child>>,
}

/// Starts the browser visibly with a random loopback-only port and connects to it.
pub async fn launch(profile: &std::path::Path, own: bool) -> AppResult<TabsBrowser<EdgeTabs>> {
    let s = super::cdp_browser::start_browser(profile, own).await?;
    let version = super::cdp_ws::http_get_json(s.port, "/json/version").await?;
    let path = browser_path(&version, s.port).ok_or_else(|| AppError::msg("the browser gave no control address BYTE could trust"))?;
    let browser = super::cdp_ws::WsTransport::connect_tab(s.port, &path).await?;
    Ok(TabsBrowser::new(EdgeTabs { port: s.port, exe: s.exe, profile: (!own).then(|| profile.to_path_buf()), browser, first: Mutex::new(Some(s.page_path)), child: tokio::sync::Mutex::new(Some(s.child)) }))
}

/// The browser-level control address from `/json/version`, only if it points at this computer's own port.
pub fn browser_path(version: &Value, port: u16) -> Option<String> {
    let u = url::Url::parse(version["webSocketDebuggerUrl"].as_str()?).ok()?;
    let local = matches!(u.host_str(), Some("127.0.0.1" | "localhost"));
    (u.scheme() == "ws" && local && u.port() == Some(port) && u.path().starts_with("/devtools/browser/")).then(|| u.path().to_string())
}

/// The arguments for the normal window afterwards: no debugging port, and the addresses as tabs.
pub fn reopen_args(profile: Option<&std::path::Path>, urls: &[String]) -> Vec<String> {
    let mut a = vec!["--new-window".to_string(), "--no-first-run".to_string()];
    if let Some(p) = profile {
        a.push(format!("--user-data-dir={}", p.display()));
    }
    a.extend(urls.iter().cloned());
    a
}

impl EdgeTabs {
    async fn tab_at(&self, page_path: &str) -> AppResult<Box<dyn Browser>> {
        let ws = super::cdp_ws::WsTransport::connect_tab(self.port, page_path).await?;
        let b = CdpBrowser::new(ws, Timing::REAL, None);
        b.init().await;
        Ok(Box::new(b))
    }
}

impl TabSource for EdgeTabs {
    fn new_tab(&self) -> BoxFuture<'_, AppResult<(String, Box<dyn Browser>)>> {
        Box::pin(async move {
            let first = self.first.lock().ok().and_then(|mut f| f.take());
            if let Some(path) = first {
                let id = path.rsplit('/').next().unwrap_or("").to_string();
                return Ok((id, self.tab_at(&path).await?));
            }
            let r = self.browser.send("Target.createTarget", json!({ "url": "about:blank" })).await?;
            let id = r["targetId"].as_str().filter(|i| i.chars().all(|c| c.is_ascii_alphanumeric())).ok_or_else(|| AppError::msg("the browser did not open a new tab"))?.to_string();
            Ok((id.clone(), self.tab_at(&format!("/devtools/page/{id}")).await?))
        })
    }

    fn close_tab<'a>(&'a self, id: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let _ = self.browser.send("Target.closeTarget", json!({ "targetId": id })).await;
        })
    }

    fn gone(&self) -> bool {
        self.browser.closed()
    }

    fn finish<'a>(&'a self, reopen: Vec<String>) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // Detach first, then end the process: after this nothing listens on the debugging port.
            let _ = self.browser.send("Browser.close", json!({})).await;
            if let Some(mut child) = self.child.lock().await.take() {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
                let _ = child.kill().await;
            }
            if !reopen.is_empty() {
                // The profile folder is free again a moment after the process ends.
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                let _ = std::process::Command::new(&self.exe)
                    .args(reopen_args(self.profile.as_deref(), &reopen))
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        })
    }
}

#[cfg(test)]
#[path = "cdp_tabs_tests.rs"]
mod tests;
