//! Uçtan uca duman testi: modeli indir (LaMa), iki sayfayı içe aktar, maske çek, temizle, dışa aktar.
//! Kullanım: cargo run --release -p safink-core --example e2e -- <veri_dizini> <resim1> [resim2...]
use safink_core::engine::{Engine, Event, JobSpec};
use safink_core::paths::AppPaths;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(&args[0]);
    let images: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
    let paths = AppPaths::new(&root)?;
    let err_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ef = err_flag.clone();
    let t00 = std::time::Instant::now();
    let eng = Engine::new(
        paths,
        Arc::new(move |e| match &e {
            Event::Download(p) if p.stage == "downloading" => {}
            Event::Download(p) if p.stage == "error" => {
                ef.store(true, std::sync::atomic::Ordering::Relaxed);
                println!("download HATA: {}", p.file)
            }
            Event::EnvLog { line } => println!("env: {line}"),
            Event::Page { page_id, status, error, .. } => println!("[{:.1}s] page {page_id}: {status} {error}", t00.elapsed().as_secs_f32()),
            Event::Job { state, cleaned, fetched, failed, message, wait_secs, .. } => {
                println!("[{:.1}s] job {state} fetched={fetched} cleaned={cleaned} failed={failed} {message} wait={wait_secs:.0}", t00.elapsed().as_secs_f32())
            }
            other => println!("{}", serde_json::to_string(other).unwrap_or_default()),
        }),
    )?;
    let model = std::env::var("SAFINK_MODEL").unwrap_or_else(|_| "lama-manga".into());
    let spec = eng.catalog.get(&model).unwrap().clone();
    if !spec.is_installed(&eng.paths) {
        // uygulamanın kendi kurulum akışı: çalışma ortamı + model tek zincirde
        println!("kuruluyor: {}", spec.name);
        let t = std::time::Instant::now();
        eng.install_model(&model)?;
        let mut last = std::time::Instant::now();
        while !spec.is_installed(&eng.paths) {
            if err_flag.load(std::sync::atomic::Ordering::Relaxed) {
                anyhow::bail!("kurulum hatası (yukarıdaki günlüğe bakın)");
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if last.elapsed().as_secs() > 60 {
                println!("  … {:.0}s", t.elapsed().as_secs_f32());
                last = std::time::Instant::now();
            }
        }
        println!("kurulum süresi: {:.0}s", t.elapsed().as_secs_f32());
    }
    let mut s = eng.settings.read().clone();
    s.model_id = model;
    eng.set_settings(s)?;
    let cid = eng.quick_new(&images)?;
    let pages = eng.db.pages(cid)?;
    let ids: Vec<i64> = pages.iter().map(|p| p.id).collect();
    let t = std::time::Instant::now();
    eng.start_job(JobSpec { page_ids: ids.clone(), mode: "clean".into(), force: true, preset: None })?;
    while eng.current_job().is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    println!("temizleme süresi: {:.1}s", t.elapsed().as_secs_f32());
    let out = root.join("export");
    let (ok, skipped, _) = eng.export(&ids, &out, "original", 95)?;
    println!("export ok={ok} skipped={skipped} -> {}", out.display());
    for p in eng.db.pages(cid)? {
        println!("{} {} det={} conf={} err={}", p.name, p.status, p.detections, p.mask_conf, p.error);
    }
    Ok(())
}
