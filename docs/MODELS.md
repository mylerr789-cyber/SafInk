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

**The model processes only the text areas, not the whole page.** Remaining text is clustered; each cluster goes to the model with its surroundings (128 px of context) at native resolution, never downscaled, and the result is written back only onto the text pixels. Nothing outside the mask changes; tall webtoon strips use the same path. Measured (1115×1600 page, RTX 4060 Ti 16 GB): Qwen ~49 s → ~28 s, Klein ~17 s → ~14 s, LaMa ~1 s (unchanged).

## Recommendation by graphics card

- **NVIDIA, 12 GB or more:** LaMa Manga first; Qwen for text over artwork.
- **NVIDIA, 8–12 GB:** LaMa Manga first; FLUX.2 Klein if needed.
- **NVIDIA, 4–6 GB:** LaMa Manga is recommended. FLUX.2 Klein 4B (NVIDIA) runs, but very slowly: Low VRAM mode (on automatically for these cards) streams the model layer by layer from system RAM. On a 1115×1600 page GPU memory rises by only ~2.7 GB (~7.8 GB with it off) and the output is byte-identical. Time was 27 s instead of 21 s on the measured card; expect ~2 minutes per page on old 4 GB cards (MangaTranslator user report). Pages are never downscaled. Qwen and sd.cpp Klein do not fit these cards.
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
