//! Inpaint arka uçları: LaMa (NVIDIA: PyTorch worker, diğer: ONNX CPU), sd.cpp (sd-cli sidecar), SDNQ (gömülü Python).
pub mod ort_lama;
pub mod pyworker;
pub mod sdcpp;
pub mod sdserver;
pub mod sdnq;
pub mod torch_lama;

use crate::catalog::ModelSpec;
use crate::paths::AppPaths;
use crate::settings::Settings;
use anyhow::{bail, Result};
use image::{GrayImage, RgbImage};
use std::sync::atomic::AtomicBool;

/// Karo planı. LaMa (Koharu gibi): maske bileşenleri çekirdekleri `core` px'i aşmayan karolara gruplanır, her karo çekirdek + `context`
/// px bağlamla yerel çözünürlükte (ölçekleme yok) modele gider, çıktı yalnızca maskeli piksellerde yazılır.
/// `mt_regions`: redraw (difüzyon) modelleri karo yerine MangaTranslator bölge yolunu kullanır (bileşen başına kırpıntı, ~1 MP ölçek,
/// hizalama + ışıklılık eşleme + glif kompoziti; bkz. `mtcrop`, `pipeline::clean_regions_mt`). `multiple`: bölge yolunda
/// kırpıntı/çıkarım boyutlarının katı (katalog `params.multiple`; Klein 16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSpec {
    pub core: u32,
    pub context: u32,
    pub mt_regions: bool,
    pub multiple: u32,
}

impl TileSpec {
    pub const DEFAULT: TileSpec = TileSpec { core: 512, context: 128, mt_regions: false, multiple: 16 };
    /// Redraw (difüzyon) modelleri: yalnızca MangaTranslator bölge yolu.
    pub fn redraw(multiple: u32) -> TileSpec { TileSpec { mt_regions: true, multiple, ..TileSpec::DEFAULT } }
}

pub struct Ctx<'a> {
    pub cancel: &'a AtomicBool,
}

pub trait Inpainter: Send {
    fn label(&self) -> String;
    fn tile_spec(&self) -> TileSpec;
    /// `img`: yerel çözünürlükte kırpıntı (en çok core + 2*context). `mask`: 255 = silinecek/yeniden çizilecek.
    /// Çıktı `img` ile aynı boyutta olmalı; model boyut katına dolgu/kırpmayı kendisi yapar.
    fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> Result<RgbImage>;
}

pub fn create(paths: &AppPaths, spec: &ModelSpec, settings: &Settings) -> Result<Box<dyn Inpainter>> {
    create_with(paths, spec, settings, &AtomicBool::new(false), &|_| {})
}

/// `create` + yükleme ilerlemesi/iptali (sd.cpp sunucusu modeli burada yükler; diğer arka uçlar yok sayar).
pub fn create_with(paths: &AppPaths, spec: &ModelSpec, settings: &Settings, cancel: &AtomicBool, progress: &dyn Fn(String)) -> Result<Box<dyn Inpainter>> {
    if !spec.is_installed(paths) {
        bail!(crate::t!("model kurulu değil: {} (Modeller bölümünden indirin)", "model not installed: {} (download it in Models)", spec.name));
    }
    match spec.backend.as_str() {
        "lama" if crate::catalog::hardware().nvidia() => Ok(Box::new(torch_lama::TorchLama::new(paths, spec)?)),
        "lama" => Ok(Box::new(ort_lama::OrtInpainter::new(paths, spec)?)),
        "sdcpp" => Ok(Box::new(sdcpp::SdCppInpainter::new_with(paths, spec, settings, cancel, progress)?)),
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
/// Model çıktısını dolgusuz özgün boyuta kırpar; beklenmeyen boyutta hata (ölçekleme yapılmaz).
pub fn crop_to(out: RgbImage, w: u32, h: u32, padded: (u32, u32)) -> Result<RgbImage> {
    if out.dimensions() != padded {
        bail!(crate::t!("model beklenmeyen boyut döndürdü: {}x{} (beklenen {}x{})", "model returned an unexpected size: {}x{} (expected {}x{})", out.width(), out.height(), padded.0, padded.1));
    }
    Ok(image::imageops::crop_imm(&out, 0, 0, w, h).to_image())
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

/// Katalog `params.multiple` (varsayılan 16).
pub fn multiple_of(params: &serde_json::Value) -> u32 {
    params.get("multiple").and_then(|v| v.as_u64()).unwrap_or(16).max(1) as u32
}

/// Redraw modelinin komutu: katalog `params.prompt` (MangaTranslator komutu). Geliştirme için `SAFINK_PROMPT` ortam değişkeni üstüne yazar.
pub fn prompt_of(spec: &ModelSpec) -> String {
    std::env::var("SAFINK_PROMPT")
        .ok()
        .or_else(|| spec.params.get("prompt").and_then(|v| v.as_str()).map(str::to_string))
        .unwrap_or_else(|| "Remove all text.".into())
}
