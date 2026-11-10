// Otomatik güncelleme: Tauri updater eklentisi. Tarayıcı (mock) modunda hiçbir şey yapmaz.
import { inTauri } from './host';
import { app } from './store.svelte';
import { t } from './i18n/index.svelte';

const LAST_KEY = 'safink.updateCheck';
const DAY = 24 * 3600 * 1000;

class Updater {
  version = $state<string | null>(null); // bulunan yeni sürüm
  notes = $state('');
  open = $state(false); // bildirim kutusu görünür
  busy = $state(false); // indiriliyor / kuruluyor
  done = $state(0);
  total = $state(0);
  private upd: any = null;

  /** auto: açılıştaki sessiz denetim (günde en çok bir kez). */
  async check(auto: boolean) {
    if (!inTauri || this.busy) return;
    if (auto) {
      try { if (Date.now() - Number(localStorage.getItem(LAST_KEY) ?? 0) < DAY) return; } catch {}
    }
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const u = await check();
      try { localStorage.setItem(LAST_KEY, String(Date.now())); } catch {}
      if (u) { this.upd = u; this.version = u.version; this.notes = u.body ?? ''; this.open = true; }
      else if (!auto) app.toast(t('update.latest'), 'ok');
    } catch (e) {
      if (!auto) app.toast(t('update.failed', { err: String(e) }), 'err');
    }
  }

  later() { this.open = false; }

  async install() {
    if (!this.upd || this.busy) return;
    this.busy = true; this.done = 0; this.total = 0;
    try {
      await this.upd.downloadAndInstall((ev: any) => {
        if (ev.event === 'Started') this.total = ev.data.contentLength ?? 0;
        else if (ev.event === 'Progress') this.done += ev.data.chunkLength;
      });
      const { relaunch } = await import('@tauri-apps/plugin-process');
      await relaunch();
    } catch (e) {
      this.busy = false;
      app.toast(t('update.failed', { err: String(e) }), 'err');
    }
  }
}

export const updater = new Updater();
