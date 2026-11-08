<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from './lib/store.svelte';
  import { onDragDrop } from './lib/host';
  import TitleBar from './views/TitleBar.svelte';
  import PagesPanel from './views/PagesPanel.svelte';
  import Desk from './views/Desk.svelte';
  import Inspector from './views/Inspector.svelte';
  import Home from './views/Home.svelte';
  import Preferences from './views/Preferences.svelte';

  onMount(() => {
    app.init();
    let un: (() => void) | undefined;
    onDragDrop((p) => {
      if (p.type === 'enter' || p.type === 'over') app.dragging = true;
      else if (p.type === 'leave') app.dragging = false;
      else if (p.type === 'drop') { app.dragging = false; app.quick(p.paths); }
    }).then((f) => (un = f));
    return () => un?.();
  });
  $effect(() => { app.pagesOpen; app.inspectorOpen; app.savePanels(); });

  function onKey(e: KeyboardEvent) {
    const el = e.target as HTMLElement;
    if (el?.closest?.('input,textarea,select,[contenteditable]')) return;
    const k = e.key.toLowerCase(), ctrl = e.ctrlKey || e.metaKey;
    if (ctrl) {
      if (k === 'z') { e.shiftKey ? app.cv?.redo() : app.cv?.undo(); e.preventDefault(); }
      else if (k === 'y') { app.cv?.redo(); e.preventDefault(); }
      else if (k === 'o') { app.dialog = 'start'; e.preventDefault(); }
      else if (k === 'e') { app.exportChapter(false); e.preventDefault(); }
      else if (k === ',') { app.dialog = 'prefs'; e.preventDefault(); }
      else if (k === '0') { app.cv?.fit('page'); e.preventDefault(); }
      else if (k === '1') { app.cv?.fit('width'); e.preventDefault(); }
      else if (k === '2') { app.cv?.zoomTo(1); e.preventDefault(); }
      else if (k === '[') { app.pagesOpen = !app.pagesOpen; e.preventDefault(); }
      else if (k === 'enter') { app.run('clean', e.shiftKey ? 'chapter' : 'page'); e.preventDefault(); }
      else if (k === 'a') { app.selected = app.pages.map((p) => p.id); e.preventDefault(); }
      return;
    }
    if (app.dialog) return;
    if (k === 'b') app.tool = 'brush';
    else if (k === 'e') app.tool = 'eraser';
    else if (k === 'h') app.tool = 'hand';
    else if (k === 'z') app.tool = 'zoom';
    else if (k === 'l') app.tool = 'lasso';
    else if (k === 'm') app.toggleMask();
    else if (k === 'c') { if (app.hasClean) app.compare = !app.compare; }
    else if (k === 'f') app.cv?.fit('page');
    else if (k === '[') app.brushSize = Math.max(2, Math.round(app.brushSize * 0.8));
    else if (k === ']') app.brushSize = Math.min(400, Math.round(app.brushSize * 1.25));
    else if (k === 'arrowright' || k === 'pagedown') { app.go(app.index + 1); e.preventDefault(); }
    else if (k === 'arrowleft' || k === 'pageup') { app.go(app.index - 1); e.preventDefault(); }
    else if (k === 'tab') { app.inspectorOpen = !app.inspectorOpen; e.preventDefault(); }
    else if (k === 'escape') app.selected = [];
  }
  const showHome = $derived(app.dialog === 'start' || !app.project);
  const cols = $derived(`${app.pagesOpen ? '196px ' : ''}minmax(0,1fr)${app.inspectorOpen ? ' 312px' : ''}`);
</script>

<svelte:window onkeydown={onKey} />

<!-- pencere = manga sayfası: kareler + oluk -->
<div class="h-full p-[10px] grid gap-[10px]" style="grid-template-rows:52px minmax(0,1fr); grid-template-columns:{cols}">
  <TitleBar />
  {#if app.pagesOpen}<PagesPanel />{/if}
  <Desk />
  {#if app.inspectorOpen}<Inspector />{/if}
</div>

{#if app.importProg}
  <div class="fixed bottom-6 left-1/2 -translate-x-1/2 z-[150] koma shadow-[5px_5px_0_var(--color-ink)] h-11 px-4 flex items-center gap-3 slidein">
    <span class="font-semibold">İçe aktarılıyor</span><span class="mono">{app.importProg.done}/{app.importProg.total}</span>
    <span class="w-40 h-[11px] border-[1.5px] border-ink relative"><span class="tone absolute inset-y-0 left-0" style="width:{(app.importProg.done / app.importProg.total) * 100}%"></span></span>
  </div>
{/if}

{#if showHome}<Home />{/if}
{#if app.dialog === 'prefs'}<Preferences />{/if}

{#if app.dragging}
  <div class="fixed inset-0 z-[200] pointer-events-none p-4 bg-paper/70 fade-in">
    <div class="w-full h-full border-[3px] border-dashed border-ink grid place-items-center">
      <div class="text-center">
        <div class="disp text-[64px] leading-none">Bırak</div>
        <div class="mono mt-2">sayfalar ya da klasör → hızlı temizlik</div>
      </div>
    </div>
  </div>
{/if}

<div class="fixed bottom-20 left-1/2 -translate-x-1/2 flex flex-col items-center gap-2.5 z-[160] pointer-events-none">
  {#each app.toasts as t (t.id)}
    <div class="slidein pointer-events-auto flex items-center gap-2.5 pl-2 pr-4 py-2 max-w-[560px] bg-sheet border-2 {t.kind === 'err' ? 'border-red shadow-[4px_4px_0_var(--color-red)]' : 'border-ink shadow-[4px_4px_0_var(--color-ink)]'}">
      <span class="tag {t.kind === 'err' ? 'tag-red' : t.kind === 'ok' ? 'tag-ink' : ''}">{t.kind === 'err' ? 'HATA' : t.kind === 'ok' ? 'TAMAM' : 'NOT'}</span>
      <span class="select-text font-medium">{t.text}</span>
    </div>
  {/each}
</div>
