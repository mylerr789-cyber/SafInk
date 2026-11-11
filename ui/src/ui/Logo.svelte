<script lang="ts">
  // İşaret: fırçayla kavis çizen anime kızı silüeti (docs/logo/mark-1024.png'den küçültülmüş).
  // Kelime işareti: SAFINK.
  import mark from '../assets/logo-mark.png';
  let { size = 26 }: { size?: number } = $props();
</script>

<span class="inline-flex items-center select-none whitespace-nowrap" style="gap:{Math.round(size * 0.3)}px">
  <img src={mark} alt="" aria-hidden="true" draggable="false" class="shrink-0" width={Math.round(size * 1.4)} height={Math.round(size * 1.4)} />
  <span class="disp leading-none" style="font-size:{size}px; letter-spacing:.03em">SAFINK</span>
</span>
