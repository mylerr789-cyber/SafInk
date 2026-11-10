<script lang="ts">
  import { Paintbrush, Eraser, Lasso, Hand, ZoomIn } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  import { t } from '../lib/i18n/index.svelte';
  import Scrub from '../ui/Scrub.svelte';
  import Segmented from '../ui/Segmented.svelte';

  const icons = { brush: Paintbrush, eraser: Eraser, lasso: Lasso, hand: Hand, zoom: ZoomIn } as const;
  const m = $derived({ icon: icons[app.tool], name: t(`tool.${app.tool}`), hint: t(`tool.${app.tool}_hint`) });
  const cmp = $derived(!app.compare ? 'mask' : app.split ? 'split' : 'result');
  function setCmp(v: string) { app.compare = v !== 'mask'; app.split = v === 'split'; }
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
      <Scrub bind:value={app.brushSize} min={2} max={400} label={t('tool.size')} unit="px" width="180px" compact tip={t('tool.size_tip')} />
    </div>
  {:else if app.tool === 'lasso'}
    <div class="flex items-center px-2 border-r-[1.5px] border-ink">
      <Segmented bind:value={app.lassoMode} compact options={[{ value: 'add', label: t('tool.lasso_add') }, { value: 'remove', label: t('tool.lasso_remove') }]} />
    </div>
  {:else}
    <div class="flex items-center px-2 gap-1.5 border-r-[1.5px] border-ink">
      <button class="btn btn-sm" onclick={() => app.cv?.fit('page')}>{t('view.fit_page')}</button>
      <button class="btn btn-sm" onclick={() => app.cv?.fit('width')}>{t('tool.width')}</button>
      <button class="btn btn-sm" onclick={() => app.cv?.zoomTo(1)}>{t('tool.pct100')}</button>
    </div>
  {/if}
  <div class="flex items-center px-2 border-r-[1.5px] border-ink">
    <Scrub bind:value={maskOp} min={0.1} max={1} step={0.05} percent label={t('tool.marks')} width="150px" compact onchange={setOp} tip={t('tool.marks_tip')} />
  </div>
  {#if app.hasClean}
    <div class="flex items-center px-2 border-r-[1.5px] border-ink">
      <Segmented value={cmp} onchange={setCmp} compact options={[{ value: 'mask', label: t('tool.cmp_mask'), tip: 'C' }, { value: 'split', label: t('tool.cmp_split'), tip: t('tool.cmp_split_tip') }, { value: 'result', label: t('tool.cmp_result'), tip: t('tool.cmp_result_tip') }]} />
    </div>
  {/if}
  <div class="flex-1 min-w-0 flex items-center justify-end px-3.5 mono text-[11px] muted truncate max-[1500px]:hidden">{m.hint}</div>
</div>
