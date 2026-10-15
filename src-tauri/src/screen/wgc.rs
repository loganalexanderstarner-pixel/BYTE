//! Windows screen capture: Windows.Graphics.Capture, the API OBS and the Game Bar use. Nothing is injected into a game and no
//! hook is installed; Windows itself hands over the window's picture (and draws its yellow capture border while it runs, which
//! is a free "BYTE is looking" indicator). If that API is missing (very old Windows) a GDI copy of the window's area is used.
//!
//! Real capture needs an interactive desktop, so its tests are `#[ignore]`d and run by hand on a PC.

#![cfg(windows)]

use windows::core::{Interface, HSTRING};
use windows::Foundation::PropertyType;
use windows::Foundation::PropertyValue;
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapPixelFormat, BitmapPropertySet, BitmapTransform, BitmapTypedValue, ColorManagementMode, ExifOrientationMode};
use windows::Graphics::SizeInt32;
use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
use windows::Win32::Foundation::{CloseHandle, HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

use super::capture::{CaptureError, FrameSource, Grabbed, Target};
use super::frame::Frame;
use super::policy::{self, Candidate, WindowInfo};

fn fail<E: std::fmt::Display>(what: &str, e: E) -> CaptureError {
    CaptureError::Failed(format!("{what}: {e}"))
}

/// Set up WinRT on this thread once, and undo it (paired) when the thread ends. A thread that already has COM in another mode
/// (Tauri's own) gets an error here, which is fine and needs no undo.
struct WinRtGuard(bool);

impl Drop for WinRtGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { RoUninitialize() };
        }
    }
}

thread_local! {
    static WINRT: WinRtGuard = WinRtGuard(unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok());
}

fn ensure_winrt() {
    WINRT.with(|_| {});
}

fn program_of(window: HWND) -> Option<String> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    let _ = unsafe { CloseHandle(process) };
    if !ok {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned())
}

fn describe(w: HWND) -> WindowInfo {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(w, &mut buf) }.max(0) as usize;
    WindowInfo { program: program_of(w).unwrap_or_default(), title: String::from_utf16_lossy(&buf[..n.min(buf.len())]) }
}

/// The window in front, exactly as Windows reports it (it may be BYTE's own).
pub fn front() -> Option<(HWND, WindowInfo)> {
    let w = unsafe { GetForegroundWindow() };
    if w.0.is_null() {
        return None;
    }
    Some((w, describe(w)))
}

/// Every top-level window from the top of the Z-order down, the foreground window first.
fn enumerate() -> Vec<(HWND, Candidate)> {
    unsafe extern "system" fn each(h: HWND, lparam: windows::Win32::Foundation::LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        out.push(h);
        BOOL(1)
    }
    let mut all: Vec<HWND> = Vec::new();
    let _ = unsafe { EnumWindows(Some(each), windows::Win32::Foundation::LPARAM(&mut all as *mut Vec<HWND> as isize)) };
    let fg = unsafe { GetForegroundWindow() };
    // The foreground window leads, then the rest in Z-order.
    if let Some(i) = all.iter().position(|h| *h == fg) {
        all.remove(i);
    }
    let mut ordered = vec![fg];
    ordered.extend(all);
    ordered
        .into_iter()
        .filter(|h| !h.0.is_null())
        .map(|h| {
            let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
            let mut cloaked = 0u32;
            let _ = unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4) };
            let mut class = [0u16; 64];
            let n = unsafe { GetClassNameW(h, &mut class) }.max(0) as usize;
            let class = String::from_utf16_lossy(&class[..n.min(64)]);
            let shell = matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow" | "NotifyIconOverflowWindow" | "XamlExplorerHostIslandWindow");
            let c = Candidate {
                id: h.0 as isize,
                info: describe(h),
                visible: unsafe { IsWindowVisible(h) }.as_bool(),
                minimized: unsafe { IsIconic(h) }.as_bool(),
                tool_window: ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0,
                cloaked: cloaked != 0,
                shell,
            };
            (h, c)
        })
        .collect()
}

/// The window BYTE should look at: the one in front, or, when BYTE's own window is in front, the first ordinary window below it.
pub fn target() -> Option<(HWND, WindowInfo)> {
    let list = enumerate();
    let cands: Vec<Candidate> = list.iter().map(|(_, c)| c.clone()).collect();
    let i = policy::pick_target(&cands)?;
    Some((list[i].0, list[i].1.info.clone()))
}

/// BYTE's own window is the foreground one.
pub fn own_in_front() -> bool {
    front().is_some_and(|(_, w)| w.program.eq_ignore_ascii_case("byte"))
}

/// The secure desktop (lock screen, UAC prompt) is showing: it cannot be opened from here.
pub fn screen_locked() -> bool {
    match unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) } {
        Ok(d) => {
            let _ = unsafe { CloseDesktop(d) };
            false
        }
        Err(_) => true,
    }
}

struct Dev {
    d3d: ID3D11Device,
    ctx: ID3D11DeviceContext,
    winrt: IDirect3DDevice,
}

fn make_device() -> Result<Dev, CaptureError> {
    let mut device: Option<ID3D11Device> = None;
    let mut ctx: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(None::<&windows::Win32::Graphics::Dxgi::IDXGIAdapter>, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut ctx))
    }
    .map_err(|e| fail("Direct3D", e))?;
    let (d3d, ctx) = (device.ok_or_else(|| fail("Direct3D", "no device"))?, ctx.ok_or_else(|| fail("Direct3D", "no context"))?);
    let dxgi: IDXGIDevice = d3d.cast().map_err(|e| fail("DXGI", e))?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(|e| fail("Direct3D", e))?;
    let winrt: IDirect3DDevice = inspectable.cast().map_err(|e| fail("Direct3D", e))?;
    Ok(Dev { d3d, ctx, winrt })
}

/// A running capture of one window: the frame pool keeps the newest picture, so asking costs one copy.
struct Active {
    hwnd: isize,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    size: SizeInt32,
    held: Option<Direct3D11CaptureFrame>,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.held = None;
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

pub struct Wgc {
    dev: Option<Dev>,
    active: Option<Active>,
    supported: Option<bool>,
}

impl Wgc {
    pub fn new() -> Wgc {
        Wgc { dev: None, active: None, supported: None }
    }

    fn is_supported(&mut self) -> bool {
        *self.supported.get_or_insert_with(|| {
            ensure_winrt();
            GraphicsCaptureSession::IsSupported().unwrap_or(false)
        })
    }

    fn start(&mut self, hwnd: HWND) -> Result<(), CaptureError> {
        ensure_winrt();
        if self.dev.is_none() {
            self.dev = Some(make_device()?);
        }
        let dev = self.dev.as_ref().expect("device just made");
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(|e| fail("capture", e))?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd) }.map_err(|e| fail("capture this window", e))?;
        let size = item.Size().map_err(|e| fail("capture", e))?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&dev.winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).map_err(|e| fail("capture", e))?;
        let session = pool.CreateCaptureSession(&item).map_err(|e| fail("capture", e))?;
        // Nobody wants the mouse pointer in a screenshot, and the border is optional on newer Windows (ignored where it is not).
        let _ = session.SetIsCursorCaptureEnabled(false);
        session.StartCapture().map_err(|e| fail("capture", e))?;
        self.active = Some(Active { hwnd: hwnd.0 as isize, pool, session, size, held: None, staging: None });
        Ok(())
    }

    fn read(&mut self) -> Result<Frame, CaptureError> {
        let dev = self.dev.as_ref().ok_or_else(|| fail("capture", "no device"))?;
        let a = self.active.as_mut().ok_or_else(|| fail("capture", "not started"))?;
        // Take the newest frame the pool has; keep the previous one if the window did not change.
        let mut waited = 0;
        loop {
            while let Ok(f) = a.pool.TryGetNextFrame() {
                a.held = Some(f);
            }
            if a.held.is_some() || waited >= 600 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            waited += 20;
        }
        let frame = a.held.as_ref().ok_or_else(|| fail("capture", "the window gave no picture"))?;
        let content = frame.ContentSize().map_err(|e| fail("capture", e))?;
        if content.Width != a.size.Width || content.Height != a.size.Height {
            a.size = content;
            let _ = a.pool.Recreate(&dev.winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, content);
        }
        let surface = frame.Surface().map_err(|e| fail("capture", e))?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| fail("capture", e))?;
        let tex: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(|e| fail("capture", e))?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        let reuse = matches!(&a.staging, Some((_, w, h)) if *w == desc.Width && *h == desc.Height);
        if !reuse {
            let mut d = desc;
            d.Usage = D3D11_USAGE_STAGING;
            d.BindFlags = 0;
            d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            d.MiscFlags = 0;
            let mut t: Option<ID3D11Texture2D> = None;
            unsafe { dev.d3d.CreateTexture2D(&d, None, Some(&mut t)) }.map_err(|e| fail("Direct3D", e))?;
            a.staging = Some((t.ok_or_else(|| fail("Direct3D", "no texture"))?, desc.Width, desc.Height));
        }
        let staging = &a.staging.as_ref().expect("staging just made").0;
        let (src, dst): (ID3D11Resource, ID3D11Resource) = (tex.cast().map_err(|e| fail("Direct3D", e))?, staging.cast().map_err(|e| fail("Direct3D", e))?);
        unsafe { dev.ctx.CopyResource(&dst, &src) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { dev.ctx.Map(&dst, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.map_err(|e| fail("Direct3D", e))?;
        let (w, h) = ((content.Width.max(1) as u32).min(desc.Width), (content.Height.max(1) as u32).min(desc.Height));
        let mut bgra = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h as usize {
            let row = unsafe { std::slice::from_raw_parts((mapped.pData as *const u8).add(y * mapped.RowPitch as usize), w as usize * 4) };
            bgra.extend_from_slice(row);
        }
        unsafe { dev.ctx.Unmap(&dst, 0) };
        for px in bgra.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Frame::new(w, h, bgra).ok_or_else(|| fail("capture", "empty picture"))
    }
}

impl Default for Wgc {
    fn default() -> Self {
        Wgc::new()
    }
}

impl FrameSource for Wgc {
    fn name(&self) -> &'static str {
        "Windows.Graphics.Capture"
    }

    fn front_window(&mut self) -> Option<WindowInfo> {
        target().map(|(_, w)| w)
    }

    fn front_is_own(&mut self) -> bool {
        own_in_front()
    }

    fn locked(&mut self) -> bool {
        screen_locked()
    }

    fn grab(&mut self, _: Target) -> Result<Grabbed, CaptureError> {
        // The window is read once, here, and the capture is bound to exactly that window.
        let (hwnd, window) = target().ok_or_else(|| fail("capture", "no other window to look at"))?;
        if unsafe { IsIconic(hwnd) }.as_bool() {
            return Err(fail("capture", "the window is minimised"));
        }
        if !self.is_supported() {
            return gdi_grab(hwnd).map(|frame| Grabbed { window, frame });
        }
        if self.active.as_ref().is_none_or(|a| a.hwnd != hwnd.0 as isize) {
            self.active = None;
            if let Err(e) = self.start(hwnd) {
                log::info!("screen: Windows.Graphics.Capture would not start ({e:?}); using the GDI copy");
                return gdi_grab(hwnd).map(|frame| Grabbed { window, frame });
            }
        }
        match self.read() {
            Ok(frame) => Ok(Grabbed { window, frame }),
            Err(e) => {
                // A window that was closed or changed kind: start over next time.
                self.active = None;
                Err(e)
            }
        }
    }

    fn release(&mut self) {
        // The pool's last frame stays on the GPU side while held; let it go whenever nothing is being kept.
        if let Some(a) = self.active.as_mut() {
            a.held = None;
        }
    }
}

/// The fallback: copies the window's rectangle from the screen (anything covering it is copied too).
fn gdi_grab(hwnd: HWND) -> Result<Frame, CaptureError> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.map_err(|e| fail("window size", e))?;
    let (w, h) = ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let copied = BitBlt(mem, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY);
        let mut info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
        let mut bgra = vec![0u8; w as usize * h as usize * 4];
        let lines = GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        copied.map_err(|e| fail("screen copy", e))?;
        if lines == 0 {
            return Err(fail("screen copy", "no lines"));
        }
        for px in bgra.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Frame::new(w as u32, h as u32, bgra).ok_or_else(|| fail("screen copy", "empty picture"))
    }
}

/// JPEG bytes of a frame (the Windows imaging encoder; `quality` 1 to 100).
pub fn encode_jpeg(f: &Frame, quality: u8) -> Result<Vec<u8>, String> {
    ensure_winrt();
    let m = |e: windows::core::Error| e.message();
    let stream = InMemoryRandomAccessStream::new().map_err(m)?;
    let props = BitmapPropertySet::new().map_err(m)?;
    let q = BitmapTypedValue::Create(&PropertyValue::CreateSingle(f32::from(quality) / 100.0).map_err(m)?, PropertyType::Single).map_err(m)?;
    props.Insert(&HSTRING::from("ImageQuality"), &q).map_err(m)?;
    let encoder = BitmapEncoder::CreateWithEncodingOptionsAsync(BitmapEncoder::JpegEncoderId().map_err(m)?, &stream, &props).map_err(m)?.join().map_err(m)?;
    encoder.SetPixelData(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Ignore, f.width, f.height, 96.0, 96.0, &f.bgra).map_err(m)?;
    encoder.FlushAsync().map_err(m)?.join().map_err(m)?;
    let size = stream.Size().map_err(m)? as u32;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(m)?).map_err(m)?;
    reader.LoadAsync(size).map_err(m)?.join().map_err(m)?;
    let mut out = vec![0u8; size as usize];
    reader.ReadBytes(&mut out).map_err(m)?;
    Ok(out)
}

/// The pixels of an encoded picture (what `encode_jpeg` made).
pub fn decode_image(bytes: &[u8]) -> Result<Frame, String> {
    ensure_winrt();
    let m = |e: windows::core::Error| e.message();
    let stream = InMemoryRandomAccessStream::new().map_err(m)?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(m)?;
    writer.WriteBytes(bytes).map_err(m)?;
    writer.StoreAsync().map_err(m)?.join().map_err(m)?;
    writer.FlushAsync().map_err(m)?.join().map_err(m)?;
    writer.DetachStream().map_err(m)?;
    stream.Seek(0).map_err(m)?;
    let decoder = BitmapDecoder::CreateAsync(&stream).map_err(m)?.join().map_err(m)?;
    let (w, h) = (decoder.PixelWidth().map_err(m)?, decoder.PixelHeight().map_err(m)?);
    let provider = decoder
        .GetPixelDataTransformedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Ignore, &BitmapTransform::new().map_err(m)?, ExifOrientationMode::IgnoreExifOrientation, ColorManagementMode::DoNotColorManage)
        .map_err(m)?
        .join()
        .map_err(m)?;
    let data = provider.DetachPixelData().map_err(m)?;
    Frame::new(w, h, data.to_vec()).ok_or_else(|| "the picture has an unexpected size".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jpeg_round_trips_to_about_the_same_picture() {
        let f = Frame::solid(64, 48, 200, 100, 50);
        let back = decode_image(&encode_jpeg(&f, 90).unwrap()).unwrap();
        assert_eq!((back.width, back.height), (64, 48));
        let p = back.pixel(10, 10);
        assert!((i32::from(p[0]) - 200).abs() < 12 && (i32::from(p[2]) - 50).abs() < 12, "{p:?}");
    }

    #[test]
    fn jpeg_encoding_makes_a_real_jpeg() {
        let mut f = Frame::solid(320, 200, 30, 120, 200);
        // Some detail so the file is not trivially tiny.
        for i in 0..2000usize {
            f.bgra[i * 7 % (320 * 200 * 4)] = (i % 251) as u8;
        }
        let j = encode_jpeg(&f, 70).unwrap();
        assert_eq!(&j[..3], &[0xFF, 0xD8, 0xFF], "a JPEG starts with FF D8 FF");
        assert!(j.len() > 500 && j.len() < 320 * 200 * 4);
    }

    /// Needs the interactive desktop of a signed-in PC (the test harness has none).
    #[test]
    #[ignore = "needs an interactive desktop: run by hand on a PC with a window open"]
    fn real_capture_of_the_window_in_front() {
        let mut w = Wgc::new();
        let f = w.grab(Target::Foreground).expect("capture").frame;
        assert!(f.width > 50 && f.height > 50);
        assert!(!f.is_blank());
    }

    #[test]
    #[ignore = "needs an interactive desktop"]
    fn real_gdi_fallback_of_the_window_in_front() {
        let (h, _) = front().expect("a window in front");
        let f = gdi_grab(h).expect("gdi");
        assert!(f.width > 50);
    }
}
