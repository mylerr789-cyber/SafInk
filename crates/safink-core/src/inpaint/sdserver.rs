//! stable-diffusion.cpp `sd-server` yöneticisi: model bir kez yüklenir, sayfalar HTTP ile gönderilir.
//! Süreç model/argüman anahtarı başına tek; iş bitince canlı kalır, 10 dk boşta kalırsa, başka model seçilince
//! ya da uygulama kapanınca kapatılır. Windows'ta süreç bir job nesnesine bağlanır (uygulama ölürse sunucu da ölür).
use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const IDLE_LIMIT: Duration = Duration::from_secs(600);
const READY_TIMEOUT: Duration = Duration::from_secs(900);

struct Srv {
    child: Child,
    port: u16,
    key: String,
    last_used: Instant,
}

static SRV: Mutex<Option<Srv>> = Mutex::new(None);
static WATCHER: AtomicBool = AtomicBool::new(false);

/// `SAFINK_SDSERVER=0` sunucu yolunu kapatır (yalnızca sd-cli).
pub fn enabled() -> bool {
    std::env::var("SAFINK_SDSERVER").map(|v| v != "0").unwrap_or(true)
}

pub fn server_exe(cli: &Path) -> Option<PathBuf> {
    let p = cli.parent()?.join("sd-server.exe");
    p.exists().then_some(p)
}

fn rt() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().expect("tokio"))
}

fn client() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(120)).build().expect("reqwest"))
}

/// (durum kodu, gövde)
fn http(method: &str, port: u16, path: &str, body: Option<&serde_json::Value>) -> Result<(u16, Vec<u8>)> {
    let url = format!("http://127.0.0.1:{port}{path}");
    rt().block_on(async {
        let rb = if method == "POST" { client().post(&url) } else { client().get(&url) };
        let rb = if let Some(b) = body { rb.json(b) } else { rb };
        let r = rb.send().await?;
        let st = r.status().as_u16();
        Ok((st, r.bytes().await?.to_vec()))
    })
}

fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(l.local_addr()?.port())
}

#[cfg(windows)]
fn bind_to_job(child: &Child) {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Basic {
        per_proc: i64,
        per_job: i64,
        flags: u32,
        min_ws: usize,
        max_ws: usize,
        active: u32,
        affinity: usize,
        prio: u32,
        sched: u32,
    }
    #[repr(C)]
    struct Ext {
        basic: Basic,
        io: [u64; 6],
        proc_mem: usize,
        job_mem: usize,
        peak_proc: usize,
        peak_job: usize,
    }
    extern "system" {
        fn CreateJobObjectW(attr: *mut c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(job: *mut c_void, class: i32, info: *mut c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    }
    static JOB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let h = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if !h.is_null() {
            let mut e: Ext = std::mem::zeroed();
            e.basic.flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            SetInformationJobObject(h, 9, &mut e as *mut Ext as *mut c_void, std::mem::size_of::<Ext>() as u32);
        }
        h as usize
    });
    if job != 0 {
        unsafe {
            AssignProcessToJobObject(job as *mut c_void, child.as_raw_handle() as *mut c_void);
        }
    }
}
#[cfg(not(windows))]
fn bind_to_job(_c: &Child) {}

fn kill(mut s: Srv) {
    let _ = s.child.kill();
    let _ = s.child.wait();
}

/// Çalışan sunucuyu kapatır (başka model seçilince, uygulama çıkışında).
pub fn shutdown() {
    if let Some(s) = SRV.lock().take() {
        kill(s);
    }
}

fn watcher() {
    if WATCHER.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(15));
        let mut g = SRV.lock();
        let dead = match g.as_mut() {
            Some(s) => s.child.try_wait().ok().flatten().is_some() || s.last_used.elapsed() > IDLE_LIMIT,
            None => false,
        };
        if dead {
            if let Some(s) = g.take() {
                kill(s);
            }
        }
    });
}

fn log_tail(log: &Path) -> String {
    let t = std::fs::read_to_string(log).unwrap_or_default();
    t.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ")
}

/// Aynı anahtarlı canlı sunucuyu döndürür; yoksa başlatır ve hazır olana dek bekler. Dönen değer: bağlantı noktası.
pub fn ensure(exe: &Path, args: &[std::ffi::OsString], key: &str, log: &Path, cancel: &AtomicBool, progress: &dyn Fn(String)) -> Result<u16> {
    {
        let mut g = SRV.lock();
        if let Some(s) = g.as_mut() {
            if s.key == key && s.child.try_wait().ok().flatten().is_none() {
                s.last_used = Instant::now();
                return Ok(s.port);
            }
        }
        if let Some(old) = g.take() {
            kill(old); // başka model/ayar ya da çökmüş süreç
        }
    }
    watcher();
    let port = free_port()?;
    let logf = std::fs::File::create(log)?;
    let mut cmd = Command::new(exe);
    cmd.current_dir(exe.parent().unwrap_or(Path::new(".")));
    cmd.arg("--listen-ip").arg("127.0.0.1").arg("--listen-port").arg(port.to_string());
    cmd.args(args);
    cmd.stdin(Stdio::null()).stdout(Stdio::from(logf.try_clone()?)).stderr(Stdio::from(logf));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().with_context(|| format!("sd-server başlatılamadı: {}", exe.display()))?;
    bind_to_job(&child);
    let t0 = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            bail!(crate::loc::pick("iptal edildi", "cancelled"));
        }
        if let Some(st) = child.try_wait()? {
            bail!(crate::t!("sd-server kapandı ({}): {}", "sd-server exited ({}): {}", st, log_tail(log)));
        }
        if t0.elapsed() > READY_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!(crate::loc::pick("sd-server zamanında hazır olmadı", "sd-server did not become ready in time"));
        }
        if let Ok((200, _)) = http("GET", port, "/v1/models", None) {
            break;
        }
        progress(crate::t!("model yükleniyor… {} sn", "loading model… {} s", t0.elapsed().as_secs()));
        std::thread::sleep(Duration::from_millis(500));
    }
    *SRV.lock() = Some(Srv { child, port, key: key.to_string(), last_used: Instant::now() });
    Ok(port)
}

fn touch() {
    if let Some(s) = SRV.lock().as_mut() {
        s.last_used = Instant::now();
    }
}

/// Tek görüntü üretir (`ref_png`: referans görüntü PNG baytları). Dönen: çıktı PNG baytları.
pub fn img_gen(port: u16, prompt: &str, w: u32, h: u32, seed: i64, ref_png: &[u8], cancel: &AtomicBool) -> Result<Vec<u8>> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let body = serde_json::json!({
        "prompt": prompt, "width": w, "height": h, "seed": seed,
        "ref_images": [b64.encode(ref_png)], "output_format": "png"
    });
    let (st, resp) = http("POST", port, "/sdcpp/v1/img_gen", Some(&body))?;
    let v: serde_json::Value = serde_json::from_slice(&resp).unwrap_or_default();
    if st >= 300 {
        bail!("sd-server {st}: {}", v.get("error").map(|e| e.to_string()).unwrap_or_else(|| String::from_utf8_lossy(&resp).chars().take(200).collect()));
    }
    let id = v.get("id").and_then(|x| x.as_str()).ok_or_else(|| anyhow!("sd-server: iş kimliği yok"))?.to_string();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = http("POST", port, &format!("/sdcpp/v1/jobs/{id}/cancel"), Some(&serde_json::json!({})));
            bail!(crate::loc::pick("iptal edildi", "cancelled"));
        }
        touch();
        let (st, resp) = http("GET", port, &format!("/sdcpp/v1/jobs/{id}"), None)?;
        let v: serde_json::Value = serde_json::from_slice(&resp).unwrap_or_default();
        if st >= 300 {
            bail!("sd-server {st}");
        }
        match v.get("status").and_then(|s| s.as_str()).unwrap_or("") {
            "completed" => {
                let img = v.pointer("/result/images/0/b64_json").and_then(|x| x.as_str()).ok_or_else(|| anyhow!("sd-server: çıktı görüntüsü yok"))?;
                return b64.decode(img).map_err(|e| anyhow!("sd-server çıktısı çözülemedi: {e}"));
            }
            "failed" | "cancelled" => {
                bail!(crate::t!("sd-server üretimi başarısız: {}", "sd-server generation failed: {}", v.get("error").map(|e| e.to_string()).unwrap_or_default()));
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
