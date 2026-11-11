//! NVIDIA GPU'da LaMa (PyTorch): yönetilen Python ortamında kalıcı worker; model bir kez yüklenir.
//! TEST EDİLMEDİ (bu makinede torch ortamı kurulmadı).
use super::pyworker::PyWorker;
use super::{Ctx, Inpainter, TileSpec};
use crate::catalog::ModelSpec;
use crate::paths::AppPaths;
use anyhow::{anyhow, bail, Result};
use image::{GrayImage, RgbImage};
use std::sync::atomic::Ordering;

const WORKER_PY: &str = include_str!("lama_worker.py");

pub struct TorchLama {
    w: PyWorker,
    label: String,
    counter: u32,
}

impl TorchLama {
    pub fn new(paths: &AppPaths, spec: &ModelSpec) -> Result<Self> {
        let weights = spec.file_path(paths, "torch").ok_or_else(|| anyhow!("PyTorch ağırlığı tanımsız"))?;
        let mut w = PyWorker::spawn(paths, "lama", "lama_worker.py", WORKER_PY)?;
        let r = w.call(&serde_json::json!({
            "cmd": "load", "weights": weights.to_string_lossy(),
            "kind": spec.params.get("torch").and_then(|v| v.as_str()).unwrap_or("auto"),
        }))?;
        if r["ok"] != true {
            bail!(crate::t!("LaMa (GPU) yüklenemedi: {}", "could not load LaMa (GPU): {}", r["error"]));
        }
        Ok(Self { w, label: format!("{} [GPU]", spec.name), counter: 0 })
    }
}

impl Inpainter for TorchLama {
    fn label(&self) -> String { self.label.clone() }
    fn tile_spec(&self) -> TileSpec { TileSpec::DEFAULT }
    fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> Result<RgbImage> {
        if ctx.cancel.load(Ordering::Relaxed) {
            bail!(crate::loc::pick("iptal edildi", "cancelled"));
        }
        self.counter += 1;
        let d = self.w.workdir.clone();
        let (inp, mp, outp) = (d.join(format!("in_{}.png", self.counter)), d.join(format!("mask_{}.png", self.counter)), d.join(format!("out_{}.png", self.counter)));
        // yerel çözünürlük: yalnızca 8'in katına kenar çoğaltarak tamamla (ölçekleme yok), sonra kırp
        let (w0, h0) = img.dimensions();
        let (pw, ph) = ((w0 + 7) / 8 * 8, (h0 + 7) / 8 * 8);
        super::pad_rgb(img, pw, ph).save(&inp)?;
        super::pad_gray(mask, pw, ph).save(&mp)?;
        let r = self.w.call(&serde_json::json!({"cmd": "run", "image": inp.to_string_lossy(), "mask": mp.to_string_lossy(), "out": outp.to_string_lossy()}))?;
        if r["ok"] != true {
            bail!(crate::t!("LaMa (GPU) hatası: {}", "LaMa (GPU) error: {}", r["error"]));
        }
        let out = image::open(&outp)?.to_rgb8();
        for p in [&inp, &mp, &outp] {
            let _ = std::fs::remove_file(p);
        }
        super::crop_to(out, w0, h0, (pw, ph))
    }
}
