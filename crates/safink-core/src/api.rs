//! temizle.saficizgi.com `/v1/mask` istemcisi: token-bucket throttle, 429/503 backoff, Retry-After.
use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Yerleşik API adresi; yalnızca geliştirme için `SAFINK_API_BASE` ile değiştirilebilir.
const DEFAULT_API_BASE: &str = "https://temizle.saficizgi.com";
const KEYRING_SERVICE: &str = "SafInk";

fn resolve_base() -> String {
    match std::env::var("SAFINK_API_BASE") {
        Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
        _ => DEFAULT_API_BASE.to_string(),
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn keyring_get(name: &str) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, name).ok()?.get_password().ok().filter(|s| !s.is_empty())
}
fn keyring_set(name: &str, v: &str) {
    if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, name) {
        let _ = e.set_password(v);
    }
}

/// Kurulum başına anonim kimlik: install_id üretir, sunucudan jeton alır, saklar, süre dolunca yeniler.
/// Jeton asla loglanmaz.
struct DeviceAuth {
    /// (jeton, bitiş unix sn)
    cached: tokio::sync::Mutex<Option<(String, u64)>>,
}

impl DeviceAuth {
    fn new() -> Self {
        Self { cached: tokio::sync::Mutex::new(None) }
    }

    fn install_id() -> String {
        if let Some(id) = keyring_get("install_id") {
            return id;
        }
        let id = uuid_v4();
        keyring_set("install_id", &id);
        id
    }

    /// Geçerli jetonu döndürür. `force` ise önbelleği yok sayıp yeniden kaydolur.
    async fn token(&self, http: &reqwest::Client, base: &str, force: bool) -> Result<String> {
        let mut g = self.cached.lock().await;
        if !force {
            if g.is_none() {
                if let (Some(t), Some(e)) = (keyring_get("device_token"), keyring_get("device_exp")) {
                    if let Ok(e) = e.parse::<u64>() {
                        *g = Some((t, e));
                    }
                }
            }
            if let Some((t, e)) = g.as_ref() {
                if *e > now_secs() + 3600 {
                    return Ok(t.clone());
                }
            }
        }
        let id = Self::install_id();
        let res = http
            .post(format!("{base}/v1/device"))
            .json(&serde_json::json!({ "install_id": id }))
            .send()
            .await
            .map_err(|e| anyhow!(crate::t!("sunucuya ulaşılamadı: {}", "could not reach the server: {}", e.without_url())))?;
        let st = res.status();
        if !st.is_success() {
            if st.as_u16() == 429 {
                bail!(crate::loc::pick("Çok hızlı istek, birkaç dakika sonra tekrar deneyin", "Too many requests, try again in a few minutes"));
            }
            bail!(crate::t!("Kimlik doğrulama başarısız (HTTP {})", "Authentication failed (HTTP {})", st.as_u16()));
        }
        let v: serde_json::Value = res.json().await.map_err(|_| anyhow!(crate::loc::pick("Kimlik doğrulama yanıtı okunamadı", "Could not read the authentication response")))?;
        let tok = v["token"].as_str().ok_or_else(|| anyhow!(crate::loc::pick("Kimlik doğrulama yanıtı geçersiz", "Invalid authentication response")))?.to_string();
        let exp = v["expires_at"].as_u64().unwrap_or(now_secs() + 86400);
        keyring_set("device_token", &tok);
        keyring_set("device_exp", &exp.to_string());
        *g = Some((tok.clone(), exp));
        Ok(tok)
    }
}

fn uuid_v4() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub struct MaskResponse {
    pub png: Vec<u8>,
    pub detections: i64,
    pub seg_seconds: f64,
}

/// Basit token-bucket: dakikada `rate` istek, en çok `burst` birikmiş jeton.
pub struct Throttle {
    inner: Mutex<(f64, Instant)>,
    rate_per_sec: f64,
    burst: f64,
}

impl Throttle {
    pub fn new(per_min: u32) -> Self {
        let burst = 3.0;
        Self { inner: Mutex::new((burst, Instant::now())), rate_per_sec: per_min.max(1) as f64 / 60.0, burst }
    }
    /// Jeton alana kadar bekler.
    pub async fn acquire(&self) {
        loop {
            let wait = {
                let mut g = self.inner.lock();
                let now = Instant::now();
                let dt = now.duration_since(g.1).as_secs_f64();
                g.0 = (g.0 + dt * self.rate_per_sec).min(self.burst);
                g.1 = now;
                if g.0 >= 1.0 {
                    g.0 -= 1.0;
                    None
                } else {
                    Some(((1.0 - g.0) / self.rate_per_sec).max(0.05))
                }
            };
            match wait {
                None => return,
                Some(s) => tokio::time::sleep(Duration::from_secs_f64(s)).await,
            }
        }
    }
    /// Sunucu 429 döndüğünde jetonları sıfırla.
    pub fn drain(&self) {
        self.inner.lock().0 = 0.0;
    }
}

pub struct ApiClient {
    http: reqwest::Client,
    pub base: String,
    pub throttle: Throttle,
    auth: DeviceAuth,
}

pub const MAX_UPLOAD: usize = 15 * 1024 * 1024;

impl ApiClient {
    /// `_base` yok sayılır (adres yerleşik; geliştirmede `SAFINK_API_BASE`).
    pub fn new(_base: &str, per_min: u32) -> Result<Self> {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(180)).user_agent("SafInk/0.1").build()?;
        Ok(Self { http, base: resolve_base(), throttle: Throttle::new(per_min), auth: DeviceAuth::new() })
    }

    /// Maske ister. `cancel` true olursa erken çıkar. `on_wait(saniye, neden)` bekleme bilgisi için.
    pub async fn fetch_mask(
        &self,
        file_name: &str,
        bytes: Vec<u8>,
        mime: &str,
        confidence: f32,
        cancel: &std::sync::atomic::AtomicBool,
        on_wait: &(dyn Fn(f64, &str) + Send + Sync),
    ) -> Result<MaskResponse> {
        // Geliştirme: SAFINK_API_KEY varsa X-API-Key kullanılır; aksi halde anonim cihaz jetonu.
        let dev_key = std::env::var("SAFINK_API_KEY").ok().filter(|k| !k.is_empty());
        let mut refreshed = false;
        if bytes.len() > MAX_UPLOAD {
            bail!(crate::t!("dosya 15 MB sınırını aşıyor ({} bayt)", "file exceeds the 15 MB limit ({} bytes)", bytes.len()));
        }
        let url = format!("{}/v1/mask?confidence={:.2}", self.base, confidence.clamp(0.05, 0.95));
        let max_attempts = 6;
        let mut backoff = 2.0f64;
        for attempt in 1..=max_attempts {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                bail!(crate::loc::pick("iptal edildi", "cancelled"));
            }
            self.throttle.acquire().await;
            let part = reqwest::multipart::Part::bytes(bytes.clone()).file_name(file_name.to_string()).mime_str(mime)?;
            let form = reqwest::multipart::Form::new().part("file", part);
            let req = self.http.post(&url).multipart(form);
            let req = match &dev_key {
                Some(k) => req.header("X-API-Key", k.as_str()),
                None => {
                    let t = self.auth.token(&self.http, &self.base, false).await?;
                    req.header("Authorization", format!("Device {t}"))
                }
            };
            let res = req.send().await;
            let retry_after: Option<f64>;
            let reason: String;
            match res {
                Ok(r) => {
                    let st = r.status();
                    if st.is_success() {
                        let det = r.headers().get("x-detections").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
                        let secs = r.headers().get("x-seg-seconds").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                        let png = r.bytes().await?.to_vec();
                        return Ok(MaskResponse { png, detections: det, seg_seconds: secs });
                    }
                    let daily = r.headers().get("x-limit").and_then(|v| v.to_str().ok()) == Some("daily");
                    let ra = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<f64>().ok());
                    let body = r.text().await.unwrap_or_default();
                    let body: String = body.chars().take(200).collect();
                    match st.as_u16() {
                        429 if daily => bail!(crate::loc::pick("Günlük ücretsiz sınıra ulaşıldı, yarın tekrar deneyin", "Daily free limit reached, try again tomorrow")),
                        429 | 503 | 502 | 504 => {
                            if st.as_u16() == 429 {
                                self.throttle.drain();
                                reason = crate::loc::pick("Çok hızlı istek, birkaç saniye bekleniyor", "Too many requests, waiting a few seconds");
                            } else {
                                reason = crate::loc::pick("Sunucu meşgul, birkaç saniye bekleniyor", "Server busy, waiting a few seconds");
                            }
                            retry_after = ra;
                        }
                        401 | 403 if dev_key.is_none() && !refreshed => {
                            // Jeton süresi dolmuş/geçersiz: yenile ve bir kez daha dene.
                            refreshed = true;
                            self.auth.token(&self.http, &self.base, true).await?;
                            continue;
                        }
                        401 | 403 => bail!(crate::t!("Kimlik doğrulanamadı (HTTP {}); biraz sonra tekrar deneyin", "Could not authenticate (HTTP {}); try again shortly", st.as_u16())),
                        413 => bail!(crate::loc::pick("dosya çok büyük (HTTP 413)", "file too large (HTTP 413)")),
                        c => bail!(crate::t!("API hatası HTTP {c}: {body}", "API error HTTP {c}: {body}")),
                    }
                }
                Err(e) if e.is_timeout() || e.is_connect() || e.is_request() => {
                    retry_after = None;
                    reason = crate::t!("Ağ hatası: {}", "Network error: {}", e.without_url());
                }
                Err(e) => return Err(e.without_url().into()),
            }
            if attempt == max_attempts {
                bail!(crate::t!("{reason}; {max_attempts} deneme sonrası vazgeçildi", "{reason}; gave up after {max_attempts} attempts"));
            }
            let jitter = 1.0 + rand::random::<f64>() * 0.25;
            let wait = retry_after.unwrap_or(backoff * jitter).clamp(1.0, 120.0);
            backoff = (backoff * 2.0).min(60.0);
            on_wait(wait, &reason);
            let end = Instant::now() + Duration::from_secs_f64(wait);
            while Instant::now() < end {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    bail!(crate::loc::pick("iptal edildi", "cancelled"));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        bail!(crate::loc::pick("beklenmeyen durum", "unexpected state"))
    }
}
