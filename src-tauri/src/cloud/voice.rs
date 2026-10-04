//! BYTE Cloud voices (docs/CLUSTER-REQUESTS.md request 7): speech made on the
//! cluster's GPU, so bigger, more expressive voices don't cost the Mac any
//! memory. Each chunk of an answer is sent as text and comes back as audio,
//! which goes into the same playback queue as BYTE's own voices, so it's just
//! as smooth. Private chats never use it, and when the cloud can't be reached
//! (or doesn't offer voices yet) BYTE quietly speaks with a voice on the Mac.
//!
//!   GET  /api/tts/voices  → [{id, name, about, lang, gender, expressive}]
//!   POST /api/tts {text, voice, style, speed} → audio/wav (or raw 16-bit PCM, `audio/L16; rate=24000`)

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use super::{CloudClient, CloudError, CloudResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudVoice {
    pub id: String,
    pub name: String,
    pub about: String,
    pub lang: String,
    pub gender: String,
    pub expressive: bool,
}

pub fn parse_voices(v: &Value) -> Vec<CloudVoice> {
    let list = v.get("voices").unwrap_or(v);
    list.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let s = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    let id = s("id");
                    (!id.is_empty()).then(|| CloudVoice {
                        name: Some(s("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone()),
                        id,
                        about: s("about"),
                        lang: s("lang"),
                        gender: s("gender"),
                        expressive: x.get("expressive").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The cloud's voices, remembered for 10 minutes (an empty list = the cloud doesn't offer voices).
static CACHE: Mutex<Option<(Instant, Vec<CloudVoice>)>> = Mutex::new(None);

pub async fn voices(client: &CloudClient) -> CloudResult<Vec<CloudVoice>> {
    if let Some((at, v)) = CACHE.lock().ok().and_then(|c| c.clone()) {
        if at.elapsed() < Duration::from_secs(600) {
            return Ok(v);
        }
    }
    let list = match client.get("/api/tts/voices").await {
        Ok(v) => parse_voices(&v),
        // Not there yet: no cloud voices (remembered, so BYTE doesn't keep asking).
        Err(CloudError::Other(e)) if e.to_string().contains("(404") || e.to_string().contains("(405") => vec![],
        Err(e) => return Err(e),
    };
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((Instant::now(), list.clone()));
    }
    Ok(list)
}

/// Audio bytes from the cloud as 24 kHz samples: a WAV file, or raw 16-bit little-endian PCM (`audio/L16; rate=N`).
pub fn decode(bytes: &[u8], mime: &str) -> Option<Vec<f32>> {
    if bytes.starts_with(b"RIFF") {
        let tmp = tempfile::Builder::new().suffix(".wav").tempfile().ok()?;
        std::fs::write(tmp.path(), bytes).ok()?;
        let (s, rate) = crate::tts::read_wav(tmp.path())?;
        return Some(crate::tts::resample(&s, rate, crate::tts::SAMPLE_RATE));
    }
    if mime.starts_with("audio/l16") || mime.starts_with("audio/pcm") {
        let rate = mime.split(';').find_map(|p| p.trim().strip_prefix("rate=")).and_then(|r| r.parse::<u32>().ok()).unwrap_or(crate::tts::SAMPLE_RATE);
        let s: Vec<f32> = bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        return Some(crate::tts::resample(&s, rate, crate::tts::SAMPLE_RATE));
    }
    None
}

/// One chunk of speech from the cloud.
pub async fn synthesize(client: &CloudClient, text: &str, voice: &str, style: &str, speed: f32) -> CloudResult<Vec<f32>> {
    let body = json!({ "text": text, "voice": voice, "style": style, "speed": speed });
    let (bytes, mime) = client.post_bytes("/api/tts", &body).await?;
    decode(&bytes, &mime.to_lowercase()).ok_or_else(|| CloudError::Other(crate::error::AppError::msg("The BYTE cloud sent audio BYTE can't play.")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn reads_voice_lists_in_either_shape() {
        let a = parse_voices(&json!([{ "id": "aria", "name": "Aria", "lang": "en-US", "gender": "female", "expressive": true }, { "name": "no id" }]));
        assert_eq!(a.len(), 1);
        assert!(a[0].expressive && a[0].name == "Aria");
        assert_eq!(parse_voices(&json!({ "voices": [{ "id": "x" }] }))[0].name, "x");
    }

    #[test]
    fn decodes_pcm_and_wav() {
        let pcm: Vec<u8> = [0i16, 16384, -16384, 0].iter().flat_map(|s| s.to_le_bytes()).collect();
        let s = decode(&pcm, "audio/l16; rate=24000").unwrap();
        assert_eq!(s.len(), 4);
        assert!((s[1] - 0.5).abs() < 1e-3);
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.wav");
        crate::wake::write_wav(&p, &[0.0, 0.5, 0.25, 0.0]).unwrap();
        let s = decode(&std::fs::read(&p).unwrap(), "audio/wav").unwrap();
        assert_eq!(s.len(), 6); // 16 kHz → 24 kHz
        assert!(decode(b"nope", "text/html").is_none());
    }

    #[tokio::test]
    async fn speaks_through_the_cloud_and_knows_when_it_cannot() {
        let server = MockServer::start().await;
        let pcm: Vec<u8> = (0..2400i16).flat_map(|i| (i * 7).to_le_bytes()).collect();
        Mock::given(method("POST"))
            .and(path("/api/tts"))
            .and(header("authorization", "Bearer byte_test_key"))
            .and(body_partial_json(json!({ "voice": "aria", "style": "lively" })))
            .respond_with(ResponseTemplate::new(200).insert_header("content-type", "audio/L16; rate=24000").set_body_bytes(pcm))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/api/tts/voices")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
        let c = CloudClient::new(&server.uri(), "byte_test_key");
        let audio = synthesize(&c, "Hello there!", "aria", "lively", 1.0).await.unwrap();
        assert_eq!(audio.len(), 2400);
        // A cloud without voices yet answers 404: no voices, not an error.
        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        assert!(voices(&c).await.unwrap().is_empty());
    }
}
