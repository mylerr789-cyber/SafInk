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
    let (target, pad_max, core_max) = match policy {
        WindowPolicy::Fixed(s) => (s as i32, 48, s as i32 - 96),
        WindowPolicy::Flexible { max, .. } => (max as i32, 96, max as i32 - 192),
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
                let (tw, th) = match policy {
                    WindowPolicy::Fixed(s) => (s as i32, s as i32),
                    WindowPolicy::Flexible { max, min } => {
                        let pad = ((core.w().max(core.h()) as f32 * 0.5) as i32).clamp(32, pad_max);
                        ((core.w() + 2 * pad).clamp(min as i32, max as i32), (core.h() + 2 * pad).clamp(min as i32, max as i32))
                    }
                };
                let _ = target;
                wins.push(Win { rect: make_window(core, tw, th, iw, ih), core });
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
    let wins = plan(&m, inp.policy());
    let mut remaining = m;
    let mut work = img.clone();
    let total = wins.len();
    progress(0, total);
    let fixed = match inp.policy() {
        WindowPolicy::Fixed(s) => Some(s),
        _ => None,
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
