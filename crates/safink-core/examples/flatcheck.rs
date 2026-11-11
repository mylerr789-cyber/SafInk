//! Düz doldurma kararını gerçek sayfalarda sayar (model çağrılmaz).
//! cargo run --release -p safink-core --example flatcheck -- <masks dir> <out dir> <image dir>...
//! Maske dosyaları `<sha256>_cXXX.png`; görüntüler sha256 ile eşlenir.
use std::collections::HashMap;
use std::path::PathBuf;

use safink_core::{bubbles::BubbleDetector, maskops, pipeline};
use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let masks = PathBuf::from(&args[0]);
    let out = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&out).unwrap();
    let mut by_sha: HashMap<String, PathBuf> = HashMap::new();
    for e in std::fs::read_dir(&masks).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if let Some((sha, _)) = n.split_once('_') {
            by_sha.insert(sha.to_string(), e.path());
        }
    }
    // BUBBLE_ONNX verilirse balon kuralı da ölçülür (eski halka kuralıyla yan yana)
    let mut det = std::env::var("BUBBLE_ONNX").ok().map(|p| BubbleDetector::new(std::path::Path::new(&p)).unwrap());
    let (mut tf, mut tm, mut pages, mut bf, mut bm) = (0, 0, 0, 0, 0);
    for dir in &args[2..] {
        let mut stack = vec![PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                if !["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
                    continue;
                }
                let bytes = std::fs::read(&p).unwrap();
                let sha = hex(&Sha256::digest(&bytes));
                let Some(mp) = by_sha.get(&sha) else { continue };
                let img = image::load_from_memory(&bytes).unwrap().to_rgb8();
                let m = image::open(mp).unwrap().to_luma8();
                if m.dimensions() != img.dimensions() {
                    continue;
                }
                let m = maskops::dilate(&m, 2);
                let f = pipeline::flat_fill(&img, &m, 4, &pipeline::FLAT_CFG);
                let mut line = format!("{}	ring flat={} model={}", p.display(), f.stats.flat, f.stats.model);
                let mut work = f.work;
                if let Some(d) = det.as_mut() {
                    let b = d.detect(&img).unwrap();
                    let g = pipeline::flat_fill_bubbles(&img, &m, 4, &b, &pipeline::FLAT_CFG);
                    line += &format!("	bubble({}) flat={} model={}", b.len(), g.stats.flat, g.stats.model);
                    bf += g.stats.flat;
                    bm += g.stats.model;
                    work = g.work;
                }
                println!("{line}");
                tf += f.stats.flat;
                tm += f.stats.model;
                pages += 1;
                let name = format!("{pages:03}_{}.png", p.file_stem().unwrap().to_string_lossy());
                work.save(out.join(name)).unwrap();
            }
        }
    }
    println!("TOTAL pages={pages} ring: flat={tf} model={tm} | bubble: flat={bf} model={bm}");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
