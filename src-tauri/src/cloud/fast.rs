//! Hey BYTE's fast lane: `POST /api/voice/ask` on the cluster (infra: byte-ai `voice.py`). A spoken question that no PC action
//! claimed is answered by the cluster's small voice model, streamed, instead of going through a saved cloud conversation, which
//! costs about two seconds before the first word. The first spoken sentence arrives in well under a second.
//!
//! The cluster adds BYTE's spoken persona, a quick web lookup for real questions of fact (it says "Let me check." first), and
//! scrubs emoji and markdown. This side sends the question, the last few turns for follow-ups, the person's name and town and the
//! local date and time, and turns the stream into the same `Content` events every answer uses.
//!
//! Private chats never get here (nothing about them goes to the cloud). When the lane is not there (an older cluster, the voice
//! model restarting) `ask` says so and the caller asks the normal cloud chat instead.

use serde_json::{json, Value};

use super::{CloudClient, CloudError, CloudResult};
use crate::commands::ChatRequest;

/// How many earlier messages go along, for follow-ups ("and in Memphis?").
const HISTORY: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Delta(String),
    Done,
    Error(String),
}

/// One line of the stream, if it is one of ours.
pub fn parse_event(line: &str) -> Option<Event> {
    let v: Value = serde_json::from_str(line.trim().strip_prefix("data:")?.trim()).ok()?;
    if let Some(e) = v.get("error").and_then(Value::as_str) {
        return Some(Event::Error(e.to_string()));
    }
    if v.get("done").and_then(Value::as_bool) == Some(true) {
        return Some(Event::Done);
    }
    v.get("delta").and_then(Value::as_str).filter(|d| !d.is_empty()).map(|d| Event::Delta(d.to_string()))
}

/// The request: the newest question, and up to `HISTORY` messages before it.
pub fn body(request: &ChatRequest, name: &str, place: &str, now: &str) -> Option<Value> {
    let text = |m: &crate::chat::ChatMessage| crate::chat::question_text(&m.content).trim().to_string();
    let last = request.messages.iter().rposition(|m| m.role == "user")?;
    let question = text(&request.messages[last]);
    if question.is_empty() {
        return None;
    }
    let before = &request.messages[..last];
    let history: Vec<Value> = before
        .iter()
        .filter(|m| (m.role == "user" || m.role == "assistant") && !text(m).is_empty())
        .rev()
        .take(HISTORY)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| json!({ "role": m.role, "content": text(m) }))
        .collect();
    Some(json!({ "text": question, "history": history, "name": name, "place": place, "now": now }))
}

/// The local date and time the way a person says it ("Tuesday, October 13, 2026, 4:10 PM").
pub fn now_text<Tz: chrono::TimeZone>(now: &chrono::DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    now.format("%A, %B %-d, %Y, %-I:%M %p").to_string()
}

/// Streams the answer into `on_delta`. `Ok(true)`: answered (the text went to `on_delta`). `Err(Unreachable)`: the lane is not
/// there, nothing was said, the caller may ask the normal chat. After the first word an error is not recoverable, so a stream
/// that breaks part-way is reported as `Other`.
pub async fn ask(client: &CloudClient, body: &Value, mut on_delta: impl FnMut(String) -> bool) -> CloudResult<bool> {
    let started = std::time::Instant::now();
    let mut r = client.post_stream("/api/voice/ask", body).await?;
    if crate::wake::test_log_on() {
        log::info!("[test] timing: the cloud answered the request after {} ms (waiting for the first words)", started.elapsed().as_millis());
    }
    let mut buf = String::new();
    let mut spoke = false;
    let mut first_logged = false;
    loop {
        let chunk = match r.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                return Err(if spoke {
                    CloudError::Other(crate::error::AppError::msg(format!("The answer stopped part-way: {e}")))
                } else {
                    CloudError::Unreachable(e.to_string())
                })
            }
        };
        if !first_logged && crate::wake::test_log_on() {
            first_logged = true;
            log::info!("[test] timing: first answer data {} ms after asking", started.elapsed().as_millis());
        }
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(i) = buf.find('\n') {
            let line: String = buf.drain(..=i).collect();
            match parse_event(&line) {
                Some(Event::Delta(d)) => {
                    spoke = true;
                    if !on_delta(d) {
                        return Ok(true);
                    }
                }
                Some(Event::Done) => return Ok(true),
                Some(Event::Error(e)) => {
                    return Err(if spoke { CloudError::Other(crate::error::AppError::msg(e)) } else { CloudError::Unreachable(e) });
                }
                None => {}
            }
        }
    }
    Ok(spoke)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatMessage;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request(msgs: &[(&str, &str)]) -> ChatRequest {
        serde_json::from_value(json!({
            "requestId": "r", "mode": "auto", "thinking": "off", "spoken": true,
            "messages": msgs.iter().map(|(r, c)| json!({ "role": r, "content": c })).collect::<Vec<_>>()
        }))
        .unwrap()
    }

    #[test]
    fn events_are_read_and_other_lines_ignored() {
        assert_eq!(parse_event("data: {\"delta\": \"Hi \"}"), Some(Event::Delta("Hi ".into())));
        assert_eq!(parse_event("data: {\"done\": true}\n"), Some(Event::Done));
        assert_eq!(parse_event("data: {\"error\": \"nope\"}"), Some(Event::Error("nope".into())));
        for other in ["", ": keep-alive", "event: x", "data: not json", "data: {\"delta\": \"\"}", "data: {\"other\": 1}"] {
            assert_eq!(parse_event(other), None, "{other:?}");
        }
    }

    #[test]
    fn the_request_carries_the_question_and_the_last_turns() {
        let many: Vec<(String, String)> = (0..10).flat_map(|i| [("user".to_string(), format!("q{i}")), ("assistant".to_string(), format!("a{i}"))]).collect();
        let mut msgs: Vec<(&str, &str)> = many.iter().map(|(r, c)| (r.as_str(), c.as_str())).collect();
        msgs.push(("user", "and in Memphis?"));
        let b = body(&request(&msgs), "Logan", "Dover, Tennessee", "Tuesday, 4:10 PM").unwrap();
        assert_eq!(b["text"], "and in Memphis?");
        let h = b["history"].as_array().unwrap();
        assert_eq!(h.len(), 6);
        assert_eq!(h[0]["content"], "q7");
        assert_eq!(h[5], json!({ "role": "assistant", "content": "a9" }));
        assert_eq!((b["name"].as_str(), b["place"].as_str()), (Some("Logan"), Some("Dover, Tennessee")));
        assert!(body(&request(&[("assistant", "hello")]), "", "", "").is_none(), "nothing asked");
        assert!(body(&request(&[("user", "  ")]), "", "", "").is_none());
    }

    #[test]
    fn the_time_is_said_the_way_a_person_says_it() {
        let t = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap().and_hms_opt(16, 10, 0).unwrap().and_utc();
        assert_eq!(now_text(&t), "Tuesday, October 13, 2026, 4:10 PM");
    }

    #[tokio::test]
    async fn the_answer_streams_in_pieces_and_a_missing_lane_is_skipped() {
        let server = MockServer::start().await;
        let sse = "data: {\"delta\": \"Let me check. \"}\n\ndata: {\"delta\": \"It's sunny.\"}\n\ndata: {\"done\": true}\n\n";
        Mock::given(method("POST"))
            .and(path("/api/voice/ask"))
            .and(header("authorization", "Bearer byte_key"))
            .and(body_partial_json(json!({ "text": "weather?" })))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;
        let client = CloudClient::new(&server.uri(), "byte_key");
        let mut said = String::new();
        let ok = ask(&client, &json!({ "text": "weather?" }), |d| {
            said.push_str(&d);
            true
        })
        .await
        .unwrap();
        assert!(ok);
        assert_eq!(said, "Let me check. It's sunny.");
        // Stopping early (the person interrupted) ends it cleanly.
        let mut first = String::new();
        assert!(ask(&client, &json!({ "text": "weather?" }), |d| {
            first = d;
            false
        })
        .await
        .unwrap());
        assert_eq!(first, "Let me check. ");
        // No such route, or the voice model is down: skipped, so the caller asks the normal chat.
        let off = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(503)).mount(&off).await;
        assert!(matches!(ask(&CloudClient::new(&off.uri(), "k"), &json!({}), |_| true).await, Err(CloudError::Unreachable(_))));
        let gone = MockServer::start().await;
        assert!(matches!(ask(&CloudClient::new(&gone.uri(), "k"), &json!({}), |_| true).await, Err(CloudError::Unreachable(_))));
        let key = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(401)).mount(&key).await;
        assert!(matches!(ask(&CloudClient::new(&key.uri(), "bad"), &json!({}), |_| true).await, Err(CloudError::Unauthorized)));
    }

    /// The real cluster: a spoken question streamed back through the app's own client. Needs `BYTE_TEST_CLOUD_URL` and
    /// `BYTE_TEST_CLOUD_KEY` (use a temporary personal API key and revoke it).
    #[tokio::test]
    #[ignore = "needs BYTE_TEST_CLOUD_URL and BYTE_TEST_CLOUD_KEY"]
    async fn e2e_the_fast_lane_from_the_real_cluster() {
        let (Ok(url), Ok(key)) = (std::env::var("BYTE_TEST_CLOUD_URL"), std::env::var("BYTE_TEST_CLOUD_KEY")) else { return };
        let client = CloudClient::new(&url, &key);
        for (q, history) in [("What's seventeen times twenty three?", json!([])), ("And what's that plus ten?", json!([{ "role": "user", "content": "What's seventeen times twenty three?" }, { "role": "assistant", "content": "It's 391." }]))] {
            let body = json!({ "text": q, "history": history, "name": "Test", "place": "", "now": "Tuesday, October 13, 2026, 4:10 PM" });
            let started = std::time::Instant::now();
            let (mut first, mut said) = (None, String::new());
            let ok = ask(&client, &body, |d| {
                first.get_or_insert(started.elapsed());
                said.push_str(&d);
                true
            })
            .await
            .expect("the fast lane answers");
            println!("{q:?} -> {said:?}  (first word {:.2}s, whole answer {:.2}s)", first.unwrap_or_default().as_secs_f32(), started.elapsed().as_secs_f32());
            assert!(ok && !said.trim().is_empty());
        }
    }
}
