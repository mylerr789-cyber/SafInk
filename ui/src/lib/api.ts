import { invoke } from './host';
export { thumbUrl } from './host';

export interface Manga { id: number; name: string; chapters: number }
export interface Chapter { id: number; manga_id: number; name: string; path: string; quick: boolean; pages: number; cleaned: number }
export interface Page {
  id: number; chapter_id: number; idx: number; path: string; name: string; width: number; height: number;
  sha256: string; status: string; error: string; mask_conf: number; detections: number; rev: number;
}
export interface Preset { id: string; name: string; model_id: string; confidence: number; dilation: number; feather: number }
export interface Settings {
  confidence: number; dilation: number; feather: number; model_id: string;
  export_dir: string; export_format: string; export_quality: number; api_rate_per_min: number; presets: Preset[];
  hf_token: string; low_vram: boolean;
}
export interface FileSpec { name: string; role: string; url: string; sha256: string | null; size: number }
export interface Model {
  id: string; name: string; backend: string; kind: string; mode: string; description: string; license: string; license_url: string;
  min_vram_gb: number; min_ram_gb: number; notes: string; recommended: boolean; files: FileSpec[];
  installed: boolean; fit: string; total_bytes: number; downloaded_bytes: number; download_bytes: number; visible: boolean; variant: string;
}
/** gpu_kind: nvidia | other | none */
export interface Hardware { gpu_name: string; vram_gb: number; ram_gb: number; gpu_kind: string }
export interface JobEvt {
  job_id: number; mode: string; total: number; fetched: number; cleaned: number; failed: number; current: number | null;
  state: string; message: string; wait_secs: number;
}

export const api = {
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  listManga: () => invoke<Manga[]>('list_manga'),
  createManga: (name: string) => invoke<number>('create_manga', { name }),
  deleteManga: (id: number) => invoke<void>('delete_manga', { id }),
  listChapters: (mangaId: number) => invoke<Chapter[]>('list_chapters', { mangaId }),
  deleteChapter: (id: number) => invoke<void>('delete_chapter', { id }),
  listPages: (chapterId: number) => invoke<Page[]>('list_pages', { chapterId }),
  getPage: (id: number) => invoke<Page | null>('get_page', { id }),
  deletePage: (id: number) => invoke<void>('delete_page', { id }),
  importFolder: (mangaId: number, path: string) => invoke<number[]>('import_folder', { mangaId, path }),
  addToChapter: (chapterId: number, paths: string[]) => invoke<number>('add_to_chapter', { chapterId, paths }),
  quickNew: (paths: string[]) => invoke<number>('quick_new', { paths }),
  startJob: (spec: { page_ids: number[]; mode: 'fetch' | 'clean'; force?: boolean; preset?: Preset | null }) => invoke<number>('start_job', { spec }),
  cancelJob: () => invoke<void>('cancel_job'),
  pauseJob: (paused: boolean) => invoke<void>('pause_job', { paused }),
  exportPages: (pageIds: number[], outDir?: string) => invoke<{ ok: number; skipped: number; dir: string }>('export_pages', { pageIds, outDir }),
  revealPath: (path: string) => invoke<void>('reveal_path', { path }),
  saveEdit: (pageId: number, body: Uint8Array) => invoke<void>('save_edit', body, { headers: { 'x-page-id': String(pageId) } }),
  resetEdits: (pageId: number) => invoke<void>('reset_edits', { pageId }),
  listModels: () => invoke<Model[]>('list_models'),
  hardware: () => invoke<Hardware>('hardware'),
  installModel: (id: string) => invoke<void>('install_model', { id }),
  cancelInstall: (id: string) => invoke<void>('cancel_install', { id }),
  removeModel: (id: string) => invoke<void>('remove_model', { id }),
};


export const fmtBytes = (n: number) => {
  if (n >= 2 ** 30) return (n / 2 ** 30).toFixed(1) + ' GB';
  if (n >= 2 ** 20) return (n / 2 ** 20).toFixed(0) + ' MB';
  return (n / 1024).toFixed(0) + ' KB';
};

/** Sayfa durumu: etiket + müsvedde dilindeki işaret (tag sınıfı). */
export const STATUS: Record<string, { label: string; tag: string }> = {
  new: { label: 'Yeni', tag: '' },
  mask_fetched: { label: 'Maskeli', tag: 'tag-red' },
  mask_edited: { label: 'Düzenlendi', tag: 'tag-blue' },
  cleaned: { label: 'Temiz', tag: 'tag-ink' },
  error: { label: 'Hata', tag: 'tag-red' },
};
