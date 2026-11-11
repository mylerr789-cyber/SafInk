<p align="center"><img src="docs/logo/mark-1024.png" alt="SafInk" width="160"></p>

# SafInk

[English](README.en.md) · **Türkçe**

Manga ve webtoon sayfalarındaki yazıları temizleyen Windows uygulaması.
Sayfaları bırakın, yazılar bulunsun, tek tıkla temizlensin. Kaçan ya da yanlış işaretlenen yerleri fırçayla düzeltebilirsiniz.

## İndir

[Releases](../../releases) sayfasından `SafInk_x.y.z_x64-setup.exe` dosyasını indirip kurun.
Windows "bilinmeyen yayıncı" uyarısı verirse **Ek bilgi ▸ Yine de çalıştır**.
Yeni sürüm çıkınca uygulama kendini günceller.

## Kullanım

Hangi modeli seçeceğinizi bilmiyorsanız: [Model rehberi](docs/MODELLER.md).

1. **Tercihler ▸ Modeller**'den bir model indirin (önerilen: *LaMa Manga*).
2. Sayfaları ya da klasörü pencereye bırakın veya bir bölüm klasörü içe aktarın.
3. **Sayfayı temizle** ya da **Bölümün tamamı**.
4. **Dışa aktar** ile temiz sayfaları kaydedin.

Arayüz Türkçe ve İngilizcedir (Tercihler ▸ Dil).
Ücretsizdir; günlük bir kullanım sınırı vardır.

## Kaynaktan derleme

Rust (MSVC), Visual Studio 2022 Build Tools ve Node.js 22 gerekir.

```
npm ci
npm --prefix ui ci
npx tauri build
```

## Lisans

SafInk MIT lisanslıdır. Modeller uygulamayla gelmez, kendi kaynaklarından indirilir ve kendi lisanslarını korur:
[LaMa Manga](https://huggingface.co/mayocream/lama-manga) (MIT),
[LaMa](https://github.com/advimman/lama) (Apache-2.0),
[FLUX.2 Klein 4B](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF) (Apache-2.0),
[Qwen-Image 2.1 Turbo](https://huggingface.co/Qwen/Qwen-Image-2.1-Turbo/blob/main/LICENSE) (Qwen Research Lisansı, ticari kullanım yok) ve Qwen3-VL-8B (Apache-2.0),
[stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp) (MIT),
[SDNQ](https://github.com/Disty0/sdnq).
