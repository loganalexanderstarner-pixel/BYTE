//! The agent's browser: a hidden, private Tauri window (WKWebView on macOS,
//! WebKitGTK on Linux, WebView2 on Windows) that BYTE drives with the bridge
//! script. Commands go in with `eval`; replies come back as a navigation to
//! `byteagent://r/<id>?d=<json>` that the navigation handler catches and
//! cancels. Pages get no access to BYTE's commands (the window isn't in any
//! capability), and the window is destroyed when the answer ends.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::Value;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::{oneshot, watch};

use super::{allowed_host, Browser, Capture};
use crate::error::{AppError, AppResult};

pub const LABEL: &str = "byte-agent";
const BRIDGE: &str = include_str!("bridge.js");
/// A normal Safari, so sites serve their usual pages.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";
const LOAD_WAIT: Duration = Duration::from_secs(25);
const CALL_WAIT: Duration = Duration::from_secs(10);

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

/// Page loads started and finished so far.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Loads {
    started: u64,
    finished: u64,
}

pub struct TauriBrowser {
    win: WebviewWindow,
    pending: Pending,
    loads: watch::Receiver<Loads>,
    /// A page asked for a new window (target=_blank, window.open): opened here instead.
    popup: Arc<Mutex<Option<url::Url>>>,
    seq: AtomicU64,
}

/// A reply from the bridge: `byteagent://r/<id>?d=<json>`.
pub fn parse_reply(url: &url::Url) -> Option<(String, String)> {
    if url.scheme() != "byteagent" {
        return None;
    }
    // "byteagent://r/<id>": host "r", path "/<id>".
    if url.host_str() != Some("r") {
        return None;
    }
    let id = url.path().trim_start_matches('/');
    let id = percent_decode(id);
    let data = url.query_pairs().find(|(k, _)| k == "d").map(|(_, v)| v.into_owned())?;
    (!id.is_empty()).then_some((id, data))
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_default()
}

impl TauriBrowser {
    /// Opens the hidden window (one at a time: a second agent waits its turn).
    pub fn start(app: &AppHandle) -> AppResult<TauriBrowser> {
        if let Some(old) = app.get_webview_window(LABEL) {
            // Left over from an answer that ended abruptly.
            let _ = old.destroy();
        }
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, loads) = watch::channel(Loads::default());
        let tx = Arc::new(tx);
        let popup = Arc::new(Mutex::new(None));

        let nav_pending = pending.clone();
        let load_tx = tx.clone();
        let popup_slot = popup.clone();
        let blank = url::Url::parse("about:blank").map_err(|e| AppError::msg(e.to_string()))?;
        let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(blank))
            .title("BYTE is browsing")
            .inner_size(1280.0, 900.0)
            .visible(false)
            .focused(false)
            .incognito(true)
            .user_agent(USER_AGENT)
            .initialization_script(BRIDGE)
            .on_navigation(move |url| {
                if let Some((id, data)) = parse_reply(url) {
                    if let Some(tx) = nav_pending.lock().ok().and_then(|mut m| m.remove(&id)) {
                        let _ = tx.send(data);
                    }
                    return false;
                }
                match url.scheme() {
                    "about" | "blob" | "data" => true,
                    "http" | "https" => !crate::offline::is_offline() && allowed_host(url),
                    // javascript:, file:, mailto:, app links…
                    _ => false,
                }
            })
            .on_new_window(move |url, _features| {
                if allowed_host(&url) {
                    if let Ok(mut slot) = popup_slot.lock() {
                        *slot = Some(url);
                    }
                }
                NewWindowResponse::Deny
            })
            .on_page_load(move |_w, payload| {
                load_tx.send_modify(|l| match payload.event() {
                    PageLoadEvent::Started => l.started += 1,
                    PageLoadEvent::Finished => l.finished += 1,
                });
            })
            .build()
            .map_err(|e| AppError::msg(format!("couldn't open BYTE's browser: {e}")))?;
        Ok(TauriBrowser { win, pending, loads, popup, seq: AtomicU64::new(0) })
    }

    /// Waits until the page loads started so far have finished (or time runs out).
    async fn wait_loaded(&self, wait: Duration) {
        let mut rx = self.loads.clone();
        let _ = tokio::time::timeout(wait, rx.wait_for(|l| l.finished >= l.started)).await;
        // Let scripts draw the page.
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    async fn navigate(&self, url: url::Url) -> AppResult<()> {
        let before = *self.loads.borrow();
        self.win.navigate(url).map_err(|e| AppError::msg(e.to_string()))?;
        // Wait for this load to start, then to finish.
        let mut rx = self.loads.clone();
        let _ = tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|l| l.started > before.started)).await;
        self.wait_loaded(LOAD_WAIT).await;
        Ok(())
    }
}

impl Drop for TauriBrowser {
    fn drop(&mut self) {
        // The private session (cookies, storage) goes with the window.
        let _ = self.win.destroy();
    }
}

impl Browser for TauriBrowser {
    fn open<'a>(&'a self, url: &'a url::Url) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move { self.navigate(url.clone()).await })
    }

    fn call<'a>(&'a self, method: &'a str, args: Value) -> BoxFuture<'a, AppResult<Value>> {
        Box::pin(async move {
            let id = format!("c{}x{}", self.seq.fetch_add(1, Ordering::Relaxed), super::rand_u32());
            let (tx, rx) = oneshot::channel();
            self.pending.lock().map_err(|_| AppError::msg("browser state poisoned"))?.insert(id.clone(), tx);
            let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
            let js = format!(
                "(function(){{var i={id},m={m},a={a};if(window.__byteAgent){{window.__byteAgent.run(i,m,a);}}else{{location.href='byteagent://r/'+encodeURIComponent(i)+'?d='+encodeURIComponent(JSON.stringify({{ok:false,error:'The page is still loading.'}}));}}}})();",
                id = q(&id),
                m = q(method),
                a = args,
            );
            self.win.eval(js).map_err(|e| AppError::msg(e.to_string()))?;
            let reply = tokio::time::timeout(CALL_WAIT, rx).await;
            if let Ok(mut m) = self.pending.lock() {
                m.remove(&id);
            }
            match reply {
                Ok(Ok(data)) => serde_json::from_str(&data).map_err(|e| AppError::msg(format!("the page's reply wasn't readable: {e}"))),
                _ => Err(AppError::msg("the page didn't answer (it may still be loading)")),
            }
        })
    }

    fn settle(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move {
            let before = *self.loads.borrow();
            // A click may start a page load a moment later.
            tokio::time::sleep(Duration::from_millis(800)).await;
            let popup = self.popup.lock().ok().and_then(|mut p| p.take());
            if let Some(url) = popup {
                return self.navigate(url).await;
            }
            if self.loads.borrow().started > before.started {
                self.wait_loaded(LOAD_WAIT).await;
            }
            Ok(())
        })
    }

    fn back(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move {
            self.win.eval("history.back()").map_err(|e| AppError::msg(e.to_string()))?;
            self.settle().await
        })
    }

    fn capture(&self, kind: Capture) -> BoxFuture<'_, AppResult<Vec<u8>>> {
        Box::pin(async move {
            #[cfg(target_os = "macos")]
            {
                // A picture of the whole page: make the (hidden) window as tall as the page first.
                let restore = if kind == Capture::Png {
                    let size = self.call("size", serde_json::json!({})).await.ok().and_then(|v| v.get("value").cloned());
                    let h = size.as_ref().and_then(|s| s["height"].as_f64()).unwrap_or(900.0).clamp(600.0, 12000.0);
                    let old = self.win.inner_size().ok();
                    let scale = self.win.scale_factor().unwrap_or(2.0);
                    let _ = self.win.set_size(tauri::LogicalSize::new(1280.0, h));
                    tokio::time::sleep(Duration::from_millis(600)).await;
                    old.map(|o| o.to_logical::<f64>(scale))
                } else {
                    None
                };
                let (tx, rx) = oneshot::channel();
                self.win
                    .with_webview(move |wv| super::capture_mac::capture(wv.inner(), kind, tx))
                    .map_err(|e| AppError::msg(e.to_string()))?;
                let out = tokio::time::timeout(Duration::from_secs(60), rx).await;
                if let Some(o) = restore {
                    let _ = self.win.set_size(o);
                }
                match out {
                    Ok(Ok(r)) => r,
                    _ => Err(AppError::msg("saving the page took too long")),
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = kind;
                Err(AppError::msg("saving pictures and PDFs of pages needs macOS"))
            }
        })
    }
}

/// Shows or hides the agent's browser, if one is open (command `agent_show`).
pub fn show(app: &AppHandle, visible: bool) -> AppResult<bool> {
    match app.get_webview_window(LABEL) {
        Some(w) => {
            let r = if visible { w.show().and_then(|_| w.set_focus()) } else { w.hide() };
            r.map_err(|e| AppError::msg(e.to_string()))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_parsed_and_other_links_ignored() {
        let data = serde_json::json!({ "ok": true, "value": { "title": "Hi & bye ?#" } }).to_string();
        let raw = format!("byteagent://r/c1x42?d={}", url::form_urlencoded::byte_serialize(data.as_bytes()).collect::<String>());
        let u = url::Url::parse(&raw).unwrap();
        let (id, d) = parse_reply(&u).unwrap();
        assert_eq!(id, "c1x42");
        assert_eq!(serde_json::from_str::<Value>(&d).unwrap()["value"]["title"], "Hi & bye ?#");
        // JavaScript's encodeURIComponent output (%20 for spaces, not +).
        let u = url::Url::parse("byteagent://r/abc?d=%7B%22ok%22%3Atrue%2C%22value%22%3A%22a%20b%2Bc%22%7D").unwrap();
        assert_eq!(parse_reply(&u).unwrap().1, r#"{"ok":true,"value":"a b+c"}"#);
        for other in ["https://example.com/?d=1", "byteagent://x/abc?d=1", "byteagent://r/?d=1", "byteagent://r/abc"] {
            assert!(parse_reply(&url::Url::parse(other).unwrap()).is_none(), "{other}");
        }
    }
}

/// A real browser window (WebKitGTK) under a display: `xvfb-run cargo test
/// e2e_real_browser -- --ignored`. Checks the bridge round trip, typing,
/// the approval gate for submit buttons, and link clicks that load a page.
#[cfg(all(test, target_os = "linux"))]
mod e2e {
    use super::*;

    const FORM: &str = "<html><head><title>Pizza order</title></head><body><main><h1>Order a pizza</h1>\
        <form action='https://httpbin.org/post' method='post'><label>Your name <input name='custname'></label>\
        <label>Size <select name='size'><option>Small</option><option>Large</option></select></label>\
        <input type='password' name='pw' aria-label='Password'>\
        <button>Submit order</button></form><a href='https://example.com/'>Example link</a></main></body></html>";

    #[test]
    #[ignore]
    fn e2e_real_browser() {
        if std::env::var("DISPLAY").is_err() {
            eprintln!("no display: run under xvfb-run");
            return;
        }
        let app = tauri::Builder::default().any_thread().build(tauri::generate_context!()).expect("app");
        let handle = app.handle().clone();
        let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<String>, String>>();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r = rt.block_on(async {
                let b = TauriBrowser::start(&handle).map_err(|e| e.to_string())?;
                let mut notes = Vec::new();
                let data = format!("data:text/html;charset=utf-8,{}", url::form_urlencoded::byte_serialize(FORM.as_bytes()).collect::<String>().replace('+', "%20"));
                b.open(&url::Url::parse(&data).unwrap()).await.map_err(|e| e.to_string())?;
                let snap = b.call("snapshot", serde_json::json!({})).await.map_err(|e| e.to_string())?;
                notes.push(format!("snapshot: {snap}"));
                let els = snap["value"]["elements"].as_array().cloned().ok_or("no elements")?;
                let find = |label: &str| els.iter().find(|e| e["label"].as_str().unwrap_or("").contains(label)).and_then(|e| e["n"].as_u64()).ok_or(format!("no {label}"));
                let name = find("Your name")?;
                let typed = b.call("type", serde_json::json!({ "n": name, "text": "Ada" })).await.map_err(|e| e.to_string())?;
                notes.push(format!("typed: {typed}"));
                if typed["value"]["value"] != "Ada" {
                    return Err(format!("typing failed: {typed}"));
                }
                let pw = find("Password")?;
                let refused = b.call("type", serde_json::json!({ "n": pw, "text": "x" })).await.map_err(|e| e.to_string())?;
                if refused["ok"] != false {
                    return Err(format!("typed into a password field: {refused}"));
                }
                let submit = find("Submit order")?;
                let gate = b.call("click", serde_json::json!({ "n": submit })).await.map_err(|e| e.to_string())?;
                if gate["value"]["needsApproval"] != true {
                    return Err(format!("submit wasn't gated: {gate}"));
                }
                let info = b.call("formInfo", serde_json::json!({ "n": submit })).await.map_err(|e| e.to_string())?;
                notes.push(format!("form: {info}"));
                // A link click that loads a real page (needs the internet).
                let link = find("Example link")?;
                b.call("click", serde_json::json!({ "n": link })).await.map_err(|e| e.to_string())?;
                b.settle().await.map_err(|e| e.to_string())?;
                let after = b.call("snapshot", serde_json::json!({})).await.map_err(|e| e.to_string())?;
                notes.push(format!("after click: {} / {}", after["value"]["url"], after["value"]["title"]));
                Ok(notes)
            });
            let _ = tx.send(r);
            handle.exit(0);
        });
        let mut app = app;
        #[allow(deprecated)]
        loop {
            app.run_iteration(|_, _| {});
            if let Ok(r) = rx.try_recv() {
                let notes = r.expect("browser run");
                for n in &notes {
                    eprintln!("{n}");
                }
                assert!(notes.last().unwrap().contains("example.com"), "{notes:?}");
                return;
            }
        }
    }
}
