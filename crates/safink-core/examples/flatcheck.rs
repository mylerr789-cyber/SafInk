//! Düz doldurma kararını gerçek sayfalarda sayar (model çağrılmaz).
//! cargo run --release -p safink-core --example flatcheck -- <masks dir> <out dir> <image dir>...
//! Maske dosyaları `<sha256>_cXXX.png`; görüntüler sha256 ile eşlenir. Yanında `<sha256>_cXXX.flat.json` varsa (sunucu sınıflandırması)
//! o kural da halka kuralının yanında ölçülür.
use std::collections::HashMap;
use std::path::PathBuf;

use safink_core::{api, maskops, pipeline};
use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let masks = PathBuf::from(&args[0]);
    let out = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&out).unwrap();
    let mut by_sha: HashMap<String, PathBuf> = HashMap::new();
    for e in std::fs::read_dir(&masks).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if !n.ends_with(".png") {
            continue;
        }
        if let Some((sha, _)) = n.split_once('_') {
            by_sha.insert(sha.to_string(), e.path());
        }
    }
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
                let fj = mp.with_extension("flat.json");
                if let Some(parts) = std::fs::read(&fj).ok().and_then(|b| api::parse_flat_json(&b)) {
                    let g = pipeline::flat_pass(&img, &m, 4, Some(&parts));
                    line += &format!("	server({}) flat={} model={}", parts.len(), g.stats.flat, g.stats.model);
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
    println!("TOTAL pages={pages} ring: flat={tf} model={tm} | server: flat={bf} model={bm}");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
