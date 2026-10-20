//! Image Studio, part two: changing pictures that already exist (edit with a sentence, enlarge, crop, motion blur,
//! film grain, camera finish), the prompt helper, and "draw me …" from a chat. Windows builds only for now.
//!
//! Editing with a sentence ("remove the man on the left", "make it a watercolor", "restore this old photo") uses an
//! edit model from the catalog (Qwen-Image Edit, FLUX.1 Kontext, FLUX.2 klein). Everything else needs no model.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::imagegallery::{write_meta, Meta};
use crate::imagegen::{self, gallery_dir, Made, Options, Progress, Resolved, ACTIVE, PROGRESS_EVENT};
use crate::images::{self, ImageModel};
use crate::state::AppState;

/// A picture for the window to show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    pub path: String,
    pub preview: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditRequest {
    pub source: String,
    /// "edit", "enlarge", "crop", "motion", "grain" or "finish".
    pub op: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub quant: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub seed: Option<i64>,
    /// Longest side of the edited picture in pixels (edit).
    #[serde(default)]
    pub max_side: Option<u32>,
    /// Upscaler id and 2 or 4 (enlarge).
    #[serde(default)]
    pub upscaler: String,
    #[serde(default)]
    pub scale: Option<u32>,
    /// Crop rectangle as fractions of the picture.
    #[serde(default)]
    pub x: Option<f32>,
    #[serde(default)]
    pub y: Option<f32>,
    #[serde(default)]
    pub w: Option<f32>,
    #[serde(default)]
    pub h: Option<f32>,
    /// Motion blur length (pixels) and angle (degrees).
    #[serde(default)]
    pub length: Option<u32>,
    #[serde(default)]
    pub angle: Option<f32>,
    /// Grain and finish strength, 0-100.
    #[serde(default)]
    pub strength: Option<u32>,
}

/// Rounds a size to what the models draw well: a multiple of 16, longest side at most `max`.
pub fn fit_size(w: u32, h: u32, max: u32) -> (u32, u32) {
    let max = max.clamp(512, 1536);
    let f = (max as f32 / w.max(h) as f32).min(1.0);
    let r = |v: u32| (((v as f32 * f) / 16.0).round() as u32).max(1) * 16;
    (r(w), r(h))
}

/// The engine's command line for editing a picture with a sentence.
pub fn edit_args(m: &ImageModel, r: &Resolved, src: &Path, prompt: &str, size: (u32, u32), seed: i64, steps: u32, opt: &Options, out: &Path) -> Vec<String> {
    let s = imagegen::s;
    let mut a: Vec<String> = Vec::new();
    let mut push = |x: &[&str]| a.extend(x.iter().map(|v| v.to_string()));
    push(&["--diffusion-model", &s(&r.main)]);
    match m.family.as_str() {
        "Qwen-Image Edit" => {
            push(&["--cfg-scale", "2.5", "--sampling-method", "euler", "--flow-shift", "3", "--diffusion-fa"]);
            if m.name.contains("2511") {
                push(&["--model-args", "qwen_image_zero_cond_t=true"]);
            }
        }
        "FLUX.1 Kontext" => push(&["--cfg-scale", "1.0", "--sampling-method", "euler", "--diffusion-fa", "--clip-on-cpu"]),
        // FLUX.2 klein (the quick one has no guidance; the base one needs it).
        _ => push(&["--cfg-scale", if m.name.to_lowercase().contains("base") { "4.0" } else { "1.0" }, "--sampling-method", "euler"]),
    }
    for (flag, p) in [("--vae", &r.vae), ("--clip_l", &r.clip_l), ("--t5xxl", &r.t5xxl), ("--llm", &r.llm), ("--llm_vision", &r.mmproj)] {
        if let Some(p) = p {
            push(&[flag, &s(p)]);
        }
    }
    push(&["-r", &s(src), "-p", prompt, "--steps", &steps.max(1).to_string(), "-W", &size.0.to_string(), "-H", &size.1.to_string(), "--seed", &seed.to_string()]);
    let place = imagegen::placement_args(&r.main, opt);
    push(&place.iter().map(String::as_str).collect::<Vec<_>>());
    push(&["-o", &s(out)]);
    a
}

fn allowed(settings: &crate::settings::Settings) -> AppResult<()> {
    if !cfg!(windows) {
        return Err(AppError::msg("Editing pictures is in the Windows app for now."));
    }
    if settings.kids_mode {
        return Err(AppError::msg("Picture editing is off in kids mode."));
    }
    if !settings.image_studio {
        return Err(AppError::msg("Turn on picture making in Settings → Images first."));
    }
    if !settings.image_edit {
        return Err(AppError::msg("Turn on \"Edit photos\" in Settings → Images first."));
    }
    Ok(())
}

#[tauri::command]
pub async fn image_edit(app: AppHandle, state: State<'_, AppState>, req: EditRequest) -> AppResult<Made> {
    crate::lock::ensure(&state)?;
    let settings = state.settings.lock().await.clone();
    allowed(&settings)?;
    let models_dir = state.paths.models.clone();
    let src = crate::imagegallery::source_picture(&models_dir, &req.source)?;
    let token = CancellationToken::new();
    ACTIVE.lock().await.insert("generate".into(), token.clone());
    let r = edit(&app, &settings, &models_dir, &src, &req, &token).await;
    ACTIVE.lock().await.remove("generate");
    r
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| AppError::msg(e.to_string()))?
}

async fn edit(app: &AppHandle, settings: &crate::settings::Settings, models_dir: &Path, src: &Path, req: &EditRequest, cancel: &CancellationToken) -> AppResult<Made> {
    let started = Instant::now();
    let dir = gallery_dir(models_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let stamp = crate::imagegallery::now();
    let seed = req.seed.filter(|s| *s >= 0).unwrap_or((stamp as i64 * 7919) % 2_000_000_000);
    let out = dir.join(format!("{stamp}-{seed}-{}.png", req.op));
    let (s, o) = (src.to_path_buf(), out.clone());
    let mut notes: Vec<String> = Vec::new();
    let mut kind = "changed";
    let mut model_name = String::new();
    let phase = |p: &str| {
        let _ = app.emit(PROGRESS_EVENT, Progress { phase: p.into(), step: 0, total: 0 });
    };
    match req.op.as_str() {
        "edit" => {
            if req.prompt.trim().is_empty() {
                return Err(AppError::msg("Say what to change, for example \"remove the man on the left\"."));
            }
            let m = images::get(&req.model).ok_or_else(|| AppError::msg("Pick an editing model first."))?;
            let can = m.kind == "edit" || m.tags.iter().any(|t| t == "edit");
            if !can {
                return Err(AppError::msg("That model cannot edit photos."));
            }
            let v = imagegen::variant_of(m, &req.quant)?;
            let files = imagegen::resolve(models_dir, m, v)?;
            let exe = imagegen::engine_path(app, models_dir).ok_or_else(|| AppError::msg("The picture engine is not installed yet."))?;
            let (w, h) = crate::finish::dims(src)?;
            let size = fit_size(w, h, req.max_side.unwrap_or(1024));
            let vram = crate::gpu::best_discrete(crate::gpu::detect()).map(|g| g.dedicated_bytes);
            let opt = Options { placement: settings.image_placement.clone(), vram_bytes: vram, hires: None };
            let steps = if imagegen::distilled(m) || m.name.to_lowercase().contains("klein") && !m.name.to_lowercase().contains("base") { 4 } else { m.steps.max(20) };
            let args = edit_args(m, &files, src, req.prompt.trim(), size, seed, steps, &opt, &out);
            phase("loading");
            imagegen::run_engine(app, &exe, &args, "loading", cancel).await?;
            notes.push(format!("Edited with {}", m.name));
            model_name = m.name.clone();
            kind = "edited";
        }
        "enlarge" => {
            let exe = imagegen::engine_path(app, models_dir).ok_or_else(|| AppError::msg("The picture engine is not installed yet."))?;
            let (up, name, _) = imagegen::pick_upscaler(models_dir, &req.upscaler).ok_or_else(|| AppError::msg("Download an upscaler first (Settings → Images → Upscale)."))?;
            phase("enlarging");
            imagegen::run_engine(app, &exe, &imagegen::upscale_args(&up, src, &out), "enlarging", cancel).await?;
            // The upscaler makes 4x (or 2x for the 2x models). The owner can ask for less.
            let native = if name.starts_with("2x") || name.contains("2x") { 2 } else { 4 };
            let want = req.scale.unwrap_or(native);
            notes.push(format!("Enlarged {want}x with {name}"));
            if want < native {
                let (a, b) = (out.clone(), out.clone());
                let pct = (want * 100 / native).max(10);
                let tmp = a.with_extension("small.png");
                let t2 = tmp.clone();
                blocking(move || crate::finish::resize_file(&a, &t2, pct).map(|_| ())).await?;
                let _ = tokio::fs::rename(&tmp, &b).await;
            }
            kind = "enlarged";
        }
        "crop" => {
            let (x, y, w, h) = (req.x.unwrap_or(0.0), req.y.unwrap_or(0.0), req.w.unwrap_or(1.0), req.h.unwrap_or(1.0));
            blocking(move || crate::finish::crop_file(&s, &o, x, y, w, h).map(|_| ())).await?;
            notes.push("Cropped".into());
        }
        "motion" => {
            let (len, ang) = (req.length.unwrap_or(20), req.angle.unwrap_or(0.0));
            phase("polishing");
            blocking(move || crate::finish::motion_blur_file(&s, &o, len, ang)).await?;
            notes.push(format!("Motion blur {len}px at {ang:.0}°"));
        }
        "grain" => {
            let k = req.strength.unwrap_or(40);
            blocking(move || crate::finish::grain_file(&s, &o, k)).await?;
            notes.push(format!("Film grain {k}"));
        }
        "finish" => {
            let k = req.strength.unwrap_or(settings.image_finish_strength);
            let jpg = out.with_extension("jpg");
            let j2 = jpg.clone();
            phase("polishing");
            blocking(move || crate::finish::finish_file(&s, &j2, k)).await?;
            notes.push("Camera finish".into());
            return finish_up(settings, models_dir, jpg, kind, &req.prompt, &model_name, seed, started, notes).await;
        }
        _ => return Err(AppError::msg("Unknown edit.")),
    }
    finish_up(settings, models_dir, out, kind, &req.prompt, &model_name, seed, started, notes).await
}

#[allow(clippy::too_many_arguments)]
async fn finish_up(settings: &crate::settings::Settings, models_dir: &Path, out: PathBuf, kind: &str, prompt: &str, model: &str, seed: i64, started: Instant, notes: Vec<String>) -> AppResult<Made> {
    if settings.image_strip_info && out.extension().map(|e| e == "png").unwrap_or(false) {
        let p = out.clone();
        blocking(move || crate::finish::strip_file(&p)).await?;
    }
    let p = out.clone();
    let (preview, width, height) = blocking(move || crate::finish::preview(&p, 1600)).await?;
    let seconds = started.elapsed().as_secs_f32();
    let meta = Meta { kind: kind.into(), prompt: prompt.into(), model: model.into(), seed, seconds, notes: notes.clone(), created: crate::imagegallery::now(), saved: settings.image_keep != "saved", width, height };
    write_meta(&out, &meta);
    let _ = models_dir;
    if settings.image_keep == "none" {
        let _ = tokio::fs::remove_file(&out).await;
        let _ = tokio::fs::remove_file(out.with_extension("json")).await;
    }
    Ok(Made { path: out.to_string_lossy().into_owned(), preview, width, height, seed, seconds, notes })
}

// ---------- the prompt helper ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Improved {
    pub prompt: String,
    pub negative: String,
    /// Where it came from: "model" (BYTE's chat model wrote it) or "template".
    #[serde(default)]
    pub by: String,
}

/// What a plain template adds when no chat model is running.
pub fn template_improve(idea: &str, style: &str) -> Improved {
    let idea = idea.trim().trim_end_matches('.');
    let (extra, negative) = match style {
        "photo" => ("natural light, shot on a 35mm lens, shallow depth of field, fine detail, true-to-life colour", "painting, illustration, cartoon, cgi, 3d render, plastic skin, oversaturated, extra fingers, text, watermark"),
        "painting" => ("oil on canvas, visible brush strokes, rich colour, soft light", "photo, blurry, extra fingers, text, watermark"),
        "poster" => ("bold composition, a calm dark area for text, cinematic light, high detail", "blurry, cluttered, text, watermark"),
        "cartoon" => ("clean outlines, bright flat colours, friendly shapes", "photo, realistic, blurry, text, watermark"),
        _ => ("soft light, rich detail, balanced composition", "blurry, low quality, extra fingers, text, watermark"),
    };
    Improved { prompt: format!("{idea}, {extra}"), negative: negative.into(), by: "template".into() }
}

const HELPER_SYSTEM: &str = "You write prompts for text-to-image models. Turn the user's idea into one vivid prompt of 40 to 80 words: the subject, the setting, the light, the lens or medium and the mood. \
Keep the user's meaning; do not add people, text or brands they did not ask for. Write plain descriptive English, no lists. \
Also give a short 'negative' list of things to avoid (for example blurry, extra fingers, watermark). Reply with JSON only.";

fn helper_schema() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": { "prompt": { "type": "string" }, "negative": { "type": "string" } }, "required": ["prompt", "negative"] })
}

pub fn parse_improved(reply: &str) -> Option<Improved> {
    let (a, b) = (reply.find('{')?, reply.rfind('}')?);
    let mut i: Improved = serde_json::from_str(reply.get(a..=b)?).ok()?;
    i.prompt = i.prompt.trim().trim_matches('"').to_string();
    i.negative = i.negative.trim().to_string();
    i.by = "model".into();
    (i.prompt.split_whitespace().count() >= 5).then_some(i)
}

/// Turns a short idea into a better prompt, with BYTE's chat model when it is running.
#[tauri::command]
pub async fn image_improve_prompt(state: State<'_, AppState>, idea: String, style: String) -> AppResult<Improved> {
    crate::lock::ensure(&state)?;
    let settings = state.settings.lock().await.clone();
    if !cfg!(windows) || settings.kids_mode || !settings.image_prompt_helper {
        return Err(AppError::msg("Turn on the prompt helper in Settings → Images first."));
    }
    if idea.trim().is_empty() {
        return Err(AppError::msg("Write your idea first."));
    }
    if let Some(ep) = state.engine.endpoint().await {
        let user = format!("Style: {style}\nIdea: {}", idea.trim());
        if let Ok(reply) = crate::chat::complete_json(&state.local_http, &ep, HELPER_SYSTEM, &user, helper_schema(), 400).await {
            if let Some(i) = parse_improved(&reply) {
                return Ok(i);
            }
        }
    }
    Ok(template_improve(&idea, &style))
}

// ---------- "draw me …" in a chat ----------

/// The picture a message asks for ("draw me a lighthouse", "make an image of a cat in a hat"), if it does.
pub fn picture_request(question: &str) -> Option<String> {
    let q0 = question.trim().trim_end_matches(['.', '!', '?']);
    let leads = ["hey byte", "okay byte", "ok byte", "byte", "please", "can you", "could you", "would you", "will you", "go ahead and"];
    let mut rest = q0;
    loop {
        let t = rest.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        let tl = t.to_lowercase();
        let hit = leads.iter().find(|l| tl.starts_with(**l) && !tl[l.len()..].chars().next().is_some_and(|c| c.is_alphanumeric()));
        match hit {
            Some(l) => rest = &t[l.len()..],
            None => {
                rest = t;
                break;
            }
        }
    }
    let q = rest.to_string();
    let lower = q.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let skip = |ws: &[&str], i: usize| -> usize {
        let mut i = i;
        while i < ws.len() && ["me", "us", "a", "an", "the", "some", "another", "new"].contains(&ws[i]) {
            i += 1;
        }
        i
    };
    let rest_of = |from: usize| -> String { q.split_whitespace().skip(from).collect::<Vec<_>>().join(" ") };
    let first = *words.first()?;
    if ["draw", "sketch", "paint", "illustrate"].contains(&first) {
        let i = skip(&words, 1);
        let rest = rest_of(i);
        return (rest.split_whitespace().count() >= 1).then_some(rest);
    }
    if ["make", "create", "generate", "render", "produce", "show"].contains(&first) {
        let i = skip(&words, 1);
        let noun = *words.get(i)?;
        let styled = ["photo", "photograph", "painting", "drawing", "illustration", "portrait", "render", "poster", "wallpaper", "sketch"];
        let plain = ["picture", "image", "pic", "artwork"];
        if styled.contains(&noun) || plain.contains(&noun) {
            let mut j = i + 1;
            if let Some(w) = words.get(j) {
                if ["of", "showing", "with", "for", "about"].contains(w) {
                    j += 1;
                }
            }
            if plain.contains(&noun) {
                j = skip(&words, j);
            }
            let rest = rest_of(j);
            if rest.is_empty() {
                return None;
            }
            return Some(if styled.contains(&noun) { format!("{noun} of {rest}") } else { rest });
        }
    }
    None
}

/// The shape a chat picture gets, from its words.
pub fn chat_size(prompt: &str) -> (u32, u32) {
    let p = prompt.to_lowercase();
    if ["wallpaper", "landscape", "panorama", "banner", "cityscape", "skyline"].iter().any(|k| p.contains(k)) {
        (1344, 768)
    } else if ["portrait", "poster", "phone", "full body", "standing"].iter().any(|k| p.contains(k)) {
        (768, 1152)
    } else {
        (1024, 1024)
    }
}

/// Makes the picture a chat message asked for. Returns the words BYTE answers with.
pub async fn from_chat(app: &AppHandle, prompt: &str) -> String {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await.clone();
    if !cfg!(windows) || !settings.image_studio || !settings.image_from_chat || settings.kids_mode {
        return String::new();
    }
    let st = &state;
    let catalog = match crate::images::list(
        crate::images::Memory { vram: None, ram: 0 },
        &|m, v| imagegen::installed(&st.paths.models, m, v),
    ) {
        Ok(c) => c,
        Err(e) => return format!("I couldn't read the picture models: {e}"),
    };
    // The best installed picture model (highest quality).
    let pick = catalog
        .iter()
        .filter(|m| m.model.kind == "generate")
        .filter_map(|m| m.variants.iter().filter(|v| v.installed).min_by_key(|v| v.variant.quality_cost).map(|v| (m.model.quality, m.model.id.clone(), v.variant.quant.clone(), m.model.name.clone())))
        .max_by_key(|(q, ..)| *q);
    let Some((_, model, quant, name)) = pick else {
        return "I can draw that, but there is no picture model on this PC yet. Download one in Settings → Images → Image models (Z-Image Turbo is a good first one), then ask again.".into();
    };
    let (width, height) = chat_size(prompt);
    let req = imagegen::Request { model, quant, prompt: prompt.to_string(), negative: String::new(), width, height, steps: None, seed: None };
    match imagegen::generate(app, &state, req).await {
        Ok(m) => format!(
            "Here you go: I drew \"{prompt}\" with {name} in {:.0} seconds. It's in Settings → Images → Gallery{}.",
            m.seconds,
            if settings.image_keep == "none" { ", but your setting is to keep nothing, so open it now before the next picture" } else { "" }
        ),
        Err(AppError::Cancelled) => "Okay, I stopped.".into(),
        Err(e) => format!("I couldn't make that picture: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_picture_requests_in_chat() {
        assert_eq!(picture_request("draw me a lighthouse at dusk").as_deref(), Some("lighthouse at dusk"));
        assert_eq!(picture_request("Hey BYTE, can you make a picture of a cat in a hat?").as_deref(), Some("cat in a hat"));
        assert_eq!(picture_request("generate an image of a red bicycle").as_deref(), Some("red bicycle"));
        assert_eq!(picture_request("create a photo of my grandmother's kitchen").as_deref(), Some("photo of my grandmother's kitchen"));
        assert_eq!(picture_request("please paint a stormy sea").as_deref(), Some("stormy sea"));
    }

    #[test]
    fn ordinary_messages_are_not_picture_requests() {
        assert_eq!(picture_request("make a list of groceries"), None);
        assert_eq!(picture_request("how do I draw blood for a test"), None);
        assert_eq!(picture_request("create a plan for my week"), None);
        assert_eq!(picture_request("what is the picture on the wall"), None);
        assert_eq!(picture_request("show me the weather"), None);
    }

    #[test]
    fn chat_pictures_get_a_shape_from_their_words() {
        assert_eq!(chat_size("a wallpaper of mountains"), (1344, 768));
        assert_eq!(chat_size("a poster for a bake sale"), (768, 1152));
        assert_eq!(chat_size("a red bicycle"), (1024, 1024));
    }

    #[test]
    fn edit_sizes_keep_the_shape_in_multiples_of_16() {
        assert_eq!(fit_size(4000, 3000, 1024), (1024, 768));
        assert_eq!(fit_size(500, 800, 1024), (496, 800));
        let (w, h) = fit_size(1234, 987, 900);
        assert!(w % 16 == 0 && h % 16 == 0 && w.max(h) <= 912);
    }

    #[test]
    fn the_helpers_reply_is_read_leniently() {
        let r = "Sure! {\"prompt\": \"A weathered lighthouse on black rocks at golden hour, warm low light, soft sea haze\", \"negative\": \"blurry, watermark\"}";
        let i = parse_improved(r).unwrap();
        assert!(i.prompt.starts_with("A weathered") && i.by == "model");
        assert!(parse_improved("{\"prompt\": \"cat\", \"negative\": \"\"}").is_none());
        assert!(parse_improved("no json here").is_none());
    }

    #[test]
    fn the_template_fallback_keeps_the_idea() {
        let i = template_improve("a lighthouse.", "photo");
        assert!(i.prompt.starts_with("a lighthouse, natural light") && i.negative.contains("watermark") && i.by == "template");
    }

    #[test]
    fn qwen_edit_2511_gets_its_extra_switch_and_the_vision_file() {
        let m = images::get("unsloth-qwen-image-edit-2511-gguf").expect("catalog has Qwen edit 2511");
        let r = Resolved { main: "m.gguf".into(), vae: Some("v.safetensors".into()), llm: Some("l.gguf".into()), mmproj: Some("p.gguf".into()), ..Default::default() };
        let opt = Options { placement: "auto".into(), vram_bytes: Some(16 << 30), hires: None };
        let j = edit_args(m, &r, Path::new("in.png"), "remove the man", (1024, 768), 3, 20, &opt, Path::new("o.png")).join(" ");
        assert!(j.contains("qwen_image_zero_cond_t=true") && j.contains("--llm_vision p.gguf") && j.contains("-r in.png") && j.contains("-W 1024 -H 768"));
        assert!(j.ends_with("-o o.png"));
    }

    #[test]
    fn kontext_and_klein_command_lines() {
        let opt = Options { placement: "auto".into(), vram_bytes: Some(16 << 30), hires: None };
        let r = Resolved { main: "m.gguf".into(), vae: Some("v".into()), clip_l: Some("c".into()), t5xxl: Some("t".into()), llm: Some("l".into()), ..Default::default() };
        let k = edit_args(images::get("quantstack-flux-1-kontext-dev-gguf").expect("kontext"), &r, Path::new("i.png"), "x", (512, 512), 1, 28, &opt, Path::new("o.png")).join(" ");
        assert!(k.contains("--cfg-scale 1.0") && k.contains("--clip_l c") && k.contains("--t5xxl t"));
        let q = edit_args(images::get("leejet-flux-2-klein-4b-gguf").expect("klein"), &r, Path::new("i.png"), "x", (512, 512), 1, 4, &opt, Path::new("o.png")).join(" ");
        assert!(q.contains("--cfg-scale 1.0") && q.contains("--steps 4"));
    }
}
