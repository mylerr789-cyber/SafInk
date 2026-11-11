# Hangi modeli seçmeliyim?

[English](MODELS.md) · **Türkçe**

Kısa cevap: **LaMa Manga ile başlayın.** Desenli ya da çizimli zeminde kalan yazılar için **Qwen** kullanın.

Hangi modeli seçerseniz seçin, düz zeminli konuşma balonlarının içindeki yazı (beyaz, siyah, gri) modele hiç gitmez. SafInk balonları küçük bir bölütleme modeliyle bulur ve bunları kendisi anında doldurur. Model yalnızca desenli ve çizimli yerlerde çalışır.

## Karşılaştırma

| Model | Neye iyi | Zayıf yanı | Hız* | Ekran kartı | İndirme |
|---|---|---|---|---|---|
| **LaMa Manga** (önerilen) | Sade arka planlar, tramlar, çoğu sayfa. Keskin sonuç verir, uydurma şekil çizmez. | Çizimin üstündeki büyük yazılarda gri leke bırakabilir. | ~2 sn/sayfa (NVIDIA), CPU'da birkaç kat yavaş | Gerekmez | ~0.2 GB (CPU) · ~3 GB (NVIDIA) |
| **LaMa Genel** | Dokulu, karışık arka planlar | LaMa Manga kadar manga odaklı değil | LaMa Manga ile aynı | Gerekmez | ~0.2 GB · ~3 GB |
| **Qwen-Image 2.1 Turbo** | Çizimin üstündeki efekt yazıları (SFX), logolar, karmaşık sahneler. Çizimi en sadık tamamlayan model. | Siyah zemindeki gri yazıda iz bırakabilir; çok ince dokuyu biraz yumuşatır. Yavaştır. | ~20–25 sn/sayfa (16 GB NVIDIA), ~40 sn (daha küçük kartlar) | 12 GB+ VRAM önerilir | ~12–15 GB |
| **FLUX.2 Klein 4B** | Desenli arka planlar; AMD ve Intel kartlarda da çalışır | Büyük boşlukları "bir şey çizerek" doldurabilir (saç, gölge vb.) | ~13–17 sn/sayfa | 6 GB+ VRAM | ~5.5 GB |

\* Süreler RTX 4060 Ti 16 GB'ta ölçüldü. Yalnızca modele giden sayfalar için geçerlidir. Düz balonlu sayfalar modelden geçmediği için anında biter.

**Model sayfanın tamamını değil, yalnızca yazılı yerleri işler.** Kalan yazılar kümelenir; her küme çevresiyle (128 px bağlam) birlikte, küçültülmeden, kendi çözünürlüğünde modele gider ve sonuç yalnızca yazının olduğu piksellere yazılır. Maske dışında hiçbir piksel değişmez; uzun webtoon şeritleri de aynı yolla işlenir. Ölçüm (1115×1600 sayfa, RTX 4060 Ti 16 GB): Qwen ~49 sn → ~28 sn, Klein ~17 sn → ~14 sn, LaMa ~1 sn (değişmedi).

## Ekran kartınıza göre öneri

- **NVIDIA, 12 GB ve üstü:** önce LaMa Manga; çizimli yazılar için Qwen.
- **NVIDIA, 8–12 GB:** önce LaMa Manga; gerekirse FLUX.2 Klein.
- **NVIDIA, 4–6 GB:** LaMa Manga önerilir. İsterseniz FLUX.2 Klein 4B (NVIDIA) çok yavaş ama çalışır: Düşük VRAM modunda (bu kartlarda kendiliğinden açık) katman katman işlemci belleğinden akıtılır; 1115×1600 bir sayfada GPU belleği yalnızca ~2,7 GB artıyor (kapalıyken ~7,8 GB) ve çıktı birebir aynı. Süre ölçülen kartta 21 sn yerine 27 sn; 4 GB'lık eski kartlarda sayfa başına ~2 dakika bekleyin (MangaTranslator kullanıcı raporu). Sayfa küçültülmez. Qwen ve sd.cpp Klein bu kartlara yetmez.
- **AMD / Intel:** LaMa Manga (işlemcide çalışır); gerekirse FLUX.2 Klein (Vulkan).
- **Ekran kartı yok:** yalnızca LaMa Manga. Yeniden çizim modelleri işlemcide çok yavaştır.

Gerekli çalışma ortamı (CUDA, Vulkan veya CPU sürümü) ve kartınıza uygun model dosyası otomatik seçilir; sizin bir şey ayarlamanız gerekmez.

## "(NVIDIA)" ile biten modeller

Bunlar aynı modellerin SDNQ sürümleridir ve yalnızca NVIDIA kartlarda görünür. Testlerimizde Qwen için normal sürümden yaklaşık 2 kat yavaştı. Normal sürümü öneriyoruz.

## Lisanslar

- **LaMa:** MIT / Apache-2.0
- **FLUX.2 Klein 4B:** Apache-2.0
- **Qwen-Image 2.1 Turbo:** Qwen Research lisansı. **Ticari kullanıma izin vermez.**

Modeller uygulamayla gelmez, kendi kaynaklarından indirilir.
