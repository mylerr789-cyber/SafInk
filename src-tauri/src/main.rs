#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use safink_core::catalog::Hardware;
use safink_core::db::{Chapter, Manga, Page};
use safink_core::engine::{Engine, Event, JobSpec, ModelView};
use safink_core::paths::AppPaths;
use safink_core::settings::Settings;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::{InvokeBody, Request};
use tauri::{Emitter, Manager, State};

type Eng<'a> = State<'a, Arc<Engine>>;
type R<T> = Result<T, String>;

fn e2s<E: std::fmt::Display>(e: E) -> String { e.to_string() }

// ---------- ayarlar ----------
#[tauri::command]
fn get_settings(eng: Eng) -> Settings { eng.settings.read().clone() }
#[tauri::command]
fn save_settings(eng: Eng, settings: Settings) -> R<()> { eng.set_settings(settings).map_err(e2s) }

// ---------- kÃ¼tÃ¼phane ----------
#[tauri::command]
fn list_manga(eng: Eng) -> R<Vec<Manga>> { eng.db.list_manga().map_err(e2s) }
#[tauri::command]
fn create_manga(eng: Eng, name: String) -> R<i64> { eng.db.ensure_manga(name.trim()).map_err(e2s) }
#[tauri::command]
fn delete_manga(eng: Eng, id: i64) -> R<()> {
    for c in eng.db.list_chapters(id).map_err(e2s)? {
        eng.delete_chapter_full(c.id).map_err(e2s)?;
    }
    eng.db.delete_manga(id).map_err(e2s)
}
#[tauri::command]
fn list_chapters(eng: Eng, manga_id: i64) -> R<Vec<Chapter>> { eng.db.list_chapters(manga_id).map_err(e2s) }
#[tauri::command]
fn delete_chapter(eng: Eng, id: i64) -> R<()> { eng.delete_chapter_full(id).map_err(e2s) }
#[tauri::command]
fn rename_chapter(eng: Eng, id: i64, name: String) -> R<()> { eng.db.rename_chapter(id, name.trim()).map_err(e2s) }
#[tauri::command]
fn list_pages(eng: Eng, chapter_id: i64) -> R<Vec<Page>> { eng.db.pages(chapter_id).map_err(e2s) }
#[tauri::command]
fn get_page(eng: Eng, id: i64) -> R<Option<Page>> { eng.db.page(id).map_err(e2s) }
#[tauri::command]
fn delete_page(eng: Eng, id: i64) -> R<()> {
    eng.delete_page_files(id);
    eng.db.delete_page(id).map_err(e2s)
}
#[tauri::command]
async fn import_folder(eng: Eng<'_>, manga_id: i64, path: String) -> R<Vec<i64>> {
    let e = eng.inner().clone();
    tokio::task::spawn_blocking(move || e.import_folder(manga_id, &PathBuf::from(path))).await.map_err(e2s)?.map_err(e2s)
}
#[tauri::command]
async fn add_to_chapter(eng: Eng<'_>, chapter_id: i64, paths: Vec<String>) -> R<usize> {
    let e = eng.inner().clone();
    tokio::task::spawn_blocking(move || e.add_files_to_chapter(chapter_id, &paths.into_iter().map(PathBuf::from).collect::<Vec<_>>()))
        .await
        .map_err(e2s)?
        .map_err(e2s)
}
#[tauri::command]
async fn quick_new(eng: Eng<'_>, paths: Vec<String>) -> R<i64> {
    let e = eng.inner().clone();
    tokio::task::spawn_blocking(move || e.quick_new(&paths.into_iter().map(PathBuf::from).collect::<Vec<_>>())).await.map_err(e2s)?.map_err(e2s)
}

// ---------- iÅŸ kuyruÄŸu ----------
#[tauri::command]
async fn start_job(eng: Eng<'_>, spec: JobSpec) -> R<u64> { eng.inner().start_job(spec).map_err(e2s) }
#[tauri::command]
fn cancel_job(eng: Eng) { eng.cancel_job() }
#[tauri::command]
fn pause_job(eng: Eng, paused: bool) { eng.pause_job(paused) }
#[tauri::command]
fn job_running(eng: Eng) -> bool { eng.current_job().is_some() }

#[derive(Serialize)]
struct ExportResult { ok: usize, skipped: usize, dir: String }

#[tauri::command]
async fn export_pages(eng: Eng<'_>, page_ids: Vec<i64>, out_dir: Option<String>) -> R<ExportResult> {
    let e = eng.inner().clone();
    tokio::task::spawn_blocking(move || {
        let s = e.settings.read().clone();
        let dir = out_dir.filter(|d| !d.is_empty()).or(Some(s.export_dir.clone()).filter(|d| !d.is_empty())).ok_or_else(|| anyhow::anyhow!("Ã§Ä±ktÄ± klasÃ¶rÃ¼ seÃ§ilmedi"))?;
        let (ok, skipped, d) = e.export(&page_ids, &PathBuf::from(dir), &s.export_format, s.export_quality)?;
        Ok::<_, anyhow::Error>(ExportResult { ok, skipped, dir: d.to_string_lossy().to_string() })
    })
    .await
    .map_err(e2s)?
    .map_err(e2s)
}
#[tauri::command]
fn reveal_path(path: String) { let _ = std::process::Command::new("explorer").arg(path).spawn(); }

// ---------- dÃ¼zenleme katmanÄ± ----------
/// Ham gÃ¶vde: [u32 tx][u32 ty][1024*1024 bayt] kayÄ±tlarÄ±. Sayfa id'si `x-page-id` baÅŸlÄ±ÄŸÄ±nda.
#[tauri::command]
fn save_edit(eng: Eng, req: Request) -> R<()> {
    let InvokeBody::Raw(bytes) = req.body() else { return Err("ham gÃ¶vde bekleniyordu".into()) };
    let pid: i64 = req.headers().get("x-page-id").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).ok_or("x-page-id eksik")?;
    let page = eng.db.page(pid).map_err(e2s)?.ok_or("sayfa yok")?;
    safink_core::tiles::save_edit_tiles(&eng.paths, &page, bytes).map_err(e2s)?;
    if page.status != "new" || eng.paths.mask_file(&page.sha256, page.mask_conf as f32).exists() {
        eng.db.set_status(pid, "mask_edited", "").map_err(e2s)?;
    }
    eng.tiles.invalidate(pid);
    if let Ok(Some(p)) = eng.db.page(pid) {
        eng.emit(Event::Page { page_id: pid, status: p.status, error: p.error, rev: p.rev });
    }
    Ok(())
}
#[tauri::command]
fn reset_edits(eng: Eng, page_id: i64) -> R<()> {
    let _ = std::fs::remove_file(eng.paths.edit_file(page_id));
    eng.tiles.invalidate(page_id);
    Ok(())
}

// ---------- modeller ----------
#[tauri::command]
fn list_models(eng: Eng) -> Vec<ModelView> { eng.models() }
#[tauri::command]
fn hardware(eng: Eng) -> Hardware { eng.hardware().clone() }
#[tauri::command]
async fn install_model(eng: Eng<'_>, id: String) -> R<()> { eng.inner().install_model(&id).map_err(e2s) }
#[tauri::command]
fn cancel_install(eng: Eng, id: String) { eng.cancel_install(&id) }
#[tauri::command]
fn remove_model(eng: Eng, id: String) -> R<()> { eng.remove_model(&id).map_err(e2s) }

fn cors(b: tauri::http::response::Builder) -> tauri::http::response::Builder {
    b.header("Access-Control-Allow-Origin", "*").header("Cache-Control", "no-store")
}

fn handle_protocol(eng: Arc<Engine>, path: &str) -> tauri::http::Response<Vec<u8>> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let res: anyhow::Result<(Vec<u8>, &'static str)> = (|| match parts.as_slice() {
        ["tile", pid, layer, tx, ty] => {
            let page = eng.db.page(pid.parse()?)?.ok_or_else(|| anyhow::anyhow!("sayfa yok"))?;
            let req = safink_core::tiles::TileRequest { page_id: page.id, layer: layer.to_string(), tx: tx.parse()?, ty: ty.trim_end_matches(".png").parse()? };
            safink_core::tiles::render_tile(&eng.paths, &eng.tiles, &page, &req)
        }
        ["thumb", pid] => {
            let page = eng.db.page(pid.trim_end_matches(".jpg").parse()?)?.ok_or_else(|| anyhow::anyhow!("sayfa yok"))?;
            Ok((safink_core::tiles::thumbnail(&eng.paths, &page)?, "image/jpeg"))
        }
        _ => Err(anyhow::anyhow!("bilinmeyen yol")),
    })();
    match res {
        Ok((b, mime)) => cors(tauri::http::Response::builder()).header("Content-Type", mime).body(b).unwrap(),
        Err(_) => cors(tauri::http::Response::builder()).status(404).body(Vec::new()).unwrap(),
    }
}

fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())).init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("safink", |ctx, request, responder| {
            let eng = ctx.app_handle().state::<Arc<Engine>>().inner().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(handle_protocol(eng, &path)));
        })
        .setup(|app| {
            let root = std::env::var("SAFINK_DATA").map(PathBuf::from).unwrap_or_else(|_| app.path().app_data_dir().expect("app_data_dir"));
            let paths = AppPaths::new(root)?;
            let handle = app.handle().clone();
            let eng = Engine::new(paths, Arc::new(move |e: Event| {
                let _ = handle.emit("safink", &e);
            }))?;
            app.manage(eng);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings, save_settings, list_manga, create_manga, delete_manga, list_chapters, delete_chapter,
            rename_chapter, list_pages, get_page, delete_page, import_folder, add_to_chapter, quick_new, start_job, cancel_job, pause_job,
            job_running, export_pages, reveal_path, save_edit, reset_edits, list_models, hardware, install_model, cancel_install, remove_model
        ])
        .run(tauri::generate_context!())
        .expect("SafInk baÅŸlatÄ±lamadÄ±");
}

