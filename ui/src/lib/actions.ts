import { pick as open } from './host';
import { api } from './api';
import { app } from './store.svelte';

const lastSeg = (p: string, up = 0) => {
  const parts = p.replace(/[\\/]+$/, '').split(/[\\/]/);
  return parts[parts.length - 1 - up] || 'Manga';
};

/** Klasör seç ve (verilen ya da klasörün üst klasör adından türetilen) mangaya içe aktar; ilk bölümü aç. */
export async function importChapterFolder(mangaId?: number, mangaName?: string) {
  const dir = await open({ directory: true, title: 'Bölüm klasörünü seç (ya da bölümleri içeren klasör)' });
  if (!dir) return;
  try {
    let mid = mangaId;
    let mname = mangaName ?? '';
    if (!mid) {
      mname = app.project && !app.project.quick ? app.project.manga : lastSeg(dir as string, 1);
      mid = await api.createManga(mname);
    }
    const ids = await api.importFolder(mid, dir as string);
    const chs = await api.listChapters(mid);
    const ch = chs.find((c) => c.id === ids[0]);
    await app.openChapter(ids[0], ch?.name ?? lastSeg(dir as string), mname || 'Manga');
    app.toast(`${ids.length} bölüm içe aktarıldı`, 'ok');
  } catch (e) { app.toast(String(e), 'err'); }
}

export async function importPages() {
  if (!app.project) return;
  const f = await open({ multiple: true, filters: [{ name: 'Görseller', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'gif'] }] });
  if (!f) return;
  try {
    await api.addToChapter(app.project.chapterId, Array.isArray(f) ? f : [f]);
    await app.reloadPages();
  } catch (e) { app.toast(String(e), 'err'); }
}

export async function quickFromPicker(kind: 'folder' | 'files') {
  const f = kind === 'folder'
    ? await open({ directory: true })
    : await open({ multiple: true, filters: [{ name: 'Görseller', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'gif'] }] });
  if (f) await app.quick(Array.isArray(f) ? f : [f as string]);
}

export async function resetPageEdits() {
  if (!app.page || !confirm('Bu sayfadaki elle düzenlemeler silinsin? (API maskesi korunur)')) return;
  await app.cv?.flush();
  await api.resetEdits(app.page.id);
  app.cv?.reloadAll();
}
