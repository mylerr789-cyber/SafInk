# Değişiklikler / Changelog

Her sürümün notları buradan alınır ve GitHub sürüm sayfasına ve uygulamadaki güncelleme bildirimine yazılır.
Yeni bir sürüm çıkmadan önce "Yayınlanmadı" bölümündekiler o sürümün başlığı altına taşınır.

## [Yayınlanmadı]

## [0.6.1] - 2026-11-11

- Qwen ile temizlenen yerlerin birkaç piksel kayık yapıştırılması düzeltildi: karo, Qwen'in beklediği boyuta büyütülüp gönderiliyor ve çıktı yapıştırılmadan önce çevresine göre hizalanıyor.

*English:* Fixed Qwen results being pasted a few pixels off: tiles are sent at the size Qwen expects and the output is aligned to its surroundings before pasting.

## [0.6.0] - 2026-11-11

- Temizleme artık sayfanın tamamını değil, yalnızca yazılı yerleri işliyor (LaMa, FLUX.2 Klein, Qwen hepsinde): yazılar kümelenir, her küme çevresiyle birlikte küçültülmeden modele gider ve sonuç yalnızca maskeli piksellere yazılır. Maske dışında tek piksel bile değişmiyor; eskiden yeniden çizim modelleri sayfanın tamamını baştan çiziyordu (balon çizgileri kayabiliyor, ince çizgiler değişiyordu). 1115×1600 sayfada Qwen ~49 sn → ~28 sn, Klein ~17 → ~14 sn.
- Yeniden çizim (Klein/Qwen) çıktısına MangaTranslator'ın son işlemi eklendi: ışıklılık/renk eşleme, yazıya yakın piksellerin yapıştırılıp uzaktaki çizimin korunması ve ton eşleme.
- Düz renk doldurma artık düz beyaz/siyah yazı alanlarını sunucudaki bir sınıflandırıcıyla (kendi verimizle eğitildi) belirliyor: yalnızca zemini saf beyaz ya da siyah olan yazılar modele gitmeden doldurulur; sanat, ton ya da gri zemin üstündeki yazı her zaman modele gider. Sunucu bilgisi yoksa eski halka kuralı kullanılır.

*English:* Cleaning now processes only the text areas, at native resolution, and writes back only masked pixels — nothing outside the mask changes (Qwen ~49 s → ~28 s per page, much faster on long webtoon pages); MangaTranslator-style post-processing for Klein/Qwen; plain white/black text areas are now decided by a server-side classifier trained on our own data, text over art, tone or grey always goes to the model.

## [0.5.2] - 2026-11-11

- Düz renk doldurma artık yazının altından çizgi, şerit ya da tram geçen yerleri silmiyor; bu yazılar modele gidiyor (önceden beyaz zeminde ama arkasından çizgi geçen yazılar beyazla kapatılıp çizgiyi de siliyordu).
- Sayfalar arasında geçerken temizlenmiş sayfa doğrudan sonuçla açılıyor (her seferinde maskeden sonuca geçmek gerekmiyor).
- Yeni kurulum ekranı görseli ve GitHub sayfası afişi.

*English:* Plain-colour fill no longer wipes lines, stripes or screentone passing under the text (those go to the model); cleaned pages open in result view when switching pages; new installer artwork and README banner.

## [0.5.1] - 2026-11-11

- Yeni logo.
- Uygulama yeni sürüme artık her açılışta ve açık kaldıkça 6 saatte bir bakıyor (eskiden günde bir kezdi, aynı gün çıkan sürüm görünmüyordu).
- Dışa aktarma artık özgün dosyaların üzerine yazmıyor: çıktı klasörü özgün klasörle aynıysa uyarıp duruyor; aynı adlı iki bölüm birbirini ezmiyor ("Bölüm (2)").
- Quick Clean'e görüntü olmayan dosya bırakmak önceki sonuçları silmiyor; dışa aktarılmamış temiz sayfalar varken yeni Quick Clean önce soruyor.
- Temizlenmiş bir sayfanın maskesini düzenlemek onu dışa aktarmadan düşürmüyor; sayfa "Temiz (maske değişti)" olarak görünüyor ve eldeki temiz sonuç aktarılıyor.
- Günlük sınır, kimlik hatası veya sunucuya ulaşılamaması artık işi tek bir açık iletiyle durduruyor (her sayfada ayrı hata vermiyor).
- Modeller sekmesinde "Kullan"dan sonra başka sekmeden Kaydet'e basmak seçilen modeli eskisine döndürmüyor.
- Bir iş sürerken ayarları (model, düşük VRAM) değiştirmek artık çalışan sunucuyu kesmiyor; yeniden başlatma iş bitince yapılıyor. İş veya indirme sürerken model silme engelleniyor.

*English:* New logo; export never overwrites your original files; Quick Clean no longer deletes earlier results when you drop non-image files; pages stay exportable after a mask edit; daily limit / sign-in / connection errors stop the job with one message; model choice in Preferences no longer reverts; changing settings mid-job no longer breaks the running job; update check on every launch.

## [0.5.0] - 2026-11-11

- Linux desteği (deneysel): AppImage ve .deb. Ubuntu 24.04, Debian 13, Fedora 39 veya daha yeni gerekir.
- Flux ve Qwen'in çıktısı artık doğrudan temiz sayfa olarak kullanılıyor; tramlı zeminde yazının yerinde kalan açık dikdörtgenler ve kutu kenarları gitti.
- Düşük VRAM modu (Tercihler ▸ Temizleme): 12 GB'tan küçük kartlarda kendiliğinden açık. Qwen'in bellek ihtiyacı 14 GB'tan ~11 GB'a, Flux'ınki 6 GB'tan 5 GB'a iniyor; biraz daha yavaş.
- 4–6 GB NVIDIA kartlarda FLUX.2 Klein 4B (NVIDIA) artık çalışıyor (çok yavaş): Düşük VRAM modu MangaTranslator gibi ardışık işlemci boşaltma kullanıyor; sayfa başına GPU belleği ~7,8 GB yerine ~2,7 GB, çıktı aynı. Model kartı bu kartlarda "çok yavaş · düşük VRAM" diyor.
- Hangi modelin neye iyi olduğunu anlatan model rehberi (Türkçe ve İngilizce).
- README'nin İngilizce sürümü.

*English:* Linux support (experimental, AppImage/.deb); Flux and Qwen output is used directly (no more light boxes on screentone); Low VRAM mode, auto-on below 12 GB; FLUX.2 Klein (NVIDIA) now runs on 4–6 GB cards (very slow); model guide and English README.

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
