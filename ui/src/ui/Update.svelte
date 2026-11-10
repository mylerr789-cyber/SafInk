<script lang="ts">
  import { updater as u } from '../lib/updater.svelte';
  import { t } from '../lib/i18n/index.svelte';
  const pct = $derived(u.total ? Math.min(100, (u.done / u.total) * 100) : 0);
</script>

{#if u.open && u.version}
  <div class="fixed bottom-6 right-6 z-[170] koma w-[320px] shadow-[6px_6px_0_var(--color-ink)] slidein" role="alert">
    <div class="h-[40px] px-4 flex items-center bg-ink text-paper"><span class="disp text-[18px] leading-none">{t('update.title')}</span></div>
    <div class="px-4 py-3 text-[13.5px] leading-5">
      <div class="font-semibold">{t('update.available', { v: u.version })}</div>
      {#if u.busy}
        <div class="mt-3 flex items-center gap-3">
          <span class="flex-1 h-[11px] border-[1.5px] border-ink relative"><span class="tone absolute inset-y-0 left-0" style="width:{u.total ? pct : 100}%"></span></span>
          <span class="mono text-[12px]">{u.total ? Math.round(pct) + '%' : '…'}</span>
        </div>
        <div class="mono text-[11px] muted mt-1">{t('update.installing')}</div>
      {/if}
    </div>
    {#if !u.busy}
      <div class="flex justify-end gap-3 px-4 pb-4">
        <button class="btn" onclick={() => u.later()}>{t('update.later')}</button>
        <button class="btn print btn-ink" onclick={() => u.install()}>{t('update.install')}</button>
      </div>
    {/if}
  </div>
{/if}
