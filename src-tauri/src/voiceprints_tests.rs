use super::*;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn who(name: Option<&str>, score: f32, taught: u32) -> Speaker {
    Speaker { checked: true, name: name.map(str::to_string), best: name.map(str::to_string), score, taught }
}

#[test]
fn replies_are_read() {
    let v = parse_voices(&json!([{ "id": 3, "name": "Logan", "samples": 3 }, { "name": "no id" }, { "id": 4, "name": "Sam" }]));
    assert_eq!(v, vec![Voice { id: 3, name: "Logan".into(), samples: 3 }, Voice { id: 4, name: "Sam".into(), samples: 1 }]);
    let s = parse_identity(&json!({ "name": "Logan", "best": "Logan", "score": 0.83, "known": true, "taught": 2 }));
    assert!(s.checked && s.name.as_deref() == Some("Logan") && (s.score - 0.83).abs() < 1e-6 && s.taught == 2 && s.sure());
    // A clear-enough match for a greeting is not necessarily sure enough for a lock.
    assert!(!parse_identity(&json!({ "name": "Logan", "best": "Logan", "score": 0.63, "taught": 2 })).sure());
    assert!(!Speaker::default().sure());
}

#[test]
fn the_lock_turns_away_what_it_should_and_only_that() {
    let me = who(Some("Logan"), 0.85, 2);
    let stranger = who(None, 0.31, 2);
    let lookalike = who(Some("Logan"), 0.62, 2);
    // Off, or typed (not spoken): nothing is ever turned away.
    for sp in [Some(&me), Some(&stranger), None] {
        assert_eq!(refusal("off", true, sp, true), None);
        assert_eq!(refusal("all", false, sp, true), None);
    }
    // Actions: a stranger can ask a question but not make BYTE do something; so can a voice that is only similar.
    assert_eq!(refusal("actions", true, Some(&stranger), false), None);
    assert!(refusal("actions", true, Some(&stranger), true).is_some_and(|m| m.contains("won't do that")));
    assert!(refusal("actions", true, Some(&lookalike), true).is_some());
    assert_eq!(refusal("actions", true, Some(&me), true), None);
    // All: only a known voice at all.
    assert!(refusal("all", true, Some(&stranger), false).is_some());
    assert_eq!(refusal("all", true, Some(&me), false), None);
    // Could not check (no cloud, a problem): actions are held back, questions still answered.
    assert!(refusal("actions", true, None, true).is_some_and(|m| m.contains("couldn't check")));
    assert_eq!(refusal("actions", true, None, false), None);
    assert!(refusal("all", true, Some(&Speaker::default()), false).is_some());
    // A lock with nobody taught locks nobody out.
    assert_eq!(refusal("all", true, Some(&who(None, 0.0, 0)), true), None);
}

#[tokio::test]
async fn the_client_talks_to_the_cluster() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/voiceprints")).and(header("authorization", "Bearer byte_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "id": 1, "name": "Logan", "samples": 3 }]))).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/voiceprints")).and(body_partial_json(json!({ "name": "Haley" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 2, "name": "Haley", "samples": 3, "used": 3 }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/voiceprints/identify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "Logan", "best": "Logan", "score": 0.9, "known": true, "taught": 1 }))).mount(&server).await;
    Mock::given(method("DELETE")).and(path("/api/voiceprints/2")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true }))).mount(&server).await;
    let c = CloudClient::new(&server.uri(), "byte_key");
    assert_eq!(list(&c).await.unwrap().len(), 1);
    assert_eq!(teach(&c, "Haley", &["QQ==".to_string()]).await.unwrap()["used"], 3);
    assert!(identify(&c, "QQ==").await.unwrap().sure());
    forget(&c, 2).await.unwrap();
    assert_eq!(b64(b"abc"), "YWJj");
}
