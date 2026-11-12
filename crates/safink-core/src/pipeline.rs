//! Sayfa temizleme hattı (Koharu gibi): genişlet -> (düz zemin doldur) -> kalan maske bileşenleri karolara bölünür; her karo yerel
//! çözünürlükte (çekirdek <= 512, + 128 px bağlam) modele gider ve yalnızca maskeli pikseller sayfaya yazılır (LaMa, Flux aynı yol).
use crate::api::FlatPart;
use crate::inpaint::{Ctx, Inpainter, TileSpec};
use crate::maskops;
use crate::postproc;
use anyhow::{bail, Result};
use image::{GrayImage, RgbImage};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug)]
pub struct CleanParams {
    pub dilation: u32,
    pub feather: u32,
    /// Yazı çevresindeki halka düz renkse modele gitmeden ortanca renkle doldur.
    pub flat_fill: bool,
}

/// Düz doldurma ayarları. Halka: bileşenin 2..=6 px dışındaki bant (hiçbir maskeye ait olmayan pikseller).
#[derive(Clone, Copy, Debug)]
pub struct FlatCfg {
    pub ring_in: u32,
    pub ring_out: u32,
    pub min_ring: usize,
    /// Ortancadan bu kadar (herhangi bir kanalda) uzak piksel "aykırı" (balon çizgisi, sanat) sayılır.
    pub outlier_dev: i32,
    /// Aykırı oranı bunu aşarsa düz değil.
    pub max_outlier_frac: f32,
    /// Aykırılar atıldıktan sonra kanal başına en büyük standart sapma (0-255).
    pub max_std: f32,
}
pub const FLAT_CFG: FlatCfg = FlatCfg { ring_in: 2, ring_out: 6, min_ring: 40, outlier_dev: 40, max_outlier_frac: 0.12, max_std: 8.0 };

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlatStats {
    pub flat: usize,
    pub model: usize,
}

pub struct FlatOut {
    /// Düz doldurulmuş sayfa.
    pub work: RgbImage,
    /// Modele kalan (genişletilmiş) maske: düz doldurulmayan bileşenler.
    pub remaining: GrayImage,
    pub stats: FlatStats,
}

/// Bileşen halkası düz mü? Düzse ortanca rengi döner.
fn ring_flat(img: &RgbImage, ring: &[(u32, u32)], c: &FlatCfg) -> Option<[u8; 3]> {
    if ring.len() < c.min_ring {
        return None;
    }
    let mut med = [0i32; 3];
    for ch in 0..3 {
        let mut v: Vec<u8> = ring.iter().map(|&(x, y)| img.get_pixel(x, y)[ch]).collect();
        v.sort_unstable();
        med[ch] = v[v.len() / 2] as i32;
    }
    let mut inl: Vec<[i32; 3]> = Vec::with_capacity(ring.len());
    for &(x, y) in ring {
        let p = img.get_pixel(x, y);
        let q = [p[0] as i32, p[1] as i32, p[2] as i32];
        if (0..3).all(|ch| (q[ch] - med[ch]).abs() <= c.outlier_dev) {
            inl.push(q);
        }
    }
    let out_frac = 1.0 - inl.len() as f32 / ring.len() as f32;
    if out_frac > c.max_outlier_frac || inl.len() < c.min_ring / 2 {
        return None;
    }
    for ch in 0..3 {
        let mean = inl.iter().map(|q| q[ch] as f32).sum::<f32>() / inl.len() as f32;
        let var = inl.iter().map(|q| (q[ch] as f32 - mean).powi(2)).sum::<f32>() / inl.len() as f32;
        if var.sqrt() > c.max_std {
            return None;
        }
    }
    Some([med[0] as u8, med[1] as u8, med[2] as u8])
}

/// Halkadaki aykırı piksellerin (zemin renginden uzak) kaç ayrı öbek oluşturduğu (8-komşuluk, ≥3 px).
fn ring_outlier_groups(img: &RgbImage, ring: &[(u32, u32)], col: [u8; 3], crop: (i32, i32, u32, u32), c: &FlatCfg) -> usize {
    let (x0, y0, cw, ch) = crop;
    let (cw, ch) = (cw as i32, ch as i32);
    let mut grid = vec![false; (cw * ch) as usize];
    for &(x, y) in ring {
        let p = img.get_pixel(x, y);
        if (0..3).any(|k| (p[k] as i32 - col[k] as i32).abs() > c.outlier_dev) {
            grid[((y as i32 - y0) * cw + (x as i32 - x0)) as usize] = true;
        }
    }
    let mut groups = 0;
    let mut stack = Vec::new();
    for start in 0..grid.len() {
        if !grid[start] {
            continue;
        }
        grid[start] = false;
        stack.push(start as i32);
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = (i % cw, i / cw);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < cw && ny < ch {
                        let j = (ny * cw + nx) as usize;
                        if grid[j] {
                            grid[j] = false;
                            stack.push(j as i32);
                        }
                    }
                }
            }
        }
        if size >= 3 {
            groups += 1;
        }
    }
    groups
}

/// Eski halka kuralı (sunucu verisi yokken): bileşenin 2..=6 px dışındaki halka düz renkse ortanca renk, değilse `None`.
fn ring_decision(img: &RgbImage, labels: &[u32], l: usize, b: (i32, i32, i32, i32), pad: i32, mraw: &[u8], c: &FlatCfg) -> Option<[u8; 3]> {
    let (w, h) = (img.width() as i32, img.height() as i32);
        let (x0, y0, x1, y1) = ((b.0 - pad).max(0), (b.1 - pad).max(0), (b.2 + pad + 1).min(w), (b.3 + pad + 1).min(h));
        let (cw, ch) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let mut cm = GrayImage::new(cw, ch);
        for y in y0..y1 {
            for x in x0..x1 {
                if labels[(y * w + x) as usize] as usize == l {
                    cm.put_pixel((x - x0) as u32, (y - y0) as u32, image::Luma([255]));
                }
            }
        }
        let outer = maskops::dilate(&cm, c.ring_out);
        let inner = maskops::dilate(&cm, c.ring_in);
        let mut ring = Vec::new();
        for y in 0..ch {
            for x in 0..cw {
                if outer.get_pixel(x, y)[0] >= 128 && inner.get_pixel(x, y)[0] < 128 {
                    let (gx, gy) = (x as i32 + x0, y as i32 + y0);
                    if mraw[(gy * w + gx) as usize] < 128 {
                        ring.push((gx as u32, gy as u32));
                    }
                }
            }
        }
        let col = ring_flat(img, &ring, c)?;
        // Halkadaki aykırı pikseller ayrı ayrı yerlerde çıkıyorsa yazının altından bir şey geçiyordur
        // (çizgi, tram şeridi, sanat): düz dolgu onu silerdi, bileşen modele gider. Balon çizgisine
        // değen yazıda aykırılar tek bir yayda toplanır.
        if ring_outlier_groups(img, &ring, col, (x0, y0, cw, ch), c) >= 2 {
            return None;
        }
        Some(col)
}

/// Düz zemin doldurma (BallonsTranslator gibi): her bileşenin halkası düzse bileşen ortanca renkle (tüylü) doldurulur.
/// `m`: genişletilmiş maske. Düz olmayan bileşenler `remaining` içinde kalır.
pub fn flat_fill(img: &RgbImage, m: &GrayImage, feather: u32, c: &FlatCfg) -> FlatOut {
    flat_fill_impl(img, m, feather, c, None)
}

/// `parts` verilirse (sunucu sınıflandırması) karar `server_flat_color` ile, yoksa halka kuralıyla verilir; boyama aynıdır.
fn flat_fill_impl(img: &RgbImage, m: &GrayImage, feather: u32, c: &FlatCfg, parts: Option<&[FlatPart]>) -> FlatOut {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (labels, spans) = label_components(m);
    let n = spans.len().saturating_sub(1);
    let mut work = img.clone();
    let mut remaining = m.clone();
    let mut stats = FlatStats::default();
    if n == 0 {
        return FlatOut { work, remaining, stats };
    }
    // bileşen sınır kutuları
    let mut bb = vec![(i32::MAX, i32::MAX, -1i32, -1i32); n + 1];
    for y in 0..h {
        for x in 0..w {
            let l = labels[(y * w + x) as usize] as usize;
            if l != 0 {
                let b = &mut bb[l];
                b.0 = b.0.min(x);
                b.1 = b.1.min(y);
                b.2 = b.2.max(x);
                b.3 = b.3.max(y);
            }
        }
    }
    let pad = (c.ring_out + 2) as i32;
    let fpad = (feather as f32 * 1.2).ceil() as i32 * 3 + 2;
    let mraw = m.as_raw();
    for l in 1..=n {
        let b = bb[l];
        let col = match parts {
            Some(p) => server_flat_color(img, m, [b.0 as u32, b.1 as u32, b.2 as u32 + 1, b.3 as u32 + 1], p),
            None => ring_decision(img, &labels, l, b, pad, mraw, c),
        };
        let Some(col) = col else {
            stats.model += 1;
            continue;
        };
        stats.flat += 1;
        // tüylü dolgu (bileşen kutusu + tüy payı)
        let (fx0, fy0, fx1, fy1) = ((b.0 - fpad).max(0), (b.1 - fpad).max(0), (b.2 + fpad + 1).min(w), (b.3 + fpad + 1).min(h));
        let (fw, fh) = ((fx1 - fx0) as u32, (fy1 - fy0) as u32);
        let mut fm = GrayImage::new(fw, fh);
        for y in fy0..fy1 {
            for x in fx0..fx1 {
                if labels[(y * w + x) as usize] as usize == l {
                    fm.put_pixel((x - fx0) as u32, (y - fy0) as u32, image::Luma([255]));
                }
            }
        }
        let alpha = maskops::feather_alpha(&fm, feather as f32);
        for y in 0..fh {
            for x in 0..fw {
                let a = alpha[(y * fw + x) as usize];
                if a <= 0.004 {
                    continue;
                }
                let (gx, gy) = (x as i32 + fx0, y as i32 + fy0);
                let gl = labels[(gy * w + gx) as usize] as usize;
                if gl != 0 && gl != l {
                    continue; // başka bileşenin pikseli: dokunma
                }
                let o = *work.get_pixel(gx as u32, gy as u32);
                // tüy payında zemine benzemeyen (balon çizgisi, sanat) pikseller bozulmaz
                if gl == 0 && (0..3).any(|k| (o[k] as i32 - col[k] as i32).abs() > c.outlier_dev) {
                    continue;
                }
                let mut px = o;
                for k in 0..3 {
                    px[k] = (o[k] as f32 * (1.0 - a) + col[k] as f32 * a).round().clamp(0.0, 255.0) as u8;
                }
                work.put_pixel(gx as u32, gy as u32, px);
                if gl == l {
                    remaining.put_pixel(gx as u32, gy as u32, image::Luma([0]));
                }
            }
        }
    }
    FlatOut { work, remaining, stats }
}

/// Sunucu sınıflandırmalı düz doldurma kuralı (bileşen = genişletilmiş maskenin 8-bağlantılı bir bileşeni):
///  (a) sınır kutusu bileşenin kutusuyla kesişen (pozitif alanlı) en az bir sunucu parçası olmalı ve hepsinin `p_flat >= FLAT_P_MIN` olmalı;
///  (b) piksel güvenlik denetimi: "yazı dışı" pikseller = bileşen kutusunun `SAFE_PAD` px genişletilmişi içinde, genişletilmiş maskenin
///      (tüm bileşenler, `m`) `SAFE_HALO` px dışında kalanlar. Bunların (>= `SAFE_MIN_PIXELS`) kanal ortancası hepsinde >= `SAFE_WHITE` (beyaz) ya da
///      hepsinde <= `SAFE_BLACK` (siyah) olmalı ve herhangi bir kanalda ortancadan `SAFE_DEV` fazla sapanlar `SAFE_DEV_FRAC` oranının altında kalmalı.
/// Geçerse dolgu rengi saf beyaz (255) ya da saf siyah (0); geçmezse `None` (bileşen modele gider).
pub const FLAT_P_MIN: f32 = 0.80;
pub const SAFE_PAD: u32 = 8;
pub const SAFE_HALO: u32 = 2;
pub const SAFE_WHITE: u8 = 245;
pub const SAFE_BLACK: u8 = 10;
pub const SAFE_DEV: i32 = 30;
pub const SAFE_DEV_FRAC: f64 = 0.25;
pub const SAFE_MIN_PIXELS: usize = 16;

fn median_u8(v: &mut [u8]) -> u8 {
    v.sort_unstable();
    let mid = v.len() / 2;
    if v.len() % 2 == 0 { ((v[mid - 1] as u16 + v[mid] as u16) / 2) as u8 } else { v[mid] }
}

/// `bbox`: `[x0, y0, x1, y1)` (x1/y1 dışta).
pub fn server_flat_color(img: &RgbImage, m: &GrayImage, bbox: [u32; 4], parts: &[FlatPart]) -> Option<[u8; 3]> {
    let hit: Vec<&FlatPart> = parts.iter().filter(|p| p.x0 < bbox[2] && bbox[0] < p.x1 && p.y0 < bbox[3] && bbox[1] < p.y1).collect();
    if hit.is_empty() || hit.iter().any(|p| p.p_flat < FLAT_P_MIN) {
        return None;
    }
    let (w, h) = img.dimensions();
    let crop_pad = SAFE_PAD + SAFE_HALO + 2;
    let (cx0, cy0) = (bbox[0].saturating_sub(crop_pad), bbox[1].saturating_sub(crop_pad));
    let (cx1, cy1) = ((bbox[2] + crop_pad).min(w), (bbox[3] + crop_pad).min(h));
    let (cw, ch) = (cx1 - cx0, cy1 - cy0);
    let mut cm = GrayImage::new(cw, ch);
    for y in 0..ch {
        for x in 0..cw {
            cm.put_pixel(x, y, *m.get_pixel(x + cx0, y + cy0));
        }
    }
    let near = maskops::dilate(&cm, SAFE_HALO);
    let (rx0, ry0) = (bbox[0].saturating_sub(SAFE_PAD), bbox[1].saturating_sub(SAFE_PAD));
    let (rx1, ry1) = ((bbox[2] + SAFE_PAD).min(w), (bbox[3] + SAFE_PAD).min(h));
    let mut px: Vec<[u8; 3]> = Vec::new();
    for y in ry0..ry1 {
        for x in rx0..rx1 {
            if near.get_pixel(x - cx0, y - cy0)[0] < 128 {
                px.push(img.get_pixel(x, y).0);
            }
        }
    }
    if px.len() < SAFE_MIN_PIXELS {
        return None;
    }
    let mut med = [0u8; 3];
    for k in 0..3 {
        let mut v: Vec<u8> = px.iter().map(|p| p[k]).collect();
        med[k] = median_u8(&mut v);
    }
    let col = if med.iter().all(|&v| v >= SAFE_WHITE) {
        255u8
    } else if med.iter().all(|&v| v <= SAFE_BLACK) {
        0u8
    } else {
        return None;
    };
    let dev = px.iter().filter(|p| (0..3).any(|k| (p[k] as i32 - med[k] as i32).abs() > SAFE_DEV)).count();
    if dev as f64 >= SAFE_DEV_FRAC * px.len() as f64 {
        return None;
    }
    Some([col; 3])
}

/// Düz doldurma geçişi: sunucu sınıflandırması (`Some`) varsa yukarıdaki kural, yoksa (eski önbellek, sunucu sınıflandırıcısı kapalı) halka kuralı.
pub fn flat_pass(img: &RgbImage, m: &GrayImage, feather: u32, parts: Option<&[FlatPart]>) -> FlatOut {
    match parts {
        Some(p) => flat_fill_impl(img, m, feather, &FLAT_CFG, Some(p)),
        None => flat_fill(img, m, feather, &FLAT_CFG),
    }
}

/// Tüm sayfayı temizler. `mask`: son (dilate edilmemiş) maske, 0/255.
pub fn clean_image(
    img: &RgbImage,
    mask: &GrayImage,
    p: &CleanParams,
    parts: Option<&[FlatPart]>,
    inp: &mut dyn Inpainter,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, usize),
) -> Result<RgbImage> {
    let (iw, ih) = img.dimensions();
    if mask.dimensions() != (iw, ih) {
        bail!("maske boyutu görüntüyle uyuşmuyor");
    }
    let m = maskops::dilate(mask, p.dilation);
    if p.flat_fill {
        let f = flat_pass(img, &m, p.feather, parts);
        if f.stats.model == 0 {
            return Ok(f.work);
        }
        return clean_dilated(&f.work, f.remaining, p, inp, cancel, progress);
    }
    clean_dilated(img, m, p, inp, cancel, progress)
}

/// Koharu karo planı (koharu-pipeline stages/inpainting.rs `inpaint_tiles`): bir karo = bir ya da daha çok maske bileşeni.
/// `core`: karonun sahip olduğu bölge `[x0, y0, x1, y1)`; `crop`: çekirdek + bağlam (sayfaya sıkıştırılmış), modele giden kırpıntı.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub components: Vec<u32>,
    pub core: [u32; 4],
    pub crop: [u32; 4],
}

fn union_bounds(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}
fn bounds_area(b: [u32; 4]) -> u64 { (b[2] - b[0]) as u64 * (b[3] - b[1]) as u64 }
fn expand_bounds(b: [u32; 4], ctx: u32, w: u32, h: u32) -> [u32; 4] {
    [b[0].saturating_sub(ctx), b[1].saturating_sub(ctx), b[2].saturating_add(ctx).min(w), b[3].saturating_add(ctx).min(h)]
}

/// Maskeyi (≥128) 8-bağlantılı bileşenlere ayırır ve Koharu gibi karolara böler: çekirdeği ≤ `spec.core`×`spec.core` olan bileşenler,
/// birleşim sınır kutusu bu sınırı aşmıyorsa en az büyüyen karoya gruplanır; daha büyük bileşenler `core`'luk çekirdeklere bölünür.
/// Her karonun kırpıntısı çekirdek + `spec.context` px (sayfaya sıkıştırılmış). Döner: piksel başına bileşen etiketi (0 = yok) ve karolar.
pub fn plan_tiles(mask: &GrayImage, spec: &TileSpec) -> (Vec<u32>, Vec<Tile>) {
    let (w, h) = mask.dimensions();
    let (labels, spans) = label_components(mask);
    let n = spans.len().saturating_sub(1);
    let mut bb = vec![[u32::MAX, u32::MAX, 0u32, 0u32]; n + 1];
    for y in 0..h {
        for x in 0..w {
            let l = labels[(y * w + x) as usize] as usize;
            if l != 0 {
                let b = &mut bb[l];
                *b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
            }
        }
    }
    let (size, ctx) = (spec.core.max(1), spec.context);
    let mut bounded: Vec<Tile> = Vec::new();
    let mut split: Vec<Tile> = Vec::new();
    for l in 1..=n {
        let b = bb[l];
        if b[2] - b[0] <= size && b[3] - b[1] <= size {
            let best = bounded
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    let u = union_bounds(t.core, b);
                    ((u[2] - u[0] <= size) && (u[3] - u[1] <= size)).then_some((bounds_area(u) - bounds_area(t.core), i, u))
                })
                .min_by_key(|&(g, i, _)| (g, i));
            if let Some((_, i, u)) = best {
                let t = &mut bounded[i];
                t.components.push(l as u32);
                t.core = u;
                t.crop = expand_bounds(u, ctx, w, h);
            } else {
                bounded.push(Tile { components: vec![l as u32], core: b, crop: expand_bounds(b, ctx, w, h) });
            }
            continue;
        }
        let mut ct = b[1];
        while ct < b[3] {
            let cb = ct.saturating_add(size).min(b[3]);
            let mut cl = b[0];
            while cl < b[2] {
                let cr = cl.saturating_add(size).min(b[2]);
                let core = [cl, ct, cr, cb];
                // çekirdeğin içinde bu bileşenin kapladığı kutu
                let (mut x0, mut y0, mut x1, mut y1) = (cr, cb, 0u32, 0u32);
                for y in ct..cb {
                    for x in cl..cr {
                        if labels[(y * w + x) as usize] == l as u32 {
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x + 1);
                            y1 = y1.max(y + 1);
                        }
                    }
                }
                if x1 > x0 && y1 > y0 {
                    split.push(Tile { components: vec![l as u32], core, crop: expand_bounds([x0, y0, x1, y1], ctx, w, h) });
                }
                cl = cr;
            }
            ct = cb;
        }
    }
    bounded.append(&mut split);
    for t in bounded.iter_mut() {
        t.components.sort_unstable();
    }
    bounded.sort_by_key(|t| (t.core[1], t.core[0], t.core[3], t.core[2]));
    (labels, bounded)
}

/// Genişletilmiş maskeyi (`m`) seçilen modelle temizler. Redraw (difüzyon) modelleri MangaTranslator bölge yolundan geçer
/// (`clean_regions_mt`). LaMa Koharu gibi karo karo çalışır: her karo yerel çözünürlükte modele gider (yalnızca model boyut katına
/// dolgu), çıktının yalnızca karonun maske bileşenlerinden çekirdek içine düşen pikselleri sayfaya sert yapıştırılır (Koharu ton
/// eşleme/tüy yapmaz); maske dışı hiçbir piksel değişmez. `_p.feather` yalnızca düz doldurmada kullanılır.
pub fn clean_dilated(
    img: &RgbImage,
    m: GrayImage,
    _p: &CleanParams,
    inp: &mut dyn Inpainter,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, usize),
) -> Result<RgbImage> {
    let spec = inp.tile_spec();
    if spec.mt_regions {
        return clean_regions_mt(img, m, inp, cancel, progress);
    }
    let (iw, _ih) = img.dimensions();
    let (labels, tiles) = plan_tiles(&m, &spec);
    let mut work = img.clone();
    let mut pending = m;
    let total = tiles.len();
    progress(0, total);
    for (i, t) in tiles.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            bail!("iptal edildi");
        }
        let [cx0, cy0, cx1, cy1] = t.crop;
        let (cw, ch) = (cx1 - cx0, cy1 - cy0);
        let crop = image::imageops::crop_imm(&work, cx0, cy0, cw, ch).to_image();
        // kırpıntı maskesi: hâlâ bekleyen tüm maske (önceki karoların yazdığı pikseller artık temiz bağlamdır)
        let mut cm = GrayImage::new(cw, ch);
        for y in 0..ch {
            for x in 0..cw {
                if pending.get_pixel(cx0 + x, cy0 + y)[0] >= 128 {
                    cm.put_pixel(x, y, image::Luma([255]));
                }
            }
        }
        let out = inp.inpaint(&crop, &cm, &Ctx { cancel })?;
        if out.dimensions() != (cw, ch) {
            bail!("model beklenmeyen boyut döndürdü: {:?} (beklenen {}x{})", out.dimensions(), cw, ch);
        }
        // karonun sahip olduğu pikseller: çekirdek içinde, karonun bileşenlerinden
        let [kx0, ky0, kx1, ky1] = t.core;
        let mut own = vec![false; (cw * ch) as usize];
        for y in ky0..ky1 {
            for x in kx0..kx1 {
                let l = labels[(y * iw + x) as usize];
                if l != 0 && t.components.binary_search(&l).is_ok() {
                    let k = ((y - cy0) * cw + (x - cx0)) as usize;
                    own[k] = true;
                }
            }
        }
        for y in 0..ch {
            for x in 0..cw {
                if own[(y * cw + x) as usize] {
                    work.put_pixel(cx0 + x, cy0 + y, *out.get_pixel(x, y));
                    pending.put_pixel(cx0 + x, cy0 + y, image::Luma([0]));
                }
            }
        }
        progress(i + 1, total);
    }
    Ok(work)
}

/// `align_to` düzeltmesinin uygulandığı kırpıntı sayısı (ölçüm/tanı sayacı).
pub static ALIGN_FIRED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// MangaTranslator bölge yolu (Klein): genişletilmiş (düz doldurma sonrası kalan) maskenin her 8-bağlantılı bileşeni için sırayla
/// (sayfa sırası: üstten alta, soldan sağa), o anki çalışma görüntüsünden bir kırpıntı alınır (`mtcrop::plan_crop`); model ham kırpıntıyı
/// ~1 MP'de işler (ölçekleme `Inpainter::inpaint` içinde); sonra `align_to` (güvenlik ağı), `_match_luminance`, bileşen kutusuna kırpılmış
/// sert maskeyle yapıştırma ve glif kompoziti. Bileşen maskesi dışındaki hiçbir piksel değişmez.
pub fn clean_regions_mt(img: &RgbImage, m: GrayImage, inp: &mut dyn Inpainter, cancel: &AtomicBool, progress: &dyn Fn(usize, usize)) -> Result<RgbImage> {
    clean_regions_mt_dbg(img, m, inp, cancel, progress, None)
}

/// Tanı: bir bölgenin her işleme adımındaki kırpıntı boyutlu görüntüleri (`clean_regions_mt_dbg`).
pub struct RegionDebug {
    pub index: usize,
    /// bileşen kutusu [x0, y0, x1, y1] (dahil) ve kırpıntı kutusu (x, y, w, h), sayfa koordinatı
    pub bbox: [u32; 4],
    pub crop_rect: [u32; 4],
    pub crop: RgbImage,
    /// bölge maskesi (modele verilen maske)
    pub mask: GrayImage,
    /// yapıştırma maskesi (`composite_mask`)
    pub own: GrayImage,
    /// ham model çıktısı (kırpıntı boyutuna ölçeklenmiş, son işlem yok)
    pub raw: RgbImage,
    pub aligned: RgbImage,
    pub lum: RgbImage,
    /// glif kompoziti sonrası (sayfaya yazılan)
    pub patch: RgbImage,
}

pub fn clean_regions_mt_dbg(
    img: &RgbImage,
    m: GrayImage,
    inp: &mut dyn Inpainter,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, usize),
    mut dbg: Option<&mut dyn FnMut(RegionDebug)>,
) -> Result<RgbImage> {
    let (iw, ih) = img.dimensions();
    let (labels, spans) = label_components(&m);
    let n = spans.len().saturating_sub(1);
    let mut bb = vec![[u32::MAX, u32::MAX, 0u32, 0u32]; n + 1]; // [x_min, y_min, x_max, y_max] dahil
    for y in 0..ih {
        for x in 0..iw {
            let l = labels[(y * iw + x) as usize] as usize;
            if l != 0 {
                let b = &mut bb[l];
                b[0] = b[0].min(x);
                b[1] = b[1].min(y);
                b[2] = b[2].max(x);
                b[3] = b[3].max(y);
            }
        }
    }
    let mut order: Vec<usize> = (1..=n).collect();
    order.sort_by_key(|&l| (bb[l][1], bb[l][0], bb[l][3], bb[l][2]));
    let radius = postproc::glyph_radius(iw, ih);
    let mult = inp.tile_spec().multiple;
    let mut work = img.clone();
    progress(0, order.len());
    for (k, &l) in order.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            bail!("iptal edildi");
        }
        let b = bb[l];
        if let Some(plan) = crate::mtcrop::plan_crop(b, iw, ih, mult) {
            let (cw, ch) = (plan.w, plan.h);
            let crop = image::imageops::crop_imm(&work, plan.x, plan.y, cw, ch).to_image();
            // bölge maskesi: yalnızca bu bileşen (MT: bölge başına maske)
            let mut cm = GrayImage::new(cw, ch);
            let mut own = vec![false; (cw * ch) as usize];
            for y in 0..ch {
                for x in 0..cw {
                    if labels[((plan.y + y) * iw + plan.x + x) as usize] as usize == l {
                        cm.put_pixel(x, y, image::Luma([255]));
                    }
                }
            }
            let clip = [b[0], b[1], b[2] + 1, b[3] + 1];
            for (i, v) in crate::mtcrop::composite_mask(&cm, &plan, clip).into_iter().enumerate() {
                own[i] = v;
            }
            let out = inp.inpaint(&crop, &cm, &Ctx { cancel })?;
            if out.dimensions() != (cw, ch) {
                bail!("model beklenmeyen boyut döndürdü: {:?} (beklenen {}x{})", out.dimensions(), cw, ch);
            }
            let aligned = align_to(&crop, out.clone(), &cm);
            if aligned.as_raw() != out.as_raw() {
                ALIGN_FIRED.fetch_add(1, Ordering::Relaxed);
            }
            let lum = postproc::match_luminance(&crop, &aligned, &cm);
            let lab: Vec<u32> = own.iter().map(|&o| o as u32).collect();
            let patch = if postproc::Post::from_env() == postproc::Post::Full { postproc::composite_glyphs(&crop, &lum, &cm, &own, &lab, radius) } else { lum.clone() };
            for y in 0..ch {
                for x in 0..cw {
                    if own[(y * cw + x) as usize] {
                        work.put_pixel(plan.x + x, plan.y + y, *patch.get_pixel(x, y));
                    }
                }
            }
            if let Some(f) = dbg.as_mut() {
                let own_img = GrayImage::from_fn(cw, ch, |x, y| image::Luma([if own[(y * cw + x) as usize] { 255 } else { 0 }]));
                f(RegionDebug { index: k, bbox: b, crop_rect: [plan.x, plan.y, cw, ch], crop, mask: cm, own: own_img, raw: out, aligned, lum, patch });
            }
        }
        progress(k + 1, order.len());
    }
    Ok(work)
}

/// Model çıktısını girdiye hizalar: maske dışındaki (maskeden ≥6 px uzak) piksellerde ±8 px içinde en düşük ortalama farkı veren
/// tam sayı kaymayı bulur ve çıktıyı o kadar geri kaydırır (kenarlar tekrarlanır). Kayma yoksa ya da kazanç %5'ten azsa çıktı aynen döner.
pub fn align_to(src: &RgbImage, out: RgbImage, mask: &GrayImage) -> RgbImage {
    const R: i32 = 8;
    let (w, h) = src.dimensions();
    if w <= (2 * R + 4) as u32 || h <= (2 * R + 4) as u32 {
        return out;
    }
    let far = maskops::dilate(mask, 6);
    let lum = |p: &image::Rgb<u8>| (p[0] as i32 * 77 + p[1] as i32 * 150 + p[2] as i32 * 29) >> 8;
    // hız için örnek noktalar: maske dışı, kenardan R uzak, 2 px adım
    let mut pts: Vec<(i32, i32, i32)> = Vec::new();
    for y in (R..h as i32 - R).step_by(2) {
        for x in (R..w as i32 - R).step_by(2) {
            if far.get_pixel(x as u32, y as u32)[0] < 128 {
                pts.push((x, y, lum(src.get_pixel(x as u32, y as u32))));
            }
        }
    }
    if pts.len() < 200 {
        return out;
    }
    let ol: Vec<i32> = out.pixels().map(lum).collect();
    let err = |dx: i32, dy: i32| -> u64 { pts.iter().map(|&(x, y, v)| (ol[((y + dy) as u32 * w + (x + dx) as u32) as usize] - v).unsigned_abs() as u64).sum() };
    let base = err(0, 0);
    let mut best = (base, 0, 0);
    for dy in -R..=R {
        for dx in -R..=R {
            let e = err(dx, dy);
            if e < best.0 {
                best = (e, dx, dy);
            }
        }
    }
    let (e, dx, dy) = best;
    if (dx, dy) == (0, 0) || e as f64 > base as f64 * 0.95 {
        return out;
    }
    // çıktıdaki içerik girdide (x, y)'de olanı (x+dx, y+dy)'de çizmiş: geri al
    RgbImage::from_fn(w, h, |x, y| {
        let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
        let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
        *out.get_pixel(sx, sy)
    })
}

/// 8-bağlantılı bileşen etiketleri (0 = arka plan, 1..=n) ve her etiketin (y0, y1) aralığı.
fn label_components(mask: &GrayImage) -> (Vec<u32>, Vec<(i32, i32)>) {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let src = mask.as_raw();
    let mut lab = vec![0u32; w * h];
    let mut ys: Vec<(i32, i32)> = vec![(0, 0)];
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..w * h {
        if src[start] < 128 || lab[start] != 0 {
            continue;
        }
        let id = ys.len() as u32;
        lab[start] = id;
        stack.push(start);
        let (mut y0, mut y1) = (i32::MAX, 0i32);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            y0 = y0.min(y as i32);
            y1 = y1.max(y as i32 + 1);
            for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let j = ny * w + nx;
                    if src[j] >= 128 && lab[j] == 0 {
                        lab[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        ys.push((y0, y1));
    }
    (lab, ys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inpaint::{Ctx, Inpainter, TileSpec};
    use image::Rgb;

    /// Maskeli pikselleri macenta yapan sahte model; kırpıntı boyutlarını kaydeder. Çıktının maske dışı pikselleri bilerek bozulur
    /// (+1): sayfaya yalnızca maskeli piksellerin geçtiği doğrulanır.
    struct Mock {
        calls: Vec<(u32, u32)>,
        spec: TileSpec,
    }
    impl Mock {
        fn new() -> Self { Mock { calls: vec![], spec: TileSpec::DEFAULT } }
    }
    impl Inpainter for Mock {
        fn label(&self) -> String { "mock".into() }
        fn tile_spec(&self) -> TileSpec { self.spec }
        fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, _c: &Ctx) -> Result<RgbImage> {
            self.calls.push(img.dimensions());
            let mut o = img.clone();
            for (x, y, m) in mask.enumerate_pixels() {
                let p = o.get_pixel_mut(x, y);
                *p = if m[0] >= 128 { Rgb([255, 0, 255]) } else { Rgb([p[0].saturating_add(1), p[1], p[2]]) };
            }
            Ok(o)
        }
    }

    fn rect(mask: &mut GrayImage, x0: u32, y0: u32, x1: u32, y1: u32) {
        for y in y0..y1 {
            for x in x0..x1 {
                mask.put_pixel(x, y, image::Luma([255]));
            }
        }
    }

    fn params(dilation: u32) -> CleanParams { CleanParams { dilation, feather: 2, flat_fill: false } }

    #[test]
    fn align_to_undoes_a_shifted_model_output() {
        let src = RgbImage::from_fn(120, 90, |x, y| { let v = ((x * 7 + y * 13) % 97 * 2) as u8; Rgb([v, v, v]) });
        let shifted = RgbImage::from_fn(120, 90, |x, y| *src.get_pixel((x + 3).min(119), (y + 117 - 2).min(89 + 117) .saturating_sub(117).min(89)));
        let mut mask = GrayImage::new(120, 90);
        rect(&mut mask, 50, 35, 70, 55);
        let fixed = align_to(&src, shifted.clone(), &mask);
        // iç bölgede hizalı
        let d: u64 = (20..100).flat_map(|x| (20..70).map(move |y| (x, y))).map(|(x, y)| (fixed.get_pixel(x, y)[0] as i64 - src.get_pixel(x, y)[0] as i64).unsigned_abs()).sum();
        assert_eq!(d, 0);
        // kaymasız çıktı aynen kalır
        assert_eq!(align_to(&src, src.clone(), &mask).as_raw(), src.as_raw());
    }

    #[test]
    fn tiles_geometry_cores_crops_and_exact_cover() {
        let (w, h) = (1500u32, 2400u32);
        let mut mask = GrayImage::new(w, h);
        rect(&mut mask, 50, 100, 200, 160); // küçük, gruplanır
        rect(&mut mask, 220, 130, 300, 190); // yakın komşu: aynı karoya girer
        rect(&mut mask, 1400, 2300, 1499, 2399); // sayfa köşesi
        rect(&mut mask, 600, 500, 1300, 640); // 700 px geniş: 512'lik çekirdeklere bölünür
        rect(&mut mask, 100, 1000, 700, 1800); // 600x800 dev bileşen
        let spec = TileSpec::DEFAULT;
        let (labels, tiles) = plan_tiles(&mask, &spec);
        let mut covered = vec![0u8; (w * h) as usize];
        for t in &tiles {
            let [x0, y0, x1, y1] = t.core;
            assert!(x1 - x0 <= 512 && y1 - y0 <= 512, "çekirdek 512'yi aştı {:?}", t.core);
            let [cx0, cy0, cx1, cy1] = t.crop;
            assert!(cx1 - cx0 <= 512 + 2 * 128 && cy1 - cy0 <= 512 + 2 * 128, "kırpıntı 768'i aştı {:?}", t.crop);
            assert!(cx1 <= w && cy1 <= h);
            // kırpıntı çekirdeği (sahip olunan pikselleri) bağlamla kapsar
            for y in y0..y1 {
                for x in x0..x1 {
                    let l = labels[(y * w + x) as usize];
                    if l != 0 && t.components.binary_search(&l).is_ok() {
                        assert!(x >= cx0 && x < cx1 && y >= cy0 && y < cy1, "sahip piksel kırpıntı dışında");
                        covered[(y * w + x) as usize] += 1;
                    }
                }
            }
        }
        for (i, &c) in covered.iter().enumerate() {
            let m = mask.as_raw()[i] >= 128;
            assert_eq!(c, m as u8, "piksel {} {} kez kapsandı (maske {m})", i, c);
        }
        assert!(tiles.len() >= 6, "{}", tiles.len());
    }

    #[test]
    fn tiled_clean_changes_only_masked_pixels_each_once() {
        let (w, h) = (1100u32, 1700u32);
        let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 200) as u8, (y % 241) as u8, 7]));
        let mut mask = GrayImage::new(w, h);
        rect(&mut mask, 50, 100, 200, 160);
        rect(&mut mask, 300, 1150, 900, 1300); // 600 geniş: iki çekirdek
        rect(&mut mask, 900, 1600, 1000, 1690);
        let mut mk = Mock::new();
        let out = clean_image(&img, &mask, &params(0), None, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert!(mk.calls.len() >= 3 && mk.calls.iter().all(|c| c.0 <= 768 && c.1 <= 768), "{:?}", mk.calls);
        for (x, y, p) in out.enumerate_pixels() {
            if mask.get_pixel(x, y)[0] >= 128 {
                assert_eq!(*p, Rgb([255, 0, 255]), "maskeli piksel temizlenmedi {x},{y}");
            } else {
                assert_eq!(p, img.get_pixel(x, y), "maske dışı değişti {x},{y}");
            }
        }
    }

    #[test]
    fn tall_strip_is_tiles_not_whole_page() {
        let (w, h) = (600u32, 5120u32);
        let img = RgbImage::from_pixel(w, h, Rgb([10, 20, 30]));
        let mut mask = GrayImage::new(w, h);
        rect(&mut mask, 50, 100, 200, 160);
        rect(&mut mask, 300, 3400, 420, 3500);
        let mut mk = Mock::new();
        clean_image(&img, &mask, &params(3), None, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert_eq!(mk.calls.len(), 2);
        assert!(mk.calls.iter().all(|c| c.1 < 400), "{:?}", mk.calls); // bütün sayfa/şerit gitmedi
    }

    #[test]
    fn fixed_size_model_gets_smaller_cores() {
        let img = RgbImage::from_pixel(900, 900, Rgb([10, 20, 30]));
        let mut mask = GrayImage::new(900, 900);
        rect(&mut mask, 100, 100, 700, 200);
        let mut mk = Mock::new();
        mk.spec = TileSpec { core: 256, ..TileSpec::DEFAULT };
        clean_image(&img, &mask, &params(0), None, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert!(mk.calls.len() >= 3 && mk.calls.iter().all(|c| c.0 <= 512 && c.1 <= 512), "{:?}", mk.calls);
    }

    #[test]
    fn diffusion_region_postprocess_keeps_outside_mask_and_cleans_text() {
        // (bölge yolu) gri zemin + yazı; "difüzyon" modeli zemini 3 seviye koyulaştırıp maske içini düz gri yapar
        struct Diff;
        impl Inpainter for Diff {
            fn label(&self) -> String { "diff".into() }
            fn tile_spec(&self) -> TileSpec { TileSpec::redraw(16) }
            fn inpaint(&mut self, img: &RgbImage, _m: &GrayImage, _c: &Ctx) -> Result<RgbImage> {
                Ok(RgbImage::from_pixel(img.width(), img.height(), Rgb([197, 197, 197])))
            }
        }
        let mut img = RgbImage::from_pixel(300, 200, Rgb([200, 200, 200]));
        fill_rect(&mut img, 100, 90, 200, 100, Rgb([0, 0, 0]));
        let mut mask = GrayImage::new(300, 200);
        rect(&mut mask, 95, 85, 205, 105);
        let out = clean_image(&img, &mask, &params(0), None, &mut Diff, &AtomicBool::new(false), &|_, _| {}).unwrap();
        for (x, y, p) in out.enumerate_pixels() {
            if mask.get_pixel(x, y)[0] < 128 {
                assert_eq!(p, img.get_pixel(x, y), "maske dışı değişti {x},{y}");
            }
        }
        // yazı gitti; ışıklılık+ton eşleme sonrası zemin ~200 (modelin 197'si değil)
        let p = out.get_pixel(150, 95);
        assert!((p[0] as i32 - 200).abs() <= 3, "{p:?}");
    }

    fn fill_rect(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>) {
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x, y, c);
            }
        }
    }

    #[test]
    fn white_balloon_text_is_flat_filled() {
        let mut img = RgbImage::from_pixel(200, 200, Rgb([255, 255, 255]));
        let mut mask = GrayImage::new(200, 200);
        for k in 0..3u32 {
            fill_rect(&mut img, 50, 60 + k * 20, 150, 70 + k * 20, Rgb([0, 0, 0]));
            rect(&mut mask, 48, 58 + k * 20, 152, 72 + k * 20);
        }
        let f = flat_fill(&img, &mask, 2, &FLAT_CFG);
        assert_eq!(f.stats, FlatStats { flat: 3, model: 0 });
        assert!(f.remaining.as_raw().iter().all(|&v| v == 0));
        assert!(f.work.pixels().all(|p| p[0] >= 250), "yazı kaldı");
        // model hiç çağrılmaz
        let mut mk = Mock::new();
        let out = clean_image(&img, &mask, &CleanParams { dilation: 0, feather: 2, flat_fill: true }, None, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert!(mk.calls.is_empty());
        assert!(out.pixels().all(|p| p[0] >= 250));
    }

    #[test]
    fn text_over_screentone_goes_to_model() {
        let mut img = RgbImage::from_fn(200, 200, |x, y| if (x / 2 + y / 2) % 2 == 0 { Rgb([40, 40, 40]) } else { Rgb([230, 230, 230]) });
        fill_rect(&mut img, 50, 90, 150, 100, Rgb([0, 0, 0]));
        let mut mask = GrayImage::new(200, 200);
        rect(&mut mask, 48, 88, 152, 102);
        let f = flat_fill(&img, &mask, 2, &FLAT_CFG);
        assert_eq!(f.stats, FlatStats { flat: 0, model: 1 });
        assert_eq!(f.remaining.as_raw(), mask.as_raw());
        assert_eq!(f.work.as_raw(), img.as_raw());
        let mut mk = Mock::new();
        clean_image(&img, &mask, &CleanParams { dilation: 0, feather: 2, flat_fill: true }, None, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert_eq!(mk.calls.len(), 1);
    }

    #[test]
    fn gradient_and_light_dots_are_not_flat() {
        // eğim: halka boyunca renk değişir
        let mut img = RgbImage::from_fn(300, 100, |x, _| { let v = (x as f32 * 0.8) as u8; Rgb([v, v, v]) });
        fill_rect(&mut img, 20, 40, 280, 52, Rgb([0, 0, 0]));
        let mut mask = GrayImage::new(300, 100);
        rect(&mut mask, 18, 38, 282, 54);
        assert_eq!(flat_fill(&img, &mask, 2, &FLAT_CFG).stats.flat, 0);
    }

    #[test]
    fn balloon_outline_touching_ring_is_still_flat() {
        let mut img = RgbImage::from_pixel(200, 120, Rgb([255, 255, 255]));
        fill_rect(&mut img, 40, 55, 150, 65, Rgb([0, 0, 0])); // yazı
        fill_rect(&mut img, 156, 0, 158, 120, Rgb([0, 0, 0])); // balon çizgisi: maske kenarından 4 px sonra
        let mut mask = GrayImage::new(200, 120);
        rect(&mut mask, 38, 53, 152, 67);
        let f = flat_fill(&img, &mask, 2, &FLAT_CFG);
        assert_eq!(f.stats, FlatStats { flat: 1, model: 0 });
        // çizgi bozulmadı
        assert_eq!(*f.work.get_pixel(157, 60), Rgb([0, 0, 0]));
        assert!(f.work.get_pixel(100, 60)[0] >= 250);
    }

    #[test]
    fn line_passing_under_text_goes_to_model() {
        // beyaz zeminde yazının altından çapraz gri şerit geçiyor (gerçek sayfadaki kapı kenarı gibi)
        let mut img = RgbImage::from_pixel(240, 240, Rgb([250, 250, 250]));
        for y in 0..240i32 {
            for x in 0..240i32 {
                if (x + y - 240).abs() <= 3 {
                    img.put_pixel(x as u32, y as u32, Rgb([150, 150, 150]));
                }
            }
        }
        fill_rect(&mut img, 90, 110, 150, 130, Rgb([20, 20, 20])); // yazı
        let mut mask = GrayImage::new(240, 240);
        rect(&mut mask, 80, 100, 160, 140);
        let f = flat_fill(&img, &mask, 2, &FLAT_CFG);
        assert_eq!(f.stats, FlatStats { flat: 0, model: 1 });
        assert_eq!(f.work.as_raw(), img.as_raw(), "şerit silindi");
    }

    fn part(x0: u32, y0: u32, x1: u32, y1: u32, p: f32) -> FlatPart {
        FlatPart { x0, y0, x1, y1, p_flat: p }
    }

    fn page(bg: u8) -> (RgbImage, GrayImage) {
        let mut img = RgbImage::from_pixel(240, 160, Rgb([bg, bg, bg]));
        let fg = if bg > 128 { 0 } else { 255 };
        fill_rect(&mut img, 80, 70, 160, 84, Rgb([fg, fg, fg]));
        let mut mask = GrayImage::new(240, 160);
        rect(&mut mask, 78, 68, 162, 86);
        (img, mask)
    }

    #[test]
    fn server_rule_low_p_goes_to_model() {
        let (img, mask) = page(255);
        let f = flat_pass(&img, &mask, 2, Some(&[part(78, 68, 162, 86, 0.79)]));
        assert_eq!(f.stats, FlatStats { flat: 0, model: 1 });
        assert_eq!(f.work.as_raw(), img.as_raw());
        assert_eq!(f.remaining.as_raw(), mask.as_raw());
    }

    #[test]
    fn server_rule_p_ok_but_grey_background_goes_to_model() {
        let (img, mask) = page(200);
        let f = flat_pass(&img, &mask, 2, Some(&[part(78, 68, 162, 86, 0.99)]));
        assert_eq!(f.stats, FlatStats { flat: 0, model: 1 });
        assert_eq!(f.work.as_raw(), img.as_raw());
    }

    #[test]
    fn server_rule_p_ok_white_fills_pure_white() {
        let (img, mask) = page(252); // hafif kirli beyaz: dolgu saf 255
        let f = flat_pass(&img, &mask, 2, Some(&[part(78, 68, 162, 86, 0.97)]));
        assert_eq!(f.stats, FlatStats { flat: 1, model: 0 });
        assert_eq!(*f.work.get_pixel(120, 77), Rgb([255, 255, 255]));
        assert_eq!(f.remaining.get_pixel(120, 77)[0], 0);
    }

    #[test]
    fn server_rule_p_ok_black_fills_pure_black() {
        let (img, mask) = page(3);
        let f = flat_pass(&img, &mask, 2, Some(&[part(70, 60, 170, 95, 0.95)]));
        assert_eq!(f.stats, FlatStats { flat: 1, model: 0 });
        assert_eq!(*f.work.get_pixel(120, 77), Rgb([0, 0, 0]));
    }

    #[test]
    fn server_rule_needs_a_part_and_all_parts_ok() {
        let (img, mask) = page(255);
        // kesişen parça yok
        assert_eq!(flat_pass(&img, &mask, 2, Some(&[part(0, 0, 20, 20, 0.99)])).stats.model, 1);
        assert_eq!(flat_pass(&img, &mask, 2, Some(&[])).stats.model, 1);
        // kesişenlerden biri düşük
        assert_eq!(flat_pass(&img, &mask, 2, Some(&[part(70, 60, 120, 95, 0.99), part(120, 60, 170, 95, 0.5)])).stats.model, 1);
        assert_eq!(flat_pass(&img, &mask, 2, Some(&[part(70, 60, 120, 95, 0.99), part(120, 60, 170, 95, 0.95)])).stats.flat, 1);
    }

    #[test]
    fn server_rule_pixel_check_catches_grey_band_under_text() {
        // ince çizgiler sınıflandırıcının işi; piksel kontrolü zeminin rengine bakar (balon çizgisi takılmasın diye %25 sapmaya izin var)
        let (mut img, mask) = page(255);
        fill_rect(&mut img, 40, 58, 200, 96, Rgb([120, 120, 120])); // yazının çevresini kaplayan gri bant
        let f = flat_pass(&img, &mask, 2, Some(&[part(78, 68, 162, 86, 0.99)]));
        assert_eq!(f.stats, FlatStats { flat: 0, model: 1 });
    }

    #[test]
    fn no_server_data_falls_back_to_ring_rule() {
        let mut img = RgbImage::from_pixel(200, 200, Rgb([255, 255, 255]));
        fill_rect(&mut img, 50, 90, 150, 100, Rgb([0, 0, 0]));
        let mut mask = GrayImage::new(200, 200);
        rect(&mut mask, 48, 88, 152, 102);
        assert_eq!(flat_pass(&img, &mask, 2, None).stats.flat, 1);
        // aynı sayfa, sunucu "parça yok" derse (veri var) model
        assert_eq!(flat_pass(&img, &mask, 2, Some(&[])).stats, FlatStats { flat: 0, model: 1 });
    }

    #[test]
    fn ring_fallback_keeps_outlier_guard() {
        // halka kuralı: yazının altından geçen şerit hâlâ modele gider (sunucu verisi yok)
        let (mut img, mask) = page(250);
        fill_rect(&mut img, 40, 74, 76, 77, Rgb([120, 120, 120]));
        fill_rect(&mut img, 164, 74, 200, 77, Rgb([120, 120, 120]));
        assert_eq!(flat_pass(&img, &mask, 2, None).stats, FlatStats { flat: 0, model: 1 });
    }

    /// MangaTranslator bölge yolu: bileşen başına tek çağrı, kırpıntı `multiple`'ın katı (16 ve 32 denenir) ve sayfa içinde, maske dışı bayt bayt aynı.
    #[test]
    fn mt_regions_one_call_per_component_and_outside_mask_byte_identical() {
        struct MtMock {
            calls: Vec<(u32, u32)>,
            multiple: u32,
        }
        impl Inpainter for MtMock {
            fn label(&self) -> String { "mt".into() }
            fn tile_spec(&self) -> TileSpec { TileSpec::redraw(self.multiple) }
            fn inpaint(&mut self, img: &RgbImage, _m: &GrayImage, _c: &Ctx) -> Result<RgbImage> {
                self.calls.push(img.dimensions());
                // tüm kırpıntıyı yeniden çizer (maske dışı da değişir) -> yalnız bileşen pikselleri geçmeli
                Ok(RgbImage::from_pixel(img.width(), img.height(), Rgb([197, 197, 197])))
            }
        }
        let mut img = RgbImage::from_fn(500, 400, |x, y| Rgb([200, (x % 7) as u8 * 3, (y % 5) as u8 * 4]));
        fill_rect(&mut img, 100, 90, 160, 100, Rgb([0, 0, 0]));
        fill_rect(&mut img, 300, 250, 340, 262, Rgb([0, 0, 0]));
        let mut mask = GrayImage::new(500, 400);
        rect(&mut mask, 98, 88, 162, 102);
        rect(&mut mask, 298, 248, 342, 264);
        for m in [16u32, 32] {
        let mut mk = MtMock { calls: vec![], multiple: m };
        let out = clean_dilated(&img, mask.clone(), &params(0), &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert_eq!(mk.calls.len(), 2);
        assert!(mk.calls.iter().all(|c| c.0 % m == 0 && c.1 % m == 0 && c.0 >= 64 && c.1 >= 64 && c.0 <= 500 && c.1 <= 400), "{:?}", mk.calls);
        for (x, y, p) in out.enumerate_pixels() {
            if mask.get_pixel(x, y)[0] < 128 {
                assert_eq!(p, img.get_pixel(x, y), "maske dışı değişti {x},{y}");
            }
        }
        // yazı gitti
        assert!(out.get_pixel(130, 95)[0] > 150);
        assert!(out.get_pixel(320, 255)[0] > 150);
        }
    }
}
