use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub model_id: String,
    pub confidence: f32,
    pub dilation: u32,
    pub feather: u32,
}

impl Default for Preset {
    fn default() -> Self {
        Self { id: "fast".into(), name: "Hızlı".into(), model_id: "lama-manga".into(), confidence: 0.25, dilation: 2, feather: 4 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub confidence: f32,
    /// Maskeyi genişletme (px) – kullanıcı ayarı.
    pub dilation: u32,
    pub feather: u32,
    pub model_id: String,
    pub export_dir: String,
    /// "original" | "png" | "jpeg" | "webp"
    pub export_format: String,
    pub export_quality: u8,
    pub api_rate_per_min: u32,
    pub presets: Vec<Preset>,
    pub hf_token: String,
    pub low_vram: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            confidence: 0.25,
            dilation: 2,
            feather: 4,
            model_id: "lama-manga".into(),
            export_dir: String::new(),
            export_format: "original".into(),
            export_quality: 95,
            api_rate_per_min: 25,
            presets: vec![
                Preset { id: "fast".into(), name: "Hızlı – LaMa Manga".into(), model_id: "lama-manga".into(), ..Default::default() },
                Preset { id: "bold".into(), name: "Kalın yazı – LaMa Manga, geniş maske".into(), model_id: "lama-manga".into(), dilation: 6, feather: 6, ..Default::default() },
                Preset { id: "general".into(), name: "Genel – LaMa Genel".into(), model_id: "lama-big".into(), dilation: 3, ..Default::default() },
                Preset { id: "redraw".into(), name: "Yeniden çiz – Flux.2 Klein".into(), model_id: "sdcpp-flux2-klein-4b".into(), dilation: 4, ..Default::default() },
            ],
            hf_token: String::new(),
            low_vram: true,
        }
    }
}

/// Eski sürümlerden kalan (kaldırılmış) model kimliklerini güncel olanlara çevirir.
pub fn migrate_model_id(id: &str) -> String {
    match id {
        "lama-manga-onnx" => "lama-manga".into(),
        "lama-carve-fp32" => "lama-big".into(),
        "sdcpp-flux2-klein-4b-q4" | "sdcpp-flux2-klein-4b-q8" => "sdcpp-flux2-klein-4b".into(),
        "sdnq-flux2-klein-4b" => id.into(),
        x if x.starts_with("lama-") || x.starts_with("sdcpp-flux2") || x.starts_with("sdnq-flux2") => x.into(),
        _ => "lama-manga".into(),
    }
}

impl Settings {
    pub fn load(paths: &AppPaths) -> Self {
        let mut s: Self = std::fs::read(paths.settings()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        s.model_id = migrate_model_id(&s.model_id);
        for p in &mut s.presets {
            p.model_id = migrate_model_id(&p.model_id);
        }
        s
    }
    pub fn save(&self, paths: &AppPaths) -> anyhow::Result<()> {
        crate::paths::atomic_write(&paths.settings(), &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}
