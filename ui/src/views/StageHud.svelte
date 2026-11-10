<script lang="ts">
  import { ChevronLeft, ChevronRight } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  import { t } from '../lib/i18n/index.svelte';
  let zoomOpen = $state(false);
  const zooms = [0.25, 0.5, 1, 2, 4];
  const pad = (n: number) => String(n).padStart(2, '0');
</script>

<svelte:window onpointerdown={(e) => { if (!(e.target as HTMLElement).closest('[data-zoom]')) zoomOpen = false; }} />

{#if app.pages.length > 1}
  <div class="absolute bottom-3.5 left-1/2 -translate-x-1/2 z-20 flex bg-sheet border-2 border-ink shadow-[4px_4px_0_var(--color-ink)]">
    <button class="w-9 h-9 grid place-items-center hover:bg-ink hover:text-paper disabled:opacity-25" aria-label={t('hud.prev_page')} data-tip="{t('hud.prev')}  ←" data-tip-side="top" disabled={app.index === 0} onclick={() => app.go(app.index - 1)}><ChevronLeft size={17} strokeWidth={2.5} /></button>
    <span class="px-3.5 grid place-items-center mono font-semibold border-x-[1.5px] border-ink">P.{pad(app.index + 1)} / {pad(app.pages.length)}</span>
    <button class="w-9 h-9 grid place-items-center hover:bg-ink hover:text-paper disabled:opacity-25" aria-label={t('hud.next_page')} data-tip="{t('hud.next')}  →" data-tip-side="top" disabled={app.index >= app.pages.length - 1} onclick={() => app.go(app.index + 1)}><ChevronRight size={17} strokeWidth={2.5} /></button>
  </div>
{/if}

<div class="absolute bottom-3.5 right-3.5 z-20 flex items-end gap-3" data-zoom>
  {#if app.saving}<span class="mono text-[11px] bg-sheet px-1.5 border-[1.5px] border-ink">{t('hud.saving')}</span>{/if}
  <div class="relative flex bg-sheet border-2 border-ink shadow-[4px_4px_0_var(--color-ink)] mono font-semibold text-[12.5px]">
    <button class="h-9 px-3 hover:bg-ink hover:text-paper" onclick={() => app.cv?.fit('page')} data-tip="{t('view.fit_page')}  Ctrl 0" data-tip-side="top">{t('hud.fit')}</button>
    <button class="h-9 px-3 border-l-[1.5px] border-ink hover:bg-ink hover:text-paper min-w-[62px]" onclick={() => (zoomOpen = !zoomOpen)}>{app.zoomPct}%</button>
    {#if zoomOpen}
      <div class="pop bottom-[calc(100%+8px)] right-0 w-[200px] font-sans font-normal">
        <button class="pop-item" onclick={() => { app.cv?.fit('page'); zoomOpen = false; }}><span class="flex-1">{t('view.fit_page')}</span><span class="mono text-[11px] muted">Ctrl 0</span></button>
        <button class="pop-item" onclick={() => { app.cv?.fit('width'); zoomOpen = false; }}><span class="flex-1">{t('view.fit_width')}</span><span class="mono text-[11px] muted">Ctrl 1</span></button>
        <div class="pop-sep"></div>
        {#each zooms as z}
          <button class="pop-item mono" onclick={() => { app.cv?.zoomTo(z); zoomOpen = false; }}><span class="flex-1">{z * 100}%</span>{#if z === 1}<span class="text-[11px] muted">Ctrl 2</span>{/if}</button>
        {/each}
      </div>
    {/if}
  </div>
</div>
