//! Konuşma balonu bölütleyici: YOLO11n-seg (manga109-segmentation-bubble), ONNX Runtime CPU.
//! Düz doldurma (pipeline.rs) yalnızca balonların içindeki yazıyı düz renkle doldurur; balon dışı yazı modele gider.
//! Sayfa letterbox ile 1024 kareye sığdırılır; uzun şeritler üst üste binen dilimlere bölünür, sonuçlar birleştirilir.
//! Sonuç sayfa çözünürlüğünde balon başına ikili maskedir ve sayfanın SHA-256'sına göre önbelleğe alınır.
use crate::paths::AppPaths;
use anyhow::{anyhow, bail, Result};
use image::{imageops::FilterType, Rgb, RgbImage};
use ort::{ep, session::Session, value::Tensor};
use std::io::{Read, Write};
use std::path::Path;

/// Modelin sabit giriş kenarı (ONNX dışa aktarımı 1024).
pub const INPUT: usize = 1024;
/// Güven eşiği ve NMS IoU eşiği.
pub const CONF: f32 = 0.35;
pub const NMS_IOU: f32 = 0.7;
const PROTO: usize = 256;
const NMASK: usize = 32;

/// Tek balon: sayfa koordinatlarında sınır kutusu `[x0, y0, x1, y1)` ve kutu boyutunda 0/1 maske.
#[derive(Clone, Debug)]
pub struct Bubble {
    pub bounds: [u32; 4],
    pub mask: Vec<u8>,
    pub score: f32,
}

impl Bubble {
    pub fn w(&self) -> u32 { self.bounds[2] - self.bounds[0] }
    pub fn h(&self) -> u32 { self.bounds[3] - self.bounds[1] }
    /// Sayfa koordinatındaki piksel balonun içinde mi?
    pub fn contains(&self, x: u32, y: u32) -> bool {
        let [x0, y0, x1, y1] = self.bounds;
        x >= x0 && x < x1 && y >= y0 && y < y1 && self.mask[((y - y0) * self.w() + (x - x0)) as usize] != 0
    }
    pub fn area(&self) -> usize { self.mask.iter().filter(|&&v| v != 0).count() }
    /// Sınır kutusu içinde `f(x, y)` (sayfa koordinatı) doğruysa içeride sayılan balon (test ve araçlar için).
    pub fn from_fn(bounds: [u32; 4], f: impl Fn(u32, u32) -> bool) -> Bubble {
        let (w, h) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
        let mut mask = vec![0u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                if f(bounds[0] + x, bounds[1] + y) {
                    mask[(y * w + x) as usize] = 1;
                }
            }
        }
        Bubble { bounds, mask, score: 1.0 }
    }
}

pub struct BubbleDetector {
    session: Session,
}

impl BubbleDetector {
    pub fn new(model: &Path) -> Result<Self> {
        let env = crate::inpaint::ort_lama::env()?;
        let b = Session::builder(env).map_err(|e| anyhow!("{e}"))?;
        let mut b = b.with_execution_providers([ep::CPU::default().build()]).map_err(|e| anyhow!("{e}"))?;
        let session = b.commit_from_file(model).map_err(|e| anyhow!("{e}"))?;
        Ok(Self { session })
    }

    /// Sayfadaki balonlar (sayfa koordinatlarında). Uzun şeritte dilimler birleştirilir.
    pub fn detect(&mut self, img: &RgbImage) -> Result<Vec<Bubble>> {
        let (w, h) = img.dimensions();
        let mut all: Vec<Bubble> = Vec::new();
        for (y0, y1) in slice_plan(w, h) {
            let part = if (y0, y1) == (0, h) { None } else { Some(image::imageops::crop_imm(img, 0, y0, w, y1 - y0).to_image()) };
            let found = self.detect_slice(part.as_ref().unwrap_or(img))?;
            for mut b in found {
                b.bounds[1] += y0;
                b.bounds[3] += y0;
                all.push(b);
            }
        }
        Ok(merge(all))
    }

    fn detect_slice(&mut self, img: &RgbImage) -> Result<Vec<Bubble>> {
        let (w, h) = img.dimensions();
        let scale = (INPUT as f32 / w as f32).min(INPUT as f32 / h as f32);
        let (nw, nh) = (((w as f32 * scale).round() as u32).clamp(1, INPUT as u32), ((h as f32 * scale).round() as u32).clamp(1, INPUT as u32));
        let (left, top) = ((INPUT as u32 - nw) / 2, (INPUT as u32 - nh) / 2);
        let small = image::imageops::resize(img, nw, nh, FilterType::Triangle);
        let plane = INPUT * INPUT;
        let mut x = vec![114.0f32 / 255.0; 3 * plane];
        for (px, py, p) in small.enumerate_pixels() {
            let i = (py + top) as usize * INPUT + (px + left) as usize;
            for c in 0..3 {
                x[c * plane + i] = p[c] as f32 / 255.0;
            }
        }
        let t = Tensor::from_array(([1usize, 3, INPUT, INPUT], x)).map_err(|e| anyhow!("{e}"))?;
        let name = self.session.inputs()[0].name().to_string();
        let outs = self.session.run(ort::inputs![name.as_str() => t]).map_err(|e| anyhow!("{e}"))?;
        let (s0, det) = outs[0].try_extract_tensor::<f32>().map_err(|e| anyhow!("{e}"))?;
        let (s1, proto) = outs[1].try_extract_tensor::<f32>().map_err(|e| anyhow!("{e}"))?;
        if s0.len() != 3 || s0[1] as usize != 5 + NMASK || s1.len() != 4 || s1[1] as usize != NMASK || s1[2] as usize != PROTO || s1[3] as usize != PROTO {
            bail!("beklenmeyen balon modeli çıktısı: {:?} {:?}", s0, s1);
        }
        let n = s0[2] as usize;
        // aday seçimi
        struct Cand {
            score: f32,
            bx: [f32; 4],
            idx: usize,
        }
        let mut cands: Vec<Cand> = Vec::new();
        for i in 0..n {
            let score = det[4 * n + i];
            if score < CONF {
                continue;
            }
            let (cx, cy, bw, bh) = (det[i], det[n + i], det[2 * n + i], det[3 * n + i]);
            cands.push(Cand { score, bx: [cx - bw / 2.0, cy - bh / 2.0, cx + bw / 2.0, cy + bh / 2.0], idx: i });
        }
        cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        let mut kept: Vec<Cand> = Vec::new();
        for c in cands {
            if kept.iter().all(|k| iou(&k.bx, &c.bx) <= NMS_IOU) {
                kept.push(c);
            }
        }
        let mut out = Vec::new();
        for c in kept {
            // kutu: giriş uzayından dilim piksellerine
            let to_img_x = |v: f32| ((v - left as f32) / scale).clamp(0.0, w as f32);
            let to_img_y = |v: f32| ((v - top as f32) / scale).clamp(0.0, h as f32);
            let (x0, y0) = (to_img_x(c.bx[0]).floor() as u32, to_img_y(c.bx[1]).floor() as u32);
            let (x1, y1) = (to_img_x(c.bx[2]).ceil() as u32, to_img_y(c.bx[3]).ceil() as u32);
            if x1 <= x0 + 1 || y1 <= y0 + 1 {
                continue;
            }
            // proto ızgarasında (256x256, giriş uzayının 1/4'ü) kutu çevresindeki mantık değerleri
            let ratio = INPUT as f32 / PROTO as f32;
            let (gx0, gy0) = (((c.bx[0] / ratio).floor() as i32 - 1).clamp(0, PROTO as i32 - 1), ((c.bx[1] / ratio).floor() as i32 - 1).clamp(0, PROTO as i32 - 1));
            let (gx1, gy1) = (((c.bx[2] / ratio).ceil() as i32 + 1).clamp(1, PROTO as i32), ((c.bx[3] / ratio).ceil() as i32 + 1).clamp(1, PROTO as i32));
            let (gw, gh) = ((gx1 - gx0).max(1) as usize, (gy1 - gy0).max(1) as usize);
            let coef: Vec<f32> = (0..NMASK).map(|k| det[(5 + k) * n + c.idx]).collect();
            let mut logit = vec![0f32; gw * gh];
            for k in 0..NMASK {
                let pk = &proto[k * PROTO * PROTO..(k + 1) * PROTO * PROTO];
                for gy in 0..gh {
                    let row = &pk[(gy0 as usize + gy) * PROTO + gx0 as usize..];
                    let o = &mut logit[gy * gw..(gy + 1) * gw];
                    for gx in 0..gw {
                        o[gx] += coef[k] * row[gx];
                    }
                }
            }
            let sample = |ix: f32, iy: f32| -> f32 {
                // ix, iy: giriş uzayı koordinatı -> proto hücre merkezleri
                let fx = (ix / ratio - 0.5 - gx0 as f32).clamp(0.0, (gw - 1) as f32);
                let fy = (iy / ratio - 0.5 - gy0 as f32).clamp(0.0, (gh - 1) as f32);
                let (xa, ya) = (fx.floor() as usize, fy.floor() as usize);
                let (xb, yb) = ((xa + 1).min(gw - 1), (ya + 1).min(gh - 1));
                let (tx, ty) = (fx - xa as f32, fy - ya as f32);
                let l = |x: usize, y: usize| logit[y * gw + x];
                (l(xa, ya) * (1.0 - tx) + l(xb, ya) * tx) * (1.0 - ty) + (l(xa, yb) * (1.0 - tx) + l(xb, yb) * tx) * ty
            };
            let (bw, bh) = (x1 - x0, y1 - y0);
            let mut mask = vec![0u8; (bw * bh) as usize];
            let mut any = false;
            for yy in 0..bh {
                let iy = (y0 + yy) as f32 * scale + top as f32 + 0.5 * scale;
                if iy < c.bx[1] || iy > c.bx[3] {
                    continue;
                }
                for xx in 0..bw {
                    let ix = (x0 + xx) as f32 * scale + left as f32 + 0.5 * scale;
                    if ix < c.bx[0] || ix > c.bx[2] {
                        continue;
                    }
                    if sample(ix, iy) > 0.0 {
                        mask[(yy * bw + xx) as usize] = 1;
                        any = true;
                    }
                }
            }
            if any {
                out.push(Bubble { bounds: [x0, y0, x1, y1], mask, score: c.score });
            }
        }
        Ok(out)
    }
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let (ix0, iy0, ix1, iy1) = (a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3]));
    let inter = (ix1 - ix0).max(0.0) * (iy1 - iy0).max(0.0);
    let ua = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter;
    if ua <= 0.0 { 0.0 } else { inter / ua }
}

/// Dilim aralıkları `(y0, y1)`: sayfa en/boy oranı ≤ 2 ise tek parça; daha uzunsa genişliğe göre seçilen yükseklikte, 1/4 örtüşmeli dilimler.
pub fn slice_plan(w: u32, h: u32) -> Vec<(u32, u32)> {
    if h as u64 <= 2 * w as u64 {
        return vec![(0, h)];
    }
    let sh = (w * 3 / 2).clamp(1024, 2048).min(h);
    let step = (sh - sh / 4).max(1);
    let mut v = Vec::new();
    let mut y = 0u32;
    loop {
        if y + sh >= h {
            v.push((h - sh, h));
            break;
        }
        v.push((y, y + sh));
        y += step;
    }
    v
}

/// Dilimlerden gelen tekrarları ayıkla: küçük kutunun %60'ından fazlası başka (daha büyük) balonun içindeyse eler.
fn merge(mut all: Vec<Bubble>) -> Vec<Bubble> {
    all.sort_by_key(|b| std::cmp::Reverse(b.w() as u64 * b.h() as u64));
    let mut kept: Vec<Bubble> = Vec::new();
    for b in all {
        let dup = kept.iter().any(|k| {
            let (ix0, iy0, ix1, iy1) = (k.bounds[0].max(b.bounds[0]), k.bounds[1].max(b.bounds[1]), k.bounds[2].min(b.bounds[2]), k.bounds[3].min(b.bounds[3]));
            if ix1 <= ix0 || iy1 <= iy0 {
                return false;
            }
            let inter = (ix1 - ix0) as f64 * (iy1 - iy0) as f64;
            inter / (b.w() as f64 * b.h() as f64) > 0.6
        });
        if !dup {
            kept.push(b);
        }
    }
    kept.sort_by_key(|b| (b.bounds[1], b.bounds[0]));
    kept
}

// ---------------- önbellek ----------------

const MAGIC: &[u8; 4] = b"SBB1";

fn encode(bubbles: &[Bubble]) -> Result<Vec<u8>> {
    let mut raw: Vec<u8> = Vec::new();
    raw.extend_from_slice(MAGIC);
    raw.extend_from_slice(&(bubbles.len() as u32).to_le_bytes());
    for b in bubbles {
        for v in b.bounds {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        raw.extend_from_slice(&b.score.to_le_bytes());
        raw.extend_from_slice(&b.mask);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw)?;
    Ok(z.finish()?)
}

fn decode(bytes: &[u8]) -> Result<Vec<Bubble>> {
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(bytes).read_to_end(&mut raw)?;
    if raw.len() < 8 || &raw[..4] != MAGIC {
        bail!("balon önbelleği geçersiz");
    }
    let u32_at = |o: usize| -> Result<u32> { raw.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| anyhow!("balon önbelleği kısa")) };
    let n = u32_at(4)? as usize;
    let mut o = 8;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let bounds = [u32_at(o)?, u32_at(o + 4)?, u32_at(o + 8)?, u32_at(o + 12)?];
        let score = f32::from_bits(u32_at(o + 16)?);
        o += 20;
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
            bail!("balon önbelleği bozuk");
        }
        let len = ((bounds[2] - bounds[0]) as usize) * ((bounds[3] - bounds[1]) as usize);
        let mask = raw.get(o..o + len).ok_or_else(|| anyhow!("balon önbelleği kısa"))?.to_vec();
        o += len;
        out.push(Bubble { bounds, mask, score });
    }
    Ok(out)
}

pub fn load_cached(paths: &AppPaths, sha: &str) -> Option<Vec<Bubble>> {
    let b = std::fs::read(paths.bubble_file(sha)).ok()?;
    decode(&b).ok()
}

pub fn save_cache(paths: &AppPaths, sha: &str, bubbles: &[Bubble]) {
    if let Ok(b) = encode(bubbles) {
        let _ = std::fs::create_dir_all(paths.bubbles());
        let _ = crate::paths::atomic_write(&paths.bubble_file(sha), &b);
    }
}

/// Önbellekte yoksa algılayıp önbelleğe yazar.
pub fn detect_cached(paths: &AppPaths, sha: &str, det: &mut BubbleDetector, img: &RgbImage) -> Result<Vec<Bubble>> {
    if let Some(b) = load_cached(paths, sha) {
        if b.iter().all(|b| b.bounds[2] <= img.width() && b.bounds[3] <= img.height()) {
            return Ok(b);
        }
    }
    let b = det.detect(img)?;
    save_cache(paths, sha, &b);
    Ok(b)
}

/// Sayfa başına balonları verir: önce önbellek, yoksa tembel yüklenen algılayıcı. Model yok/hatalıysa `None` (halka kuralına düşülür).
pub struct BubbleSource {
    model: Option<std::path::PathBuf>,
    det: Option<BubbleDetector>,
    failed: bool,
}

impl BubbleSource {
    pub fn new(model: Option<std::path::PathBuf>) -> Self {
        let off = std::env::var("SAFINK_BUBBLES").map(|v| v == "0").unwrap_or(false);
        Self { model: if off { None } else { model }, det: None, failed: false }
    }
    pub fn get(&mut self, paths: &AppPaths, sha: &str, img: &RgbImage) -> Option<Vec<Bubble>> {
        self.model.as_ref()?;
        if let Some(b) = load_cached(paths, sha) {
            if b.iter().all(|b| b.bounds[2] <= img.width() && b.bounds[3] <= img.height()) {
                return Some(b);
            }
        }
        if self.failed {
            return None;
        }
        if self.det.is_none() {
            match BubbleDetector::new(self.model.as_ref().unwrap()) {
                Ok(d) => self.det = Some(d),
                Err(e) => {
                    tracing::warn!("balon modeli yüklenemedi, halka kuralı kullanılacak: {e}");
                    self.failed = true;
                    return None;
                }
            }
        }
        match detect_cached(paths, sha, self.det.as_mut().unwrap(), img) {
            Ok(b) => Some(b),
            Err(e) => {
                tracing::warn!("balon algılama başarısız, halka kuralı kullanılacak: {e}");
                None
            }
        }
    }
}

/// Algılanan balonları kırmızıyla işaretleyen önizleme (araçlar için).
pub fn overlay(img: &RgbImage, bubbles: &[Bubble]) -> RgbImage {
    let mut o = img.clone();
    for b in bubbles {
        for y in 0..b.h() {
            for x in 0..b.w() {
                if b.mask[(y * b.w() + x) as usize] != 0 {
                    let p = o.get_pixel_mut(b.bounds[0] + x, b.bounds[1] + y);
                    *p = Rgb([(p[0] as u16 / 2 + 110) as u8, p[1] / 2, p[2] / 2]);
                }
            }
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_cover_tall_strip_with_overlap() {
        assert_eq!(slice_plan(1115, 1600), vec![(0, 1600)]);
        let p = slice_plan(800, 20_000);
        assert_eq!(p[0].0, 0);
        assert_eq!(p.last().unwrap().1, 20_000);
        for w in p.windows(2) {
            assert!(w[1].0 < w[0].1, "dilimler örtüşmeli");
            assert!(w[1].0 > w[0].0);
        }
        assert!(p.iter().all(|s| s.1 - s.0 == 1200));
    }

    #[test]
    fn cache_roundtrip_and_slice_merge() {
        let a = Bubble::from_fn([10, 20, 60, 80], |x, y| (x as i32 - 35).pow(2) + (y as i32 - 50).pow(2) < 600);
        let b = Bubble::from_fn([12, 30, 40, 70], |_, _| true); // a'nın içinde kalan kesik kopya
        let m = merge(vec![b.clone(), a.clone()]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].bounds, a.bounds);
        let back = decode(&encode(&m).unwrap()).unwrap();
        assert_eq!(back[0].mask, a.mask);
        assert_eq!(back[0].bounds, a.bounds);
        assert!(back[0].contains(35, 50) && !back[0].contains(10, 20));
    }
}
