use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, ImageFormat, RgbImage};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif"];

pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// Doğal sıralama (page2 < page10).
pub fn natural_key(s: &str) -> Vec<(bool, u64, String)> {
    let mut out = Vec::new();
    let mut num = String::new();
    let mut txt = String::new();
    let flush_txt = |txt: &mut String, out: &mut Vec<(bool, u64, String)>| {
        if !txt.is_empty() {
            out.push((false, 0, txt.to_lowercase()));
            txt.clear();
        }
    };
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            flush_txt(&mut txt, &mut out);
            num.push(ch);
        } else {
            if !num.is_empty() {
                out.push((true, num.parse().unwrap_or(0), String::new()));
                num.clear();
            }
            txt.push(ch);
        }
    }
    if !num.is_empty() {
        out.push((true, num.parse().unwrap_or(0), String::new()));
    }
    flush_txt(&mut txt, &mut out);
    out
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn load_rgb(path: &Path) -> Result<RgbImage> {
    let img = image::ImageReader::open(path)
        .with_context(|| format!("açılamadı: {}", path.display()))?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("çözülemedi: {}", path.display()))?;
    Ok(flatten_rgb(img))
}

/// Alfa varsa beyaz zemine yaslar.
pub fn flatten_rgb(img: DynamicImage) -> RgbImage {
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        let mut out = RgbImage::new(rgba.width(), rgba.height());
        for (o, p) in out.pixels_mut().zip(rgba.pixels()) {
            let a = p[3] as u32;
            for c in 0..3 {
                o[c] = ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
            }
        }
        out
    } else {
        img.to_rgb8()
    }
}

pub fn load_gray(path: &Path) -> Result<GrayImage> {
    Ok(image::open(path).with_context(|| format!("maske açılamadı: {}", path.display()))?.to_luma8())
}

pub fn dimensions(path: &Path) -> Result<(u32, u32)> {
    Ok(image::ImageReader::open(path)?.with_guessed_format()?.into_dimensions()?)
}

pub fn encode_png(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png)?;
    Ok(buf.into_inner())
}

pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    img.write_with_encoder(enc)?;
    Ok(buf)
}

pub fn save_png_atomic(path: &Path, img: &DynamicImage) -> Result<()> {
    crate::paths::atomic_write(path, &encode_png(img)?)?;
    Ok(())
}

pub fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("gif") => "image/gif",
        _ => "image/jpeg",
    }
}
