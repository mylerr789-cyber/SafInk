<script lang="ts" module>
  export interface Opt<T> { value: T; label: string; hint?: string; group?: string; disabled?: boolean; mark?: 'ok' | 'off' }
</script>

<script lang="ts" generics="V extends string">
  import { ChevronDown, Check } from 'lucide-svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    value: V; options: Opt<V>[]; placeholder?: string; onchange?: (v: V) => void; width?: string; compact?: boolean;
    disabled?: boolean; footer?: Snippet<[() => void]>; align?: 'left' | 'right'; menuWidth?: string; aside?: string;
  }
  let { value = $bindable(), options, placeholder = 'Seçin', onchange, width = '', compact = false, disabled = false, footer, align = 'left', menuWidth = '', aside = '' }: Props = $props();

  let open = $state(false);
  let hl = $state(-1);
  let root: HTMLDivElement;
  let pos = $state('');
  // liste sabit konumda açılır: kaydırılan/kırpan panellerin dışına taşabilir
  function place() {
    const r = root.getBoundingClientRect();
    const w = menuWidth ? parseFloat(menuWidth) : r.width;
    const left = align === 'right' ? Math.max(8, r.right - w) : Math.min(r.left, window.innerWidth - w - 8);
    const below = window.innerHeight - r.bottom - 12, up = below < 260 && r.top > below;
    const maxH = Math.min(380, (up ? r.top : below) - 12);
    pos = `position:fixed; left:${left}px; width:${w}px; max-height:${maxH}px; ${up ? `bottom:${window.innerHeight - r.top + 5}px` : `top:${r.bottom + 5}px`}`;
  }
  const cur = $derived(options.find((o) => o.value === value));

  function choose(o: Opt<V>) {
    if (o.disabled) return;
    open = false;
    if (o.value !== value) { value = o.value; onchange?.(o.value); }
  }
  function toggle() { if (disabled) return; if (!open) place(); open = !open; hl = options.findIndex((o) => o.value === value); }
  function key(e: KeyboardEvent) {
    if (!open) { if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown') { toggle(); e.preventDefault(); } return; }
    if (e.key === 'Escape') { open = false; e.stopPropagation(); }
    else if (e.key === 'ArrowDown') { hl = Math.min(options.length - 1, hl + 1); e.preventDefault(); }
    else if (e.key === 'ArrowUp') { hl = Math.max(0, hl - 1); e.preventDefault(); }
    else if (e.key === 'Enter' && options[hl]) { choose(options[hl]); e.preventDefault(); }
  }
</script>

<svelte:window onpointerdown={(e) => { if (open && !(e.target instanceof Node && root.contains(e.target))) open = false; }} onkeydown={(e) => { if (open && e.key === 'Escape') open = false; }} onresize={() => (open = false)} />

<div class="relative" bind:this={root} style={width ? `width:${width}` : ''}>
  <button type="button" class="w-full flex items-center gap-2.5 bg-sheet border-[1.5px] border-ink text-left {compact ? 'h-[28px] px-2' : 'h-[36px] px-2.5'} {open ? 'shadow-[3px_3px_0_var(--color-ink)]' : 'hover:shadow-[3px_3px_0_var(--color-ink)]'} disabled:opacity-35"
    {disabled} onclick={toggle} onkeydown={key} aria-haspopup="listbox" aria-expanded={open}>
    {#if cur?.mark}<span class="w-2 h-2 rounded-full shrink-0 {cur.mark === 'ok' ? 'bg-ink' : 'border-[1.5px] border-ink3'}"></span>{/if}
    <span class="flex-1 truncate font-semibold {cur ? '' : 'muted font-normal'}">{cur?.label ?? placeholder}</span>
    {#if aside}<span class="mono text-[11px] muted shrink-0">{aside}</span>{/if}
    <ChevronDown size={14} strokeWidth={2.25} class="shrink-0 transition-transform {open ? 'rotate-180' : ''}" />
  </button>
  {#if open}
    <div class="pop !fixed overflow-y-auto z-[300]" style={pos} role="listbox">
      {#each options as o, i}
        {#if o.group && (i === 0 || options[i - 1].group !== o.group)}<div class="pop-head">{o.group}</div>{/if}
        <button type="button" class="pop-item {i === hl ? 'hl' : ''}" disabled={o.disabled} onclick={() => choose(o)} onmouseenter={() => (hl = i)} role="option" aria-selected={o.value === value}>
          {#if o.mark}<span class="w-2 h-2 rounded-full shrink-0 {o.mark === 'ok' ? 'bg-current' : 'border-[1.5px] border-current opacity-50'}"></span>{/if}
          <span class="flex-1 min-w-0 py-1.5">
            <span class="block truncate font-medium">{o.label}</span>
            {#if o.hint}<span class="block truncate mono text-[10.5px] muted">{o.hint}</span>{/if}
          </span>
          <Check size={14} strokeWidth={2.5} class="shrink-0 {o.value === value ? '' : 'invisible'}" />
        </button>
      {/each}
      {#if footer}<div class="pop-sep"></div>{@render footer(() => (open = false))}{/if}
    </div>
  {/if}
</div>
