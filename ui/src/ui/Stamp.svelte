<script lang="ts">
  // Hanko mühür: temizlenen sayfaya basılır. Hata: dikdörtgen ret damgası.
  let { kind = 'done', size = 50, text = '' }: { kind?: 'done' | 'error'; size?: number; text?: string } = $props();
</script>

{#if kind === 'done'}
  <span class="stamp grid place-items-center rounded-full border-[2.5px] border-red text-red disp bg-sheet/80"
    style="--r:-12deg; width:{size}px; height:{size}px; font-size:{Math.round(size * 0.26)}px">{text || 'Temiz'}</span>
{:else}
  <span class="stamp inline-block border-[2.5px] border-red text-red disp bg-sheet/85 px-1.5 py-0.5"
    style="--r:5deg; font-size:{Math.round(size * 0.26)}px">{text || 'Hata'}</span>
{/if}

<style>
  .stamp { transform: rotate(var(--r)); animation: stamp 260ms cubic-bezier(.3, .7, .3, 1.2) both; line-height: 1; }
</style>
