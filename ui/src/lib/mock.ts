// Yalnızca geliştirme: tarayıcıda arayüzü Tauri olmadan çalıştırmak için sahte arka uç (ui/mock içindeki örnek sayfalar).
// Dosyalar vite.config.ts içindeki geliştirme ara katmanından gelir (/__mock/...).
const url = (n: string) => `/__mock/${n}`;
const [catalog, meta] = await Promise.all([fetch(url('catalog.json')).then((r) => r.json()), fetch(url('pages.json')).then((r) => r.json())]);
const TILE = 1024;

type Cb = (ev: { payload: any }) => void;
const subs: Cb[] = [];
const emit = (payload: any) => subs.forEach((f) => f({ payload }));

const settings: any = {
  confidence: 0.25, dilation: 2, feather: 4, model_id: 'lama-manga',
  export_dir: '', export_format: 'original', export_quality: 92, api_rate_per_min: 24, hf_token: '', low_vram: false, flat_fill: true,
  presets: [
    { id: 'fast', name: 'Hızlı – LaMa Manga', model_id: 'lama-manga', confidence: 0.25, dilation: 2, feather: 4 },
    { id: 'bold', name: 'Kalın yazı – geniş maske', model_id: 'lama-manga', confidence: 0.25, dilation: 6, feather: 6 },
    { id: 'general', name: 'Genel – LaMa Genel', model_id: 'lama-big', confidence: 0.25, dilation: 3, feather: 4 },
    { id: 'redraw', name: 'Yeniden çiz – Flux.2 Klein', model_id: 'sdcpp-flux2-klein-4b', confidence: 0.25, dilation: 4, feather: 4 },
  ],
};
const mangas = [
  { id: 1, name: 'Hyouka', chapters: 2 },
  { id: 2, name: 'Tonari no Gyaru wa Kuchisabishii-chan', chapters: 1 },
  { id: 3, name: 'Dame Ningen no Aishikata', chapters: 1 },
  { id: 4, name: 'Regressor of the Fallen Family', chapters: 3 },
];
const chapters = [
  { id: 11, manga_id: 1, name: 'Bölüm 91', path: 'D:/manga/Hyouka/91', quick: false, pages: 7, cleaned: 2 },
  { id: 12, manga_id: 1, name: 'Bölüm 90', path: 'D:/manga/Hyouka/90', quick: false, pages: 22, cleaned: 22 },
  { id: 21, manga_id: 2, name: 'Vol.01 Ch.3', path: '', quick: false, pages: 18, cleaned: 0 },
  { id: 31, manga_id: 3, name: 'Ch. 11.5 Extras', path: '', quick: false, pages: 12, cleaned: 5 },
  { id: 41, manga_id: 4, name: 'Chapter 137', path: '', quick: false, pages: 64, cleaned: 64 },
  { id: 42, manga_id: 4, name: 'Chapter 136', path: '', quick: false, pages: 58, cleaned: 31 },
  { id: 43, manga_id: 4, name: 'Chapter 135', path: '', quick: false, pages: 61, cleaned: 0 },
];
const covers: Record<number, number> = { 11: 0, 12: 0, 21: 1, 31: 2, 41: 6, 42: 6, 43: 6 };
const statuses = ['cleaned', 'cleaned', 'mask_edited', 'mask_fetched', 'new', 'new', 'error'];
const pages = (meta as any[]).map((m, i) => ({
  id: i + 1, chapter_id: 11, idx: i, path: `D:/manga/Hyouka/91/${m.name}`, name: m.name, width: m.w, height: m.h, sha256: '',
  status: statuses[i], error: statuses[i] === 'error' ? 'API 503: sunucu geçici olarak kullanılamıyor' : '', mask_conf: statuses[i] === 'new' ? 0 : 0.25,
  detections: statuses[i] === 'new' ? 0 : m.det, rev: 1,
}));
let loc = 'tr'; // set_locale ile gelir; mock iletileri de seçili dilde üretir
const L = (tr: string, en: string) => (loc === 'en' ? en : tr);
const edits = new Map<string, Uint8Array>();
const removed = new Set<number>(); // silinen sayfa id'leri (mock)
let order: number[] = pages.map((p) => p.id); // bölüm 11 sayfa sırası (reorder_pages)
const live = () => order.map((id) => pages[id - 1]).filter((p) => !removed.has(p.id));

const imgCache = new Map<string, Promise<ImageBitmap | null>>();
function bitmap(n: string) {
  if (!imgCache.has(n)) imgCache.set(n, fetch(url(n)).then((r) => r.blob()).then((b) => createImageBitmap(b)).catch(() => null));
  return imgCache.get(n)!;
}
async function cleanBitmap(i: number): Promise<ImageBitmap | null> {
  const key = `clean${i}`;
  if (!imgCache.has(key)) imgCache.set(key, (async () => {
    const [o, m] = [await bitmap(`p${i}.jpg`), await bitmap(`p${i}_mask.png`)];
    if (!o || !m) return null;
    const c = new OffscreenCanvas(o.width, o.height), x = c.getContext('2d')!;
    const mc = new OffscreenCanvas(o.width, o.height), mx = mc.getContext('2d')!;
    mx.filter = 'blur(3px)'; mx.drawImage(m, 0, 0); mx.filter = 'none';
    const d = mx.getImageData(0, 0, o.width, o.height);
    for (let k = 0; k < d.data.length; k += 4) { const v = d.data[k] > 8 ? 255 : 0; d.data[k] = d.data[k + 1] = d.data[k + 2] = 255; d.data[k + 3] = v; }
    mx.putImageData(d, 0, 0);
    x.drawImage(o, 0, 0); x.drawImage(mc, 0, 0);
    return createImageBitmap(c);
  })());
  return imgCache.get(key)!;
}

export function thumb(page: number) { return url(`p${(page - 1) % pages.length}.jpg`); }
export function listen(cb: Cb) { subs.push(cb); return () => subs.splice(subs.indexOf(cb), 1); }
export async function pick(_o: any) { return ['C:/demo/01.png', 'C:/demo/02.png']; }

export async function tile(page: number, layer: string, tx: number, ty: number): Promise<ImageBitmap | null> {
  const p = pages[page - 1];
  if (!p) return null;
  const i = page - 1, w = Math.min(TILE, p.width - tx * TILE), h = Math.min(TILE, p.height - ty * TILE);
  let src: ImageBitmap | null = null;
  if (layer === 'orig') src = await bitmap(`p${i}.jpg`);
  else if (layer === 'mask') src = p.status === 'new' ? null : await bitmap(`p${i}_mask.png`);
  else if (layer === 'clean') src = p.status === 'cleaned' ? await cleanBitmap(i) : null;
  else if (layer === 'edit') {
    const e = edits.get(`${page}:${tx},${ty}`);
    if (!e) return null;
    const d = new ImageData(w, h);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) { const o = (y * w + x) * 4; d.data[o] = d.data[o + 1] = d.data[o + 2] = e[y * TILE + x]; d.data[o + 3] = 255; }
    return createImageBitmap(d);
  }
  if (!src) return null;
  return createImageBitmap(src, tx * TILE, ty * TILE, w, h);
}

// Sahte donanım: ?hw=none|other|nvidia ile denenebilir (varsayılan nvidia). Gerçek uygulamada Rust algılar.
const HW = new URLSearchParams(location.search).get('hw') ?? 'nvidia';
const hardware = { gpu_name: HW === 'none' ? '' : HW === 'other' ? 'AMD Radeon RX 6600' : 'NVIDIA GeForce RTX 4070', vram_gb: HW === 'nvidia' ? 12 : 0, ram_gb: 32, gpu_kind: HW };
const installedSet = new Set<string>(['lama-manga']);
const partial = new Map<string, number>();
const RUNTIME_BYTES: Record<string, number> = { nvidia: 930377011, other: 29947836, none: 18172997 };
const ENV_BYTES = 3_600_000_000;
const active = (m: any) => (m.files ?? []).filter((f: any) => !f.only || (f.only === 'cuda') === (HW === 'nvidia'));
const models = () => (catalog as any).models.map((m: any) => {
  const total = active(m).reduce((n: number, f: any) => n + (f.size || 0), 0);
  const installed = installedSet.has(m.id);
  const visible = m.kind !== 'runtime' && !(m.backend === 'sdnq' && HW !== 'nvidia');
  const rt = m.backend === 'sdcpp' ? RUNTIME_BYTES[HW] : m.backend === 'sdnq' || (m.backend === 'lama' && HW === 'nvidia') ? ENV_BYTES : 0;
  const fit = m.backend === 'lama' ? 'fits' : HW === 'none' ? 'slow' : HW === 'other' ? 'unknown' : m.min_vram_gb > 12 ? 'no' : m.min_vram_gb > 8 ? 'tight' : 'fits';
  return { mode: '', license: '', license_url: '', notes: '', min_vram_gb: 0, min_ram_gb: 0, description: '', recommended: false, variant: '', ...m,
    installed, fit, visible, total_bytes: total, downloaded_bytes: installed ? total : (partial.get(m.id) ?? 0), download_bytes: installed ? 0 : total + rt };
});
const timers = new Map<string, number>();
function simInstall(id: string) {
  const m = models().find((x: any) => x.id === id);
  if (!m) throw 'model yok';
  if (timers.has(id)) throw 'bu model zaten indiriliyor';
  const grand = m.download_bytes; let done = 0;
  const step = grand / 40;
  const phases = (m.backend === 'sdcpp' || m.backend === 'sdnq' || (m.backend === 'lama' && HW === 'nvidia')) ? [L('Çalışma ortamı kuruluyor', 'Setting up runtime'), L('Model indiriliyor', 'Downloading model')] : [L('Model indiriliyor', 'Downloading model')];
  const t = window.setInterval(() => {
    done = Math.min(grand, done + step);
    const phase = phases.length === 2 && done < grand * 0.3 ? phases[0] : phases[phases.length - 1];
    if (done >= grand) {
      clearInterval(t); timers.delete(id); installedSet.add(id);
      emit({ type: 'download', model_id: id, file: '', file_done: 0, file_total: 0, done: grand, total: grand, stage: 'done', phase });
    } else emit({ type: 'download', model_id: id, file: '', file_done: 0, file_total: 0, done, total: grand, stage: 'downloading', phase });
  }, 250);
  timers.set(id, t);
}

let job = 0;
async function runJob(spec: any) {
  const id = ++job, total = spec.page_ids.length;
  let done = 0;
  for (const pid of spec.page_ids) {
    emit({ type: 'job', job_id: id, mode: spec.mode, total, fetched: done, cleaned: spec.mode === 'clean' ? done : 0, failed: 0, current: pid, state: 'running', message: L(`Sayfa ${pid} işleniyor`, `Processing page ${pid}`), wait_secs: 0 });
    await new Promise((r) => setTimeout(r, 900));
    const p = pages[pid - 1];
    p.status = spec.mode === 'clean' ? 'cleaned' : 'mask_fetched'; p.rev++; p.error = '';
    emit({ type: 'page', page_id: pid, status: p.status, error: '', rev: p.rev });
    done++;
  }
  emit({ type: 'job', job_id: id, mode: spec.mode, total, fetched: done, cleaned: spec.mode === 'clean' ? done : 0, failed: 0, current: null, state: 'done', message: '', wait_secs: 0 });
}

export async function invoke(cmd: string, a: any = {}, opts?: any): Promise<any> {
  switch (cmd) {
    case 'get_settings': return structuredClone(settings);
    case 'save_settings': Object.assign(settings, a.settings); return;
    case 'set_locale': loc = a.locale; return;
    case 'reorder_pages': { const ids: number[] = a.pageIds; const rest = order.filter((id) => !ids.includes(id)); order = [...ids.filter((id) => order.includes(id)), ...rest]; return; }
    case 'list_manga': return structuredClone(mangas);
    case 'create_manga': { const f = mangas.find((m) => m.name === a.name); if (f) return f.id; const id = Math.max(...mangas.map((m) => m.id)) + 1; mangas.push({ id, name: a.name, chapters: 0 }); return id; }
    case 'rename_chapter': { const c = chapters.find((x) => x.id === a.id); if (c) c.name = a.name; return; }
    case 'delete_chapter': { const i = chapters.findIndex((x) => x.id === a.id); if (i >= 0) { const m = mangas.find((x) => x.id === chapters[i].manga_id); if (m) m.chapters--; chapters.splice(i, 1); } return; }
    case 'delete_manga': { const i = mangas.findIndex((x) => x.id === a.id); if (i >= 0) mangas.splice(i, 1); for (let k = chapters.length - 1; k >= 0; k--) if (chapters[k].manga_id === a.id) chapters.splice(k, 1); return; }
    case 'delete_page': { await new Promise((r) => setTimeout(r, 150)); removed.add(a.id); const c = chapters.find((x) => x.id === 11); if (c) c.pages = live().length; return; }
    case 'list_chapters': return chapters.filter((c) => c.manga_id === a.mangaId);
    case 'list_pages': return a.chapterId === 11 ? structuredClone(live()).map((p, i) => ({ ...p, idx: i })) : covers[a.chapterId] != null ? [structuredClone(pages[covers[a.chapterId]])] : [];
    case 'get_page': return structuredClone(pages[a.id - 1] ?? null);
    case 'quick_new': return 11;
    case 'import_folder': return [11];
    case 'start_job': runJob(a.spec); return job + 1;
    case 'save_edit': {
      const pid = Number(opts?.headers?.['x-page-id']); const body = a as Uint8Array;
      for (let o = 0; o + 8 <= body.length; o += 8 + TILE * TILE) {
        const dv = new DataView(body.buffer, body.byteOffset + o, 8);
        edits.set(`${pid}:${dv.getUint32(0, true)},${dv.getUint32(4, true)}`, body.slice(o + 8, o + 8 + TILE * TILE));
      }
      return;
    }
    case 'reset_edits': for (const k of [...edits.keys()]) if (k.startsWith(a.pageId + ':')) edits.delete(k); return;
    case 'list_models': return models();
    case 'hardware': return hardware;
    case 'install_model': simInstall(a.id); return;
    case 'cancel_install': { const t = timers.get(a.id); if (t) { clearInterval(t); timers.delete(a.id); emit({ type: 'download', model_id: a.id, file: L('iptal edildi', 'cancelled'), file_done: 0, file_total: 0, done: 0, total: 0, stage: 'error', phase: '' }); } return; }
    case 'remove_model': installedSet.delete(a.id); return;
    case 'export_pages': { await new Promise((r) => setTimeout(r, 1200)); const ok = a.pageIds.filter((id: number) => pages[id - 1]?.status === 'cleaned').length; return { ok, skipped: a.pageIds.length - ok, dir: 'C:/out' }; }
    default: return;
  }
}
