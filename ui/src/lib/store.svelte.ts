import { listen, pick } from './host';
import { api, type Settings, type JobEvt, type Model, type Hardware, type Page, type Preset } from './api';
import { i18n, t, type Locale } from './i18n/index.svelte';

export type Tool = 'hand' | 'zoom' | 'brush' | 'eraser' | 'lasso';
export type PrefsTab = 'clean' | 'export' | 'models';
interface Toast { id: number; text: string; kind: 'info' | 'ok' | 'err'; action?: { label: string; run: () => void } }
interface Ask { text: string; ok: string; danger: boolean; resolve: (v: boolean) => void }
interface DlState { stage: string; done: number; total: number; file: string; phase: string }
interface Layer { vis: boolean; op: number }

export interface Opts { preset_id: string; model_id: string; confidence: number; dilation: number; feather: number }

export interface LastOpen { chapterId: number; name: string; manga: string; pageId: number; index: number }

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
  /** Başlatılan işin düğme anahtarı (örn. 'clean:page'): ilgili düğme çalışıyor görünür. */
  busy = $state<string | null>(null);
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

  // maske genişletme önizlemesi: değer denetimi üstündeyken ya da yakın zamanda değiştirilince tuvalde gösterilir
  dilHover = $state(false);
  dilPoke = $state(false);
  private dilTimer: any = null;
  get dilOn() { return this.dilHover || this.dilPoke; }
  pokeDilation() {
    this.dilPoke = true;
    clearTimeout(this.dilTimer);
    this.dilTimer = setTimeout(() => (this.dilPoke = false), 1400);
  }

  // "kaldığın yerden devam": son açılan bölüm ve sayfa
  last = $state<LastOpen | null>(load<LastOpen | null>('safink.last', null as any));

  private tid = 1;

  get locale(): Locale { return i18n.locale; }
  /** Arayüz dilini değiştirir ve arka uca bildirir (arka uç iletileri bu dilde gelir). */
  setLocale(l: Locale) {
    i18n.set(l);
    api.setLocale(l).catch(() => {});
  }
  saveLast() {
    const p = this.project, pg = this.page;
    if (!p || p.quick || !pg) return;
    const l: LastOpen = { chapterId: p.chapterId, name: p.name, manga: p.manga, pageId: pg.id, index: this.index };
    this.last = l;
    try { localStorage.setItem('safink.last', JSON.stringify(l)); } catch {}
  }
  forgetLast() { this.last = null; try { localStorage.removeItem('safink.last'); } catch {} }

  savePanels() { try { localStorage.setItem('safink.panels', JSON.stringify({ pages: this.pagesOpen, insp: this.inspectorOpen })); } catch {} }
  toggleMask() { const v = !(this.layers.api.vis || this.layers.edit.vis); this.layers.api.vis = v; this.layers.edit.vis = v; this.saveOpts(); }

  get page(): Page | null { return this.pages[this.index] ?? null; }

  toast(text: string, kind: Toast['kind'] = 'info', action?: Toast['action']) {
    const id = this.tid++;
    this.toasts.push({ id, text, kind, action });
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), kind === 'err' ? 7000 : action ? 8000 : 3000);
  }
  /** Uygulama içi onay kutusu (tarayıcı confirm'i yerine). */
  ask(text: string, ok = t('common.delete'), danger = true): Promise<boolean> {
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
    this.saveLast();
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
  /** Son açılan bölümü ve sayfayı yeniden açar; bölüm artık yoksa son kaydı unutur. */
  async resume() {
    const l = this.last;
    if (!l) return;
    try {
      const pages = await api.listPages(l.chapterId);
      if (!pages.length) { this.forgetLast(); return; }
      const i = pages.findIndex((p) => p.id === l.pageId);
      await this.openChapter(l.chapterId, l.name, l.manga, false, i >= 0 ? i : l.index);
    } catch (e) { this.toast(String(e), 'err'); }
  }
  /** Sayfaları sürükleyerek/klavyeyle yeniden sırala: `ids` taşınan sayfalar, `slot` taşınmayanlar arasındaki hedef konum. */
  async movePages(ids: number[], slot: number) {
    if (!this.project || !ids.length) return;
    const set = new Set(ids);
    const moving = this.pages.filter((p) => set.has(p.id));
    const rest = this.pages.filter((p) => !set.has(p.id));
    const at = Math.max(0, Math.min(rest.length, slot));
    const next = [...rest.slice(0, at), ...moving, ...rest.slice(at)];
    if (next.every((p, i) => p.id === this.pages[i].id)) return;
    const cur = this.page?.id;
    this.pages = next;
    const ci = next.findIndex((p) => p.id === cur);
    if (ci >= 0) this.index = ci;
    try { await api.reorderPages(this.project.chapterId, next.map((p) => p.id)); }
    catch (e) { this.toast(String(e), 'err'); this.reloadPages(); }
  }
  /** Geçerli sayfayı bir yukarı (-1) ya da aşağı (+1) taşır. */
  async nudgePage(d: -1 | 1) {
    const i = this.index, p = this.page;
    if (!p || i + d < 0 || i + d >= this.pages.length) return;
    const ids = this.selected.length > 1 && this.selected.includes(p.id) ? this.selected : [p.id];
    const first = this.pages.findIndex((x) => ids.includes(x.id));
    const before = this.pages.slice(0, first).filter((x) => !ids.includes(x.id)).length;
    await this.movePages(ids, before + d);
  }
  /** Açık Quick Clean sonuçları dışa aktarıldı mı (yeni Quick Clean eskisini silmeden önce sormak için). */
  quickExported = false;
  async quick(paths: string[]) {
    // geçerli girdi (görüntü uzantısı ya da klasör) yoksa hiçbir şey silinmez; arka uç da doğrular
    const cand = paths.filter((p) => !/\.[^\\/.]+$/.test(p) || /\.(png|jpe?g|webp|bmp|gif)$/i.test(p));
    if (!cand.length) { this.toast(t('quick.no_images'), 'err'); return; }
    const hasResults = this.pages.some((p) => ['cleaned', 'cleaned_stale'].includes(this.statusOf(p)));
    if (this.project?.quick && hasResults && !this.quickExported && !(await this.ask(t('quick.ask_replace'), t('quick.replace_ok')))) return;
    try {
      const cid = await api.quickNew(paths);
      this.quickExported = false;
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
    if (!ids.length || this.jobActive) return;
    // düğmeye basılır basılmaz görünür durum: arka uç olayı beklemeden iş "çalışıyor" sayılır
    this.busy = `${mode}:${scope}`;
    this.jobActive = true;
    this.job = { job_id: 0, mode, total: ids.length, fetched: 0, cleaned: 0, failed: 0, current: null, state: 'running', message: t('job.starting'), wait_secs: 0 };
    try { await api.startJob({ page_ids: ids, mode, force: scope !== 'chapter', preset: this.presetFromOpts() }); }
    catch (e) { this.jobActive = false; this.busy = null; this.job = null; this.toast(String(e), 'err'); }
  }
  async exportChapter(onlySelected = false) {
    if (this.exporting) return;
    const list = onlySelected && this.selected.length ? this.pages.filter((p) => this.selected.includes(p.id)) : this.pages;
    const ready = list.filter((p) => ['cleaned', 'cleaned_stale'].includes(this.statusOf(p))).length;
    if (!ready) { this.toast(t('export.none'), 'err'); return; }
    const skip = list.length - ready;
    if (skip && !(await this.ask(t('export.confirm', { n: ready, ready, skip }), t('export.confirm_ok'), false))) return;
    let dir = this.settings?.export_dir;
    if (!dir) { const d = await pick({ directory: true, title: t('export.pick_title') }); if (!d) return; dir = d as string; }
    this.exporting = true;
    try {
      const r = await api.exportPages(list.map((p) => p.id), dir);
      if (this.project?.quick && !onlySelected && r.ok) this.quickExported = true;
      this.toast(t('export.done', { n: r.ok }) + (r.skipped ? t('export.skipped', { n: r.skipped }) : ''), 'ok', { label: t('export.open_folder'), run: () => api.revealPath(r.dir) });
    } catch (e) { this.toast(String(e), 'err'); }
    this.exporting = false;
  }
  async deletePage(id: number) {
    const p = this.pages.find((x) => x.id === id);
    if (!p || !(await this.ask(t('pages.ask_remove_one', { name: p.name })))) return;
    await this.cv?.flush();
    try {
      await api.deletePage(id);
      const keep = this.index;
      this.selected = this.selected.filter((x) => x !== id);
      this.pages = await api.listPages(this.project!.chapterId);
      this.index = Math.max(0, Math.min(keep, this.pages.length - 1));
      this.toast(t('pages.removed_one'));
    } catch (e) { this.toast(String(e), 'err'); }
  }
  async deleteSelected() {
    const ids = [...this.selected];
    if (ids.length === 1) return this.deletePage(ids[0]);
    if (!ids.length || !(await this.ask(t('pages.ask_remove_many', { n: ids.length })))) return;
    await this.cv?.flush();
    try {
      const keep = this.index;
      for (const id of ids) await api.deletePage(id);
      this.selected = [];
      this.pages = await api.listPages(this.project!.chapterId);
      this.index = Math.max(0, Math.min(keep, this.pages.length - 1));
      this.toast(t('pages.removed_many', { n: ids.length }));
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
    api.setLocale(i18n.locale).catch(() => {});
    await this.reloadSettings();
    if (this.settings && !localStorage.getItem('safink.opts')) {
      const p = this.settings.presets.find((x) => x.model_id === this.settings!.model_id) ?? this.settings.presets[0];
      if (p) this.applyPreset(p.id);
    }
    api.hardware().then((h) => (this.hw = h));
    this.reloadModels();
    await listen<any>('safink', (ev) => {
      let e = ev.payload;
      switch (e.type) {
        case 'page':
          this.pageStatus[e.page_id] = { status: e.status, error: e.error, rev: e.rev };
          if (this.jobActive) {
            // sayfa hazır olur olmaz liste (maske sayısı, küçük resim) ve açık sayfanın maske/sonuç görüntüsü tazelenir
            this.reloadPages();
            if (e.page_id === this.page?.id) this.tick++;
          }
          break;
        case 'job':
          // bitiş olayı sayaçları sıfırlı gelir; son bilinen sayaçları koru
          if (this.job && this.job.job_id === e.job_id && e.state !== 'running' && e.state !== 'paused') e = { ...e, fetched: this.job.fetched, cleaned: this.job.cleaned, failed: this.job.failed };
          this.job = e;
          this.jobActive = e.state === 'running' || e.state === 'paused';
          if (!this.jobActive) {
            this.busy = null;
            this.tick++;
            this.reloadPages();
            if (e.state === 'error') this.toast(e.message, 'err');
            else if (e.state === 'done') this.toast(e.failed ? t('job.toast_partial', { n: e.total - e.failed, ok: e.total - e.failed, failed: e.failed }) : t('job.toast_done', { n: e.total }), e.failed ? 'info' : 'ok');
            else if (e.state === 'cancelled') this.toast(t('job.toast_cancelled'));
          }
          break;
        case 'download':
          this.dl[e.model_id] = { stage: e.stage, done: e.done, total: e.total, file: e.file, phase: e.phase ?? '' };
          if (e.stage === 'done') { this.reloadModels(); this.toast(t('models.ready'), 'ok'); }
          else if (e.stage === 'error') { this.toast(t('models.install_failed', { msg: e.file }), 'err'); this.reloadModels(); }
          break;
        case 'import':
          this.importProg = e.done >= e.total ? null : { done: e.done, total: e.total };
          break;
      }
    });
  }
}

export const app = new AppState();
