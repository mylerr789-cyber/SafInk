<script lang="ts">
  import { X } from 'lucide-svelte';
  import { api, type Settings } from '../lib/api';
  import { pick } from '../lib/host';
  import { t, LOCALES, type Locale } from '../lib/i18n/index.svelte';
  import { app, type PrefsTab } from '../lib/store.svelte';
  import Models from './Models.svelte';
  import Scrub from '../ui/Scrub.svelte';
  import Select from '../ui/Select.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Switch from '../ui/Switch.svelte';

  let s = $state<Settings | null>(null);
  $effect(() => { if (!s && app.settings) s = structuredClone($state.snapshot(app.settings)); });
  const dirty = $derived(!!s && !!app.settings && JSON.stringify(s) !== JSON.stringify(app.settings));

  const tabs = $derived<{ id: PrefsTab; label: string; no: string }[]>([
    { id: 'clean', label: t('prefs.tab_clean'), no: '01' },
    { id: 'export', label: t('prefs.tab_export'), no: '02' },
    { id: 'models', label: t('common.models'), no: '03' },
  ]);
  async function save() {
    if (!s) return;
    try {
      await api.saveSettings($state.snapshot(s));
      await app.reloadSettings();
      app.toast(t('prefs.saved'), 'ok');
    } catch (e) { app.toast(String(e), 'err'); }
  }
  async function pickDir() { const d = await pick({ directory: true }); if (d && s) s.export_dir = d as string; }
  async function close() {
    if (app.asking) return;
    if (dirty && !(await app.ask(t('prefs.ask_discard'), t('common.close')))) return;
    app.dialog = null;
  }
  const modelOpts = $derived(app.models.map((m) => ({ value: m.id, label: m.name, hint: m.installed ? t('prefs.installed') : t('insp.not_installed'), mark: (m.installed ? 'ok' : 'off') as 'ok' | 'off' })));
  const cur = $derived(tabs.find((t) => t.id === app.prefsTab)!);
</script>

{#snippet row(title: string, help: string = '')}
  <div class="min-w-0"><div class="font-semibold">{title}</div>{#if help}<div class="text-[12px] muted leading-4 mt-0.5">{help}</div>{/if}</div>
{/snippet}

<svelte:window onkeydown={(e) => e.key === 'Escape' && close()} />
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-[80] bg-paper/75 grid place-items-center fade-in" onclick={close}>
  <div class="slidein w-[940px] max-w-[94vw] h-[660px] max-h-[92vh] koma shadow-[8px_8px_0_var(--color-ink)] flex overflow-hidden" onclick={(e) => e.stopPropagation()}>
    <nav class="w-[210px] shrink-0 border-r-[2.5px] border-ink flex flex-col bg-paper">
      <div class="h-[56px] shrink-0 bg-ink text-paper flex items-center px-4"><span class="disp text-[22px] leading-none">{t('prefs.title')}</span></div>
      {#each tabs as t}
        <button class="h-[46px] px-4 flex items-center gap-3 border-b-[1.5px] border-ink text-left {app.prefsTab === t.id ? 'bg-sheet' : 'hover:bg-desk'}" onclick={() => (app.prefsTab = t.id)}>
          <span class="mono text-[11px] {app.prefsTab === t.id ? 'bg-ink text-paper px-1' : 'muted'}">{t.no}</span>
          <span class="font-semibold">{t.label}</span>
        </button>
      {/each}
      <div class="flex-1 tone-light"></div>
      <!-- dil: hemen uygulanır, Kaydet gerektirmez -->
      <div class="shrink-0 border-t-[2.5px] border-ink p-3 flex flex-col gap-2 bg-paper">
        <span class="eyebrow">{t('prefs.language')}</span>
        <Segmented value={app.locale} full options={LOCALES} onchange={(v) => app.setLocale(v as Locale)} />
      </div>
    </nav>
    <div class="flex-1 min-w-0 flex flex-col">
      <div class="h-[56px] shrink-0 px-6 flex items-center border-b-[2.5px] border-ink">
        <span class="mono text-[12px] mr-3">{cur.no}</span><span class="disp text-[22px] leading-none">{cur.label}</span>
        <div class="flex-1"></div>
        <button class="btn btn-icon" aria-label={t('common.close')} onclick={close}><X size={17} strokeWidth={2.5} /></button>
      </div>
      <div class="flex-1 min-h-0 overflow-y-auto px-6 py-3 {app.prefsTab === 'models' ? 'bg-paper' : ''}">
        {#if app.prefsTab === 'models'}
          <div class="py-3"><Models /></div>
        {:else if s}
          {#if app.prefsTab === 'clean'}
            <div class="prow">{@render row(t('prefs.default_model'), t('prefs.default_model_help'))}<Select bind:value={s.model_id} options={modelOpts} width="320px" /></div>
            <div class="prow">{@render row(t('prefs.dilation'), t('prefs.dilation_help'))}<Scrub bind:value={s.dilation} min={0} max={16} unit="px" width="220px" /></div>
            <div class="prow">{@render row(t('prefs.feather'), t('prefs.feather_help'))}<Scrub bind:value={s.feather} min={0} max={16} unit="px" width="220px" /></div>
            <div class="prow">{@render row(t('prefs.flat_fill'))}<Switch bind:checked={s.flat_fill} /></div>
          {:else if app.prefsTab === 'export'}
            <div class="prow">{@render row(t('prefs.out_dir'), t('prefs.out_dir_help'))}
              <div class="flex gap-2"><input class="field w-[240px] mono text-[12px]" bind:value={s.export_dir} placeholder={t('prefs.out_dir_ph')} /><button class="btn" onclick={pickDir}>{t('prefs.browse')}</button></div></div>
            <div class="prow">{@render row(t('prefs.format'))}
              <Segmented bind:value={s.export_format} options={[{ value: 'original', label: t('prefs.f_original') }, { value: 'png', label: 'PNG' }, { value: 'jpeg', label: 'JPEG' }, { value: 'webp', label: 'WebP' }]} /></div>
            <div class="prow">{@render row(t('prefs.jpeg_quality'))}<Scrub bind:value={s.export_quality} min={50} max={100} width="220px" disabled={s.export_format !== 'jpeg' && s.export_format !== 'original'} /></div>
          {/if}
        {/if}
      </div>
      {#if app.prefsTab !== 'models'}
        <div class="h-[62px] shrink-0 border-t-[2.5px] border-ink px-6 flex items-center justify-end gap-3 bg-paper">
          {#if dirty}<span class="mono text-[11.5px] text-red mr-auto">{t('prefs.unsaved')}</span>{/if}
          <button class="btn" onclick={close}>{t('common.close')}</button>
          <button class="btn btn-ink print" onclick={save}>{t('prefs.save')}</button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .prow { display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 14px 0; border-bottom: 1.5px dashed #12121230; }
</style>
