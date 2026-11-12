# Which model should I choose?

**English** · [Türkçe](MODELLER.md)

Short answer: **start with LaMa Manga.** Use **FLUX.2 Klein** for the text that is left on patterned or drawn backgrounds.

Whatever model you pick, plain balloons (white, black, grey) never go to the model. SafInk fills them itself, instantly. The model only runs on patterned and drawn areas.

## Comparison

| Model | Good at | Weak spot | Speed* | Graphics card | Download |
|---|---|---|---|---|---|
| **LaMa Manga** (recommended) | Plain backgrounds, screentone, most pages. Sharp results, never invents shapes. | May leave a grey smudge under large text drawn over artwork. | ~2 s/page (NVIDIA), several times slower on CPU | Not needed | ~0.2 GB (CPU) · ~3 GB (NVIDIA) |
| **LaMa General** | Textured, busy backgrounds | Less manga-focused than LaMa Manga | Same as LaMa Manga | Not needed | ~0.2 GB · ~3 GB |
| **FLUX.2 Klein 4B** | Patterned backgrounds; also runs on AMD and Intel cards | May fill large gaps by "drawing something" (hair, shadows, etc.) | ~13–17 s/page | 6 GB+ VRAM | ~5.5 GB |

\* Measured on an RTX 4060 Ti 16 GB. Times only apply to pages that go to the model; pages with plain balloons finish instantly because they skip the model.

**The model processes only the text areas, not the whole page.** FLUX.2 Klein works the way MangaTranslator does: every remaining text region goes to the model as a raw crop with surrounding context (text still in place, nothing painted over it), small crops are upscaled to about 1 MP and scaled back, and the result is written back only onto the text pixels of that region. Nothing outside the mask changes; tall webtoon strips use the same path.

## Recommendation by graphics card

- **NVIDIA, 12 GB or more:** LaMa Manga first; FLUX.2 Klein for text over artwork.
- **NVIDIA, 8–12 GB:** LaMa Manga first; FLUX.2 Klein if needed.
- **NVIDIA, 4–6 GB:** LaMa Manga is recommended. FLUX.2 Klein 4B (NVIDIA) runs, but very slowly: Low VRAM mode (on automatically for these cards) streams the model layer by layer from system RAM. On a 1115×1600 page GPU memory rises by only ~2.7 GB (~7.8 GB with it off) and the output is byte-identical. Time was 27 s instead of 21 s on the measured card; expect ~2 minutes per page on old 4 GB cards (MangaTranslator user report). sd.cpp Klein does not fit these cards.
- **AMD / Intel:** LaMa Manga (runs on the CPU); FLUX.2 Klein if needed (Vulkan).
- **No graphics card:** LaMa Manga only. Redraw models are very slow on the CPU.

The required runtime (CUDA, Vulkan or CPU build) and the right model files for your card are chosen automatically; there is nothing to configure.

## Models ending in "(NVIDIA)"

These are SDNQ builds of the same models and only appear on NVIDIA cards. If the regular build runs on your card, we recommend it.

## Licenses

- **LaMa:** MIT / Apache-2.0
- **FLUX.2 Klein 4B:** Apache-2.0

Models are not bundled with the app; they are downloaded from their original sources.
