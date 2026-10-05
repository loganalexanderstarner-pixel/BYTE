//! Reading files people attach to a chat: PDF, Word, PowerPoint, Excel, text,
//! Markdown, CSV, code and saved web pages become plain text for the model;
//! photos are kept as images for models that can see them.
//!
//! Everything runs on this Mac. Long files aren't sent whole: when the chat is
//! answered, `for_model` keeps the passages that best match the question
//! (`tools::fetch::relevant_passages`) within the room the context has.

use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Largest file BYTE reads.
pub const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
/// Text kept per file (about 60k tokens); more than any Mac's context holds anyway.
pub const MAX_TEXT_CHARS: usize = 240_000;
/// Longest side photos are sent at (the vision encoders downscale anyway).
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    Pdf,
    Word,
    Slides,
    Sheet,
    Text,
    Web,
    Image,
    /// A recording, read as its transcript (voice.rs).
    Audio,
}

/// A file as the chat keeps it (in the message, so reloads and edits work).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ingested {
    pub name: String,
    pub kind: FileKind,
    /// Pages (PDF), slides or sheets, when the format has them.
    #[serde(default)]
    pub pages: Option<u32>,
    /// Extracted text (empty for photos).
    #[serde(default)]
    pub text: String,
    /// The text was cut at `MAX_TEXT_CHARS`.
    #[serde(default)]
    pub truncated: bool,
    /// Photos: a data URL for vision models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// The text was read from a scan or photo (text recognition), so it may
    /// have small mistakes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ocr: bool,
}

impl Ingested {
    /// A recording's transcript (voice.rs), capped like any file's text.
    pub fn transcript(name: String, text: String) -> Ingested {
        let truncated = text.chars().count() > MAX_TEXT_CHARS;
        let body: String = if truncated { text.chars().take(MAX_TEXT_CHARS).collect() } else { text };
        Ingested { name, kind: FileKind::Audio, pages: None, text: format!("[Transcript of the recording]\n{body}"), truncated, image: None, ocr: false }
    }
}

pub fn kind_of(path: &Path) -> Option<FileKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => FileKind::Pdf,
        "docx" | "docm" | "odt" => FileKind::Word,
        "pptx" | "pptm" | "odp" => FileKind::Slides,
        "xlsx" | "xlsm" | "ods" => FileKind::Sheet,
        "html" | "htm" | "xhtml" => FileKind::Web,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "bmp" => FileKind::Image,
        "txt" | "md" | "markdown" | "csv" | "tsv" | "json" | "jsonl" | "yaml" | "yml" | "toml" | "xml" | "log" | "ini"
        | "cfg" | "conf" | "tex" | "srt" | "vtt" | "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "java" | "kt"
        | "swift" | "c" | "h" | "cpp" | "hpp" | "cs" | "go" | "rb" | "php" | "sh" | "zsh" | "bash" | "sql" | "css"
        | "scss" | "lua" | "r" | "m" | "scala" | "dart" | "vue" | "svelte" => FileKind::Text,
        _ => return None,
    })
}

/// Photos the engine reads directly (llama.cpp decodes JPEG, PNG, GIF, BMP).
fn image_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

/// Photos larger than this are shrunk (vision models see ~1–2k pixels anyway).
const SHRINK_OVER_BYTES: u64 = 2_000_000;

/// A photo in a format the engine can decode. On macOS, HEIC/WebP photos are
/// converted and big photos shrunk to 2048 px with the built-in `sips` tool.
fn photo_for_model(path: &Path, ext: &str, bytes: Vec<u8>, name: &str) -> AppResult<(&'static str, Vec<u8>)> {
    let direct = image_mime(ext);
    if let Some(mime) = direct.filter(|_| bytes.len() as u64 <= SHRINK_OVER_BYTES) {
        return Ok((mime, bytes));
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::env::temp_dir().join(format!("byte-photo-{}.jpg", std::process::id()));
        let ok = std::process::Command::new("/usr/bin/sips")
            .args(["-s", "format", "jpeg", "-Z", "2048"])
            .arg(path)
            .arg("--out")
            .arg(&out)
            .output()
            .is_ok_and(|o| o.status.success());
        let converted = if ok { std::fs::read(&out).ok() } else { None };
        let _ = std::fs::remove_file(&out);
        if let Some(b) = converted {
            return Ok(("image/jpeg", b));
        }
    }
    let _ = path;
    match direct {
        Some(mime) => Ok((mime, bytes)),
        None => Err(AppError::msg(format!("{name}: save it as JPEG or PNG first"))),
    }
}

/// Reads a file into text (or an image), capped and cleaned.
pub fn ingest(path: &Path) -> AppResult<Ingested> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let meta = std::fs::metadata(path).map_err(|_| AppError::msg(format!("{name} can't be opened")))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(AppError::msg(format!("{name} is larger than 25 MB")));
    }
    let kind = kind_of(path).or_else(|| looks_like_text(path).then_some(FileKind::Text)).ok_or_else(|| {
        AppError::msg(format!("{name}: BYTE can read PDF, Word, PowerPoint, Excel, text, code and photos, but not this kind of file"))
    })?;
    let bytes = std::fs::read(path)?;
    let mut ocr = false;
    let (text, pages, image) = match kind {
        FileKind::Pdf => match pdf_text(&bytes) {
            Ok((t, p)) if has_text_layer(&t, p) => (t, Some(p), None),
            // No text layer (a scan): read the rendered pages.
            other => match scanned_pdf_text(&bytes) {
                Some((t, p)) => {
                    ocr = true;
                    (t, Some(p), None)
                }
                None => match other {
                    Ok((t, p)) if t.lines().any(|l| !l.starts_with("[Page ") && l.chars().any(char::is_alphanumeric)) => (t, Some(p), None),
                    Ok(_) => {
                        let hint = if cfg!(target_os = "macos") { "" } else { " (it may be a scan: reading scans needs macOS)" };
                        return Err(AppError::msg(format!("{name} has no readable text{hint}")));
                    }
                    Err(e) => return Err(AppError::msg(format!("{name}: {e}"))),
                },
            },
        },
        FileKind::Word => (office_text(&bytes, OfficeKind::Word)?.0, None, None),
        FileKind::Slides => {
            let (t, n) = office_text(&bytes, OfficeKind::Slides)?;
            (t, Some(n), None)
        }
        FileKind::Sheet => {
            let (t, n) = office_text(&bytes, OfficeKind::Sheet)?;
            (t, Some(n), None)
        }
        FileKind::Web => (crate::tools::fetch::extract(&String::from_utf8_lossy(&bytes), "file://local").text, None, None),
        FileKind::Text => (String::from_utf8_lossy(&bytes).into_owned(), None, None),
        FileKind::Audio => return Err(AppError::msg(format!("{name}: recordings are read with voice input (voice.rs)"))),
        FileKind::Image => {
            if meta.len() > MAX_IMAGE_BYTES {
                return Err(AppError::msg(format!("{name} is too large for a photo (8 MB max)")));
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            // Words in the photo (a receipt, a screenshot, a page) help every model, not only ones that see.
            let words = crate::ocr::image_text(&bytes).unwrap_or_default();
            ocr = !words.trim().is_empty();
            let (mime, bytes) = photo_for_model(path, &ext, bytes, &name)?;
            (words, None, Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes))))
        }
    };
    let text = tidy(&text);
    if kind != FileKind::Image && text.trim().is_empty() {
        let hint = if kind == FileKind::Pdf && !cfg!(any(target_os = "macos", windows)) { " (it may be a scan: reading scans isn't available on this system yet)" } else { "" };
        return Err(AppError::msg(format!("{name} has no readable text{hint}")));
    }
    let truncated = text.chars().count() > MAX_TEXT_CHARS;
    let text = if truncated { text.chars().take(MAX_TEXT_CHARS).collect() } else { text };
    Ok(Ingested { name, kind, pages, text, truncated, image, ocr })
}

/// No known extension: treat it as text if the first bytes are valid UTF-8 without NULs.
fn looks_like_text(path: &Path) -> bool {
    let mut buf = [0u8; 4096];
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let n = f.read(&mut buf).unwrap_or(0);
    n > 0 && !buf[..n].contains(&0) && std::str::from_utf8(&buf[..n]).is_ok()
}

/// Trims each line and drops runs of blank lines.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let l = line.trim_end();
        if l.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(l);
        out.push('\n');
    }
    out.trim().to_string()
}

/// A PDF with real text has at least ~20 letters or digits per page; scans have
/// none (or only a stray page number).
fn has_text_layer(text: &str, pages: u32) -> bool {
    let chars = text.lines().filter(|l| !l.starts_with("[Page ")).flat_map(|l| l.chars()).filter(|c| c.is_alphanumeric()).count();
    chars as u32 >= 20 * pages.max(1)
}

/// Text of a scanned PDF (Apple Vision), with `[Page N]` markers; None if
/// nothing could be read.
fn scanned_pdf_text(bytes: &[u8]) -> Option<(String, u32)> {
    let (texts, total) = crate::ocr::pdf_text(bytes).ok()?;
    if texts.iter().all(|t| t.trim().is_empty()) {
        return None;
    }
    let mut text = texts.iter().enumerate().map(|(i, p)| format!("[Page {}]\n{}", i + 1, p.trim())).collect::<Vec<_>>().join("\n\n");
    if total > texts.len() {
        text.push_str(&format!("\n\n[Only the first {} of {total} scanned pages were read.]", texts.len()));
    }
    Some((text, total as u32))
}

fn pdf_text(bytes: &[u8]) -> Result<(String, u32), String> {
    let pages = pdf_extract::extract_text_from_mem_by_pages(bytes).map_err(|e| format!("couldn't read this PDF ({e})"))?;
    let n = pages.len() as u32;
    let text = pages
        .iter()
        .enumerate()
        .map(|(i, p)| format!("[Page {}]\n{}", i + 1, p.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok((text, n))
}

#[derive(Clone, Copy)]
enum OfficeKind {
    Word,
    Slides,
    Sheet,
}

/// Word, PowerPoint and Excel files are zip archives of XML: read the text runs.
fn office_text(bytes: &[u8], kind: OfficeKind) -> AppResult<(String, u32)> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| AppError::msg("this file looks damaged (not a valid Office file)"))?;
    let all_names: Vec<String> = zip.file_names().map(str::to_string).collect();
    let mut read = |name: &str| -> Option<String> {
        let mut f = zip.by_name(name).ok()?;
        let mut s = String::new();
        f.read_to_string(&mut s).ok()?;
        Some(s)
    };
    match kind {
        OfficeKind::Word => {
            let xml = read("word/document.xml").or_else(|| read("content.xml")).ok_or_else(|| AppError::msg("no document text found"))?;
            Ok((xml_text(&xml, &["w:p", "text:p", "text:h"], &["w:t", "text:span", "text:p", "text:h"], &["w:tab"]), 1))
        }
        OfficeKind::Slides => {
            let names: Vec<String> = {
                let mut n: Vec<String> = all_names.iter().filter(|f| f.starts_with("ppt/slides/slide") && f.ends_with(".xml")).cloned().collect();
                n.sort_by_key(|f| f.trim_start_matches("ppt/slides/slide").trim_end_matches(".xml").parse::<u32>().unwrap_or(0));
                n
            };
            let mut out = Vec::new();
            for (i, n) in names.iter().enumerate() {
                if let Some(xml) = read(n) {
                    out.push(format!("[Slide {}]\n{}", i + 1, xml_text(&xml, &["a:p"], &["a:t"], &[])));
                }
            }
            if out.is_empty() {
                if let Some(xml) = read("content.xml") {
                    out.push(xml_text(&xml, &["text:p"], &["text:span", "text:p"], &[]));
                }
            }
            let n = names.len().max(out.len()) as u32;
            Ok((out.join("\n\n"), n))
        }
        OfficeKind::Sheet => {
            let shared: Vec<String> = read("xl/sharedStrings.xml").map(|x| shared_strings(&x)).unwrap_or_default();
            let mut names: Vec<String> = all_names.iter().filter(|f| f.starts_with("xl/worksheets/sheet") && f.ends_with(".xml")).cloned().collect();
            names.sort_by_key(|f| f.trim_start_matches("xl/worksheets/sheet").trim_end_matches(".xml").parse::<u32>().unwrap_or(0));
            let mut out = Vec::new();
            for (i, n) in names.iter().enumerate() {
                if let Some(xml) = read(n) {
                    out.push(format!("[Sheet {}]\n{}", i + 1, sheet_rows(&xml, &shared)));
                }
            }
            Ok((out.join("\n\n"), names.len() as u32))
        }
    }
}

/// Text inside `text_tags`, a line break after each `para_tags` element, a tab for `tab_tags`.
fn xml_text(xml: &str, para_tags: &[&str], text_tags: &[&str], tab_tags: &[&str]) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = String::new();
    let mut depth_in_text = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if text_tags.contains(&name.as_str()) {
                    depth_in_text += 1;
                }
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if tab_tags.contains(&name.as_str()) {
                    out.push('\t');
                } else if name == "w:br" || name == "a:br" || name == "text:line-break" {
                    out.push('\n');
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if text_tags.contains(&name.as_str()) {
                    depth_in_text = depth_in_text.saturating_sub(1);
                }
                if para_tags.contains(&name.as_str()) && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Ok(Event::Text(t)) if depth_in_text > 0 => {
                if let Ok(s) = t.unescape() {
                    out.push_str(&s);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

fn shared_strings(xml: &str) -> Vec<String> {
    // Each <si> is one string; its <t> runs are joined.
    xml.split("<si>").skip(1).map(|si| xml_text(&format!("<si>{si}"), &[], &["t"], &[]).trim().to_string()).collect()
}

/// One line per row, cells separated by " | " (shared strings resolved).
fn sheet_rows(xml: &str, shared: &[String]) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut rows: Vec<String> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut shared_cell = false;
    let mut in_value = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"c" => {
                    shared_cell = e.attributes().flatten().any(|a| a.key.as_ref() == b"t" && a.value.as_ref() == b"s");
                }
                b"v" | b"t" => in_value = true,
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"v" | b"t" => in_value = false,
                b"row" => {
                    if cells.iter().any(|c| !c.is_empty()) {
                        rows.push(cells.join(" | "));
                    }
                    cells.clear();
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_value => {
                let v = t.unescape().map(|s| s.into_owned()).unwrap_or_default();
                let v = if shared_cell { v.trim().parse::<usize>().ok().and_then(|i| shared.get(i).cloned()).unwrap_or(v) } else { v };
                cells.push(v);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    rows.join("\n")
}

/// What the model sees for attached files, fitted to `budget` characters:
/// short files whole, long ones reduced to the passages that best match the
/// question.
pub fn for_model(files: &[Ingested], question: &str, budget: usize) -> String {
    let texts: Vec<&Ingested> = files.iter().filter(|f| !f.text.is_empty()).collect();
    if texts.is_empty() {
        return String::new();
    }
    let per = (budget / texts.len()).max(1500);
    let mut out = String::new();
    for f in texts {
        let body = crate::tools::fetch::relevant_passages(&f.text, question, per);
        let cut = body.len() < f.text.len();
        let what = match f.pages {
            Some(n) if f.kind == FileKind::Pdf => format!(", {n} pages"),
            Some(n) if f.kind == FileKind::Slides => format!(", {n} slides"),
            Some(n) if f.kind == FileKind::Sheet => format!(", {n} sheets"),
            _ => String::new(),
        };
        let note = if cut { " — only the parts most relevant to the question are shown" } else { "" };
        out.push_str(&format!("\n\n<file name=\"{}\"{what}{note}>\n{body}\n</file>", f.name.replace('"', "'")));
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn photos_become_data_urls_the_engine_can_read() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("dot.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\nfake").unwrap();
        let got = ingest(&png).unwrap();
        assert_eq!(got.kind, FileKind::Image);
        assert!(got.image.as_deref().unwrap().starts_with("data:image/png;base64,"));
        assert!(got.text.is_empty());
        // WebP/HEIC need converting (macOS does it with sips; elsewhere: a clear message).
        #[cfg(not(target_os = "macos"))]
        {
            let webp = dir.path().join("x.webp");
            std::fs::write(&webp, b"RIFF....WEBP").unwrap();
            assert!(ingest(&webp).unwrap_err().to_string().contains("JPEG or PNG"));
        }
    }

    fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (name, body) in files {
                z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn reads_word_slides_and_sheets() {
        let dir = tempfile::tempdir().unwrap();
        let docx = zip_with(&[(
            "word/document.xml",
            r#"<w:document><w:body><w:p><w:r><w:t>Kitchen budget</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">Cabinets: </w:t></w:r><w:r><w:t>$8,000 &amp; up</w:t></w:r></w:p></w:body></w:document>"#,
        )]);
        let f = ingest(&write(dir.path(), "plan.docx", &docx)).unwrap();
        assert_eq!(f.kind, FileKind::Word);
        assert_eq!(f.text, "Kitchen budget\nCabinets: $8,000 & up");

        let pptx = zip_with(&[
            ("ppt/slides/slide2.xml", r#"<p:sld><a:p><a:r><a:t>Second</a:t></a:r></a:p></p:sld>"#),
            ("ppt/slides/slide1.xml", r#"<p:sld><a:p><a:r><a:t>Title</a:t></a:r></a:p><a:p><a:r><a:t>Point one</a:t></a:r></a:p></p:sld>"#),
        ]);
        let f = ingest(&write(dir.path(), "deck.pptx", &pptx)).unwrap();
        assert_eq!(f.pages, Some(2));
        assert_eq!(f.text, "[Slide 1]\nTitle\nPoint one\n\n[Slide 2]\nSecond");

        let xlsx = zip_with(&[
            ("xl/sharedStrings.xml", r#"<sst><si><t>Item</t></si><si><t>Cost</t></si><si><t>Paint</t></si></sst>"#),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row><c t="s"><v>0</v></c><c t="s"><v>1</v></c></row><row><c t="s"><v>2</v></c><c><v>120.5</v></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let f = ingest(&write(dir.path(), "costs.xlsx", &xlsx)).unwrap();
        assert_eq!(f.text, "[Sheet 1]\nItem | Cost\nPaint | 120.5");
    }

    /// A small PDF with one line of Helvetica text per page (also used by `ocr` tests).
    pub(crate) fn test_pdf(lines: &[&str]) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let mut kids = vec![];
        for &text in lines {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![72.into(), 700.into()]),
                    Operation::new("Tj", vec![Object::string_literal(text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id });
            kids.push(page_id.into());
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => lines.len() as i64, "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()] }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn reads_pdfs_page_by_page() {
        let bytes = test_pdf(&["Tides come from the Moon.", "Spring tides happen twice a month."]);
        let dir = tempfile::tempdir().unwrap();
        let f = ingest(&write(dir.path(), "tides.pdf", &bytes)).unwrap();
        assert_eq!(f.pages, Some(2));
        assert!(f.text.contains("[Page 1]") && f.text.contains("Moon") && f.text.contains("[Page 2]") && f.text.contains("twice a month"), "{}", f.text);
    }

    #[test]
    fn text_code_and_photos() {
        let dir = tempfile::tempdir().unwrap();
        let f = ingest(&write(dir.path(), "notes.md", b"# Trip\n\n\n\nPack light.   \n")).unwrap();
        assert_eq!((f.kind, f.text.as_str()), (FileKind::Text, "# Trip\n\nPack light."));
        // No extension, but plain text.
        assert_eq!(ingest(&write(dir.path(), "README", b"hello there")).unwrap().kind, FileKind::Text);
        let png = ingest(&write(dir.path(), "shot.png", &[0x89, b'P', b'N', b'G', 0, 1, 2])).unwrap();
        assert_eq!(png.kind, FileKind::Image);
        assert!(png.image.as_deref().unwrap().starts_with("data:image/png;base64,"));
        // Binary with an unknown extension is refused with a clear message.
        let err = ingest(&write(dir.path(), "blob.bin", &[0, 159, 146, 150])).unwrap_err().to_string();
        assert!(err.contains("can read PDF"), "{err}");
        let err = ingest(&write(dir.path(), "empty.txt", b"   \n")).unwrap_err().to_string();
        assert!(err.contains("no readable text"), "{err}");
    }

    #[test]
    fn long_files_are_cut_to_the_relevant_parts() {
        let mut text = String::new();
        for i in 0..400 {
            text.push_str(&format!("Paragraph {i} about gardening and soil.\n"));
        }
        text.push_str("The warranty lasts five years from purchase.\n");
        let f = Ingested { name: "manual.pdf".into(), kind: FileKind::Pdf, pages: Some(40), text, truncated: false, image: None, ocr: false };
        let out = for_model(&[f], "How long is the warranty?", 3000);
        assert!(out.contains("warranty lasts five years"));
        assert!(out.contains("<file name=\"manual.pdf\", 40 pages — only the parts most relevant"));
        assert!(out.len() < 3600, "{}", out.len());
        // Short files go in whole; photos add only the words read in them.
        let short = Ingested { name: "a.txt".into(), kind: FileKind::Text, pages: None, text: "Hi".into(), truncated: false, image: None, ocr: false };
        let photo = Ingested { name: "p.png".into(), kind: FileKind::Image, pages: None, text: String::new(), truncated: false, image: Some("data:".into()), ocr: false };
        assert_eq!(for_model(&[short.clone(), photo], "q", 3000), "\n\n<file name=\"a.txt\">\nHi\n</file>");
        let receipt = Ingested { name: "r.png".into(), kind: FileKind::Image, pages: None, text: "TOTAL 12.40".into(), truncated: false, image: Some("data:".into()), ocr: true };
        assert!(for_model(&[receipt], "q", 3000).contains("TOTAL 12.40"));
    }

    #[test]
    fn scans_are_told_apart_from_pdfs_with_text() {
        assert!(has_text_layer("[Page 1]\nTides come from the Moon and the Sun.", 1));
        assert!(!has_text_layer("[Page 1]\n\n\n[Page 2]\n3", 2));
        // A scan without macOS: a clear message, not garbage.
        #[cfg(not(target_os = "macos"))]
        {
            let dir = tempfile::tempdir().unwrap();
            let blank = test_pdf(&[""]);
            let err = ingest(&write(dir.path(), "scan.pdf", &blank)).unwrap_err().to_string();
            assert!(err.contains("reading scans needs macOS"), "{err}");
        }
    }
}
