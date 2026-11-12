//! Model çıktısı girdiye göre kayıyor mu? Sayfanın karolarını modelden geçirir, maske DIŞINDAKİ piksellerde
//! girdi ile çıktı arasındaki en iyi (dx, dy) kaymasını ve hatayı ölçer.
//! cargo run --release -p safink-core --example shiftcheck -- <data root> <model id> <sayfa> <maske> <çıktı klasörü> [karo sayısı]
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use image::{GrayImage, RgbImage};
use safink_core::{catalog::Catalog, inpaint, maskops, paths::AppPaths, pipeline, settings::Settings};

fn mad(a: &RgbImage, b: &RgbImage, m: &GrayImage, dx: i32, dy: i32) -> f64 {
    let (w, h) = a.dimensions();
    let (mut s, mut n) = (0f64, 0f64);
    for y in 8..h as i32 - 8 {
        for x in 8..w as i32 - 8 {
            if m.get_pixel(x as u32, y as u32)[0] >= 128 {
                continue;
            }
            let (bx, by) = (x + dx, y + dy);
            let p = a.get_pixel(x as u32, y as u32);
            let q = b.get_pixel(bx as u32, by as u32);
            s += (0..3).map(|k| (p[k] as f64 - q[k] as f64).abs()).sum::<f64>() / 3.0;
            n += 1.0;
        }
    }
    if n == 0.0 { f64::NAN } else { s / n }
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let paths = AppPaths::new(&PathBuf::from(&a[0]))?;
    let settings = Settings::load(&paths);
    let cat = Catalog::load(&paths, settings.low_vram);
    let spec = cat.get(&a[1]).ok_or_else(|| anyhow::anyhow!("model yok: {}", a[1]))?.clone();
    let img = image::open(&a[2])?.to_rgb8();
    let mask = maskops::dilate(&image::open(&a[3])?.to_luma8(), 2);
    let out = PathBuf::from(&a[4]);
    std::fs::create_dir_all(&out)?;
    let maxn: usize = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(4);
    let cancel = AtomicBool::new(false);
    let mut inp = inpaint::create_with(&paths, &spec, &settings, &cancel, &|_| {})?;
    let (_, tiles) = pipeline::plan_tiles(&mask, &inp.tile_spec());
    // en büyük karolar (kayma en iyi orada görünür)
    let mut tiles = tiles;
    tiles.sort_by_key(|t| std::cmp::Reverse((t.crop[2] - t.crop[0]) * (t.crop[3] - t.crop[1])));
    for (i, t) in tiles.iter().take(maxn).enumerate() {
        let [x0, y0, x1, y1] = t.crop;
        let crop = image::imageops::crop_imm(&img, x0, y0, x1 - x0, y1 - y0).to_image();
        let cm = image::imageops::crop_imm(&mask, x0, y0, x1 - x0, y1 - y0).to_image();
        let res = inp.inpaint(&crop, &cm, &safink_core::inpaint::Ctx { cancel: &cancel })?;
        let res = if std::env::var("NOALIGN").is_ok() { res } else { pipeline::align_to(&crop, res, &cm) };
        let cmd = maskops::dilate(&cm, 6);
        let base = mad(&crop, &res, &cmd, 0, 0);
        let mut best = (base, 0, 0);
        for dy in -6..=6 {
            for dx in -6..=6 {
                let v = mad(&crop, &res, &cmd, dx, dy);
                if v < best.0 {
                    best = (v, dx, dy);
                }
            }
        }
        println!("{} karo {} {}x{}: kaymasız hata {:.2}, en iyi kayma dx={} dy={} hata {:.2}", a[1], i, x1 - x0, y1 - y0, base, best.1, best.2, best.0);
        let mut sbs = RgbImage::new((x1 - x0) * 2 + 4, y1 - y0);
        image::imageops::replace(&mut sbs, &crop, 0, 0);
        image::imageops::replace(&mut sbs, &res, ((x1 - x0) + 4) as i64, 0);
        sbs.save(out.join(format!("{}_{i}.png", a[1])))?;
    }
    Ok(())
}
