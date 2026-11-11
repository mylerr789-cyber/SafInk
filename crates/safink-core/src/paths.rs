use std::path::{Path, PathBuf};

/// Uygulama veri dizini düzeni.
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root: PathBuf = root.into();
        // mutlak yol: alt süreçler (sd-cli) başka çalışma dizininde başlar
        let root = std::path::absolute(&root).unwrap_or(root);
        let p = Self { root };
        for d in [p.masks(), p.edits(), p.cleaned(), p.thumbs(), p.models(), p.tools(), p.work()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(p)
    }
    pub fn db(&self) -> PathBuf { self.root.join("safink.db") }
    pub fn settings(&self) -> PathBuf { self.root.join("settings.json") }
    pub fn masks(&self) -> PathBuf { self.root.join("masks") }
    pub fn edits(&self) -> PathBuf { self.root.join("edits") }
    pub fn cleaned(&self) -> PathBuf { self.root.join("cleaned") }
    pub fn thumbs(&self) -> PathBuf { self.root.join("thumbs") }
    pub fn models(&self) -> PathBuf { self.root.join("models") }
    pub fn tools(&self) -> PathBuf { self.root.join("tools") }
    pub fn work(&self) -> PathBuf { self.root.join("work") }
    pub fn python_env(&self) -> PathBuf { self.root.join("tools").join("pyenv") }
    pub fn mask_file(&self, sha: &str, conf: f32) -> PathBuf {
        self.masks().join(format!("{}_c{:03}.png", sha, (conf * 100.0).round() as i32))
    }
    /// Sunucunun sınıflandırdığı yazı parçaları (maskeyle aynı anahtar, `.flat.json`); dosya yoksa veri yok.
    pub fn flat_file(&self, sha: &str, conf: f32) -> PathBuf { self.mask_file(sha, conf).with_extension("flat.json") }
    pub fn edit_file(&self, page_id: i64) -> PathBuf { self.edits().join(format!("{page_id}.png")) }
    pub fn cleaned_file(&self, page_id: i64) -> PathBuf { self.cleaned().join(format!("{page_id}.png")) }
    pub fn thumb_file(&self, page_id: i64) -> PathBuf { self.thumbs().join(format!("{page_id}.jpg")) }
}

/// Atomik yazım: `.tmp` -> rename.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("bin")));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
