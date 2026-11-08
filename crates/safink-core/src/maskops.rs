//! Maske işlemleri: birleştirme (API ∪ eklenen − silinen), genişletme, bileşen/grup bulma.
//! `MaskProcessor` ileride daha iyi maske işleme için takılabilir kalır.
use image::GrayImage;

/// Düzenleme katmanı değerleri (L8): 0 = yok, 1 = ekle, 2 = sil.
pub const EDIT_NONE: u8 = 0;
pub const EDIT_ADD: u8 = 1;
pub const EDIT_REMOVE: u8 = 2;

/// Son maske = (API ∪ eklenen) − silinen. İkili (0/255) döner.
pub fn combine(api: Option<&GrayImage>, edit: Option<&GrayImage>, w: u32, h: u32) -> GrayImage {
    let mut out = GrayImage::new(w, h);
    if let Some(a) = api {
        let a = if a.dimensions() == (w, h) {
            std::borrow::Cow::Borrowed(a)
        } else {
            std::borrow::Cow::Owned(image::imageops::resize(a, w, h, image::imageops::FilterType::Nearest))
        };
        for (o, p) in out.as_mut().iter_mut().zip(a.as_raw().iter()) {
            *o = if *p >= 128 { 255 } else { 0 };
        }
    }
    if let Some(e) = edit {
        if e.dimensions() == (w, h) {
            for (o, p) in out.as_mut().iter_mut().zip(e.as_raw().iter()) {
                match *p {
                    EDIT_ADD => *o = 255,
                    EDIT_REMOVE => *o = 0,
                    _ => {}
                }
            }
        }
    }
    out
}

/// Disk yapı elemanıyla genişletme (r px). Yalnızca kenar piksellerine disk basar, bu yüzden uzun şeritlerde hızlıdır.
pub fn dilate(mask: &GrayImage, r: u32) -> GrayImage {
    if r == 0 {
        return mask.clone();
    }
    let (w, h) = mask.dimensions();
    let (wi, hi) = (w as i32, h as i32);
    let ri = r as i32;
    // disk için her dy'de dx yarıçapı
    let spans: Vec<i32> = (-ri..=ri).map(|dy| (((ri * ri - dy * dy) as f32 + 0.25).sqrt()).floor() as i32).collect();
    let src = mask.as_raw();
    let mut out = src.clone();
    let at = |x: i32, y: i32| -> bool { x >= 0 && y >= 0 && x < wi && y < hi && src[(y * wi + x) as usize] >= 128 };
    for y in 0..hi {
        for x in 0..wi {
            if src[(y * wi + x) as usize] < 128 {
                continue;
            }
            // kenar mı?
            if at(x - 1, y) && at(x + 1, y) && at(x, y - 1) && at(x, y + 1) {
                continue;
            }
            for (i, dy) in (-ri..=ri).enumerate() {
                let yy = y + dy;
                if yy < 0 || yy >= hi {
                    continue;
                }
                let s = spans[i];
                let x0 = (x - s).max(0);
                let x1 = (x + s).min(wi - 1);
                let row = (yy * wi) as usize;
                for xx in x0..=x1 {
                    out[row + xx as usize] = 255;
                }
            }
        }
    }
    GrayImage::from_raw(w, h, out).unwrap()
}

#[derive(Clone, Copy, Debug)]
pub struct Rect { pub x0: i32, pub y0: i32, pub x1: i32, pub y1: i32 }
impl Rect {
    pub fn w(&self) -> i32 { self.x1 - self.x0 }
    pub fn h(&self) -> i32 { self.y1 - self.y0 }
    pub fn grow(&self, m: i32) -> Rect { Rect { x0: self.x0 - m, y0: self.y0 - m, x1: self.x1 + m, y1: self.y1 + m } }
    pub fn intersects(&self, o: &Rect) -> bool { self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1 }
    pub fn union(&self, o: &Rect) -> Rect { Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) } }
}

/// 8-bağlantılı bileşenlerin sınır kutuları (alanı `min_area` altındakiler atılır).
pub fn component_boxes(mask: &GrayImage, min_area: u32) -> Vec<(Rect, u32)> {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let src = mask.as_raw();
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..w * h {
        if src[start] < 128 || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut area = 0u32;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            area += 1;
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if src[j] >= 128 && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if area >= min_area {
            out.push((Rect { x0: x0 as i32, y0: y0 as i32, x1: x1 as i32 + 1, y1: y1 as i32 + 1 }, area));
        }
    }
    out
}

/// Yakın kutuları (aralarındaki boşluk ≤ `gap`) birleştirerek gruplar.
pub fn merge_boxes(mut boxes: Vec<Rect>, gap: i32) -> Vec<Rect> {
    loop {
        let mut merged = false;
        let mut i = 0;
        while i < boxes.len() {
            let mut j = i + 1;
            while j < boxes.len() {
                if boxes[i].grow(gap / 2 + 1).intersects(&boxes[j].grow(gap / 2 + 1)) {
                    let b = boxes.swap_remove(j);
                    boxes[i] = boxes[i].union(&b);
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

/// Ayrık gauss benzeri yumuşatma (kutu bulanıklığı x3) – sadece küçük pencerelerde kullanılır.
pub fn feather_alpha(mask: &GrayImage, sigma: f32) -> Vec<f32> {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let mut a: Vec<f32> = mask.as_raw().iter().map(|&v| if v >= 128 { 1.0 } else { 0.0 }).collect();
    if sigma <= 0.0 {
        return a;
    }
    let r = (sigma * 1.2).ceil().max(1.0) as usize;
    let mut tmp = vec![0f32; a.len()];
    for _ in 0..3 {
        // yatay
        for y in 0..h {
            let row = &a[y * w..(y + 1) * w];
            let o = &mut tmp[y * w..(y + 1) * w];
            let mut acc = 0f32;
            let win = (2 * r + 1) as f32;
            for x in 0..=r.min(w - 1) {
                acc += row[x];
            }
            for x in 0..w {
                o[x] = acc / win;
                if x + r + 1 < w {
                    acc += row[x + r + 1];
                }
                if x >= r {
                    acc -= row[x - r];
                }
            }
        }
        // dikey
        for x in 0..w {
            let mut acc = 0f32;
            let win = (2 * r + 1) as f32;
            for y in 0..=r.min(h - 1) {
                acc += tmp[y * w + x];
            }
            for y in 0..h {
                a[y * w + x] = acc / win;
                if y + r + 1 < h {
                    acc += tmp[(y + r + 1) * w + x];
                }
                if y >= r {
                    acc -= tmp[(y - r) * w + x];
                }
            }
        }
    }
    // maske içi 1.0, dışı yumuşak düşüş: kenarda 0.5 -> 1.0
    for (v, m) in a.iter_mut().zip(mask.as_raw().iter()) {
        *v = if *m >= 128 { 1.0 } else { (*v * 2.0).min(1.0) };
    }
    a
}
