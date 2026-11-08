<script lang="ts">
  let { checked = $bindable(), onchange, disabled = false }: { checked: boolean; onchange?: (v: boolean) => void; disabled?: boolean } = $props();
</script>

<button type="button" role="switch" aria-checked={checked} {disabled} aria-label="Aç/kapat"
  class="relative w-[44px] h-[24px] border-[1.5px] border-ink transition-colors {checked ? 'bg-ink' : 'bg-sheet'} disabled:opacity-35"
  onclick={() => { checked = !checked; onchange?.(checked); }}>
  <span class="absolute top-[3px] left-[3px] w-[15px] h-[15px] transition-transform duration-100 {checked ? 'translate-x-[20px] bg-paper' : 'bg-ink'}"></span>
</button>
