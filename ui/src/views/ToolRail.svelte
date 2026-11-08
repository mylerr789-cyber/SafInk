<script lang="ts">
  import { Hand, ZoomIn, Paintbrush, Eraser, Lasso, Eye, EyeOff, Undo2, Redo2 } from 'lucide-svelte';
  import { app, type Tool } from '../lib/store.svelte';

  const groups: { id: Tool; icon: any; tip: string; k: string }[][] = [
    [
      { id: 'brush', icon: Paintbrush, tip: 'Fırça — maskeye ekle', k: 'B' },
      { id: 'eraser', icon: Eraser, tip: 'Silgi — maskeden çıkar', k: 'E' },
      { id: 'lasso', icon: Lasso, tip: 'Kement — alan doldur/sil', k: 'L' },
    ],
    [
      { id: 'hand', icon: Hand, tip: 'El — kaydır (Boşluk basılı)', k: 'H' },
      { id: 'zoom', icon: ZoomIn, tip: 'Büyüteç (Alt: uzaklaş)', k: 'Z' },
    ],
  ];
  const maskOn = $derived(app.layers.api.vis || app.layers.edit.vis);
</script>

{#snippet sep()}<div class="h-[7px] tone-light border-b-[1.5px] border-ink"></div>{/snippet}

<div class="absolute left-3.5 top-1/2 -translate-y-1/2 z-20 flex flex-col bg-sheet border-[2.5px] border-ink shadow-[5px_5px_0_var(--color-ink)]">
  {#each groups as g, gi}
    {#if gi}{@render sep()}{/if}
    {#each g as t}
      <button class="relative w-[46px] h-[46px] grid place-items-center border-b-[1.5px] border-ink {app.tool === t.id ? 'bg-ink text-paper' : 'hover:bg-desk'}"
        data-tip="{t.tip}  {t.k}" data-tip-side="right" aria-label={t.tip} onclick={() => (app.tool = t.id)}>
        <t.icon size={19} strokeWidth={2} />
        <span class="absolute right-[3px] bottom-[1px] mono text-[8.5px] font-semibold opacity-55">{t.k}</span>
      </button>
    {/each}
  {/each}
  {@render sep()}
  <button class="relative w-[46px] h-[46px] grid place-items-center border-b-[1.5px] border-ink hover:bg-desk" data-tip={'Kalem izlerini göster/gizle  M'} data-tip-side="right" aria-label="Maske" onclick={() => app.toggleMask()}>
    {#if maskOn}<Eye size={19} strokeWidth={2} />{:else}<EyeOff size={19} strokeWidth={2} class="text-ink3" />{/if}
    <span class="absolute right-[3px] bottom-[1px] mono text-[8.5px] font-semibold opacity-55">M</span>
  </button>
  <div class="flex">
    <button class="w-[23px] h-[34px] grid place-items-center border-r-[1.5px] border-ink hover:bg-desk disabled:opacity-25" data-tip="Geri al  Ctrl Z" data-tip-side="right" aria-label="Geri al" disabled={!app.canUndo} onclick={() => app.cv?.undo()}><Undo2 size={14} strokeWidth={2.25} /></button>
    <button class="w-[20px] h-[34px] grid place-items-center hover:bg-desk disabled:opacity-25" data-tip="Yinele  Ctrl Y" data-tip-side="right" aria-label="Yinele" disabled={!app.canRedo} onclick={() => app.cv?.redo()}><Redo2 size={14} strokeWidth={2.25} /></button>
  </div>
</div>
