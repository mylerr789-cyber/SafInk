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
    pub catalog: Catalog,
    pub http: reqwest::Client,
    pub tiles: TileCache,
    emit: Arc<dyn Fn(Event) + Send + Sync>,
    job: Mutex<Option<Arc<JobCtl>>>,
    job_counter: AtomicU64,
    api: Mutex<Option<(u32, Arc<ApiClient>)>>,
    dl_cancel: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

fn emit_page(e: &Engine, id: i64) {
    if let Ok(Some(p)) = e.db.page(id) {
        (e.emit)(Event::Page { page_id: id, status: p.status, error: p.error, rev: p.rev });
    }
}

impl Engine {
    pub fn new(paths: AppPaths, emit: Arc<dyn Fn(Event) + Send + Sync>) -> Result<Arc<Self>> {
        let settings = Settings::load(&paths);
        let catalog = Catalog::load(&paths);
        let db = Db::open(&paths.db())?;
        let http = reqwest::Client::builder().user_agent("SafInk/0.1").connect_timeout(std::time::Duration::from_secs(30)).build()?;
        Ok(Arc::new(Self {
            paths,
            db,
            settings: RwLock::new(settings),
            catalog,
            http,
            tiles: TileCache::default(),
            emit,
            job: Mutex::new(None),
            job_counter: AtomicU64::new(1),
            api: Mutex::new(None),
            dl_cancel: Mutex::new(HashMap::new()),
        }))
    }

    pub fn emit(&self, e: Event) { (self.emit)(e) }

    pub fn set_settings(&self, s: Settings) -> Result<()> {
        s.save(&self.paths)?;
        *self.settings.write() = s;
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
            bail!("klasörde görüntü bulunamadı: {}", folder.display());
        }
        Ok(ids)
    }

    /// Quick Clean: önceki geçici bölümü siler, verilen dosya/klasörlerden yenisini kurar.
    pub fn quick_new(&self, inputs: &[PathBuf]) -> Result<i64> {
        for pid in self.db.delete_quick_chapters()? {
            for f in [self.paths.edit_file(pid), self.paths.cleaned_file(pid), self.paths.thumb_file(pid)] {
                let _ = std::fs::remove_file(f);
            }
            self.tiles.invalidate(pid);
        }
        let mut files = Vec::new();
        for p in inputs {
            if p.is_dir() {
                files.extend(Self::sorted_images(p, true));
            } else if imgio::is_image(p) {
                files.push(p.clone());
            }
        }
        if files.is_empty() {
            bail!("görüntü bulunamadı");
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
        (crate::settings::migrate_model_id(&mid), conf, CleanParams { dilation, feather })
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
            image::load_from_memory(&r.png).map_err(|e| anyhow!("API geçersiz maske döndü: {e}"))?;
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
            bail!("bu sayfa için maske yok (önce maskeleri çekin)");
        }
        Ok(maskops::combine(api.as_ref(), edit.as_ref(), page.width as u32, page.height as u32))
    }

    fn clean_page(&self, pid: i64, inp: &mut dyn inpaint::Inpainter, params: &CleanParams, cancel: &AtomicBool, prog: &dyn Fn(usize, usize)) -> Result<()> {
        let page = self.db.page(pid)?.ok_or_else(|| anyhow!("sayfa yok"))?;
        let img = imgio::load_rgb(Path::new(&page.path))?;
        let mask = self.final_mask(&page)?;
        let (mw, mh) = (img.width(), img.height());
        let mask = if mask.dimensions() != (mw, mh) { image::imageops::resize(&mask, mw, mh, image::imageops::FilterType::Nearest) } else { mask };
        let out = crate::pipeline::clean_image(&img, &mask, params, inp, cancel, prog)?;
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
            bail!("zaten çalışan bir iş var");
        }
        let id = self.job_counter.fetch_add(1, Ordering::Relaxed);
        let ctl = Arc::new(JobCtl { cancel: AtomicBool::new(false), paused: AtomicBool::new(false), id });
        *g = Some(ctl.clone());
        drop(g);
        let me = self.clone();
        tokio::spawn(async move {
            let res = me.clone().run_job(spec.clone(), ctl.clone()).await;
            let (state, msg) = match res {
                Ok(()) if ctl.cancel.load(Ordering::Relaxed) => ("cancelled", "iptal edildi".to_string()),
                Ok(()) => ("done", String::new()),
                Err(e) => ("error", e.to_string()),
            };
            *me.job.lock() = None;
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
            let m = self.catalog.get(&model_id).ok_or_else(|| anyhow!("model bulunamadı: {model_id}"))?.clone();
            if !m.is_installed(&self.paths) {
                bail!("model kurulu değil: {} (Modeller bölümünden indirin)", m.name);
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
            Some(tokio::task::spawn_blocking(move || {
                let mut rx = rx;
                let mut inp: Option<Box<dyn inpaint::Inpainter>> = None;
                while let Some(pid) = rx.blocking_recv() {
                    while ctl2.paused.load(Ordering::Relaxed) && !ctl2.cancel.load(Ordering::Relaxed) {
                        mk2("paused");
                        std::thread::sleep(std::time::Duration::from_millis(400));
                    }
                    if ctl2.cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    cur2.store(pid as u64, Ordering::Relaxed);
                    if inp.is_none() {
                        *msg2.lock() = "model yükleniyor…".into();
                        mk2("running");
                        match inpaint::create(&me.paths, &ms, &settings) {
                            Ok(i) => {
                                *msg2.lock() = i.label();
                                inp = Some(i);
                            }
                            Err(e) => {
                                return Err(e);
                            }
                        }
                    }
                    mk2("running");
                    let r = me.clean_page(pid, inp.as_mut().unwrap().as_mut(), &params2, &ctl2.cancel, &|d, t| {
                        *msg2.lock() = format!("pencere {d}/{t}");
                    });
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
                *m2.lock() = format!("{why}; {secs:.0} sn bekleniyor");
                mk2("running");
            };
            *message.lock() = "maske alınıyor".into();
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
                    failed.fetch_add(1, Ordering::Relaxed);
                    let _ = me.db.set_status(pid, "error", &e.to_string());
                    emit_page(&me, pid);
                    if e.to_string().contains("API anahtarı") {
                        drop(tx);
                        if let Some(c) = consumer {
                            let _ = c.await;
                        }
                        return Err(e);
                    }
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
        let mut ok = 0;
        let mut skipped = 0;
        let mut used: HashMap<(i64, String), usize> = HashMap::new();
        for &pid in page_ids {
            let Some(page) = self.db.page(pid)? else { continue };
            let cf = self.paths.cleaned_file(pid);
            if page.status != "cleaned" || !cf.exists() {
                skipped += 1;
                continue;
            }
            let ch = self.db.get_chapter(page.chapter_id)?.map(|c| c.name).unwrap_or_else(|| "chapter".into());
            let dir = out_dir.join(sanitize(&ch));
            std::fs::create_dir_all(&dir)?;
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
            let img = imgio::load_rgb(&cf)?;
            let bytes = match fmt {
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

    pub fn hardware(&self) -> &'static Hardware { crate::catalog::hardware() }

    fn runtime_missing(&self, m: &ModelSpec) -> Vec<ModelSpec> {
        m.runtime_ids().into_iter().filter_map(|id| self.catalog.get(id)).filter(|t| !t.is_installed(&self.paths)).cloned().collect()
    }

    pub fn models(&self) -> Vec<ModelView> {
        let hw = self.hardware();
        self.catalog
            .models
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
        let spec = self.catalog.get(id).ok_or_else(|| anyhow!("model yok"))?.clone();
        if !spec.visible() && spec.kind != "runtime" {
            bail!("bu model bu bilgisayarda kullanılamaz");
        }
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut g = self.dl_cancel.lock();
            if g.contains_key(id) {
                bail!("bu model zaten indiriliyor");
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
                let msg = if cancel.load(Ordering::Relaxed) { "iptal edildi".to_string() } else { e.to_string() };
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
            let phase = if st.kind != "runtime" { "Model indiriliyor" } else if st.variant == "python" { "Çalışma ortamı kuruluyor" } else { "Çalışma ortamı indiriliyor" };
            let ch = Chain { report_id, offset, grand, phase, last };
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
        let spec = self.catalog.get(id).ok_or_else(|| anyhow!("model yok"))?;
        crate::download::remove(&self.paths, spec)
    }
}

fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let t = t.trim().trim_matches('.').to_string();
    if t.is_empty() { "chapter".into() } else { t }
}

fn chrono_like_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}", secs)
}
