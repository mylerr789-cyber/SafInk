//! Motor: kütüphane içe aktarma, iş kuyruğu (maske çek -> temizle), dışa aktarma, model yönetimi.
use crate::api::{ApiClient, MAX_UPLOAD};
use crate::catalog::{fit_hint, Catalog, Hardware, ModelSpec};
use crate::db::{Db, Page};
use crate::download::Progress;
use crate::paths::AppPaths;
use crate::pipeline::CleanParams;
use crate::settings::{Preset, Settings};
use crate::tiles::TileCache;
use crate::{imgio, inpaint, maskops};
use anyhow::{anyhow, bail, Result};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Job { job_id: u64, mode: String, total: usize, fetched: usize, cleaned: usize, failed: usize, current: Option<i64>, state: String, message: String, wait_secs: f64 },
    Page { page_id: i64, status: String, error: String, rev: i64 },
    Download(Progress),
    EnvLog { line: String },
    Import { done: usize, total: usize },
}

#[derive(Deserialize, Clone, Debug)]
pub struct JobSpec {
    pub page_ids: Vec<i64>,
    /// "fetch" | "clean"
    pub mode: String,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub preset: Option<Preset>,
}

pub struct JobCtl {
    pub cancel: AtomicBool,
    pub paused: AtomicBool,
    pub id: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelView {
    #[serde(flatten)]
    pub spec: ModelSpec,
    pub installed: bool,
    pub fit: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    /// Kullanıcıya gösterilsin mi (çalışma ortamları ve bu donanımda anlamsız modeller gizli).
    pub visible: bool,
    /// Tek tuşla indirilecek toplam bayt (model + henüz kurulu olmayan çalışma ortamı).
    pub download_bytes: u64,
}

pub struct Engine {
    pub paths: AppPaths,
    pub db: Db,
    pub settings: RwLock<Settings>,
    catalog: RwLock<Arc<Catalog>>,
    pub http: reqwest::Client,
    pub tiles: TileCache,
    emit: Arc<dyn Fn(Event) + Send + Sync>,
    job: Mutex<Option<Arc<JobCtl>>>,
    job_counter: AtomicU64,
    api: Mutex<Option<(u32, Arc<ApiClient>)>>,
    dl_cancel: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// İş sürerken ayar değişti: sd.cpp sunucusu iş bitince yeniden başlatılır.
    restart_pending: AtomicBool,
}

fn emit_page(e: &Engine, id: i64) {
    if let Ok(Some(p)) = e.db.page(id) {
        (e.emit)(Event::Page { page_id: id, status: p.status, error: p.error, rev: p.rev });
    }
}

impl Engine {
    pub fn new(paths: AppPaths, emit: Arc<dyn Fn(Event) + Send + Sync>) -> Result<Arc<Self>> {
        crate::api::set_secret_dir(paths.root.clone());
        let settings = Settings::load(&paths);
        let catalog = Catalog::load(&paths, settings.low_vram);
        let db = Db::open(&paths.db())?;
        let http = reqwest::Client::builder().user_agent("SafInk/0.1").connect_timeout(std::time::Duration::from_secs(30)).build()?;
        Ok(Arc::new(Self {
            paths,
            db,
            settings: RwLock::new(settings),
            catalog: RwLock::new(Arc::new(catalog)),
            http,
            tiles: TileCache::default(),
            emit,
            job: Mutex::new(None),
            job_counter: AtomicU64::new(1),
            api: Mutex::new(None),
            dl_cancel: Mutex::new(HashMap::new()),
            restart_pending: AtomicBool::new(false),
        }))
    }

    pub fn emit(&self, e: Event) { (self.emit)(e) }

    /// Geçerli (düşük VRAM ayarına göre dosya takımı çözülmüş) katalog.
    pub fn catalog(&self) -> Arc<Catalog> { self.catalog.read().clone() }

    pub fn set_settings(&self, s: Settings) -> Result<()> {
        let mut s = s;
        let (model_changed, vram_changed) = {
            let old = self.settings.read();
            (old.model_id != s.model_id, old.low_vram != s.low_vram)
        };
        if vram_changed {
            s.low_vram_user_set = true; // anahtar elle çevrildi: otomatik seçim artık ezmez
        }
        s.save(&self.paths)?;
        if vram_changed {
            *self.catalog.write() = Arc::new(Catalog::load(&self.paths, s.low_vram));
        }
        *self.settings.write() = s;
        if model_changed || vram_changed {
            // başka model/VRAM kipi: sd.cpp sunucusu yeni bayraklarla yeniden başlasın; iş sürüyorsa iş bitince
            let job = self.job.lock();
            if job.is_some() {
                self.restart_pending.store(true, Ordering::Relaxed);
            } else {
                inpaint::sdserver::shutdown();
            }
        }
        Ok(())
    }

    fn api_client(&self) -> Result<Arc<ApiClient>> {
        let rate = self.settings.read().api_rate_per_min;
        let mut g = self.api.lock();
        if let Some((r, c)) = g.as_ref() {
            if *r == rate {
                return Ok(c.clone());
            }
        }
        let c = Arc::new(ApiClient::new("", rate)?);
        *g = Some((rate, c.clone()));
        Ok(c)
    }

    // ---------------- içe aktarma ----------------

    fn collect_images(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if recursive {
                        Self::collect_images(&p, true, out);
                    }
                } else if imgio::is_image(&p) {
                    out.push(p);
                }
            }
        }
    }

    pub fn sorted_images(dir: &Path, recursive: bool) -> Vec<PathBuf> {
        let mut v = Vec::new();
        Self::collect_images(dir, recursive, &mut v);
        v.sort_by_key(|p| imgio::natural_key(&p.to_string_lossy()));
        v
    }

    fn add_pages(&self, chapter_id: i64, files: &[PathBuf]) -> Result<usize> {
        let mut idx = self.db.next_idx(chapter_id)?;
        let total = files.len();
        let mut n = 0;
        for (i, f) in files.iter().enumerate() {
            let r: Result<()> = (|| {
                let bytes = std::fs::read(f)?;
                let sha = imgio::sha256_hex(&bytes);
                let (w, h) = imgio::dimensions(f)?;
                let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                self.db.add_page(chapter_id, idx, &f.to_string_lossy(), &name, w as i64, h as i64, &sha)?;
                idx += 1;
                n += 1;
                Ok(())
            })();
            if let Err(e) = r {
                tracing::warn!("içe aktarılamadı {}: {e}", f.display());
            }
            if i % 4 == 0 || i + 1 == total {
                self.emit(Event::Import { done: i + 1, total });
            }
        }
        Ok(n)
    }

    /// Klasör görüntü içeriyorsa tek bölüm; yoksa alt klasörlerin her biri bir bölüm. Bölüm id'lerini döner.
    pub fn import_folder(&self, manga_id: i64, folder: &Path) -> Result<Vec<i64>> {
        let direct = Self::sorted_images(folder, false);
        let mut ids = Vec::new();
        if !direct.is_empty() {
            let name = folder.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Bölüm".into());
            let cid = self.db.create_chapter(manga_id, &name, &folder.to_string_lossy(), false)?;
            self.add_pages(cid, &direct)?;
            ids.push(cid);
        } else {
            let mut subs: Vec<PathBuf> = std::fs::read_dir(folder)?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            subs.sort_by_key(|p| imgio::natural_key(&p.to_string_lossy()));
            for s in subs {
                let imgs = Self::sorted_images(&s, true);
                if imgs.is_empty() {
                    continue;
                }
                let name = s.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                let cid = self.db.create_chapter(manga_id, &name, &s.to_string_lossy(), false)?;
                self.add_pages(cid, &imgs)?;
                ids.push(cid);
            }
        }
        if ids.is_empty() {
            bail!(crate::t!("klasörde görüntü bulunamadı: {}", "no images found in folder: {}", folder.display()));
        }
        Ok(ids)
    }

    /// Quick Clean: önceki geçici bölümü siler, verilen dosya/klasörlerden yenisini kurar.
    pub fn quick_new(&self, inputs: &[PathBuf]) -> Result<i64> {
        let mut files = Vec::new();
        for p in inputs {
            if p.is_dir() {
                files.extend(Self::sorted_images(p, true));
            } else if imgio::is_image(p) {
                files.push(p.clone());
            }
        }
        if files.is_empty() {
            bail!(crate::loc::pick("görüntü bulunamadı", "no images found"));
        }
        // girdi doğrulandı: önceki geçici bölüm ancak şimdi silinir
        for pid in self.db.delete_quick_chapters()? {
            for f in [self.paths.edit_file(pid), self.paths.cleaned_file(pid), self.paths.thumb_file(pid)] {
                let _ = std::fs::remove_file(f);
            }
            self.tiles.invalidate(pid);
        }
        let mid = self.db.ensure_manga("__quick__")?;
        let name = format!("Quick Clean {}", chrono_like_stamp());
        let cid = self.db.create_chapter(mid, &name, "", true)?;
        self.add_pages(cid, &files)?;
        Ok(cid)
    }

    pub fn add_files_to_chapter(&self, chapter_id: i64, inputs: &[PathBuf]) -> Result<usize> {
        let mut files = Vec::new();
        for p in inputs {
            if p.is_dir() {
                files.extend(Self::sorted_images(p, true));
            } else if imgio::is_image(p) {
                files.push(p.clone());
            }
        }
        self.add_pages(chapter_id, &files)
    }

    pub fn delete_page_files(&self, page_id: i64) {
        for f in [self.paths.edit_file(page_id), self.paths.cleaned_file(page_id), self.paths.thumb_file(page_id)] {
            let _ = std::fs::remove_file(f);
        }
        self.tiles.invalidate(page_id);
    }

    pub fn delete_chapter_full(&self, id: i64) -> Result<()> {
        for p in self.db.pages(id)? {
            self.delete_page_files(p.id);
        }
        self.db.delete_chapter(id)
    }

    // ---------------- maske / temizleme ----------------

    fn effective(&self, preset: &Option<Preset>) -> (String, f32, CleanParams) {
        let s = self.settings.read();
        let (mid, conf, dilation, feather) = match preset {
            Some(p) => (p.model_id.clone(), p.confidence, p.dilation, p.feather),
            None => (s.model_id.clone(), s.confidence, s.dilation, s.feather),
        };
        (crate::settings::migrate_model_id(&mid), conf, CleanParams { dilation, feather, flat_fill: s.flat_fill })
    }

    async fn ensure_mask(&self, page: &Page, conf: f32, ctl: &JobCtl, on_wait: &(dyn Fn(f64, &str) + Send + Sync)) -> Result<()> {
        let mp = self.paths.mask_file(&page.sha256, conf);
        if !mp.exists() {
            let api = self.api_client()?;
            let src = PathBuf::from(&page.path);
            let mut bytes = tokio::fs::read(&src).await?;
            let mut name = page.name.clone();
            let mut mime = imgio::mime_for(&src).to_string();
            if bytes.len() > MAX_UPLOAD - 512 * 1024 {
                // 15 MB sınırı: boyutu koruyarak JPEG'e yeniden kodla
                let img = tokio::task::spawn_blocking({
                    let s = src.clone();
                    move || imgio::load_rgb(&s)
                })
                .await??;
                let mut q = 90u8;
                loop {
                    let b = imgio::encode_jpeg(&img, q)?;
                    if b.len() <= MAX_UPLOAD - 512 * 1024 || q <= 40 {
                        bytes = b;
                        break;
                    }
                    q -= 10;
                }
                name = format!("{}.jpg", page.sha256);
                mime = "image/jpeg".into();
            }
            let r = api.fetch_mask(&name, bytes, &mime, conf, &ctl.cancel, on_wait).await?;
            // geçerli PNG mi doğrula
            image::load_from_memory(&r.png).map_err(|e| anyhow!(crate::t!("API geçersiz maske döndü: {e}", "API returned an invalid mask: {e}")))?;
            crate::paths::atomic_write(&mp, &r.png)?;
            let _ = std::fs::write(mp.with_extension("det"), r.detections.to_string());
            if matches!(page.status.as_str(), "new" | "error" | "mask_fetched") {
                self.db.set_mask_info(page.id, conf as f64, r.detections)?;
            } else {
                self.db.set_conf(page.id, conf as f64, r.detections)?;
            }
        } else if matches!(page.status.as_str(), "new" | "error") {
            let det = std::fs::read_to_string(mp.with_extension("det")).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(page.detections);
            self.db.set_mask_info(page.id, conf as f64, det)?;
        } else if (page.mask_conf as f32 - conf).abs() > 0.001 {
            self.db.set_conf(page.id, conf as f64, page.detections)?;
        }
        self.tiles.invalidate(page.id);
        emit_page(self, page.id);
        Ok(())
    }

    /// Son maske (API ∪ eklenen − silinen), sayfa boyutunda 0/255.
    pub fn final_mask(&self, page: &Page) -> Result<image::GrayImage> {
        let mp = self.paths.mask_file(&page.sha256, page.mask_conf as f32);
        let api = if mp.exists() { Some(imgio::load_gray(&mp)?) } else { None };
        let ep = self.paths.edit_file(page.id);
        let edit = if ep.exists() { imgio::load_gray(&ep).ok() } else { None };
        if api.is_none() && edit.is_none() {
            bail!(crate::loc::pick("bu sayfa için maske yok (önce maskeleri çekin)", "no mask for this page (fetch masks first)"));
        }
        Ok(maskops::combine(api.as_ref(), edit.as_ref(), page.width as u32, page.height as u32))
    }

    /// Düz zeminler önce doldurulur; yalnızca kalan bileşenler varsa model (`make` ile tembel) yüklenir.
    fn clean_page(&self, pid: i64, bub: &mut crate::bubbles::BubbleSource, inp: &mut Option<Box<dyn inpaint::Inpainter>>, make: &mut dyn FnMut() -> Result<Box<dyn inpaint::Inpainter>>, params: &CleanParams, cancel: &AtomicBool, prog: &dyn Fn(usize, usize)) -> Result<()> {
        let page = self.db.page(pid)?.ok_or_else(|| anyhow!(crate::loc::pick("sayfa yok", "page not found")))?;
        let img = imgio::load_rgb(Path::new(&page.path))?;
        let mask = self.final_mask(&page)?;
        let (mw, mh) = (img.width(), img.height());
        let mask = if mask.dimensions() != (mw, mh) { image::imageops::resize(&mask, mw, mh, image::imageops::FilterType::Nearest) } else { mask };
        let m = maskops::dilate(&mask, params.dilation);
        let out = if params.flat_fill {
            let bubbles = bub.get(&self.paths, &page.sha256, &img);
            let f = crate::pipeline::flat_pass(&img, &m, params.feather, bubbles.as_deref());
            if std::env::var_os("SAFINK_FLAT_LOG").is_some() {
                eprintln!("[düz doldurma] sayfa {pid}: düz={} model={} balon={}", f.stats.flat, f.stats.model, bubbles.as_ref().map(|b| b.len().to_string()).unwrap_or_else(|| "yok".into()));
            }
            if f.stats.model == 0 {
                f.work
            } else {
                if inp.is_none() {
                    *inp = Some(make()?);
                }
                crate::pipeline::clean_dilated(&f.work, f.remaining, params, inp.as_mut().unwrap().as_mut(), cancel, prog)?
            }
        } else {
            if inp.is_none() {
                *inp = Some(make()?);
            }
            crate::pipeline::clean_dilated(&img, m, params, inp.as_mut().unwrap().as_mut(), cancel, prog)?
        };
        imgio::save_png_atomic(&self.paths.cleaned_file(pid), &image::DynamicImage::ImageRgb8(out))?;
        self.db.set_status(pid, "cleaned", "")?;
        self.tiles.invalidate(pid);
        Ok(())
    }

    pub fn current_job(&self) -> Option<Arc<JobCtl>> { self.job.lock().clone() }
    pub fn cancel_job(&self) {
        if let Some(j) = self.job.lock().as_ref() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }
    pub fn pause_job(&self, paused: bool) {
        if let Some(j) = self.job.lock().as_ref() {
            j.paused.store(paused, Ordering::Relaxed);
        }
    }

    pub fn start_job(self: &Arc<Self>, spec: JobSpec) -> Result<u64> {
        let mut g = self.job.lock();
        if g.is_some() {
            bail!(crate::loc::pick("zaten çalışan bir iş var", "a job is already running"));
        }
        let id = self.job_counter.fetch_add(1, Ordering::Relaxed);
        let ctl = Arc::new(JobCtl { cancel: AtomicBool::new(false), paused: AtomicBool::new(false), id });
        *g = Some(ctl.clone());
        drop(g);
        // düğmeye basılır basılmaz arayüz "çalışıyor" görsün: ilk olay model/API beklemeden hemen gider
        self.emit(Event::Job { job_id: id, mode: spec.mode.clone(), total: spec.page_ids.len(), fetched: 0, cleaned: 0, failed: 0, current: None, state: "running".into(), message: crate::loc::pick("başlatılıyor…", "starting…"), wait_secs: 0.0 });
        let me = self.clone();
        tokio::spawn(async move {
            let res = me.clone().run_job(spec.clone(), ctl.clone()).await;
            let (state, msg) = match res {
                Ok(()) if ctl.cancel.load(Ordering::Relaxed) => ("cancelled", crate::loc::pick("iptal edildi", "cancelled")),
                Ok(()) => ("done", String::new()),
                Err(e) => ("error", e.to_string()),
            };
            *me.job.lock() = None;
            if me.restart_pending.swap(false, Ordering::Relaxed) {
                inpaint::sdserver::shutdown();
            }
            me.emit(Event::Job { job_id: id, mode: spec.mode.clone(), total: spec.page_ids.len(), fetched: 0, cleaned: 0, failed: 0, current: None, state: state.into(), message: msg, wait_secs: 0.0 });
        });
        Ok(id)
    }

    async fn run_job(self: Arc<Self>, spec: JobSpec, ctl: Arc<JobCtl>) -> Result<()> {
        let (model_id, conf, params) = self.effective(&spec.preset);
        let clean = spec.mode == "clean";
        let total = spec.page_ids.len();
        let fetched = Arc::new(AtomicUsize::new(0));
        let cleaned = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicUsize::new(0));
        let wait_bits = Arc::new(AtomicU64::new(0));
        let message = Arc::new(Mutex::new(String::new()));
        let current = Arc::new(AtomicU64::new(0));

        let mk = {
            let (me, f, c, fl, w, m, cur, ctl2, mode) = (self.clone(), fetched.clone(), cleaned.clone(), failed.clone(), wait_bits.clone(), message.clone(), current.clone(), ctl.clone(), spec.mode.clone());
            Arc::new(move |state: &str| {
                let cp = cur.load(Ordering::Relaxed) as i64;
                me.emit(Event::Job {
                    job_id: ctl2.id, mode: mode.clone(), total, fetched: f.load(Ordering::Relaxed), cleaned: c.load(Ordering::Relaxed), failed: fl.load(Ordering::Relaxed),
                    current: if cp > 0 { Some(cp) } else { None }, state: state.into(), message: m.lock().clone(), wait_secs: f64::from_bits(w.load(Ordering::Relaxed)),
                });
            })
        };

        // model önceden doğrula (hata erken görünsün)
        let model_spec = if clean {
            let m = self.catalog().get(&model_id).ok_or_else(|| anyhow!(crate::t!("model bulunamadı: {model_id}", "model not found: {model_id}")))?.clone();
            if !m.is_installed(&self.paths) {
                bail!(crate::t!("model kurulu değil: {} (Modeller bölümünden indirin)", "model not installed: {} (download it in Models)", m.name));
            }
            Some(m)
        } else {
            None
        };

        let (tx, rx) = tokio::sync::mpsc::channel::<i64>(8);
        // tüketici (GPU hattı, tek şeritli)
        let consumer = if clean {
            let me = self.clone();
            let (ctl2, mk2, cleaned2, failed2, cur2, msg2) = (ctl.clone(), mk.clone(), cleaned.clone(), failed.clone(), current.clone(), message.clone());
            let ms = model_spec.clone().unwrap();
            let settings = self.settings.read().clone();
            let params2 = params.clone();
            // balon bölütleyici yoksa bir kez sessizce indirmeyi dene (başarısızsa halka kuralı); yalnızca düz doldurma açıkken
            let bubble_model = if params.flat_fill {
                self.ensure_bubble_model(&ctl.cancel).await;
                self.catalog().bubble_model(&self.paths)
            } else {
                None
            };
            Some(tokio::task::spawn_blocking(move || {
                let mut rx = rx;
                let mut inp: Option<Box<dyn inpaint::Inpainter>> = None;
                let mut bub = crate::bubbles::BubbleSource::new(bubble_model);
                while let Some(pid) = rx.blocking_recv() {
                    while ctl2.paused.load(Ordering::Relaxed) && !ctl2.cancel.load(Ordering::Relaxed) {
                        mk2("paused");
                        std::thread::sleep(std::time::Duration::from_millis(400));
                    }
                    if ctl2.cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    cur2.store(pid as u64, Ordering::Relaxed);
                    *msg2.lock() = crate::loc::pick("sayfa temizleniyor", "cleaning page");
                    mk2("running");
                    let mut create_err: Option<anyhow::Error> = None;
                    let r = me.clean_page(
                        pid,
                        &mut bub,
                        &mut inp,
                        &mut || {
                            *msg2.lock() = crate::loc::pick("model yükleniyor…", "loading model…");
                            mk2("running");
                            let (mm, mk3) = (msg2.clone(), mk2.clone());
                            let last = std::cell::Cell::new(std::time::Instant::now());
                            match inpaint::create_with(&me.paths, &ms, &settings, &ctl2.cancel, &|m: String| {
                                *mm.lock() = m;
                                if last.get().elapsed().as_secs() >= 2 {
                                    last.set(std::time::Instant::now());
                                    mk3("running");
                                }
                            }) {
                                Ok(i) => {
                                    *msg2.lock() = i.label();
                                    Ok(i)
                                }
                                Err(e) => {
                                    create_err = Some(anyhow!("{e}"));
                                    Err(e)
                                }
                            }
                        },
                        &params2,
                        &ctl2.cancel,
                        &|d, t| {
                            *msg2.lock() = crate::t!("pencere {d}/{t}", "window {d}/{t}");
                            mk2("running");
                        },
                    );
                    if let Some(e) = create_err {
                        return Err(e);
                    }
                    match r {
                        Ok(()) => {
                            cleaned2.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            if ctl2.cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            failed2.fetch_add(1, Ordering::Relaxed);
                            let _ = me.db.set_status(pid, "error", &e.to_string());
                        }
                    }
                    emit_page(&me, pid);
                    mk2("running");
                }
                Ok(())
            }))
        } else {
            drop(rx);
            None
        };

        // üretici: maske çekme
        for &pid in &spec.page_ids {
            if ctl.cancel.load(Ordering::Relaxed) {
                break;
            }
            while ctl.paused.load(Ordering::Relaxed) && !ctl.cancel.load(Ordering::Relaxed) {
                mk("paused");
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
            let Some(page) = self.db.page(pid)? else { continue };
            if clean && !spec.force && page.status == "cleaned" {
                cleaned.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            current.store(pid as u64, Ordering::Relaxed);
            let me = self.clone();
            let (w2, m2, mk2) = (wait_bits.clone(), message.clone(), mk.clone());
            let on_wait = move |secs: f64, why: &str| {
                w2.store(secs.to_bits(), Ordering::Relaxed);
                *m2.lock() = crate::t!("{why}; {secs:.0} sn bekleniyor", "{why}; waiting {secs:.0} s");
                mk2("running");
            };
            *message.lock() = crate::loc::pick("maske alınıyor", "fetching mask");
            mk("running");
            // maske zaten varsa ve düzenleme-yalnız (API'siz) sayfa değilse ağa gitmez
            match me.ensure_mask(&page, conf, &ctl, &on_wait).await {
                Ok(()) => {
                    wait_bits.store(0f64.to_bits(), Ordering::Relaxed);
                    fetched.fetch_add(1, Ordering::Relaxed);
                    if clean && tx.send(pid).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    if ctl.cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if crate::api::is_fatal(&e) {
                        // günlük sınır / kimlik / sunucuya ulaşılamıyor: her sayfada tekrar etmesin, iş tek iletiyle dursun
                        drop(tx);
                        if let Some(c) = consumer {
                            let _ = c.await;
                        }
                        return Err(e);
                    }
                    failed.fetch_add(1, Ordering::Relaxed);
                    let _ = me.db.set_status(pid, "error", &e.to_string());
                    emit_page(&me, pid);
                }
            }
            mk("running");
        }
        drop(tx);
        if let Some(c) = consumer {
            c.await??;
        }
        Ok(())
    }

    // ---------------- dışa aktarma ----------------

    pub fn export(&self, page_ids: &[i64], out_dir: &Path, format: &str, quality: u8) -> Result<(usize, usize, PathBuf)> {
        let mut skipped = 0;
        let mut used: HashMap<(i64, String), usize> = HashMap::new();
        let mut dir_owner: HashMap<String, i64> = HashMap::new();
        let mut chapter_dir: HashMap<i64, String> = HashMap::new();
        // (sayfa, klasör, dosya adı, biçim, bölüm). Önce hepsi planlanıp özgün dosyaları ezme denetimi yapılır; ancak sonra yazılır.
        let mut plan: Vec<(Page, PathBuf, String, String)> = Vec::new();
        for &pid in page_ids {
            let Some(page) = self.db.page(pid)? else { continue };
            let cf = self.paths.cleaned_file(pid);
            // "cleaned_stale": temizlendikten sonra maske değişti; eldeki temiz sonuç yine de aktarılır
            if !matches!(page.status.as_str(), "cleaned" | "cleaned_stale") || !cf.exists() {
                skipped += 1;
                continue;
            }
            let dname = match chapter_dir.get(&page.chapter_id) {
                Some(d) => d.clone(),
                None => {
                    let ch = self.db.get_chapter(page.chapter_id)?.map(|c| c.name).unwrap_or_else(|| "chapter".into());
                    let d = unique_dir_name(&mut dir_owner, &sanitize(&ch), page.chapter_id);
                    chapter_dir.insert(page.chapter_id, d.clone());
                    d
                }
            };
            let dir = out_dir.join(dname);
            let src_ext = Path::new(&page.name).extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
            let fmt = match format {
                "original" => match src_ext.as_str() {
                    "jpg" | "jpeg" => "jpeg",
                    "webp" => "webp",
                    _ => "png",
                },
                f => f,
            };
            let ext = match fmt { "jpeg" => "jpg", "webp" => "webp", _ => "png" };
            let stem = Path::new(&page.name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| format!("page{}", page.id));
            let n = used.entry((page.chapter_id, format!("{stem}.{ext}"))).or_insert(0);
            *n += 1;
            let fname = if *n > 1 { format!("{stem}_{}.{ext}", *n) } else { format!("{stem}.{ext}") };
            if export_hits_source(&dir, &fname, Path::new(&page.path)) {
                bail!(crate::t!(
                    "dışa aktarma durduruldu: çıktı özgün dosyanın üzerine yazacak veya özgün klasörle aynı ({}). Başka bir çıktı klasörü seçin.",
                    "export stopped: the output would overwrite an original file or is the same as the original folder ({}). Choose a different output folder.",
                    dir.display()
                ));
            }
            plan.push((page, dir, fname, fmt.to_string()));
        }
        let mut ok = 0;
        for (page, dir, fname, fmt) in plan {
            std::fs::create_dir_all(&dir)?;
            let img = imgio::load_rgb(&self.paths.cleaned_file(page.id))?;
            let bytes = match fmt.as_str() {
                "jpeg" => imgio::encode_jpeg(&img, quality)?,
                "webp" => {
                    let mut buf = Vec::new();
                    let enc = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
                    img.write_with_encoder(enc)?;
                    buf
                }
                _ => imgio::encode_png(&image::DynamicImage::ImageRgb8(img))?,
            };
            crate::paths::atomic_write(&dir.join(fname), &bytes)?;
            ok += 1;
        }
        Ok((ok, skipped, out_dir.to_path_buf()))
    }

    // ---------------- modeller ----------------

    /// Balon bölütleyici kurulu değilse (model önceden kurulmuştu) bir kez, sessizce ve en çok 90 sn indirmeyi dener.
    /// Olay yayınlamaz; başarısızlık temizlemeyi engellemez (halka kuralına düşülür).
    async fn ensure_bubble_model(&self, cancel: &AtomicBool) {
        static TRIED: AtomicBool = AtomicBool::new(false);
        if std::env::var("SAFINK_BUBBLES").map(|v| v == "0").unwrap_or(false) {
            return;
        }
        let cat = self.catalog();
        let Some(spec) = cat.get(crate::catalog::BUBBLE_MODEL_ID).cloned() else { return };
        if spec.is_installed(&self.paths) || TRIED.swap(true, Ordering::Relaxed) {
            return;
        }
        let ch = crate::download::Chain { report_id: "", offset: 0, grand: 1, phase: "", last: false };
        let r = tokio::time::timeout(std::time::Duration::from_secs(90), crate::download::install(&self.http, &self.paths, &spec, "", cancel, &ch, &|_| {})).await;
        match r {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("balon modeli indirilemedi: {e}"),
            Err(_) => tracing::warn!("balon modeli indirme zaman aşımı"),
        }
    }

    pub fn hardware(&self) -> &'static Hardware { crate::catalog::hardware() }

    fn runtime_missing(&self, m: &ModelSpec) -> Vec<ModelSpec> {
        let cat = self.catalog();
        m.runtime_ids().into_iter().filter_map(|id| cat.get(id)).filter(|t| !t.is_installed(&self.paths)).cloned().collect()
    }

    pub fn models(&self) -> Vec<ModelView> {
        let hw = self.hardware();
        let cat = self.catalog();
        cat.models
            .iter()
            .map(|m| {
                let installed = m.is_installed(&self.paths);
                let rt: u64 = if installed { 0 } else { self.runtime_missing(m).iter().map(|t| t.runtime_weight()).sum() };
                ModelView {
                    spec: m.clone(),
                    installed,
                    fit: fit_hint(m, hw).to_string(),
                    total_bytes: m.total_size(),
                    downloaded_bytes: if installed { m.total_size() } else { m.downloaded_bytes(&self.paths) },
                    visible: m.visible() && m.kind != "runtime",
                    download_bytes: if installed { 0 } else { m.total_size().saturating_sub(m.downloaded_bytes(&self.paths)) + rt },
                }
            })
            .collect()
    }

    /// Tek tuş: gerekli çalışma ortamı (sd.cpp derlemesi ya da Python ortamı) önce otomatik kurulur, sonra model indirilir.
    /// Arayüz yalnızca `id` için tek ilerleme çubuğu görür (`Download` olayları, `phase` ile Türkçe aşama adı).
    pub fn install_model(self: &Arc<Self>, id: &str) -> Result<()> {
        let spec = self.catalog().get(id).ok_or_else(|| anyhow!(crate::loc::pick("model yok", "no such model")))?.clone();
        if !spec.visible() && spec.kind != "runtime" {
            bail!(crate::loc::pick("bu model bu bilgisayarda kullanılamaz", "this model cannot be used on this computer"));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut g = self.dl_cancel.lock();
            if g.contains_key(id) {
                bail!(crate::loc::pick("bu model zaten indiriliyor", "this model is already downloading"));
            }
            g.insert(id.to_string(), cancel.clone());
        }
        let mut steps: Vec<ModelSpec> = if spec.kind == "runtime" { vec![] } else { self.runtime_missing(&spec) };
        steps.push(spec.clone());
        let me = self.clone();
        let token = self.settings.read().hf_token.clone();
        tokio::spawn(async move {
            let r = me.run_install_chain(&spec.id, steps, &token, &cancel).await;
            me.dl_cancel.lock().remove(&spec.id);
            if let Err(e) = r {
                let msg = if cancel.load(Ordering::Relaxed) { crate::loc::pick("iptal edildi", "cancelled") } else { e.to_string() };
                me.emit(Event::Download(Progress { model_id: spec.id.clone(), file: msg, file_done: 0, file_total: 0, done: 0, total: 0, stage: "error".into(), phase: String::new() }));
            }
        });
        Ok(())
    }

    async fn run_install_chain(self: &Arc<Self>, report_id: &str, steps: Vec<ModelSpec>, token: &str, cancel: &Arc<AtomicBool>) -> Result<()> {
        use crate::download::{progress, Chain};
        let weight = |m: &ModelSpec| if m.kind == "runtime" { m.runtime_weight() } else { m.total_size().saturating_sub(m.downloaded_bytes(&self.paths)).max(1) };
        let grand: u64 = steps.iter().map(weight).sum::<u64>().max(1);
        let n = steps.len();
        let mut offset = 0u64;
        for (i, st) in steps.iter().enumerate() {
            let last = i + 1 == n;
            let phase = if st.kind != "runtime" { crate::loc::pick("Model indiriliyor", "Downloading model") } else if st.variant == "python" { crate::loc::pick("Çalışma ortamı kuruluyor", "Setting up runtime") } else { crate::loc::pick("Çalışma ortamı indiriliyor", "Downloading runtime") };
            let ch = Chain { report_id, offset, grand, phase: &phase, last };
            let em = self.clone();
            let emit = move |p: Progress| em.emit(Event::Download(p));
            let w = weight(st);
            if st.variant == "python" && st.kind == "runtime" {
                if !st.is_installed(&self.paths) {
                    self.install_python(&ch, cancel).await?;
                }
                if last {
                    emit(progress(&ch, "", 0, 0, grand, "done"));
                }
            } else if st.is_installed(&self.paths) {
                if last {
                    emit(progress(&ch, "", 0, 0, grand, "done"));
                }
            } else {
                // kısmen inmiş dosyalar ağırlıktan düşüldüğü için ofseti onlar kadar geri çek
                let already = if st.kind == "runtime" { 0 } else { st.downloaded_bytes(&self.paths) };
                let ch2 = Chain { offset: offset.saturating_sub(already), ..ch };
                crate::download::install(&self.http, &self.paths, st, token, cancel, &ch2, &emit).await?;
            }
            offset += w;
        }
        Ok(())
    }

    /// uv + Python + torch/diffusers/sdnq/spandrel; ilerleme, indirme önbelleğinin büyümesinden tahmin edilir.
    async fn install_python(self: &Arc<Self>, ch: &crate::download::Chain<'_>, cancel: &Arc<AtomicBool>) -> Result<()> {
        let uv = inpaint::pyworker::ensure_uv(&self.http, &self.paths, cancel).await?;
        fn dir_size(p: &Path) -> u64 {
            let mut n = 0;
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    match e.metadata() {
                        Ok(m) if m.is_dir() => n += dir_size(&e.path()),
                        Ok(m) => n += m.len(),
                        _ => {}
                    }
                }
            }
            n
        }
        let (cache, env) = (self.paths.tools().join("uvcache"), self.paths.python_env());
        let base = dir_size(&cache) + dir_size(&env);
        let done_flag = Arc::new(AtomicBool::new(false));
        let poll = {
            let (done_flag, cache, env) = (done_flag.clone(), cache.clone(), env.clone());
            let em = self.clone();
            let (rid, off, grand, phase) = (ch.report_id.to_string(), ch.offset, ch.grand, ch.phase.to_string());
            tokio::task::spawn_blocking(move || {
                while !done_flag.load(Ordering::Relaxed) {
                    let grown = (dir_size(&cache) + dir_size(&env)).saturating_sub(base);
                    let d = grown.min(crate::catalog::ENV_BYTES * 95 / 100);
                    em.emit(Event::Download(Progress {
                        model_id: rid.clone(),
                        file: String::new(),
                        file_done: d,
                        file_total: crate::catalog::ENV_BYTES,
                        done: (off + d).min(grand),
                        total: grand,
                        stage: "downloading".into(),
                        phase: phase.clone(),
                    }));
                    std::thread::sleep(std::time::Duration::from_millis(900));
                }
            })
        };
        let (paths, em, c2) = (self.paths.clone(), self.clone(), cancel.clone());
        let r = tokio::task::spawn_blocking(move || {
            inpaint::pyworker::setup_env(&paths, &uv, "https://download.pytorch.org/whl/cu128", &move |l| em.emit(Event::EnvLog { line: l.to_string() }), &c2)
        })
        .await;
        done_flag.store(true, Ordering::Relaxed);
        let _ = poll.await;
        r??;
        Ok(())
    }

    pub fn cancel_install(&self, id: &str) {
        if let Some(c) = self.dl_cancel.lock().get(id) {
            c.store(true, Ordering::Relaxed);
        }
    }

    pub fn remove_model(&self, id: &str) -> Result<()> {
        let cat = self.catalog();
        let spec = cat.get(id).ok_or_else(|| anyhow!(crate::loc::pick("model yok", "no such model")))?;
        if self.job.lock().is_some() {
            bail!(crate::loc::pick("bir iş çalışırken model silinemez; işin bitmesini bekleyin", "cannot remove a model while a job is running; wait for it to finish"));
        }
        if self.dl_cancel.lock().contains_key(id) {
            bail!(crate::loc::pick("bu model indiriliyor; önce indirmeyi iptal edin", "this model is downloading; cancel the download first"));
        }
        crate::download::remove(&self.paths, spec)
    }
}

fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let t = t.trim().trim_matches('.').to_string();
    if t.is_empty() { "chapter".into() } else { t }
}

/// Aynı adlı iki bölüm tek klasörde birbirini ezmesin: sonraki bölüme " (2)", " (3)" eklenir (büyük/küçük harf duyarsız).
fn unique_dir_name(owner: &mut HashMap<String, i64>, base: &str, chapter_id: i64) -> String {
    let mut n = 1;
    loop {
        let cand = if n == 1 { base.to_string() } else { format!("{base} ({n})") };
        let key = cand.to_lowercase();
        match owner.get(&key) {
            Some(&c) if c != chapter_id => n += 1,
            _ => {
                owner.insert(key, chapter_id);
                return cand;
            }
        }
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Hedef dosya özgün dosyayla aynı mı ya da hedef klasör özgün dosyanın klasörü mü?
fn export_hits_source(target_dir: &Path, target_name: &str, src: &Path) -> bool {
    if same_path(&target_dir.join(target_name), src) {
        return true;
    }
    match src.parent() {
        Some(sd) => !sd.as_os_str().is_empty() && same_path(target_dir, sd),
        None => false,
    }
}

fn chrono_like_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}", secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ayni_adli_bolumler_ayri_klasore_gider() {
        let mut m = HashMap::new();
        assert_eq!(unique_dir_name(&mut m, "Bölüm 1", 1), "Bölüm 1");
        assert_eq!(unique_dir_name(&mut m, "Bölüm 1", 1), "Bölüm 1");
        assert_eq!(unique_dir_name(&mut m, "bölüm 1", 2), "bölüm 1 (2)");
        assert_eq!(unique_dir_name(&mut m, "Bölüm 1", 3), "Bölüm 1 (3)");
    }

    #[test]
    fn disa_aktarma_ozgun_dosyayi_ezmez() {
        let root = std::env::temp_dir().join(format!("safink_exp_{}", std::process::id()));
        let src_dir = root.join("ozgun");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("001.png");
        std::fs::write(&src, b"x").unwrap();
        // aynı klasör (başka uzantı olsa da) ve aynı dosya reddedilir
        assert!(export_hits_source(&src_dir, "001.png", &src));
        assert!(export_hits_source(&src_dir, "001.jpg", &src));
        assert!(export_hits_source(&src_dir.join("..").join("ozgun"), "002.png", &src));
        // başka klasör serbest
        assert!(!export_hits_source(&root.join("cikti").join("ozgun"), "001.png", &src));
        let _ = std::fs::remove_dir_all(&root);
    }
}
