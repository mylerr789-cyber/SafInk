# SafInk IPC (Tauri komutları, olaylar, URI şeması)

Tüm komutlar `invoke('<ad>', {camelCase argümanlar})` ile çağrılır; hata durumunda `string` ile reddedilir.
Tip tanımları: `ui/src/lib/api.ts` (Rust yapıları: `safink-core::{db,settings,engine,catalog}`).

## Komutlar (mevcut, adlar sabit)

| Komut | Argümanlar | Döner |
|---|---|---|
| `get_settings` | – | `Settings` |
| `save_settings` | `settings: Settings` | – |
| `list_manga` | – | `Manga[] {id,name,chapters}` |
| `create_manga` | `name` | `id` (varsa mevcut id) |
| `delete_manga` | `id` | – |
| `list_chapters` | `mangaId` | `Chapter[] {id,manga_id,name,path,quick,pages,cleaned}` |
| `delete_chapter` | `id` | – |
| `rename_chapter` | `id, name` | – |
| `list_pages` | `chapterId` | `Page[]` |
| `get_page` | `id` | `Page \| null` |
| `delete_page` | `id` | – |
| `import_folder` | `mangaId, path` | `number[]` (oluşan bölüm id'leri; klasörde görsel varsa tek bölüm, yoksa her alt klasör bir bölüm) |
| `add_to_chapter` | `chapterId, paths: string[]` | `number` (eklenen sayfa) |
| `quick_new` | `paths: string[]` | `chapterId` (önceki Quick Clean bölümünü siler; manga adı `__quick__`, listelerde görünmez) |
| `start_job` | `spec: {page_ids, mode: 'fetch'\|'clean', force?, preset?: Preset}` | `jobId` (aynı anda tek iş) |
| `cancel_job` / `pause_job {paused}` / `job_running` | – | – / – / `bool` |
| `export_pages` | `pageIds, outDir?` | `{ok, skipped, dir}` (yalnızca `cleaned` sayfalar) |
| `reveal_path` | `path` | – (Explorer) |
| `save_edit` | **ham gövde** (`Uint8Array`), başlık `x-page-id` | – ; gövde = `[u32 tx LE][u32 ty LE][1024*1024 bayt]` kayıtları; bayt: 0 yok, 1 ekle, 2 sil. Durumu `mask_edited` yapar |
| `reset_edits` | `pageId` | – |
| `list_models` | – | `Model[]` (katalog + `installed, fit, visible, total_bytes, downloaded_bytes, download_bytes`); çalışma ortamları (sd.cpp, Python) `visible=false` |
| `hardware` | – | `{gpu_name, vram_gb, ram_gb, gpu_kind: nvidia\|other\|none}` |
| `install_model` | `id` | – Tek tuş: gerekli çalışma ortamı (NVIDIA: sd.cpp CUDA12, diğer GPU: Vulkan, GPU yok: CPU; SDNQ ve NVIDIA'da LaMa için Python ortamı) otomatik önce kurulur, sonra model iner; tek ilerleme akışı `download{model_id=id}` |
| `cancel_install` / `remove_model` | `id` | – |

### Yeni (UI için henüz bağlanmadı) — yok
Bu sürümde yeni komut eklenmedi.

## Olaylar

Tek kanal: `listen('safink', ev => ev.payload)`; `payload.type` ayırt eder.

- `page {page_id, status, error, rev}`
- `job {job_id, mode, total, fetched, cleaned, failed, current, state: running|paused|done|cancelled|error, message, wait_secs}`
- `download {model_id, file, file_done, file_total, done, total, stage: downloading|verifying|extracting|done|error, phase}`  (`done/total` zincirin tümü için tek çubuk; `phase` Türkçe aşama adı; `error` durumunda `file` hata metnidir)
- `env_log {line}` (Python ortamı kurulum günlüğü; arayüz göstermez)
- `import {done, total}`

## Özel URI şeması (görüntüler; base64 yok)

Windows: `http://safink.localhost/…`, diğerleri: `safink://localhost/…` (CORS `*`, `no-store`; `?v=` yalnızca önbellek kırma).

- `/tile/{pageId}/{layer}/{tx}/{ty}` — 1024x1024 karo. `layer`: `orig` (JPEG q92), `clean` (JPEG), `mask` (API maskesi, gri PNG, sayfa boyutuna yeniden örneklenmiş), `edit` (kullanıcı katmanı, gri PNG: 0/1/2). Yoksa 404.
- `/thumb/{pageId}` — 320x440 içine sığan JPEG küçük resim (disk önbellekli).

## Davranış notları

- Maske önbelleği: `masks/<sha256>_c<güven*100>.png` (+ `.det` = tespit sayısı). Sayfa durumu `new` iken önbellek isabeti API çağrısı yapmaz.
- Son maske = (API ∪ eklenen) − silinen; `dilation` çalışma anında uygulanır (sayfada saklanmaz).
- LaMa arka ucu otomatik: NVIDIA GPU → PyTorch (yönetilen Python ortamı, kalıcı worker); aksi halde ONNX CPU. DirectML kullanılmaz. Kullanıcı seçmez.
- Redraw modellerinde adım/guidance/komut katalog `params` içindedir (ayar değil). MangaTranslator gibi: kırpıntı referans görüntü + sabit komut; maske yalnızca geri yazmada kullanılır (yeşil boyama/"Remove the green areas" MangaTranslator kaynağında yoktur).
- Qwen-Image 2.1 Turbo (`sdcpp-qwen-image-2-1-turbo`, `sdnq-qwen-image-2-1-turbo`): MangaTranslator ile aynı dosyalar/ayarlar. sd.cpp: `--sigmas qwen-image-2.1-turbo` (sabit 8 adım), cfg 1, euler, `--llm_vision` (mmproj) ve `--offload-to-cpu`; SDNQ: `QwenImage21Pipeline`, adım/guidance geçilmez, `output_resolution=sqrt(w*h)`, VAE döşemeli. Çözünürlük 32'nin katı (`params.multiple`), dolgu ile; ölçekleme yok. Lisans: Qwen Research (ticari olmayan).
- `Settings` artık `api_base`, `provider`, `sdcpp_variant`, `redraw_prompt`, `redraw_steps` içermez; `Preset` `prompt/steps` içermez.
