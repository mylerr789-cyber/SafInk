# Which model should I choose?

**English** · [Türkçe](MODELLER.md)

Short answer: **start with LaMa Manga.** Use **Qwen** for the text that is left on patterned or drawn backgrounds.

Whatever model you pick, plain balloons (white, black, grey) never go to the model. SafInk fills them itself, instantly. The model only runs on patterned and drawn areas.

## Comparison

| Model | Good at | Weak spot | Speed* | Graphics card | Download |
|---|---|---|---|---|---|
| **LaMa Manga** (recommended) | Plain backgrounds, screentone, most pages. Sharp results, never invents shapes. | May leave a grey smudge under large text drawn over artwork. | ~2 s/page (NVIDIA), several times slower on CPU | Not needed | ~0.2 GB (CPU) · ~3 GB (NVIDIA) |
| **LaMa General** | Textured, busy backgrounds | Less manga-focused than LaMa Manga | Same as LaMa Manga | Not needed | ~0.2 GB · ~3 GB |
| **Qwen-Image 2.1 Turbo** | Sound effects (SFX) over artwork, logos, complex scenes. The most faithful at completing the drawing. | Can leave a trace of grey text on black; slightly softens very fine texture. Slow. | ~20–25 s/page (16 GB NVIDIA), ~40 s (smaller cards) | 12 GB+ VRAM recommended | ~12–15 GB |
| **FLUX.2 Klein 4B** | Patterned backgrounds; also runs on AMD and Intel cards | May fill large gaps by "drawing something" (hair, shadows, etc.) | ~13–17 s/page | 6 GB+ VRAM | ~5.5 GB |

\* Measured on an RTX 4060 Ti 16 GB. Times only apply to pages that go to the model; pages with plain balloons finish instantly because they skip the model.

## Recommendation by graphics card

- **NVIDIA, 12 GB or more:** LaMa Manga first; Qwen for text over artwork.
- **NVIDIA, 6–12 GB:** LaMa Manga first; FLUX.2 Klein if needed.
- **AMD / Intel:** LaMa Manga (runs on the CPU); FLUX.2 Klein if needed (Vulkan).
- **No graphics card:** LaMa Manga only. Redraw models are very slow on the CPU.

The required runtime (CUDA, Vulkan or CPU build) and the right model files for your card are chosen automatically; there is nothing to configure.

## Models ending in "(NVIDIA)"

These are SDNQ builds of the same models and only appear on NVIDIA cards. In our tests the Qwen one was about 2× slower than the regular build, so we recommend the regular one.

## Licenses

- **LaMa:** MIT / Apache-2.0
- **FLUX.2 Klein 4B:** Apache-2.0
- **Qwen-Image 2.1 Turbo:** Qwen Research License. **Commercial use is not allowed.**

Models are not bundled with the app; they are downloaded from their original sources.
