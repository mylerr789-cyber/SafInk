"""SafInk LaMa (PyTorch/CUDA) worker: stdin'den JSON satirlari okur, stdout'a JSON satirlari yazar.

Komutlar:
  {"cmd":"load","weights":"...","kind":"spandrel_prefixed"|"auto"}
  {"cmd":"run","image":"in.png","mask":"mask.png","out":"out.png"}   (mask: 255 = silinecek)
  {"cmd":"quit"}
TEST EDILMEDI. Yukleme: .safetensors -> spandrel (MangaTranslator gibi 'generator.' onekiyle);
.pt/.ckpt -> once TorchScript, olmazsa state_dict + spandrel.
"""
import json
import sys
import traceback


def reply(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


model = None
dev = "cpu"


def _spandrel(sd):
    from spandrel import ModelLoader

    if not any(k.startswith("generator.") for k in sd):
        sd = {"generator." + k: v for k, v in sd.items()}
    return ModelLoader().load_from_state_dict(sd)


def load(msg):
    global model, dev
    import torch

    dev = "cuda" if torch.cuda.is_available() else "cpu"
    path = msg["weights"]
    if path.endswith(".safetensors"):
        from safetensors.torch import load_file

        model = _spandrel(load_file(path)).model.to(dev).eval()
        return
    try:
        model = torch.jit.load(path, map_location=dev).eval()
    except Exception:  # noqa: BLE001
        obj = torch.load(path, map_location="cpu", weights_only=True)
        sd = obj.get("state_dict", obj) if isinstance(obj, dict) else obj
        model = _spandrel(sd).model.to(dev).eval()


def run(msg):
    import numpy as np
    import torch
    from PIL import Image

    img = np.asarray(Image.open(msg["image"]).convert("RGB"), dtype=np.float32) / 255.0
    m = (np.asarray(Image.open(msg["mask"]).convert("L"), dtype=np.float32) >= 128).astype(np.float32)
    x = torch.from_numpy(img).permute(2, 0, 1)[None].to(dev)
    mk = torch.from_numpy(m)[None, None].to(dev)
    with torch.inference_mode():
        y = model(x, mk)
    y = y[0].float().permute(1, 2, 0).cpu().numpy()
    if y.max() > 2.0:  # bazi ihracatlar 0..255 dondurur
        y = y / 255.0
    Image.fromarray((np.clip(y, 0, 1) * 255.0 + 0.5).astype("uint8")).save(msg["out"])


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
        if msg["cmd"] == "load":
            load(msg)
            reply({"ok": True})
        elif msg["cmd"] == "run":
            run(msg)
            reply({"ok": True})
        elif msg["cmd"] == "quit":
            break
    except Exception as e:  # noqa: BLE001
        traceback.print_exc(file=sys.stderr)
        reply({"ok": False, "error": f"{type(e).__name__}: {e}"})
