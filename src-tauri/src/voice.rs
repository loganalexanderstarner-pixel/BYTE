//! Voice input (Phase 11): speech to text on this Mac with whisper.cpp.
//!
//! `whisper-cli` ships as a second sidecar next to llama-server (built by
//! `scripts/build-whisper.sh`, Metal on a Mac). Its speech models are downloaded
//! on first use into `<models>/voice/` by the same resumable, SHA-256-checked
//! downloader as chat models. The UI records the microphone, makes a 16 kHz
//! WAV, and hands it here; dropped audio files come by path. Nothing leaves the
//! Mac.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::models::{self, ModelFile, Variant};
use crate::state::AppState;

const REPO: &str = "ggerganov/whisper.cpp";
const SIDECAR: &str = "whisper-cli";
/// A transcription that takes longer than this is stopped (very long recordings).
const TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Recordings from the mic button: up to 10 minutes of 16-bit 16 kHz mono.
const MAX_RECORDING_BYTES: usize = 10 * 60 * 16_000 * 2 + 44;

/// A speech model BYTE offers.
pub struct VoiceModel {
    pub id: &'static str,
    pub name: &'static str,
    pub about: &'static str,
    file: &'static str,
    pub size: u64,
    sha256: &'static str,
    /// Understands English only (faster and a little more accurate for English).
    pub english_only: bool,
}

pub const MODELS: &[VoiceModel] = &[
    VoiceModel {
        id: "turbo",
        name: "Best (any language)",
        about: "Whisper large-v3 turbo: very accurate, about 100 languages, fast on Apple Silicon.",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        english_only: false,
    },
    VoiceModel {
        id: "base-en",
        name: "Quick (English)",
        about: "Whisper base, English only: a small download that's good for short dictation.",
        file: "ggml-base.en.bin",
        size: 147_964_211,
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        english_only: true,
    },
];

pub fn find(id: &str) -> Option<&'static VoiceModel> {
    MODELS.iter().find(|m| m.id == id)
}

pub fn dir(models_dir: &Path) -> PathBuf {
    models_dir.join("voice")
}

pub fn key(m: &VoiceModel) -> String {
    format!("voice:{}", m.id)
}

fn variant(m: &VoiceModel) -> Variant {
    Variant {
        quant: "whisper".into(),
        bits: 0.0,
        size_bytes: m.size,
        files: vec![ModelFile { name: m.file.into(), size: m.size, sha256: m.sha256.into() }],
    }
}

pub fn installed(models_dir: &Path, m: &VoiceModel) -> bool {
    models::is_installed(&dir(models_dir), &variant(m))
}

fn model_path(models_dir: &Path, m: &VoiceModel) -> PathBuf {
    models::entry_path(&dir(models_dir), &variant(m))
}

/// The model to use: the chosen one when it's downloaded, else any downloaded one.
pub fn pick(models_dir: &Path, chosen: &str) -> Option<&'static VoiceModel> {
    find(chosen).filter(|m| installed(models_dir, m)).or_else(|| MODELS.iter().find(|m| installed(models_dir, m)))
}

/// whisper-cli's arguments: plain text out (no progress), with `[start --> end]` timestamps when asked.
pub fn args(model: &Path, audio: &Path, language: &str, english_only: bool, threads: usize) -> Vec<String> {
    with_timestamps(model, audio, language, english_only, threads, false)
}

fn with_timestamps(model: &Path, audio: &Path, language: &str, english_only: bool, threads: usize, timestamps: bool) -> Vec<String> {
    let lang = if english_only { "en" } else { language_code(language) };
    let mut a = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        audio.to_string_lossy().into_owned(),
        "-l".into(),
        lang.into(),
        "-t".into(),
        threads.to_string(),
        "-nt".into(),
        "-np".into(),
    ];
    if timestamps {
        a.retain(|x| x != "-nt");
    }
    a
}

/// A stretch of speech with its time (from whisper's timestamped output).
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// `[00:01:02.500 --> 00:01:05.000]   text` lines → segments (sound markers removed, empty ones dropped).
pub fn parse_segments(stdout: &str) -> Vec<Segment> {
    fn ms(t: &str) -> Option<u64> {
        let (hms, frac) = t.trim().split_once('.')?;
        let mut parts = hms.split(':').map(|x| x.parse::<u64>().ok());
        let (h, m, s) = (parts.next()??, parts.next()??, parts.next()??);
        Some(((h * 60 + m) * 60 + s) * 1000 + frac.parse::<u64>().ok()?)
    }
    stdout
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix('[')?;
            let (stamps, text) = rest.split_once(']')?;
            let (a, b) = stamps.split_once("-->")?;
            let text = clean(text);
            (!text.is_empty()).then_some(Segment { start_ms: ms(a)?, end_ms: ms(b)?, text })
        })
        .collect()
}

/// A language setting → whisper's code ("auto" when unknown, so nothing odd reaches the command).
fn language_code(language: &str) -> &str {
    let l = language.trim();
    if l.len() >= 2 && l.len() <= 3 && l.chars().all(|c| c.is_ascii_lowercase()) {
        l
    } else {
        "auto"
    }
}

/// whisper's text → what goes in the message box: one paragraph, without
/// the markers it writes for silence and sounds ("[BLANK_AUDIO]", "(music)").
pub fn clean(stdout: &str) -> String {
    let mut out = String::new();
    for line in stdout.lines() {
        let mut t = line.trim().to_string();
        // Drop bracketed or parenthesised sound markers anywhere in the line.
        for (open, close) in [('[', ']'), ('(', ')')] {
            while let (Some(a), Some(b)) = (t.find(open), t.find(close)) {
                if b <= a {
                    break;
                }
                let inner = &t[a + 1..b];
                let marker = inner.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c == ' ')
                    || ["music", "applause", "laughter", "silence", "noise", "inaudible", "blank_audio"].contains(&inner.to_lowercase().as_str());
                if !marker {
                    break;
                }
                t.replace_range(a..=b, "");
            }
        }
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&t);
    }
    out
}

/// Audio whisper-cli can't read itself (it reads WAV, MP3, FLAC and OGG).
fn needs_convert(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "m4a" | "aac" | "mp4" | "mov" | "caf" | "aiff" | "aif" | "3gp" | "amr")
}

pub fn is_audio(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "wav" | "mp3" | "flac" | "ogg" | "oga" | "opus") || needs_convert(path)
}

/// Turns an audio file macOS can play into a 16 kHz WAV with the built-in `afconvert`.
async fn to_wav(src: &Path, dest: &Path) -> AppResult<()> {
    if !cfg!(target_os = "macos") {
        return Err(AppError::msg("BYTE can read WAV, MP3, FLAC and OGG files here; other audio needs a Mac."));
    }
    let out = tokio::process::Command::new("/usr/bin/afconvert")
        .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
        .arg(src)
        .arg(dest)
        .output()
        .await?;
    if !out.status.success() {
        return Err(AppError::msg("macOS couldn't read that audio file."));
    }
    Ok(())
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8)
}

/// Runs whisper-cli: the test binary when BYTE_TEST_WHISPER is set, else the bundled sidecar.
async fn run_whisper(app: Option<&AppHandle>, args: Vec<String>) -> AppResult<String> {
    let (ok, stdout, stderr) = match (std::env::var("BYTE_TEST_WHISPER").ok().filter(|b| !b.is_empty()), app) {
        (Some(bin), _) => {
            let fut = tokio::process::Command::new(bin).args(&args).kill_on_drop(true).output();
            let out = tokio::time::timeout(TIMEOUT, fut).await.map_err(|_| AppError::msg("Transcribing took too long."))??;
            (out.status.success(), out.stdout, out.stderr)
        }
        (None, Some(app)) => {
            let cmd = crate::bundled::tool(&app, SIDECAR).map_err(|e| AppError::msg(format!("The voice engine is missing from this build: {e}")))?;
            let out = tokio::time::timeout(TIMEOUT, cmd.args(args).output())
                .await
                .map_err(|_| AppError::msg("Transcribing took too long."))?
                .map_err(|e| AppError::msg(format!("The voice engine didn't start: {e}")))?;
            (out.status.success(), out.stdout, out.stderr)
        }
        (None, None) => return Err(AppError::msg("The voice engine isn't available here.")),
    };
    if !ok {
        let err = String::from_utf8_lossy(&stderr);
        log::warn!("whisper-cli failed: {}", err.lines().rev().take(5).collect::<Vec<_>>().join(" | "));
        return Err(AppError::msg("BYTE couldn't make out any speech in that audio."));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// Transcribes an audio file with the chosen model.
pub async fn transcribe(app: Option<&AppHandle>, models_dir: &Path, chosen: &str, language: &str, audio: &Path) -> AppResult<String> {
    let m = pick(models_dir, chosen).ok_or_else(|| AppError::msg("Voice input needs a speech model first: Settings → Models → Voice."))?;
    let tmp = tempfile::Builder::new().prefix("byte-voice").suffix(".wav").tempfile()?;
    let input = if needs_convert(audio) {
        to_wav(audio, tmp.path()).await?;
        tmp.path().to_path_buf()
    } else {
        audio.to_path_buf()
    };
    Ok(clean(&run_whisper(app, args(&model_path(models_dir, m), &input, language, m.english_only, threads())).await?))
}

/// The sample rate and channel count of a PCM WAV file (None if it isn't one).
fn wav_format(path: &Path) -> Option<(u32, u16)> {
    use std::io::Read;
    let mut h = [0u8; 36];
    std::fs::File::open(path).ok()?.read_exact(&mut h).ok()?;
    (&h[0..4] == b"RIFF" && &h[8..12] == b"WAVE").then(|| (u32::from_le_bytes([h[24], h[25], h[26], h[27]]), u16::from_le_bytes([h[22], h[23]])))
}

/// A 16 kHz mono WAV of `audio` (what speaker labelling reads): `audio` itself when it already is one.
pub async fn wav_16k(audio: &Path, tmp: &Path) -> AppResult<PathBuf> {
    if wav_format(audio) == Some((16_000, 1)) {
        return Ok(audio.to_path_buf());
    }
    if !cfg!(target_os = "macos") {
        return Err(AppError::msg("Speaker labels need a 16 kHz mono WAV here; other audio needs a Mac."));
    }
    let out = tokio::process::Command::new("/usr/bin/afconvert").args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"]).arg(audio).arg(tmp).output().await?;
    if !out.status.success() {
        return Err(AppError::msg("macOS couldn't read that audio file."));
    }
    Ok(tmp.to_path_buf())
}

/// A short clip (the wake word): the smallest installed model, English, steered by `prompt`.
pub async fn transcribe_short(app: Option<&AppHandle>, models_dir: &Path, wav: &Path, prompt: &str) -> AppResult<String> {
    let m = pick(models_dir, "base-en").ok_or_else(|| AppError::msg("needs a speech model"))?;
    let mut a = args(&model_path(models_dir, m), wav, "en", true, threads().min(4));
    a.extend(["--prompt".into(), prompt.into()]);
    Ok(clean(&run_whisper(app, a).await?))
}

/// Transcribes a 16 kHz WAV into timed segments.
pub async fn transcribe_segments(app: Option<&AppHandle>, models_dir: &Path, chosen: &str, language: &str, wav: &Path) -> AppResult<Vec<Segment>> {
    let m = pick(models_dir, chosen).ok_or_else(|| AppError::msg("Voice input needs a speech model first: Settings → Models → Voice."))?;
    let out = run_whisper(app, with_timestamps(&model_path(models_dir, m), wav, language, m.english_only, threads(), true)).await?;
    Ok(parse_segments(&out))
}

/// A recording from the UI must be a WAV file of a sensible size.
pub fn check_recording(bytes: &[u8]) -> AppResult<()> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(AppError::msg("That recording isn't a WAV file."));
    }
    if bytes.len() > MAX_RECORDING_BYTES {
        return Err(AppError::msg("Recordings can be up to 10 minutes. For longer audio, drop the file into the chat."));
    }
    Ok(())
}

/// Audio files can be long; bigger than this, BYTE asks for a shorter one.
const MAX_AUDIO_FILE_BYTES: u64 = 500_000_000;

/// A dropped or attached recording, as a file BYTE reads: its transcript.
pub async fn ingest(app: &AppHandle, state: &AppState, path: &Path) -> AppResult<crate::files::Ingested> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("recording").to_string();
    let size = std::fs::metadata(path).map_err(|_| AppError::msg(format!("{name} can't be opened")))?.len();
    if size > MAX_AUDIO_FILE_BYTES {
        return Err(AppError::msg(format!("{name} is larger than 500 MB")));
    }
    let (chosen, language, speakers) = {
        let s = state.settings.lock().await;
        (s.voice_model.clone(), s.voice_language.clone(), s.voice_speakers)
    };
    let models = &state.paths.models;
    // Who said what, when that's on and its models are here; the plain transcript otherwise.
    let labelled = if speakers && crate::speakers::ready(models) {
        match crate::speakers::labelled_transcript(Some(app), models, &chosen, &language, path).await {
            Ok(t) => Some(t),
            Err(e) => {
                log::warn!("speaker labels for {name}: {e}");
                None
            }
        }
    } else {
        None
    };
    let text = match labelled {
        Some(t) => t,
        None => transcribe(Some(app), models, &chosen, &language, path).await?,
    };
    if text.trim().is_empty() {
        return Err(AppError::msg(format!("{name}: BYTE couldn't make out any speech in it")));
    }
    Ok(crate::files::Ingested::transcript(name, text))
}

// ------------------------------------------------------------------ commands

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceInfo {
    pub id: String,
    pub key: String,
    pub name: String,
    pub about: String,
    pub size_bytes: u64,
    pub installed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub models: Vec<VoiceInfo>,
    /// The model voice input uses now (None until one is downloaded).
    pub ready: Option<String>,
}

#[tauri::command]
pub async fn voice_status(state: State<'_, AppState>) -> AppResult<VoiceStatus> {
    let chosen = state.settings.lock().await.voice_model.clone();
    let dir = &state.paths.models;
    Ok(VoiceStatus {
        models: MODELS
            .iter()
            .map(|m| VoiceInfo { id: m.id.into(), key: key(m), name: m.name.into(), about: m.about.into(), size_bytes: m.size, installed: installed(dir, m) })
            .collect(),
        ready: pick(dir, &chosen).map(|m| m.id.to_string()),
    })
}

#[tauri::command]
pub async fn voice_download(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let m = find(&id).ok_or_else(|| AppError::msg("Unknown speech model."))?;
    let d = dir(&state.paths.models);
    std::fs::create_dir_all(&d)?;
    state.downloads.start(app, state.net.clone(), d, REPO.into(), variant(m), key(m)).await
}

#[tauri::command]
pub async fn voice_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let m = find(&id).ok_or_else(|| AppError::msg("Unknown speech model."))?;
    state.downloads.pause(&key(m)).await;
    models::delete(&dir(&state.paths.models), &variant(m))
}

/// A recording from the mic button (a WAV file, base64).
#[tauri::command]
pub async fn voice_transcribe(app: AppHandle, state: State<'_, AppState>, wav_base64: String) -> AppResult<String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(wav_base64.as_bytes()).map_err(|_| AppError::msg("That recording couldn't be read."))?;
    check_recording(&bytes)?;
    let mut tmp = tempfile::Builder::new().prefix("byte-voice").suffix(".wav").tempfile()?;
    std::io::Write::write_all(&mut tmp, &bytes)?;
    let (chosen, language) = {
        let s = state.settings.lock().await;
        (s.voice_model.clone(), s.voice_language.clone())
    };
    transcribe(Some(&app), &state.paths.models, &chosen, &language, tmp.path()).await
}

/// An audio file dropped into the chat or picked with 📎.
#[tauri::command]
pub async fn voice_transcribe_file(app: AppHandle, state: State<'_, AppState>, path: String) -> AppResult<String> {
    let p = PathBuf::from(&path);
    if !is_audio(&p) || !p.is_file() {
        return Err(AppError::msg("That isn't an audio file BYTE can read."));
    }
    let (chosen, language) = {
        let s = state.settings.lock().await;
        (s.voice_model.clone(), s.voice_language.clone())
    };
    transcribe(Some(&app), &state.paths.models, &chosen, &language, &p).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_whisper_output() {
        assert_eq!(clean("\n And so my fellow Americans, ask not\n what your country can do for you.\n"), "And so my fellow Americans, ask not what your country can do for you.");
        assert_eq!(clean("[BLANK_AUDIO]\n"), "");
        assert_eq!(clean(" (music) Hello there [MUSIC] friend\n"), "Hello there friend");
        // Ordinary brackets in speech stay.
        assert_eq!(clean("Call me (maybe) tomorrow"), "Call me (maybe) tomorrow");
    }

    #[test]
    fn reads_timestamped_segments() {
        let out = "\n[00:00:00.000 --> 00:00:07.600]   And so my fellow Americans,\n[00:00:07.600 --> 00:00:11.000]   [BLANK_AUDIO]\n[01:02:03.250 --> 01:02:05.000]  ask not.\nnoise line\n";
        let s = parse_segments(out);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], Segment { start_ms: 0, end_ms: 7600, text: "And so my fellow Americans,".into() });
        assert_eq!(s[1].start_ms, 3_723_250);
        assert!(!with_timestamps(Path::new("m"), Path::new("a"), "en", false, 4, true).contains(&"-nt".to_string()));
    }

    #[test]
    fn builds_safe_arguments() {
        let a = args(Path::new("/m/model.bin"), Path::new("/tmp/a.wav"), "es", false, 4);
        assert_eq!(a, ["-m", "/m/model.bin", "-f", "/tmp/a.wav", "-l", "es", "-t", "4", "-nt", "-np"]);
        // English-only models always get "en"; odd language settings become "auto".
        assert!(args(Path::new("m"), Path::new("a"), "es", true, 4).contains(&"en".to_string()));
        assert!(args(Path::new("m"), Path::new("a"), "-x; rm", false, 4).contains(&"auto".to_string()));
        assert!(args(Path::new("m"), Path::new("a"), "English", false, 4).contains(&"auto".to_string()));
    }

    #[test]
    fn picks_a_downloaded_model() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(pick(tmp.path(), "turbo").is_none());
        let base = find("base-en").unwrap();
        std::fs::create_dir_all(dir(tmp.path())).unwrap();
        let f = std::fs::File::create(model_path(tmp.path(), base)).unwrap();
        f.set_len(base.size).unwrap();
        // The chosen one isn't there, so the downloaded one is used.
        assert_eq!(pick(tmp.path(), "turbo").unwrap().id, "base-en");
        assert_eq!(pick(tmp.path(), "base-en").unwrap().id, "base-en");
    }

    #[test]
    fn checks_recordings_and_files() {
        assert!(check_recording(b"nope").is_err());
        let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
        wav.resize(100, 0);
        assert!(check_recording(&wav).is_ok());
        assert!(is_audio(Path::new("memo.m4a")) && is_audio(Path::new("a.MP3")) && !is_audio(Path::new("a.pdf")));
        assert!(needs_convert(Path::new("memo.m4a")) && !needs_convert(Path::new("a.wav")));
    }

    /// Real whisper-cli + model (BYTE_TEST_WHISPER, BYTE_TEST_WHISPER_MODEL = ggml-base.en.bin):
    /// whisper.cpp's JFK sample (BYTE_TEST_WHISPER_AUDIO) comes back as text.
    #[tokio::test]
    #[ignore]
    async fn e2e_whisper_transcribes() {
        let (Ok(_), Ok(model), Ok(audio)) = (std::env::var("BYTE_TEST_WHISPER"), std::env::var("BYTE_TEST_WHISPER_MODEL"), std::env::var("BYTE_TEST_WHISPER_AUDIO")) else {
            eprintln!("skipped: set BYTE_TEST_WHISPER, BYTE_TEST_WHISPER_MODEL and BYTE_TEST_WHISPER_AUDIO");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let base = find("base-en").unwrap();
        std::fs::create_dir_all(dir(tmp.path())).unwrap();
        std::fs::copy(&model, model_path(tmp.path(), base)).unwrap();
        let text = transcribe(None, tmp.path(), "turbo", "auto", Path::new(&audio)).await.unwrap();
        println!("transcript: {text}");
        let l = text.to_lowercase();
        assert!(l.contains("ask not what your country can do for you"), "{text}");
    }
}
