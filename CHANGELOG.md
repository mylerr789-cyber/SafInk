# Değişiklikler / Changelog

Her sürümün notları buradan alınır ve GitHub sürüm sayfasına ve uygulamadaki güncelleme bildirimine yazılır.
Yeni bir sürüm çıkmadan önce "Yayınlanmadı" bölümündekiler o sürümün başlığı altına taşınır.

## [Yayınlanmadı]

- Linux desteği (deneysel): AppImage ve .deb.
- Flux ve Qwen'in çıktısı artık doğrudan temiz sayfa olarak kullanılıyor; tramlı zeminde yazının yerinde kalan açık dikdörtgenler ve kutu kenarları gitti.
- Düşük VRAM modu (Tercihler ▸ Temizleme): 12 GB'tan küçük kartlarda kendiliğinden açık. Qwen'in bellek ihtiyacı 14 GB'tan ~11 GB'a, Flux'ınki 6 GB'tan 5 GB'a iniyor; biraz daha yavaş.
- 4–6 GB NVIDIA kartlarda FLUX.2 Klein 4B (NVIDIA) artık çalışıyor (çok yavaş): Düşük VRAM modu MangaTranslator gibi ardışık işlemci boşaltma kullanıyor; sayfa başına GPU belleği ~7,8 GB yerine ~2,7 GB, çıktı aynı. Model kartı bu kartlarda "çok yavaş · düşük VRAM" diyor.
- Hangi modelin neye iyi olduğunu anlatan model rehberi (Türkçe ve İngilizce).
- README'nin İngilizce sürümü.

## [0.4.1] - 2026-11-11

- Qwen-Image 2.1 Turbo ortalama %42 daha hızlı (16 GB ve üstü NVIDIA kartlarda int8 model + önbellek). Kalite değişmedi.
- Daha küçük kartlarda uygulama otomatik olarak eski model dosyalarını kullanıyor; daha önce kurulanlar yeniden indirilmiyor.

*English:* Qwen-Image 2.1 Turbo is about 42% faster on 16 GB+ NVIDIA cards with the same quality; smaller cards keep the previous files automatically.

## [0.4.0] - 2026-11-11

- Otomatik güncelleme: yeni sürüm çıkınca uygulama haber veriyor ve tek tıkla güncelleniyor. Yardım ▸ Güncellemeleri denetle.
- Önce | Sonra görünümünden çıkınca maske ve sonucun kaybolması düzeltildi.
- Maske alma ve temizleme sırasında anında geri bildirim: düğmede dönen simge, üst çubukta "3/12 · P.03 · temizleniyor" gibi ilerleme, biten sayfa hemen yenileniyor.

*English:* Automatic updates; fixed images disappearing after leaving the Before | After view; live progress while fetching masks and cleaning.

## [0.3.0] - 2026-11-11

- Flux ve Qwen modeli bir kez yüklenip açık tutuluyor; her sayfada yeniden yüklenmiyor (Qwen'de sayfa başına ~10 sn kazanç). Çıktı birebir aynı.

*English:* Flux and Qwen stay loaded between pages (about 10 s saved per page with Qwen); output is identical.

## [0.2.0] - 2026-11-11

- Düz balonlar (beyaz, siyah, gri) modele gitmeden anında dolduruluyor; temizlik süresi yarıya indi.
- Yeni model: Qwen-Image 2.1 Turbo.
- Flux ve Qwen'e sayfanın tamamı gidiyor; webtoonlar 2000 px'lik parçalara bölünüyor, küçültme yok.
- Yeşil yöntem: Flux ve Qwen artık silinen yazının yerine sahte yazı uydurmuyor.
- İngilizce arayüz (Tercihler ▸ Dil).
- Sayfaları sürükleyerek sıralama, sayfa silme, bölüm adını değiştirme.
- Önce | Sonra bölmeli görünüm, genişletilmiş maskenin önizlemesi, kısayollar penceresi (? / F1).
- Kütüphanede "kaldığın yerden devam", ilk açılışta model indirme rehberi, dışa aktarma bildirimi.

*English:* Plain balloons are filled instantly without a model; new Qwen-Image 2.1 Turbo; whole-page redraw; no more invented text; English UI; page reorder/delete, chapter rename, before/after split view, dilation preview, shortcuts, resume, export feedback.

## [0.1.0] - 2026-11-08

- İlk sürüm: kütüphane, hızlı temizlik, maske düzenleme, LaMa ve Flux ile temizleme, dışa aktarma.

*English:* First release.
