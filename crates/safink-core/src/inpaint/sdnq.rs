//! SDNQ: 4-bit diffusers modeli, yönetilen Python ortamında kalıcı worker olarak çalışır.
//! Adım/guidance/komut modelin katalog `params` alanından gelir (kullanıcı ayarı değil).
//! Yaklaşım MangaTranslator ile aynı: kırpıntı referans görüntü + sabit metin; maske yalnızca geri yazmada kullanılır.
//! Gerçek model çalıştırma TEST EDİLMEDİ (multi-GB indirme yapılmadı).
use super::pyworker::PyWorker;
use super::{Ctx, Inpainter, WindowPolicy};
use crate::catalog::ModelSpec;
use crate::paths::AppPaths;
use crate::settings::Settings;
use anyhow::{bail, Result};
use image::{GrayImage, RgbImage};
use std::sync::atomic::Ordering;

const WORKER_PY: &str = include_str!("sdnq_worker.py");

pub struct SdnqInpainter {
    w: PyWorker,
    spec: ModelSpec,
    counter: u32,
    steps: u32,
    guidance: f64,
    prep: super::InputPrep,
}

impl SdnqInpainter {
    pub fn new(paths: &AppPaths, spec: &ModelSpec, settings: &Settings) -> Result<Self> {
        let w = PyWorker::spawn(paths, "sdnq", "sdnq_worker.py", WORKER_PY)?;
        let pu = |k: &str, d: u64| spec.params.get(k).and_then(|v| v.as_u64()).unwrap_or(d);
        let mut me = Self {
            w,
            spec: spec.clone(),
            counter: 0,
            steps: pu("steps", 4) as u32,
            guidance: spec.params.get("guidance").and_then(|v| v.as_f64()).unwrap_or(1.0),
            prep: super::InputPrep::new(paths, spec, settings)?,
        };
        let load = serde_json::json!({
            "cmd": "load",
            "dir": spec.dir(paths).to_string_lossy(),
            "pipeline": spec.params.get("pipeline").and_then(|v| v.as_str()).unwrap_or("Flux2KleinPipeline"),
            "low_vram": settings.low_vram,
        });
        let r = me.w.call(&load)?;
        if r["ok"] != true {
            bail!(crate::t!("model yüklenemedi: {}", "could not load model: {}", r["error"]));
        }
        Ok(me)
    }
}

impl Inpainter for SdnqInpainter {
    fn label(&self) -> String { format!("SDNQ · {}", self.spec.name) }
    fn policy(&self) -> WindowPolicy { super::page_policy(&self.spec.params) }
    fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> Result<RgbImage> {
        if ctx.cancel.load(Ordering::Relaxed) {
            bail!(crate::loc::pick("iptal edildi", "cancelled"));
        }
        self.counter += 1;
        let (w0, h0) = img.dimensions();
        let inp = self.w.workdir.join(format!("in_{}.png", self.counter));
        let outp = self.w.workdir.join(format!("out_{}.png", self.counter));
        let prepared = self.prep.prepare(img, mask, ctx)?;
        let padded = super::pad_for(&prepared, &self.spec.params);
        let pd = padded.dimensions();
        padded.save(&inp)?;
        let r = self.w.call(&serde_json::json!({
            "cmd": "edit", "image": inp.to_string_lossy(), "out": outp.to_string_lossy(),
            "prompt": self.prep.prompt, "steps": self.steps, "guidance": self.guidance, "seed": 1,
        }))?;
        if r["ok"] != true {
            bail!(crate::t!("yeniden çizim hatası: {}", "redraw error: {}", r["error"]));
        }
        let out = image::open(&outp)?.to_rgb8();
        for p in [&inp, &outp] {
            let _ = std::fs::remove_file(p);
        }
        super::crop_to(out, w0, h0, pd)
    }
}
