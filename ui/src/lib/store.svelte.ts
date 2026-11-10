import { listen, pick } from './host';
import { api, type Settings, type JobEvt, type Model, type Hardware, type Page, type Preset } from './api';

export type Tool = 'hand' | 'zoom' | 'brush' | 'eraser' | 'lasso';
export type PrefsTab = 'clean' | 'export' | 'models';
interface Toast { id: number; text: string; kind: 'info' | 'ok' | 'err'; action?: { label: string; run: () => void } }
interface Ask { text: string; ok: string; danger: boolean; resolve: (v: boolean) => void }
interface DlState { stage: string; done: number; total: number; file: string; phase: string }
interface Layer { vis: boolean; op: number }

export interface Opts { preset_id: string; model_id: string; confidence: number; dilation: number; feather: number }

function load<T>(k: string, d: T): T {
  try { const v = localStorage.getItem(k); return v ? { ...d, ...JSON.parse(v) } : d; } catch { return d; }
}

class AppState {
  settings = $state<Settings | null>(null);
  models = $state<Model[]>([]);
  hw = $state<Hardware | null>(null);
  dl = $state<Record<string, DlState>>({});
  importProg = $state<{ done: number; total: number } | null>(null);
  toasts = $state<Toast[]>([]);
  asking = $state<Ask | null>(null);
  help = $state(false);
  exporting = $state(false);

  job = $state<JobEvt | null>(null);
  jobActive = $state(false);
  pageStatus = $state<Record<number, { status: string; error: string; rev: number }>>({});
  tick = $state(0);

  // proje
  project = $state<{ chapterId: number; name: string; manga: string; quick: boolean } | null>(null);
  pages = $state<Page[]>([]);
  index = $state(0);
  selected = $state<number[]>([]);

  // diyaloglar
  dialog = $state<null | 'start' | 'prefs'>('start');
  prefsTab = $state<PrefsTab>('clean');
  menu = $state<string | null>(null);
  dragging = $state(false);
  pagesOpen = $state(load('safink.panels', { pages: true }).pages);
  inspectorOpen = $state(load('safink.panels', { insp: true }).insp);

  // araçlar
  tool = $state<Tool>('brush');
  brushSize = $state(40);
  lassoMode = $state<'add' | 'remove'>('add');
  layers = $state<Record<'orig' | 'api' | 'edit' | 'result', Layer>>(load('safink.layers', {
    orig: { vis: true, op: 1 }, api: { vis: true, op: 0.55 }, edit: { vis: true, op: 0.65 }, result: { vis: true, op: 1 },
  }) as any);
  compare = $state(false); // true: sonucu göster (önce/sonra)
  split = $state(false); // true: bölünmüş önce/sonra (compare ile birlikte)
  splitAt = $state(0.5); // bölme çizgisi, sayfa genişliğinin oranı
  opts = $state<Opts>(load('safink.opts', { preset_id: 'fast', model_id: 'lama-manga', confidence: 0.25, dilation: 2, feather: 4 }));

  // tuval köprüsü
  cv = $state<{ fit: (m: 'width' | 'page') => void; zoomTo: (z: number) => void; undo: () => void; redo: () => void; flush: () => Promise<void>; reloadAll: () => void } | null>(null);
  canUndo = $state(false);
  canRedo = $state(false);
  zoomPct = $state(100);
  view = $state({ x: 0, y: 0, s: 1 }); // tuval dönüşümü (cetveller için)
  cursor = $state<{ x: number; y: number } | null>(null);
  saving = $state(false);
  hasClean = $state(false);
  hasApiMask = $state(false);

  private tid = 1;

  savePanels() { try { localStorage.setItem('safink.panels', JSON.stringify({ pages: this.pagesOpen, insp: this.inspectorOpen })); } catch {} }
  toggleMask() { const v = !(this.layers.api.vis || this.layers.edit.vis); this.layers.api.vis = v; this.layers.edit.vis = v; this.saveOpts(); }

  get page(): Page | null { return this.pages[this.index] ?? null; }

  toast(text: string, kind: Toast['kind'] = 'info', action?: Toast['action']) {
    const id = this.tid++;
    this.toasts.push({ id, text, kind, action });
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), kind === 'err' ? 7000 : action ? 8000 : 3000);
  }
  /** Uygulama içi onay kutusu (tarayıcı confirm'i yerine). */
  ask(text: string, ok = 'Sil', danger = true): Promise<boolean> {
    this.asking?.resolve(false);
    return new Promise((resolve) => (this.asking = { text, ok, danger, resolve }));
  }
  answer(v: boolean) { const a = this.asking; this.asking = null; a?.resolve(v); }
  saveOpts() { try { localStorage.setItem('safink.opts', JSON.stringify($state.snapshot(this.opts))); localStorage.setItem('safink.layers', JSON.stringify($state.snapshot(this.layers))); } catch {} }

  async reloadSettings() {
    this.settings = await api.getSettings();
  }
  async reloadModels() {
    this.models = (await api.listModels()).filter((m) => m.visible);
    // eski/artık görünmeyen bir model seçiliyse güvenli varsayılana dön
    if (this.models.length && !this.models.some((m) => m.id === this.opts.model_id)) {
      this.opts.model_id = this.models.find((m) => m.id === this.settings?.model_id)?.id ?? this.models[0].id;
      this.saveOpts();
    }
  }
  statusOf(p: Page) { return this.pageStatus[p.id]?.status ?? p.status; }
  revOf(p: Page) { return this.pageStatus[p.id]?.rev ?? p.rev; }

  applyPreset(id: string) {
    const p = this.settings?.presets.find((x) => x.id === id);
    if (!p) return;
    this.opts = { preset_id: id, model_id: p.model_id, confidence: p.confidence, dilation: p.dilation, feather: p.feather };
    this.saveOpts();
  }
  presetFromOpts(): Preset {
    const o = this.opts;
    return { id: 'ui', name: 'ui', model_id: o.model_id, confidence: o.confidence, dilation: o.dilation, feather: o.feather };
  }

  async openChapter(chapterId: number, name: string, manga: string, quick = false, index = 0) {
    await this.cv?.flush();
    this.project = { chapterId, name, manga, quick };
    this.pages = await api.listPages(chapterId);
    this.index = Math.min(index, Math.max(0, this.pages.length - 1));
    this.selected = [];
    this.dialog = null;
  }
  async reloadPages() {
    if (!this.project) return;
    const cur = this.page?.id;
    this.pages = await api.listPages(this.project.chapterId);
    const i = this.pages.findIndex((p) => p.id === cur);
    if (i >= 0) this.index = i;
  }
  async go(i: number) {
    if (i < 0 || i >= this.pages.length || i === this.index) return;
    await this.cv?.flush();
    this.index = i;
  }
  async quick(paths: string[]) {
    try {
      const cid = await api.quickNew(paths);
      await this.openChapter(cid, 'Quick Clean', 'Quick Clean', true);
    } catch (e) { this.toast(String(e), 'err'); }
  }

  async run(mode: 'fetch' | 'clean', scope: 'page' | 'selection' | 'chapter' | 'errors') {
    if (!this.project) return;
    await this.cv?.flush();
    let ids: number[];
    if (scope === 'page') ids = this.page ? [this.page.id] : [];
    else if (scope === 'errors') ids = this.pages.filter((p) => this.statusOf(p) === 'error').map((p) => p.id);
    else if (scope === 'selection' && this.selected.length) ids = this.pages.filter((p) => this.selected.includes(p.id)).map((p) => p.id);
    else ids = this.pages.map((p) => p.id);
    try { await api.startJob({ page_ids: ids, mode, force: scope !== 'chapter', preset: this.presetFromOpts() }); } catch (e) { this.toast(String(e), 'err'); }
  }
  async exportChapter(onlySelected = false) {
    if (this.exporting) return;
    const list = onlySelected && this.selected.length ? this.pages.filter((p) => this.selected.includes(p.id)) : this.pages;
    const ready = list.filter((p) => this.statusOf(p) === 'cleaned').length;
    if (!ready) { this.toast('Dışa aktarılacak temiz sayfa yok. Önce sayfaları temizleyin.', 'err'); return; }
    const skip = list.length - ready;
    if (skip && !(await this.ask(`${ready} temiz sayfa aktarılacak. ${skip} sayfa henüz temiz değil, atlanacak.`, 'Aktar', false))) return;
    let dir = this.settings?.export_dir;
    if (!dir) { const d = await pick({ directory: true, title: 'Çıktı klasörü' }); if (!d) return; dir = d as string; }
    this.exporting = true;
    try {
      const r = await api.exportPages(list.map((p) => p.id), dir);
      this.toast(`${r.ok} sayfa dışa aktarıldı${r.skipped ? ` · ${r.skipped} atlandı` : ''}`, 'ok', { label: 'Klasörü aç', run: () => api.revealPath(r.dir) });
    } catch (e) { this.toast(String(e), 'err'); }
    this.exporting = false;
  }
  async deletePage(id: number) {
    const p = this.pages.find((x) => x.id === id);
    if (!p || !(await this.ask(`"${p.name}" bölümden kaldırılsın? Orijinal dosyaya dokunulmaz.`))) return;
    await this.cv?.flush();
    try {
      await api.deletePage(id);
      const keep = this.index;
      this.selected = this.selected.filter((x) => x !== id);
      this.pages = await api.listPages(this.project!.chapterId);
      this.index = Math.max(0, Math.min(keep, this.pages.length - 1));
      this.toast('Sayfa kaldırıldı');
    } catch (e) { this.toast(String(e), 'err'); }
  }
  async deleteSelected() {
    const ids = [...this.selected];
    if (ids.length === 1) return this.deletePage(ids[0]);
    if (!ids.length || !(await this.ask(`${ids.length} sayfa bölümden kaldırılsın? Orijinal dosyalara dokunulmaz.`))) return;
    await this.cv?.flush();
    try {
      const keep = this.index;
      for (const id of ids) await api.deletePage(id);
      this.selected = [];
      this.pages = await api.listPages(this.project!.chapterId);
      this.index = Math.max(0, Math.min(keep, this.pages.length - 1));
      this.toast(`${ids.length} sayfa kaldırıldı`);
    } catch (e) { this.toast(String(e), 'err'); }
  }
  async renameChapter(id: number, name: string) {
    name = name.trim();
    if (!name) return false;
    try {
      await api.renameChapter(id, name);
      if (this.project?.chapterId === id) this.project.name = name;
      return true;
    } catch (e) { this.toast(String(e), 'err'); return false; }
  }

  async init() {
    await this.reloadSettings();
    if (this.settings && !localStorage.getItem('safink.opts')) {
      const p = this.settings.presets.find((x) => x.model_id === this.settings!.model_id) ?? this.settings.presets[0];
      if (p) this.applyPreset(p.id);
    }
    api.hardware().then((h) => (this.hw = h));
    this.reloadModels();
    await listen<any>('safink', (ev) => {
      const e = ev.payload;
      switch (e.type) {
        case 'page':
          this.pageStatus[e.page_id] = { status: e.status, error: e.error, rev: e.rev };
          break;
        case 'job':
          this.job = e;
          this.jobActive = e.state === 'running' || e.state === 'paused';
          if (!this.jobActive) {
            this.tick++;
            this.reloadPages();
            if (e.state === 'error') this.toast(e.message, 'err');
            else if (e.state === 'done') this.toast(e.failed ? `Bitti: ${e.total - e.failed} sayfa tamam, ${e.failed} hata` : `Bitti: ${e.total} sayfa işlendi`, e.failed ? 'info' : 'ok');
            else if (e.state === 'cancelled') this.toast('İş iptal edildi');
          }
          break;
        case 'download':
          this.dl[e.model_id] = { stage: e.stage, done: e.done, total: e.total, file: e.file, phase: e.phase ?? '' };
          if (e.stage === 'done') { this.reloadModels(); this.toast('Model hazır', 'ok'); }
          else if (e.stage === 'error') { this.toast('Kurulum yapılamadı: ' + e.file, 'err'); this.reloadModels(); }
          break;
        case 'import':
          this.importProg = e.done >= e.total ? null : { done: e.done, total: e.total };
          break;
      }
    });
  }
}

export const app = new AppState();
