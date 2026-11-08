//! Inpaint arka uçları: LaMa (NVIDIA: PyTorch worker, diğer: ONNX CPU), sd.cpp (sd-cli sidecar), SDNQ (gömülü Python).
pub mod ort_lama;
pub mod pyworker;
pub mod sdcpp;
pub mod sdnq;
pub mod torch_lama;

use crate::catalog::ModelSpec;
use crate::paths::AppPaths;
use crate::settings::Settings;
use anyhow::{bail, Result};
use image::{GrayImage, RgbImage};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug)]
pub enum WindowPolicy {
    /// Sabit kare pencere (ONNX, 512).
    Fixed(u32),
    /// Tam sayfa (redraw): sayfa bütün gider; yüksekliği `seg_h`'yi aşan şeritler `overlap` px örtüşen dilimlere bölünür.
    /// Genişlik `max_w`, alan `max_px` aşılırsa hata (küçültme yok).
    Page { seg_h: u32, overlap: u32, max_w: u32, max_px: u64 },
}

pub struct Ctx<'a> {
    pub cancel: &'a AtomicBool,
}

pub trait Inpainter: Send {
    fn label(&self) -> String;
    fn policy(&self) -> WindowPolicy;
    /// `mask`: 255 = silinecek/yeniden çizilecek. Çıktı `img` ile aynı boyutta olmalı.
    fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> Result<RgbImage>;
}

pub fn create(paths: &AppPaths, spec: &ModelSpec, settings: &Settings) -> Result<Box<dyn Inpainter>> {
    if !spec.is_installed(paths) {
        bail!("model kurulu değil: {} (Modeller bölümünden indirin)", spec.name);
    }
    match spec.backend.as_str() {
        "lama" if crate::catalog::hardware().nvidia() => Ok(Box::new(torch_lama::TorchLama::new(paths, spec)?)),
        "lama" => Ok(Box::new(ort_lama::OrtInpainter::new(paths, spec)?)),
        "sdcpp" => Ok(Box::new(sdcpp::SdCppInpainter::new(paths, spec, settings)?)),
        "sdnq" => Ok(Box::new(sdnq::SdnqInpainter::new(paths, spec, settings)?)),
        b => bail!("bilinmeyen arka uç: {b}"),
    }
}

/// Kırpıntıyı `w`x`h`'ye kenar çoğaltarak tamamlar.
pub fn pad_rgb(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    if img.width() == w && img.height() == h {
        return img.clone();
    }
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.put_pixel(x, y, *img.get_pixel(x.min(img.width() - 1), y.min(img.height() - 1)));
        }
    }
    out
}
/// Boyutları 16'nın katına kenar çoğaltarak tamamlar (ölçekleme yok); sonucu `crop_to` ile özgün boyuta kırpın.
pub fn pad16(img: &RgbImage) -> RgbImage {
    let r = |v: u32| ((v + 15) / 16 * 16).max(64);
    pad_rgb(img, r(img.width()), r(img.height()))
}
/// Model çıktısını dolgusuz özgün boyuta kırpar; beklenmeyen boyutta hata (ölçekleme yapılmaz).
pub fn crop_to(out: RgbImage, w: u32, h: u32, padded: (u32, u32)) -> Result<RgbImage> {
    if out.dimensions() != padded {
        bail!("model beklenmeyen boyut döndürdü: {}x{} (beklenen {}x{})", out.width(), out.height(), padded.0, padded.1);
    }
    Ok(image::imageops::crop_imm(&out, 0, 0, w, h).to_image())
}
/// Redraw modelleri için tam sayfa politikası (katalog `params`: seg_h, overlap, max_width, max_pixels).
pub fn page_policy(params: &serde_json::Value) -> WindowPolicy {
    let u = |k: &str, d: u64| params.get(k).and_then(|v| v.as_u64()).unwrap_or(d);
    WindowPolicy::Page { seg_h: u("seg_h", 2000) as u32, overlap: u("overlap", 200) as u32, max_w: u("max_width", 2048) as u32, max_px: u("max_pixels", 6_300_000) }
}
pub fn pad_gray(img: &GrayImage, w: u32, h: u32) -> GrayImage {
    let mut out = GrayImage::new(w, h);
    for y in 0..img.height().min(h) {
        for x in 0..img.width().min(w) {
            out.put_pixel(x, y, *img.get_pixel(x, y));
        }
    }
    out
}
