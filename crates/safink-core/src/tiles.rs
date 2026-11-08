//! Karo sunucusu: büyük şeritler için 1024x1024 parçalar (orig / clean / mask / edit katmanları) ve küçük resimler.
//! Arayüz bunları özel URI şeması ile ister; base64 yok.
use crate::imgio;
use crate::paths::AppPaths;
use anyhow::{anyhow, bail, Result};
use image::{DynamicImage, GrayImage, ImageFormat, RgbImage};
use parking_lot::Mutex;
use std::sync::Arc;

pub const TILE: u32 = 1024;

pub struct TileCache {
    items: Mutex<Vec<(String, Arc<DynamicImage>)>>,
    cap: usize,
}

impl Default for TileCache {
    fn default() -> Self { Self { items: Mutex::new(Vec::new()), cap: 4 } }
}

impl TileCache {
    pub fn get_or_load(&self, key: &str, load: impl FnOnce() -> Result<DynamicImage>) -> Result<Arc<DynamicImage>> {
        {
            let mut g = self.items.lock();
            if let Some(i) = g.iter().position(|(k, _)| k == key) {
                let it = g.remove(i);
                g.push(it.clone());
                return Ok(it.1);
            }
        }
        let img = Arc::new(load()?);
        let mut g = self.items.lock();
        g.push((key.to_string(), img.clone()));
        while g.len() > self.cap {
            g.remove(0);
        }
        Ok(img)
    }
    /// Bir sayfanın tüm katmanlarını önbellekten at.
    pub fn invalidate(&self, page_id: i64) {
        let prefix = format!("{page_id}:");
        self.items.lock().retain(|(k, _)| !k.starts_with(&prefix));
    }
}

pub struct TileRequest {
    pub page_id: i64,
    pub layer: String,
    pub tx: u32,
    pub ty: u32,
}

/// Çözülmüş katman görüntüsü. Rev, önbellek anahtarına girer: değişince yeniden yüklenir.
fn layer_image(paths: &AppPaths, cache: &TileCache, page: &crate::db::Page, layer: &str) -> Result<Arc<DynamicImage>> {
    let key = format!("{}:{}:{}", page.id, layer, page.rev);
    let (pid, rev) = (page.id, page.rev);
    let _ = (pid, rev);
    cache.get_or_load(&key, || match layer {
        "orig" => Ok(DynamicImage::ImageRgb8(imgio::load_rgb(std::path::Path::new(&page.path))?)),
        "clean" => Ok(DynamicImage::ImageRgb8(imgio::load_rgb(&paths.cleaned_file(page.id))?)),
        "mask" => {
            let p = paths.mask_file(&page.sha256, page.mask_conf as f32);
            if !p.exists() {
                bail!("maske yok");
            }
            let g = imgio::load_gray(&p)?;
            Ok(DynamicImage::ImageLuma8(fit_gray(g, page.width as u32, page.height as u32)))
        }
        "edit" => {
            let p = paths.edit_file(page.id);
            if !p.exists() {
                bail!("düzenleme yok");
            }
            Ok(DynamicImage::ImageLuma8(imgio::load_gray(&p)?))
        }
        l => Err(anyhow!("bilinmeyen katman {l}")),
    })
}

fn fit_gray(g: GrayImage, w: u32, h: u32) -> GrayImage {
    if g.dimensions() == (w, h) { g } else { image::imageops::resize(&g, w, h, image::imageops::FilterType::Nearest) }
}

pub fn render_tile(paths: &AppPaths, cache: &TileCache, page: &crate::db::Page, req: &TileRequest) -> Result<(Vec<u8>, &'static str)> {
    let img = layer_image(paths, cache, page, &req.layer)?;
    let (x0, y0) = (req.tx * TILE, req.ty * TILE);
    if x0 >= img.width() || y0 >= img.height() {
        bail!("karo sınır dışı");
    }
    let (w, h) = (TILE.min(img.width() - x0), TILE.min(img.height() - y0));
    let crop = img.crop_imm(x0, y0, w, h);
    let mut buf = std::io::Cursor::new(Vec::new());
    match req.layer.as_str() {
        "orig" | "clean" => {
            let rgb: RgbImage = crop.to_rgb8();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 92);
            rgb.write_with_encoder(enc)?;
            Ok((buf.into_inner(), "image/jpeg"))
        }
        _ => {
            crop.write_to(&mut buf, ImageFormat::Png)?;
            Ok((buf.into_inner(), "image/png"))
        }
    }
}

/// Küçük resim (disk önbellekli JPEG).
pub fn thumbnail(paths: &AppPaths, page: &crate::db::Page) -> Result<Vec<u8>> {
    let tp = paths.thumb_file(page.id);
    if let Ok(b) = std::fs::read(&tp) {
        return Ok(b);
    }
    let img = imgio::load_rgb(std::path::Path::new(&page.path))?;
    let t = DynamicImage::ImageRgb8(img).thumbnail(320, 440).to_rgb8();
    let bytes = imgio::encode_jpeg(&t, 82)?;
    let _ = crate::paths::atomic_write(&tp, &bytes);
    Ok(bytes)
}

/// Düzenleme kareleri: [u32 tx][u32 ty][TILE*TILE bayt] kayıtları (LE).
pub fn save_edit_tiles(paths: &AppPaths, page: &crate::db::Page, body: &[u8]) -> Result<()> {
    let (w, h) = (page.width as u32, page.height as u32);
    let p = paths.edit_file(page.id);
    let mut edit = if p.exists() { imgio::load_gray(&p).ok().filter(|g| g.dimensions() == (w, h)).unwrap_or_else(|| GrayImage::new(w, h)) } else { GrayImage::new(w, h) };
    let rec = 8 + (TILE * TILE) as usize;
    if body.len() % rec != 0 {
        bail!("geçersiz karo verisi");
    }
    for chunk in body.chunks_exact(rec) {
        let tx = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let ty = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
        let data = &chunk[8..];
        let (x0, y0) = (tx * TILE, ty * TILE);
        if x0 >= w || y0 >= h {
            continue;
        }
        for y in 0..TILE.min(h - y0) {
            for x in 0..TILE.min(w - x0) {
                edit.put_pixel(x0 + x, y0 + y, image::Luma([data[(y * TILE + x) as usize].min(2)]));
            }
        }
    }
    if edit.as_raw().iter().all(|&v| v == 0) {
        let _ = std::fs::remove_file(&p);
    } else {
        imgio::save_png_atomic(&p, &DynamicImage::ImageLuma8(edit))?;
    }
    Ok(())
}
