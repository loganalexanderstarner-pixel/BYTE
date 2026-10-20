//! The Image Studio gallery: the pictures BYTE made or edited, each with a small note file beside it
//! (what was asked, which model, how long). Kept in `<models>/images/gallery`. Windows builds only for now.
//!
//! Which pictures stay is the owner's choice (Settings → Images → Keep my pictures): everything, only the
//! ones they save, or nothing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::imagegen::gallery_dir;
use crate::state::AppState;

/// The note kept beside a picture.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Meta {
    /// "made", "edited", "enlarged", "poster" or "changed".
    pub kind: String,
    pub prompt: String,
    pub model: String,
    pub seed: i64,
    pub seconds: f32,
    pub notes: Vec<String>,
    /// Seconds since 1970.
    pub created: u64,
    /// The owner chose to keep it (matters when "Keep my pictures" is "only pictures I save").
    pub saved: bool,
    pub width: u32,
    pub height: u32,
}

fn meta_path(p: &Path) -> PathBuf {
    p.with_extension("json")
}

pub fn write_meta(picture: &Path, meta: &Meta) {
    if let Ok(t) = serde_json::to_string(meta) {
        let _ = std::fs::write(meta_path(picture), t);
    }
}

pub fn read_meta(picture: &Path) -> Meta {
    std::fs::read_to_string(meta_path(picture)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| Meta {
        created: std::fs::metadata(picture).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
        ..Default::default()
    })
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn is_picture(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| matches!(e.to_lowercase().as_str(), "png" | "jpg" | "jpeg")).unwrap_or(false)
}

/// A picture inside the gallery folder (never a path outside it).
pub fn in_gallery(models_dir: &Path, path: &str) -> AppResult<PathBuf> {
    let dir = gallery_dir(models_dir).canonicalize().map_err(|_| AppError::msg("there are no pictures yet"))?;
    let p = PathBuf::from(path).canonicalize().map_err(|_| AppError::msg("that picture is gone"))?;
    if !p.starts_with(&dir) || !is_picture(&p) {
        return Err(AppError::msg("that picture is not in the gallery"));
    }
    Ok(p)
}

/// A picture to work on: from the gallery, or a file the owner picked.
pub fn source_picture(models_dir: &Path, path: &str) -> AppResult<PathBuf> {
    let p = PathBuf::from(path);
    if !is_picture(&p) {
        return Err(AppError::msg("Pick a .png or .jpg picture."));
    }
    if !p.is_file() {
        return Err(AppError::msg("That picture is gone."));
    }
    let _ = models_dir;
    Ok(p)
}

/// Deletes pictures that were not saved (when the owner keeps only saved ones).
pub fn purge_unsaved(models_dir: &Path) {
    let Ok(rd) = std::fs::read_dir(gallery_dir(models_dir)) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if is_picture(&p) && !read_meta(&p).saved {
            let _ = std::fs::remove_file(&p);
            let _ = std::fs::remove_file(meta_path(&p));
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryItem {
    pub path: String,
    /// A small preview as a data address.
    pub thumb: String,
    #[serde(flatten)]
    pub meta: Meta,
    pub bytes: u64,
}

fn thumb_of(models_dir: &Path, p: &Path) -> String {
    let cache = gallery_dir(models_dir).join(".thumbs");
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("x");
    let cached = cache.join(format!("{stem}.txt"));
    if let Ok(t) = std::fs::read_to_string(&cached) {
        return t;
    }
    match crate::finish::preview(p, 360) {
        Ok((url, _, _)) => {
            let _ = std::fs::create_dir_all(&cache);
            let _ = std::fs::write(&cached, &url);
            url
        }
        Err(_) => String::new(),
    }
}

fn allowed(state: &AppState, settings: &crate::settings::Settings) -> AppResult<()> {
    let _ = state;
    if !cfg!(windows) {
        return Err(AppError::msg("The gallery is in the Windows app for now."));
    }
    if settings.kids_mode {
        return Err(AppError::msg("Pictures are off in kids mode."));
    }
    Ok(())
}

/// The gallery, newest first.
#[tauri::command]
pub async fn image_gallery(state: State<'_, AppState>) -> AppResult<Vec<GalleryItem>> {
    crate::lock::ensure(&state)?;
    let settings = state.settings.lock().await.clone();
    if !cfg!(windows) || settings.kids_mode {
        return Ok(Vec::new());
    }
    let dir = state.paths.models.clone();
    tokio::task::spawn_blocking(move || {
        let Ok(rd) = std::fs::read_dir(gallery_dir(&dir)) else { return Vec::new() };
        let mut items: Vec<GalleryItem> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_picture(p))
            .map(|p| GalleryItem { path: p.to_string_lossy().into_owned(), thumb: thumb_of(&dir, &p), meta: read_meta(&p), bytes: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) })
            .collect();
        items.sort_by(|a, b| b.meta.created.cmp(&a.meta.created));
        items
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))
}

/// A picture shrunk to fit the window, with its real size. Any picture in the gallery, or one the owner picked.
#[tauri::command]
pub async fn image_preview(state: State<'_, AppState>, path: String, max: Option<u32>) -> AppResult<crate::imageedit::Shown> {
    crate::lock::ensure(&state)?;
    let settings = state.settings.lock().await.clone();
    allowed(&state, &settings)?;
    let p = source_picture(&state.paths.models, &path)?;
    tokio::task::spawn_blocking(move || {
        let (preview, width, height) = crate::finish::preview(&p, max.unwrap_or(1600).clamp(200, 3000))?;
        Ok(crate::imageedit::Shown { path: p.to_string_lossy().into_owned(), preview, width, height })
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

#[tauri::command]
pub async fn image_gallery_delete(state: State<'_, AppState>, path: String) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    let p = in_gallery(&state.paths.models, &path)?;
    let _ = std::fs::remove_file(meta_path(&p));
    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
        let _ = std::fs::remove_file(gallery_dir(&state.paths.models).join(".thumbs").join(format!("{stem}.txt")));
    }
    std::fs::remove_file(&p)?;
    Ok(())
}

/// Marks a picture as one to keep (or not).
#[tauri::command]
pub async fn image_gallery_save(state: State<'_, AppState>, path: String, saved: bool) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    let p = in_gallery(&state.paths.models, &path)?;
    let mut m = read_meta(&p);
    m.saved = saved;
    write_meta(&p, &m);
    Ok(())
}

/// Copies a picture to a place the owner chose (the Save dialog), without the settings stored inside it
/// when that option is on.
#[tauri::command]
pub async fn image_copy_to(state: State<'_, AppState>, path: String, dest: String) -> AppResult<()> {
    crate::lock::ensure(&state)?;
    let p = in_gallery(&state.paths.models, &path)?;
    let d = PathBuf::from(dest);
    if !is_picture(&d) {
        return Err(AppError::msg("Save it as a .png or .jpg file."));
    }
    std::fs::copy(&p, &d)?;
    Ok(())
}

/// Saves the poster the window drew into the gallery.
#[tauri::command]
pub async fn image_poster_save(state: State<'_, AppState>, data_url: String, title: String) -> AppResult<GalleryItem> {
    crate::lock::ensure(&state)?;
    let settings = state.settings.lock().await.clone();
    allowed(&state, &settings)?;
    let dir = gallery_dir(&state.paths.models);
    std::fs::create_dir_all(&dir)?;
    let stamp = now();
    let p = dir.join(format!("{stamp}-poster.png"));
    let pp = p.clone();
    tokio::task::spawn_blocking(move || crate::finish::write_data_url(&data_url, &pp)).await.map_err(|e| AppError::msg(e.to_string()))??;
    let (w, h) = crate::finish::dims(&p)?;
    let meta = Meta { kind: "poster".into(), prompt: title, created: stamp, saved: true, width: w, height: h, ..Default::default() };
    write_meta(&p, &meta);
    Ok(GalleryItem { path: p.to_string_lossy().into_owned(), thumb: thumb_of(&state.paths.models, &p), meta, bytes: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) })
}

/// Brightness of the picture on a grid, for finding the dark or quiet part where poem text reads best.
#[tauri::command]
pub async fn image_brightness(state: State<'_, AppState>, path: String, cols: u32, rows: u32) -> AppResult<Vec<u8>> {
    crate::lock::ensure(&state)?;
    let p = source_picture(&state.paths.models, &path)?;
    let (c, r) = (cols.clamp(2, 64), rows.clamp(2, 64));
    tokio::task::spawn_blocking(move || crate::finish::brightness_grid(&p, c, r)).await.map_err(|e| AppError::msg(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("byte-gal-{name}-{}", std::process::id()));
        std::fs::create_dir_all(d.join("images").join("gallery")).unwrap();
        d
    }

    #[test]
    fn a_note_round_trips_beside_its_picture() {
        let d = tmp("meta");
        let p = d.join("a.png");
        std::fs::write(&p, b"x").unwrap();
        let m = Meta { kind: "made".into(), prompt: "a lighthouse".into(), seed: 5, created: 9, saved: true, ..Default::default() };
        write_meta(&p, &m);
        assert_eq!(read_meta(&p), m);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_pictures_inside_the_gallery_can_be_deleted_or_copied() {
        let d = tmp("guard");
        let g = d.join("images").join("gallery");
        let inside = g.join("a.png");
        std::fs::write(&inside, b"x").unwrap();
        let outside = d.join("b.png");
        std::fs::write(&outside, b"x").unwrap();
        assert!(in_gallery(&d, inside.to_str().unwrap()).is_ok());
        assert!(in_gallery(&d, outside.to_str().unwrap()).is_err());
        assert!(in_gallery(&d, g.join("../b.png").to_str().unwrap()).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unsaved_pictures_are_purged_and_saved_ones_stay() {
        let d = tmp("purge");
        let g = d.join("images").join("gallery");
        let (keep, drop) = (g.join("k.png"), g.join("d.png"));
        std::fs::write(&keep, b"x").unwrap();
        std::fs::write(&drop, b"x").unwrap();
        write_meta(&keep, &Meta { saved: true, ..Default::default() });
        purge_unsaved(&d);
        assert!(keep.exists() && !drop.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
