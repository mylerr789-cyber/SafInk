<p align="center"><img src="docs/logo/mark-1024.png" alt="SafInk logosu" width="160"></p>

# SafInk

Manga ve webtoon sayfalarındaki yazıları temizleyen Windows masaüstü uygulaması. Yazı maskesini SafInk maske servisinden
(`https://temizle.saficizgi.com`) alır; silme / yeniden çizme işlemi **kendi bilgisayarınızda** yapılır. Maskeyi elle düzeltebilirsiniz.

## İndirme

[Releases](../../releases) sayfasından `SafInk_x.y.z_x64-setup.exe` yükleyicisini indirip çalıştırın.
Yükleyici kod imzalı değildir; Windows SmartScreen uyarı verirse "Ek bilgi ▸ Yine de çalıştır" deyin. WebView2 Windows 11'de hazır gelir.

## İlk kullanım

1. API anahtarı gerekmez; uygulama ilk açılışta cihazı kendisi tanıtır.
2. **Tercihler ▸ Modeller**'den bir model seçip indirin (varsayılan: *LaMa Manga*). Gerekli çalışma ortamı otomatik kurulur:
   NVIDIA ekran kartında GPU, diğer makinelerde CPU kullanılır.
3. Açılış ekranına sayfaları ya da klasörü bırakın (Quick Clean) veya kütüphaneden bölüm açın / "Bölüm klasörü içe aktar".
4. Sağ panelde ön ayarı ve modeli seçip **Sayfayı temizle / Bölümü temizle**; sonra **Dosya ▸ Bölümü dışa aktar**.
5. Maske düzenleme: fırça (ekle), silgi (çıkar), kement; düzenlemeler ayrı katmandadır, servis maskesi asla ezilmez.

Maske servisi ücretsizdir ve **günlük bir kullanım sınırı** vardır; sınıra ulaşılınca uygulama bunu bildirir, ertesi gün devam edebilirsiniz.

## Kaynaktan derleme

Gerekenler (Windows): Rust (MSVC), Visual Studio 2022 Build Tools (VC++ iş yükü), Node.js 22.

```
npm ci
npm --prefix ui ci
npx tauri dev       # geliştirme (hot reload)
npx tauri build     # NSIS yükleyici: target/release/bundle/nsis/
```

Tarayıcıda (Tauri olmadan) `npm --prefix ui run dev` sahte bir arka uç kullanır; örnek sayfalar telif nedeniyle depoda yoktur,
`ui/mock/` içine kendi `p0.jpg`, `p0_mask.png` ... dosyalarınızı koyabilirsiniz.

## Klasör yapısı

```
SafInk/
  Cargo.toml                 çalışma alanı (workspace)
  crates/safink-core/        çekirdek: db, api (throttle+retry), maskops, pipeline, inpaint/{ort_lama,sdcpp,sdnq}, katalog, indirme, motor
    catalog.json             model kataloğu (tools/gen_catalog.py ile üretilir: gerçek URL + sha256 + boyut)
    examples/e2e.rs          uçtan uca duman testi (CLI)
  src-tauri/                 Tauri uygulaması (komutlar + safink:// karo sunucusu)
  ui/                        Svelte arayüzü (Vite)
  tools/                     gen_catalog.py, cdp.mjs (WebView2 test yardımcısı)
```

## Maske API'si

- `POST /v1/mask` (multipart `file`, `confidence` 0.05–0.95); başlıklar `X-Detections`, `X-Seg-Seconds`.
- İstemci **token-bucket** ile dakikada ~25 istek (ayarlanabilir), tek eşzamanlı istek; 429/503/ağ hatasında `Retry-After`'a ya da
  üstel geri çekilmeye (2→60 sn, jitter, en çok 6 deneme) uyar. 15 MB üstü görseller boyut korunarak JPEG'e yeniden kodlanır.
- **Önbellek:** `masks/<sha256>_c<güven>.png`. Aynı sayfa + aynı eşik için API'ye bir daha gidilmez.
- Maske işleme tak-çıkar: `maskops::combine` + `dilate`; ileride daha iyi işleme aynı yere takılır.

## Temizleme hattı

1. Son maske = API ∪ eklenen − silinen → kullanıcı genişletmesi (disk yapı elemanı, px).
2. Bağlı bileşenler → sınırlı birleştirme ile gruplar → her grup için pencere (ONNX: 512x512 sabit; redraw: metin kutusu + bağlam, ≤1024).
   Büyük gruplar çekirdek karolara bölünür; her pencere yalnızca kendi çekirdeğindeki maskeyi "sahiplenir"; bitenler çıkarılır.
   Uzun şeritler tam çözünürlükte, pencere pencere işlenir (tüm şerit modele verilmez; webtoon için ayrıca şerit kesmeye gerek yok).
3. Model çalışır (tam çözünürlük, küçültme yok).
4. **Yalnızca maskeli pikseller** geri yazılır, kenarda yumuşatma (feather) ile; maske dışı pikseller aynen kalır.
5. Temiz sayfa `cleaned/<id>.png` (kayıpsız) olarak saklanır; dışa aktarma biçimi (orijinal/PNG/JPEG/WebP-kayıpsız, JPEG kalitesi) Ayarlar'dan.

## Modeller ve arka uçlar

`crates/safink-core/catalog.json` (üretici: `python tools/gen_catalog.py > crates/safink-core/catalog.json`) yalnızca güncel modelleri tutar;
her model için dosyalar + URL + sha256 + boyut + `params` (adım, guidance, komut) bulunur. Kullanıcı tek tuşla indirir; gerekli çalışma ortamı otomatik önce kurulur.

| Model | Arka uç (otomatik) |
|---|---|
| **LaMa Manga**, **LaMa Genel** (hızlı silme) | NVIDIA GPU: PyTorch (yönetilen uv Python ortamı, kalıcı worker, `lama-manga.safetensors` / `big-lama.pt`). Diğer: ONNX Runtime CPU (DirectML LaMa'da çalışmadığı için kullanılmaz). Makine yalnızca kendi dosyasını indirir. |
| **FLUX.2 Klein 4B** (yeniden çizim) | stable-diffusion.cpp GGUF (Q4_K_M + Qwen3-4B Q4_K_XL + VAE); NVIDIA: CUDA 12, diğer GPU: Vulkan, GPU yok: CPU. |
| **FLUX.2 Klein 4B (NVIDIA)** | SDNQ 4-bit diffusers, yalnızca NVIDIA'da görünür. |

Redraw yaklaşımı MangaTranslator ile aynıdır (kaynak koddan doğrulandı): yazı grubunun etrafından kırpılan bölge referans görüntü olarak, sabit komutla
("Erase only the Japanese lettering and sound effect characters. ...") modele verilir (4 adım, guidance 1.0, euler); maske modele verilmez, sonuç yalnızca maskeli piksellere feather ile yapıştırılır.
**Test durumu:** ONNX CPU yolu test edildi; sd.cpp, SDNQ ve PyTorch LaMa yolları yazıldı ama çok GB'lık indirme/ortam kurulumu yapılmadığı için çalıştırılmadı.

## Veri konumu

`%APPDATA%\com.safink.app\` — `safink.db`, `settings.json`, `masks/`, `edits/`, `cleaned/`, `thumbs/`, `models/`, `tools/`.
`SAFINK_DATA` ortam değişkeni ile değiştirilebilir.

## Bilinen sınırlar (v0.1)

- DirectML, LaMa'nın FFT bloklarında hata verdiği için kullanılmaz; NVIDIA'da LaMa PyTorch ile GPU'da, diğerlerinde ONNX CPU ile çalışır.
- Redraw modellerinde ışık/ton eşleme yok (yalnızca feather).
- Mask kalite iyileştirmeleri, CBZ/ZIP dışa aktarma, sd-server (kalıcı süreç) sonraki sürüm.
- Önizlemede genişletilmiş maske gösterilmez (yalnızca API + düzenleme katmanı).

## Model lisansları

Model ağırlıkları uygulamayla dağıtılmaz; uygulama onları özgün kaynaklarından indirir ve her biri kendi lisansını korur:

| Model / bileşen | Lisans | Kaynak |
|---|---|---|
| LaMa Manga | MIT | https://huggingface.co/mayocream/lama-manga |
| LaMa Genel (big-lama) | Apache-2.0 | https://github.com/advimman/lama |
| FLUX.2 Klein 4B (GGUF) | Apache-2.0 | https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF |
| FLUX.2 Klein 4B (SDNQ, NVIDIA) | Apache-2.0 | https://huggingface.co/Disty0/FLUX.2-klein-4B-SDNQ-4bit-dynamic |
| stable-diffusion.cpp (çalışma ortamı) | MIT | https://github.com/leejet/stable-diffusion.cpp |
| SDNQ Python ortamı | BSD/Apache | https://github.com/Disty0/sdnq |

SafInk kodu MIT lisanslıdır (`LICENSE`).

## English

SafInk is a Windows desktop app that removes text from manga/webtoon pages. Text masks come from the SafInk mask service
(`https://temizle.saficizgi.com`, free, with a daily limit); cleaning (inpainting/redraw) runs locally on your PC.
Download the installer from [Releases](../../releases) (unsigned: SmartScreen may warn). No API key is needed. On first run open
Preferences > Models and pick a model; the runtime is installed automatically (NVIDIA GPU: GPU, otherwise CPU).
Build from source: `npm ci`, `npm --prefix ui ci`, `npx tauri build`. Model weights are downloaded from their original sources and
keep their own licenses (see the table above). Code: MIT.

