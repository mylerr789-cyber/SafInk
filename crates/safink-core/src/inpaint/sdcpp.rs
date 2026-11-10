//! stable-diffusion.cpp sidecar (`sd-cli.exe`) – her pencere için bir süreç.
//! Bayraklar `sd-cli --help` (master-1060) ile doğrulandı; model çalıştırma TEST EDİLMEDİ (GB'lık indirme yok).
use super::{Ctx, Inpainter, WindowPolicy};
use crate::catalog::ModelSpec;
use crate::paths::AppPaths;
use crate::settings::Settings;
use anyhow::{anyhow, bail, Context, Result};
use image::{GrayImage, RgbImage};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;

pub struct SdCppInpainter {
    exe: PathBuf,
    workdir: PathBuf,
    spec: ModelSpec,
    model_paths: Vec<(String, PathBuf)>,
    counter: u32,
    prep: super::InputPrep,
}

/// Donanıma uygun sd.cpp derlemesini bulur (NVIDIA: CUDA 12, diğer GPU: Vulkan, GPU yok: CPU); yoksa kurulu olan ilk derleme.
pub fn find_sdcli(paths: &AppPaths) -> Option<PathBuf> {
    let want = match crate::catalog::hardware().gpu_kind.as_str() {
        "nvidia" => ["cuda12", "vulkan", "cpu"],
        "other" => ["vulkan", "cpu", "cuda12"],
        _ => ["cpu", "vulkan", "cuda12"],
    };
    want.into_iter().map(|v| paths.tools().join(v).join("sd-cli.exe")).find(|p| p.exists())
}

impl SdCppInpainter {
    pub fn new(paths: &AppPaths, spec: &ModelSpec, settings: &Settings) -> Result<Self> {
        let exe = find_sdcli(paths).ok_or_else(|| anyhow!(crate::loc::pick("çalışma ortamı kurulu değil; modeli yeniden indirin", "runtime is not installed; download the model again")))?;
        let workdir = paths.work().join("sdcpp");
        std::fs::create_dir_all(&workdir)?;
        let model_paths = spec.active_files().into_iter().map(|f| (f.role.clone(), spec.dir(paths).join(&f.name))).collect();
        let prep = super::InputPrep::new(paths, spec, settings)?;
        Ok(Self { exe, workdir, spec: spec.clone(), model_paths, counter: 0, prep })
    }

    fn pf(&self, key: &str, default: f64) -> f64 { self.spec.params.get(key).and_then(|v| v.as_f64()).unwrap_or(default) }
    fn ps(&self, key: &str, default: &str) -> String { self.spec.params.get(key).and_then(|v| v.as_str()).unwrap_or(default).to_string() }
}

impl Inpainter for SdCppInpainter {
    fn label(&self) -> String { format!("sd.cpp · {}", self.spec.name) }
    fn policy(&self) -> WindowPolicy { super::page_policy(&self.spec.params) }

    fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> Result<RgbImage> {
        self.counter += 1;
        let (w0, h0) = img.dimensions();
        let prepared = self.prep.prepare(img, mask, ctx)?;
        let padded = super::pad_for(&prepared, &self.spec.params);
        let (w, h) = padded.dimensions();
        let inp = self.workdir.join(format!("in_{}.png", self.counter));
        let outp = self.workdir.join(format!("out_{}.png", self.counter));
        padded.save(&inp)?;

        let mut cmd = Command::new(&self.exe);
        cmd.current_dir(self.exe.parent().unwrap());
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
            cmd.arg(flag).arg(p);
        }
        let prompt = self.prep.prompt.clone();
        let steps = self.pf("steps", 4.0);
        cmd.arg("-p").arg(&prompt).arg("-W").arg(w.to_string()).arg("-H").arg(h.to_string());
        cmd.arg("--steps").arg((steps as u32).to_string());
        cmd.arg("--cfg-scale").arg(self.pf("cfg", 1.0).to_string());
        if self.spec.params.get("guidance").is_some() {
            cmd.arg("--guidance").arg(self.pf("guidance", 3.5).to_string());
        }
        if let Some(sig) = self.spec.params.get("sigmas").and_then(|v| v.as_str()) {
            cmd.arg("--sigmas").arg(sig); // sabit adım çizelgesi (Qwen-Image 2.1 Turbo: "qwen-image-2.1-turbo")
        }
        cmd.arg("--sampling-method").arg(self.ps("sampler", "euler"));
        cmd.arg("--seed").arg("1");
        // MangaTranslator gibi: kırpıntı referans görüntü (-r) + sabit komut; maske yalnızca geri yazmada kullanılır.
        cmd.arg("-r").arg(&inp);
        if self.spec.params.get("offload").and_then(|v| v.as_bool()).unwrap_or(true) {
            cmd.arg("--offload-to-cpu");
        }
        cmd.arg("--flash-attn").arg("on");
        cmd.arg("-o").arg(&outp);
        let log = self.workdir.join("sd-cli.log");
        let logf = std::fs::File::create(&log)?;
        cmd.stdin(Stdio::null()).stdout(Stdio::from(logf.try_clone()?)).stderr(Stdio::from(logf));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().with_context(|| format!("sd-cli başlatılamadı: {}", self.exe.display()))?;
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
