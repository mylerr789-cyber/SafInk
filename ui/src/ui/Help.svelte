<script lang="ts">
  import { X } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  import { t } from '../lib/i18n/index.svelte';
  const groups = $derived<{ title: string; rows: [string, string][] }[]>([
    { title: t('help.g_tools'), rows: [['B', t('help.brush')], ['E', t('help.eraser')], ['L', t('help.lasso')], ['H', t('help.hand')], ['Z', t('help.zoom')], ['[  ]', t('help.brush_size')], [t('help.k_ctrl_wheel'), t('help.brush_size')]] },
    { title: t('help.g_view'), rows: [['M', t('help.marks')], ['C', t('help.compare')], [t('help.k_hold_bs'), t('help.show_orig')], ['F · Ctrl 0', t('help.fit_page')], ['Ctrl 1', t('help.fit_width')], ['Ctrl 2', t('help.actual')], [t('help.k_space_drag'), t('help.pan')]] },
    { title: t('help.g_pages'), rows: [['← →', t('help.prev_next')], ['Ctrl A', t('help.select_all')], [t('help.k_multi'), t('help.multi')], ['Del', t('help.remove_page')], ['Esc', t('help.clear_sel')], [t('help.k_drag'), t('help.reorder')], ['Alt ↑ ↓', t('help.move_page')], ['Ctrl [', t('help.pages_panel')], ['Tab', t('help.inspector')]] },
    { title: t('help.g_work'), rows: [['Ctrl ↵', t('help.clean_page')], ['Ctrl ⇧ ↵', t('help.clean_chapter')], ['Ctrl Z · Ctrl Y', t('help.undo_redo')], ['Ctrl E', t('help.export')], ['Ctrl O', t('help.library')], ['Ctrl ,', t('help.prefs')], ['?', t('help.this_window')]] },
  ]);
  const close = () => (app.help = false);
</script>

<svelte:window onkeydown={(e) => { if (app.help && e.key === 'Escape') close(); }} />
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-[85] bg-paper/75 grid place-items-center fade-in" onclick={close}>
  <div class="slidein w-[860px] max-w-[94vw] max-h-[92vh] koma shadow-[8px_8px_0_var(--color-ink)] flex flex-col overflow-hidden" onclick={(e) => e.stopPropagation()}>
    <div class="h-[56px] shrink-0 bg-ink text-paper flex items-center px-5">
      <span class="disp text-[22px] leading-none">{t('help.shortcuts')}</span><div class="flex-1"></div>
      <button class="btn btn-icon btn-sm btn-bare !text-paper hover:!bg-paper hover:!text-ink" aria-label={t('common.close')} onclick={close}><X size={16} strokeWidth={2.5} /></button>
    </div>
    <div class="overflow-y-auto grid grid-cols-2 gap-[1.5px] bg-ink">
      {#each groups as g}
        <section class="bg-sheet px-5 py-4">
          <div class="disp text-[17px] leading-none mb-2.5">{g.title}</div>
          {#each g.rows as [k, desc]}
            <div class="flex items-center gap-3 h-[28px] border-b border-dashed border-ink/20"><span class="kbd shrink-0">{k}</span><span class="ml-auto text-[12.5px]">{desc}</span></div>
          {/each}
        </section>
      {/each}
    </div>
  </div>
</div>
