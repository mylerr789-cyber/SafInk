//! Düz doldurma kararını gerçek sayfalarda sayar (model çağrılmaz).
//! cargo run --release -p safink-core --example flatcheck -- <masks dir> <out dir> <image dir>...
//! Maske dosyaları `<sha256>_cXXX.png`; görüntüler sha256 ile eşlenir.
use std::collections::HashMap;
use std::path::PathBuf;

use safink_core::{maskops, pipeline};
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
    let (mut tf, mut tm, mut pages) = (0, 0, 0);
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
                println!("{}\tflat={}\tmodel={}", p.display(), f.stats.flat, f.stats.model);
                tf += f.stats.flat;
                tm += f.stats.model;
                pages += 1;
                let name = format!("{pages:03}_{}.png", p.file_stem().unwrap().to_string_lossy());
                f.work.save(out.join(name)).unwrap();
            }
        }
    }
    println!("TOTAL pages={pages} flat={tf} model={tm}");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
