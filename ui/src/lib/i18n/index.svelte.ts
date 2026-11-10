// Küçük i18n katmanı: sözlükler tr.ts / en.ts, `t(key, params?)` ve reaktif geçerli dil.
// Dil seçimi localStorage'da tutulur; ilk açılışta işletim sistemi dili "tr" ile başlıyorsa Türkçe, değilse İngilizce.
// Arka uç iletileri için dil `set_locale` komutuyla Rust'a bildirilir (bkz. docs/ipc.md).
import { tr, type Key } from './tr';
import { en } from './en';

export type Locale = 'tr' | 'en';
export const LOCALES: { value: Locale; label: string }[] = [{ value: 'tr', label: 'Türkçe' }, { value: 'en', label: 'English' }];
const KEY = 'safink.locale';

function detect(): Locale {
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === 'tr' || saved === 'en') return saved;
  } catch { /* localStorage yok */ }
  const l = (typeof navigator !== 'undefined' ? navigator.languages?.[0] || navigator.language : '') || '';
  return l.toLowerCase().startsWith('tr') ? 'tr' : 'en';
}

class I18n {
  locale = $state<Locale>(detect());
  constructor() { this.apply(this.locale); }
  /** html lang özniteliği: CSS text-transform: uppercase (İ/I) doğru çalışsın. */
  private apply(l: Locale) { if (typeof document !== 'undefined') document.documentElement.lang = l; }
  set(l: Locale) {
    this.locale = l;
    this.apply(l);
    try { localStorage.setItem(KEY, l); } catch { /* yoksay */ }
  }
}
export const i18n = new I18n();

const dicts: Record<Locale, Record<string, string>> = { tr, en };

/** Çeviri: `{ad}` yer tutucuları params ile doldurulur; params.n === 1 ve `anahtar.one` varsa o kullanılır. */
export function t(key: Key, params?: Record<string, string | number>): string {
  const d = dicts[i18n.locale];
  let s = (params && params.n === 1 && d[key + '.one']) || d[key] || tr[key] || key;
  if (params) s = s.replace(/\{(\w+)\}/g, (m, k) => (k in params ? String(params[k]) : m));
  return s;
}
