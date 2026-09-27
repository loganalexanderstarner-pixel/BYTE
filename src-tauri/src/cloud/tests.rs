//! Cloud mode against a mock server (no real key or cluster needed; the key
//! used here is a fake).

use super::*;
use crate::chat::e2e_support::collecting_channel;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FAKE_KEY: &str = "byte_test_not_a_real_key";

fn sse(events: &[(&str, Value)]) -> ResponseTemplate {
    let body: String = events.iter().map(|(e, d)| format!("event: {e}\ndata: {d}\n\n")).collect();
    ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(body)
}

fn state_at(dir: &std::path::Path) -> crate::state::AppState {
    let mut s = crate::state::AppState::new(crate::paths::Paths::at(dir.to_path_buf()).unwrap());
    s.secrets = Box::new(keychain::MemoryStore::default());
    s
}

#[test]
fn reads_the_account_without_assuming_its_shape() {
    let me = parse_me(&json!({
        "user": { "name": "Logan", "email": "l@example.com", "tier": { "name": "pro" } },
        "modes": ["fast", "auto", "extended_plus"],
        "budgets": { "daily_tokens": 100000 }
    }));
    assert_eq!(me.name.as_deref(), Some("Logan"));
    assert_eq!(me.tier.as_deref(), Some("pro"));
    assert_eq!(me.modes.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), ["Fast", "Auto", "Extended+"]);
    assert_eq!(me.budgets["daily_tokens"], 100000);
    // Objects with labels, and modes the app has never heard of, pass through as sent.
    let me = parse_me(&json!({ "tier": "free", "modes": [{ "id": "fast", "label": "Quick" }, { "id": "super_deep" }] }));
    assert_eq!(me.modes, vec![CloudMode { id: "fast".into(), label: "Quick".into() }, CloudMode { id: "super_deep".into(), label: "Super deep".into() }]);
    assert!(parse_me(&json!({})).modes.is_empty(), "no hard-coded fallback list");
}

#[test]
fn finds_message_ids_in_any_reply_shape() {
    assert_eq!(posted_ids(&json!({ "id": 5, "role": "user" })), (Some("5".into()), None));
    assert_eq!(
        posted_ids(&json!({ "user_message": { "id": 5, "role": "user" }, "assistant_message": { "id": 6, "role": "assistant" } })),
        (Some("5".into()), Some("6".into()))
    );
    assert_eq!(posted_ids(&json!([{ "id": "a", "role": "user" }, { "id": "b", "role": "assistant" }])), (Some("a".into()), Some("b".into())));
}

#[tokio::test]
async fn streams_deltas_phases_and_early_sources() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/conversations/c1/stream"))
        .and(query_param("since", "10"))
        .and(header("authorization", format!("Bearer {FAKE_KEY}").as_str()))
        .respond_with(sse(&[
            ("message", json!({ "id": 11, "role": "assistant", "content": "", "status": "streaming" })),
            ("phase", json!({ "id": 11, "phase": "searching: tides" })),
            ("sources", json!({ "id": 11, "sources": [{ "title": "NOAA", "url": "https://noaa.gov/tides" }], "grounded": true })),
            ("delta", json!({ "id": 11, "append": "Tides come ", "status": "streaming" })),
            ("delta", json!({ "id": 11, "append": "from the Moon.", "status": "streaming" })),
            ("status", json!({ "id": 11, "status": "done" })),
            ("done", json!({})),
        ]))
        .mount(&server)
        .await;
    let client = CloudClient::new(&server.uri(), FAKE_KEY);
    let (ch, seen) = collecting_channel();
    let end = follow(&client, "c1", Some("10".into()), None, &CancellationToken::new(), &ch).await.unwrap();
    assert_eq!(end.text, "Tides come from the Moon.");
    assert_eq!(end.assistant_id.as_deref(), Some("11"));
    let events = seen.lock().unwrap().clone();
    let kinds: Vec<&str> = events.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["phase", "sources", "content", "content", "stats"]);
    assert_eq!(events[0]["text"], "searching: tides");
    assert_eq!(events[1]["sources"][0]["url"], "https://noaa.gov/tides");
}

#[tokio::test]
async fn reconnects_after_a_dropped_stream_without_repeating_text() {
    let server = MockServer::start().await;
    // First connection: part of the answer, then the server closes.
    Mock::given(method("GET"))
        .and(path("/api/conversations/c2/stream"))
        .respond_with(sse(&[("delta", json!({ "id": 21, "append": "Hel" })), ("bye", json!({}))]))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    // Second: the full row (already finished) arrives again.
    Mock::given(method("GET"))
        .and(path("/api/conversations/c2/stream"))
        .respond_with(sse(&[("message", json!({ "id": 21, "role": "assistant", "content": "Hello!", "status": "done" })), ("done", json!({}))]))
        .with_priority(2)
        .mount(&server)
        .await;
    let client = CloudClient::new(&server.uri(), FAKE_KEY);
    let (ch, seen) = collecting_channel();
    let end = follow(&client, "c2", Some("20".into()), None, &CancellationToken::new(), &ch).await.unwrap();
    assert_eq!(end.text, "Hello!");
    let text: String = seen.lock().unwrap().iter().filter(|e| e["kind"] == "content").map(|e| e["delta"].as_str().unwrap().to_string()).collect();
    assert_eq!(text, "Hello!");
}

#[tokio::test]
async fn other_messages_on_the_stream_are_ignored() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/conversations/c3/stream"))
        .respond_with(sse(&[
            ("delta", json!({ "id": 99, "append": "old answer" })),
            ("delta", json!({ "id": 31, "append": "mine" })),
            ("status", json!({ "id": 31, "status": "done" })),
        ]))
        .mount(&server)
        .await;
    let client = CloudClient::new(&server.uri(), FAKE_KEY);
    let (ch, _) = collecting_channel();
    let end = follow(&client, "c3", None, Some("31".into()), &CancellationToken::new(), &ch).await.unwrap();
    assert_eq!(end.text, "mine");
}

#[tokio::test]
async fn an_error_status_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/conversations/c4/stream"))
        .respond_with(sse(&[("delta", json!({ "id": 41, "append": "par" })), ("status", json!({ "id": 41, "status": "error" }))]))
        .mount(&server)
        .await;
    let client = CloudClient::new(&server.uri(), FAKE_KEY);
    let (ch, _) = collecting_channel();
    assert!(matches!(follow(&client, "c4", None, None, &CancellationToken::new(), &ch).await, Err(CloudError::Other(_))));
}

#[tokio::test]
async fn cancelling_stops_the_answer_on_the_cloud() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/conversations/c5/stream"))
        .respond_with(sse(&[("delta", json!({ "id": 51, "append": "x" }))]).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    Mock::given(method("POST")).and(path("/api/messages/51/stop")).respond_with(ResponseTemplate::new(200)).expect(1).mount(&server).await;
    let client = CloudClient::new(&server.uri(), FAKE_KEY);
    let (ch, _) = collecting_channel();
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        c2.cancel();
    });
    let end = follow(&client, "c5", None, Some("51".into()), &cancel, &ch).await.unwrap();
    assert_eq!(end.finish, "cancelled");
}

#[tokio::test]
async fn gateway_errors_and_refused_connections_count_as_unreachable() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/auth/me")).respond_with(ResponseTemplate::new(503)).mount(&server).await;
    let client = CloudClient::new(&server.uri(), FAKE_KEY);
    assert!(matches!(client.get("/api/auth/me").await, Err(CloudError::Unreachable(_))));
    let down = CloudClient::new("http://127.0.0.1:9", FAKE_KEY);
    assert!(matches!(down.get("/api/auth/me").await, Err(CloudError::Unreachable(_))));
}

#[tokio::test]
async fn a_key_is_saved_only_after_the_cloud_accepts_it() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path());
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/auth/me"))
        .and(header("authorization", "Bearer byte_test_rejected_key_000"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "detail": "not authenticated" })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/auth/me"))
        .and(header("authorization", format!("Bearer {FAKE_KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "Logan", "tier": "pro", "modes": ["fast", "auto", "extended"] })))
        .mount(&server)
        .await;
    let base = Some(server.uri());

    let err = cmd::connect(&state, "byte_test_rejected_key_000", base.clone()).await.unwrap_err().to_string();
    assert!(err.contains("didn't accept"), "{err}");
    assert_eq!(state.secrets.get(&state.cloud_account()).unwrap(), None);
    assert!(!state.settings.lock().await.cloud_connected);

    assert!(cmd::connect(&state, "not a key", base.clone()).await.is_err());

    cmd::connect(&state, FAKE_KEY, base).await.unwrap();
    assert_eq!(state.secrets.get(&state.cloud_account()).unwrap().as_deref(), Some(FAKE_KEY));
    let s = state.settings.lock().await.clone();
    assert!(s.cloud_connected);
    assert_eq!(s.cloud_mode.as_deref(), Some("auto"));
    // The key is never written to the settings file.
    let file = std::fs::read_to_string(&state.paths.settings_file).unwrap();
    assert!(!file.contains(FAKE_KEY));
}

#[tokio::test]
async fn a_new_chat_creates_a_conversation_and_streams_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path());
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/api/conversations")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 7 }))).expect(1).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/conversations/7/messages"))
        .and(wiremock::matchers::body_partial_json(json!({ "content": "Why are tides?", "mode": "extended" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 70, "role": "user" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/conversations/7/stream"))
        .and(query_param("since", "70"))
        .respond_with(sse(&[("delta", json!({ "id": 71, "append": "The Moon." })), ("status", json!({ "id": 71, "status": "done" }))]))
        .mount(&server)
        .await;
    state.settings.lock().await.cloud_base_url = Some(server.uri());
    *state.cloud_key.lock().await = Some(FAKE_KEY.into());
    let request: crate::commands::ChatRequest = serde_json::from_value(json!({
        "requestId": "r1", "messages": [{ "role": "user", "content": "Why are tides?" }], "mode": "auto", "thinking": "auto",
    }))
    .unwrap();
    let turn: cmd::CloudTurn = serde_json::from_value(json!({ "mode": "extended" })).unwrap();
    let (ch, seen) = collecting_channel();
    cmd::send(&state, &request, &turn, &ch).await.unwrap();
    let events = seen.lock().unwrap().clone();
    let remote: Vec<&Value> = events.iter().filter(|e| e["kind"] == "remote").collect();
    assert_eq!(remote[0]["conversationId"], "7");
    assert_eq!(remote.last().unwrap()["messageId"], "71");
    assert!(events.iter().any(|e| e["kind"] == "content" && e["delta"] == "The Moon."));
    assert_eq!(events.last().unwrap()["kind"], "done");
}

#[tokio::test]
async fn an_unreachable_cloud_is_reported_before_anything_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path());
    state.settings.lock().await.cloud_base_url = Some("http://127.0.0.1:9".into());
    *state.cloud_key.lock().await = Some(FAKE_KEY.into());
    let request: crate::commands::ChatRequest =
        serde_json::from_value(json!({ "requestId": "r2", "messages": [{ "role": "user", "content": "hi" }], "mode": "auto", "thinking": "auto" })).unwrap();
    let turn: cmd::CloudTurn = serde_json::from_value(json!({ "mode": "fast" })).unwrap();
    let (ch, _) = collecting_channel();
    assert!(matches!(cmd::send(&state, &request, &turn, &ch).await, Err(CloudError::Unreachable(_))));
    assert_eq!(cmd::local_mode("fast"), crate::settings::Mode::Fast);
    assert_eq!(cmd::local_mode("extended_plus"), crate::settings::Mode::Extended);
}

#[test]
fn cloud_conversations_become_local_chats() {
    let v = json!({
        "id": 7, "title": "Tides", "created_at": "2026-09-27T10:00:00Z",
        "messages": [
            { "id": 70, "role": "user", "content": "Why tides?" },
            { "id": 71, "role": "assistant", "content": "The Moon [1].", "sources": [{ "title": "NOAA", "url": "https://noaa.gov" }] },
            { "id": 72, "role": "system", "content": "hidden" }
        ]
    });
    let chat = cmd::to_local_chat(&v, "7", None);
    assert_eq!(chat["id"], "cloud-7");
    assert_eq!(chat["cloudId"], "7");
    let msgs = chat["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1]["remoteId"], "71");
    assert_eq!(msgs[1]["sources"][0]["n"], 1);
    assert_eq!(cmd::to_local_chat(&v, "7", Some("mine".into()))["id"], "mine");
}

#[test]
fn only_cloud_api_paths_are_allowed() {
    for ok in ["/api/documents", "/api/jobs/12/outline", "/api/templates?kind=pptx&topic=solar%20power"] {
        assert!(cmd::api_path(ok).is_ok(), "{ok}");
    }
    for bad in ["/other", "api/x", "/api/../admin", "https://evil.example/api/x", "/api/x y", "/api/a\\b", "/api/x#frag"] {
        assert!(cmd::api_path(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn uploads_send_the_file_as_multipart() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/conversations/9/attachments"))
        .and(wiremock::matchers::header_regex("content-type", "^multipart/form-data"))
        .and(wiremock::matchers::body_string_contains("filename=\"photo.png\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 5, "name": "photo.png" })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, b"fake png bytes").unwrap();
    let client = CloudClient::new(&server.uri(), FAKE_KEY);
    let v = client.upload("/api/conversations/9/attachments", &file, 1024).await.unwrap();
    assert_eq!(v["id"], 5);
    // Too big: refused before anything is sent.
    std::fs::write(&file, vec![0u8; 2048]).unwrap();
    assert!(client.upload("/api/conversations/9/attachments", &file, 1024).await.is_err());
    assert_eq!(mime_for("Deck.PPTX"), "application/vnd.openxmlformats-officedocument.presentationml.presentation");
}

#[tokio::test]
async fn downloads_keep_bytes_and_type() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/documents/3/preview/1"))
        .respond_with(ResponseTemplate::new(200).insert_header("content-type", "image/png").set_body_bytes(vec![1u8, 2, 3]))
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/api/documents/404/download")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
    let client = CloudClient::new(&server.uri(), FAKE_KEY);
    let (b, mime) = client.bytes("/api/documents/3/preview/1").await.unwrap();
    assert_eq!((b, mime.as_str()), (vec![1, 2, 3], "image/png"));
    assert!(matches!(client.bytes("/api/documents/404/download").await, Err(CloudError::Other(_))));
}
