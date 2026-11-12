//! MangaTranslator (core/image/inpainting.py `_CropInpainter.inpaint_mask`) kırpıntı geometrisi ve ölçek kuralları.
//! Qwen-Image 2.1 Turbo yolu: bölge = maskenin 8-bağlantılı bir bileşeni; kırpıntı = bileşen kutusu + bağlam, 32'nin katına
//! aşağı yuvarlanmış, sayfa içinde tutulmuş; model ~1 MP'ye büyütülmüş kırpıntıda çalışır, çıktı kırpıntı boyutuna geri küçülür.
use image::RgbImage;

pub const BLUR_SCALE_FACTOR: f64 = 0.1;
pub const MIN_BLUR_RADIUS: i64 = 1;
pub const MAX_BLUR_RADIUS: i64 = 10;
pub const CONTEXT_PADDING_RATIO: f64 = 0.5;
pub const MAX_CONTEXT_PADDING: i64 = 80;
pub const PADDING_MULTIPLIER: f64 = 2.0;
pub const MIN_RESOLUTION: i64 = 64;
pub const MAX_RESOLUTION: i64 = 2048;
pub const MAX_INFERENCE_PIXELS: i64 = 4_000_000;
pub const TARGET_PIXELS: f64 = 1_048_576.0;

/// Bir bölgenin kırpıntısı: sayfa içinde `[x, y, x + w, y + h)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CropPlan {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub padding: u32,
    pub blur_radius: u32,
}

fn snap(dim: i64, m: i64) -> i64 {
    (dim.max(MIN_RESOLUTION) / m) * m
}

/// MT `_expand_bounds_to_min_size` (kutuyu en az 64 px'e, sayfa içinde genişletir).
fn expand_to_min(mut x1: i64, mut y1: i64, mut x2: i64, mut y2: i64, iw: i64, ih: i64) -> (i64, i64, i64, i64) {
    let tw = MIN_RESOLUTION.min(iw);
    let th = MIN_RESOLUTION.min(ih);
    if x2 - x1 < tw {
        let extra = tw - (x2 - x1);
        x1 = (x1 - extra / 2).max(0);
        x2 = (x2 + extra - extra / 2).min(iw);
        if x2 - x1 < tw {
            if x1 == 0 {
                x2 = iw.min(tw);
            } else {
                x1 = (iw - tw).max(0);
            }
        }
    }
    if y2 - y1 < th {
        let extra = th - (y2 - y1);
        y1 = (y1 - extra / 2).max(0);
        y2 = (y2 + extra - extra / 2).min(ih);
        if y2 - y1 < th {
            if y1 == 0 {
                y2 = ih.min(th);
            } else {
                y1 = (ih - th).max(0);
            }
        }
    }
    (x1, y1, x2, y2)
}

/// Bileşen kutusu `bbox = [x_min, y_min, x_max, y_max]` (ikisi de DAHİL) için kırpıntı planı (MT `inpaint_mask` L1487-1525).
pub fn plan_crop(bbox: [u32; 4], img_w: u32, img_h: u32, multiple: u32) -> Option<CropPlan> {
    let m = multiple.max(1) as i64;
    let (iw, ih) = (img_w as i64, img_h as i64);
    let (x_min, y_min, x_max, y_max) = (bbox[0] as i64, bbox[1] as i64, bbox[2] as i64, bbox[3] as i64);
    let big = (x_max - x_min).max(y_max - y_min);
    let padding_pixels = (big as f64 * CONTEXT_PADDING_RATIO) as i64;
    let padding = (padding_pixels.min(MAX_CONTEXT_PADDING) as f64 * PADDING_MULTIPLIER) as i64;
    let blur = ((big as f64 * BLUR_SCALE_FACTOR) as i64).clamp(MIN_BLUR_RADIUS, MAX_BLUR_RADIUS);
    let x1 = (x_min - padding).max(0);
    let y1 = (y_min - padding).max(0);
    let x2 = (x_max + 1 + padding).min(iw);
    let y2 = (y_max + 1 + padding).min(ih);
    let (mut x1, mut y1, x2, y2) = expand_to_min(x1, y1, x2, y2, iw, ih);
    let width = snap(x2 - x1, m).min(iw);
    let height = snap(y2 - y1, m).min(ih);
    if x1 + width > iw {
        x1 = (iw - width).max(0);
    }
    if y1 + height > ih {
        y1 = (ih - height).max(0);
    }
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(CropPlan { x: x1 as u32, y: y1 as u32, w: width as u32, h: height as u32, padding: padding as u32, blur_radius: blur as u32 })
}

/// MT `_prepare_image_for_inference` (upscale_small_crops = açık): ~1 MP, en-boy korunur, her kenar ≤ 2048, 32'nin katı, ≤ 4 MP.
pub fn inference_size(w: u32, h: u32, multiple: u32) -> (u32, u32) {
    let m = multiple.max(1) as i64;
    let (ow, oh) = (w as i64, h as i64);
    let cur = ow * oh;
    let mut scale = if cur <= 0 { 1.0 } else { (TARGET_PIXELS / cur as f64).sqrt() };
    if cur > 0 {
        scale = scale.min(MAX_RESOLUTION as f64 / ow as f64).min(MAX_RESOLUTION as f64 / oh as f64);
    }
    let q = |d: i64| snap(d.min(MAX_RESOLUTION), m);
    let mut nw = q((ow as f64 * scale) as i64);
    let mut nh = q((oh as f64 * scale) as i64);
    while nw * nh > MAX_INFERENCE_PIXELS {
        if nw >= nh && nw > MIN_RESOLUTION {
            nw -= m;
        } else if nh > MIN_RESOLUTION {
            nh -= m;
        } else {
            break;
        }
    }
    (nw as u32, nh as u32)
}

/// Görüntüyü `w`x`h`'ye Lanczos ile ölçekler (boyut aynıysa kopya).
pub fn scale_to(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    if img.dimensions() == (w, h) {
        img.clone()
    } else {
        image::imageops::resize(img, w, h, image::imageops::FilterType::Lanczos3)
    }
}

/// Kırpıntıdaki sert bileşen maskesi, bileşen kutusuna (`clip = [x0, y0, x1, y1)`, sayfa koordinatı) kırpılmış
/// (MT: `_feathered_crop_mask` * sert maske [strict_mask_clipping], sonra `_clip_composite_mask`; tüy sert maskeyle çarpılınca yok olur,
/// geriye maske ∩ kutu kalır). `mask`: kırpıntı maskesi (>= 128).
pub fn composite_mask(mask: &image::GrayImage, plan: &CropPlan, clip: [u32; 4]) -> Vec<bool> {
    let (w, h) = mask.dimensions();
    let mut out = vec![false; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let (gx, gy) = (plan.x + x, plan.y + y);
            if mask.get_pixel(x, y)[0] >= 128 && gx >= clip[0] && gx < clip[2] && gy >= clip[1] && gy < clip[3] {
                out[(y * w + x) as usize] = true;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_geometry_matches_mangatranslator() {
        // kutu 100x40 (x 500..=599, y 700..=739): dolgu = int(min(int(99*0.5), 80) * 2) = 98
        let p = plan_crop([500, 700, 599, 739], 1115, 1600, 32).unwrap();
        assert_eq!(p.padding, 98);
        assert_eq!(p.blur_radius, 9); // int(99 * 0.1)
        // x1 = 402, x2 = 698 -> 296 -> 288; y1 = 602, y2 = 838 -> 236 -> 224
        assert_eq!((p.x, p.y, p.w, p.h), (402, 602, 288, 224));
        // dolgu tavanı: büyük kutu -> 80 * 2
        let p = plan_crop([100, 100, 899, 499], 1115, 1600, 32).unwrap();
        assert_eq!(p.padding, 160);
        assert_eq!(p.blur_radius, 10);
        assert_eq!((p.w % 32, p.h % 32), (0, 0));
    }

    #[test]
    fn crop_min_size_and_stays_inside_page() {
        let p = plan_crop([0, 0, 0, 0], 1115, 1600, 32).unwrap();
        assert_eq!((p.x, p.y, p.w, p.h), (0, 0, 64, 64));
        assert_eq!(p.blur_radius, 1);
        let p = plan_crop([1110, 1595, 1114, 1599], 1115, 1600, 32).unwrap();
        assert!(p.x + p.w <= 1115 && p.y + p.h <= 1600);
        assert!(p.w >= 64 && p.h >= 64);
        let p = plan_crop([900, 300, 1100, 400], 1115, 1600, 32).unwrap();
        assert!(p.x + p.w <= 1115);
        assert_eq!(p.w % 32, 0);
        // MT gibi aşağı yuvarlama kutunun uç kenarını kesebilir (x 740..1092); kutunun başı kırpıntı içinde
        assert_eq!((p.x, p.w), (740, 352));
        let p = plan_crop([10, 10, 20, 20], 50, 40, 32).unwrap();
        assert!(p.x + p.w <= 50 && p.y + p.h <= 40);
    }

    #[test]
    fn inference_size_is_about_one_megapixel_in_multiples_of_32() {
        for (w, h) in [(288, 224), (64, 64), (160, 800), (1024, 1024), (500, 333), (2000, 300), (64, 1600)] {
            let (sw, sh) = inference_size(w, h, 32);
            assert_eq!((sw % 32, sh % 32), (0, 0), "{w}x{h}");
            assert!(sw <= 2048 && sh <= 2048 && sw * sh <= 4_000_000, "{w}x{h} -> {sw}x{sh}");
        }
        // 288x224: ölçek sqrt(1048576/64512) = 4.0316 -> 1161x903 -> 1152x896
        assert_eq!(inference_size(288, 224, 32), (1152, 896));
        assert_eq!(inference_size(1024, 1024, 32), (1024, 1024));
        let (sw, sh) = inference_size(500, 333, 32);
        let a = (sw * sh) as f64 / TARGET_PIXELS;
        assert!((0.9..=1.05).contains(&a), "{sw}x{sh} {a}");
        // 2048 tavanı: 64x1600 -> ölçek min(, 2048/1600)
        let (_, sh) = inference_size(64, 1600, 32);
        assert!(sh <= 2048);
    }

    #[test]
    fn composite_mask_is_hard_mask_clipped_to_bbox() {
        let plan = CropPlan { x: 10, y: 20, w: 8, h: 8, padding: 0, blur_radius: 1 };
        let mut m = image::GrayImage::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                m.put_pixel(x, y, image::Luma([255]));
            }
        }
        // kutu yalnız sol yarıyı kapsar (sayfa x 12..14)
        let c = composite_mask(&m, &plan, [12, 22, 14, 26]);
        assert_eq!(c.iter().filter(|&&v| v).count(), 2 * 4);
        assert!(c[(2 * 8 + 2) as usize] && !c[(2 * 8 + 4) as usize]);
    }
}
