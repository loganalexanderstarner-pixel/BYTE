//! Saving the agent's page on macOS with WKWebView: a PDF of the whole page
//! (`createPDFWithConfiguration`), a picture (`takeSnapshotWithConfiguration`
//! → PNG) or a Safari web archive (`createWebArchiveData`). Runs on the main
//! thread (called from `with_webview`); the result goes back over `tx`.

use std::ffi::c_void;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSData, NSDictionary, NSError};
use objc2_web_kit::WKWebView;
use tokio::sync::oneshot;

use super::Capture;
use crate::error::{AppError, AppResult};

type Reply = oneshot::Sender<AppResult<Vec<u8>>>;

fn error_text(err: *mut NSError, what: &str) -> AppError {
    let detail = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_default();
    AppError::msg(format!("couldn't save the page as {what}: {detail}"))
}

fn data_bytes(data: *mut NSData) -> Option<Vec<u8>> {
    unsafe { data.as_ref() }.map(|d| d.to_vec())
}

fn png_of(image: &NSImage) -> Option<Vec<u8>> {
    let tiff: Retained<NSData> = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let props = NSDictionary::new();
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) }?;
    Some(png.to_vec())
}

/// `webview` is the `WKWebView*` from Tauri's `PlatformWebview::inner()`.
pub fn capture(webview: *mut c_void, kind: Capture, tx: Reply) {
    let Some(wv) = (unsafe { (webview as *mut WKWebView).as_ref() }) else {
        let _ = tx.send(Err(AppError::msg("the browser isn't available")));
        return;
    };
    // The completion blocks are `Fn`, so the one-shot sender sits in a Mutex<Option>.
    let tx = std::sync::Arc::new(Mutex::new(Some(tx)));
    let finish = move |r: AppResult<Vec<u8>>| {
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(r);
        }
    };
    unsafe {
        match kind {
            Capture::Pdf => {
                let block = RcBlock::new(move |data: *mut NSData, err: *mut NSError| {
                    finish(data_bytes(data).ok_or_else(|| error_text(err, "a PDF")));
                });
                wv.createPDFWithConfiguration_completionHandler(None, &block);
            }
            Capture::Archive => {
                let block = RcBlock::new(move |data: *mut NSData, err: *mut NSError| {
                    finish(data_bytes(data).ok_or_else(|| error_text(err, "a web archive")));
                });
                wv.createWebArchiveDataWithCompletionHandler(&block);
            }
            Capture::Png => {
                let block = RcBlock::new(move |image: *mut NSImage, err: *mut NSError| {
                    let png = image.as_ref().and_then(png_of);
                    finish(png.ok_or_else(|| error_text(err, "a picture")));
                });
                wv.takeSnapshotWithConfiguration_completionHandler(None, &block);
            }
        }
    }
}
