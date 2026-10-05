//! Reads text in photos and scanned PDFs: Apple's Vision framework on a Mac (runs
//! on the Neural Engine), Windows.Media.Ocr and Windows.Data.Pdf on Windows (both
//! built into the OS, so nothing extra to install or bundle). Other systems get a
//! clear error.

use crate::error::{AppError, AppResult};

/// Pages of a scanned PDF read at most (about 1–2 s per page).
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
const MAX_SCAN_PAGES: usize = 60;

/// Text lines in a photo (any format macOS reads: JPEG, PNG, HEIC, WebP, TIFF…).
pub fn image_text(bytes: &[u8]) -> AppResult<String> {
    #[cfg(target_os = "macos")]
    return mac::image_text(bytes).map_err(AppError::msg);
    #[cfg(windows)]
    return win::image_text(bytes).map_err(AppError::msg);
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = bytes;
        Err(AppError::msg("reading text in photos isn't available on this system yet"))
    }
}

/// Renders a PDF's pages and reads each one (for scans without a text layer).
/// Returns the text of up to `MAX_SCAN_PAGES` pages and the total page count.
pub fn pdf_text(bytes: &[u8]) -> AppResult<(Vec<String>, usize)> {
    #[cfg(target_os = "macos")]
    return mac::pdf_text(bytes, MAX_SCAN_PAGES).map_err(AppError::msg);
    #[cfg(windows)]
    return win::pdf_text(bytes, MAX_SCAN_PAGES).map_err(AppError::msg);
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = bytes;
        Err(AppError::msg("reading scanned pages isn't available on this system yet"))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSSize};
    use objc2_pdf_kit::{PDFDisplayBox, PDFDocument};
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

    pub fn image_text(bytes: &[u8]) -> Result<String, String> {
        autoreleasepool(|_| {
            let data = NSData::with_bytes(bytes);
            let options = NSDictionary::new();
            let handler = VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &options);
            let request = VNRecognizeTextRequest::new();
            request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
            request.setUsesLanguageCorrection(true);
            request.setAutomaticallyDetectsLanguage(true);
            let as_request: Retained<VNRequest> = Retained::into_super(Retained::into_super(request.clone()));
            let requests = NSArray::from_retained_slice(&[as_request]);
            handler.performRequests_error(&requests).map_err(|e| e.localizedDescription().to_string())?;
            let mut lines = Vec::new();
            for obs in request.results().iter().flat_map(|r| r.iter()) {
                if let Some(best) = obs.topCandidates(1).firstObject() {
                    lines.push(best.string().to_string());
                }
            }
            Ok(lines.join("\n"))
        })
    }

    pub fn pdf_text(bytes: &[u8], max_pages: usize) -> Result<(Vec<String>, usize), String> {
        autoreleasepool(|_| {
            let data = NSData::with_bytes(bytes);
            let doc = unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }.ok_or("not a readable PDF")?;
            let total = unsafe { doc.pageCount() };
            let mut pages = Vec::new();
            for i in 0..total.min(max_pages) {
                let Some(page) = (unsafe { doc.pageAtIndex(i) }) else { continue };
                let bounds = unsafe { page.boundsForBox(PDFDisplayBox::MediaBox) };
                // About 200 dpi for a letter page: sharp enough for small print.
                let scale = (2200.0 / bounds.size.width.max(bounds.size.height).max(1.0)).min(3.0);
                let size = NSSize::new(bounds.size.width * scale, bounds.size.height * scale);
                let image = unsafe { page.thumbnailOfSize_forBox(size, PDFDisplayBox::MediaBox) };
                let Some(tiff) = image.TIFFRepresentation() else { continue };
                pages.push(image_text(&tiff.to_vec())?);
            }
            Ok((pages, total))
        })
    }
}

#[cfg(windows)]
mod win {
    use windows::core::Error;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::{BitmapDecoder, BitmapTransform, ColorManagementMode, ExifOrientationMode};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, IRandomAccessStream, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    fn msg(e: Error) -> String {
        e.message()
    }

    /// WinRT needs COM on the calling thread, and worker threads do not have it
    /// unless something set it up -- without this every call fails with
    /// CO_E_NOTINITIALIZED. An error here means the thread is already initialised
    /// (Tauri's own threads are), which is exactly what is wanted.
    fn ensure_winrt() {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    }

    /// Windows keeps OCR languages as optional features. With none installed for
    /// the user's profile there is no engine at all, and the bare error is
    /// unhelpful, so say what to do about it.
    fn engine() -> Result<OcrEngine, String> {
        OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
            "Windows has no text-recognition language installed for your account. Add one in Settings \u{2192} Time & language \u{2192} Language & region (an \"OCR\" feature is included with each language), then try again."
                .to_string()
        })
    }

    fn stream_of(bytes: &[u8]) -> Result<InMemoryRandomAccessStream, String> {
        let stream = InMemoryRandomAccessStream::new().map_err(msg)?;
        let writer = DataWriter::CreateDataWriter(&stream).map_err(msg)?;
        writer.WriteBytes(bytes).map_err(msg)?;
        writer.StoreAsync().map_err(msg)?.get().map_err(msg)?;
        writer.FlushAsync().map_err(msg)?.get().map_err(msg)?;
        // Hand the stream back instead of closing it along with the writer.
        writer.DetachStream().map_err(msg)?;
        stream.Seek(0).map_err(msg)?;
        Ok(stream)
    }

    /// Lines of text in an encoded image held in `stream`.
    fn read_image(stream: &IRandomAccessStream, engine: &OcrEngine) -> Result<String, String> {
        let decoder = BitmapDecoder::CreateAsync(stream).map_err(msg)?.get().map_err(msg)?;
        let (w, h) = (decoder.PixelWidth().map_err(msg)?, decoder.PixelHeight().map_err(msg)?);
        // The engine refuses anything past its own limit (about 10,000 px), which
        // a photo from a modern phone can exceed. Scale down rather than fail.
        let limit = OcrEngine::MaxImageDimension().map_err(msg)?;
        let bitmap = if w.max(h) > limit {
            let scale = limit as f64 / w.max(h) as f64;
            let transform = BitmapTransform::new().map_err(msg)?;
            transform.SetScaledWidth(((w as f64 * scale) as u32).max(1)).map_err(msg)?;
            transform.SetScaledHeight(((h as f64 * scale) as u32).max(1)).map_err(msg)?;
            decoder
                .GetSoftwareBitmapTransformedAsync(
                    decoder.BitmapPixelFormat().map_err(msg)?,
                    decoder.BitmapAlphaMode().map_err(msg)?,
                    &transform,
                    ExifOrientationMode::RespectExifOrientation,
                    ColorManagementMode::DoNotColorManage,
                )
                .map_err(msg)?
                .get()
                .map_err(msg)?
        } else {
            decoder.GetSoftwareBitmapAsync().map_err(msg)?.get().map_err(msg)?
        };
        let result = engine.RecognizeAsync(&bitmap).map_err(msg)?.get().map_err(msg)?;
        let mut lines = Vec::new();
        for line in result.Lines().map_err(msg)? {
            lines.push(line.Text().map_err(msg)?.to_string());
        }
        Ok(lines.join("\n"))
    }

    pub fn image_text(bytes: &[u8]) -> Result<String, String> {
        ensure_winrt();
        let engine = engine()?;
        let stream = stream_of(bytes)?;
        read_image(&stream.cast().map_err(msg)?, &engine)
    }

    pub fn pdf_text(bytes: &[u8], max_pages: usize) -> Result<(Vec<String>, usize), String> {
        ensure_winrt();
        let engine = engine()?;
        let source = stream_of(bytes)?;
        let doc = PdfDocument::LoadFromStreamAsync(&source)
            .map_err(msg)?
            .get()
            .map_err(|_| "not a readable PDF (it may be password protected)".to_string())?;
        let total = doc.PageCount().map_err(msg)? as usize;
        let mut pages = Vec::new();
        for i in 0..total.min(max_pages) {
            let page = doc.GetPage(i as u32).map_err(msg)?;
            let size = page.Size().map_err(msg)?;
            // About 200 dpi for a letter page: sharp enough for small print.
            let scale = (2200.0 / size.Width.max(size.Height).max(1.0)).min(3.0);
            let opts = PdfPageRenderOptions::new().map_err(msg)?;
            opts.SetDestinationWidth((size.Width * scale) as u32).map_err(msg)?;
            opts.SetDestinationHeight((size.Height * scale) as u32).map_err(msg)?;
            let rendered = InMemoryRandomAccessStream::new().map_err(msg)?;
            page.RenderWithOptionsToStreamAsync(&rendered, &opts).map_err(msg)?.get().map_err(msg)?;
            rendered.Seek(0).map_err(msg)?;
            pages.push(read_image(&rendered.cast().map_err(msg)?, &engine)?);
        }
        Ok((pages, total))
    }
}

#[cfg(test)]
mod tests {
    /// Real OCR end to end: a PDF page is rendered to an image and its words read
    /// back. Vision + PDFKit on a Mac (run by the Mac engine workflow),
    /// Windows.Media.Ocr + Windows.Data.Pdf on Windows. The Windows run needs an
    /// OCR language installed for the account, which a client Windows has and a
    /// CI Server image may not, so a failure there is about the environment first.
    #[test]
    #[cfg_attr(not(any(target_os = "macos", windows)), ignore)]
    fn e2e_reads_text_from_a_rendered_page() {
        let pdf = crate::files::tests::test_pdf(&["Invoice number 48213 for BYTE"]);
        let (pages, total) = super::pdf_text(&pdf).unwrap();
        assert_eq!(total, 1);
        assert!(pages[0].contains("48213") && pages[0].to_lowercase().contains("invoice"), "{pages:?}");
    }
}
