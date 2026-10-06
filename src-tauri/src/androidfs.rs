//! Files on Android. The system file picker hands back `content://` links, not paths, so the Rust side can't open
//! them with `std::fs`. `localize` copies a picked file into the app's cache (through tauri-plugin-fs, which opens
//! content links) and returns a normal path; `save` writes to a link the user chose in the save dialog. On a computer
//! both pass paths straight through. Nothing here is Mac-specific.

use std::path::PathBuf;

use tauri::AppHandle;

use crate::error::{AppError, AppResult};

/// A link from Android's file picker (rather than a path).
pub fn is_uri(p: &str) -> bool {
    p.starts_with("content://")
}

/// The file's kind from its first bytes, as an extension ("" when unknown): picked files often have no name.
pub fn sniff_ext(b: &[u8]) -> &'static str {
    if b.starts_with(b"%PDF") {
        return "pdf";
    }
    if b.starts_with(b"\x89PNG") {
        return "png";
    }
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "jpg";
    }
    if b.starts_with(b"GIF8") {
        return "gif";
    }
    if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return "webp";
    }
    if b.len() > 12 && &b[4..8] == b"ftyp" && matches!(&b[8..12], b"heic" | b"heix" | b"mif1" | b"msf1") {
        return "heic";
    }
    if b.starts_with(b"PK\x03\x04") {
        // An Office file is a zip whose entries are named by part.
        let has = |needle: &[u8]| b.windows(needle.len()).any(|w| w == needle);
        return if has(b"word/") {
            "docx"
        } else if has(b"ppt/") {
            "pptx"
        } else if has(b"xl/") {
            "xlsx"
        } else {
            "zip"
        };
    }
    if b.starts_with(b"RIFF") || b.starts_with(b"ID3") || b.starts_with(b"OggS") || b.starts_with(b"fLaC") || (b.len() > 8 && &b[4..8] == b"ftyp") {
        return "";
    }
    if std::str::from_utf8(&b[..b.len().min(4096)]).is_ok() || std::str::from_utf8(b).is_ok() {
        return "txt";
    }
    ""
}

/// A readable name from the link when it carries one ("…/primary%3ADownload%2Freport.pdf" -> "report.pdf").
pub fn name_from_uri(uri: &str) -> Option<String> {
    let last = uri.rsplit('/').next()?;
    let decoded = percent_decode(last);
    let name = decoded.rsplit(['/', ':']).next()?.trim().to_string();
    (name.contains('.') && name.len() <= 120 && !name.starts_with('.')).then_some(name)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A normal path for something the user picked: the path itself, or a cache copy of a content link.
pub fn localize(app: &AppHandle, picked: &str) -> AppResult<PathBuf> {
    if !is_uri(picked) {
        return Ok(PathBuf::from(picked));
    }
    let bytes = read_uri(app, picked)?;
    use tauri::Manager;
    let dir = app.path().app_cache_dir().map_err(|e| AppError::msg(format!("no cache folder: {e}")))?.join("picked");
    std::fs::create_dir_all(&dir)?;
    let name = match name_from_uri(picked) {
        Some(n) => n,
        None => {
            let ext = sniff_ext(&bytes);
            let stem = format!("file-{:08x}", crc(picked.as_bytes()));
            if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
        }
    };
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// Writes to a path, or to the link chosen in the save dialog.
pub fn save(app: &AppHandle, target: &str, bytes: &[u8]) -> AppResult<()> {
    if !is_uri(target) {
        if let Some(dir) = std::path::Path::new(target).parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(target, bytes)?;
        return Ok(());
    }
    write_uri(app, target, bytes)
}

fn crc(b: &[u8]) -> u32 {
    b.iter().fold(0x811c9dc5u32, |h, x| (h ^ *x as u32).wrapping_mul(0x01000193))
}

#[cfg(target_os = "android")]
fn read_uri(app: &AppHandle, uri: &str) -> AppResult<Vec<u8>> {
    use tauri_plugin_fs::FsExt;
    let url = url::Url::parse(uri).map_err(|e| AppError::msg(format!("bad file link: {e}")))?;
    app.fs().read(url).map_err(|e| AppError::msg(format!("couldn't read the file you picked: {e}")))
}

#[cfg(target_os = "android")]
fn write_uri(app: &AppHandle, uri: &str, bytes: &[u8]) -> AppResult<()> {
    use std::io::Write as _;
    use tauri_plugin_fs::{FsExt, OpenOptions};
    let url = url::Url::parse(uri).map_err(|e| AppError::msg(format!("bad file link: {e}")))?;
    let mut opts = OpenOptions::new();
    opts.write(true).truncate(true).create(true);
    let mut f = app.fs().open(url, opts).map_err(|e| AppError::msg(format!("couldn't save there: {e}")))?;
    f.write_all(bytes)?;
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn read_uri(_: &AppHandle, _: &str) -> AppResult<Vec<u8>> {
    Err(AppError::msg("content links only exist on Android"))
}

#[cfg(not(target_os = "android"))]
fn write_uri(_: &AppHandle, _: &str, _: &[u8]) -> AppResult<()> {
    Err(AppError::msg("content links only exist on Android"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_common_files() {
        assert_eq!(sniff_ext(b"%PDF-1.7 ..."), "pdf");
        assert_eq!(sniff_ext(b"\x89PNG\r\n\x1a\n"), "png");
        assert_eq!(sniff_ext(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), "jpg");
        assert_eq!(sniff_ext(b"PK\x03\x04....word/document.xml"), "docx");
        assert_eq!(sniff_ext(b"PK\x03\x04....ppt/slides/slide1.xml"), "pptx");
        assert_eq!(sniff_ext(b"PK\x03\x04....xl/workbook.xml"), "xlsx");
        assert_eq!(sniff_ext("plain notes, with ünïcode".as_bytes()), "txt");
        assert_eq!(sniff_ext(&[0, 159, 146, 150, 0xFE, 0xFF, 0x80]), "");
    }

    #[test]
    fn names_come_from_the_link_when_it_has_one() {
        assert_eq!(name_from_uri("content://com.android.externalstorage.documents/document/primary%3ADownload%2Freport.pdf").as_deref(), Some("report.pdf"));
        assert_eq!(name_from_uri("content://com.android.providers.media.documents/document/document%3A1234"), None);
        assert_eq!(name_from_uri("content://x/doc/My%20Notes.txt").as_deref(), Some("My Notes.txt"));
    }

    #[test]
    fn paths_pass_straight_through() {
        assert!(!is_uri("/home/me/file.pdf"));
        assert!(is_uri("content://media/external/file/12"));
    }
}
