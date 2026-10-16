use std::sync::{Arc, Mutex};

use super::*;
use crate::web_agent::cdp::tests::FakeBrowser;
use crate::web_agent::cdp::Timing;

/// Hands out one scripted fake browser per tab and remembers what happened.
#[derive(Clone, Default)]
struct FakeSource {
    tabs: Arc<Mutex<Vec<FakeBrowser>>>,
    closed: Arc<Mutex<Vec<String>>>,
    finished: Arc<Mutex<Option<Vec<String>>>>,
    fail_next: Arc<Mutex<bool>>,
    gone: Arc<AtomicBool>,
}

impl TabSource for FakeSource {
    fn new_tab(&self) -> BoxFuture<'_, AppResult<(String, Box<dyn Browser>)>> {
        Box::pin(async move {
            let fake = FakeBrowser::new("about:blank");
            if std::mem::take(&mut *self.fail_next.lock().unwrap()) {
                fake.st.lock().unwrap().nav_error = Some("net::ERR_NAME_NOT_RESOLVED".into());
            }
            let id = format!("tab{}", self.tabs.lock().unwrap().len() + 1);
            self.tabs.lock().unwrap().push(fake.clone());
            Ok((id, Box::new(CdpBrowser::new(fake, Timing::INSTANT, None)) as Box<dyn Browser>))
        })
    }
    fn close_tab<'a>(&'a self, id: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.closed.lock().unwrap().push(id.to_string()) })
    }
    fn gone(&self) -> bool {
        self.gone.load(Ordering::SeqCst)
    }
    fn finish<'a>(&'a self, reopen: Vec<String>) -> BoxFuture<'a, ()> {
        Box::pin(async move { *self.finished.lock().unwrap() = Some(reopen) })
    }
}

fn url(s: &str) -> url::Url {
    url::Url::parse(s).unwrap()
}

#[tokio::test]
async fn each_page_opens_in_its_own_new_tab_and_the_last_one_is_current() {
    let src = FakeSource::default();
    let b = TabsBrowser::new(src.clone());
    b.open(&url("https://a.example/one")).await.unwrap();
    b.open(&url("https://b.example/two")).await.unwrap();
    b.open(&url("https://c.example/three")).await.unwrap();
    let tabs = src.tabs.lock().unwrap();
    assert_eq!(tabs.len(), 3, "three pages, three tabs");
    assert_eq!(tabs[0].calls("Page.navigate")[0]["url"], "https://a.example/one");
    assert_eq!(tabs[2].calls("Page.navigate")[0]["url"], "https://c.example/three");
    assert!(tabs[0].calls("Page.navigate").len() == 1 && tabs[1].calls("Page.navigate").len() == 1, "no tab is reused");
    drop(tabs);
    assert_eq!(b.visited(), ["https://a.example/one", "https://b.example/two", "https://c.example/three"]);
    assert!(b.live_tabs());
    // Reading goes to the newest tab only.
    b.settle().await.unwrap();
    assert!(!src.tabs.lock().unwrap()[2].calls("Page.navigate").is_empty());
}

#[tokio::test]
async fn the_site_rules_still_apply_in_a_new_tab_and_a_failed_page_leaves_no_tab() {
    let src = FakeSource::default();
    let b = TabsBrowser::new(src.clone());
    // A bank is refused before the tab navigates anywhere.
    let e = b.open(&url("https://www.chase.com/login")).await.unwrap_err();
    assert!(!e.to_string().is_empty());
    assert!(src.tabs.lock().unwrap()[0].calls("Page.navigate").is_empty(), "the forbidden page was never loaded");
    assert_eq!(src.closed.lock().unwrap().as_slice(), ["tab1"], "the empty tab is closed again");
    assert!(b.visited().is_empty());
    // A page that cannot load is closed too.
    *src.fail_next.lock().unwrap() = true;
    assert!(b.open(&url("https://nowhere.example/")).await.is_err());
    assert_eq!(src.closed.lock().unwrap().len(), 2);
    // Local addresses are never opened.
    assert!(b.open(&url("http://127.0.0.1:8080/")).await.is_err());
}

#[tokio::test]
async fn finishing_detaches_once_and_leaves_the_pages_for_the_person_unless_told_to_close_them() {
    let src = FakeSource::default();
    let b = TabsBrowser::new(src.clone());
    b.open(&url("https://a.example/one")).await.unwrap();
    b.open(&url("https://b.example/two")).await.unwrap();
    b.finish_job(false).await;
    assert_eq!(src.finished.lock().unwrap().clone().unwrap(), ["https://a.example/one", "https://b.example/two"]);
    // Afterwards nothing can be driven any more.
    assert!(b.open(&url("https://c.example/")).await.is_err());
    assert!(b.settle().await.is_err());
    // A second finish does nothing.
    *src.finished.lock().unwrap() = None;
    b.finish_job(false).await;
    assert!(src.finished.lock().unwrap().is_none());

    let src2 = FakeSource::default();
    let b2 = TabsBrowser::new(src2.clone());
    b2.open(&url("https://a.example/one")).await.unwrap();
    b2.finish_job(true).await;
    assert_eq!(src2.finished.lock().unwrap().clone().unwrap(), Vec::<String>::new(), "close the tabs: nothing is reopened");
}

#[tokio::test]
async fn the_person_closing_the_browser_counts_as_stop() {
    let src = FakeSource::default();
    let b = TabsBrowser::new(src.clone());
    assert!(!b.stopped());
    src.gone.store(true, Ordering::SeqCst);
    assert!(b.stopped());
}

#[test]
fn the_handover_list_is_web_pages_once_and_capped() {
    let many: Vec<String> = (0..40).map(|i| format!("https://s{i}.example/")).collect();
    assert_eq!(handover_list(&many).len(), MAX_HANDOVER);
    let l = handover_list(&["https://a.example/".into(), "file:///etc/passwd".into(), "javascript:alert(1)".into(), "https://a.example/".into(), "about:blank".into()]);
    assert_eq!(l, ["https://a.example/"]);
}

#[test]
fn the_normal_window_afterwards_has_no_debugging_port() {
    let urls = vec!["https://a.example/".to_string()];
    let a = reopen_args(Some(std::path::Path::new("/p/byte-browser-profile")), &urls);
    assert!(a.iter().all(|x| !x.contains("remote-debugging")), "{a:?}");
    assert!(a.contains(&"--new-window".to_string()) && a.contains(&"--user-data-dir=/p/byte-browser-profile".to_string()));
    assert_eq!(a.last().unwrap(), "https://a.example/");
    let own = reopen_args(None, &urls);
    assert!(!own.iter().any(|x| x.starts_with("--user-data-dir")), "the person's own profile is the program's default");
}

#[test]
fn only_a_control_address_on_our_own_loopback_port_is_trusted() {
    let ok = json!({ "webSocketDebuggerUrl": "ws://127.0.0.1:9222/devtools/browser/abc-123" });
    assert_eq!(browser_path(&ok, 9222).unwrap(), "/devtools/browser/abc-123");
    assert!(browser_path(&ok, 9333).is_none(), "a different port");
    assert!(browser_path(&json!({ "webSocketDebuggerUrl": "ws://evil.example:9222/devtools/browser/x" }), 9222).is_none());
    assert!(browser_path(&json!({ "webSocketDebuggerUrl": "ws://127.0.0.1:9222/devtools/page/x" }), 9222).is_none());
    assert!(browser_path(&json!({}), 9222).is_none());
}

#[test]
fn in_a_research_tab_other_pages_are_not_closed_but_elsewhere_they_still_are() {
    let ev = json!({ "targetInfo": { "targetId": "T2", "type": "page" } });
    assert!(crate::web_agent::cdp::react(crate::web_agent::cdp::ANY_TAB, "Target.targetCreated", &ev).is_empty());
    assert_eq!(crate::web_agent::cdp::react("T1", "Target.targetCreated", &ev).len(), 1);
    // The site rules are still enforced on every tab.
    let paused = json!({ "requestId": "r1", "request": { "url": "https://www.chase.com/login" } });
    assert_eq!(crate::web_agent::cdp::react(crate::web_agent::cdp::ANY_TAB, "Fetch.requestPaused", &paused)[0].0, "Fetch.failRequest");
}
