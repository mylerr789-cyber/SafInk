//! Model kataloğu: gömülü `catalog.json` + isteğe bağlı kullanıcı katalog dosyası (`catalog.user.json`).
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const EMBEDDED: &str = include_str!("../catalog.json");

/// Python ortamı için tahmini indirme boyutu (torch CUDA + diffusers ...); yalnızca ilerleme çubuğu ağırlığı.
pub const ENV_BYTES: u64 = 3_600_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileSpec {
    pub name: String,
    pub role: String,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    pub size: u64,
    /// "" = her makinede | "cuda" = yalnızca NVIDIA (PyTorch) | "cpu" = yalnızca NVIDIA olmayan makineler (ONNX) |
    /// "fast" = yalnızca 16 GB+ NVIDIA (hızlı dosya takımı) | "compact" = diğer makineler ya da eski kurulum (küçük takım);
    /// "fast"/"compact" `Catalog::load` içinde çözülür (bkz. `ModelSpec::resolve_tier`).
    #[serde(default)]
    pub only: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelSpec {
    pub id: String,
    pub name: String,
    /// lama (NVIDIA: PyTorch, diğerleri: ONNX CPU) | sdcpp | sdnq | tool
    pub backend: String,
    /// inpaint | redraw | runtime
    pub kind: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub adapter: String,
    #[serde(default)]
    pub variant: String,
    /// Kartta görünen tek kısa cümle (Türkçe); `description_en` İngilizcesi.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub description_en: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub license_url: String,
    #[serde(default)]
    pub min_vram_gb: f64,
    #[serde(default)]
    pub min_ram_gb: f64,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub recommended: bool,
    #[serde(default)]
    pub hf_repo: String,
    /// Modele özel sabitler: steps, guidance, cfg, sampler, input_mode (original|green|lama_prefill), prompt / prompt_green / prompt_lama_prefill, offload, pipeline, max_width, max_pixels, seg_h, overlap.
    #[serde(default)]
    pub params: serde_json::Value,
    /// "fast" dosya takımı seçilince `params` üzerine eklenen/ezilen değerler (örn. önbellek ayarı, nicemleme etiketi).
    #[serde(default)]
    pub fast_params: serde_json::Value,
    #[serde(default)]
    pub files: Vec<FileSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    #[serde(default)]
    pub sdcpp_tag: String,
    pub models: Vec<ModelSpec>,
}

impl Catalog {
    pub fn load(paths: &AppPaths) -> Catalog {
        let mut c: Catalog = serde_json::from_str(EMBEDDED).expect("gömülü katalog geçersiz");
        if let Ok(b) = std::fs::read(paths.root.join("catalog.user.json")) {
            if let Ok(u) = serde_json::from_slice::<Catalog>(&b) {
                for m in u.models {
                    c.models.retain(|x| x.id != m.id);
                    c.models.push(m);
                }
            }
        }
        for m in c.models.iter_mut() {
            m.resolve_tier(paths);
        }
        c
    }
    pub fn get(&self, id: &str) -> Option<&ModelSpec> { self.models.iter().find(|m| m.id == id) }
}

/// "fast" takımı için en az ekran belleği (16 GB kartlar ~15,99 GB bildirir).
pub const FAST_MIN_VRAM_GB: f64 = 15.0;

impl ModelSpec {
    /// "fast"/"compact" dosya takımlarından birini seçer, ötekini listeden çıkarır. Fast: NVIDIA ve ≥ 16 GB VRAM; ancak
    /// bu makinede küçük takım eksiksiz kuruluysa ve fast takım değilse eski kurulum bozulmasın diye küçük takım kalır
    /// (yeniden indirme yok; model silinip yeniden kurulunca fast gelir).
    pub fn resolve_tier(&mut self, paths: &AppPaths) {
        if !self.files.iter().any(|f| f.only == "fast" || f.only == "compact") {
            return;
        }
        let hw = hardware();
        let eligible = hw.nvidia() && hw.vram_gb >= FAST_MIN_VRAM_GB;
        let dir = self.dir(paths);
        let marker_ok = self.marker(paths).exists();
        let complete = |tier: &str| {
            marker_ok
                && self.files.iter().filter(|f| f.only == tier || f.only.is_empty()).all(|f| std::fs::metadata(dir.join(&f.name)).map(|m| m.len() == f.size).unwrap_or(false))
        };
        let fast = eligible && !(complete("compact") && !complete("fast"));
        let (keep, drop) = if fast { ("fast", "compact") } else { ("compact", "fast") };
        self.files.retain(|f| f.only != drop);
        for f in self.files.iter_mut().filter(|f| f.only == keep) {
            f.only.clear();
        }
        if fast {
            if let (Some(p), Some(fp)) = (self.params.as_object_mut(), self.fast_params.as_object()) {
                for (k, v) in fp {
                    p.insert(k.clone(), v.clone());
                }
            }
        }
    }
    /// Bu makinede gerçekten indirilecek/kullanılacak dosyalar (`only` süzgeci).
    pub fn active_files(&self) -> Vec<&FileSpec> {
        let cuda = hardware().nvidia();
        self.files.iter().filter(|f| match f.only.as_str() { "cuda" => cuda, "cpu" => !cuda, "fast" | "compact" => false, _ => true }).collect()
    }
    pub fn total_size(&self) -> u64 { self.active_files().iter().map(|f| f.size).sum() }
    /// Bu modelin çalışması için otomatik kurulacak çalışma ortamları (kullanıcıdan gizli).
    pub fn runtime_ids(&self) -> Vec<&'static str> {
        let hw = hardware();
        match self.backend.as_str() {
            "sdcpp" => vec![match hw.gpu_kind.as_str() { "nvidia" => "tool-sdcpp-cuda12", "other" => "tool-sdcpp-vulkan", _ => "tool-sdcpp-cpu" }],
            "sdnq" => vec!["tool-sdnq-env"],
            "lama" if hw.nvidia() => vec!["tool-sdnq-env"],
            _ => vec![],
        }
    }
    /// Kullanıcıya gösterilsin mi? (Çalışma ortamları hiç; SDNQ yalnızca NVIDIA'da.)
    pub fn visible(&self) -> bool {
        match self.backend.as_str() {
            "tool" => false,
            "sdnq" => hardware().nvidia(),
            _ => true,
        }
    }
    pub fn dir(&self, paths: &AppPaths) -> PathBuf { paths.models().join(&self.id) }
    pub fn file_path(&self, paths: &AppPaths, role: &str) -> Option<PathBuf> {
        self.active_files().into_iter().find(|f| f.role == role).map(|f| self.dir(paths).join(&f.name))
    }
    /// Çalışma dizini (sd.cpp araçları için çıkarılmış klasör).
    pub fn tool_dir(&self, paths: &AppPaths) -> PathBuf { paths.tools().join(&self.variant) }

    fn marker(&self, paths: &AppPaths) -> PathBuf { self.dir(paths).join(".safink_ok") }

    pub fn is_installed(&self, paths: &AppPaths) -> bool {
        match (self.backend.as_str(), self.variant.as_str()) {
            ("tool", "python") => crate::inpaint::pyworker::env_ready(paths),
            ("tool", _) => self.tool_dir(paths).join("sd-cli.exe").exists(),
            _ => {
                let files = self.active_files();
                if files.is_empty() || !self.marker(paths).exists() {
                    return false;
                }
                files.iter().all(|f| std::fs::metadata(self.dir(paths).join(&f.name)).map(|m| m.len() == f.size).unwrap_or(false))
            }
        }
    }
    pub fn mark_ok(&self, paths: &AppPaths) -> std::io::Result<()> {
        std::fs::create_dir_all(self.dir(paths))?;
        std::fs::write(self.marker(paths), b"ok")
    }
    /// Kurulum bayt sayısı (kısmi indirmeler dahil).
    pub fn downloaded_bytes(&self, paths: &AppPaths) -> u64 {
        self.active_files()
            .iter()
            .map(|f| {
                let p = self.dir(paths).join(&f.name);
                std::fs::metadata(&p).map(|m| m.len()).or_else(|_| std::fs::metadata(part_path(&p)).map(|m| m.len())).unwrap_or(0)
            })
            .sum()
    }
    /// Bu çalışma ortamının (tool) ilerleme çubuğundaki ağırlığı.
    pub fn runtime_weight(&self) -> u64 {
        if self.variant == "python" { ENV_BYTES } else { self.files.iter().map(|f| f.size).sum() }
    }
}

pub fn part_path(p: &std::path::Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Donanım özeti (otomatik çalışma ortamı seçimi ve "ekran kartına uyar mı" ipuçları için).
#[derive(Clone, Debug, Serialize, Default)]
pub struct Hardware {
    pub gpu_name: String,
    pub vram_gb: f64,
    pub ram_gb: f64,
    /// "nvidia" | "other" (AMD/Intel vb.; Vulkan) | "none"
    pub gpu_kind: String,
}

impl Hardware {
    pub fn nvidia(&self) -> bool { self.gpu_kind == "nvidia" }
}

fn no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

static HW: std::sync::OnceLock<Hardware> = std::sync::OnceLock::new();

/// Donanım bir kez algılanır. Geliştirme/test için `SAFINK_HW=nvidia|other|none` ile zorlanabilir.
pub fn hardware() -> &'static Hardware {
    HW.get_or_init(|| {
        let mut hw = detect_hardware();
        if let Ok(f) = std::env::var("SAFINK_HW") {
            if matches!(f.as_str(), "nvidia" | "other" | "none") {
                if f != "nvidia" {
                    hw.vram_gb = 0.0;
                }
                hw.gpu_kind = f;
            }
        }
        hw
    })
}

pub fn detect_hardware() -> Hardware {
    let mut hw = Hardware::default();
    if let Ok(o) = no_window(std::process::Command::new("nvidia-smi").args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])).output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            if let Some(line) = s.lines().next() {
                let mut it = line.split(',');
                hw.gpu_name = it.next().unwrap_or("").trim().to_string();
                hw.vram_gb = it.next().and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0) / 1024.0;
                if !hw.gpu_name.is_empty() {
                    hw.gpu_kind = "nvidia".into();
                }
            }
        }
    }
    // RAM + GPU adları tek PowerShell çağrısında
    if let Ok(o) = no_window(std::process::Command::new("powershell").args([
        "-NoProfile",
        "-Command",
        "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory; (Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }) -join '|'",
    ]))
    .output()
    {
        let text = String::from_utf8_lossy(&o.stdout).to_string();
        let mut lines = text.lines();
        if let Some(v) = lines.next().and_then(|l| l.trim().parse::<f64>().ok()) {
            hw.ram_gb = v / 1024.0 / 1024.0 / 1024.0;
        }
        if let Some(names) = lines.next() {
            let real: Vec<&str> = names
                .split('|')
                .map(|n| n.trim())
                .filter(|n| {
                    let l = n.to_lowercase();
                    !n.is_empty() && !l.contains("basic") && !l.contains("remote") && !l.contains("virtual") && !l.contains("hyper-v") && !l.contains("parsec") && !l.contains("displaylink")
                })
                .collect();
            if hw.gpu_kind.is_empty() {
                if let Some(n) = real.iter().find(|n| n.to_lowercase().contains("nvidia")) {
                    hw.gpu_kind = "nvidia".into(); // nvidia-smi yok ama NVIDIA kartı var
                    hw.gpu_name = n.to_string();
                } else if let Some(n) = real.first() {
                    hw.gpu_kind = "other".into();
                    hw.gpu_name = n.to_string();
                }
            }
        }
    }
    if hw.gpu_kind.is_empty() {
        hw.gpu_kind = "none".into();
    }
    hw
}

/// "fits" | "tight" | "no" | "slow" | "unknown"
pub fn fit_hint(m: &ModelSpec, hw: &Hardware) -> &'static str {
    if m.kind == "runtime" || m.backend == "lama" {
        return "fits";
    }
    if hw.gpu_kind == "none" {
        return "slow";
    }
    if hw.vram_gb <= 0.0 {
        return "unknown";
    }
    if hw.vram_gb >= m.min_vram_gb * 1.15 && (hw.ram_gb <= 0.0 || hw.ram_gb >= m.min_ram_gb) {
        "fits"
    } else if hw.vram_gb >= m.min_vram_gb * 0.6 && (hw.ram_gb <= 0.0 || hw.ram_gb >= m.min_ram_gb) {
        "tight"
    } else {
        "no"
    }
}
