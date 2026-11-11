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


# MangaTranslator (meangrinch) ile aynı komut: kırpıntı referans görüntü + sabit metin; maske yalnızca geri yazmada kullanılır.
EDIT_PROMPT = ("Erase only the Japanese lettering and sound effect characters. "
               "Do not remove or alter any other lines or marks in the artwork.")

# Redraw girdi kipi (inpaint/mod.rs InputPrep): varsayılan green; karo karo (çekirdek <= 512 + 128 bağlam), ölçeksiz.
PAGE = {"input_mode": "green", "prompt": EDIT_PROMPT,
        "prompt_green": "Remove the green areas and restore the artwork underneath, matching the surrounding lines and screentone. Do not add any text.",
        "prompt_lama_prefill": "Clean manga artwork. Keep everything as is; fix smudges and continue lines and screentone naturally. No text."}

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
# Sabit sürüm: test edilen derleme (Qwen-Image 2.1 için --sigmas önayarı bu sürümde var). Güncellemek için etiketi değiştirip yeniden test edin.
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
    params={"steps": 4, "cfg": 1.0, "guidance": 1.0, "sampler": "euler", "offload": True, "quant": "GGUF Q4_K_M", **PAGE},
    files=[hf_file("unsloth/FLUX.2-klein-4B-GGUF", "flux-2-klein-4b-Q4_K_M.gguf", "diffusion"),
           hf_file("unsloth/Qwen3-4B-GGUF", "Qwen3-4B-UD-Q4_K_XL.gguf", "llm"),
           hf_file("Comfy-Org/flux2-dev", "split_files/vae/flux2-vae.safetensors", "vae")])
repo = "Disty0/FLUX.2-klein-4B-SDNQ-4bit-dynamic"
add(id="sdnq-flux2-klein-4b", name="FLUX.2 Klein 4B (NVIDIA)", backend="sdnq", kind="redraw", mode="edit",
    description="Aynı model, NVIDIA kartlar için; ince ayrıntıda daha iyi olabilir.", description_en="Same model, for NVIDIA cards; may be better on fine detail.",
    license="Apache-2.0", license_url="https://huggingface.co/" + repo, min_vram_gb=6, min_ram_gb=16, hf_repo=repo,
    params={"pipeline": "Flux2KleinPipeline", "steps": 4, "guidance": 1.0, "quant": "4-bit dinamik", **PAGE},
    files=hf_repo_all(repo))

# ---- Qwen-Image 2.1 Turbo (MangaTranslator core/ml/model_manager.py + utils/model_metadata.py ile aynı dosyalar):
# 8 adım sabit sigma çizelgesi ("--sigmas qwen-image-2.1-turbo"), cfg 1, euler; metin kodlayıcı Qwen3-VL-8B (+ mmproj), 32'nin katı çözünürlük. ----
QWEN = {"steps": 8, "cfg": 1.0, "guidance": 1.0, "sampler": "euler", "offload": True, "sigmas": "qwen-image-2.1-turbo", "multiple": 32, **PAGE}
add(id="sdcpp-qwen-image-2-1-turbo", name="Qwen-Image 2.1 Turbo", backend="sdcpp", kind="redraw", mode="edit",
    description="Qwen-Image 2.1 Turbo ile yazı yerini çevresine uygun yeniden çizer; Klein'dan ağır ve yavaş.", description_en="Redraws the text area to match its surroundings with Qwen-Image 2.1 Turbo; heavier and slower than Klein.",
    license="Qwen Research (ticari olmayan)", license_url="https://huggingface.co/Qwen/Qwen-Image-2.1-Turbo/blob/main/LICENSE",
    min_vram_gb=12, min_ram_gb=24, notes="Qwen3-VL-8B metin kodlayıcı; 8 adım sabit. 16 GB+ NVIDIA'da INT8 takımı (RTX 4060 Ti 16 GB: sayfa başına ~25 sn), altında Q4_K_M takımı (~45 sn); en çok ~15 GB VRAM.",
    params={**QWEN, "quant": "GGUF Q4_K_M"},
    # 16 GB+ NVIDIA: "fast" takımı (INT8 convrot yayılım + dbcache; IQ3 kodlayıcı sonuçta bozulma yaptığı için kullanılmadı); diğer makineler ve eski Q4 kurulumları: "compact" takımı.
    # Seçim Catalog::load -> ModelSpec::resolve_tier içinde; kullanıcı seçim yapmaz.
    fast_params={"quant": "INT8 convrot", "cache_mode": "dbcache"},
    files=[hf_file("unsloth/Qwen-Image-2.1-Turbo-GGUF", "qwen-image-2.1-turbo-Q4_K_M.gguf", "diffusion", only="compact"),
           hf_file("Comfy-Org/Qwen-Image-2.1", "diffusion_models/qwen_image_2.1_turbo_int8_convrot.safetensors", "diffusion", only="fast"),
           hf_file("unsloth/Qwen3-VL-8B-Instruct-GGUF", "Qwen3-VL-8B-Instruct-UD-Q4_K_XL.gguf", "llm"),
           hf_file("unsloth/Qwen3-VL-8B-Instruct-GGUF", "mmproj-F16.gguf", "llm_vision", name="Qwen3-VL-8B-Instruct-mmproj-F16.gguf"),
           hf_file("unsloth/Qwen-Image-2.1-FP8", "vae/qwen_image_2.1_vae_bf16.safetensors", "vae")])
repo = "SamuelTallet/Qwen-Image-2.1-Turbo-SDNQ-4bit-dynamic-hadamard256"  # MangaTranslator'ın SDNQ deposu (texture-fix VAE dahil)
add(id="sdnq-qwen-image-2-1-turbo", name="Qwen-Image 2.1 Turbo (NVIDIA)", backend="sdnq", kind="redraw", mode="edit",
    description="Aynı model, NVIDIA kartlar için 4-bit SDNQ sürümü; daha az VRAM kullanır.", description_en="Same model, 4-bit SDNQ build for NVIDIA cards; uses less VRAM.",
    license="Qwen Research (ticari olmayan)", license_url="https://huggingface.co/Qwen/Qwen-Image-2.1-Turbo/blob/main/LICENSE",
    min_vram_gb=10, min_ram_gb=24, hf_repo=repo, notes="Qwen3-VL-8B metin kodlayıcı dahil; 8 adım sabit. RTX 4060 Ti 16 GB: sayfa başına ~70 sn, en çok ~10,5 GB VRAM.",
    params={"pipeline": "QwenImage21Pipeline", "steps": 8, "multiple": 32, "quant": "4-bit dinamik · Hadamard", **PAGE},
    files=hf_repo_all(repo))

json.dump({"version": 2, "sdcpp_tag": tag, "models": models}, sys.stdout, indent=1, ensure_ascii=False)
