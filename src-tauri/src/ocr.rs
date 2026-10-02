//! Reads text in photos and scanned PDFs with Apple's Vision framework (runs
//! on the Neural Engine, on this Mac). Other systems get a clear error.

use crate::error::{AppError, AppResult};

/// Pages of a scanned PDF read at most (about 1–2 s per page).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAX_SCAN_PAGES: usize = 60;

/// Text lines in a photo (any format macOS reads: JPEG, PNG, HEIC, WebP, TIFF…).
pub fn image_text(bytes: &[u8]) -> AppResult<String> {
    #[cfg(target_os = "macos")]
    return mac::image_text(bytes).map_err(AppError::msg);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bytes;
        Err(AppError::msg("reading text in photos needs macOS"))
    }
}

/// Renders a PDF's pages and reads each one (for scans without a text layer).
/// Returns the text of up to `MAX_SCAN_PAGES` pages and the total page count.
pub fn pdf_text(bytes: &[u8]) -> AppResult<(Vec<String>, usize)> {
    #[cfg(target_os = "macos")]
    return mac::pdf_text(bytes, MAX_SCAN_PAGES).map_err(AppError::msg);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bytes;
        Err(AppError::msg("reading scanned pages needs macOS"))
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

#[cfg(test)]
mod tests {
    /// Real Vision + PDFKit (macOS only; run by the Mac engine workflow):
    /// a PDF page is rendered to an image and its words read back.
    #[test]
    #[cfg_attr(not(target_os = "macos"), ignore)]
    fn e2e_reads_text_from_a_rendered_page() {
        let pdf = crate::files::tests::test_pdf(&["Invoice number 48213 for BYTE"]);
        let (pages, total) = super::pdf_text(&pdf).unwrap();
        assert_eq!(total, 1);
        assert!(pages[0].contains("48213") && pages[0].to_lowercase().contains("invoice"), "{pages:?}");
    }
}
