<script lang="ts">
  // Cetvel alanı: yatay sürükleyerek değiştir (Shift = ince), tıklayıp yaz. Değer cetvel üzerindeki kırmızı kalem çizgisidir.
  interface Props {
    value: number; min: number; max: number; step?: number; label?: string; unit?: string; decimals?: number;
    width?: string; compact?: boolean; disabled?: boolean; tip?: string; percent?: boolean; bare?: boolean; onchange?: (v: number) => void;
  }
  let { value = $bindable(), min, max, step = 1, label = '', unit = '', decimals = 0, width = '', compact = false, disabled = false, tip, percent = false, bare = false, onchange }: Props = $props();

  let editing = $state(false);
  let text = $state('');
  let el: HTMLDivElement;
  let input = $state<HTMLInputElement>();
  let drag: { x: number; v: number; moved: boolean } | null = null;

  const frac = $derived(Math.max(0, Math.min(1, (value - min) / (max - min))));
  const shown = $derived(percent ? String(Math.round(value * 100)) : value.toFixed(decimals));

  function clamp(v: number) {
    const s = Math.round((v - min) / step) * step + min;
    return +Math.max(min, Math.min(max, s)).toFixed(6);
  }
  function set(v: number) { const c = clamp(v); if (c !== value) { value = c; onchange?.(c); } }

  function down(e: PointerEvent) {
    if (disabled || editing || e.button !== 0) return;
    el.setPointerCapture(e.pointerId);
    drag = { x: e.clientX, v: value, moved: false };
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    if (!drag.moved && Math.abs(dx) < 3) return;
    drag.moved = true;
    set(drag.v + (dx / (el.clientWidth || 120)) * (max - min) * (e.shiftKey ? 0.1 : 1));
  }
  async function up() {
    if (!drag) return;
    const clicked = !drag.moved; drag = null;
    if (clicked) { text = shown; editing = true; await Promise.resolve(); input?.focus(); input?.select(); }
  }
  function commit() {
    const v = parseFloat(text.replace(',', '.'));
    if (!isNaN(v)) set(percent ? v / 100 : v);
    editing = false;
  }
  function key(e: KeyboardEvent) {
    if (e.key === 'Enter') commit();
    else if (e.key === 'Escape') editing = false;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={el}
  class="relative flex items-center {bare ? 'bare h-[26px] px-1 text-[12px]' : `ruler bg-sheet border-[1.5px] border-ink ${compact ? 'h-[28px] px-2 text-[12px]' : 'h-[34px] px-2.5'}`} {disabled ? 'opacity-35' : 'cursor-ew-resize'}"
  style={width ? `width:${width}` : ''}
  data-tip={tip}
  onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up}
>
  {#if bare}
    <div class="absolute left-1 right-1 bottom-[3px] border-b-[1.5px] border-dotted border-ink3 pointer-events-none"></div>
    <div class="absolute left-1 bottom-[2px] h-[3px] bg-red pointer-events-none" style="width:calc(({frac}) * (100% - 8px))"></div>
  {:else}
    <div class="absolute left-0 bottom-0 h-[6px] bg-red/20 pointer-events-none" style="width:{frac * 100}%"></div>
    <div class="ticks absolute inset-x-0 bottom-0 h-[6px] pointer-events-none"></div>
    <div class="absolute bottom-[-1px] w-[3px] h-[14px] bg-red pointer-events-none" style="left:calc({frac * 100}% - {frac * 3}px)"></div>
  {/if}
  {#if editing}
    <input bind:this={input} bind:value={text} onblur={commit} onkeydown={key}
      class="relative w-full bg-transparent outline-none text-right mono font-semibold select-text" />
  {:else}
    {#if label}<span class="relative truncate pr-2 font-medium">{label}</span>{/if}
    <span class="relative ml-auto mono font-semibold whitespace-nowrap">{shown}{#if unit || percent}<span class="muted font-medium">{percent ? '%' : unit}</span>{/if}</span>
  {/if}
</div>

<style>
  .ticks { background: repeating-linear-gradient(90deg, var(--color-ink) 0 1px, transparent 1px 8px); opacity: .55; }
  .ruler:hover:not(.opacity-35) { box-shadow: 3px 3px 0 var(--color-ink); }
  .bare:hover { background: var(--color-desk); }
</style>
