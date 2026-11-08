// Tauri köprüsü. Tauri dışında (yalnızca `npm run dev` + tarayıcı) sahte arka uç kullanılır; üretim derlemesinde mock yoktur.
import { invoke as tInvoke } from '@tauri-apps/api/core';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const useMock = import.meta.env.DEV && !inTauri;
const mock = useMock ? import('./mock') : null;

export async function invoke<T>(cmd: string, args?: any, opts?: any): Promise<T> {
  if (mock) return (await mock).invoke(cmd, args, opts) as T;
  return tInvoke<T>(cmd, args, opts);
}

export async function listen<T>(ch: string, cb: (ev: { payload: T }) => void): Promise<() => void> {
  if (mock) return (await mock).listen(cb as any);
  const { listen } = await import('@tauri-apps/api/event');
  return listen<T>(ch, cb);
}

export async function onDragDrop(cb: (p: { type: string; paths: string[] }) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  return getCurrentWebview().onDragDropEvent((ev) => cb(ev.payload as any));
}

export async function pick(o: { directory?: boolean; multiple?: boolean; title?: string; filters?: { name: string; extensions: string[] }[] }): Promise<string | string[] | null> {
  if (mock) return (await mock).pick(o);
  const { open } = await import('@tauri-apps/plugin-dialog');
  return open(o) as any;
}

const isWin = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
const base = isWin ? 'http://safink.localhost' : 'safink://localhost';

/** Bir sayfa katmanının 1024 karosunu getirir; yoksa null. */
export async function tile(page: number, layer: string, tx: number, ty: number, v: number | string): Promise<ImageBitmap | null> {
  if (mock) return (await mock).tile(page, layer, tx, ty);
  try {
    const r = await fetch(`${base}/tile/${page}/${layer}/${tx}/${ty}?v=${v}`);
    if (!r.ok) return null;
    return await createImageBitmap(await r.blob(), { colorSpaceConversion: 'none' });
  } catch { return null; }
}

let mockThumbs: ((p: number) => string) | null = null;
/** mock yüklenene kadar bekle (main.ts). */
export const ready: Promise<void> = mock ? mock.then((m) => { mockThumbs = m.thumb; }) : Promise.resolve();
export const thumbUrl = (page: number, v: number | string) => (mockThumbs ? mockThumbs(page) : `${base}/thumb/${page}?v=${v}`);
