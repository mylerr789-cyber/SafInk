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

/// Koharu gibi karo planı: maske bileşenleri çekirdekleri `core` px'i aşmayan karolara gruplanır, her karo çekirdek + `context` px
/// bağlamla yerel çözünürlükte (ölçekleme yok) modele gider. `postprocess`: difüzyon çıktısına MangaTranslator son işlemi
/// (ışıklılık eşleme + glif kompoziti + ton eşleme) uygulanır; LaMa çıktısı sert yapıştırılır.
/// `mt_regions`: Koharu karoları yerine MangaTranslator bölge yolu (Qwen): bileşen başına kırpıntı, ~1 MP ölçek, bkz. `mtcrop`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSpec {
    pub core: u32,
    pub context: u32,
    pub postprocess: bool,
    pub mt_regions: bool,
}

impl TileSpec {
    pub const DEFAULT: TileSpec = TileSpec { core: 512, context: 128, postprocess: false, mt_regions: false };
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
/// Boyutları 16'nın katına kenar çoğaltarak tamamlar (ölçekleme yok); sonucu `crop_to` ile özgün boyuta kırpın.
pub fn pad16(img: &RgbImage) -> RgbImage {
    let r = |v: u32| ((v + 15) / 16 * 16).max(64);
    pad_rgb(img, r(img.width()), r(img.height()))
}
/// Dolgu katı katalog `params.multiple` (varsayılan 16; Qwen-Image 2.1 için 32).
pub fn pad_for(img: &RgbImage, params: &serde_json::Value) -> RgbImage {
    let m = params.get("multiple").and_then(|v| v.as_u64()).unwrap_or(16).max(1) as u32;
    let r = |v: u32| ((v + m - 1) / m * m).max(64);
    pad_rgb(img, r(img.width()), r(img.height()))
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

/// Katalog `params.mt_crop`: MangaTranslator bölge yolu (ham kırpıntı + ~1 MP ölçek, bkz. `mtcrop`).
pub fn is_mt(params: &serde_json::Value) -> bool {
    params.get("mt_crop").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Redraw modellerinin girdi hazırlığı (katalog `params.input_mode`):
/// - `original`: sayfa olduğu gibi gider (maske modele verilmez),
/// - `raw`: `original` ile aynı (ham kırpıntı, yeşil yok); MangaTranslator komutu `prompt` ile gelir (Qwen),
/// - `green`: maskeli (genişletilmiş) pikseller saf #00FF00 boyanır, komut "yeşil alanları kaldır" der,
/// - `lama_prefill`: maske önce LaMa ile doldurulur (karo karo), Flux yalnızca düzeltir.
/// Karo çıktıları yalnızca maskeli piksellerde sayfaya yazılır. Geliştirme için `SAFINK_INPUT_MODE` / `SAFINK_PROMPT`
/// ortam değişkenleri katalog değerinin üstüne yazar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Original,
    Raw,
    Green,
    LamaPrefill,
}

pub struct InputPrep {
    pub mode: InputMode,
    pub prompt: String,
    lama: Option<Box<dyn Inpainter>>,
}

impl InputPrep {
    pub fn new(paths: &AppPaths, spec: &ModelSpec, settings: &Settings) -> Result<Self> {
        let ps = |k: &str| spec.params.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let mode_name = std::env::var("SAFINK_INPUT_MODE").ok().or_else(|| ps("input_mode")).unwrap_or_else(|| "original".into());
        let mode = match mode_name.as_str() {
            "original" => InputMode::Original,
            "raw" => InputMode::Raw,
            "green" => InputMode::Green,
            "lama_prefill" => InputMode::LamaPrefill,
            m => bail!("bilinmeyen input_mode: {m}"),
        };
        let key = match mode {
            InputMode::Original | InputMode::Raw => "prompt",
            InputMode::Green => "prompt_green",
            InputMode::LamaPrefill => "prompt_lama_prefill",
        };
        let prompt = std::env::var("SAFINK_PROMPT").ok().or_else(|| ps(key)).or_else(|| ps("prompt")).unwrap_or_else(|| "Remove all text.".into());
        let lama = if mode == InputMode::LamaPrefill {
            let cat = crate::catalog::Catalog::load(paths, settings.low_vram);
            let lspec = cat.get("lama-manga").ok_or_else(|| anyhow::anyhow!("lama-manga kataloğda yok"))?.clone();
            Some(create(paths, &lspec, settings)?)
        } else {
            None
        };
        Ok(Self { mode, prompt, lama })
    }

    /// Modele gidecek görüntü (kırpıntı boyutunda; dolgu çağıranda yapılır).
    pub fn prepare(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> Result<RgbImage> {
        match self.mode {
            InputMode::Original | InputMode::Raw => Ok(img.clone()),
            InputMode::Green => {
                let mut o = img.clone();
                for (x, y, m) in mask.enumerate_pixels() {
                    if m[0] >= 128 {
                        o.put_pixel(x, y, image::Rgb([0, 255, 0]));
                    }
                }
                Ok(o)
            }
            InputMode::LamaPrefill => {
                let lama = self.lama.as_mut().expect("lama_prefill için LaMa yüklü");
                // maske zaten genişletilmiş; LaMa sonucu tüysüz tam doldurma olarak alınır
                crate::pipeline::clean_image(img, mask, &crate::pipeline::CleanParams { dilation: 0, feather: 0, flat_fill: false }, None, lama.as_mut(), ctx.cancel, &|_, _| {})
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn raw_input_mode_sends_the_original_crop_without_green() {
        let mut img = RgbImage::from_pixel(64, 64, Rgb([200, 190, 180]));
        img.put_pixel(10, 10, Rgb([0, 0, 0]));
        let mut mask = GrayImage::new(64, 64);
        for y in 5..20 {
            for x in 5..20 {
                mask.put_pixel(x, y, image::Luma([255]));
            }
        }
        let mut prep = InputPrep { mode: InputMode::Raw, prompt: String::new(), lama: None };
        let cancel = AtomicBool::new(false);
        let out = prep.prepare(&img, &mask, &Ctx { cancel: &cancel }).unwrap();
        assert_eq!(out.as_raw(), img.as_raw());
        assert!(out.pixels().all(|p| *p != Rgb([0, 255, 0])));
        // karşılaştırma: green kipi maskeli pikselleri boyar
        let mut g = InputPrep { mode: InputMode::Green, prompt: String::new(), lama: None };
        assert_eq!(g.prepare(&img, &mask, &Ctx { cancel: &cancel }).unwrap().get_pixel(10, 10), &Rgb([0, 255, 0]));
    }
}
