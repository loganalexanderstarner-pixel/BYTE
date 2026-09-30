//! BYTE in the background (Phase 10): opening at login, staying open when the
//! window is closed, and `byte://` links (from Shortcuts).
//!
//! - **Open at login** uses a LaunchAgent (tauri-plugin-autostart) that starts
//!   BYTE with `--background`: no window, so schedules, watchers and "when BYTE
//!   opens" automations just run. Clicking BYTE in the Dock shows the window.
//! - **Keep running** (macOS): closing the window hides it instead of quitting;
//!   ⌘Q quits. Without a Dock icon to bring it back, other systems quit as before.
//! - **Links**: `byte://run/<id>?key=<key>` runs an automation (only with its
//!   own random key, so a web page can't start it); `byte://ask?q=…` shows the
//!   window with the question in the message box, and never sends it.

use serde::Serialize;
use tauri::{AppHandle, Manager, Url};

use crate::state::AppState;

pub const ASK_EVENT: &str = "deeplink://ask";

/// Started by the login item.
pub fn launched_in_background() -> bool {
    std::env::args().any(|a| a == "--background")
}

/// Turns the login item on or off.
pub fn apply_login(app: &AppHandle, on: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let auto = app.autolaunch();
    let now = auto.is_enabled().unwrap_or(false);
    let result = match (on, now) {
        (true, false) => auto.enable(),
        (false, true) => auto.disable(),
        _ => Ok(()),
    };
    result.map_err(|e| format!("Couldn't change opening at login: {e}"))
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// What a `byte://` link asks for.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Link {
    Run { id: i64, key: String },
    Ask { text: String },
}

pub fn read_link(url: &Url) -> Option<Link> {
    if url.scheme() != "byte" {
        return None;
    }
    let param = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.to_string());
    match url.host_str()? {
        "run" => {
            let id = url.path().trim_matches('/').parse::<i64>().ok().filter(|id| *id > 0)?;
            let key = param("key").filter(|k| !k.is_empty())?;
            Some(Link::Run { id, key })
        }
        "ask" => {
            let text: String = param("q").unwrap_or_default().chars().take(4000).collect();
            Some(Link::Ask { text })
        }
        _ => None,
    }
}

/// Handles links from Shortcuts (or anywhere else).
pub fn open_links(app: &AppHandle, urls: Vec<Url>) {
    for url in urls {
        match read_link(&url) {
            Some(Link::Run { id, key }) => {
                let state = app.state::<AppState>();
                if !state.settings.blocking_lock().automations_enabled {
                    crate::scheduler::notify(app, "Automations are off", "Turn them on in BYTE → Settings → Features.");
                    continue;
                }
                match crate::automations::by_link(&state.db, id, &key) {
                    Ok(Some(a)) => {
                        let name = a.name.clone();
                        match crate::automations::start(app, a, 0) {
                            Ok(_) => crate::scheduler::notify(app, &format!("Running \"{name}\""), "BYTE will let you know when it's done."),
                            Err(e) => log::warn!("automation from a link: {e}"),
                        }
                    }
                    Ok(None) => log::warn!("a byte:// link named automation {id} with a key that doesn't match; ignored"),
                    Err(e) => log::warn!("automation link: {e}"),
                }
            }
            Some(Link::Ask { text }) => {
                show_main(app);
                let _ = tauri::Emitter::emit(app, ASK_EVENT, text);
            }
            None => log::info!("ignored link {url}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(s: &str) -> Option<Link> {
        read_link(&Url::parse(s).unwrap())
    }

    #[test]
    fn links_are_read_strictly() {
        assert_eq!(link("byte://run/7?key=abc"), Some(Link::Run { id: 7, key: "abc".into() }));
        assert_eq!(link("byte://run/7"), None, "no key, no run");
        assert_eq!(link("byte://run/7?key="), None);
        assert_eq!(link("byte://run/x?key=abc"), None);
        assert_eq!(link("byte://run/-1?key=abc"), None);
        assert_eq!(link("byte://ask?q=what%27s%20the%20weather"), Some(Link::Ask { text: "what's the weather".into() }));
        assert_eq!(link("byte://delete/everything"), None);
        assert_eq!(link("https://run/7?key=abc"), None);
    }
}
