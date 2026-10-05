//! BYTE's voice catalog (Phase 11): free voices from several open projects
//! (Kokoro, Piper, Kitten TTS, Supertonic, Pocket TTS), all made on this Mac by
//! one engine, sherpa-onnx's speech tool (the `sherpa-tts` sidecar). No
//! accounts, nothing sent anywhere.
//!
//! `catalog/voices.json` is built by `scripts/build-voices.mjs`: one entry per
//! download ("package": one archive from sherpa-onnx's `tts-models` release,
//! pinned size and SHA-256) with its speakers. A voice is chosen as
//! "<package>/<speaker>" (setting `byteVoice`).
//!
//! The speech tool runs on the CPU only (never the GPU the chat model uses), in
//! a short-lived process per chunk, with at most half the cores.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::{self, ModelFile, Variant};

const RELEASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models";
/// The voice used until the owner picks another (and the one old settings meant).
pub const DEFAULT_VOICE: &str = "kokoro-v1_0/af_heart";
const KOKORO: &str = "kokoro-v1_0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Speaker {
    pub id: String,
    pub sid: u32,
    pub name: String,
    /// BCP-47-ish: "en-US", "de-DE", "en".
    pub lang: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub about: String,
    /// Pocket TTS: the sample recording whose tone it copies (inside the package).
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub id: String,
    /// kokoro | vits | kitten | supertonic | pocket
    pub engine: String,
    pub provider: String,
    pub name: String,
    pub about: String,
    pub archive: String,
    /// The folder the archive unpacks to.
    pub folder: String,
    /// Where it lives under <models>/voice (default: its id). Kokoro keeps "kokoro" from v0.11.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    pub size: u64,
    pub sha256: String,
    /// Memory while speaking (measured or estimated).
    pub ram_mb: u32,
    pub quality: String,
    pub license: String,
    #[serde(default)]
    pub expressive: bool,
    pub languages: Vec<String>,
    pub language_name: String,
    pub speakers: Vec<Speaker>,
}

#[derive(Deserialize)]
struct CatalogFile {
    packages: Vec<Package>,
}

pub fn catalog() -> &'static [Package] {
    static C: OnceLock<Vec<Package>> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str::<CatalogFile>(include_str!("../catalog/voices.json")).map(|c| c.packages).unwrap_or_default())
}

pub fn package(id: &str) -> Option<&'static Package> {
    catalog().iter().find(|p| p.id == id)
}

/// The package and speaker a `byteVoice` setting names. Old settings (v0.11.4) named a Kokoro speaker alone.
pub fn pick(setting: &str) -> (&'static Package, &'static Speaker) {
    let (pkg, sp) = setting.split_once('/').unwrap_or((KOKORO, setting));
    let p = package(pkg).or_else(|| package(KOKORO)).expect("Kokoro is in the catalog");
    let s = p.speakers.iter().find(|s| s.id == sp).unwrap_or(&p.speakers[0]);
    (p, s)
}

// ------------------------------------------------------------------ files on disk

pub fn package_dir(models_dir: &Path, p: &Package) -> PathBuf {
    crate::voice::dir(models_dir).join(p.dir.as_deref().unwrap_or(&p.id))
}

pub fn model_dir(models_dir: &Path, p: &Package) -> PathBuf {
    package_dir(models_dir, p).join(&p.folder)
}

pub fn ready(models_dir: &Path, p: &Package) -> bool {
    model_dir(models_dir, p).join(".ready").exists()
}

pub fn key(p: &Package) -> String {
    if p.id == KOKORO { "voice:kokoro".into() } else { format!("voice:pkg:{}", p.id) }
}

pub fn variant(p: &Package) -> Variant {
    Variant { quant: p.engine.clone(), bits: 0.0, size_bytes: p.size, files: vec![ModelFile { name: p.archive.clone(), size: p.size, sha256: p.sha256.clone() }] }
}

pub fn release_url(_repo: &str, file: &str) -> String {
    format!("{RELEASE}/{file}")
}

pub fn downloaded(models_dir: &Path, p: &Package) -> bool {
    models::is_installed(&package_dir(models_dir, p), &variant(p))
}

/// The `tar` that unpacks voice packages. Windows 10 and 11 ship their own
/// (bsdtar, which reads .tar.bz2), and it is named by full path rather than left to
/// PATH: a GNU tar installed with Git can sit earlier on PATH, and GNU tar reads a
/// Windows path like C:\voices as the remote host "C" and fails.
fn tar_program() -> std::path::PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        std::path::PathBuf::from(root).join("System32").join("tar.exe")
    } else {
        "tar".into()
    }
}

/// Unpacks a downloaded package (`tar` reads .tar.bz2 on macOS, Linux and Windows), then removes the archive.
pub async fn unpack(models_dir: &Path, p: &Package) -> AppResult<()> {
    if ready(models_dir, p) {
        return Ok(());
    }
    let d = package_dir(models_dir, p);
    if !downloaded(models_dir, p) {
        return Err(AppError::msg("That voice hasn't finished downloading."));
    }
    let archive = models::entry_path(&d, &variant(p));
    let out = tokio::process::Command::new(tar_program()).arg("-xjf").arg(&archive).arg("-C").arg(&d).output().await?;
    let m = model_dir(models_dir, p);
    if !out.status.success() || !m.is_dir() {
        return Err(AppError::msg("BYTE couldn't unpack that voice; deleting and downloading it again usually fixes it."));
    }
    let _ = std::fs::remove_file(&archive);
    std::fs::write(m.join(".ready"), b"ok")?;
    Ok(())
}

// ------------------------------------------------------------------ the speech tool's arguments

/// The first file in `dir` whose name passes `want` (sorted, so the choice is stable).
fn find(dir: &Path, want: impl Fn(&str) -> bool) -> Option<PathBuf> {
    let mut names: Vec<String> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| want(n)).collect();
    names.sort();
    names.first().map(|n| dir.join(n))
}

fn arg(flag: &str, p: &Path) -> String {
    format!("--{flag}={}", p.display())
}

/// How fast to speak: 1.0 normal, above 1 faster.
pub fn speed_factor(speed: &str, style: &str) -> f32 {
    let base = match speed {
        "slow" => 0.87,
        "fast" => 1.15,
        _ => 1.0,
    };
    let style = match style {
        "calm" => 0.94,
        "lively" => 1.06,
        _ => 1.0,
    };
    base * style
}

/// The speech tool's arguments for one chunk. The text is the last argument and never starts with "-".
pub fn args(p: &Package, dir: &Path, s: &Speaker, speed: f32, threads: usize, out: &Path, text: &str) -> AppResult<Vec<String>> {
    let missing = || AppError::msg(format!("The {} voice's files are incomplete; delete it and download it again.", p.name));
    let file = |name: &str| -> AppResult<PathBuf> {
        let f = dir.join(name);
        if f.exists() { Ok(f) } else { Err(missing()) }
    };
    let espeak = dir.join("espeak-ng-data");
    let length_scale = format!("{:.3}", 1.0 / speed.max(0.3));
    let mut a = match p.engine.as_str() {
        "kokoro" => {
            let model = find(dir, |n| n.starts_with("model") && n.ends_with(".onnx")).ok_or_else(missing)?;
            let lexicon = if s.lang == "en-GB" { "lexicon-gb-en.txt" } else { "lexicon-us-en.txt" };
            let mut a = vec![arg("kokoro-model", &model), arg("kokoro-voices", &file("voices.bin")?), arg("kokoro-tokens", &file("tokens.txt")?), arg("kokoro-data-dir", &espeak)];
            if dir.join(lexicon).exists() {
                a.push(arg("kokoro-lexicon", &dir.join(lexicon)));
            }
            a.push(format!("--kokoro-length-scale={length_scale}"));
            a
        }
        "vits" => {
            let model = find(dir, |n| n.ends_with(".onnx")).ok_or_else(missing)?;
            let mut a = vec![arg("vits-model", &model), arg("vits-tokens", &file("tokens.txt")?)];
            if espeak.is_dir() {
                a.push(arg("vits-data-dir", &espeak));
            }
            if dir.join("lexicon.txt").exists() {
                a.push(arg("vits-lexicon", &dir.join("lexicon.txt")));
            }
            if dir.join("dict").is_dir() {
                a.push(arg("vits-dict-dir", &dir.join("dict")));
            }
            a.push(format!("--vits-length-scale={length_scale}"));
            a
        }
        "kitten" => {
            let model = find(dir, |n| n.ends_with(".onnx")).ok_or_else(missing)?;
            vec![
                arg("kitten-model", &model),
                arg("kitten-voices", &file("voices.bin")?),
                arg("kitten-tokens", &file("tokens.txt")?),
                arg("kitten-data-dir", &espeak),
                format!("--kitten-length-scale={length_scale}"),
            ]
        }
        "supertonic" => {
            let part = |prefix: &str| find(dir, |n| n.starts_with(prefix) && n.ends_with(".onnx")).ok_or_else(missing);
            vec![
                arg("supertonic-duration-predictor", &part("duration_predictor")?),
                arg("supertonic-text-encoder", &part("text_encoder")?),
                arg("supertonic-vector-estimator", &part("vector_estimator")?),
                arg("supertonic-vocoder", &part("vocoder")?),
                arg("supertonic-tts-json", &file("tts.json")?),
                arg("supertonic-unicode-indexer", &file("unicode_indexer.bin")?),
                arg("supertonic-voice-style", &file("voice.bin")?),
                format!("--lang={}", s.lang.split('-').next().unwrap_or("en")),
                format!("--speed={speed:.3}"),
            ]
        }
        "pocket" => {
            let part = |prefix: &str| find(dir, |n| n.starts_with(prefix) && n.ends_with(".onnx")).ok_or_else(missing);
            let reference = s.reference.as_deref().filter(|r| !r.contains("..") && !r.starts_with('/')).ok_or_else(missing)?;
            vec![
                arg("pocket-lm-flow", &part("lm_flow")?),
                arg("pocket-lm-main", &part("lm_main")?),
                arg("pocket-encoder", &part("encoder")?),
                arg("pocket-decoder", &part("decoder")?),
                arg("pocket-text-conditioner", &part("text_conditioner")?),
                arg("pocket-vocab-json", &file("vocab.json")?),
                arg("pocket-token-scores-json", &file("token_scores.json")?),
                arg("reference-audio", &file(reference)?),
                format!("--speed={speed:.3}"),
            ]
        }
        other => return Err(AppError::msg(format!("BYTE doesn't know the {other} voice engine yet."))),
    };
    a.push(format!("--num-threads={threads}"));
    a.push(format!("--sid={}", s.sid));
    a.push(arg("output-filename", out));
    a.push(text.trim_start_matches(['-', ' ']).to_string());
    Ok(a)
}

/// Threads for the speech tool: half the cores (2–4), so the chat model keeps the rest.
pub fn threads() -> usize {
    (std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) / 2).clamp(2, 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete_and_unique() {
        let c = catalog();
        assert!(c.len() > 100, "{}", c.len());
        let mut ids: Vec<&str> = c.iter().map(|p| p.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), c.len());
        for p in c {
            assert!(p.size > 1_000_000 && p.sha256.len() == 64, "{}", p.id);
            assert!(["kokoro", "vits", "kitten", "supertonic", "pocket"].contains(&p.engine.as_str()), "{}", p.id);
            assert!(!p.speakers.is_empty() && !p.languages.is_empty(), "{}", p.id);
            assert!(p.archive.ends_with(".tar.bz2") && !p.archive.contains('/'), "{}", p.id);
        }
        for e in ["kokoro", "vits", "kitten", "supertonic", "pocket"] {
            assert!(c.iter().any(|p| p.engine == e), "{e}");
        }
        assert!(c.iter().filter(|p| p.languages.iter().any(|l| l.starts_with("en"))).count() >= 40);
    }

    #[test]
    fn picks_voices_and_reads_old_settings() {
        let (p, s) = pick("kokoro-v1_0/bm_george");
        assert_eq!((p.id.as_str(), s.sid, s.lang.as_str()), ("kokoro-v1_0", 26, "en-GB"));
        // v0.11.4 stored a Kokoro speaker alone.
        assert_eq!(pick("am_michael").1.sid, 16);
        assert_eq!(pick("nope/nobody").1.id, "af_heart");
        let (p, s) = pick("piper-en_GB-vctk-medium/p239");
        assert_eq!((p.engine.as_str(), s.sid), ("vits", 0));
        assert_eq!(key(package(KOKORO).unwrap()), "voice:kokoro");
        assert_eq!(package_dir(Path::new("/m"), package(KOKORO).unwrap()), crate::voice::dir(Path::new("/m")).join("kokoro"));
    }

    #[test]
    fn speed_and_style() {
        assert_eq!(speed_factor("normal", "natural"), 1.0);
        assert!(speed_factor("slow", "calm") < 0.85);
        assert!(speed_factor("fast", "lively") > 1.2);
    }

    fn fake(dir: &Path, files: &[&str]) {
        for f in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
    }

    #[test]
    fn arguments_per_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let out = Path::new("/tmp/o.wav");
        // Kokoro
        fake(d, &["model.onnx", "voices.bin", "tokens.txt", "lexicon-gb-en.txt", "espeak-ng-data/x"]);
        let (p, s) = pick("kokoro-v1_0/bm_george");
        let a = args(p, d, s, 1.15, 2, out, "--sid=9 Hello").unwrap();
        assert!(a.iter().any(|x| x.ends_with("lexicon-gb-en.txt")) && a.contains(&"--sid=26".into()));
        assert!(a.contains(&"--kokoro-length-scale=0.870".into()), "{a:?}");
        assert_eq!(a.last().unwrap(), "sid=9 Hello");
        // Piper
        let v = tmp.path().join("v");
        fake(&v, &["en_GB-vctk-medium.onnx", "en_GB-vctk-medium.onnx.json", "tokens.txt", "espeak-ng-data/x"]);
        let (p, s) = pick("piper-en_GB-vctk-medium/p236");
        let a = args(p, &v, s, 1.0, 2, out, "Hi").unwrap();
        assert!(a[0].ends_with("en_GB-vctk-medium.onnx") && a.contains(&"--sid=1".into()) && a.contains(&"--vits-length-scale=1.000".into()), "{a:?}");
        // Supertonic speaks the speaker's language.
        let st = tmp.path().join("st");
        fake(&st, &["duration_predictor.int8.onnx", "text_encoder.int8.onnx", "vector_estimator.int8.onnx", "vocoder.int8.onnx", "tts.json", "unicode_indexer.bin", "voice.bin"]);
        let (p, s) = pick("supertonic-3/M2");
        let a = args(p, &st, s, 1.0, 2, out, "Hi").unwrap();
        assert!(a.contains(&"--lang=en".into()) && a.contains(&"--sid=6".into()) && a.contains(&"--speed=1.000".into()), "{a:?}");
        // Pocket copies its reference recording.
        let pk = tmp.path().join("pk");
        fake(&pk, &["lm_flow.int8.onnx", "lm_main.int8.onnx", "encoder.onnx", "decoder.int8.onnx", "text_conditioner.onnx", "vocab.json", "token_scores.json", "test_wavs/bria.wav"]);
        let (p, s) = pick("pocket-tts/bria");
        assert!(args(p, &pk, s, 1.0, 2, out, "Hi").unwrap().iter().any(|x| x.ends_with("test_wavs/bria.wav")));
        // Missing files are a clear error, not a crash.
        let empty = tempfile::tempdir().unwrap();
        assert!(args(p, empty.path(), s, 1.0, 2, out, "Hi").is_err());
    }

    /// The real speech tool (BYTE_TEST_SHERPA_TTS) with every package unpacked in BYTE_TEST_VOICES
    /// (folders named as in the catalog): each makes about the right length of audio. Prints time and memory.
    #[tokio::test]
    #[ignore]
    async fn e2e_voices_speak() {
        let (Ok(_), Ok(root)) = (std::env::var("BYTE_TEST_SHERPA_TTS"), std::env::var("BYTE_TEST_VOICES")) else {
            eprintln!("skipped: set BYTE_TEST_SHERPA_TTS and BYTE_TEST_VOICES");
            return;
        };
        let mut tried = 0;
        for p in catalog() {
            let dir = Path::new(&root).join(&p.folder);
            if !dir.is_dir() {
                continue;
            }
            tried += 1;
            let s = &p.speakers[0];
            let started = std::time::Instant::now();
            let audio = crate::tts::synthesize(None, p, &dir, s, 1.0, "Oh, that's a great question! Here's what I found.").await.unwrap();
            let secs = audio.len() as f32 / crate::tts::SAMPLE_RATE as f32;
            println!("{} ({}): {secs:.1} s of audio in {:.1} s", p.id, s.name, started.elapsed().as_secs_f32());
            assert!((1.0..9.0).contains(&secs), "{}: {secs}", p.id);
            assert!(audio.iter().map(|x| x.abs()).fold(0.0, f32::max) > 0.03, "{} is silent", p.id);
        }
        assert!(tried > 0, "no voice packages in {root}");
    }
}
