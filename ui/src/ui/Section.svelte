<script lang="ts">
  import { ChevronDown } from 'lucide-svelte';
  import type { Snippet } from 'svelte';
  interface Props { title: string; id: string; children: Snippet; aside?: Snippet; last?: boolean }
  let { title, id, children, aside, last = false }: Props = $props();
  const key = () => 'safink.sec.' + id;
  let open = $state((() => { try { return localStorage.getItem('safink.sec.' + id) !== '0'; } catch { return true; } })());
  function toggle() { open = !open; try { localStorage.setItem(key(), open ? '1' : '0'); } catch {} }
</script>

<section class={last ? '' : 'border-b-[2.5px] border-ink'}>
  <button class="tone-light w-full h-[36px] px-3 flex items-center gap-2 {open ? 'border-b-[1.5px] border-ink' : ''}" onclick={toggle}>
    <span class="disp text-[17px] leading-none bg-sheet px-1.5 py-1 -ml-1.5">{title}</span>
    <span class="flex-1"></span>
    {#if aside}<span class="bg-sheet px-1">{@render aside()}</span>{/if}
    <span class="bg-sheet p-0.5"><ChevronDown size={14} strokeWidth={2.25} class="transition-transform {open ? '' : '-rotate-90'}" /></span>
  </button>
  {#if open}<div class="p-3">{@render children()}</div>{/if}
</section>
