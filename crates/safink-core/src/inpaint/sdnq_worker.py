"""SafInk SDNQ worker: stdin'den JSON satırları okur, stdout'a JSON satırları yazar.

Komutlar:
  {"cmd":"load","dir":..., "pipeline":"Flux2KleinPipeline"|"FluxKontextPipeline", "low_vram":bool}  (true: grup bosaltma + dosemeli VAE; false: model_cpu_offload)
  {"cmd":"edit","image":..., "out":..., "prompt":..., "steps":4, "guidance":1.0, "seed":1}
  {"cmd":"quit"}
Not: TEST EDILMEDI (multi-GB model indirilmedi). Maske bu worker'da kullanilmaz; geri yazimi Rust tarafi
yalnizca maskeli piksellere yapar (MangaTranslator ile ayni 'kirpinti duzenle + maskeyle yapistir' yaklasimi).
"""
import json
import sys
import traceback


def reply(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


pipe = None
device = "cpu"


def load(msg):
    global pipe, device
    import torch
    import diffusers
    from sdnq import SDNQConfig  # noqa: F401  (import SDNQ'yu diffusers/transformers'a kaydeder)

    device = "cuda" if torch.cuda.is_available() else "cpu"
    cls = getattr(diffusers, msg.get("pipeline", "Flux2KleinPipeline"))
    pipe = cls.from_pretrained(msg["dir"], torch_dtype=torch.bfloat16)
    if device == "cuda":
        if msg.get("low_vram", True):
            # MangaTranslator low-VRAM modu: yaprak duzeyi grup bosaltma (transformer + metin kodlayici RAM'den akitilir),
            # VAE GPU'da kalir ama dosemeli calisir
            from diffusers.hooks import apply_group_offloading

            for m in (pipe.transformer, pipe.text_encoder):
                apply_group_offloading(
                    m, onload_device=torch.device("cuda"), offload_device=torch.device("cpu"),
                    offload_type="leaf_level", use_stream=True,
                )
            pipe.vae.to("cuda")
            try:
                pipe.vae.enable_tiling(tile_sample_min_height=512, tile_sample_min_width=512)
            except Exception:  # noqa: BLE001  (dosemeyi desteklemeyen VAE)
                pass
        else:
            pipe.enable_model_cpu_offload()
    if not (device == "cuda" and msg.get("low_vram", True)) and msg.get("pipeline") == "QwenImage21Pipeline":
        # bir ~1MP'lik VAE cozumu bile 8 GB'i asar; dosemeli kodlama/cozme
        pipe.vae.enable_tiling(tile_sample_min_height=512, tile_sample_min_width=512)


def edit(msg):
    import torch
    from PIL import Image

    img = Image.open(msg["image"]).convert("RGB")
    w, h = img.size
    if w % 16 or h % 16:
        raise ValueError(f"image size {w}x{h} must be a multiple of 16 (Qwen: 32) (Rust side pads; no resizing)")
    w16, h16 = w, h
    gen = torch.Generator(device="cpu").manual_seed(int(msg.get("seed", 1)))
    if type(pipe).__name__ == "QwenImage21Pipeline":
        # MangaTranslator (inpainting.py QwenImage21Inpainter): adım/guidance yok (Turbo sabit 8 adım);
        # referans, çıktı alanında (sqrt(w*h)) yeniden boyutlanır -> girdi ile piksel hizalı kalır
        out = pipe(
            prompt=msg["prompt"], image=img, width=w16, height=h16,
            output_resolution=(w16 * h16) ** 0.5, generator=gen,
        ).images[0]
        out.save(msg["out"])
        return
    out = pipe(
        image=img,
        prompt=msg["prompt"],
        height=h16,
        width=w16,
        num_inference_steps=int(msg.get("steps", 4)),
        guidance_scale=float(msg.get("guidance", 1.0)),
        generator=gen,
    ).images[0]
    out.save(msg["out"])


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
        if msg["cmd"] == "load":
            load(msg)
            reply({"ok": True})
        elif msg["cmd"] == "edit":
            edit(msg)
            reply({"ok": True})
        elif msg["cmd"] == "quit":
            break
    except Exception as e:  # noqa: BLE001
        traceback.print_exc(file=sys.stderr)
        reply({"ok": False, "error": f"{type(e).__name__}: {e}"})
