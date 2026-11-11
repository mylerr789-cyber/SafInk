//! Balon bölütleyiciyi sayfalarda dener: sayı + süre yazar, kırmızı önizleme kaydeder.
//! cargo run --release -p safink-core --example bubblecheck -- <bubble-seg.onnx> <out dir> <image>...
use safink_core::bubbles::{overlay, BubbleDetector};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut det = BubbleDetector::new(std::path::Path::new(&args[0])).unwrap();
    let out = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&out).unwrap();
    for f in &args[2..] {
        let img = image::open(f).unwrap().to_rgb8();
        let t = std::time::Instant::now();
        let b = det.detect(&img).unwrap();
        let ms = t.elapsed().as_secs_f32() * 1000.0;
        let name = PathBuf::from(f).file_stem().unwrap().to_string_lossy().to_string();
        println!("{name}\t{}x{}\tbubbles={}\t{ms:.0} ms", img.width(), img.height(), b.len());
        overlay(&img, &b).save(out.join(format!("{name}.png"))).unwrap();
    }
}
