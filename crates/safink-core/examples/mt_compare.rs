//! Redraw modeli (Klein) MangaTranslator bölge yolu: gerçek sayfalarda temizleme. Düz doldurma önbellekteki .flat.json ile yapılır
//! (API çağrısı yok). Çıktı: `<ad>_full_orig_new.png` (özgün | sonuç), en büyük 3 bileşenin yakınlaştırması, süre, maske dışı değişim sayısı.
//! `SAFINK_DEBUG_DIR=<dizin>`: her bölge için ara görüntüler (girdi kırpıntısı, maske, ham model çıktısı, hizalı, ışıklılık eşlenmiş,
//! glif kompoziti sonrası, yapıştırma maskesi, yan yana özet) `<dizin>/<ad>/rNNN_x_y_*.png` olarak yazılır.
//! cargo run --release -p safink-core --example mt_compare -- <data root> <masks dir> <out dir> <sayfa.png>...
//! Maske: <masks dir>/<sha256>_c025.png (+ .flat.json). Model: SAFINK_MODEL (varsayılan sdcpp-flux2-klein-4b); SAFINK_PROMPT / SAFINK_SEED / SAFINK_POST geliştirme anahtarları.
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use image::{GrayImage, RgbImage};
use safink_core::{api, catalog::Catalog, inpaint, maskops, paths::AppPaths, pipeline, settings::Settings};
use sha2::{Digest, Sha256};

fn sbs(panels: &[&RgbImage], scale: u32) -> RgbImage {
    let h = panels.iter().map(|p| p.height()).max().unwrap() * scale;
    let w: u32 = panels.iter().map(|p| p.width() * scale + 6).sum::<u32>() - 6;
    let mut o = RgbImage::from_pixel(w, h, image::Rgb([255, 0, 255]));
    let mut x = 0i64;
    for p in panels {
        let q = if scale > 1 { image::imageops::resize(*p, p.width() * scale, p.height() * scale, image::imageops::FilterType::Lanczos3) } else { (*p).clone() };
        image::imageops::replace(&mut o, &q, x, 0);
        x += q.width() as i64 + 6;
    }
    o
}

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let paths = AppPaths::new(&PathBuf::from(&a[0]))?;
    let masks = PathBuf::from(&a[1]);
    let out = PathBuf::from(&a[2]);
    std::fs::create_dir_all(&out)?;
    let pages: Vec<PathBuf> = a[3..].iter().map(PathBuf::from).collect();
    let settings = Settings::load(&paths);
    let cat = Catalog::load(&paths, settings.low_vram);
    let model = std::env::var("SAFINK_MODEL").unwrap_or_else(|_| "sdcpp-flux2-klein-4b".into());
    let spec = cat.get(&model).ok_or_else(|| anyhow::anyhow!("model yok"))?.clone();
    let cancel = AtomicBool::new(false);
    let dil = settings.dilation;
    let feather = settings.feather;
    let dbg_dir = std::env::var_os("SAFINK_DEBUG_DIR").map(PathBuf::from);

    struct Page {
        name: String,
        orig: RgbImage,
        remaining: GrayImage,
        work: RgbImage,
        res: Option<RgbImage>,
    }
    let mut ps: Vec<Page> = vec![];
    for p in &pages {
        let bytes = std::fs::read(p)?;
        let sha = hex(&Sha256::digest(&bytes));
        let img = image::load_from_memory(&bytes)?.to_rgb8();
        let mp = masks.join(format!("{sha}_c025.png"));
        let mask = image::open(&mp)?.to_luma8();
        let m = maskops::dilate(&mask, dil);
        let parts = std::fs::read(mp.with_extension("flat.json")).ok().and_then(|b| api::parse_flat_json(&b));
        let f = pipeline::flat_pass(&img, &m, feather, parts.as_deref());
        println!("{}: düz={} model={}", p.display(), f.stats.flat, f.stats.model);
        ps.push(Page { name: p.file_stem().unwrap().to_string_lossy().to_string(), orig: img, remaining: f.remaining, work: f.work, res: None });
    }

    let t0 = std::time::Instant::now();
    let mut inp = inpaint::create_with(&paths, &spec, &settings, &cancel, &|s| println!("  {s}"))?;
    println!("model yükleme {:.1}s", t0.elapsed().as_secs_f32());
    for pg in ps.iter_mut() {
        let t = std::time::Instant::now();
        let dir = dbg_dir.as_ref().map(|d| d.join(&pg.name));
        if let Some(d) = &dir {
            std::fs::create_dir_all(d)?;
        }
        let mut dump = |r: pipeline::RegionDebug| {
            let Some(d) = &dir else { return };
            let pre = format!("r{:03}_{}_{}", r.index, r.crop_rect[0], r.crop_rect[1]);
            let _ = r.crop.save(d.join(format!("{pre}_1_crop.png")));
            let _ = r.mask.save(d.join(format!("{pre}_2_mask.png")));
            let _ = r.raw.save(d.join(format!("{pre}_3_raw.png")));
            let _ = r.aligned.save(d.join(format!("{pre}_4_aligned.png")));
            let _ = r.lum.save(d.join(format!("{pre}_5_lum.png")));
            let _ = r.patch.save(d.join(format!("{pre}_6_glyph.png")));
            let _ = r.own.save(d.join(format!("{pre}_7_own.png")));
            let scale = if r.crop.width() < 300 { 2 } else { 1 };
            let _ = sbs(&[&r.crop, &r.raw, &r.aligned, &r.lum, &r.patch], scale).save(d.join(format!("{pre}_0_steps.png")));
        };
        let res = pipeline::clean_regions_mt_dbg(&pg.work, pg.remaining.clone(), inp.as_mut(), &cancel, &|_, _| {}, Some(&mut dump))?;
        println!("{} : {:.1}s", pg.name, t.elapsed().as_secs_f32());
        // maske dışı bayt bayt aynı mı?
        let bad = pg.work.pixels().zip(res.pixels()).zip(pg.remaining.pixels()).filter(|((a, b), m)| m[0] < 128 && a != b).count();
        println!("   maske dışı değişen piksel: {bad}");
        pg.res = Some(res);
    }

    for pg in &ps {
        let n = pg.res.as_ref().unwrap();
        sbs(&[&pg.orig, n], 1).save(out.join(format!("{}_full_orig_new.png", pg.name)))?;
        // yakınlaştırmalar: modele giden en büyük 3 bileşen
        let comps = component_boxes(&pg.remaining);
        let (iw, ih) = pg.orig.dimensions();
        for (i, b) in comps.iter().take(3).enumerate() {
            let pad = 48u32;
            let (x0, y0) = (b[0].saturating_sub(pad), b[1].saturating_sub(pad));
            let (x1, y1) = ((b[2] + pad).min(iw), (b[3] + pad).min(ih));
            let cr = |im: &RgbImage| image::imageops::crop_imm(im, x0, y0, x1 - x0, y1 - y0).to_image();
            let sc = if (x1 - x0) < 300 { 3 } else if (x1 - x0) < 600 { 2 } else { 1 };
            sbs(&[&cr(&pg.orig), &cr(n)], sc).save(out.join(format!("{}_zoom{}_orig_new.png", pg.name, i + 1)))?;
        }
    }
    println!("çıktı: {}", out.display());
    Ok(())
}

/// Maskenin en büyük (alan) 8-bağlantılı bileşenlerinin kutuları [x0, y0, x1, y1) .
fn component_boxes(m: &GrayImage) -> Vec<[u32; 4]> {
    let mut v: Vec<(u32, [u32; 4])> = maskops::component_boxes(m, 30).into_iter().map(|(r, area)| (area, [r.x0 as u32, r.y0 as u32, r.x1 as u32, r.y1 as u32])).collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.0));
    v.into_iter().map(|x| x.1).collect()
}
