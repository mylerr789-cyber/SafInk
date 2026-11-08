//! ONNX Runtime (CPU) ile sabit 512x512 LaMa. NVIDIA dışı makinelerde kullanılır; DirectML bu modellerde çalışmadığı için kullanılmaz.
use super::{Ctx, Inpainter, WindowPolicy};
use crate::catalog::ModelSpec;
use crate::paths::AppPaths;
use anyhow::{anyhow, bail, Result};
use image::{GrayImage, RgbImage};
use ort::{ep, session::Session, value::Tensor};
use std::sync::OnceLock;

const S: usize = 512;

static ENV: OnceLock<std::result::Result<ort::environment::Environment, String>> = OnceLock::new();

fn env() -> Result<&'static ort::environment::Environment> {
    ENV.get_or_init(|| ort::init().with_name("safink").build().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| anyhow!("ONNX Runtime başlatılamadı: {e}"))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Adapter {
    /// image 0..1, mask 1=sil, çıktı 0..1
    Lama01,
    /// çıktı 0..255
    Lama255,
    /// uint8 giriş/çıkış, mask 255=koru
    MiganU8,
}

pub struct OrtInpainter {
    session: Session,
    adapter: Adapter,
    pub provider: String,
    label: String,
    in_names: (String, String),
}

fn build(path: &std::path::Path) -> Result<Session> {
    let env = env()?;
    let b = Session::builder(env).map_err(|e| anyhow!("{e}"))?;
    let mut b = b.with_execution_providers([ep::CPU::default().build()]).map_err(|e| anyhow!("{e}"))?;
    b.commit_from_file(path).map_err(|e| anyhow!("{e}"))
}

impl OrtInpainter {
    pub fn new(paths: &AppPaths, spec: &ModelSpec) -> Result<Self> {
        let path = spec.file_path(paths, "model").ok_or_else(|| anyhow!("model dosyası tanımsız"))?;
        let adapter = match spec.adapter.as_str() {
            "lama01" => Adapter::Lama01,
            "lama255" => Adapter::Lama255,
            "migan_u8" => Adapter::MiganU8,
            a => bail!("bilinmeyen ort adaptörü: {a}"),
        };
        let session = build(&path)?;
        let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        if names.len() < 2 {
            bail!("model 2 girişli olmalı (image, mask)");
        }
        Ok(Self { label: format!("{} [CPU]", spec.name), session, adapter, provider: "cpu".into(), in_names: (names[0].clone(), names[1].clone()) })
    }
}

impl Inpainter for OrtInpainter {
    fn label(&self) -> String { self.label.clone() }
    fn policy(&self) -> WindowPolicy { WindowPolicy::Fixed(S as u32) }

    fn inpaint(&mut self, img: &RgbImage, mask: &GrayImage, ctx: &Ctx) -> Result<RgbImage> {
        if ctx.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("iptal edildi");
        }
        assert_eq!(img.dimensions(), (S as u32, S as u32));
        let plane = S * S;
        let raw = img.as_raw();
        let mraw = mask.as_raw();
        let (n1, n2) = self.in_names.clone();
        let out_vals: Vec<f32>;
        match self.adapter {
            Adapter::Lama01 | Adapter::Lama255 => {
                let mut x = vec![0f32; 3 * plane];
                for i in 0..plane {
                    for c in 0..3 {
                        x[c * plane + i] = raw[i * 3 + c] as f32 / 255.0;
                    }
                }
                let m: Vec<f32> = mraw.iter().map(|&v| if v >= 128 { 1.0 } else { 0.0 }).collect();
                let t1 = Tensor::from_array(([1usize, 3, S, S], x)).map_err(|e| anyhow!("{e}"))?;
                let t2 = Tensor::from_array(([1usize, 1, S, S], m)).map_err(|e| anyhow!("{e}"))?;
                let outs = self.session.run(ort::inputs![n1.as_str() => t1, n2.as_str() => t2]).map_err(|e| anyhow!("{e}"))?;
                let (shape, data) = outs[0].try_extract_tensor::<f32>().map_err(|e| anyhow!("{e}"))?;
                if data.len() != 3 * plane {
                    bail!("beklenmeyen çıktı şekli {:?}", shape);
                }
                let scale = if self.adapter == Adapter::Lama01 { 255.0 } else { 1.0 };
                out_vals = data.iter().map(|v| v * scale).collect();
            }
            Adapter::MiganU8 => {
                let mut x = vec![0u8; 3 * plane];
                for i in 0..plane {
                    for c in 0..3 {
                        x[c * plane + i] = raw[i * 3 + c];
                    }
                }
                let m: Vec<u8> = mraw.iter().map(|&v| if v >= 128 { 0 } else { 255 }).collect();
                let t1 = Tensor::from_array(([1usize, 3, S, S], x)).map_err(|e| anyhow!("{e}"))?;
                let t2 = Tensor::from_array(([1usize, 1, S, S], m)).map_err(|e| anyhow!("{e}"))?;
                let outs = self.session.run(ort::inputs![n1.as_str() => t1, n2.as_str() => t2]).map_err(|e| anyhow!("{e}"))?;
                let (shape, data) = outs[0].try_extract_tensor::<u8>().map_err(|e| anyhow!("{e}"))?;
                if data.len() != 3 * plane {
                    bail!("beklenmeyen çıktı şekli {:?}", shape);
                }
                out_vals = data.iter().map(|&v| v as f32).collect();
            }
        }
        let mut out = RgbImage::new(S as u32, S as u32);
        for i in 0..plane {
            let px = out.get_pixel_mut((i % S) as u32, (i / S) as u32);
            for c in 0..3 {
                px[c] = out_vals[c * plane + i].round().clamp(0.0, 255.0) as u8;
            }
        }
        Ok(out)
    }
}
