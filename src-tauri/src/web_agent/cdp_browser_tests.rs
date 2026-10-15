use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::chat::ChatEvent;
use crate::tools::SourceBook;
use crate::web_agent::cdp::Timing;
use crate::web_agent::cdp::tests::{contact_browser, ClickMode, FakeBrowser};
use crate::web_agent::{answer, Session, CLICK, OPEN_URL, TYPE_TEXT};

fn session(b: &FakeBrowser) -> (Session, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new(Box::new(CdpBrowser::new(b.clone(), Timing::INSTANT, None)), dir.path().to_path_buf());
    s.approval_wait = Duration::from_secs(5);
    (s, dir)
}

fn events() -> (Arc<Mutex<Vec<ChatEvent>>>, impl Fn(ChatEvent) -> AppResult<()> + Sync) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    (seen, move |e: ChatEvent| {
        s.lock().unwrap().push(e);
        Ok(())
    })
}

/// Answers the next approval card.
fn answer_next(seen: &Arc<Mutex<Vec<ChatEvent>>>, ok: bool) -> tokio::task::JoinHandle<()> {
    let seen = seen.clone();
    tokio::spawn(async move {
        loop {
            let id = seen.lock().unwrap().iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.id.clone()) } else { None });
            if let Some(id) = id {
                answer(&id, ok);
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
}

#[tokio::test]
async fn the_agent_session_runs_on_the_real_browser_with_its_usual_safety() {
    let b = contact_browser();
    b.st.lock().unwrap().url = "https://example.com/contact".into();
    let (mut s, _d) = session(&b);
    let mut book = SourceBook::default();
    let (seen, send) = events();
    let c = CancellationToken::new();

    let step = s.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();
    assert!(step.ok, "{step:?}");
    assert!(step.content.contains("[1] input \"Name\""), "{}", step.content);
    assert!(step.content.contains("(private"), "{}", step.content);

    // Private fields: refused by the session before the browser is asked.
    let step = s.run(TYPE_TEXT, &json!({ "n": 2, "text": "hunter2" }), &mut book, &c, &send).await.unwrap();
    assert!(!step.ok);
    assert!(b.calls("Input.insertText").is_empty());
    // Typing in a normal box works, and the activity line has no typed text.
    let (_, what) = s.describe(TYPE_TEXT, &json!({ "n": 1, "text": "Ada" }));
    assert_eq!(what, "Typed into \u{201c}Name\u{201d}");
    let step = s.run(TYPE_TEXT, &json!({ "n": 1, "text": "Ada" }), &mut book, &c, &send).await.unwrap();
    assert!(step.ok, "{step:?}");

    // The submit button asks first; Deny leaves it unpressed and stops the browsing.
    let answerer = answer_next(&seen, false);
    let step = s.run(CLICK, &json!({ "n": 4 }), &mut book, &c, &send).await.unwrap();
    answerer.await.unwrap();
    assert!(!step.ok);
    assert!(step.content.contains("declined"));
    assert!(b.calls("Runtime.callFunctionOn").iter().all(|c| c["functionDeclaration"] != crate::web_agent::cdp::CLICK_FN_FOR_TESTS));
    assert_eq!(s.steps_left(), 0);
}

#[tokio::test]
async fn approving_presses_the_button_and_a_dead_click_is_reported() {
    let b = contact_browser();
    let (mut s, _d) = session(&b);
    let mut book = SourceBook::default();
    let (seen, send) = events();
    let c = CancellationToken::new();
    s.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();
    let answerer = answer_next(&seen, true);
    let step = s.run(CLICK, &json!({ "n": 4 }), &mut book, &c, &send).await.unwrap();
    answerer.await.unwrap();
    assert!(step.ok, "{step:?}");
    // The first click changes the page (fake script mode), so it is not reported as dead.
    assert!(!step.summary.contains("nothing seemed to change"));

    let b2 = contact_browser();
    *b2.mode.lock().unwrap() = ClickMode::Dead;
    let (mut s2, _d2) = session(&b2);
    s2.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();
    let step = s2.run(CLICK, &json!({ "n": 5 }), &mut book, &c, &send).await.unwrap();
    assert!(step.ok);
    assert!(step.summary.contains("nothing seemed to change"), "{}", step.summary);
}

#[tokio::test]
async fn closing_the_browser_window_stops_the_session() {
    let b = contact_browser();
    let (mut s, _d) = session(&b);
    let mut book = SourceBook::default();
    let (_seen, send) = events();
    let c = CancellationToken::new();
    assert!(s.run(OPEN_URL, &json!({ "url": "https://example.com/" }), &mut book, &c, &send).await.unwrap().ok);
    b.closed.store(true, std::sync::atomic::Ordering::SeqCst);
    let step = s.run(OPEN_URL, &json!({ "url": "https://example.com/two" }), &mut book, &c, &send).await.unwrap();
    assert!(!step.ok);
    assert_eq!(s.steps_left(), 0);
}

#[test]
fn only_a_folder_of_our_own_is_ever_used_as_the_profile() {
    let ok = Path::new("/home/x/.local/share/app.byte/byte-browser-profile");
    assert!(is_safe_profile_dir(ok));
    assert!(is_safe_profile_dir(Path::new("C:\\Users\\x\\AppData\\Roaming\\app.byte\\byte-browser-profile")));
    for bad in [
        "C:\\Users\\x\\AppData\\Local\\Microsoft\\Edge\\User Data",
        "C:\\Users\\x\\AppData\\Local\\Google\\Chrome\\User Data\\Default",
        "/home/x/.config/google-chrome",
        "/home/x/.config/chromium/byte-browser-profile",
        "/Users/x/Library/Application Support/Google/Chrome/byte-browser-profile",
        "/home/x/byte-browser-profile/Default",
        "/home/x/other",
        "",
    ] {
        assert!(!is_safe_profile_dir(Path::new(bad)), "{bad}");
    }
}

#[test]
fn the_launch_command_line_is_local_and_uses_the_given_profile() {
    let args = launch_args(41234, Path::new("/data/byte-browser-profile"));
    assert!(args.contains(&"--remote-debugging-port=41234".to_string()));
    assert!(args.contains(&"--remote-debugging-address=127.0.0.1".to_string()));
    assert!(args.contains(&"--user-data-dir=/data/byte-browser-profile".to_string()));
    assert!(!args.iter().any(|a| a.contains("remote-allow-origins")), "no browser page may attach");
    assert!(!args.iter().any(|a| a.contains("0.0.0.0")));
}

#[test]
fn the_port_is_free_local_and_not_always_the_same() {
    let a = free_port().unwrap();
    assert!(a > 1024);
    // Binding it again straight away works (it was released), and a second pick differs from a held port.
    let held = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    assert_ne!(free_port().unwrap(), held.local_addr().unwrap().port());
}

#[test]
fn only_a_page_on_our_own_local_port_is_driven() {
    let list = json!([
        { "type": "background_page", "webSocketDebuggerUrl": "ws://127.0.0.1:9333/devtools/page/BG" },
        { "type": "page", "webSocketDebuggerUrl": "ws://evil.example.com:9333/devtools/page/X" },
        { "type": "page", "webSocketDebuggerUrl": "ws://127.0.0.1:9444/devtools/page/Y" },
        { "type": "page", "webSocketDebuggerUrl": "wss://127.0.0.1:9333/devtools/page/Z" },
        { "type": "page", "webSocketDebuggerUrl": "ws://127.0.0.1:9333/devtools/page/GOOD" },
    ]);
    assert_eq!(pick_page(&list, 9333).as_deref(), Some("/devtools/page/GOOD"));
    assert_eq!(pick_page(&json!([]), 9333), None);
    assert_eq!(pick_page(&json!({}), 9333), None);
}

#[test]
fn candidates_name_edge_and_chrome() {
    let c = browser_candidates();
    assert!(!c.is_empty());
    let all = c.iter().map(|p| p.to_string_lossy().to_lowercase()).collect::<Vec<_>>().join("|");
    assert!(all.contains("edge") && all.contains("chrome"));
}

/// Needs Edge or Chrome installed and a screen: `cargo test --lib real_browser_cdp_round_trip -- --ignored`.
/// Starts the browser with BYTE's own temporary profile, opens a data: page and reads its marks.
#[tokio::test]
#[ignore]
async fn real_browser_cdp_round_trip() {
    let dir = std::env::temp_dir().join(PROFILE_NAME);
    let b = launch(&dir).await.expect("start edge/chrome");
    let d = b.driver();
    d.t.send("Page.navigate", json!({ "url": "data:text/html,<title>Hi</title><button>Go</button><input aria-label=Name>" })).await.unwrap();
    d.wait_loaded().await;
    let snap = d.snapshot(0, 2000).await.unwrap();
    let els = snap["elements"].as_array().unwrap();
    assert!(els.iter().any(|e| e["label"] == "Go"), "{snap}");
    let name = els.iter().find(|e| e["label"] == "Name").unwrap()["n"].as_u64().unwrap() as u32;
    let o = d.type_text(name, "Ada").await.unwrap();
    assert!(o.verified);
    drop(b);
    let _ = std::fs::remove_dir_all(&dir);
}
