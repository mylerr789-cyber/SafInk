//! stable-diffusion.cpp arka ucu. Varsayılan: model bir kez yüklenen `sd-server` (bkz. `sdserver.rs`);
//! sunucu başlamazsa / kapalıysa her karo için `sd-cli.exe` süreci (eski yol, aynı bayraklar, aynı tohum).
use super::{Ctx, Inpainter, TileSpec};
use crate::catalog::ModelSpec;
use crate::paths::AppPaths;
use crate::settings::Settings;
use anyhow::{anyhow, bail, Context, Result};
use image::{GrayImage, RgbImage};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;

pub struct SdCppInpainter {
    exe: PathBuf,
    workdir: PathBuf,
    spec: ModelSpec,
    model_paths: Vec<(String, PathBuf)>,
    counter: u32,
    prep: super::InputPrep,
    /// sd-server kullanılabilir (kurulu, açık ve başlatma başarısız olmadı).
    use_server: bool,
    low_vram: bool,
}

const SEED: i64 = 1;

/// sd.cpp komut satırı yürütülebilir adı.
pub const SDCLI: &str = if cfg!(windows) { "sd-cli.exe" } else { "sd-cli" };

/// Linux: paylaşımlı kütüphaneler (libstable-diffusion.so, libggml*.so) yürütülebilirin yanındadır.
pub fn lib_path_env(cmd: &mut std::process::Command, exe: &Path) {
    #[cfg(target_os = "linux")]
    if let Some(d) = exe.parent() {
        let mut v = d.as_os_str().to_owned();
        if let Some(old) = std::env::var_os("LD_LIBRARY_PATH").filter(|s| !s.is_empty()) {
            v.push(":");
            v.push(old);
        }
        cmd.env("LD_LIBRARY_PATH", v);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (cmd, exe);
}

/// Donanıma uygun sd.cpp derlemesini bulur (NVIDIA: CUDA 12, diğer GPU: Vulkan, GPU yok: CPU); yoksa kurulu olan ilk derleme.
pub fn find_sdcli(paths: &AppPaths) -> Option<PathBuf> {
    let want = match crate::catalog::hardware().gpu_kind.as_str() {
        "nvidia" if cfg!(windows) => ["cuda12", "vulkan", "cpu"],
        // Linux'ta CUDA derlemesi yok: NVIDIA da Vulkan kullanır
        "nvidia" | "other" => ["vulkan", "cpu", "cuda12"],
        _ => ["cpu", "vulkan", "cuda12"],
    };
    want.into_iter().map(|v| paths.tools().join(v).join(SDCLI)).find(|p| p.exists())
}

impl SdCppInpainter {
    pub fn new(paths: &AppPaths, spec: &ModelSpec, settings: &Settings) -> Result<Self> {
        Self::new_with(paths, spec, settings, &std::sync::atomic::AtomicBool::new(false), &|_| {})
    }

    /// Modeli ilk kullanımdan önce yükler (sunucu yolu); ilerleme iletisi ve iptal desteklenir.
    pub fn new_with(paths: &AppPaths, spec: &ModelSpec, settings: &Settings, cancel: &std::sync::atomic::AtomicBool, progress: &dyn Fn(String)) -> Result<Self> {
        let exe = find_sdcli(paths).ok_or_else(|| anyhow!(crate::loc::pick("çalışma ortamı kurulu değil; modeli yeniden indirin", "runtime is not installed; download the model again")))?;
        let workdir = paths.work().join("sdcpp");
        std::fs::create_dir_all(&workdir)?;
        let model_paths = spec.active_files().into_iter().map(|f| (f.role.clone(), spec.dir(paths).join(&f.name))).collect();
        let prep = super::InputPrep::new(paths, spec, settings)?;
        let use_server = super::sdserver::enabled() && super::sdserver::server_exe(&exe).is_some();
        let mut me = Self { exe, workdir, spec: spec.clone(), model_paths, counter: 0, prep, use_server, low_vram: settings.low_vram };
        if me.use_server {
            if let Err(e) = me.server_port(cancel, progress) {
                if cancel.load(Ordering::Relaxed) {
                    return Err(e);
                }
                tracing::warn!("sd-server başlamadı, sd-cli kullanılacak: {e}");
                me.use_server = false;
            }
        }
        Ok(me)
    }

    fn model_args(&self) -> Vec<OsString> {
        let mut a: Vec<OsString> = Vec::new();
        for (role, p) in &self.model_paths {
            let flag = match role.as_str() {
                "diffusion" => "--diffusion-model",
                "model" => "-m",
                "vae" => "--vae",
                "clip_l" => "--clip_l",
                "t5xxl" => "--t5xxl",
                "llm" => "--llm",
                "llm_vision" => "--llm_vision",
                _ => continue,
            };
            a.push(flag.into());
            a.push(p.into());
        }
        a
    }

    /// Üretim bayrakları (sd-cli'de doğrudan, sd-server'da varsayılan değer olarak aynı bayraklar).
    fn gen_args(&self) -> Vec<OsString> {
        let mut a: Vec<OsString> = Vec::new();
        let mut p = |k: &str, v: String| {
            a.push(k.into());
            a.push(v.into());
        };
        p("--steps", (self.pf("steps", 4.0) as u32).to_string());
        p("--cfg-scale", self.pf("cfg", 1.0).to_string());
        if self.spec.params.get("guidance").is_some() {
            p("--guidance", self.pf("guidance", 3.5).to_string());
        }
        if let Some(sig) = self.spec.params.get("sigmas").and_then(|v| v.as_str()) {
            p("--sigmas", sig.to_string()); // sabit adım çizelgesi (Qwen-Image 2.1 Turbo: "qwen-image-2.1-turbo")
        }
        p("--sampling-method", self.ps("sampler", "euler"));
        // adım önbelleği (örn. "dbcache": 8 adımda ~%20 hız, görüntü farkı ihmal edilebilir); eşik isteğe bağlı
        if let Some(mode) = self.spec.params.get("cache_mode").and_then(|v| v.as_str()) {
            p("--cache-mode", mode.to_string());
            if let Some(th) = self.spec.params.get("cache_threshold").and_then(|v| v.as_f64()) {
                p("--cache-option", format!("threshold={th}"));
            }
        }
        // 24 GB+ ekran kartında ağırlıklar VRAM'de kalır (daha hızlı); altında RAM'e boşaltılır. Düşük VRAM kipi her zaman boşaltır.
        if self.spec.params.get("offload").and_then(|v| v.as_bool()).unwrap_or(true) && (self.low_vram || crate::catalog::hardware().vram_gb < 23.5) {
            a.push("--offload-to-cpu".into());
        }
        // Düşük VRAM: döşemeli VAE (kodlama+çözme). Döşeme boyutu modele göre ölçüldü: Qwen'de varsayılan 256 sayfada gri leke
        // üretti, 384 temiz ve çıktı tepe belleği ~3 GB düşük; metin kodlayıcıyı/VAE'yi CPU'ya almak (--backend te=cpu / vae=cpu)
        // Qwen'de 80 sn / Klein'da 5 dk ekledi, bu yüzden kullanılmaz.
        if self.low_vram {
            a.push("--vae-tiling".into());
            a.push("--vae-tile-size".into());
            a.push((self.pf("vae_tile", 512.0) as u32).to_string().into());
        }
        a.push("--flash-attn".into());
        a.push("on".into());
        a
    }

    fn server_port(&self, cancel: &std::sync::atomic::AtomicBool, progress: &dyn Fn(String)) -> Result<u16> {
        let server = super::sdserver::server_exe(&self.exe).ok_or_else(|| anyhow!("sd-server.exe yok"))?;
        let mut args = self.model_args();
        args.extend(self.gen_args());
        args.push("--eager-load".into());
        let key = format!("{}|{:?}", server.display(), args);
        super::sdserver::ensure(&server, &args, &key, &self.workdir.join("sd-server.log"), cancel, progress)
    }

    /// Sunucu yolu: `Ok(Some(çıktı))`; sunucu kullanılamıyorsa `Ok(None)` (sd-cli'ye düşülür). İptal hata döner.
    fn try_server(&mut self, padded: &RgbImage, ctx: &Ctx) -> Result<Option<RgbImage>> {
        if !self.use_server {
            return Ok(None);
        }
        let mut png = Vec::new();
        padded.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        let (w, h) = padded.dimensions();
        let mut last: Option<anyhow::Error> = None;
        for attempt in 0..2 {
            let r = self.server_port(ctx.cancel, &|_| {}).and_then(|port| super::sdserver::img_gen(port, &self.prep.prompt, w, h, SEED, &png, ctx.cancel));
            match r {
                Ok(bytes) => return Ok(Some(image::load_from_memory(&bytes)?.to_rgb8())),
                Err(e) => {
                    if ctx.cancel.load(Ordering::Relaxed) {
                        return Err(e);
                    }
                    tracing::warn!("sd-server hatası (deneme {}): {e}", attempt + 1);
                    super::sdserver::shutdown(); // çökmüş/takılmış süreç: yeniden başlat
                    last = Some(e);
                }
            }
        }
        tracing::warn!("sd-server kullanılamadı, sd-cli'ye dönülüyor: {}", last.map(|e| e.to_string()).unwrap_or_default());
        self.use_server = false;
        Ok(None)
    }

    fn pf(&self, key: &str, default: f64) -> f64 { self.spec.params.get(key).and_then(|v| v.as_f64()).unwrap_or(default) }
    fn ps(&self, key: &str, default: &str) -> String { self.spec.params.get(key).and_then(|v| v.as_str()).unwrap_or(default).to_string() }
}

impl Inpainter for SdCppInpainter {
    fn label(&self) -> String { format!("sd.cpp · {}", self.spec.name) }
    fn tile_spec(&self) -> TileSpec { TileSpec { postprocess: true, ..TileSpec::DEFAULT } }

    fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> Result<RgbImage> {
        self.counter += 1;
        let (w0, h0) = img.dimensions();
        let prepared = self.prep.prepare(img, mask, ctx)?;
        let padded = super::pad_for(&prepared, &self.spec.params);
        let (w, h) = padded.dimensions();
        let inp = self.workdir.join(format!("in_{}.png", self.counter));
        let outp = self.workdir.join(format!("out_{}.png", self.counter));
        if let Some(out) = self.try_server(&padded, ctx)? {
            return super::crop_to(out, w0, h0, (w, h));
        }
        padded.save(&inp)?;
        let mut cmd = Command::new(&self.exe);
        cmd.current_dir(self.exe.parent().unwrap());
        lib_path_env(&mut cmd, &self.exe);
        cmd.args(self.model_args());
        cmd.args(self.gen_args());
        let prompt = self.prep.prompt.clone();
        cmd.arg("-p").arg(&prompt).arg("-W").arg(w.to_string()).arg("-H").arg(h.to_string());
        cmd.arg("--seed").arg(SEED.to_string());
        // MangaTranslator gibi: kırpıntı referans görüntü (-r) + sabit komut; maske yalnızca geri yazmada kullanılır.
        cmd.arg("-r").arg(&inp);
        cmd.arg("-o").arg(&outp);
        let log = self.workdir.join("sd-cli.log");
        let logf = std::fs::File::create(&log)?;
        cmd.stdin(Stdio::null()).stdout(Stdio::from(logf.try_clone()?)).stderr(Stdio::from(logf));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = crate::proc::spawn_tied(cmd).with_context(|| format!("sd-cli başlatılamadı: {}", self.exe.display()))?;
        // iptal edilebilir bekleme
        let status = loop {
            if ctx.cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                bail!(crate::loc::pick("iptal edildi", "cancelled"));
            }
            if let Some(s) = child.try_wait()? {
                break s;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        };
        if !status.success() || !outp.exists() {
            let err = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: String = err.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
            bail!(crate::t!("sd-cli başarısız ({}): {}", "sd-cli failed ({}): {}", status, tail));
        }
        let out = image::open(&outp)?.to_rgb8();
        for p in [&inp, &outp] {
            let _ = std::fs::remove_file(p);
        }
        super::crop_to(out, w0, h0, (w, h))
    }
}
