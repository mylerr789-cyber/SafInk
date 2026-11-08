<script lang="ts" generics="V extends string | boolean">
  interface Props { value: V; options: { value: V; label?: string; icon?: any; tip?: string; disabled?: boolean }[]; onchange?: (v: V) => void; compact?: boolean; full?: boolean }
  let { value = $bindable(), options, onchange, compact = false, full = false }: Props = $props();
</script>

<div class="inline-flex border-[1.5px] border-ink bg-sheet {full ? 'w-full' : ''}">
  {#each options as o, i}
    <button type="button" data-tip={o.tip}
      class="flex-1 inline-flex items-center justify-center gap-1.5 font-semibold whitespace-nowrap transition-colors
        {compact ? 'h-[25px] px-2.5 text-[12px]' : 'h-[31px] px-3 text-[12.5px]'}
        {i ? 'border-l-[1.5px] border-ink' : ''}
        {o.value === value ? 'bg-ink text-paper' : 'hover:bg-desk'} disabled:opacity-35"
      disabled={o.disabled} onclick={() => { if (o.value !== value) { value = o.value; onchange?.(o.value); } }}>
      {#if o.icon}<o.icon size={14} strokeWidth={2} />{/if}{#if o.label}<span>{o.label}</span>{/if}
    </button>
  {/each}
</div>
