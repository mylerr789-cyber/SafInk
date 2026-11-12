//! Difüzyon (Flux/Klein) kırpıntı çıktısı son işlemi: MangaTranslator'ın (meangrinch, commit 047d381) ışıklılık eşleme,
//! "sanatı koru" glif kompoziti ve ton eşlemesinin Rust karşılığı. Sabitler ve adımlar kaynakla aynıdır; kaynak satırları yorumlardadır.
//! Fark: MT glif olasılık modeliyle (SAM-TS) glifleri bulur; burada model yok, glifler bileşenin çevresindeki zeminden
//! ayrılan "mürekkep" pikselleridir (`INK_DEVIATION`). LaMa çıktısına bu adımlar uygulanmaz (MT de uygulamaz).
//! Koharu (inpainting.rs `composite_generated`) ton eşleme/tüy yapmaz: sert yapıştırır; LaMa kutucukları öyle yapıştırılır.
use image::{GrayImage, RgbImage};

/// MT core/image/inpainting.py L318 `_match_luminance(boundary_width=12)`.
pub const BOUNDARY_WIDTH: i32 = 12;
/// MT core/image/glyph_mask.py L12-L22.
pub const REFERENCE_SHORT_SIDE: f32 = 830.0;
pub const REFERENCE_DILATION: f32 = 7.0;
pub const MIN_DILATION: i32 = 2;
/// glyph_mask.py L28-L33.
pub const TONE_FLAT_RANGE: u8 = 12;
pub const TONE_FLAT_WINDOW: i32 = 7;
pub const TONE_MATCH_TOLERANCE: f32 = 24.0;
pub const TONE_MATCH_MIN_SAMPLES: f32 = 16.0;
pub const TONE_SIGMA_RADII: f32 = 3.0;
/// Glif türetme (MT'de glif modeli): zemin ortancasından en çok bir kanalda bu kadar uzak piksel mürekkeptir.
pub const INK_DEVIATION: i32 = 40;

/// Dev/ölçüm anahtarı `SAFINK_POST`: `none` (sert yapıştır), `lum` (yalnızca ışıklılık), varsayılan `full`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Post {
    None,
    Lum,
    Full,
}
impl Post {
    pub fn from_env() -> Post {
        match std::env::var("SAFINK_POST").as_deref() {
            Ok("none") => Post::None,
            Ok("lum") => Post::Lum,
            _ => Post::Full,
        }
    }
}

/// glyph_mask.py `glyph_dilation_radius`: max(MIN_DILATION, round(7 * kısa_kenar / 830)).
pub fn glyph_radius(page_w: u32, page_h: u32) -> i32 {
    ((REFERENCE_DILATION * page_w.min(page_h) as f32 / REFERENCE_SHORT_SIDE).round() as i32).max(MIN_DILATION)
}

// ---------------- OpenCV tarzı 8 bit LAB ----------------

fn lin(u: u8) -> f32 {
    let c = u as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
fn f_xyz(t: f32) -> f32 {
    if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 }
}
fn finv(t: f32) -> f32 {
    if t > 0.206893 { t * t * t } else { (t - 16.0 / 116.0) / 7.787 }
}
const XN: f32 = 0.950456;
const ZN: f32 = 1.088754;

/// RGB -> OpenCV 8 bit LAB (L*255/100, a+128, b+128), tam sayıya yuvarlanmış.
pub fn rgb_to_lab8(p: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (lin(p[0]), lin(p[1]), lin(p[2]));
    let x = (0.412453 * r + 0.357580 * g + 0.180423 * b) / XN;
    let y = 0.212671 * r + 0.715160 * g + 0.072169 * b;
    let z = (0.019334 * r + 0.119193 * g + 0.950227 * b) / ZN;
    let (fx, fy, fz) = (f_xyz(x), f_xyz(y), f_xyz(z));
    let l = 116.0 * fy - 16.0;
    [(l * 255.0 / 100.0).round(), (500.0 * (fx - fy) + 128.0).round(), (200.0 * (fy - fz) + 128.0).round()]
}

pub fn lab8_to_rgb(l8: [u8; 3]) -> [u8; 3] {
    let l = l8[0] as f32 * 100.0 / 255.0;
    let (a, b) = (l8[1] as f32 - 128.0, l8[2] as f32 - 128.0);
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let (x, y, z) = (finv(fx) * XN, finv(fy), finv(fz) * ZN);
    let r = 3.240479 * x - 1.537150 * y - 0.498535 * z;
    let g = -0.969256 * x + 1.875991 * y + 0.041556 * z;
    let bl = 0.055648 * x - 0.204043 * y + 1.057311 * z;
    let gam = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [gam(r), gam(g), gam(bl)]
}

// ---------------- yardımcılar ----------------

/// Kare (Chebyshev) genişletme: cv2.dilate(MORPH_RECT, 2r+1), ayrık.
fn dilate_rect(m: &[bool], w: usize, h: usize, r: i32) -> Vec<bool> {
    let mut tmp = vec![false; m.len()];
    for y in 0..h {
        let mut last: i64 = i64::MIN / 2;
        // soldan: son görülen true konumu
        let mut near_l = vec![i64::MIN / 2; w];
        for x in 0..w {
            if m[y * w + x] {
                last = x as i64;
            }
            near_l[x] = last;
        }
        let mut next: i64 = i64::MAX / 2;
        for x in (0..w).rev() {
            if m[y * w + x] {
                next = x as i64;
            }
            tmp[y * w + x] = (x as i64 - near_l[x]) <= r as i64 || (next - x as i64) <= r as i64;
        }
    }
    let mut out = vec![false; m.len()];
    for x in 0..w {
        let mut near_u = vec![i64::MIN / 2; h];
        let mut last: i64 = i64::MIN / 2;
        for y in 0..h {
            if tmp[y * w + x] {
                last = y as i64;
            }
            near_u[y] = last;
        }
        let mut next: i64 = i64::MAX / 2;
        for y in (0..h).rev() {
            if tmp[y * w + x] {
                next = y as i64;
            }
            out[y * w + x] = (y as i64 - near_u[y]) <= r as i64 || (next - y as i64) <= r as i64;
        }
    }
    out
}

fn mean_where(vals: impl Iterator<Item = f32>) -> f32 {
    let (mut s, mut n) = (0f64, 0usize);
    for v in vals {
        s += v as f64;
        n += 1;
    }
    if n == 0 { 0.0 } else { (s / n as f64) as f32 }
}

/// MT inpainting.py L318-L398 `_match_luminance`: 12 px sınır şeridi (< 32 px ise tüm maske dışı) referansıyla, yalnızca maske
/// piksellerine ekleyen L kayması (|kayma| ≥ 1); şerit tek renkliyse (ort. |a-128| < 3 ve |b-128| < 3) a=b=128, değilse a/b
/// ortalama kayması (|kayma| > 1). `gen` kırpıntı boyutunda olmalı.
pub fn match_luminance(orig: &RgbImage, gen: &RgbImage, mask: &GrayImage) -> RgbImage {
    let (w, h) = (orig.width() as usize, orig.height() as usize);
    let mb: Vec<bool> = mask.as_raw().iter().map(|&v| v >= 128).collect();
    if !mb.iter().any(|&v| v) {
        return gen.clone();
    }
    let dil = dilate_rect(&mb, w, h, BOUNDARY_WIDTH);
    let band: Vec<bool> = dil.iter().zip(&mb).map(|(&d, &m)| d && !m).collect();
    let reference: Vec<bool> = if band.iter().filter(|&&v| v).count() >= 32 { band } else { mb.iter().map(|&m| !m).collect() };
    if !reference.iter().any(|&v| v) {
        return gen.clone();
    }
    // MT kayması bölge kutusunun tamamına uygular (kutu = metinden büyük, korunan sanatı da içerir); burada bölge sıkı bir metin
    // maskesidir. Maskenin 12 px şeridi de kaydırılır ki glif kompoziti (bkz. `composite_glyphs`) şeritteki düz zemin
    // pikselleriyle kaymayı ölçüp düzeltebilsin. Şerit pikselleri sayfaya yazılmaz.
    let apply: Vec<bool> = dil.iter().zip(&mb).map(|(&d, &m)| d || m).collect();
    let ol: Vec<[f32; 3]> = orig.pixels().map(|p| rgb_to_lab8(p.0)).collect();
    let mut gl: Vec<[f32; 3]> = gen.pixels().map(|p| rgb_to_lab8(p.0)).collect();
    let sel = |v: &Vec<[f32; 3]>, c: usize| mean_where(v.iter().zip(&reference).filter(|(_, &r)| r).map(move |(p, _)| p[c]));
    let l_shift = sel(&ol, 0) - sel(&gl, 0);
    if l_shift.abs() >= 1.0 {
        for (p, &m) in gl.iter_mut().zip(&apply) {
            if m {
                p[0] = (p[0] + l_shift).clamp(0.0, 255.0);
            }
        }
    }
    let a_dev = mean_where(ol.iter().zip(&reference).filter(|(_, &r)| r).map(|(p, _)| (p[1] - 128.0).abs()));
    let b_dev = mean_where(ol.iter().zip(&reference).filter(|(_, &r)| r).map(|(p, _)| (p[2] - 128.0).abs()));
    if a_dev < 3.0 && b_dev < 3.0 {
        for (p, &m) in gl.iter_mut().zip(&apply) {
            if m {
                p[1] = 128.0;
                p[2] = 128.0;
            }
        }
    } else {
        for c in 1..3 {
            let shift = sel(&ol, c) - sel(&gl, c);
            if shift.abs() > 1.0 {
                for (p, &m) in gl.iter_mut().zip(&apply) {
                    if m {
                        p[c] = (p[c] + shift).clamp(0.0, 255.0);
                    }
                }
            }
        }
    }
    let mut out = gen.clone();
    for ((o, p), &m) in out.pixels_mut().zip(&gl).zip(&apply) {
        // MT: gen_lab.astype(np.uint8) (kesme); maske + şerit dışındaki pikseller olduğu gibi kalır
        if m {
            o.0 = lab8_to_rgb([p[0] as u8, p[1] as u8, p[2] as u8]);
        }
    }
    out
}

/// Tam öklid uzaklık dönüşümü (Felzenszwalb): her pikselin en yakın `zero` (true) pikseline uzaklığı. Hiç sıfır yoksa büyük değer.
fn edt(zero: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e12;
    let mut d: Vec<f32> = zero.iter().map(|&z| if z { 0.0 } else { INF }).collect();
    let pass = |f: &[f32], n: usize| -> Vec<f32> {
        let mut out = vec![0f32; n];
        let mut v = vec![0usize; n];
        let mut z = vec![0f32; n + 1];
        let mut k = 0usize;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..n {
            loop {
                let p = v[k];
                let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
                if s <= z[k] {
                    if k == 0 {
                        v[0] = q;
                        z[0] = -INF;
                        z[1] = INF;
                        break;
                    }
                    k -= 1;
                } else {
                    k += 1;
                    v[k] = q;
                    z[k] = s;
                    z[k + 1] = INF;
                    break;
                }
            }
        }
        k = 0;
        for q in 0..n {
            while z[k + 1] < q as f32 {
                k += 1;
            }
            let p = v[k];
            out[q] = (q as f32 - p as f32).powi(2) + f[p];
        }
        out
    };
    for x in 0..w {
        let col: Vec<f32> = (0..h).map(|y| d[y * w + x]).collect();
        let r = pass(&col, h);
        for y in 0..h {
            d[y * w + x] = r[y];
        }
    }
    for y in 0..h {
        let r = pass(&d[y * w..(y + 1) * w].to_vec(), w);
        d[y * w..(y + 1) * w].copy_from_slice(&r);
    }
    d.iter().map(|v| v.sqrt()).collect()
}

/// Ayrık gauss (cv2.GaussianBlur(sigma), kenar: yansıma-101), tek kanal.
fn gauss(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.0).ceil().max(1.0) as i32;
    let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = k.iter().sum();
    let refl = |i: i32, n: i32| -> usize {
        if n == 1 {
            return 0;
        }
        let mut i = i;
        loop {
            if i < 0 {
                i = -i;
            } else if i >= n {
                i = 2 * (n - 1) - i;
            } else {
                return i as usize;
            }
        }
    };
    let mut tmp = vec![0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut a = 0f32;
            for (j, kv) in k.iter().enumerate() {
                a += kv * src[y * w + refl(x as i32 + j as i32 - r, w as i32)];
            }
            tmp[y * w + x] = a / sum;
        }
    }
    let mut out = vec![0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut a = 0f32;
            for (j, kv) in k.iter().enumerate() {
                a += kv * tmp[refl(y as i32 + j as i32 - r, h as i32) * w + x];
            }
            out[y * w + x] = a / sum;
        }
    }
    out
}

/// glyph_mask.py L277-L289 `_is_flat`: 7x7 pencerede (max-min), kanallar üzerinde en büyük < 12.
fn is_flat(img: &[[u8; 3]], w: usize, h: usize) -> Vec<bool> {
    let r = TONE_FLAT_WINDOW / 2;
    let mut out = vec![true; w * h];
    for c in 0..3 {
        let mut mn = vec![255u8; w * h];
        let mut mx = vec![0u8; w * h];
        let mut tmn = vec![255u8; w * h];
        let mut tmx = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let (mut a, mut b) = (255u8, 0u8);
                for dx in -r..=r {
                    let xx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let v = img[y * w + xx][c];
                    a = a.min(v);
                    b = b.max(v);
                }
                tmn[y * w + x] = a;
                tmx[y * w + x] = b;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let (mut a, mut b) = (255u8, 0u8);
                for dy in -r..=r {
                    let yy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    a = a.min(tmn[yy * w + x]);
                    b = b.max(tmx[yy * w + x]);
                }
                mn[y * w + x] = a;
                mx[y * w + x] = b;
            }
        }
        for i in 0..w * h {
            if mx[i] - mn[i] >= TONE_FLAT_RANGE {
                out[i] = false;
            }
        }
    }
    out
}

/// glyph_mask.py L292-L313 `_match_tone`: `after`, `samples` üzerinde ölçülen yumuşatılmış before-after farkıyla kaydırılır.
fn match_tone(before: &[[u8; 3]], after: &[[u8; 3]], samples: &[bool], w: usize, h: usize, sigma: f32) -> Vec<[f32; 3]> {
    let (fb, fa) = (is_flat(before, w, h), is_flat(after, w, h));
    let n = w * h;
    let diff: Vec<[f32; 3]> = (0..n).map(|i| [0, 1, 2].map(|c| before[i][c] as f32 - after[i][c] as f32)).collect();
    let weight: Vec<f32> = (0..n)
        .map(|i| if samples[i] && fb[i] && fa[i] && diff[i].iter().map(|v| v.abs()).fold(0.0, f32::max) < TONE_MATCH_TOLERANCE { 1.0 } else { 0.0 })
        .collect();
    let wsum: f32 = weight.iter().sum();
    let after_f: Vec<[f32; 3]> = after.iter().map(|p| p.map(|v| v as f32)).collect();
    if wsum < TONE_MATCH_MIN_SAMPLES {
        return after_f;
    }
    let mut goff = [0f32; 3];
    for i in 0..n {
        for c in 0..3 {
            goff[c] += diff[i][c] * weight[i];
        }
    }
    for c in 0..3 {
        goff[c] /= wsum;
    }
    let density = gauss(&weight, w, h, sigma);
    let mut local: Vec<Vec<f32>> = Vec::new();
    for c in 0..3 {
        let ch: Vec<f32> = (0..n).map(|i| diff[i][c] * weight[i]).collect();
        local.push(gauss(&ch, w, h, sigma));
    }
    (0..n)
        .map(|i| {
            let trust = (density[i] / 0.05).clamp(0.0, 1.0);
            let dn = density[i].max(1e-6);
            [0, 1, 2].map(|c| after_f[i][c] + trust * (local[c][i] / dn) + (1.0 - trust) * goff[c])
        })
        .collect()
}

/// MT glyph_mask.py L316-L390 `composite_glyphs`, bileşen başına: `own` bu kutucuğun sahip olduğu bileşen pikselleri (etiket `labels`),
/// `before` kırpıntının mevcut hali, `after` ışıklılığı eşlenmiş model çıktısı, `crop_mask` kırpıntıdaki tüm bekleyen maske.
/// Dönen görüntüde yalnızca `own` pikselleri anlamlıdır. Bileşen dışı her şey özgün kalır.
pub fn composite_glyphs(before: &RgbImage, after: &RgbImage, crop_mask: &GrayImage, own: &[bool], labels: &[u32], radius: i32) -> RgbImage {
    let (w, h) = (before.width() as usize, before.height() as usize);
    let mut out = before.clone();
    let feather = ((radius / 2).max(1)) as f32;
    let mut ids: Vec<u32> = (0..w * h).filter(|&i| own[i]).map(|i| labels[i]).collect();
    ids.sort_unstable();
    ids.dedup();
    let cm: Vec<bool> = crop_mask.as_raw().iter().map(|&v| v >= 128).collect();
    for id in ids {
        // bileşenin sınır kutusu
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
        for y in 0..h {
            for x in 0..w {
                if own[y * w + x] && labels[y * w + x] == id {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        if x1 <= x0 {
            continue;
        }
        // referans zemin: bileşenin 12 px dışındaki, hiçbir maskeye ait olmayan şerit (yoksa tüm maske dışı) -> kanal ortancası
        let region_full: Vec<bool> = (0..w * h).map(|i| own[i] && labels[i] == id).collect();
        let dil = dilate_rect(&region_full, w, h, BOUNDARY_WIDTH);
        let mut band: Vec<usize> = (0..w * h).filter(|&i| dil[i] && !cm[i]).collect();
        if band.len() < 32 {
            band = (0..w * h).filter(|&i| !cm[i]).collect();
        }
        let med: Option<[i32; 3]> = if band.is_empty() {
            None
        } else {
            let mut m = [0i32; 3];
            for c in 0..3 {
                let mut v: Vec<u8> = band.iter().map(|&i| before.as_raw()[i * 3 + c]).collect();
                v.sort_unstable();
                m[c] = v[v.len() / 2] as i32;
            }
            Some(m)
        };
        let (bw, bh) = (x1 - x0, y1 - y0);
        let at = |x: usize, y: usize| (y0 + y) * w + x0 + x;
        let region: Vec<bool> = (0..bw * bh).map(|k| region_full[at(k % bw, k / bw)]).collect();
        // glifler: zeminden ayrılan mürekkep, bileşen içinde (+ yarıçap kadar genişletilmiş); yoksa tüm bileşen
        let ink: Vec<bool> = (0..bw * bh)
            .map(|k| {
                let i = at(k % bw, k / bw);
                region[k] && med.map(|m| (0..3).any(|c| (before.as_raw()[i * 3 + c] as i32 - m[c]).abs() > INK_DEVIATION)).unwrap_or(false)
            })
            .collect();
        let ink_img = GrayImage::from_fn(bw as u32, bh as u32, |x, y| image::Luma([if ink[y as usize * bw + x as usize] { 255 } else { 0 }]));
        let gd = crate::maskops::dilate(&ink_img, radius as u32);
        let mut glyphs: Vec<bool> = (0..bw * bh).map(|k| gd.as_raw()[k] >= 128 && region[k]).collect();
        if !glyphs.iter().any(|&v| v) {
            glyphs = region.clone();
        }
        let b_crop: Vec<[u8; 3]> = (0..bw * bh).map(|k| {
            let i = at(k % bw, k / bw);
            [before.as_raw()[i * 3], before.as_raw()[i * 3 + 1], before.as_raw()[i * 3 + 2]]
        }).collect();
        let all_glyph = (0..bw * bh).all(|k| glyphs[k] == region[k]);
        // Ton eşleme örnekleri (MT `composite_glyphs`: `region & ~glyphs`, yani kutunun korunan sanatı). MT'nin bölgesi metinden büyük
        // bir kutudur; buradaki bölge sıkı bir metin maskesi olduğundan çoğu piksel glif sayılır ve örnek kalmaz: ışıklılık kayması
        // (bkz. `match_luminance`) düzeltilmeden sayfaya geçer ve balondaki / rozetteki "hayalet" olur. Bu yüzden maskenin hemen
        // dışındaki `BOUNDARY_WIDTH` şeridi (yazı dışı, bekleyen maske dışı, aynı düz-ve-uyumlu süzgeçten geçen pikseller) de örnektir.
        let mut samples = vec![false; w * h];
        for i in 0..w * h {
            samples[i] = dil[i] && !cm[i];
        }
        for k in 0..bw * bh {
            if region[k] && !glyphs[k] {
                samples[at(k % bw, k / bw)] = true;
            }
        }
        let b_full: Vec<[u8; 3]> = before.as_raw().chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        let a_full: Vec<[u8; 3]> = after.as_raw().chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        let tone = match_tone(&b_full, &a_full, &samples, w, h, TONE_SIGMA_RADII * radius as f32);
        let redraw: Vec<[f32; 3]> = (0..bw * bh).map(|k| tone[at(k % bw, k / bw)]).collect();
        let alpha: Vec<f32> = if all_glyph {
            region.iter().map(|&r| if r { 1.0 } else { 0.0 }).collect()
        } else {
            // korunan sanata uzaklık (kutu kenarında kesilen glifler tam çizilmiş kalır): sıfırlar = bölge & !glif
            let zeros: Vec<bool> = (0..bw * bh).map(|k| region[k] && !glyphs[k]).collect();
            let inside = edt(&zeros, bw, bh);
            (0..bw * bh).map(|k| if glyphs[k] { (inside[k] / feather).clamp(0.0, 1.0) } else { 0.0 }).collect()
        };
        for k in 0..bw * bh {
            if !region[k] {
                continue;
            }
            let i = at(k % bw, k / bw);
            let p = out.get_pixel_mut((i % w) as u32, (i / w) as u32);
            for c in 0..3 {
                let v = redraw[k][c] * alpha[k] + b_crop[k][c] as f32 * (1.0 - alpha[k]);
                p[c] = v.clamp(0.0, 255.0).round() as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn lab_roundtrip_is_close() {
        for p in [[0u8, 0, 0], [255, 255, 255], [200, 30, 90], [128, 128, 128], [10, 200, 250]] {
            let l = rgb_to_lab8(p);
            let r = lab8_to_rgb([l[0] as u8, l[1] as u8, l[2] as u8]);
            for c in 0..3 {
                assert!((r[c] as i32 - p[c] as i32).abs() <= 10, "{p:?} -> {l:?} -> {r:?}");
            }
        }
        // OpenCV: beyaz L=255, nötr a=b=128
        let w = rgb_to_lab8([255, 255, 255]);
        assert_eq!((w[0], w[1], w[2]), (255.0, 128.0, 128.0));
    }

    #[test]
    fn luminance_match_shifts_masked_pixels_and_neutralises_gray() {
        let orig = RgbImage::from_pixel(60, 60, Rgb([200, 200, 200]));
        let mut gen = RgbImage::from_pixel(60, 60, Rgb([170, 180, 170])); // koyu + yeşilimsi
        let mut mask = GrayImage::new(60, 60);
        for y in 20..40 {
            for x in 20..40 {
                mask.put_pixel(x, y, image::Luma([255]));
            }
        }
        let out = match_luminance(&orig, &gen, &mask);
        let p = out.get_pixel(30, 30);
        assert!((p[0] as i32 - 200).abs() <= 4 && p[0].abs_diff(p[1]) <= 2 && p[1].abs_diff(p[2]) <= 2, "{p:?}");
        assert_eq!(out.get_pixel(5, 5), gen.get_pixel(5, 5), "maske dışı değişmemeli");
        gen.put_pixel(0, 0, Rgb([0, 0, 0]));
    }

    #[test]
    fn edt_matches_brute_force() {
        let (w, h) = (23usize, 17usize);
        let zero: Vec<bool> = (0..w * h).map(|i| (i * 7919) % 31 == 0).collect();
        let d = edt(&zero, w, h);
        for y in 0..h {
            for x in 0..w {
                let mut best = f32::MAX;
                for yy in 0..h {
                    for xx in 0..w {
                        if zero[yy * w + xx] {
                            best = best.min((((x as i32 - xx as i32).pow(2) + (y as i32 - yy as i32).pow(2)) as f32).sqrt());
                        }
                    }
                }
                assert!((d[y * w + x] - best).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn composite_keeps_art_away_from_glyphs() {
        // gri zemin, bileşende ince yazı (koyu) + yazıdan uzak bir sanat çizgisi; model her şeyi (çizgi dahil) siler
        let mut before = RgbImage::from_pixel(200, 120, Rgb([220, 220, 220]));
        for x in 20..60 {
            for y in 20..26 {
                before.put_pixel(x, y, Rgb([10, 10, 10]));
            }
        }
        for y in 20..100 {
            before.put_pixel(150, y, Rgb([185, 185, 185]));
        } // uzak soluk sanat çizgisi (zeminden < 40 fark: mürekkep sayılmaz)
        let after = RgbImage::from_pixel(200, 120, Rgb([221, 221, 221]));
        let mut mask = GrayImage::new(200, 120);
        let mut own = vec![false; 200 * 120];
        let mut labels = vec![0u32; 200 * 120];
        for y in 15usize..105 {
            for x in 15usize..160 {
                mask.put_pixel(x as u32, y as u32, image::Luma([255]));
                own[y * 200 + x] = true;
                labels[y * 200 + x] = 1;
            }
        }
        let out = composite_glyphs(&before, &after, &mask, &own, &labels, 3);
        assert!(out.get_pixel(30, 22)[0] >= 215, "yazı silinmeli");
        assert_eq!(*out.get_pixel(150, 90), Rgb([185, 185, 185]), "uzak sanat korunmalı");
        assert_eq!(out.get_pixel(5, 5), before.get_pixel(5, 5));
    }

    #[test]
    fn tight_text_mask_in_white_balloon_leaves_no_grey_ghost() {
        // beyaz balon + yazı; maskenin 12 px şeridinde balon çizgisi var (ışıklılık kaymasını eksiye çeker); model tertemiz beyaz çizer
        let mut before = RgbImage::from_pixel(120, 80, Rgb([255, 255, 255]));
        for y in 0..80 {
            for x in 0..6 {
                before.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        let mut mask = GrayImage::new(120, 80);
        let mut own = vec![false; 120 * 80];
        let mut labels = vec![0u32; 120 * 80];
        for y in 30usize..50 {
            for x in 14usize..100 {
                before.put_pixel(x as u32, y as u32, Rgb([0, 0, 0]));
                mask.put_pixel(x as u32, y as u32, image::Luma([255]));
                own[y * 120 + x] = true;
                labels[y * 120 + x] = 1;
            }
        }
        let gen = RgbImage::from_pixel(120, 80, Rgb([255, 255, 255]));
        let lum = match_luminance(&before, &gen, &mask);
        assert!(lum.get_pixel(50, 40)[0] < 252, "ön koşul: kayma balonu grileştirir");
        let out = composite_glyphs(&before, &lum, &mask, &own, &labels, 9);
        let p = out.get_pixel(50, 40);
        assert!(p[0] >= 253, "hayalet kalmamalı: {p:?}");
    }
}
