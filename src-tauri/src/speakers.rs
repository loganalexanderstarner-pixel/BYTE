//! Speaker labels (Phase 11): "who said what" in transcripts of recordings.
//!
//! sherpa-onnx's offline speaker-diarization tool ships as a third sidecar
//! (`sherpa-diarize`, built by `scripts/build-sherpa.sh`). It reads a 16 kHz
//! WAV and answers `start -- end speaker_NN` lines, using two small models
//! (pyannote segmentation + a speaker-embedding model, ~32 MB) downloaded on
//! first use into `<models>/voice/`. BYTE matches whisper's timed segments to
//! those turns and writes `Speaker 1 [0:00]: …` blocks. Only audio files are
//! labelled; dictation from the mic never is.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::models::{self, ModelFile, Variant};
use crate::state::AppState;
use crate::voice::{self, Segment};

const SIDECAR: &str = "sherpa-diarize";
const TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// sherpa's clustering threshold when the number of speakers isn't known
/// (larger → fewer speakers). 0.9 suits the English voxceleb embedding model.
const THRESHOLD: f32 = 0.9;

/// One of the two speaker-label models.
struct Part {
    key: &'static str,
    repo: &'static str,
    file: &'static str,
    size: u64,
    sha256: &'static str,
}

const PARTS: &[Part] = &[
    Part {
        key: "voice:seg",
        repo: "csukuangfj/sherpa-onnx-pyannote-segmentation-3-0",
        file: "model.onnx",
        size: 5_992_913,
        sha256: "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079",
    },
    Part {
        key: "voice:emb",
        repo: "csukuangfj/speaker-embedding-models",
        file: "3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx",
        size: 26_485_263,
        sha256: "c59158379255ad66e161679cca6af8d52d51e389e3224ab7d7a7baae295c2db5",
    },
];

/// The segmentation model is called `model.onnx` upstream; it gets its own folder so the name can't clash.
fn part_dir(models_dir: &Path, p: &Part) -> PathBuf {
    voice::dir(models_dir).join(p.key.trim_start_matches("voice:"))
}

fn variant(p: &Part) -> Variant {
    Variant { quant: "onnx".into(), bits: 0.0, size_bytes: p.size, files: vec![ModelFile { name: p.file.into(), size: p.size, sha256: p.sha256.into() }] }
}

fn path_of(models_dir: &Path, p: &Part) -> PathBuf {
    models::entry_path(&part_dir(models_dir, p), &variant(p))
}

/// Both models are downloaded.
pub fn ready(models_dir: &Path) -> bool {
    PARTS.iter().all(|p| models::is_installed(&part_dir(models_dir, p), &variant(p)))
}

/// One diarization turn: who spoke from `start_ms` to `end_ms`.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: u32,
}

/// `0.318 -- 6.865 speaker_00` lines (sherpa-onnx's output) → turns.
pub fn parse_turns(stdout: &str) -> Vec<Turn> {
    let secs = |t: &str| t.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(|v| (v * 1000.0).round() as u64);
    stdout
        .lines()
        .filter_map(|l| {
            let (a, rest) = l.trim().split_once("--")?;
            let mut words = rest.split_whitespace();
            let b = words.next()?;
            let speaker = words.next()?.strip_prefix("speaker_")?.parse::<u32>().ok()?;
            Some(Turn { start_ms: secs(a)?, end_ms: secs(b)?, speaker })
        })
        .collect()
}

/// The speaker who talks most during a segment (None when no turn overlaps it).
fn speaker_of(seg: &Segment, turns: &[Turn]) -> Option<u32> {
    let mut best: Option<(u32, u64)> = None;
    for t in turns {
        let overlap = seg.end_ms.min(t.end_ms).saturating_sub(seg.start_ms.max(t.start_ms));
        let total = turns.iter().filter(|x| x.speaker == t.speaker).map(|x| seg.end_ms.min(x.end_ms).saturating_sub(seg.start_ms.max(x.start_ms))).sum::<u64>();
        if overlap > 0 && best.is_none_or(|(_, b)| total > b) {
            best = Some((t.speaker, total));
        }
    }
    best.map(|(s, _)| s)
}

/// `m:ss` or `h:mm:ss`.
fn stamp(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Segments + turns → `Speaker 1 [0:00]: …` paragraphs. Speakers are numbered in the order they first talk;
/// a segment no turn covers stays with whoever spoke before it.
pub fn label(segments: &[Segment], turns: &[Turn]) -> (String, usize) {
    // Raw speaker per segment; gaps take the speaker before them (or, at the start, the first one heard).
    let mut raw: Vec<Option<u32>> = segments.iter().map(|s| speaker_of(s, turns)).collect();
    let first = raw.iter().flatten().next().copied();
    let mut last = first;
    for r in raw.iter_mut() {
        match r {
            Some(s) => last = Some(*s),
            None => *r = last,
        }
    }
    let mut names: Vec<Option<u32>> = vec![];
    let mut blocks: Vec<(usize, u64, String)> = vec![];
    for (seg, who) in segments.iter().zip(raw) {
        let who = match names.iter().position(|n| *n == who) {
            Some(i) => i,
            None => {
                names.push(who);
                names.len() - 1
            }
        };
        match blocks.last_mut() {
            Some((w, _, text)) if *w == who => {
                text.push(' ');
                text.push_str(&seg.text);
            }
            _ => blocks.push((who, seg.start_ms, seg.text.clone())),
        }
    }
    let text = blocks.iter().map(|(w, at, t)| format!("Speaker {} [{}]: {}", w + 1, stamp(*at), t)).collect::<Vec<_>>().join("\n\n");
    (text, names.len())
}

fn args(models_dir: &Path, wav: &Path, speakers: Option<u32>) -> Vec<String> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    let mut a = vec![
        format!("--segmentation.pyannote-model={}", path_of(models_dir, &PARTS[0]).display()),
        format!("--embedding.model={}", path_of(models_dir, &PARTS[1]).display()),
        format!("--segmentation.num-threads={threads}"),
        format!("--embedding.num-threads={threads}"),
    ];
    match speakers {
        Some(n) if (1..=20).contains(&n) => a.push(format!("--clustering.num-clusters={n}")),
        _ => a.push(format!("--clustering.cluster-threshold={THRESHOLD}")),
    }
    a.push(wav.to_string_lossy().into_owned());
    a
}

/// Runs the diarization tool: the test binary when BYTE_TEST_SHERPA is set, else the bundled sidecar.
async fn run(app: Option<&AppHandle>, args: Vec<String>) -> AppResult<String> {
    let (ok, stdout, stderr) = match (std::env::var("BYTE_TEST_SHERPA").ok().filter(|b| !b.is_empty()), app) {
        (Some(bin), _) => {
            let fut = tokio::process::Command::new(bin).args(&args).kill_on_drop(true).output();
            let out = tokio::time::timeout(TIMEOUT, fut).await.map_err(|_| AppError::msg("Labelling speakers took too long."))??;
            (out.status.success(), out.stdout, out.stderr)
        }
        (None, Some(app)) => {
            use tauri_plugin_shell::ShellExt;
            let cmd = app.shell().sidecar(SIDECAR).map_err(|e| AppError::msg(format!("The speaker-label tool is missing from this build: {e}")))?;
            let out = tokio::time::timeout(TIMEOUT, cmd.args(args).output())
                .await
                .map_err(|_| AppError::msg("Labelling speakers took too long."))?
                .map_err(|e| AppError::msg(format!("The speaker-label tool didn't start: {e}")))?;
            (out.status.success(), out.stdout, out.stderr)
        }
        (None, None) => return Err(AppError::msg("The speaker-label tool isn't available here.")),
    };
    if !ok {
        let err = String::from_utf8_lossy(&stderr);
        return Err(AppError::msg(format!("Speaker labels failed: {}", err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error"))));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// Who spoke when, in a 16 kHz mono WAV.
pub async fn diarize(app: Option<&AppHandle>, models_dir: &Path, wav: &Path, speakers: Option<u32>) -> AppResult<Vec<Turn>> {
    if !ready(models_dir) {
        return Err(AppError::msg("Speaker labels need their models first: Settings → Models → Voice."));
    }
    Ok(parse_turns(&run(app, args(models_dir, wav, speakers)).await?))
}

/// A recording's transcript with speaker labels.
pub async fn labelled_transcript(app: Option<&AppHandle>, models_dir: &Path, chosen: &str, language: &str, audio: &Path) -> AppResult<String> {
    let tmp = tempfile::Builder::new().prefix("byte-speakers").suffix(".wav").tempfile()?;
    let wav = voice::wav_16k(audio, tmp.path()).await?;
    let segments = voice::transcribe_segments(app, models_dir, chosen, language, &wav).await?;
    if segments.is_empty() {
        return Ok(String::new());
    }
    let turns = diarize(app, models_dir, &wav, None).await?;
    let (text, n) = label(&segments, &turns);
    let heard = if n == 1 { "1 speaker".to_string() } else { format!("{n} speakers") };
    Ok(format!("[{heard} heard; labels are BYTE's best guess of who is speaking]\n{text}"))
}

// ------------------------------------------------------------------ commands

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakersStatus {
    pub installed: bool,
    pub size_bytes: u64,
    /// Download keys, as in `models://download` events.
    pub keys: Vec<String>,
}

#[tauri::command]
pub async fn speakers_status(state: State<'_, AppState>) -> AppResult<SpeakersStatus> {
    Ok(SpeakersStatus { installed: ready(&state.paths.models), size_bytes: PARTS.iter().map(|p| p.size).sum(), keys: PARTS.iter().map(|p| p.key.to_string()).collect() })
}

#[tauri::command]
pub async fn speakers_download(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    for p in PARTS {
        let d = part_dir(&state.paths.models, p);
        std::fs::create_dir_all(&d)?;
        state.downloads.start(app.clone(), state.net.clone(), d, p.repo.into(), variant(p), p.key.into()).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn speakers_delete(state: State<'_, AppState>) -> AppResult<()> {
    for p in PARTS {
        state.downloads.pause(p.key).await;
        models::delete(&part_dir(&state.paths.models, p), &variant(p))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(a: u64, b: u64, t: &str) -> Segment {
        Segment { start_ms: a, end_ms: b, text: t.into() }
    }

    #[test]
    fn reads_sherpa_output() {
        let out = "OfflineSpeakerDiarizationConfig(...)\nStarted\n0.318 -- 6.865 speaker_00\n7.017 -- 10.747 speaker_01\n10.851 -- 14.000 speaker_00 confidence=0.912\nbad -- line speaker_x\n";
        assert_eq!(
            parse_turns(out),
            vec![Turn { start_ms: 318, end_ms: 6865, speaker: 0 }, Turn { start_ms: 7017, end_ms: 10747, speaker: 1 }, Turn { start_ms: 10851, end_ms: 14000, speaker: 0 }]
        );
    }

    #[test]
    fn labels_who_said_what() {
        let turns = vec![Turn { start_ms: 0, end_ms: 5000, speaker: 3 }, Turn { start_ms: 5000, end_ms: 9000, speaker: 1 }, Turn { start_ms: 9000, end_ms: 12000, speaker: 3 }];
        let segs = vec![seg(0, 2500, "Hi, how was the trip?"), seg(2500, 4800, "Did you get there okay?"), seg(5100, 8800, "Yes, it was great."), seg(9100, 11000, "Good to hear.")];
        let (text, n) = label(&segs, &turns);
        assert_eq!(n, 2);
        assert_eq!(text, "Speaker 1 [0:00]: Hi, how was the trip? Did you get there okay?\n\nSpeaker 2 [0:05]: Yes, it was great.\n\nSpeaker 1 [0:09]: Good to hear.");
    }

    #[test]
    fn uncovered_segments_stay_with_the_last_speaker() {
        let turns = vec![Turn { start_ms: 0, end_ms: 3000, speaker: 0 }];
        let (text, n) = label(&[seg(0, 2000, "One."), seg(5000, 6000, "Two.")], &turns);
        assert_eq!((text.as_str(), n), ("Speaker 1 [0:00]: One. Two.", 1));
        // Speech before the first turn goes with the first speaker heard.
        let (text, n) = label(&[seg(0, 900, "Um."), seg(1000, 2000, "Hello.")], &[Turn { start_ms: 1000, end_ms: 2000, speaker: 4 }]);
        assert_eq!((text.as_str(), n), ("Speaker 1 [0:00]: Um. Hello.", 1));
        // No turns at all: everything is speaker 1.
        let (text, n) = label(&[seg(0, 1000, "Alone.")], &[]);
        assert_eq!((text.as_str(), n), ("Speaker 1 [0:00]: Alone.", 1));
        assert_eq!(stamp(3_723_000), "1:02:03");
    }

    #[test]
    fn arguments_are_fixed_flags_and_a_path() {
        let a = args(Path::new("/m"), Path::new("/tmp/x.wav"), None);
        assert!(a.iter().any(|x| x == "--clustering.cluster-threshold=0.9"));
        assert_eq!(a.last().unwrap(), "/tmp/x.wav");
        assert!(args(Path::new("/m"), Path::new("x"), Some(3)).contains(&"--clustering.num-clusters=3".to_string()));
        assert!(args(Path::new("/m"), Path::new("x"), Some(99)).iter().any(|x| x.starts_with("--clustering.cluster-threshold")));
    }

    /// Real tools + models (BYTE_TEST_SHERPA, BYTE_TEST_SPEAKER_SEG, BYTE_TEST_SPEAKER_EMB, BYTE_TEST_SPEAKERS_AUDIO =
    /// sherpa-onnx's 4-speaker test WAV): at least two speakers are told apart.
    #[tokio::test]
    #[ignore]
    async fn e2e_speakers() {
        let (Ok(_), Ok(seg_model), Ok(emb_model), Ok(audio)) =
            (std::env::var("BYTE_TEST_SHERPA"), std::env::var("BYTE_TEST_SPEAKER_SEG"), std::env::var("BYTE_TEST_SPEAKER_EMB"), std::env::var("BYTE_TEST_SPEAKERS_AUDIO"))
        else {
            eprintln!("skipped: set BYTE_TEST_SHERPA, BYTE_TEST_SPEAKER_SEG, BYTE_TEST_SPEAKER_EMB and BYTE_TEST_SPEAKERS_AUDIO");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        for (p, src) in PARTS.iter().zip([seg_model, emb_model]) {
            std::fs::create_dir_all(part_dir(tmp.path(), p)).unwrap();
            std::fs::copy(src, path_of(tmp.path(), p)).unwrap();
        }
        assert!(ready(tmp.path()));
        let turns = diarize(None, tmp.path(), Path::new(&audio), None).await.unwrap();
        let speakers: std::collections::BTreeSet<u32> = turns.iter().map(|t| t.speaker).collect();
        println!("{} turns, speakers {speakers:?}", turns.len());
        assert!(speakers.len() >= 2, "{turns:?}");
    }

    /// The whole pipeline (whisper + speaker labels) on whisper.cpp's JFK sample: one speaker, labelled.
    /// Needs the e2e_speakers variables plus BYTE_TEST_WHISPER, BYTE_TEST_WHISPER_MODEL (base.en) and BYTE_TEST_WHISPER_AUDIO.
    #[tokio::test]
    #[ignore]
    async fn e2e_labelled_transcript() {
        let vars = ["BYTE_TEST_SHERPA", "BYTE_TEST_SPEAKER_SEG", "BYTE_TEST_SPEAKER_EMB", "BYTE_TEST_WHISPER", "BYTE_TEST_WHISPER_MODEL", "BYTE_TEST_WHISPER_AUDIO"];
        let Ok(v) = vars.iter().map(std::env::var).collect::<Result<Vec<_>, _>>() else {
            eprintln!("skipped: set {}", vars.join(", "));
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        for (p, src) in PARTS.iter().zip([&v[1], &v[2]]) {
            std::fs::create_dir_all(part_dir(tmp.path(), p)).unwrap();
            std::fs::copy(src, path_of(tmp.path(), p)).unwrap();
        }
        std::fs::create_dir_all(voice::dir(tmp.path())).unwrap();
        std::fs::copy(&v[4], voice::dir(tmp.path()).join("ggml-base.en.bin")).unwrap();
        let text = labelled_transcript(None, tmp.path(), "base-en", "auto", Path::new(&v[5])).await.unwrap();
        println!("{text}");
        assert!(text.starts_with("[1 speaker heard"), "{text}");
        assert!(text.contains("Speaker 1 [0:00]: And so my fellow Americans"), "{text}");
    }
}
