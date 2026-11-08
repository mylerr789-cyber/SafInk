<script lang="ts">
  import { Paintbrush, Eraser, Lasso, Hand, ZoomIn } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  import Scrub from '../ui/Scrub.svelte';
  import Segmented from '../ui/Segmented.svelte';

  const meta = {
    brush: { icon: Paintbrush, name: 'Fırça', hint: 'mavi kalem: kaçan yazıyı boya' },
    eraser: { icon: Eraser, name: 'Silgi', hint: 'yanlış işaretlenen yeri sil' },
    lasso: { icon: Lasso, name: 'Kement', hint: 'çiz, bırak · Alt: tersi' },
    hand: { icon: Hand, name: 'El', hint: 'sürükle: kaydır' },
    zoom: { icon: ZoomIn, name: 'Büyüteç', hint: 'tıkla · Alt: uzaklaş' },
  } as const;
  const m = $derived(meta[app.tool]);
  const drawing = $derived(app.tool === 'brush' || app.tool === 'eraser');
  let maskOp = $state(app.layers.api.op);
  function setOp(v: number) { app.layers.api.op = v; app.layers.edit.op = Math.min(1, v + 0.1); app.saveOpts(); }
</script>

<div class="absolute inset-x-0 top-0 h-[40px] flex items-stretch bg-sheet border-b-[1.5px] border-ink z-10 whitespace-nowrap">
  <div class="flex items-center gap-2 px-3.5 border-r-[1.5px] border-ink w-[138px]">
    <m.icon size={17} strokeWidth={2} /><span class="disp text-[17px] leading-none">{m.name}</span>
  </div>
  {#if drawing}
    <div class="flex items-center px-2 border-r-[1.5px] border-ink">
      <Scrub bind:value={app.brushSize} min={2} max={400} label="Boyut" unit="px" width="180px" compact tip="[ ve ] ile de değişir" />
    </div>
  {:else if app.tool === 'lasso'}
    <div class="flex items-center px-2 border-r-[1.5px] border-ink">
      <Segmented bind:value={app.lassoMode} compact options={[{ value: 'add', label: 'Maskeye ekle' }, { value: 'remove', label: 'Maskeden sil' }]} />
    </div>
  {:else}
    <div class="flex items-center px-2 gap-1.5 border-r-[1.5px] border-ink">
      <button class="btn btn-sm" onclick={() => app.cv?.fit('page')}>Sayfaya sığdır</button>
      <button class="btn btn-sm" onclick={() => app.cv?.fit('width')}>Genişliğe</button>
      <button class="btn btn-sm" onclick={() => app.cv?.zoomTo(1)}>%100</button>
    </div>
  {/if}
  <div class="flex items-center px-2 border-r-[1.5px] border-ink">
    <Scrub bind:value={maskOp} min={0.1} max={1} step={0.05} percent label="Kalem izi" width="150px" compact onchange={setOp} tip="Maske katmanlarının opaklığı" />
  </div>
  {#if app.hasClean}
    <div class="flex items-center px-2 border-r-[1.5px] border-ink">
      <Segmented bind:value={app.compare} compact options={[{ value: false, label: 'Maske', tip: 'C' }, { value: true, label: 'Sonuç', tip: 'C · \\ basılı: orijinal' }]} />
    </div>
  {/if}
  <div class="flex-1 flex items-center justify-end px-3.5 mono text-[11px] muted truncate">{m.hint}</div>
</div>
