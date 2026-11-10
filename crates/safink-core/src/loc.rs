//! Kullanıcıya görünen arka uç iletilerinin dili. Arayüz açılışta ve dil değişince `set_locale` komutuyla bildirir
//! (ayar dosyasına yazılmaz; kalıcı olan arayüz tarafındadır). Varsayılan: Türkçe.
use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

/// "tr" | "en" (başka değer Türkçe sayılır).
pub fn set(locale: &str) {
    EN.store(locale.trim().to_ascii_lowercase().starts_with("en"), Ordering::Relaxed);
}
pub fn is_en() -> bool { EN.load(Ordering::Relaxed) }
/// Düz iki dilli metin.
pub fn pick(tr: &str, en: &str) -> String { if is_en() { en } else { tr }.to_string() }

/// `t!("tr {x}", "en {x}", args…)`: seçili dile göre `format!`.
#[macro_export]
macro_rules! t {
    ($tr:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::loc::is_en() { format!($en $(, $arg)*) } else { format!($tr $(, $arg)*) }
    };
}
