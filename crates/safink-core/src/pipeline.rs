//! Sayfa temizleme hattı: genişlet -> grupla -> pencerele -> inpaint -> yalnızca maskeli pikselleri tüy ile geri yaz.
use crate::inpaint::{pad_gray, pad_rgb, Ctx, Inpainter, WindowPolicy};
use crate::maskops::{self, Rect};
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

struct Win {
    /// Görüntü koordinatlarında kırpma bölgesi (görüntü içinde).
    rect: Rect,
    /// Bu pencerenin "sahip olduğu" maske bölgesi.
    core: Rect,
}

fn clamp_rect_to(r: Rect, w: i32, h: i32) -> Rect {
    Rect { x0: r.x0.max(0), y0: r.y0.max(0), x1: r.x1.min(w), y1: r.y1.min(h) }
}

/// `core`'u içeren, hedef boyutlu (ya da görüntüden küçükse görüntü boyutlu) pencere.
fn make_window(core: Rect, tw: i32, th: i32, iw: i32, ih: i32) -> Rect {
    let (ww, wh) = (tw.min(iw), th.min(ih));
    let cx = (core.x0 + core.x1) / 2;
    let cy = (core.y0 + core.y1) / 2;
    let x0 = (cx - ww / 2).clamp(0, iw - ww);
    let y0 = (cy - wh / 2).clamp(0, ih - wh);
    Rect { x0, y0, x1: x0 + ww, y1: y0 + wh }
}

/// Sınırlı birleştirme: birleşim `max_w` x `max_h`'ı aşmıyorsa ve kutular `gap` içindeyse birleştir.
fn merge_bounded(mut boxes: Vec<Rect>, gap: i32, max_w: i32, max_h: i32) -> Vec<Rect> {
    boxes.sort_by_key(|b| (b.y0, b.x0));
    loop {
        let mut merged = false;
        let mut i = 0;
        while i < boxes.len() {
            let mut j = i + 1;
            while j < boxes.len() {
                if boxes[j].y0 > boxes[i].y1 + gap + max_h {
                    // sıralı değil ama büyük farklarda erken çıkış güvenli değil; sadece devam
                }
                let u = boxes[i].union(&boxes[j]);
                if u.w() <= max_w && u.h() <= max_h && boxes[i].grow(gap / 2).intersects(&boxes[j].grow(gap / 2)) {
                    boxes[i] = u;
                    boxes.swap_remove(j);
                    merged = true;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        if !merged {
            break;
        }
    }
    boxes.sort_by_key(|b| (b.y0, b.x0));
    boxes
}

fn plan(mask: &GrayImage, policy: WindowPolicy) -> Vec<Win> {
    let (iw, ih) = (mask.width() as i32, mask.height() as i32);
    let comps: Vec<Rect> = maskops::component_boxes(mask, 4).into_iter().map(|(r, _)| r).collect();
    let (target, core_max) = match policy {
        WindowPolicy::Fixed(s) => (s as i32, s as i32 - 96),
        WindowPolicy::Page { .. } => unreachable!("tam sayfa politikası plan() kullanmaz"),
    };
    let groups = merge_bounded(comps, 48, core_max, core_max);
    let mut wins = Vec::new();
    for g in groups {
        // çok büyük tek bileşenleri çekirdek karolara böl
        let nx = ((g.w() + core_max - 1) / core_max).max(1);
        let ny = ((g.h() + core_max - 1) / core_max).max(1);
        for ty in 0..ny {
            for tx in 0..nx {
                let core = clamp_rect_to(
                    Rect { x0: g.x0 + tx * core_max, y0: g.y0 + ty * core_max, x1: (g.x0 + (tx + 1) * core_max).min(g.x1), y1: (g.y0 + (ty + 1) * core_max).min(g.y1) },
                    iw,
                    ih,
                );
                wins.push(Win { rect: make_window(core, target, target, iw, ih), core });
            }
        }
    }
    wins
}

/// Tüm sayfayı temizler. `mask`: son (dilate edilmemiş) maske, 0/255.
pub fn clean_image(
    img: &RgbImage,
    mask: &GrayImage,
    p: &CleanParams,
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
        let f = flat_fill(img, &m, p.feather, &FLAT_CFG);
        if f.stats.model == 0 {
            return Ok(f.work);
        }
        return clean_dilated(&f.work, f.remaining, p, inp, cancel, progress);
    }
    clean_dilated(img, m, p, inp, cancel, progress)
}

/// Genişletilmiş maskeyi (`m`) seçilen modelle temizler.
pub fn clean_dilated(
    img: &RgbImage,
    m: GrayImage,
    p: &CleanParams,
    inp: &mut dyn Inpainter,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, usize),
) -> Result<RgbImage> {
    if let WindowPolicy::Page { seg_h, overlap, max_w, max_px } = inp.policy() {
        return clean_paged(img, m, p, inp, cancel, progress, seg_h, overlap, max_w, max_px);
    }
    let wins = plan(&m, inp.policy());
    let mut remaining = m;
    let mut work = img.clone();
    let total = wins.len();
    progress(0, total);
    let fixed = match inp.policy() {
        WindowPolicy::Fixed(s) => Some(s),
        WindowPolicy::Page { .. } => None,
    };
    for (i, w) in wins.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            bail!("iptal edildi");
        }
        let (rw, rh) = (w.rect.w() as u32, w.rect.h() as u32);
        let crop = image::imageops::crop_imm(&work, w.rect.x0 as u32, w.rect.y0 as u32, rw, rh).to_image();
        // pencere içi maske: yalnızca çekirdeğin sahip olduğu kalan pikseller
        let mut wm = GrayImage::new(rw, rh);
        let mut any = false;
        for y in w.core.y0.max(w.rect.y0)..w.core.y1.min(w.rect.y1) {
            for x in w.core.x0.max(w.rect.x0)..w.core.x1.min(w.rect.x1) {
                if remaining.get_pixel(x as u32, y as u32)[0] >= 128 {
                    wm.put_pixel((x - w.rect.x0) as u32, (y - w.rect.y0) as u32, image::Luma([255]));
                    any = true;
                }
            }
        }
        if !any {
            progress(i + 1, total);
            continue;
        }
        let (crop_in, wm_in) = match fixed {
            Some(s) => (pad_rgb(&crop, s, s), pad_gray(&wm, s, s)),
            None => (crop.clone(), wm.clone()),
        };
        let ctx = Ctx { cancel };
        let out_full = inp.inpaint(&crop_in, &wm_in, &ctx)?;
        let out = image::imageops::crop_imm(&out_full, 0, 0, rw, rh).to_image();
        let alpha = maskops::feather_alpha(&wm, p.feather as f32);
        for y in 0..rh {
            for x in 0..rw {
                let a = alpha[(y * rw + x) as usize];
                if a <= 0.004 {
                    continue;
                }
                let (gx, gy) = (w.rect.x0 as u32 + x, w.rect.y0 as u32 + y);
                let o = *work.get_pixel(gx, gy);
                let n = *out.get_pixel(x, y);
                let mut px = o;
                for c in 0..3 {
                    px[c] = (o[c] as f32 * (1.0 - a) + n[c] as f32 * a).round().clamp(0.0, 255.0) as u8;
                }
                work.put_pixel(gx, gy, px);
                if wm.get_pixel(x, y)[0] >= 128 {
                    remaining.put_pixel(gx, gy, image::Luma([0]));
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

/// Tam sayfa politikası: sayfa (ya da uzun şeritte en çok `seg_h` yüksekliğinde dikey dilimler) modele bütün olarak gider.
/// Dilimler `overlap` kadar örtüşür; her maskeli piksel tam olarak bir dilimden geri yazılır. Bir yazı bileşeni sınırı
/// aşarsa tamamı en çok pikselini içeren dilime verilir ve o dilimin penceresi bileşeni tam kapsayacak şekilde genişler.
/// Ölçekleme yok: boyut kısıtını (16'nın katı) modelin kendisi dolgu ile sağlar. Sığmayan sayfa hata verir.
#[allow(clippy::too_many_arguments)]
fn clean_paged(
    img: &RgbImage,
    m: GrayImage,
    p: &CleanParams,
    inp: &mut dyn Inpainter,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, usize),
    seg_h: u32,
    overlap: u32,
    max_w: u32,
    max_px: u64,
) -> Result<RgbImage> {
    let (iw, ih) = img.dimensions();
    if iw > max_w {
        bail!("sayfa çok geniş ({iw} px); bu model en çok {max_w} px genişliği işler (küçültme yapılmaz)");
    }
    let strip = ih > seg_h && ih as u64 > 2 * iw as u64;
    if !strip && iw as u64 * ih as u64 > max_px {
        bail!("sayfa çok büyük ({iw}x{ih}); bu model en çok {:.1} megapiksel işler (küçültme yapılmaz)", max_px as f64 / 1e6);
    }
    let ov = overlap as i32;
    let (iwi, ihi) = (iw as i32, ih as i32);
    // dilim çekirdekleri (birbirini kesmeyen y aralıkları)
    let cores: Vec<(i32, i32)> = if strip {
        let core_h = (seg_h - overlap) as i32;
        let n = (ihi + core_h - 1) / core_h;
        let ch = (ihi + n - 1) / n;
        (0..n).map(|k| (k * ch, ((k + 1) * ch).min(ihi))).collect()
    } else {
        vec![(0, ihi)]
    };
    let n = cores.len();
    let (labels, spans) = if n > 1 { label_components(&m) } else { (Vec::new(), Vec::new()) };
    // bileşen -> sahip dilim (en çok piksel)
    let mut owner = vec![0usize; spans.len()];
    if n > 1 {
        let mut counts = vec![0u32; spans.len() * n];
        for y in 0..ihi {
            let s = cores.iter().position(|c| y >= c.0 && y < c.1).unwrap_or(n - 1);
            for &l in &labels[(y * iwi) as usize..((y + 1) * iwi) as usize] {
                if l != 0 {
                    counts[l as usize * n + s] += 1;
                }
            }
        }
        for l in 1..spans.len() {
            owner[l] = (0..n).max_by_key(|&s| (counts[l * n + s], std::cmp::Reverse(s))).unwrap_or(0);
        }
    }
    // pencere = çekirdek +- overlap/2, sahip olunan bileşenleri (64 px bağlamla) tam kapsayacak kadar genişletilir
    let mut wins: Vec<(i32, i32)> = cores.iter().map(|&(a, b)| ((a - ov / 2).max(0), (b + ov / 2).min(ihi))).collect();
    for l in 1..spans.len() {
        let s = owner[l];
        wins[s].0 = wins[s].0.min((spans[l].0 - 64).max(0));
        wins[s].1 = wins[s].1.max((spans[l].1 + 64).min(ihi));
    }
    let mut work = img.clone();
    progress(0, n);
    for s in 0..n {
        if cancel.load(Ordering::Relaxed) {
            bail!("iptal edildi");
        }
        let (wy0, wy1) = wins[s];
        let rh = (wy1 - wy0) as u32;
        let mut wm = GrayImage::new(iw, rh);
        let mut any = false;
        for y in wy0..wy1 {
            for x in 0..iwi {
                let i = (y * iwi + x) as usize;
                let on = if n > 1 { labels[i] != 0 && owner[labels[i] as usize] == s } else { m.as_raw()[i] >= 128 };
                if on {
                    wm.put_pixel(x as u32, (y - wy0) as u32, image::Luma([255]));
                    any = true;
                }
            }
        }
        if !any {
            progress(s + 1, n);
            continue;
        }
        let crop = image::imageops::crop_imm(&work, 0, wy0 as u32, iw, rh).to_image();
        let out = inp.inpaint(&crop, &wm, &Ctx { cancel })?;
        if out.dimensions() != (iw, rh) {
            bail!("model beklenmeyen boyut döndürdü: {:?} (beklenen {}x{})", out.dimensions(), iw, rh);
        }
        let alpha = maskops::feather_alpha(&wm, p.feather as f32);
        for y in 0..rh {
            for x in 0..iw {
                let a = alpha[(y * iw + x) as usize];
                if a <= 0.004 {
                    continue;
                }
                let gy = wy0 as u32 + y;
                // başka dilimin maskesine ait piksellere dokunma (tek dilimden yazılır)
                if wm.get_pixel(x, y)[0] < 128 && m.get_pixel(x, gy)[0] >= 128 {
                    continue;
                }
                let o = *work.get_pixel(x, gy);
                let nw = *out.get_pixel(x, y);
                let mut px = o;
                for c in 0..3 {
                    px[c] = (o[c] as f32 * (1.0 - a) + nw[c] as f32 * a).round().clamp(0.0, 255.0) as u8;
                }
                work.put_pixel(x, gy, px);
            }
        }
        progress(s + 1, n);
    }
    Ok(work)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inpaint::{Ctx, Inpainter, WindowPolicy};
    use image::Rgb;

    /// Maskeli pikselleri macenta yapan sahte model; pencere boyutlarını kaydeder.
    struct Mock {
        calls: Vec<(u32, u32)>,
    }
    impl Inpainter for Mock {
        fn label(&self) -> String { "mock".into() }
        fn policy(&self) -> WindowPolicy { WindowPolicy::Page { seg_h: 2000, overlap: 200, max_w: 2048, max_px: 6_300_000 } }
        fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, _c: &Ctx) -> Result<RgbImage> {
            self.calls.push(img.dimensions());
            let mut o = img.clone();
            for (x, y, m) in mask.enumerate_pixels() {
                if m[0] >= 128 {
                    o.put_pixel(x, y, Rgb([255, 0, 255]));
                }
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

    #[test]
    fn strip_segments_paste_each_masked_pixel_once() {
        let (w, h) = (600u32, 5120u32);
        let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 251) as u8, (y % 241) as u8, 7]));
        let mut mask = GrayImage::new(w, h);
        rect(&mut mask, 50, 100, 200, 160); // ilk dilim
        rect(&mut mask, 300, 1750, 420, 1990); // sınırı aşan bileşen (1800/1707 civarı çekirdek sınırı)
        rect(&mut mask, 100, 3400, 300, 3500);
        rect(&mut mask, 10, 5050, 90, 5110);
        let mut mk = Mock { calls: vec![] };
        let out = clean_image(&img, &mask, &CleanParams { dilation: 0, feather: 0, flat_fill: false }, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert!(mk.calls.len() >= 3, "{:?}", mk.calls);
        assert!(mk.calls.iter().all(|c| c.0 == w), "tam genişlik");
        for (x, y, p) in out.enumerate_pixels() {
            let masked = mask.get_pixel(x, y)[0] >= 128;
            if masked {
                assert_eq!(*p, Rgb([255, 0, 255]), "maskeli piksel temizlenmedi {x},{y}");
            } else {
                assert_eq!(p, img.get_pixel(x, y), "maske dışı değişti {x},{y}");
            }
        }
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
        let mut mk = Mock { calls: vec![] };
        let out = clean_image(&img, &mask, &CleanParams { dilation: 0, feather: 2, flat_fill: true }, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
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
        let mut mk = Mock { calls: vec![] };
        clean_image(&img, &mask, &CleanParams { dilation: 0, feather: 2, flat_fill: true }, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
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
    fn normal_page_is_one_pass() {
        let (w, h) = (1115u32, 1600u32);
        let img = RgbImage::from_pixel(w, h, Rgb([10, 20, 30]));
        let mut mask = GrayImage::new(w, h);
        rect(&mut mask, 50, 100, 200, 160);
        rect(&mut mask, 900, 1500, 1000, 1590);
        let mut mk = Mock { calls: vec![] };
        clean_image(&img, &mask, &CleanParams { dilation: 3, feather: 2, flat_fill: false }, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert_eq!(mk.calls, vec![(w, h)]);
    }

    #[test]
    fn too_wide_page_is_reported() {
        let img = RgbImage::new(3000, 100);
        let mask = GrayImage::new(3000, 100);
        let mut mk = Mock { calls: vec![] };
        let e = clean_image(&img, &mask, &CleanParams { dilation: 0, feather: 0, flat_fill: false }, &mut mk, &AtomicBool::new(false), &|_, _| {}).unwrap_err();
        assert!(e.to_string().contains("geniş"));
    }
}
