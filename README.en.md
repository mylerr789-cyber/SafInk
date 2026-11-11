<p align="center"><img src="docs/logo/banner.jpg" alt="SafInk"></p>

# SafInk

**English** · [Türkçe](README.md)

A Windows and Linux (experimental) app that removes text from manga and webtoon pages.
Drop your pages in, the text is found automatically, and one click cleans it. Missed or wrongly marked spots can be fixed with a brush.

## Download

Get `SafInk_x.y.z_x64-setup.exe` from the [Releases](../../releases) page and install it.
If Windows warns about an unknown publisher, choose **More info ▸ Run anyway**.

**Linux (experimental):** run the `SafInk_x.y.z_amd64.AppImage` with `chmod +x SafInk_*.AppImage`, or install the `.deb` with `sudo apt install ./SafInk_*.deb`. Requires `webkit2gtk-4.1` and glibc 2.38+ (Ubuntu 24.04, Debian 13, Fedora 39 or newer). The GPU is used through Vulkan (NVIDIA included); the sd.cpp runtime is downloaded on first install.
The app updates itself when a new version is out.

## Usage

Not sure which model to pick? See the [model guide](docs/MODELS.md).

1. Download a model in **Preferences ▸ Models** (recommended: *LaMa Manga*).
2. Drop pages or a folder onto the window, or import a chapter folder.
3. Click **Clean page** or **Whole chapter**.
4. Save the cleaned pages with **Export**.

The interface is available in English and Turkish (Preferences ▸ Language).
Free to use, with a daily limit.

## Build from source

Requires Rust (MSVC), Visual Studio 2022 Build Tools and Node.js 22.

```
npm ci
npm --prefix ui ci
npx tauri build
```

## License

SafInk is MIT licensed. Models are not bundled; they are downloaded from their original sources and keep their own licenses:
[LaMa Manga](https://huggingface.co/mayocream/lama-manga) (MIT),
[LaMa](https://github.com/advimman/lama) (Apache-2.0),
[FLUX.2 Klein 4B](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF) (Apache-2.0),
[Qwen-Image 2.1 Turbo](https://huggingface.co/Qwen/Qwen-Image-2.1-Turbo/blob/main/LICENSE) (Qwen Research License, no commercial use) and Qwen3-VL-8B (Apache-2.0),
[manga109-segmentation-bubble](https://huggingface.co/huyvux3005/manga109-segmentation-bubble) (speech-balloon segmenter, YOLO11n-seg; CC-BY-NC-4.0 on its model card, no commercial use),
[stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp) (MIT),
[SDNQ](https://github.com/Disty0/sdnq).
