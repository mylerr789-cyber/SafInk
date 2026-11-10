<script lang="ts">
  import { Plus, X, Trash2, RefreshCw } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  import { thumbUrl } from '../lib/api';
  import { importPages } from '../lib/actions';
  import Stamp from '../ui/Stamp.svelte';

  let list: HTMLDivElement;
  let anchor = 0;
  const pad = (n: number) => String(n).padStart(2, '0');
  const counts = $derived.by(() => {
    const c = { cleaned: 0, error: 0 };
    for (const p of app.pages) { const s = app.statusOf(p); if (s === 'cleaned') c.cleaned++; else if (s === 'error') c.error++; }
    return c;
  });

  function click(e: MouseEvent, i: number) {
    const p = app.pages[i];
    if (e.ctrlKey || e.metaKey) {
      app.selected = app.selected.includes(p.id) ? app.selected.filter((x) => x !== p.id) : [...app.selected, p.id];
      anchor = i;
    } else if (e.shiftKey) {
      const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)];
      app.selected = app.pages.slice(a, b + 1).map((x) => x.id);
    } else { app.selected = []; anchor = i; app.go(i); }
  }
  $effect(() => {
    const i = app.index;
    (list?.children[i] as HTMLElement | undefined)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  });
</script>

<aside class="koma flex flex-col overflow-hidden">
  <div class="h-[44px] shrink-0 px-3 flex items-center gap-2 border-b-[2.5px] border-ink">
    <span class="disp text-[19px] leading-none">Sayfalar</span>
    <span class="mono text-[11px] muted">{pad(app.pages.length)}</span>
    <div class="flex-1"></div>
    <button class="btn btn-sm btn-icon" aria-label="Sayfa ekle" data-tip="Sayfa ekle" disabled={!app.project} onclick={importPages}><Plus size={15} strokeWidth={2.25} /></button>
  </div>
  {#if app.pages.length}
    <div class="h-[9px] shrink-0 flex border-b-[2.5px] border-ink" data-tip="{counts.cleaned} temiz · {counts.error} hata · {app.pages.length - counts.cleaned - counts.error} bekliyor">
      <span class="bg-ink" style="width:{(counts.cleaned / app.pages.length) * 100}%"></span>
      <span class="bg-red" style="width:{(counts.error / app.pages.length) * 100}%"></span>
      <span class="flex-1 tone-light"></span>
    </div>
  {/if}

  <div class="flex-1 min-h-0 overflow-y-auto bg-paper px-3.5 pt-3.5 pb-4 flex flex-col gap-5" bind:this={list}>
    {#each app.pages as p, i (p.id)}
      {@const st = app.statusOf(p)}
      {@const active = i === app.index}
      {@const sel = app.selected.includes(p.id)}
      {@const err = app.pageStatus[p.id]?.error || p.error}
      <button class="group relative shrink-0 text-left" onclick={(e) => click(e, i)} title={p.name + (err ? '\n' + err : '')}>
        <div class="relative bg-white border-[1.5px] border-ink overflow-hidden transition-shadow
          {active ? 'shadow-[4px_4px_0_var(--color-ink)] outline-[3px] outline outline-ink outline-offset-[3px]' : sel ? 'shadow-[4px_4px_0_var(--color-blue)] outline-2 outline-dashed outline-blue outline-offset-[3px]' : 'shadow-[4px_4px_0_var(--color-ink)] group-hover:shadow-[6px_6px_0_var(--color-ink)]'}"
          style="aspect-ratio:{Math.max(0.5, Math.min(1, p.width / p.height))}">
          <img src={thumbUrl(p.id, app.revOf(p))} alt="" loading="lazy" draggable="false" class="w-full h-full object-cover object-top" />
        </div>
        {#if app.jobActive && app.job?.current === p.id}<span class="absolute left-1.5 top-1.5 tag tag-ink animate-pulse">işleniyor</span>{/if}
        <span class="absolute left-0 top-0 w-7 h-7 grid place-items-center bg-ink text-paper hover:bg-red opacity-0 group-hover:opacity-100 cursor-pointer" role="button" tabindex="-1" aria-label="Sayfayı kaldır" data-tip="Bölümden kaldır  Del" data-tip-side="right"
          onclick={(e) => { e.stopPropagation(); app.deletePage(p.id); }} onkeydown={() => {}}><Trash2 size={13} /></span>
        {#if st === 'cleaned'}<span class="absolute -right-2 top-2.5"><Stamp size={46} /></span>
        {:else if st === 'error'}<span class="absolute -right-1.5 top-3"><Stamp kind="error" size={46} /></span>{/if}
        <div class="mt-2 flex items-center justify-between gap-2">
          <span class="mono font-semibold text-[12px] {active ? 'bg-ink text-paper px-1' : ''}">P.{pad(i + 1)}</span>
          {#if st === 'mask_fetched' || (st === 'cleaned' && p.detections)}<span class="tag tag-red">{p.detections || '—'} maske</span>
          {:else if st === 'mask_edited'}<span class="tag tag-blue">düzeltildi</span>
          {:else if st === 'new'}<span class="tag !border-ink3 text-ink3">yeni</span>{/if}
        </div>
      </button>
    {/each}
    {#if !app.pages.length}<div class="muted text-[12px] p-2 text-center">Sayfa yok</div>{/if}
  </div>

  {#if counts.error && !app.selected.length}
    <div class="shrink-0 border-t-[2.5px] border-ink p-2.5 flex flex-col gap-2 slidein">
      <div class="mono font-semibold text-[12px] text-red">{pad(counts.error)} HATA</div>
      <button class="btn btn-sm" disabled={app.jobActive} onclick={() => app.run('clean', 'errors')}><RefreshCw size={13} /> Hatalıları yeniden dene</button>
    </div>
  {/if}
  {#if app.selected.length}
    <div class="shrink-0 border-t-[2.5px] border-ink p-2.5 flex flex-col gap-2 slidein">
      <div class="flex items-center"><span class="mono font-semibold text-[12px] text-blue">{pad(app.selected.length)} SEÇİLİ</span><div class="flex-1"></div>
        <button class="btn btn-sm btn-icon btn-bare !h-6 !w-6" aria-label="Seçimi kaldır" onclick={() => (app.selected = [])}><X size={14} /></button></div>
      <button class="btn btn-sm btn-ink" disabled={app.jobActive} onclick={() => app.run('clean', 'selection')}>Seçilenleri temizle</button>
      <button class="btn btn-sm" onclick={() => app.exportChapter(true)}>Dışa aktar</button>
      <button class="btn btn-sm btn-danger" onclick={() => app.deleteSelected()}><Trash2 size={13} /> Bölümden kaldır</button>
    </div>
  {/if}
</aside>
