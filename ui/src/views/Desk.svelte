<script lang="ts">
  // Çalışma masası: üstte araç şeridi, kenarlarda gerçek cetveller, altında kesme matı ızgarası ve tuval.
  import { app } from '../lib/store.svelte';
  import Canvas from './Canvas.svelte';
  import ContextBar from './ContextBar.svelte';
  import ToolRail from './ToolRail.svelte';
  import StageHud from './StageHud.svelte';
  import Ruler from '../ui/Ruler.svelte';
</script>

<main class="koma overflow-hidden bg-desk">
  {#if app.page}
    <ContextBar />
    <div class="absolute left-0 top-[40px] w-[22px] h-[22px] bg-ink z-10"></div>
    <div class="absolute left-[22px] right-0 top-[40px] h-[22px] bg-sheet border-b-[1.5px] border-ink z-10 overflow-hidden"><Ruler dir="h" /></div>
    <div class="absolute left-0 top-[62px] bottom-0 w-[22px] bg-sheet border-r-[1.5px] border-ink z-10 overflow-hidden"><Ruler dir="v" /></div>
  {/if}
  <div class="mat absolute {app.page ? 'left-[22px] top-[62px]' : 'left-0 top-0'} right-0 bottom-0">
    <Canvas />
    {#if app.page}
      <ToolRail />
      <StageHud />
    {/if}
  </div>
</main>

<style>
  .mat {
    background-image:
      linear-gradient(#12121216 1px, transparent 1px), linear-gradient(90deg, #12121216 1px, transparent 1px),
      linear-gradient(#1212120b 1px, transparent 1px), linear-gradient(90deg, #1212120b 1px, transparent 1px);
    background-size: 100px 100px, 100px 100px, 20px 20px, 20px 20px;
  }
</style>
