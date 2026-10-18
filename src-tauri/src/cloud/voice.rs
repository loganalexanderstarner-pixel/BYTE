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

/// What the cluster heard in a recording: the words, the language, how the voice sounds (a mood word, or none), and any sound like laughter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Heard {
    pub text: String,
    pub lang: String,
    pub mood: Option<String>,
    pub event: Option<String>,
    /// "cloud" or "pc": where the listening was done.
    pub source: String,
}

pub fn parse_heard(v: &Value) -> Heard {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Heard { text: s("text").unwrap_or_default().trim().to_string(), lang: s("lang").unwrap_or_default(), mood: s("mood").filter(|m| !m.is_empty()), event: s("event"), source: "cloud".into() }
}

/// Speech to text on the cluster (`POST /api/voice/transcribe`), with the mood of the voice. `wav_base64`: a WAV recording.
pub async fn transcribe(client: &CloudClient, wav_base64: &str) -> CloudResult<Heard> {
    Ok(parse_heard(&client.post("/api/voice/transcribe", &json!({ "audio": wav_base64 })).await?))
}

/// Voices the cluster makes while the person listens (the expressive `qwen:` ones): the audio comes back as it is made.
pub fn streams(voice: &str) -> bool {
    voice.starts_with("qwen:")
}

/// One chunk of speech as raw 24 kHz 16-bit mono PCM, handed to `on_pcm` piece by piece as it is made. `on_pcm` returns false to stop
/// (the person interrupted). `tone` is optional plain words about the moment ("the person sounds tired, be gentle").
pub async fn stream(client: &CloudClient, text: &str, voice: &str, style: &str, speed: f32, tone: &str, mut on_pcm: impl FnMut(&[u8]) -> bool) -> CloudResult<()> {
    let body = json!({ "text": text, "voice": voice, "style": style, "speed": speed, "stream": true, "tone": tone });
    let started = std::time::Instant::now();
    let mut r = client.post_stream("/api/tts", &body).await?;
    let mut first = true;
    loop {
        match r.chunk().await {
            Ok(Some(c)) => {
                if first && crate::wake::test_log_on() {
                    log::info!("[test] timing: first audio from the cloud voice {} ms after asking ({} characters)", started.elapsed().as_millis(), text.chars().count());
                }
                first = false;
                if !on_pcm(&c) {
                    return Ok(());
                }
            }
            Ok(None) => return Ok(()),
            Err(e) => return Err(CloudError::Unreachable(e.to_string())),
        }
    }
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

    /// The real cluster: its voice list, then one sentence of speech through the app's own client. Needs
    /// `BYTE_TEST_CLOUD_URL` and `BYTE_TEST_CLOUD_KEY` (a personal API key; use a temporary one and revoke it).
    #[tokio::test]
    #[ignore = "needs BYTE_TEST_CLOUD_URL and BYTE_TEST_CLOUD_KEY"]
    async fn e2e_cloud_voices_from_the_real_cluster() {
        let (Ok(url), Ok(key)) = (std::env::var("BYTE_TEST_CLOUD_URL"), std::env::var("BYTE_TEST_CLOUD_KEY")) else { return };
        let client = CloudClient::new(&url, &key);
        let list = voices(&client).await.expect("the voice list");
        println!("{} voices; first: {:?}", list.len(), list.first());
        assert!(list.len() >= 20, "the cluster offers voices");
        assert!(list.iter().any(|v| v.id == "af_heart") && list.iter().all(|v| !v.name.is_empty()));
        for (voice, style) in [("af_heart", "natural"), ("bm_george", "lively")] {
            let started = std::time::Instant::now();
            let samples = synthesize(&client, "Your timer is done.", voice, style, 1.0).await.expect("speech");
            let secs = samples.len() as f32 / crate::tts::SAMPLE_RATE as f32;
            println!("{voice}/{style}: {secs:.2} s of speech in {:.2} s", started.elapsed().as_secs_f32());
            assert!(secs > 0.8 && samples.iter().any(|s| s.abs() > 0.05), "audible speech");
        }
        // An unknown voice is refused, not played as something else.
        assert!(synthesize(&client, "hi", "no_such_voice", "natural", 1.0).await.is_err());
    }

    #[tokio::test]
    async fn streamed_speech_arrives_in_pieces_and_a_missing_route_is_reported() {
        let server = MockServer::start().await;
        let pcm: Vec<u8> = (0..4800i16).flat_map(|s| s.to_le_bytes()).collect();
        Mock::given(method("POST"))
            .and(path("/api/tts"))
            .and(header("authorization", "Bearer byte_key"))
            .and(body_partial_json(json!({ "voice": "qwen:ryan", "stream": true, "style": "natural", "tone": "be gentle" })))
            .respond_with(ResponseTemplate::new(200).set_body_raw(pcm.clone(), "audio/L16; rate=24000"))
            .mount(&server)
            .await;
        let client = CloudClient::new(&server.uri(), "byte_key");
        let mut got: Vec<u8> = vec![];
        stream(&client, "Hello there.", "qwen:ryan", "natural", 1.0, "be gentle", |b| {
            got.extend_from_slice(b);
            true
        })
        .await
        .unwrap();
        assert_eq!(got, pcm);
        // Stopping early (the person interrupted) ends it cleanly.
        let mut first = 0;
        stream(&client, "Hello there.", "qwen:ryan", "natural", 1.0, "be gentle", |b| {
            first = b.len();
            false
        })
        .await
        .unwrap();
        assert!(first > 0);
        let gone = MockServer::start().await;
        assert!(stream(&CloudClient::new(&gone.uri(), "k"), "x", "qwen:ryan", "natural", 1.0, "", |_| true).await.is_err());
        assert!(streams("qwen:ryan") && !streams("af_heart"));
    }

    /// The real cluster's expressive voice, streamed: how long until the first sound, and the speech as the app's player would
    /// receive it (written to `BYTE_TEST_OUT`, if set, as a WAV to listen to). Needs `BYTE_TEST_CLOUD_URL` and `BYTE_TEST_CLOUD_KEY`.
    #[tokio::test]
    #[ignore = "needs BYTE_TEST_CLOUD_URL and BYTE_TEST_CLOUD_KEY"]
    async fn e2e_the_expressive_voice_streams_from_the_real_cluster() {
        let (Ok(url), Ok(key)) = (std::env::var("BYTE_TEST_CLOUD_URL"), std::env::var("BYTE_TEST_CLOUD_KEY")) else { return };
        let client = CloudClient::new(&url, &key);
        let list = voices(&client).await.expect("the voice list");
        assert!(list.iter().any(|v| v.id == "qwen:ryan"), "the expressive voices are offered: {:?}", list.iter().map(|v| &v.id).take(5).collect::<Vec<_>>());
        let started = std::time::Instant::now();
        let (mut first, mut dec, mut shaper) = (None, crate::tts::PcmDecoder::default(), crate::tts::StreamShaper::new(crate::tts::SAMPLE_RATE));
        let mut played: Vec<f32> = vec![];
        let text = "Yeah, it's seventy two and sunny out in Dover right now. A light breeze from the south.";
        stream(&client, text, "qwen:ryan", "natural", 1.0, "", |b| {
            first.get_or_insert(started.elapsed());
            played.extend(shaper.push(dec.push(b)));
            true
        })
        .await
        .expect("the voice streams");
        played.extend(shaper.finish(150));
        let secs = played.len() as f32 / crate::tts::SAMPLE_RATE as f32;
        println!("first sound after {:.2}s; {secs:.1}s of speech in {:.2}s", first.unwrap_or_default().as_secs_f32(), started.elapsed().as_secs_f32());
        assert!(secs > 3.0 && played.iter().any(|s| s.abs() > 0.05), "audible speech");
        if let Ok(out) = std::env::var("BYTE_TEST_OUT") {
            crate::wake::write_wav(std::path::Path::new(&out), &played).unwrap();
        }
    }

    #[test]
    fn what_the_cluster_heard_is_read() {
        let h = parse_heard(&json!({ "text": " Hello there. ", "lang": "en", "mood": "happy", "event": null, "seconds": 2.0 }));
        assert_eq!((h.text.as_str(), h.lang.as_str(), h.mood.as_deref(), h.event.clone(), h.source.as_str()), ("Hello there.", "en", Some("happy"), None, "cloud"));
        assert_eq!(parse_heard(&json!({ "text": "hi", "mood": "" })).mood, None);
        assert_eq!(parse_heard(&json!({})).text, "");
    }
}
