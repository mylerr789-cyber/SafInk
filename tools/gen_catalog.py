"""catalog.json üretici: HF API'den boyut/sha256 çeker, GitHub release'ten sd.cpp digest'leri alır.
Kullanım: python tools/gen_catalog.py > crates/safink-core/catalog.json"""
import json, urllib.request, sys


def get(u):
    return json.load(urllib.request.urlopen(urllib.request.Request(u, headers={"User-Agent": "safink"})))


def hf_file(repo, path, role, name=None, rev="main", only=""):
    d = get(f"https://huggingface.co/api/models/{repo}/tree/{rev}/{'/'.join(path.split('/')[:-1])}".rstrip("/"))
    for f in d:
        if f["path"] == path:
            return {"name": name or path.split("/")[-1], "role": role, "url": f"https://huggingface.co/{repo}/resolve/{rev}/{path}",
                    "sha256": (f.get("lfs") or {}).get("oid"), "size": f["size"], "only": only}
    raise SystemExit(f"yok: {repo}/{path}")


def hf_repo_all(repo):
    out = []

    def walk(p=""):
        for f in get(f"https://huggingface.co/api/models/{repo}/tree/main/{p}".rstrip("/")):
            if f["type"] == "directory":
                walk(f["path"])
            else:
                if f["path"] == ".gitattributes" or f["path"].endswith((".md", ".png", ".jpg")):
                    continue
                out.append({"name": f["path"], "role": "snapshot", "url": f"https://huggingface.co/{repo}/resolve/main/{f['path']}",
                            "sha256": (f.get("lfs") or {}).get("oid"), "size": f["size"], "only": ""})
    walk()
    return out


models = []


def add(**k):
    models.append(k)


# MangaTranslator (meangrinch, core/image/inpainting.py) komutu; "Japanese" sözcüğü çıkarıldı (İngilizce taramalarda orijinal
# sözcükle İngilizce yazı kalıyordu). Kırpıntı referans görüntü + sabit metin; maske yalnızca geri yazmada kullanılır.
EDIT_PROMPT = ("Erase only the lettering and sound effect characters. "
               "Do not remove or alter any other lines or marks in the artwork.")

# Redraw modelleri hep MangaTranslator bölge yolundan geçer (mtcrop.rs): bölge başına ham kırpıntı (yeşil boya yok, maske modele verilmez),
# ~1 MP'ye büyütülür, çıktı geri küçülür; boyutlar `multiple`'ın katı (Klein: 16).
PAGE = {"multiple": 16, "prompt": EDIT_PROMPT}

# ---- LaMa: NVIDIA'da PyTorch (GPU), diğerlerinde ONNX (CPU). Makine yalnızca kendi dosyasını indirir ("only"). ----
add(id="lama-manga", name="LaMa Manga", backend="lama", kind="inpaint", adapter="lama01", recommended=True,
    description="Hızlı, düz ve sade arka planlar için. Çoğu sayfa için en iyi başlangıç.", description_en="Fast, for plain and simple backgrounds. The best starting point for most pages.",
    license="MIT", license_url="https://huggingface.co/mayocream/lama-manga", min_vram_gb=1, min_ram_gb=4,
    params={"torch": "spandrel_prefixed"},
    files=[hf_file("mayocream/lama-manga-onnx", "lama-manga.onnx", "model", only="cpu"),
           hf_file("mayocream/lama-manga", "lama-manga.safetensors", "torch", only="cuda")])
add(id="lama-big", name="LaMa Genel", backend="lama", kind="inpaint", adapter="lama255",
    description="Karışık ve dokulu arka planlar için genel amaçlı sürüm.", description_en="General-purpose version for busy, textured backgrounds.",
    license="Apache-2.0", license_url="https://github.com/advimman/lama", min_vram_gb=1, min_ram_gb=4,
    params={"torch": "auto"},
    files=[hf_file("Carve/LaMa-ONNX", "lama_fp32.onnx", "model", only="cpu"),
           hf_file("fashn-ai/LaMa", "big-lama.pt", "torch", only="cuda")])

# ---- sd.cpp çalışma ortamları (kullanıcıdan gizli; donanıma göre otomatik) ----
# Sabit sürüm: test edilen derleme Güncellemek için etiketi değiştirip yeniden test edin.
SDCPP_TAG = "master-1063-3d315c4"
rel = get("https://api.github.com/repos/leejet/stable-diffusion.cpp/releases/tags/" + SDCPP_TAG)
tag = rel["tag_name"]


def asset(sub, os_):
    # os_: "windows" | "linux" (FileSpec.os; bkz. catalog.rs). Linux'ta yalnızca CPU ve Vulkan derlemeleri yayımlanıyor (CUDA yok:
    # NVIDIA da Linux'ta Vulkan kullanır).
    for a in rel["assets"]:
        if sub in a["name"]:
            return {"name": a["name"], "role": "archive", "url": a["browser_download_url"],
                    "sha256": (a.get("digest") or "").replace("sha256:", "") or None, "size": a["size"], "only": "", "os": os_}
    raise SystemExit("asset yok " + sub)


add(id="tool-sdcpp-vulkan", name=f"sd.cpp {tag} Vulkan", backend="tool", kind="runtime", variant="vulkan",
    description="NVIDIA dışı ekran kartları (AMD/Intel).", description_en="Non-NVIDIA graphics cards (AMD/Intel).", license="MIT", license_url="https://github.com/leejet/stable-diffusion.cpp",
    files=[asset("bin-win-vulkan-x64", "windows"), asset("bin-Linux-Ubuntu-26.04-x86_64-vulkan", "linux")])
add(id="tool-sdcpp-cuda12", name=f"sd.cpp {tag} CUDA 12", backend="tool", kind="runtime", variant="cuda12",
    description="NVIDIA ekran kartları.", description_en="NVIDIA graphics cards.", license="MIT", license_url="https://github.com/leejet/stable-diffusion.cpp",
    files=[asset("bin-win-cuda12-x64", "windows"), asset("cudart-sd-bin-win-cu12", "windows")])
add(id="tool-sdcpp-cpu", name=f"sd.cpp {tag} CPU", backend="tool", kind="runtime", variant="cpu",
    description="Ekran kartı yok.", description_en="No graphics card.", license="MIT", license_url="https://github.com/leejet/stable-diffusion.cpp",
    files=[asset("bin-win-cpu-x64", "windows"), asset("bin-Linux-Ubuntu-26.04-x86_64.zip", "linux")])
add(id="tool-sdnq-env", name="Python ortamı", backend="tool", kind="runtime", variant="python",
    description="uv ile yönetilen Python 3.12 + torch (CUDA) + diffusers + sdnq + spandrel.", description_en="Python 3.12 managed by uv + torch (CUDA) + diffusers + sdnq + spandrel.", license="BSD/Apache", license_url="https://github.com/Disty0/sdnq",
    files=[])

# ---- Yeniden çizim (MangaTranslator'ın Klein 4B ayarları: 4 adım, guidance 1.0, euler) ----
add(id="sdcpp-flux2-klein-4b", name="FLUX.2 Klein 4B", backend="sdcpp", kind="redraw", mode="edit", recommended=True,
    description="Desenli ve karmaşık arka planı yeniden çizerek yazıyı siler. Her ekran kartında çalışır.", description_en="Erases the text by redrawing patterned and complex backgrounds. Works on any graphics card.",
    license="Apache-2.0", license_url="https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF", min_vram_gb=6, min_ram_gb=16,
    params={"steps": 4, "cfg": 1.0, "guidance": 1.0, "sampler": "euler", "offload": True, "vae_tile": 512, "quant": "GGUF Q4_K_M", **PAGE},
    files=[hf_file("unsloth/FLUX.2-klein-4B-GGUF", "flux-2-klein-4b-Q4_K_M.gguf", "diffusion"),
           hf_file("unsloth/Qwen3-4B-GGUF", "Qwen3-4B-UD-Q4_K_XL.gguf", "llm"),
           hf_file("Comfy-Org/flux2-dev", "split_files/vae/flux2-vae.safetensors", "vae")])
repo = "Disty0/FLUX.2-klein-4B-SDNQ-4bit-dynamic"
add(id="sdnq-flux2-klein-4b", name="FLUX.2 Klein 4B (NVIDIA)", backend="sdnq", kind="redraw", mode="edit",
    description="Aynı model, NVIDIA kartlar için; ince ayrıntıda daha iyi olabilir.", description_en="Same model, for NVIDIA cards; may be better on fine detail.",
    license="Apache-2.0", license_url="https://huggingface.co/" + repo, min_vram_gb=6, min_ram_gb=16, hf_repo=repo,
    params={"pipeline": "Flux2KleinPipeline", "steps": 4, "guidance": 1.0, "quant": "4-bit dinamik", **PAGE},
    files=hf_repo_all(repo))

json.dump({"version": 2, "sdcpp_tag": tag, "models": models}, sys.stdout, indent=1, ensure_ascii=False)
