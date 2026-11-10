<script lang="ts">
  import { Upload, Pause, Play, X, PanelLeft, PanelRight, Pencil } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  import { api } from '../lib/api';
  import { t } from '../lib/i18n/index.svelte';
  import { importChapterFolder, importPages, resetPageEdits } from '../lib/actions';
  import Logo from '../ui/Logo.svelte';

  interface Item { label?: string; kbd?: string; run?: () => void; sep?: boolean; disabled?: boolean }
  const has = $derived(!!app.project);
  const busy = $derived(app.jobActive);
  const menus = $derived<{ id: string; name: string; items: Item[] }[]>([
    { id: 'file', name: t('menu.file'), items: [
      { label: t('file.library'), kbd: 'Ctrl O', run: () => (app.dialog = 'start') },
      { label: t('file.import_chapter'), run: () => importChapterFolder() },
      { label: t('file.add_pages'), run: importPages, disabled: !has },
      { sep: true },
      { label: t('file.export_chapter'), kbd: 'Ctrl E', run: () => app.exportChapter(false), disabled: !has },
      { label: t('file.export_selected'), run: () => app.exportChapter(true), disabled: !has || !app.selected.length },
      { sep: true },
      { label: t('file.prefs'), kbd: 'Ctrl ,', run: () => { app.prefsTab = 'clean'; app.dialog = 'prefs'; } },
      { label: t('common.models_dots'), run: () => { app.prefsTab = 'models'; app.dialog = 'prefs'; } },
    ] },
    { id: 'edit', name: t('menu.edit'), items: [
      { label: t('edit.undo'), kbd: 'Ctrl Z', run: () => app.cv?.undo(), disabled: !app.canUndo },
      { label: t('edit.redo'), kbd: 'Ctrl Y', run: () => app.cv?.redo(), disabled: !app.canRedo },
      { sep: true },
      { label: t('edit.select_all'), kbd: 'Ctrl A', run: () => (app.selected = app.pages.map((p) => p.id)), disabled: !has },
      { label: t('edit.reset_edits'), run: resetPageEdits, disabled: !has },
    ] },
    { id: 'view', name: t('menu.view'), items: [
      { label: t('view.fit_page'), kbd: 'Ctrl 0', run: () => app.cv?.fit('page') },
      { label: t('view.fit_width'), kbd: 'Ctrl 1', run: () => app.cv?.fit('width') },
      { label: t('view.actual'), kbd: 'Ctrl 2', run: () => app.cv?.zoomTo(1) },
      { sep: true },
      { label: t('view.marks'), kbd: 'M', run: () => app.toggleMask() },
      { label: t('view.compare'), kbd: 'C', run: () => (app.compare = !app.compare), disabled: !app.hasClean },
      { sep: true },
      { label: t('view.pages_panel'), kbd: 'Ctrl [', run: () => (app.pagesOpen = !app.pagesOpen) },
      { label: t('view.inspector'), kbd: 'Tab', run: () => (app.inspectorOpen = !app.inspectorOpen) },
    ] },
    { id: 'run', name: t('menu.run'), items: [
      { label: t('run.fetch_page'), run: () => app.run('fetch', 'page'), disabled: !has || busy },
      { label: t('run.fetch_chapter'), run: () => app.run('fetch', 'chapter'), disabled: !has || busy },
      { sep: true },
      { label: t('run.clean_page'), kbd: 'Ctrl ↵', run: () => app.run('clean', 'page'), disabled: !has || busy },
      { label: t('run.clean_selected'), run: () => app.run('clean', 'selection'), disabled: !has || busy || !app.selected.length },
      { label: t('run.clean_chapter'), kbd: 'Ctrl ⇧ ↵', run: () => app.run('clean', 'chapter'), disabled: !has || busy },
      { sep: true },
      { label: t('run.retry_errors'), run: () => app.run('clean', 'errors'), disabled: !has || busy || !app.pages.some((p) => app.statusOf(p) === 'error') },
      { label: t('run.stop'), run: () => api.cancelJob(), disabled: !busy },
    ] },
    { id: 'help', name: t('menu.help'), items: [
      { label: t('help.shortcuts'), kbd: '?', run: () => (app.help = true) },
      { label: t('common.models_dots'), run: () => { app.prefsTab = 'models'; app.dialog = 'prefs'; } },
    ] },
  ]);
  let jobsOpen = $state(false);
  let renaming = $state(false);
  let nameDraft = $state('');
  function startRename() { if (!app.project || app.project.quick) return; nameDraft = app.project.name; renaming = true; }
  async function commitRename() {
    if (!renaming || !app.project) return;
    renaming = false;
    if (nameDraft.trim() && nameDraft.trim() !== app.project.name) await app.renameChapter(app.project.chapterId, nameDraft);
  }
  const j = $derived(app.job);
  const doneN = $derived(j ? (j.mode === 'clean' ? j.cleaned : j.fetched) : 0);
  const frac = $derived(j && j.total ? Math.min(1, (doneN + j.failed) / j.total) : 0);
  const pad = (n: number) => String(n).padStart(2, '0');
</script>

<svelte:window onpointerdown={(e) => { if (!(e.target as HTMLElement).closest('[data-keep]')) { app.menu = null; jobsOpen = false; } }} />

<header class="koma col-span-full flex items-stretch z-[70] !overflow-visible">
  <button class="flex items-center px-4 bg-ink text-paper border-r-[2.5px] border-ink" onclick={() => (app.dialog = 'start')} data-tip="{t('bar.library')}  Ctrl O">
    <Logo size={26} />
  </button>

  <nav class="flex items-center px-2 gap-0.5 border-r-[2.5px] border-ink" data-keep>
    {#each menus as menu (menu.id)}
      <div class="relative">
        <button class="h-8 px-2.5 font-medium {app.menu === menu.id ? 'bg-ink text-paper' : 'hover:bg-ink hover:text-paper'}"
          onclick={() => (app.menu = app.menu === menu.id ? null : menu.id)}
          onmouseenter={() => { if (app.menu && app.menu !== menu.id) app.menu = menu.id; }}>{menu.name}</button>
        {#if app.menu === menu.id}
          <div class="pop top-[calc(100%+9px)] left-0 w-[270px]" role="menu">
            {#each menu.items as it}
              {#if it.sep}<div class="pop-sep"></div>
              {:else}
                <button class="pop-item" disabled={it.disabled} onclick={() => { app.menu = null; it.run?.(); }}>
                  <span class="flex-1">{it.label}</span>{#if it.kbd}<span class="mono text-[11px] muted">{it.kbd}</span>{/if}
                </button>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </nav>

  <div class="tone-light flex-1 min-w-0 flex items-center gap-3 px-4" data-tauri-drag-region>
    {#if app.project}
      <button class="disp text-[22px] leading-none bg-sheet px-1.5 py-1 truncate min-w-0 hover:bg-ink hover:text-paper" onclick={() => (app.dialog = 'start')}>
        {app.project.quick ? t('bar.quick') : app.project.manga}
      </button>
      {#if !app.project.quick}
        {#if renaming}
          <!-- svelte-ignore a11y_autofocus -->
          <input class="field !h-[26px] !w-[170px] !px-2 !text-[12.5px] font-semibold shrink-0" bind:value={nameDraft} autofocus onfocus={(e) => (e.currentTarget as HTMLInputElement).select()} onblur={commitRename}
            onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Enter') commitRename(); if (e.key === 'Escape') renaming = false; }} />
        {:else}
          <button class="tag bg-sheet !h-[24px] !px-2 !text-[12.5px] shrink-0 max-w-[150px] truncate hover:bg-ink hover:text-paper" onclick={startRename} data-tip={t('bar.rename_chapter')}>{app.project.name}<Pencil size={11} /></button>
        {/if}
      {/if}
      {#if app.pages.length}<span class="ml-auto mono font-semibold text-[15px] bg-sheet px-2 shrink-0">P.{pad(app.index + 1)} / {pad(app.pages.length)}</span>{/if}
    {/if}
  </div>

  <div class="relative flex border-l-[2.5px] border-ink" data-keep>
    <button class="w-[260px] flex items-center gap-2.5 px-4 hover:bg-desk" onclick={() => (jobsOpen = !jobsOpen)}>
      {#if app.jobActive && j}
        <span class="mono font-semibold">{doneN}/{j.total}</span>
        <span class="flex-1 h-[11px] border-[1.5px] border-ink bg-sheet relative"><span class="tone absolute inset-y-0 left-0 transition-[width] duration-300" style="width:{frac * 100}%"></span></span>
        <span class="font-medium">{j.state === 'paused' ? t('job.paused') : j.mode === 'clean' ? t('job.cleaning') : t('job.fetching')}</span>
      {:else if j && j.state === 'error'}
        <span class="tag tag-red">{t('toast.err')}</span><span class="truncate text-[12px]">{j.message}</span>
      {:else if j && j.state === 'done'}
        <span class="tag tag-ink">{t('job.done_tag')}</span><span class="text-[12px] muted">{t('job.done_n', { n: j.total })}</span>
      {:else}
        <span class="mono text-[12px] muted">{t('job.none')}</span>
      {/if}
    </button>
    {#if jobsOpen}
      <div class="pop top-[calc(100%+9px)] right-0 w-[300px] !p-3">
        {#if app.jobActive && j}
          <div class="flex items-baseline mb-2"><span class="disp text-[18px]">{j.mode === 'clean' ? t('job.title_clean') : t('job.title_fetch')}</span><span class="ml-auto mono font-semibold">{Math.round(frac * 100)}%</span></div>
          <div class="grid grid-cols-3 border-[1.5px] border-ink mb-2.5 text-center">
            <div class="py-1.5"><div class="mono font-semibold text-[15px]">{j.fetched}</div><div class="eyebrow">{t('job.col_mask')}</div></div>
            <div class="py-1.5 border-x-[1.5px] border-ink"><div class="mono font-semibold text-[15px]">{j.cleaned}</div><div class="eyebrow">{t('job.col_clean')}</div></div>
            <div class="py-1.5 {j.failed ? 'text-red' : ''}"><div class="mono font-semibold text-[15px]">{j.failed}</div><div class="eyebrow">{t('job.col_error')}</div></div>
          </div>
          {#if j.message}<div class="mono text-[11px] muted truncate mb-1">{j.message}</div>{/if}
          {#if j.wait_secs > 0}<div class="text-[12px] text-red mb-1">{t('job.rate_wait', { n: j.wait_secs.toFixed(0) })}</div>{/if}
          <div class="flex gap-2 mt-2">
            <button class="btn btn-sm flex-1" onclick={() => api.pauseJob(j.state !== 'paused')}>{#if j.state === 'paused'}<Play size={13} /> {t('job.resume')}{:else}<Pause size={13} /> {t('job.pause')}{/if}</button>
            <button class="btn btn-sm btn-danger flex-1" onclick={() => api.cancelJob()}><X size={13} /> {t('job.stop')}</button>
          </div>
        {:else}
          <div class="disp text-[18px] mb-1">{t('job.jobs')}</div>
          <div class="text-[12.5px] leading-5">{#if j && j.state === 'error'}<span class="text-red">{j.message}</span>{:else if j && j.state === 'done'}{t('job.last_done', { n: j.total })}{:else}{t('job.idle')}{/if}</div>
        {/if}
      </div>
    {/if}
  </div>

  <div class="flex items-center gap-1 px-2 border-l-[2.5px] border-ink">
    <button class="btn btn-sm btn-icon btn-bare {app.pagesOpen ? '' : 'opacity-40'}" onclick={() => (app.pagesOpen = !app.pagesOpen)} data-tip="{t('view.pages_panel')}  Ctrl [" aria-label={t('view.pages_panel')}><PanelLeft size={16} /></button>
    <button class="btn btn-sm btn-icon btn-bare {app.inspectorOpen ? '' : 'opacity-40'}" onclick={() => (app.inspectorOpen = !app.inspectorOpen)} data-tip="{t('view.inspector')}  Tab" aria-label={t('view.inspector')}><PanelRight size={16} /></button>
  </div>
  <button class="flex items-center gap-2 px-5 bg-ink text-paper font-bold border-l-[2.5px] border-ink hover:bg-[#2b2b2b] disabled:opacity-60" disabled={!has || app.exporting} onclick={() => app.exportChapter(false)} data-tip="{t('bar.export_tip')}  Ctrl E">
    <Upload size={16} strokeWidth={2.25} class={app.exporting ? 'animate-pulse' : ''} /> {app.exporting ? t('bar.exporting') : t('bar.export')}
  </button>
</header>
