//! A picture of the screen as raw pixels, and the small pure operations on it: shrinking (before anything is encoded or
//! stored), tiling several frames into one sheet, and a plain BMP encoder (used where no better encoder exists, and in tests).

/// Blue, green, red, alpha, row after row, no padding.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, bgra: Vec<u8>) -> Option<Frame> {
        (width > 0 && height > 0 && bgra.len() == width as usize * height as usize * 4).then_some(Frame { width, height, bgra })
    }

    /// A single colour, for tests and blank tiles.
    pub fn solid(width: u32, height: u32, b: u8, g: u8, r: u8) -> Frame {
        let mut bgra = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width as usize * height as usize {
            bgra.extend_from_slice(&[b, g, r, 255]);
        }
        Frame { width, height, bgra }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [self.bgra[i], self.bgra[i + 1], self.bgra[i + 2], self.bgra[i + 3]]
    }

    /// True when every pixel is (almost) black: a protected or failed capture comes back like that.
    pub fn is_blank(&self) -> bool {
        // Sample a grid rather than every pixel.
        let step = ((self.width.max(self.height) / 64).max(1)) as usize;
        let mut y = 0usize;
        while y < self.height as usize {
            let mut x = 0usize;
            while x < self.width as usize {
                let i = (y * self.width as usize + x) * 4;
                if self.bgra[i] > 8 || self.bgra[i + 1] > 8 || self.bgra[i + 2] > 8 {
                    return false;
                }
                x += step;
            }
            y += step;
        }
        true
    }
}

/// The size after scaling so the longest side is at most `max_side` (never enlarges).
pub fn fit(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= max_side || max_side == 0 {
        return (width, height);
    }
    let s = f64::from(max_side) / f64::from(long);
    (((f64::from(width) * s).round() as u32).max(1), ((f64::from(height) * s).round() as u32).max(1))
}

/// Shrinks so the longest side is `max_side` at most, averaging the covered source pixels (text stays readable).
pub fn downscale(src: &Frame, max_side: u32) -> Frame {
    let (w, h) = fit(src.width, src.height, max_side);
    if (w, h) == (src.width, src.height) {
        return src.clone();
    }
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for dy in 0..h as u64 {
        let y0 = (dy * u64::from(src.height) / u64::from(h)) as usize;
        let y1 = (((dy + 1) * u64::from(src.height)).div_ceil(u64::from(h)) as usize).clamp(y0 + 1, src.height as usize);
        for dx in 0..w as u64 {
            let x0 = (dx * u64::from(src.width) / u64::from(w)) as usize;
            let x1 = (((dx + 1) * u64::from(src.width)).div_ceil(u64::from(w)) as usize).clamp(x0 + 1, src.width as usize);
            let mut acc = [0u32; 3];
            for y in y0..y1 {
                let row = y * src.width as usize * 4;
                for x in x0..x1 {
                    let i = row + x * 4;
                    acc[0] += u32::from(src.bgra[i]);
                    acc[1] += u32::from(src.bgra[i + 1]);
                    acc[2] += u32::from(src.bgra[i + 2]);
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            out.extend_from_slice(&[(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, 255]);
        }
    }
    Frame { width: w, height: h, bgra: out }
}

/// Frames laid out left to right, top to bottom in `cols` columns, each shrunk to fit a `tile_w` x `tile_h` cell, with a
/// 4 px dark gap. Used to send several moments in one picture.
pub fn contact_sheet(frames: &[Frame], cols: usize, tile_w: u32, tile_h: u32) -> Option<Frame> {
    if frames.is_empty() || cols == 0 || tile_w == 0 || tile_h == 0 {
        return None;
    }
    const GAP: u32 = 4;
    let cols = cols.min(frames.len());
    let rows = frames.len().div_ceil(cols);
    let sheet_w = cols as u32 * tile_w + (cols as u32 + 1) * GAP;
    let sheet_h = rows as u32 * tile_h + (rows as u32 + 1) * GAP;
    let mut sheet = Frame::solid(sheet_w, sheet_h, 20, 20, 20);
    for (i, f) in frames.iter().enumerate() {
        let (c, r) = ((i % cols) as u32, (i / cols) as u32);
        let side = tile_w.max(tile_h);
        let mut t = downscale(f, side);
        // Fit inside the cell (the longest-side rule alone may overflow one dimension).
        if t.width > tile_w || t.height > tile_h {
            let k = (f64::from(tile_w) / f64::from(t.width)).min(f64::from(tile_h) / f64::from(t.height));
            t = downscale(&t, ((f64::from(t.width.max(t.height)) * k).floor() as u32).max(1));
        }
        let ox = GAP + c * (tile_w + GAP) + (tile_w - t.width.min(tile_w)) / 2;
        let oy = GAP + r * (tile_h + GAP) + (tile_h - t.height.min(tile_h)) / 2;
        for y in 0..t.height.min(tile_h) {
            let s = (y as usize * t.width as usize) * 4;
            let d = (((oy + y) as usize) * sheet_w as usize + ox as usize) * 4;
            let n = t.width.min(tile_w) as usize * 4;
            sheet.bgra[d..d + n].copy_from_slice(&t.bgra[s..s + n]);
        }
    }
    Some(sheet)
}

/// An uncompressed 24-bit BMP.
pub fn bmp(f: &Frame) -> Vec<u8> {
    let row = (f.width as usize * 3).div_ceil(4) * 4;
    let size = 54 + row * f.height as usize;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(f.width as i32).to_le_bytes());
    out.extend_from_slice(&(f.height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..f.height as usize).rev() {
        let start = out.len();
        for x in 0..f.width as usize {
            let i = (y * f.width as usize + x) * 4;
            out.extend_from_slice(&f.bgra[i..i + 3]);
        }
        out.resize(start + row, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_the_shape_and_never_enlarges() {
        assert_eq!(fit(1920, 1080, 1280), (1280, 720));
        assert_eq!(fit(1080, 1920, 1280), (720, 1280));
        assert_eq!(fit(800, 600, 1280), (800, 600));
        assert_eq!(fit(3840, 2160, 960), (960, 540));
    }

    #[test]
    fn downscale_averages_and_keeps_a_flat_colour() {
        let f = Frame::solid(400, 200, 10, 20, 30);
        let d = downscale(&f, 100);
        assert_eq!((d.width, d.height), (100, 50));
        assert_eq!(d.pixel(50, 25), [10, 20, 30, 255]);
        assert_eq!(d.bgra.len(), 100 * 50 * 4);
    }

    #[test]
    fn downscale_blends_a_checkerboard_to_grey() {
        let mut f = Frame::solid(8, 8, 0, 0, 0);
        for y in 0..8usize {
            for x in 0..8usize {
                if (x + y) % 2 == 0 {
                    let i = (y * 8 + x) * 4;
                    f.bgra[i..i + 3].copy_from_slice(&[200, 200, 200]);
                }
            }
        }
        let d = downscale(&f, 4);
        assert_eq!(d.pixel(1, 1)[0], 100);
    }

    #[test]
    fn new_rejects_a_wrong_buffer() {
        assert!(Frame::new(2, 2, vec![0; 15]).is_none());
        assert!(Frame::new(0, 2, vec![]).is_none());
        assert!(Frame::new(2, 2, vec![0; 16]).is_some());
    }

    #[test]
    fn blank_detection() {
        assert!(Frame::solid(100, 100, 0, 0, 0).is_blank());
        assert!(!Frame::solid(100, 100, 0, 0, 90).is_blank());
    }

    #[test]
    fn a_sheet_places_each_frame_in_its_cell() {
        let frames = vec![Frame::solid(640, 360, 255, 0, 0), Frame::solid(640, 360, 0, 255, 0), Frame::solid(640, 360, 0, 0, 255)];
        let s = contact_sheet(&frames, 2, 100, 60).unwrap();
        assert_eq!((s.width, s.height), (2 * 100 + 3 * 4, 2 * 60 + 3 * 4));
        // Cell (0,0) is blue, (1,0) green, (0,1) red; the unused cell (1,1) stays the dark gap colour.
        assert_eq!(s.pixel(4 + 50, 4 + 30), [255, 0, 0, 255]);
        assert_eq!(s.pixel(4 + 104 + 50, 4 + 30), [0, 255, 0, 255]);
        assert_eq!(s.pixel(4 + 50, 4 + 64 + 30), [0, 0, 255, 255]);
        assert_eq!(s.pixel(4 + 104 + 50, 4 + 64 + 30), [20, 20, 20, 255]);
        assert!(contact_sheet(&[], 2, 10, 10).is_none());
    }

    #[test]
    fn bmp_has_the_right_header_and_size() {
        let b = bmp(&Frame::solid(3, 2, 1, 2, 3));
        assert_eq!(&b[..2], b"BM");
        assert_eq!(b.len(), 54 + 12 * 2);
        assert_eq!(u32::from_le_bytes(b[2..6].try_into().unwrap()) as usize, b.len());
        assert_eq!(&b[54..57], &[1, 2, 3]);
    }
}
