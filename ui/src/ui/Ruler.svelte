<script lang="ts">
  // Gerçek piksel cetveli: tuvalin kaydırma ve yakınlığını izler; imleç konumu kırmızı çizgiyle gösterilir.
  import { app } from '../lib/store.svelte';
  let { dir }: { dir: 'h' | 'v' } = $props();
  let cv: HTMLCanvasElement;
  let box = $state({ w: 0, h: 0 });

  $effect(() => {
    const { x, y, s } = app.view, { w, h } = box, cur = app.cursor, pg = app.page;
    if (!cv || !w || !h) return;
    const dpr = window.devicePixelRatio || 1;
    cv.width = w * dpr; cv.height = h * dpr;
    const c = cv.getContext('2d')!;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.clearRect(0, 0, w, h);
    const len = dir === 'h' ? w : h, thick = dir === 'h' ? h : w, off = dir === 'h' ? x : y;
    // sayfanın kapladığı bölüm biraz koyu
    if (pg) {
      const a = off, b = off + (dir === 'h' ? pg.width : pg.height) * s;
      c.fillStyle = '#12121212';
      if (dir === 'h') c.fillRect(a, 0, b - a, h); else c.fillRect(0, a, w, b - a);
    }
    const steps = [10, 20, 50, 100, 200, 500, 1000, 2000, 5000];
    const major = steps.find((v) => v * s >= 60) ?? 5000, minor = major / 5;
    const start = Math.floor(-off / s / minor) * minor, end = (len - off) / s;
    c.strokeStyle = '#121212'; c.fillStyle = '#121212'; c.lineWidth = 1;
    c.font = '600 9.5px "IBM Plex Mono", monospace';
    c.beginPath();
    for (let v = start; v <= end; v += minor) {
      const p = Math.round(off + v * s) + 0.5;
      const isMajor = Math.abs(v / major - Math.round(v / major)) < 1e-6;
      const t = isMajor ? thick - 2 : 5;
      if (dir === 'h') { c.moveTo(p, thick); c.lineTo(p, thick - t); }
      else { c.moveTo(thick, p); c.lineTo(thick - t, p); }
      if (isMajor) {
        if (dir === 'h') c.fillText(String(Math.round(v)), p + 3, 10);
        else { c.save(); c.translate(10, p - 3); c.rotate(-Math.PI / 2); c.fillText(String(Math.round(v)), 0, 0); c.restore(); }
      }
    }
    c.stroke();
    if (cur) {
      const p = Math.round(off + (dir === 'h' ? cur.x : cur.y) * s);
      c.fillStyle = '#e1341e';
      if (dir === 'h') c.fillRect(p - 1, 0, 2, h); else c.fillRect(0, p - 1, w, 2);
    }
  });
</script>

<div class="absolute inset-0" bind:clientWidth={box.w} bind:clientHeight={box.h}>
  <canvas bind:this={cv} class="block" style="width:{box.w}px;height:{box.h}px"></canvas>
</div>
