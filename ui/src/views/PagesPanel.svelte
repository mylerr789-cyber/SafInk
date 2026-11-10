<script lang="ts">
  import { Plus, X, Trash2, RefreshCw } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  import { thumbUrl } from '../lib/api';
  import { importPages } from '../lib/actions';
  import { t } from '../lib/i18n/index.svelte';
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
    if (suppressClick) { suppressClick = false; return; }
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

  // ---------- sürükleyerek sıralama (işaretçi olayları: Tauri'de HTML5 sürükle-bırak dosya bırakma ile çakışır) ----------
  // pos: sayfa listesinde ekleme konumu (0..n); slot: taşınmayan sayfalar arasındaki karşılığı.
  let drag = $state<{ ids: number[]; label: string; x: number; y: number; pos: number; slot: number; noop: boolean } | null>(null);
  let pending: { i: number; x: number; y: number } | null = null;
  let suppressClick = false;
  let autoDir = 0;
  let raf = 0;
  let lastPt = { x: 0, y: 0 };

  function down(e: PointerEvent, i: number) {
    if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || (e.target as HTMLElement).closest('[data-nodrag]')) return;
    pending = { i, x: e.clientX, y: e.clientY };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('keydown', key, true);
  }
  function ptSlot(clientY: number) {
    const kids = Array.from(list.children).filter((c) => (c as HTMLElement).dataset.page) as HTMLElement[];
    let pos = kids.length;
    for (let k = 0; k < kids.length; k++) { const r = kids[k].getBoundingClientRect(); if (clientY < r.top + r.height / 2) { pos = k; break; } }
    return pos;
  }
  function update(clientX: number, clientY: number) {
    if (!drag) return;
    const pos = ptSlot(clientY);
    const set = new Set(drag.ids);
    const slot = app.pages.slice(0, pos).filter((p) => !set.has(p.id)).length;
    const first = app.pages.findIndex((p) => set.has(p.id));
    const firstSlot = app.pages.slice(0, first).filter((p) => !set.has(p.id)).length;
    const idx = app.pages.map((p, k) => (set.has(p.id) ? k : -1)).filter((k) => k >= 0);
    const contiguous = idx[idx.length - 1] - idx[0] + 1 === idx.length;
    drag = { ...drag, x: clientX, y: clientY, pos, slot, noop: contiguous && slot === firstSlot };
  }
  function move(e: PointerEvent) {
    lastPt = { x: e.clientX, y: e.clientY };
    if (!drag) {
      if (!pending || Math.hypot(e.clientX - pending.x, e.clientY - pending.y) < 6) return;
      const p = app.pages[pending.i];
      const ids = app.selected.length > 1 && app.selected.includes(p.id) ? app.pages.filter((x) => app.selected.includes(x.id)).map((x) => x.id) : [p.id];
      drag = { ids, label: 'P.' + pad(pending.i + 1) + (ids.length > 1 ? ` +${ids.length - 1}` : ''), x: e.clientX, y: e.clientY, pos: 0, slot: 0, noop: true };
      document.body.style.cursor = 'grabbing';
      loop();
    }
    update(e.clientX, e.clientY);
    const r = list.getBoundingClientRect();
    autoDir = e.clientY < r.top + 36 ? -1 : e.clientY > r.bottom - 36 ? 1 : 0;
  }
  function loop() {
    if (!drag) return;
    if (autoDir) { list.scrollTop += autoDir * 12; update(lastPt.x, lastPt.y); }
    raf = requestAnimationFrame(loop);
  }
  function finish() {
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', cancel);
    window.removeEventListener('keydown', key, true);
    cancelAnimationFrame(raf);
    document.body.style.cursor = '';
    pending = null; autoDir = 0;
  }
  function up() {
    const d = drag;
    drag = null;
    if (d) { suppressClick = true; setTimeout(() => (suppressClick = false), 0); if (!d.noop) app.movePages(d.ids, d.slot); }
    finish();
  }
  function cancel() { drag = null; finish(); }
  function key(e: KeyboardEvent) { if (e.key === 'Escape') { e.stopPropagation(); e.preventDefault(); cancel(); } }

  // bırakma çizgisinin konumu (kaydırılan listenin içeriğine göre)
  const barTop = $derived.by(() => {
    if (!drag || drag.noop || !list) return -1;
    const kids = Array.from(list.children).filter((c) => (c as HTMLElement).dataset.page) as HTMLElement[];
    if (!kids.length) return -1;
    return drag.pos < kids.length ? kids[drag.pos].offsetTop - 11 : kids[kids.length - 1].offsetTop + kids[kids.length - 1].offsetHeight + 8;
  });
</script>

<aside class="koma flex flex-col overflow-hidden">
  <div class="h-[44px] shrink-0 px-3 flex items-center gap-2 border-b-[2.5px] border-ink">
    <span class="disp text-[19px] leading-none">{t('pages.title')}</span>
    <span class="mono text-[11px] muted">{pad(app.pages.length)}</span>
    <div class="flex-1"></div>
    <button class="btn btn-sm btn-icon" aria-label={t('pages.add')} data-tip={t('pages.add')} disabled={!app.project} onclick={importPages}><Plus size={15} strokeWidth={2.25} /></button>
  </div>
  {#if app.pages.length}
    <div class="h-[9px] shrink-0 flex border-b-[2.5px] border-ink" data-tip={t('pages.summary', { cleaned: counts.cleaned, error: counts.error, pending: app.pages.length - counts.cleaned - counts.error })}>
      <span class="bg-ink" style="width:{(counts.cleaned / app.pages.length) * 100}%"></span>
      <span class="bg-red" style="width:{(counts.error / app.pages.length) * 100}%"></span>
      <span class="flex-1 tone-light"></span>
    </div>
  {/if}

  <div class="relative flex-1 min-h-0 overflow-y-auto bg-paper px-3.5 pt-3.5 pb-4 flex flex-col gap-5" bind:this={list}>
    {#each app.pages as p, i (p.id)}
      {@const st = app.statusOf(p)}
      {@const active = i === app.index}
      {@const sel = app.selected.includes(p.id)}
      {@const err = app.pageStatus[p.id]?.error || p.error}
      <button class="group relative shrink-0 text-left {drag?.ids.includes(p.id) ? 'opacity-35' : ''}" data-page={p.id} onclick={(e) => click(e, i)} onpointerdown={(e) => down(e, i)} title={p.name + (err ? '\n' + err : '')}>
        <div class="relative bg-white border-[1.5px] border-ink overflow-hidden transition-shadow
          {active ? 'shadow-[4px_4px_0_var(--color-ink)] outline-[3px] outline outline-ink outline-offset-[3px]' : sel ? 'shadow-[4px_4px_0_var(--color-blue)] outline-2 outline-dashed outline-blue outline-offset-[3px]' : 'shadow-[4px_4px_0_var(--color-ink)] group-hover:shadow-[6px_6px_0_var(--color-ink)]'}"
          style="aspect-ratio:{Math.max(0.5, Math.min(1, p.width / p.height))}">
          <img src={thumbUrl(p.id, app.revOf(p))} alt="" loading="lazy" draggable="false" class="w-full h-full object-cover object-top" />
        </div>
        {#if app.jobActive && app.job?.current === p.id}<span class="absolute left-1.5 top-1.5 tag tag-ink animate-pulse">{t('pages.processing')}</span>{/if}
        <span class="absolute left-0 top-0 w-7 h-7 grid place-items-center bg-ink text-paper hover:bg-red opacity-0 group-hover:opacity-100 cursor-pointer" data-nodrag role="button" tabindex="-1" aria-label={t('pages.remove')} data-tip="{t('pages.remove_tip')}  Del" data-tip-side="right"
          onclick={(e) => { e.stopPropagation(); app.deletePage(p.id); }} onkeydown={() => {}}><Trash2 size={13} /></span>
        {#if st === 'cleaned'}<span class="absolute -right-2 top-2.5"><Stamp size={46} /></span>
        {:else if st === 'error'}<span class="absolute -right-1.5 top-3"><Stamp kind="error" size={46} /></span>{/if}
        <div class="mt-2 flex items-center justify-between gap-2">
          <span class="mono font-semibold text-[12px] {active ? 'bg-ink text-paper px-1' : ''}">P.{pad(i + 1)}</span>
          {#if st === 'mask_fetched' || (st === 'cleaned' && p.detections)}<span class="tag tag-red">{p.detections ? t('pages.masks', { n: p.detections }) : '—'}</span>
          {:else if st === 'mask_edited'}<span class="tag tag-blue">{t('pages.fixed')}</span>
          {:else if st === 'new'}<span class="tag !border-ink3 text-ink3">{t('pages.new')}</span>{/if}
        </div>
      </button>
    {/each}
    {#if !app.pages.length}<div class="muted text-[12px] p-2 text-center">{t('pages.none')}</div>{/if}
    <!-- bırakma göstergesi: iki sayfa arasında kalın mürekkep çizgisi, uçlarda kare -->
    {#if barTop >= 0}
      <div class="absolute left-1.5 right-1.5 h-[4px] bg-ink pointer-events-none z-10" style="top:{barTop}px">
        <span class="absolute -left-1 -top-[3px] w-[10px] h-[10px] bg-ink"></span><span class="absolute -right-1 -top-[3px] w-[10px] h-[10px] bg-ink"></span>
      </div>
    {/if}
  </div>

  {#if counts.error && !app.selected.length}
    <div class="shrink-0 border-t-[2.5px] border-ink p-2.5 flex flex-col gap-2 slidein">
      <div class="mono font-semibold text-[12px] text-red">{t('pages.errors', { n: pad(counts.error) })}</div>
      <button class="btn btn-sm" disabled={app.jobActive} onclick={() => app.run('clean', 'errors')}><RefreshCw size={13} /> {t('pages.retry')}</button>
    </div>
  {/if}
  {#if app.selected.length}
    <div class="shrink-0 border-t-[2.5px] border-ink p-2.5 flex flex-col gap-2 slidein">
      <div class="flex items-center"><span class="mono font-semibold text-[12px] text-blue">{t('pages.selected', { n: pad(app.selected.length) })}</span><div class="flex-1"></div>
        <button class="btn btn-sm btn-icon btn-bare !h-6 !w-6" aria-label={t('pages.clear_sel')} onclick={() => (app.selected = [])}><X size={14} /></button></div>
      <button class="btn btn-sm btn-ink" disabled={app.jobActive} onclick={() => app.run('clean', 'selection')}>{t('pages.clean_sel')}</button>
      <button class="btn btn-sm" onclick={() => app.exportChapter(true)}>{t('pages.export_sel')}</button>
      <button class="btn btn-sm btn-danger" onclick={() => app.deleteSelected()}><Trash2 size={13} /> {t('pages.remove_sel')}</button>
    </div>
  {/if}
</aside>

{#if drag}
  <span class="tag tag-ink fixed z-[250] pointer-events-none" style="left:{drag.x + 14}px; top:{drag.y + 10}px">{drag.label}</span>
{/if}
