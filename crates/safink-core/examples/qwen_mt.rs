//! Qwen: MangaTranslator bölge yolu (yeni) ile 0.6.0 Koharu karo yolunu (yeşil boya) gerçek sayfalarda karşılaştırır.
//! Düz doldurma önbellekteki .flat.json ile yapılır (API çağrısı yok). Çıktı: tam sayfa yan yana, yakınlaştırmalar, süre, kayma ölçümü.
//! cargo run --release -p safink-core --example qwen_mt -- <data root> <masks dir> <out dir> <sayfa.png>... [-- model id]
//! Maske: <masks dir>/<sha256>_c025.png (+ .flat.json). Model: SAFINK_MODEL (varsayılan sdcpp-qwen-image-2-1-turbo).
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use image::{GrayImage, RgbImage};
use safink_core::inpaint::{self, Ctx, Inpainter, TileSpec};
use safink_core::{api, catalog::Catalog, maskops, paths::AppPaths, pipeline, settings::Settings};
use sha2::{Digest, Sha256};

struct Spy {
    inner: Box<dyn Inpainter>,
    /// (kırpıntı, ham model çıktısı, bölge maskesi)
    seen: Vec<(RgbImage, RgbImage, GrayImage)>,
}
impl Inpainter for Spy {
    fn label(&self) -> String { self.inner.label() }
    fn tile_spec(&self) -> TileSpec { self.inner.tile_spec() }
    fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> anyhow::Result<RgbImage> {
        let out = self.inner.inpaint(img, mask, ctx)?;
        self.seen.push((img.clone(), out.clone(), mask.clone()));
        Ok(out)
    }
}

fn mad(a: &RgbImage, b: &RgbImage, m: &GrayImage, dx: i32, dy: i32) -> f64 {
    let (w, h) = a.dimensions();
    let (mut s, mut n) = (0f64, 0f64);
    for y in 8..h as i32 - 8 {
        for x in 8..w as i32 - 8 {
            if m.get_pixel(x as u32, y as u32)[0] >= 128 {
                continue;
            }
            let p = a.get_pixel(x as u32, y as u32);
            let q = b.get_pixel((x + dx) as u32, (y + dy) as u32);
            s += (0..3).map(|k| (p[k] as f64 - q[k] as f64).abs()).sum::<f64>() / 3.0;
            n += 1.0;
        }
    }
    if n == 0.0 { f64::NAN } else { s / n }
}

/// En iyi artık kayma (dx, dy) ±6 px içinde ve hata; maskenin 6 px dışındaki pikseller.
fn residual(crop: &RgbImage, out: &RgbImage, cm: &GrayImage) -> (i32, i32, f64, f64) {
    let far = maskops::dilate(cm, 6);
    let base = mad(crop, out, &far, 0, 0);
    let mut best = (base, 0, 0);
    for dy in -6..=6 {
        for dx in -6..=6 {
            let v = mad(crop, out, &far, dx, dy);
            if v < best.0 {
                best = (v, dx, dy);
            }
        }
    }
    (best.1, best.2, base, best.0)
}

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
    let model = std::env::var("SAFINK_MODEL").unwrap_or_else(|_| "sdcpp-qwen-image-2-1-turbo".into());
    let spec_new = cat.get(&model).ok_or_else(|| anyhow::anyhow!("model yok"))?.clone();
    let mut spec_old = spec_new.clone();
    // 0.6.0: yeşil boya + Koharu karoları (512 + 128), ölçeksiz
    spec_old.params["input_mode"] = "green".into();
    spec_old.params.as_object_mut().unwrap().remove("mt_crop");
    let cancel = AtomicBool::new(false);
    let dil = settings.dilation;
    let feather = settings.feather;

    struct Page {
        name: String,
        orig: RgbImage,
        remaining: GrayImage,
        work: RgbImage,
        res_new: Option<RgbImage>,
        res_old: Option<RgbImage>,
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
        ps.push(Page { name: p.file_stem().unwrap().to_string_lossy().to_string(), orig: img, remaining: f.remaining, work: f.work, res_new: None, res_old: None });
    }

    for (label, spec, is_new) in [("YENİ (MT bölge)", &spec_new, true), ("0.6.0 (karo, yeşil)", &spec_old, false)] {
        let t0 = std::time::Instant::now();
        let inner = inpaint::create_with(&paths, spec, &settings, &cancel, &|s| println!("  {s}"))?;
        println!("== {label}: model yükleme {:.1}s", t0.elapsed().as_secs_f32());
        let mut spy = Spy { inner, seen: vec![] };
        for pg in ps.iter_mut() {
            spy.seen.clear();
            let before = pipeline::ALIGN_FIRED.load(Ordering::Relaxed);
            let t = std::time::Instant::now();
            let res = pipeline::clean_dilated(&pg.work, pg.remaining.clone(), &pipeline::CleanParams { dilation: dil, feather, flat_fill: true }, &mut spy, &cancel, &|_, _| {})?;
            let secs = t.elapsed().as_secs_f32();
            let fired = pipeline::ALIGN_FIRED.load(Ordering::Relaxed) - before;
            println!("{} {label}: {:.1}s, {} model çağrısı, align_to tetiklendi: {fired}", pg.name, secs, spy.seen.len());
            if is_new {
                // ham (hizalama öncesi) artık kayma, hizalama sonrası artık kayma
                let (mut worst_raw, mut worst_al) = ((0i32, 0i32), (0i32, 0i32));
                for (i, (crop, o, cm)) in spy.seen.iter().enumerate() {
                    if crop.width() < 40 || crop.height() < 40 {
                        continue;
                    }
                    let (dx, dy, e0, e1) = residual(crop, o, cm);
                    let al = pipeline::align_to(crop, o.clone(), cm);
                    let (ax, ay, a0, a1) = residual(crop, &al, cm);
                    println!("   bölge {i} {}x{}: ham kayma ({dx},{dy}) hata {e0:.2}->{e1:.2}; hizalı sonrası ({ax},{ay}) hata {a0:.2}->{a1:.2}", crop.width(), crop.height());
                    if dx.abs() + dy.abs() > worst_raw.0.abs() + worst_raw.1.abs() {
                        worst_raw = (dx, dy);
                    }
                    if ax.abs() + ay.abs() > worst_al.0.abs() + worst_al.1.abs() {
                        worst_al = (ax, ay);
                    }
                }
                println!("   EN KÖTÜ artık kayma: ham {worst_raw:?}, hizalı {worst_al:?}");
                pg.res_new = Some(res);
            } else {
                pg.res_old = Some(res);
            }
            if is_new {
                // maske dışı bayt bayt aynı mı?
                let bad = pg.work.pixels().zip(pg.res_new.as_ref().unwrap().pixels()).zip(pg.remaining.pixels()).filter(|((a, b), m)| m[0] < 128 && a != b).count();
                println!("   maske dışı değişen piksel: {bad}");
            }
        }
    }

    for pg in &ps {
        let (n, o) = (pg.res_new.as_ref().unwrap(), pg.res_old.as_ref().unwrap());
        sbs(&[&pg.orig, n], 1).save(out.join(format!("{}_full_orig_vs_new.png", pg.name)))?;
        sbs(&[&pg.orig, o], 1).save(out.join(format!("{}_full_orig_vs_v060.png", pg.name)))?;
        sbs(&[&pg.orig, o, n], 1).save(out.join(format!("{}_full_orig_v060_new.png", pg.name)))?;
        // yakınlaştırmalar: modele giden en büyük 3 bileşen
        let comps = component_boxes(&pg.remaining);
        let (iw, ih) = pg.orig.dimensions();
        for (i, b) in comps.iter().take(3).enumerate() {
            let pad = 48u32;
            let (x0, y0) = (b[0].saturating_sub(pad), b[1].saturating_sub(pad));
            let (x1, y1) = ((b[2] + pad).min(iw), (b[3] + pad).min(ih));
            let cr = |im: &RgbImage| image::imageops::crop_imm(im, x0, y0, x1 - x0, y1 - y0).to_image();
            let sc = if (x1 - x0) < 300 { 3 } else if (x1 - x0) < 600 { 2 } else { 1 };
            sbs(&[&cr(&pg.orig), &cr(o), &cr(n)], sc).save(out.join(format!("{}_zoom{}_orig_v060_new.png", pg.name, i + 1)))?;
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
