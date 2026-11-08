//! Devam ettirilebilir, SHA-256 doğrulamalı indirme + zip çıkarma.
use crate::catalog::{part_path, FileSpec, ModelSpec};
use crate::paths::AppPaths;
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::AsyncWriteExt;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Progress {
    pub model_id: String,
    pub file: String,
    pub file_done: u64,
    pub file_total: u64,
    pub done: u64,
    pub total: u64,
    /// downloading | verifying | extracting | done | error
    pub stage: String,
    /// Kullanıcıya gösterilecek Türkçe aşama adı (zincirli kurulumda).
    pub phase: String,
}

pub fn sha256_file(path: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("iptal edildi");
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Tek dosya: `.part` + Range ile devam, sonunda hash doğrula, rename.
pub async fn download_file(
    client: &reqwest::Client,
    spec: &FileSpec,
    dest: &Path,
    hf_token: &str,
    cancel: &AtomicBool,
    on_bytes: &(dyn Fn(u64, u64) + Send + Sync),
    on_stage: &(dyn Fn(&str) + Send + Sync),
) -> Result<()> {
    if let Ok(m) = std::fs::metadata(dest) {
        if m.len() == spec.size {
            on_bytes(spec.size, spec.size);
            return Ok(());
        }
    }
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p)?;
    }
    let part = part_path(dest);
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > spec.size {
        let _ = std::fs::remove_file(&part);
        have = 0;
    }
    if have < spec.size {
        let mut req = client.get(&spec.url);
        if have > 0 {
            req = req.header("Range", format!("bytes={have}-"));
        }
        if !hf_token.is_empty() && spec.url.contains("huggingface.co") {
            req = req.bearer_auth(hf_token);
        }
        let resp = req.send().await?;
        let st = resp.status();
        if have > 0 && st.as_u16() == 200 {
            // sunucu Range'i yok saydı: baştan
            have = 0;
        } else if !st.is_success() {
            bail!("indirme hatası HTTP {} ({})", st.as_u16(), spec.name);
        }
        let mut f = tokio::fs::OpenOptions::new().create(true).write(true).append(have > 0).truncate(have == 0).open(&part).await?;
        let mut done = have;
        let mut stream = resp.bytes_stream();
        let mut last = std::time::Instant::now();
        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::Relaxed) {
                f.flush().await?;
                bail!("iptal edildi");
            }
            let c = chunk?;
            f.write_all(&c).await?;
            done += c.len() as u64;
            if last.elapsed().as_millis() > 200 {
                on_bytes(done, spec.size);
                last = std::time::Instant::now();
            }
        }
        f.flush().await?;
        drop(f);
        on_bytes(done, spec.size);
    }
    let len = std::fs::metadata(&part)?.len();
    if len != spec.size {
        bail!("boyut uyuşmuyor: {} (beklenen {}, gelen {})", spec.name, spec.size, len);
    }
    if let Some(expected) = &spec.sha256 {
        on_stage("verifying");
        let p2 = part.clone();
        let exp = expected.to_lowercase();
        let cancel_flag = std::sync::Arc::new(AtomicBool::new(cancel.load(Ordering::Relaxed)));
        let got = tokio::task::spawn_blocking(move || sha256_file(&p2, &cancel_flag)).await??;
        if got != exp {
            let _ = std::fs::remove_file(&part);
            bail!("SHA-256 uyuşmuyor ({}); bozuk indirme silindi, tekrar deneyin", spec.name);
        }
    }
    std::fs::rename(&part, dest).with_context(|| format!("rename {}", dest.display()))?;
    Ok(())
}

pub fn extract_zip(zip_path: &Path, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut z = zip::ZipArchive::new(std::fs::File::open(zip_path)?)?;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        let Some(rel) = e.enclosed_name() else { continue };
        let target = out.join(rel);
        if e.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut o = std::fs::File::create(&target)?;
            std::io::copy(&mut e, &mut o)?;
            o.flush()?;
        }
    }
    Ok(())
}

/// Zincirleme kurulumda (çalışma ortamı + model) tek çubuk için konum bilgisi.
pub struct Chain<'a> {
    /// Arayüzün izlediği kimlik (kullanıcının bastığı modelin id'si).
    pub report_id: &'a str,
    /// Bu parçadan önce tamamlanan bayt.
    pub offset: u64,
    /// Tüm zincirin toplam baytı.
    pub grand: u64,
    /// Kullanıcıya gösterilen aşama adı (ör. "Çalışma ortamı indiriliyor").
    pub phase: &'a str,
    /// Zincirin son parçasıysa bitince "done" olayı yayınlansın.
    pub last: bool,
}

pub fn progress(ch: &Chain, file: &str, file_done: u64, file_total: u64, done: u64, stage: &str) -> Progress {
    Progress {
        model_id: ch.report_id.to_string(),
        file: file.to_string(),
        file_done,
        file_total,
        done: (ch.offset + done).min(ch.grand),
        total: ch.grand,
        stage: stage.to_string(),
        phase: ch.phase.to_string(),
    }
}

/// Bir katalog girdisini (bu makineye ait dosyalarıyla) indirir; araçlar için zip'leri çıkarır.
pub async fn install(
    client: &reqwest::Client,
    paths: &AppPaths,
    spec: &ModelSpec,
    hf_token: &str,
    cancel: &AtomicBool,
    ch: &Chain<'_>,
    emit: &(dyn Fn(Progress) + Send + Sync),
) -> Result<()> {
    let mut base_done = 0u64;
    let dir = spec.dir(paths);
    for f in spec.active_files() {
        let dest = dir.join(&f.name);
        let name = f.name.clone();
        let bd = base_done;
        let r = download_file(
            client,
            f,
            &dest,
            hf_token,
            cancel,
            &|d, _| emit(progress(ch, &name, d, f.size, bd + d, "downloading")),
            &|s| emit(progress(ch, &name, f.size, f.size, bd + f.size, s)),
        )
        .await;
        if let Err(e) = r {
            emit(progress(ch, &e.to_string(), 0, 0, bd, "error"));
            return Err(e);
        }
        base_done += f.size;
        if spec.backend == "tool" && f.role == "archive" {
            emit(progress(ch, &f.name, f.size, f.size, base_done, "extracting"));
            let (z, out) = (dest.clone(), spec.tool_dir(paths));
            tokio::task::spawn_blocking(move || extract_zip(&z, &out)).await??;
        }
    }
    if spec.backend != "tool" {
        spec.mark_ok(paths)?;
    }
    if ch.last {
        emit(progress(ch, "", 0, 0, ch.grand, "done"));
    }
    Ok(())
}

pub fn remove(paths: &AppPaths, spec: &ModelSpec) -> Result<()> {
    let d = spec.dir(paths);
    if d.exists() {
        std::fs::remove_dir_all(&d)?;
    }
    if spec.backend == "tool" {
        if spec.variant == "python" {
            let _ = std::fs::remove_dir_all(paths.python_env());
        } else {
            let t = spec.tool_dir(paths);
            if t.exists() {
                std::fs::remove_dir_all(t)?;
            }
        }
    }
    Ok(())
}
