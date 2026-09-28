use std::sync::{Arc, Mutex};

use super::*;

/// A scripted browser: each page is a snapshot; calls are recorded.
#[derive(Clone, Default)]
struct Fake {
    pages: Arc<Mutex<HashMap<String, Value>>>,
    current: Arc<Mutex<String>>,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// Where clicking element n goes.
    links: Arc<Mutex<HashMap<u32, String>>>,
}

impl Fake {
    fn page(self, url: &str, snap: Value) -> Self {
        self.pages.lock().unwrap().insert(url.into(), snap);
        self
    }
    fn link(self, n: u32, to: &str) -> Self {
        self.links.lock().unwrap().insert(n, to.into());
        self
    }
    fn called(&self, method: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(m, _)| m == method).map(|(_, a)| a.clone()).collect()
    }
}

impl Browser for Fake {
    fn open<'a>(&'a self, url: &'a url::Url) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            *self.current.lock().unwrap() = url.to_string();
            Ok(())
        })
    }
    fn call<'a>(&'a self, method: &'a str, args: Value) -> BoxFuture<'a, AppResult<Value>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((method.to_string(), args.clone()));
            let cur = self.current.lock().unwrap().clone();
            let v = match method {
                "snapshot" => self.pages.lock().unwrap().get(&cur).cloned().unwrap_or(json!({ "url": cur, "title": "Blank" })),
                "formInfo" => json!({ "button": "Send", "action": "https://example.com/contact", "method": "post", "fields": [{ "label": "Name", "value": "Ada" }] }),
                "click" => {
                    let n = args["n"].as_u64().unwrap() as u32;
                    if let Some(to) = self.links.lock().unwrap().get(&n) {
                        *self.current.lock().unwrap() = to.clone();
                    }
                    json!({ "label": "x" })
                }
                "type" => json!({ "label": "Name", "value": args["text"] }),
                "choose" => json!({ "label": "Branch", "value": "Main" }),
                _ => json!({}),
            };
            Ok(json!({ "ok": true, "value": v }))
        })
    }
    fn settle(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn back(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn capture(&self, _kind: Capture) -> BoxFuture<'_, AppResult<Vec<u8>>> {
        Box::pin(async { Err(AppError::msg("not here")) })
    }
}

fn contact_page() -> Value {
    json!({
        "url": "https://example.com/contact", "title": "Contact us", "text": "Write to us.", "textTotal": 12,
        "elements": [
            { "n": 1, "kind": "input", "label": "Name", "type": "text", "value": "" },
            { "n": 2, "kind": "input", "label": "Password", "type": "password", "sensitive": true },
            { "n": 3, "kind": "input", "label": "Card number", "type": "text" },
            { "n": 4, "kind": "button", "label": "Send", "commits": true },
            { "n": 5, "kind": "link", "label": "Opening hours", "href": "https://example.com/hours" },
            { "n": 6, "kind": "select", "label": "Branch", "options": ["Main", "East"], "value": "Main" }
        ],
        "forms": 1
    })
}

fn events() -> (Arc<Mutex<Vec<ChatEvent>>>, impl Fn(ChatEvent) -> AppResult<()> + Sync) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    (seen, move |e: ChatEvent| {
        s.lock().unwrap().push(e);
        Ok(())
    })
}

fn session(fake: &Fake) -> (Session, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new(Box::new(fake.clone()), dir.path().to_path_buf());
    s.approval_wait = Duration::from_secs(5);
    (s, dir)
}

#[tokio::test]
async fn opens_a_page_and_shows_it_numbered() {
    let fake = Fake::default().page("https://example.com/contact", contact_page());
    let (mut s, _d) = session(&fake);
    let mut book = SourceBook::default();
    let (_seen, send) = events();
    let step = s.run(OPEN_URL, &json!({ "url": "example.com/contact" }), &mut book, &CancellationToken::new(), &send).await.unwrap();
    assert!(step.ok, "{step:?}");
    assert_eq!(step.summary, "Opened example.com — Contact us");
    assert!(step.content.starts_with("Page [1]: Contact us\nURL: https://example.com/contact"));
    assert!(step.content.contains("[1] input \"Name\""));
    assert!(step.content.contains("[2] input (password) \"Password\" (private"));
    assert!(step.content.contains("[3] input \"Card number\" (private"), "{}", step.content);
    assert!(step.content.contains("[4] button \"Send\" (asks the user first)"));
    assert!(step.content.contains("→ example.com/hours"));
    assert!(step.content.contains("options: Main | East"));
    assert_eq!(book.sources.len(), 1);
    assert!(book.sources[0].read);
}

#[tokio::test]
async fn refuses_private_sites_and_private_fields() {
    let fake = Fake::default().page("https://example.com/contact", contact_page());
    let (mut s, _d) = session(&fake);
    let mut book = SourceBook::default();
    let (_seen, send) = events();
    let c = CancellationToken::new();
    for bad in ["http://localhost:1420/", "http://192.168.1.1/admin", "file:///etc/passwd", "http://printer.local/"] {
        let step = s.run(OPEN_URL, &json!({ "url": bad }), &mut book, &c, &send).await.unwrap();
        assert!(!step.ok, "{bad}");
    }
    assert!(fake.called("snapshot").is_empty());
    s.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();
    for n in [2, 3] {
        let step = s.run(TYPE_TEXT, &json!({ "n": n, "text": "hunter2" }), &mut book, &c, &send).await.unwrap();
        assert!(!step.ok);
        assert!(step.content.contains("never types into private fields"), "{}", step.content);
    }
    assert!(fake.called("type").is_empty(), "nothing was typed");
    let step = s.run(TYPE_TEXT, &json!({ "n": "1", "text": "Ada" }), &mut book, &c, &send).await.unwrap();
    assert!(step.ok);
    assert_eq!(step.summary, "Typed \u{201c}Ada\u{201d} into Name");
}

#[tokio::test]
async fn submitting_waits_for_approval_and_denial_stops_it() {
    let fake = Fake::default().page("https://example.com/contact", contact_page());
    let (mut s, _d) = session(&fake);
    let mut book = SourceBook::default();
    let (seen, send) = events();
    let c = CancellationToken::new();
    s.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();

    // Deny: the card appears, the answer comes back, nothing is clicked.
    let seen2 = seen.clone();
    let answerer = tokio::spawn(async move {
        loop {
            let id = seen2.lock().unwrap().iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.id.clone()) } else { None });
            if let Some(id) = id {
                assert!(answer(&id, false));
                return id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let step = s.run(CLICK, &json!({ "n": 4 }), &mut book, &c, &send).await.unwrap();
    let id = answerer.await.unwrap();
    assert!(!step.ok);
    assert!(step.content.contains("The user declined"));
    assert!(fake.called("click").is_empty());
    {
        let ev = seen.lock().unwrap();
        let card = ev.iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.clone()) } else { None }).unwrap();
        assert_eq!(card.action, "submit");
        assert_eq!(card.title, "Submit the form on example.com?");
        assert_eq!(card.fields, vec![Field { label: "Name".into(), value: "Ada".into() }]);
        assert!(ev.iter().any(|e| matches!(e, ChatEvent::ApprovalDone { id: i, ok: false } if *i == id)));
    }
    // A card that's been answered can't be answered again.
    assert!(!answer(&id, true));
    // After a Deny, the browser stops for this answer (no asking again and again).
    assert_eq!(s.steps_left(), 0);
    let again = s.run(CLICK, &json!({ "n": 4 }), &mut book, &c, &send).await.unwrap();
    assert!(!again.ok);
    assert!(again.content.contains("Stopped: the user declined"));
    assert_eq!(seen.lock().unwrap().iter().filter(|e| matches!(e, ChatEvent::Approval(_))).count(), 1);
    s.declined = false;

    // Approve: now it's clicked, with approved: true.
    seen.lock().unwrap().clear();
    let seen3 = seen.clone();
    tokio::spawn(async move {
        loop {
            let id = seen3.lock().unwrap().iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.id.clone()) } else { None });
            if let Some(id) = id {
                answer(&id, true);
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let step = s.run(CLICK, &json!({ "n": 4 }), &mut book, &c, &send).await.unwrap();
    assert!(step.ok, "{step:?}");
    assert_eq!(fake.called("click"), vec![json!({ "n": 4, "approved": true })]);
}

#[tokio::test]
async fn unanswered_cards_time_out_as_declined() {
    let fake = Fake::default().page("https://example.com/contact", contact_page());
    let (mut s, _d) = session(&fake);
    s.approval_wait = Duration::from_millis(50);
    let mut book = SourceBook::default();
    let (seen, send) = events();
    let c = CancellationToken::new();
    s.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();
    let step = s.run(CLICK, &json!({ "n": 4 }), &mut book, &c, &send).await.unwrap();
    assert!(!step.ok);
    assert!(fake.called("click").is_empty());
    assert!(seen.lock().unwrap().iter().any(|e| matches!(e, ChatEvent::ApprovalDone { ok: false, .. })));
}

#[tokio::test]
async fn stopping_the_answer_while_a_card_waits_cancels() {
    let fake = Fake::default().page("https://example.com/contact", contact_page());
    let (mut s, _d) = session(&fake);
    let mut book = SourceBook::default();
    let (_seen, send) = events();
    let c = CancellationToken::new();
    s.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();
    let c2 = c.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        c2.cancel();
    });
    let r = s.run(CLICK, &json!({ "n": 4 }), &mut book, &c, &send).await;
    assert!(matches!(r, Err(AppError::Cancelled)), "{r:?}");
}

#[tokio::test]
async fn ordinary_links_are_clicked_without_asking() {
    let hours = json!({ "url": "https://example.com/hours", "title": "Opening hours", "text": "Mon-Fri 9-5", "textTotal": 11, "elements": [] });
    let fake = Fake::default().page("https://example.com/contact", contact_page()).page("https://example.com/hours", hours).link(5, "https://example.com/hours");
    let (mut s, _d) = session(&fake);
    let mut book = SourceBook::default();
    let (seen, send) = events();
    let c = CancellationToken::new();
    s.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();
    let step = s.run(CLICK, &json!({ "n": 5 }), &mut book, &c, &send).await.unwrap();
    assert!(step.ok);
    assert_eq!(step.summary, "Clicked \u{201c}Opening hours\u{201d} → Opening hours");
    assert!(step.content.contains("Mon-Fri 9-5"));
    assert!(step.content.contains("Nothing to click"));
    assert!(!seen.lock().unwrap().iter().any(|e| matches!(e, ChatEvent::Approval(_))));
    // Numbers from an old view don't work once the page changed.
    let step = s.run(CLICK, &json!({ "n": 4 }), &mut book, &c, &send).await.unwrap();
    assert!(!step.ok);
    assert!(step.content.contains("no element 4"));
    assert_eq!(book.sources.len(), 2);
}

#[tokio::test]
async fn steps_are_limited() {
    let fake = Fake::default().page("https://example.com/contact", contact_page());
    let (mut s, _d) = session(&fake);
    let mut book = SourceBook::default();
    let (_seen, send) = events();
    let c = CancellationToken::new();
    for _ in 0..MAX_STEPS {
        s.run(READ_PAGE_AGAIN, &json!({}), &mut book, &c, &send).await.unwrap();
    }
    assert_eq!(s.steps_left(), 0);
    let step = s.run(READ_PAGE_AGAIN, &json!({}), &mut book, &c, &send).await.unwrap();
    assert!(!step.ok);
    assert!(step.content.contains("No more browser steps"));
}

#[tokio::test]
async fn saving_a_page_falls_back_to_text_off_macos() {
    let fake = Fake::default().page("https://example.com/contact", contact_page());
    let (mut s, dir) = session(&fake);
    let mut book = SourceBook::default();
    let (seen, send) = events();
    let c = CancellationToken::new();
    s.run(OPEN_URL, &json!({ "url": "https://example.com/contact" }), &mut book, &c, &send).await.unwrap();
    let step = s.run(SAVE_PAGE, &json!({ "format": "pdf" }), &mut book, &c, &send).await.unwrap();
    assert!(step.ok, "{step:?}");
    let saved = dir.path().join("Contact us.md");
    assert!(std::fs::read_to_string(&saved).unwrap().contains("Write to us."));
    assert!(seen.lock().unwrap().iter().any(|e| matches!(e, ChatEvent::Saved(f) if f.format == "text")));
    // A second save doesn't overwrite the first.
    s.run(SAVE_PAGE, &json!({}), &mut book, &c, &send).await.unwrap();
    assert!(dir.path().join("Contact us (2).md").exists());
}

#[test]
fn older_page_views_are_compacted() {
    let mut m = vec![
        json!({ "role": "tool", "content": "Page [1]: A\nURL: https://a.com\n\nText…\n[1] link \"x\"" }),
        json!({ "role": "tool", "content": "Search results for \"x\"" }),
        json!({ "role": "tool", "content": "Page [2]: B\nURL: https://b.com\n\nText…" }),
    ];
    compact(&mut m);
    assert_eq!(m[0]["content"], "Page [1]: A\nURL: https://a.com\n(an earlier view of the page; its numbers no longer apply)");
    assert_eq!(m[1]["content"], "Search results for \"x\"");
    assert!(m[2]["content"].as_str().unwrap().ends_with("Text…"));
}

#[test]
fn which_questions_use_the_browser() {
    for yes in [
        "Go to example.com and find the opening hours",
        "open https://httpbin.org/forms/post and fill in the pizza order form for Ada",
        "Download the manual from https://example.com/manual.pdf",
        "take a screenshot of https://example.com",
        "fill out the contact form on the website of Carnegie Library",
        "visit carnegielibrary.org and check when the Squirrel Hill branch opens",
        "Save this page as a PDF: https://example.com/article",
    ] {
        assert!(wants_web_agent(yes), "{yes}");
    }
    for no in [
        "what's the weather in Pittsburgh",
        "open questions in physics",
        "how do I download a website",
        "download speed test results explained",
        "summarize https://example.com/article",
        "is example.com a good site?",
        "what is the capital of France",
        "please open report.pdf",
    ] {
        assert!(!wants_web_agent(no), "{no}");
    }
}

#[test]
fn links_and_domains_are_found_in_messages() {
    assert_eq!(url_in("go to example.com/hours, then tell me").unwrap().as_str(), "https://example.com/hours");
    assert_eq!(url_in("Open (https://a.b.org/x?y=1).").unwrap().as_str(), "https://a.b.org/x?y=1");
    assert!(url_in("mail ada@example.com").is_none());
    assert!(url_in("open report.pdf").is_none());
    assert!(url_in("version 1.2.3 is out").is_none());
    assert!(url_in("e.g. this").is_none());
}

#[test]
fn guards() {
    for (u, ok) in [
        ("https://example.com", true),
        ("http://8.8.8.8/", true),
        ("http://127.0.0.1:8080", false),
        ("http://[::1]/", false),
        ("http://10.0.0.5/", false),
        ("http://router.lan/", false),
        ("javascript:alert(1)", false),
        ("file:///Users/me", false),
    ] {
        assert_eq!(allowed_host(&url::Url::parse(u).unwrap()), ok, "{u}");
    }
    assert_eq!(safe_file_name("../../etc/passwd", "download"), "passwd");
    assert_eq!(safe_file_name(".bashrc", "download"), "bashrc");
    assert_eq!(safe_file_name("a:b*c?.pdf", "download"), "a_b_c_.pdf");
    assert_eq!(safe_file_name("", "download"), "download");
    assert_eq!(safe_file_name("...", "download"), "download");
    let long = format!("{}.pdf", "x".repeat(300));
    let s = safe_file_name(&long, "d");
    assert!(s.ends_with(".pdf") && s.chars().count() <= 120, "{s}");
    let u = url::Url::parse("https://example.com/files/User%20Guide.pdf?dl=1").unwrap();
    assert_eq!(download_name(None, &u), "User Guide.pdf");
    assert_eq!(download_name(Some("attachment; filename=\"report 2026.pdf\""), &u), "report 2026.pdf");
    assert_eq!(download_name(Some("attachment; filename=x.pdf; filename*=UTF-8''caf%C3%A9.pdf"), &u), "café.pdf");
    assert_eq!(download_name(Some("attachment; filename=\"../../evil.sh\""), &u), "evil.sh");
    assert_eq!(size_text(2_500_000), "2.4 MB");
    assert_eq!(size_text(1000), "1 KB");
    let e = Element { label: "Your PIN code".into(), ..Default::default() };
    assert!(is_private_field(&e));
    let e = Element { label: "Search".into(), ..Default::default() };
    assert!(!is_private_field(&e));
}

#[test]
fn every_tool_has_a_spec() {
    let names: Vec<String> = specs().iter().map(|s| s["function"]["name"].as_str().unwrap().to_string()).collect();
    for t in TOOLS {
        assert!(names.iter().any(|n| n == t), "{t}");
    }
    assert_eq!(names.len(), TOOLS.len());
}
