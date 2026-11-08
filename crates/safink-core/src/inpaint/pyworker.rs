//! Yönetilen Python ortamı (uv + Python 3.12 + torch/diffusers/sdnq/spandrel) ve kalıcı worker süreci (JSON satırları).
//! SDNQ redraw modelleri ve NVIDIA'da PyTorch LaMa aynı ortamı ve aynı süreç yardımcısını kullanır.
use crate::catalog::FileSpec;
use crate::paths::AppPaths;
use anyhow::{anyhow, bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

/// Ortam sürümü: paket listesi değişince artırılır; eski ortam artımlı olarak yeniden kurulur (uv önbellekli, hızlı).
const ENV_VERSION: &str = "v2";

fn marker(paths: &AppPaths) -> PathBuf { paths.python_env().join(".safink_ready") }
pub fn python_exe(paths: &AppPaths) -> PathBuf { paths.python_env().join("Scripts").join("python.exe") }
pub fn env_ready(paths: &AppPaths) -> bool {
    python_exe(paths).exists() && std::fs::read_to_string(marker(paths)).map(|t| t.trim() == ENV_VERSION).unwrap_or(false)
}

pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// uv'yi bulur: tools/uv/uv.exe, sonra PATH.
pub fn find_uv(paths: &AppPaths) -> Option<PathBuf> {
    let local = paths.tools().join("uv").join("uv.exe");
    if local.exists() {
        return Some(local);
    }
    let mut c = Command::new("uv");
    c.arg("--version").stdout(Stdio::null()).stderr(Stdio::null());
    if no_window(&mut c).status().map(|s| s.success()).unwrap_or(false) {
        return Some(PathBuf::from("uv"));
    }
    None
}

/// uv yoksa resmi GitHub sürümünden (astral-sh/uv) indirir (digest doğrulamalı).
pub async fn ensure_uv(client: &reqwest::Client, paths: &AppPaths, cancel: &AtomicBool) -> Result<PathBuf> {
    if let Some(p) = find_uv(paths) {
        return Ok(p);
    }
    let rel: serde_json::Value = client.get("https://api.github.com/repos/astral-sh/uv/releases/latest").header("User-Agent", "safink").send().await?.json().await?;
    let asset = rel["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == "uv-x86_64-pc-windows-msvc.zip"))
        .ok_or_else(|| anyhow!("uv sürümü bulunamadı"))?;
    let spec = FileSpec {
        name: "uv.zip".into(),
        role: "archive".into(),
        url: asset["browser_download_url"].as_str().unwrap_or_default().to_string(),
        sha256: asset["digest"].as_str().map(|d| d.trim_start_matches("sha256:").to_string()),
        size: asset["size"].as_u64().unwrap_or(0),
        only: String::new(),
    };
    let dir = paths.tools().join("uv");
    let zip = dir.join("uv.zip");
    crate::download::download_file(client, &spec, &zip, "", cancel, &|_, _| {}, &|_| {}).await?;
    crate::download::extract_zip(&zip, &dir)?;
    find_uv(paths).ok_or_else(|| anyhow!("uv.exe çıkarılamadı"))
}

fn run_logged(cmd: &mut Command, log: &(dyn Fn(&str) + Send + Sync), cancel: &AtomicBool) -> Result<()> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut child = no_window(cmd).spawn()?;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    std::thread::scope(|s| {
        s.spawn(|| for l in BufReader::new(out).lines().map_while(|l| l.ok()) { log(&l) });
        s.spawn(|| for l in BufReader::new(err).lines().map_while(|l| l.ok()) { log(&l) });
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                break;
            }
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
            }
        }
    });
    let st = child.wait()?;
    if cancel.load(Ordering::Relaxed) {
        bail!("iptal edildi");
    }
    if !st.success() {
        bail!("komut başarısız: {st}");
    }
    Ok(())
}

/// Python ortamını kurar: python 3.12, venv, torch (CUDA 12.8), diffusers, sdnq.
pub fn setup_env(paths: &AppPaths, uv: &Path, cuda_index: &str, log: &(dyn Fn(&str) + Send + Sync), cancel: &AtomicBool) -> Result<()> {
    let pydir = paths.tools().join("uvpython");
    let cache = paths.tools().join("uvcache");
    let env = paths.python_env();
    let uv_cmd = || {
        let mut c = Command::new(uv);
        c.env("UV_PYTHON_INSTALL_DIR", &pydir).env("UV_CACHE_DIR", &cache).env("UV_PYTHON_PREFERENCE", "only-managed");
        c
    };
    log("[1/4] Python 3.12 kuruluyor (uv)…");
    run_logged(uv_cmd().args(["python", "install", "3.12"]), log, cancel)?;
    if !python_exe(paths).exists() {
        log("[2/4] sanal ortam oluşturuluyor…");
        run_logged(uv_cmd().args(["venv", "--python", "3.12"]).arg(&env), log, cancel)?;
    }
    let py = python_exe(paths);
    log("[3/4] torch + torchvision indiriliyor (büyük)…");
    run_logged(uv_cmd().args(["pip", "install", "--python"]).arg(&py).args(["torch", "torchvision", "--index-url", cuda_index]), log, cancel)?;
    log("[4/4] diffusers, sdnq, spandrel kuruluyor…");
    run_logged(
        uv_cmd().args(["pip", "install", "--python"]).arg(&py).args(["diffusers>=0.42", "transformers", "accelerate", "sdnq", "pillow", "safetensors", "sentencepiece", "protobuf", "numpy", "spandrel"]),
        log,
        cancel,
    )?;
    let mut c = Command::new(&py);
    c.args(["-I", "-c", "import torch, diffusers, sdnq; print('torch', torch.__version__, 'cuda', torch.cuda.is_available())"]);
    run_logged(&mut c, log, cancel)?;
    std::fs::write(marker(paths), ENV_VERSION)?;
    Ok(())
}

/// Kalıcı Python worker: stdin'e JSON satırı yazar, stdout'tan JSON satırı okur.
pub struct PyWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    pub workdir: PathBuf,
}

impl PyWorker {
    /// `script_name` workdir'e `source` içeriğiyle yazılır ve `python -I -u` ile başlatılır.
    pub fn spawn(paths: &AppPaths, sub: &str, script_name: &str, source: &str) -> Result<Self> {
        if !env_ready(paths) {
            bail!("çalışma ortamı kurulu değil; modeli yeniden indirin");
        }
        let workdir = paths.work().join(sub);
        std::fs::create_dir_all(&workdir)?;
        let script = workdir.join(script_name);
        std::fs::write(&script, source)?;
        let mut cmd = Command::new(python_exe(paths));
        cmd.args(["-I", "-u"]).arg(&script).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::from(std::fs::File::create(workdir.join("worker.log"))?));
        let mut child = no_window(&mut cmd).spawn().context("python worker başlatılamadı")?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Ok(Self { child, stdin, stdout, workdir })
    }

    pub fn call(&mut self, v: &serde_json::Value) -> Result<serde_json::Value> {
        writeln!(self.stdin, "{}", v)?;
        self.stdin.flush()?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line)? == 0 {
                bail!("python worker beklenmedik şekilde kapandı (work/*/worker.log'a bakın)");
            }
            if line.starts_with('{') {
                return Ok(serde_json::from_str(line.trim())?);
            }
        }
    }
}

impl Drop for PyWorker {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "{{\"cmd\":\"quit\"}}");
        let _ = self.child.kill();
    }
}
