//! The camera-style finish for pictures made by Image Studio, and the preview shown in the window.
//!
//! Image models make very clean pictures: no grain, soft even colour, edges too perfect. A real camera
//! adds a little softness, colour fringing at the edges, flatter colour, a darker corner, fine sensor
//! grain and JPEG compression. This does the same, scaled by a strength from 0 to 100.

use std::path::Path;

use base64::Engine;
use image::{codecs::jpeg::JpegEncoder, imageops, ImageEncoder, RgbImage};

use crate::error::{AppError, AppResult};

fn img_err(e: image::ImageError) -> AppError {
    AppError::msg(format!("could not process the picture: {e}"))
}

/// Small, fast, repeatable noise: the same picture and strength always get the same grain.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// About a normal distribution with mean 0 and standard deviation 1 (sum of four uniforms).
    fn gauss(&mut self) -> f32 {
        let mut t = 0.0f32;
        for _ in 0..4 {
            t += (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        }
        (t - 2.0) * 1.732
    }
}

/// Applies the finish. `strength` 0..=100.
pub fn apply(src: &RgbImage, strength: u32) -> RgbImage {
    let k = strength.min(100) as f32 / 100.0;
    // Pictures enlarged 4x are shrunk back a little: the extra pixels keep their texture and lose the
    // enlarger's over-sharp edges.
    let mut img = if src.width() > 2400 {
        let w = 2240;
        let h = (src.height() as f32 * w as f32 / src.width() as f32).round() as u32;
        imageops::resize(src, w, h, imageops::FilterType::Lanczos3)
    } else {
        src.clone()
    };
    let (w, h) = img.dimensions();
    if k > 0.0 {
        // The lens: a hair of softness, then the camera's own sharpening.
        img = imageops::blur(&img, 0.3 + 0.4 * k);
        img = imageops::unsharpen(&img, 1.4, 3);
    }
    let contrast = 1.0 - 0.06 * k;
    let saturation = 1.0 - 0.08 * k;
    let grain = 10.0 * k;
    let colour_grain = 3.0 * k;
    let mut noise = Noise(0x9E37_79B9_7F4A_7C15 ^ (w as u64) << 20 ^ h as u64 ^ strength as u64);
    let src_px = img.clone();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let max_r = (cx * cx + cy * cy).sqrt();
    let shift = if k > 0.0 { 1i64 } else { 0 };
    for y in 0..h {
        for x in 0..w {
            // Colour fringing: red a pixel to the right, blue a pixel to the left.
            let xr = (x as i64 + shift).clamp(0, w as i64 - 1) as u32;
            let xb = (x as i64 - shift).clamp(0, w as i64 - 1) as u32;
            let mut r = src_px.get_pixel(xr, y)[0] as f32;
            let mut g = src_px.get_pixel(x, y)[1] as f32;
            let mut b = src_px.get_pixel(xb, y)[2] as f32;
            // Flatter colour.
            let l = 0.299 * r + 0.587 * g + 0.114 * b;
            r = l + (r - l) * saturation;
            g = l + (g - l) * saturation;
            b = l + (b - l) * saturation;
            r = (r - 128.0) * contrast + 128.0;
            g = (g - 128.0) * contrast + 128.0;
            b = (b - 128.0) * contrast + 128.0;
            // A darker corner.
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = ((dx * dx + dy * dy).sqrt() / max_r - 0.55).max(0.0) * 0.5 * k;
            let v = 1.0 - d * 0.6;
            // Sensor grain: the same for all three colours, plus a little colour noise.
            let n = noise.gauss() * grain;
            let px = img.get_pixel_mut(x, y);
            px[0] = (r * v + n + noise.gauss() * colour_grain).clamp(0.0, 255.0) as u8;
            px[1] = (g * v + n + noise.gauss() * colour_grain).clamp(0.0, 255.0) as u8;
            px[2] = (b * v + n + noise.gauss() * colour_grain).clamp(0.0, 255.0) as u8;
        }
    }
    img
}

/// Reads `src`, applies the finish and writes a JPEG like a camera would.
pub fn finish_file(src: &Path, dst: &Path, strength: u32) -> AppResult<()> {
    let img = image::open(src).map_err(img_err)?.to_rgb8();
    let out = apply(&img, strength);
    let file = std::fs::File::create(dst)?;
    let enc = JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 92);
    enc.write_image(out.as_raw(), out.width(), out.height(), image::ExtendedColorType::Rgb8).map_err(img_err)
}

/// The picture's size without decoding it.
pub fn dims(path: &Path) -> AppResult<(u32, u32)> {
    image::image_dimensions(path).map_err(img_err)
}

fn save_png(img: &RgbImage, dst: &Path) -> AppResult<()> {
    img.save_with_format(dst, image::ImageFormat::Png).map_err(img_err)
}

/// Cuts out a rectangle given as fractions of the picture (0..1).
pub fn crop_file(src: &Path, dst: &Path, x: f32, y: f32, w: f32, h: f32) -> AppResult<(u32, u32)> {
    let img = image::open(src).map_err(img_err)?.to_rgb8();
    let (iw, ih) = img.dimensions();
    let cx = ((x.clamp(0.0, 1.0)) * iw as f32).round() as u32;
    let cy = ((y.clamp(0.0, 1.0)) * ih as f32).round() as u32;
    let cw = ((w.clamp(0.02, 1.0)) * iw as f32).round().max(8.0) as u32;
    let ch = ((h.clamp(0.02, 1.0)) * ih as f32).round().max(8.0) as u32;
    let cx = cx.min(iw.saturating_sub(8));
    let cy = cy.min(ih.saturating_sub(8));
    let out = imageops::crop_imm(&img, cx, cy, cw.min(iw - cx), ch.min(ih - cy)).to_image();
    save_png(&out, dst)?;
    Ok(out.dimensions())
}

/// Smears the picture along a line, like a camera that moved while the shutter was open.
pub fn motion_blur_file(src: &Path, dst: &Path, length: u32, angle_deg: f32) -> AppResult<()> {
    let img = image::open(src).map_err(img_err)?.to_rgb8();
    let (w, h) = img.dimensions();
    let n = length.clamp(2, 80) as i32;
    let (dx, dy) = (angle_deg.to_radians().cos(), angle_deg.to_radians().sin());
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for i in 0..n {
                let t = i as f32 - (n - 1) as f32 / 2.0;
                let sx = (x as f32 + dx * t).round().clamp(0.0, w as f32 - 1.0) as u32;
                let sy = (y as f32 + dy * t).round().clamp(0.0, h as f32 - 1.0) as u32;
                let p = img.get_pixel(sx, sy);
                r += p[0] as u32;
                g += p[1] as u32;
                b += p[2] as u32;
            }
            let n = n as u32;
            out.put_pixel(x, y, image::Rgb([(r / n) as u8, (g / n) as u8, (b / n) as u8]));
        }
    }
    save_png(&out, dst)
}

/// Film grain alone (no softening, fringing or vignette). `strength` 0..=100.
pub fn grain_file(src: &Path, dst: &Path, strength: u32) -> AppResult<()> {
    let mut img = image::open(src).map_err(img_err)?.to_rgb8();
    let (w, h) = img.dimensions();
    let k = strength.min(100) as f32 / 100.0;
    let mut noise = Noise(0x9E37_79B9_7F4A_7C15 ^ (w as u64) << 20 ^ h as u64 ^ strength as u64);
    for px in img.pixels_mut() {
        let n = noise.gauss() * 14.0 * k;
        for c in px.0.iter_mut() {
            *c = (*c as f32 + n + noise.gauss() * 3.0 * k).clamp(0.0, 255.0) as u8;
        }
    }
    save_png(&img, dst)
}

/// Makes the picture `percent` of its size (for example an enlarged 4x picture back to 2x).
pub fn resize_file(src: &Path, dst: &Path, percent: u32) -> AppResult<(u32, u32)> {
    let img = image::open(src).map_err(img_err)?.to_rgb8();
    let f = percent.clamp(10, 400) as f32 / 100.0;
    let (w, h) = ((img.width() as f32 * f).round().max(1.0) as u32, (img.height() as f32 * f).round().max(1.0) as u32);
    let out = imageops::resize(&img, w, h, imageops::FilterType::Lanczos3);
    save_png(&out, dst)?;
    Ok((w, h))
}

/// Writes a data address (the poster the window drew) to a PNG file.
pub fn write_data_url(data_url: &str, dst: &Path) -> AppResult<()> {
    let b64 = data_url.split_once("base64,").map(|(_, b)| b).ok_or_else(|| AppError::msg("not a picture"))?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| AppError::msg(format!("not a picture: {e}")))?;
    let img = image::load_from_memory(&bytes).map_err(img_err)?;
    img.save_with_format(dst, image::ImageFormat::Png).map_err(img_err)
}

/// How bright each part of the picture is: a grid of `cols` x `rows` averages, 0..=255, row by row.
pub fn brightness_grid(path: &Path, cols: u32, rows: u32) -> AppResult<Vec<u8>> {
    let img = image::open(path).map_err(img_err)?.to_rgb8();
    let small = imageops::resize(&img, cols, rows, imageops::FilterType::Triangle);
    Ok(small.pixels().map(|p| (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8).collect())
}

/// Writes the picture again without the prompt and settings the engine stores inside it.
pub fn strip_file(src: &Path) -> AppResult<()> {
    let img = image::open(src).map_err(img_err)?;
    img.save(src).map_err(img_err)
}

/// A JPEG data address of the picture shrunk to fit `max` pixels, and the picture's real size.
pub fn preview(path: &Path, max: u32) -> AppResult<(String, u32, u32)> {
    let img = image::open(path).map_err(img_err)?.to_rgb8();
    let (w, h) = img.dimensions();
    let small = if w.max(h) > max {
        let f = max as f32 / w.max(h) as f32;
        imageops::resize(&img, (w as f32 * f).round().max(1.0) as u32, (h as f32 * f).round().max(1.0) as u32, imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, 85).write_image(small.as_raw(), small.width(), small.height(), image::ExtendedColorType::Rgb8).map_err(img_err)?;
    Ok((format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&buf)), w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn smooth(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| Rgb([(100 + x * 40 / w) as u8, (110 + y * 40 / h) as u8, 120]))
    }

    fn roughness(img: &RgbImage) -> f32 {
        let mut t = 0.0f32;
        let mut n = 0.0f32;
        for y in 0..img.height() {
            for x in 1..img.width() {
                t += (img.get_pixel(x, y)[1] as f32 - img.get_pixel(x - 1, y)[1] as f32).abs();
                n += 1.0;
            }
        }
        t / n
    }

    #[test]
    fn adds_grain_to_a_smooth_picture_in_proportion_to_strength() {
        let src = smooth(200, 160);
        let none = roughness(&apply(&src, 0));
        let some = roughness(&apply(&src, 40));
        let lots = roughness(&apply(&src, 100));
        assert!(none < 1.0, "{none}");
        assert!(some > none + 2.0 && lots > some, "{none} {some} {lots}");
    }

    #[test]
    fn strength_zero_leaves_the_picture_alone() {
        let src = smooth(64, 48);
        let out = apply(&src, 0);
        let worst = src.pixels().zip(out.pixels()).map(|(a, b)| (a[0] as i32 - b[0] as i32).abs()).max().unwrap();
        assert!(worst <= 2, "{worst}");
    }

    #[test]
    fn a_big_enlarged_picture_is_shrunk_back() {
        let out = apply(&smooth(2600, 100), 30);
        assert_eq!(out.width(), 2240);
    }

    #[test]
    fn the_same_picture_gets_the_same_grain() {
        let src = smooth(80, 60);
        assert_eq!(apply(&src, 50).as_raw(), apply(&src, 50).as_raw());
    }

    #[test]
    fn writes_a_jpeg_and_a_preview() {
        let dir = std::env::temp_dir().join(format!("byte-finish-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (src, dst) = (dir.join("a.png"), dir.join("a.jpg"));
        smooth(300, 200).save(&src).unwrap();
        finish_file(&src, &dst, 40).unwrap();
        let (url, w, h) = preview(&dst, 100).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        assert_eq!((w, h), (300, 200));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tmpdir(n: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("byte-finish-{n}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn crops_a_rectangle_given_as_fractions() {
        let d = tmpdir("crop");
        let (a, b) = (d.join("a.png"), d.join("b.png"));
        smooth(400, 200).save(&a).unwrap();
        assert_eq!(crop_file(&a, &b, 0.25, 0.0, 0.5, 1.0).unwrap(), (200, 200));
        assert_eq!(dims(&b).unwrap(), (200, 200));
        // A rectangle that sticks out is pulled back inside.
        assert!(crop_file(&a, &b, 0.9, 0.9, 0.5, 0.5).is_ok());
        let (w, h) = dims(&b).unwrap();
        assert!(w <= 400 && h <= 200 && w >= 8 && h >= 8);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn motion_blur_smears_along_the_line_and_keeps_flat_areas() {
        let d = tmpdir("blur");
        let (a, b) = (d.join("a.png"), d.join("b.png"));
        // A white square on black.
        RgbImage::from_fn(60, 60, |x, y| if (20..40).contains(&x) && (20..40).contains(&y) { Rgb([255, 255, 255]) } else { Rgb([0, 0, 0]) }).save(&a).unwrap();
        motion_blur_file(&a, &b, 10, 0.0).unwrap();
        let o = image::open(&b).unwrap().to_rgb8();
        assert!(o.get_pixel(17, 30)[0] > 20 && o.get_pixel(17, 30)[0] < 200, "the edge is smeared sideways");
        assert_eq!(o.get_pixel(30, 5)[0], 0, "a dark area far from the square stays dark");
        assert_eq!(o.get_pixel(30, 30)[0], 255, "the middle of the square stays white");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn grain_only_adds_noise_and_keeps_the_size() {
        let d = tmpdir("grain");
        let (a, b) = (d.join("a.png"), d.join("b.png"));
        smooth(120, 80).save(&a).unwrap();
        grain_file(&a, &b, 60).unwrap();
        let o = image::open(&b).unwrap().to_rgb8();
        assert_eq!(o.dimensions(), (120, 80));
        assert!(roughness(&o) > roughness(&smooth(120, 80)) + 3.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resizes_by_percent() {
        let d = tmpdir("size");
        let (a, b) = (d.join("a.png"), d.join("b.png"));
        smooth(400, 300).save(&a).unwrap();
        assert_eq!(resize_file(&a, &b, 50).unwrap(), (200, 150));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_drawn_poster_data_address_becomes_a_png_file() {
        let d = tmpdir("poster");
        let src = d.join("a.png");
        smooth(30, 20).save(&src).unwrap();
        let bytes = std::fs::read(&src).unwrap();
        let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
        let out = d.join("p.png");
        write_data_url(&url, &out).unwrap();
        assert_eq!(dims(&out).unwrap(), (30, 20));
        assert!(write_data_url("data:text/plain;base64,aGk=", &out).is_err());
        assert!(write_data_url("nonsense", &out).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_brightness_grid_tells_dark_from_light() {
        let d = tmpdir("grid");
        let a = d.join("a.png");
        RgbImage::from_fn(80, 40, |x, _| if x < 40 { Rgb([10, 10, 10]) } else { Rgb([240, 240, 240]) }).save(&a).unwrap();
        let g = brightness_grid(&a, 4, 2).unwrap();
        assert_eq!(g.len(), 8);
        assert!(g[0] < 40 && g[3] > 200);
        let _ = std::fs::remove_dir_all(&d);
    }
}
