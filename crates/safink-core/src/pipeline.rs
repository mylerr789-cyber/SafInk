//! Sayfa temizleme hattı (Koharu gibi): genişlet -> (balon içi düz doldur) -> kalan maske bileşenleri karolara bölünür; her karo yerel
//! çözünürlükte (çekirdek <= 512, + 128 px bağlam) modele gider ve yalnızca maskeli pikseller sayfaya yazılır (LaMa, Flux, Qwen aynı yol).
use crate::bubbles::Bubble;
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

/// Düz zemin doldurma (BallonsTranslator gibi): her bileşenin halkası düzse bileşen ortanca renkle (tüylü) doldurulur.
/// `m`: genişletilmiş maske. Düz olmayan bileşenler `remaining` içinde kalır.
pub fn flat_fill(img: &RgbImage, m: &GrayImage, feather: u32, c: &FlatCfg) -> FlatOut {
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
        let Some(col) = ring_flat(img, &ring, c) else {
            stats.model += 1;
            continue;
        };
        // Halkadaki aykırı pikseller ayrı ayrı yerlerde çıkıyorsa yazının altından bir şey geçiyordur
        // (çizgi, tram şeridi, sanat): düz dolgu onu silerdi, bileşen modele gider. Balon çizgisine
        // değen yazıda aykırılar tek bir yayda toplanır.
        if ring_outlier_groups(img, &ring, col, (x0, y0, cw, ch), c) >= 2 {
            stats.model += 1;
            continue;
        }
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

/// Koharu/BallonsTranslator kuralı: balonun maske dışı, kenardan ≥ `BUBBLE_EDGE_MARGIN` px içerideki pikselleri (≥ `BUBBLE_MIN_PIXELS`) düzse
/// (kanal başına ortancadan standart sapma < 10; kanal sapmaları birbirinden >1 ayrışıyorsa < 7) balon içindeki maske pikselleri
/// ortanca renkle doldurulur. Balon dışında kalan yazı ve düz olmayan balonlar modele gider.
pub const BUBBLE_EDGE_MARGIN: u32 = 3;
pub const BUBBLE_MIN_PIXELS: usize = 16;
const EDGE_PAD: u32 = 4;

fn median_u8(v: &mut [u8]) -> u8 {
    v.sort_unstable();
    let mid = v.len() / 2;
    if v.len() % 2 == 0 { ((v[mid - 1] as u16 + v[mid] as u16) / 2) as u8 } else { v[mid] }
}

/// Balonun düz zemin rengi (maske dışı iç pikseller); düz değilse `None`.
fn bubble_uniform_color(img: &RgbImage, pending: &GrayImage, b: &Bubble) -> Option<[u8; 3]> {
    let [bx0, by0, _, _] = b.bounds;
    let (bw, bh) = (b.w(), b.h());
    let m = EDGE_PAD;
    // kenara uzaklık: balonun dışı 255, içi 0 -> disk genişletme -> 0 kalanlar kenardan >= margin içeride
    let mut outside = GrayImage::from_pixel(bw + 2 * m, bh + 2 * m, image::Luma([255]));
    for y in 0..bh {
        for x in 0..bw {
            if b.mask[(y * bw + x) as usize] != 0 {
                outside.put_pixel(x + m, y + m, image::Luma([0]));
            }
        }
    }
    let near = maskops::dilate(&outside, BUBBLE_EDGE_MARGIN);
    let (iw, ih) = img.dimensions();
    let mut ch = [Vec::new(), Vec::new(), Vec::new()];
    for y in 0..bh {
        for x in 0..bw {
            if near.get_pixel(x + m, y + m)[0] >= 128 {
                continue;
            }
            let (gx, gy) = (bx0 + x, by0 + y);
            // sayfa kenarına değen balon: sayfanın dışı da "kenar" sayılır
            if gx < BUBBLE_EDGE_MARGIN || gy < BUBBLE_EDGE_MARGIN || gx + BUBBLE_EDGE_MARGIN >= iw || gy + BUBBLE_EDGE_MARGIN >= ih {
                continue;
            }
            if pending.get_pixel(gx, gy)[0] >= 128 {
                continue;
            }
            let p = img.get_pixel(gx, gy);
            for k in 0..3 {
                ch[k].push(p[k]);
            }
        }
    }
    if ch[0].len() < BUBBLE_MIN_PIXELS {
        return None;
    }
    let med = [median_u8(&mut ch[0]), median_u8(&mut ch[1]), median_u8(&mut ch[2])];
    let dev: Vec<f64> = (0..3).map(|k| (ch[k].iter().map(|&v| (v as f64 - med[k] as f64).powi(2)).sum::<f64>() / ch[k].len() as f64).sqrt()).collect();
    let mean = dev.iter().sum::<f64>() / 3.0;
    let spread = (dev.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / 3.0).sqrt();
    let thr = if spread > 1.0 { 7.0 } else { 10.0 };
    (dev.iter().cloned().fold(0.0, f64::max) < thr).then_some(med)
}

/// Balon tabanlı düz doldurma. `m`: genişletilmiş maske. Yalnızca düz bir balonun içindeki maske pikselleri doldurulur (tüylü kenar balon içinde kalır).
pub fn flat_fill_bubbles(img: &RgbImage, m: &GrayImage, feather: u32, bubbles: &[Bubble], c: &FlatCfg) -> FlatOut {
    let (w, h) = img.dimensions();
    let mut work = img.clone();
    let mut pending = m.clone();
    for b in bubbles {
        let [bx0, by0, bx1, by1] = b.bounds;
        let (bx1, by1) = (bx1.min(w), by1.min(h));
        let mut targets: Vec<(u32, u32)> = Vec::new();
        for y in by0..by1 {
            for x in bx0..bx1 {
                if pending.get_pixel(x, y)[0] >= 128 && b.contains(x, y) {
                    targets.push((x, y));
                }
            }
        }
        if targets.is_empty() {
            continue;
        }
        let Some(col) = bubble_uniform_color(img, &pending, b) else { continue };
        let fpad = (feather as f32 * 1.2).ceil() as i32 * 3 + 2;
        let (fx0, fy0) = ((bx0 as i32 - fpad).max(0), (by0 as i32 - fpad).max(0));
        let (fx1, fy1) = ((bx1 as i32 + fpad).min(w as i32), (by1 as i32 + fpad).min(h as i32));
        let (fw, fh) = ((fx1 - fx0) as u32, (fy1 - fy0) as u32);
        let mut tm = GrayImage::new(fw, fh);
        for &(x, y) in &targets {
            tm.put_pixel(x - fx0 as u32, y - fy0 as u32, image::Luma([255]));
        }
        let alpha = maskops::feather_alpha(&tm, feather as f32);
        for y in 0..fh {
            for x in 0..fw {
                let a = alpha[(y * fw + x) as usize];
                if a <= 0.004 {
                    continue;
                }
                let (gx, gy) = (x + fx0 as u32, y + fy0 as u32);
                let is_target = tm.get_pixel(x, y)[0] >= 128;
                if !is_target {
                    // tüy payı: yalnızca balonun içinde, maskesiz ve zemine benzeyen pikseller (balon çizgisi/sanat bozulmaz)
                    if !b.contains(gx, gy) || pending.get_pixel(gx, gy)[0] >= 128 {
                        continue;
                    }
                    let o = work.get_pixel(gx, gy);
                    if (0..3).any(|k| (o[k] as i32 - col[k] as i32).abs() > c.outlier_dev) {
                        continue;
                    }
                }
                let o = *work.get_pixel(gx, gy);
                let mut px = o;
                for k in 0..3 {
                    px[k] = (o[k] as f32 * (1.0 - a) + col[k] as f32 * a).round().clamp(0.0, 255.0) as u8;
                }
                work.put_pixel(gx, gy, px);
            }
        }
        for &(x, y) in &targets {
            pending.put_pixel(x, y, image::Luma([0]));
        }
    }
    // istatistik: özgün maskenin bileşenleri; hiç kalan pikseli olmayan düz sayılır
    let (labels, spans) = label_components(m);
    let n = spans.len().saturating_sub(1);
    let mut left = vec![false; n + 1];
    for (i, &l) in labels.iter().enumerate() {
        if l != 0 && pending.as_raw()[i] >= 128 {
            left[l as usize] = true;
        }
    }
    let model = left.iter().filter(|&&v| v).count();
    FlatOut { work, remaining: pending, stats: FlatStats { flat: n - model, model } }
}

/// Düz doldurma geçişi: balonlar biliniyorsa Koharu kuralı, bilinmiyorsa (model yok/başarısız) halka kuralı.
pub fn flat_pass(img: &RgbImage, m: &GrayImage, feather: u32, bubbles: Option<&[Bubble]>) -> FlatOut {
    match bubbles {
        Some(b) => flat_fill_bubbles(img, m, feather, b, &FLAT_CFG),
        None => flat_fill(img, m, feather, &FLAT_CFG),
    }
}

/// Tüm sayfayı temizler. `mask`: son (dilate edilmemiş) maske, 0/255.
pub fn clean_image(
    img: &RgbImage,
    mask: &GrayImage,
    p: &CleanParams,
    bubbles: Option<&[Bubble]>,
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
        let f = flat_pass(img, &m, p.feather, bubbles);
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

/// Genişletilmiş maskeyi (`m`) seçilen modelle Koharu gibi karo karo temizler: her karo yerel çözünürlükte modele gider
/// (yalnızca model boyut katına dolgu), çıktının yalnızca karonun maske bileşenlerinden çekirdek içine düşen pikselleri sayfaya
/// yazılır; maske dışı hiçbir piksel değişmez. Difüzyon karolarında MangaTranslator son işlemi uygulanır (bkz. `postproc`).
/// Not: Koharu'nun kendisi ton eşleme/tüy yapmaz, sert yapıştırır; LaMa karoları öyle yapıştırılır. `_p.feather` yalnızca düz doldurmada kullanılır.
pub fn clean_dilated(
    img: &RgbImage,
    m: GrayImage,
    _p: &CleanParams,
    inp: &mut dyn Inpainter,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, usize),
) -> Result<RgbImage> {
    let spec = inp.tile_spec();
    let (iw, ih) = img.dimensions();
    let (labels, tiles) = plan_tiles(&m, &spec);
    let post = if spec.postprocess { postproc::Post::from_env() } else { postproc::Post::None };
    let radius = postproc::glyph_radius(iw, ih);
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
        let mut lab_crop = vec![0u32; (cw * ch) as usize];
        for y in ky0..ky1 {
            for x in kx0..kx1 {
                let l = labels[(y * iw + x) as usize];
                if l != 0 && t.components.binary_search(&l).is_ok() {
                    let k = ((y - cy0) * cw + (x - cx0)) as usize;
                    own[k] = true;
                    lab_crop[k] = l;
                }
            }
        }
        let patch = match post {
            postproc::Post::None => out,
            postproc::Post::Lum => postproc::match_luminance(&crop, &out, &cm),
            postproc::Post::Full => {
                let lum = postproc::match_luminance(&crop, &out, &cm);
                postproc::composite_glyphs(&crop, &lum, &cm, &own, &lab_crop, radius)
            }
        };
        for y in 0..ch {
            for x in 0..cw {
                if own[(y * cw + x) as usize] {
                    work.put_pixel(cx0 + x, cy0 + y, *patch.get_pixel(x, y));
                    pending.put_pixel(cx0 + x, cy0 + y, image::Luma([0]));
                }
            }
        }
        progress(i + 1, total);
    }
    Ok(work)
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
    fn diffusion_tile_postprocess_keeps_outside_mask_and_cleans_text() {
        // gri zemin + yazı; "difüzyon" modeli zemini 3 seviye koyulaştırıp maske içini düz gri yapar
        struct Diff;
        impl Inpainter for Diff {
            fn label(&self) -> String { "diff".into() }
            fn tile_spec(&self) -> TileSpec { TileSpec { postprocess: true, ..TileSpec::DEFAULT } }
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

    fn ellipse_bubble(cx: f32, cy: f32, rx: f32, ry: f32) -> Bubble {
        let b = [(cx - rx) as u32 - 1, (cy - ry) as u32 - 1, (cx + rx) as u32 + 2, (cy + ry) as u32 + 2];
        Bubble::from_fn(b, |x, y| ((x as f32 - cx) / rx).powi(2) + ((y as f32 - cy) / ry).powi(2) <= 1.0)
    }

    #[test]
    fn koharu_rule_fills_uniform_bubble_only_inside() {
        let mut img = RgbImage::from_pixel(300, 200, Rgb([200, 200, 200]));
        let bub = ellipse_bubble(100.0, 100.0, 70.0, 60.0);
        for y in 0..200 {
            for x in 0..300 {
                if bub.contains(x, y) {
                    img.put_pixel(x, y, Rgb([255, 255, 255]));
                }
            }
        }
        fill_rect(&mut img, 70, 95, 130, 105, Rgb([0, 0, 0])); // balon içi yazı
        fill_rect(&mut img, 220, 90, 270, 100, Rgb([0, 0, 0])); // balon dışı yazı
        let mut mask = GrayImage::new(300, 200);
        rect(&mut mask, 68, 93, 132, 107);
        rect(&mut mask, 218, 88, 272, 102);
        let f = flat_fill_bubbles(&img, &mask, 2, &[bub], &FLAT_CFG);
        assert_eq!(f.stats, FlatStats { flat: 1, model: 1 });
        assert!(f.work.get_pixel(100, 100)[0] >= 250, "balon içi yazı silinmedi");
        assert_eq!(*f.work.get_pixel(245, 95), Rgb([0, 0, 0]), "balon dışı yazıya dokunulmamalı");
        assert_eq!(f.remaining.get_pixel(100, 100)[0], 0);
        assert_eq!(f.remaining.get_pixel(245, 95)[0], 255);
        // maske dışı hiçbir yer değişmedi (tüy payı dışında balon içi zemin zaten aynı renk)
        for (x, y, p) in f.work.enumerate_pixels() {
            if mask.get_pixel(x, y)[0] < 128 && !(60..140).contains(&x) {
                assert_eq!(p, img.get_pixel(x, y));
            }
        }
    }

    #[test]
    fn koharu_rule_bubble_with_stripe_goes_to_model() {
        let mut img = RgbImage::from_pixel(300, 200, Rgb([255, 255, 255]));
        let bub = ellipse_bubble(150.0, 100.0, 100.0, 70.0);
        fill_rect(&mut img, 60, 60, 240, 66, Rgb([120, 120, 120])); // maskesiz şerit (tram/çizgi)
        fill_rect(&mut img, 110, 98, 190, 108, Rgb([0, 0, 0]));
        let mut mask = GrayImage::new(300, 200);
        rect(&mut mask, 108, 96, 192, 110);
        let f = flat_fill_bubbles(&img, &mask, 2, &[bub], &FLAT_CFG);
        assert_eq!(f.stats, FlatStats { flat: 0, model: 1 });
        assert_eq!(f.work.as_raw(), img.as_raw());
        assert_eq!(f.remaining.as_raw(), mask.as_raw());
    }

    #[test]
    fn no_bubbles_known_falls_back_to_ring_rule() {
        let mut img = RgbImage::from_pixel(200, 200, Rgb([255, 255, 255]));
        fill_rect(&mut img, 50, 90, 150, 100, Rgb([0, 0, 0]));
        let mut mask = GrayImage::new(200, 200);
        rect(&mut mask, 48, 88, 152, 102);
        assert_eq!(flat_pass(&img, &mask, 2, None).stats.flat, 1);
        assert_eq!(flat_pass(&img, &mask, 2, Some(&[])).stats, FlatStats { flat: 0, model: 1 }); // balon yok: balon dışı yazı modele
    }
}
