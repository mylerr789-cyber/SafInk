# Hangi modeli seçmeliyim?

[English](MODELS.md) · **Türkçe**

Kısa cevap: **LaMa Manga ile başlayın.** Desenli ya da çizimli zeminde kalan yazılar için **FLUX.2 Klein** kullanın.

Hangi modeli seçerseniz seçin, düz zeminli konuşma balonlarının içindeki yazı (beyaz, siyah, gri) modele hiç gitmez. SafInk balonları küçük bir bölütleme modeliyle bulur ve bunları kendisi anında doldurur. Model yalnızca desenli ve çizimli yerlerde çalışır.

## Karşılaştırma

| Model | Neye iyi | Zayıf yanı | Hız* | Ekran kartı | İndirme |
|---|---|---|---|---|---|
| **LaMa Manga** (önerilen) | Sade arka planlar, tramlar, çoğu sayfa. Keskin sonuç verir, uydurma şekil çizmez. | Çizimin üstündeki büyük yazılarda gri leke bırakabilir. | ~2 sn/sayfa (NVIDIA), CPU'da birkaç kat yavaş | Gerekmez | ~0.2 GB (CPU) · ~3 GB (NVIDIA) |
| **LaMa Genel** | Dokulu, karışık arka planlar | LaMa Manga kadar manga odaklı değil | LaMa Manga ile aynı | Gerekmez | ~0.2 GB · ~3 GB |
| **FLUX.2 Klein 4B** | Desenli arka planlar; AMD ve Intel kartlarda da çalışır | Büyük boşlukları "bir şey çizerek" doldurabilir (saç, gölge vb.) | ~13–17 sn/sayfa | 6 GB+ VRAM | ~5.5 GB |

\* Süreler RTX 4060 Ti 16 GB'ta ölçüldü. Yalnızca modele giden sayfalar için geçerlidir. Düz balonlu sayfalar modelden geçmediği için anında biter.

**Model sayfanın tamamını değil, yalnızca yazılı yerleri işler.** FLUX.2 Klein, MangaTranslator gibi çalışır: kalan her yazı bölgesi çevresiyle birlikte ham kırpıntı olarak (yazı yerinde, üstüne boya sürülmeden) modele gider, küçük kırpıntılar yaklaşık 1 MP'ye büyütülüp geri küçültülür ve sonuç yalnızca o bölgenin yazı piksellerine yazılır. Maske dışında hiçbir piksel değişmez; uzun webtoon şeritleri de aynı yolla işlenir.

## Ekran kartınıza göre öneri

- **NVIDIA, 12 GB ve üstü:** önce LaMa Manga; çizimli yazılar için FLUX.2 Klein.
- **NVIDIA, 8–12 GB:** önce LaMa Manga; gerekirse FLUX.2 Klein.
- **NVIDIA, 4–6 GB:** LaMa Manga önerilir. İsterseniz FLUX.2 Klein 4B (NVIDIA) çok yavaş ama çalışır: Düşük VRAM modunda (bu kartlarda kendiliğinden açık) katman katman işlemci belleğinden akıtılır; 1115×1600 bir sayfada GPU belleği yalnızca ~2,7 GB artıyor (kapalıyken ~7,8 GB) ve çıktı birebir aynı. Süre ölçülen kartta 21 sn yerine 27 sn; 4 GB'lık eski kartlarda sayfa başına ~2 dakika bekleyin (MangaTranslator kullanıcı raporu). sd.cpp Klein bu kartlara yetmez.
- **AMD / Intel:** LaMa Manga (işlemcide çalışır); gerekirse FLUX.2 Klein (Vulkan).
- **Ekran kartı yok:** yalnızca LaMa Manga. Yeniden çizim modelleri işlemcide çok yavaştır.

Gerekli çalışma ortamı (CUDA, Vulkan veya CPU sürümü) ve kartınıza uygun model dosyası otomatik seçilir; sizin bir şey ayarlamanız gerekmez.

## "(NVIDIA)" ile biten modeller

Bunlar aynı modellerin SDNQ sürümleridir ve yalnızca NVIDIA kartlarda görünür. Normal sürüm kartınızda çalışıyorsa onu öneriyoruz.

## Lisanslar

- **LaMa:** MIT / Apache-2.0
- **FLUX.2 Klein 4B:** Apache-2.0

Modeller uygulamayla gelmez, kendi kaynaklarından indirilir.
