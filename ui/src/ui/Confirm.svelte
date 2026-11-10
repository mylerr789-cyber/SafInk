<script lang="ts">
  import { app } from '../lib/store.svelte';
  function key(e: KeyboardEvent) {
    if (!app.asking) return;
    if (e.key === 'Escape') app.answer(false);
    else if (e.key === 'Enter') app.answer(true);
    else return;
    e.stopImmediatePropagation(); e.preventDefault();
  }
</script>

<svelte:window onkeydowncapture={key} />
{#if app.asking}
  {@const a = app.asking}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-[300] bg-paper/75 grid place-items-center fade-in" onclick={() => app.answer(false)}>
    <div class="slidein koma w-[420px] max-w-[92vw] shadow-[8px_8px_0_var(--color-ink)]" onclick={(e) => e.stopPropagation()} role="alertdialog" tabindex="-1">
      <div class="h-[40px] px-4 flex items-center bg-ink text-paper"><span class="disp text-[18px] leading-none">{a.danger ? 'Emin misin?' : 'Onay'}</span></div>
      <div class="px-4 py-5 text-[13.5px] leading-5">{a.text}</div>
      <div class="flex justify-end gap-3 px-4 pb-4">
        <button class="btn" onclick={() => app.answer(false)}>Vazgeç</button>
        <!-- svelte-ignore a11y_autofocus -->
        <button class="btn print {a.danger ? '!bg-red !text-white !border-ink' : 'btn-ink'}" autofocus onclick={() => app.answer(true)}>{a.ok}</button>
      </div>
    </div>
  </div>
{/if}
