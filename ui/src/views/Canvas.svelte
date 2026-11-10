<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { Application, Container, Sprite, Texture, Graphics, TilingSprite } from 'pixi.js';
  import { api, type Page } from '../lib/api';
  import { tile as fetchTile } from '../lib/host';
  import { app } from '../lib/store.svelte';
  import { t as i18 } from '../lib/i18n/index.svelte';

  const TILE = 1024;
  const MAXUNDO = 40;

  interface T {
    key: string; tx: number; ty: number; w: number; h: number;
    api: Uint8Array | null; edit: Uint8Array;
    canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D; img: ImageData; tex: Texture; over: Sprite;
    base: Sprite | null; baseKey: string; baseLoading: boolean;
    res: Sprite | null; resKey: string; resLoading: boolean;
    dil: Sprite | null; dilKey: string; dilCv: HTMLCanvasElement | null; dilTex: Texture | null;
  }

  let host: HTMLDivElement;
  let px: Application | null = null;
  let maskR: Graphics, maskO: Graphics;
  let hostH = $state(0);
  let world: Container, baseC: Container, resC: Container, overC: Container, ring: Graphics, checker: TilingSprite | null = null, frame: Graphics;
  let tiles = new Map<string, T>();
  let nx = 0, ny = 0;
  let cur: Page | null = null;
  let ready = $state(false);
  let cleanVer = 0;
  let peek = false;
  let space = false;

  let undo: { before: Map<string, Uint8Array>; after: Map<string, Uint8Array> }[] = [];
  let redo: typeof undo = [];
  let stroke: { before: Map<string, Uint8Array>; last: [number, number] | null; value: number } | null = null;
  let lasso: [number, number][] | null = null;
  let dirty = new Set<string>();
  let saveTimer: any = null;
  let panning: { x: number; y: number; wx: number; wy: number } | null = null;
  let token = 0;

  const resultShown = () => app.layers.result.vis && app.compare && !peek;

  function grayFromBitmap(bm: ImageBitmap, w: number, h: number): Uint8ClampedArray {
    const c = document.createElement('canvas'); c.width = w; c.height = h;
    const x = c.getContext('2d', { willReadFrequently: true })!;
    x.drawImage(bm, 0, 0);
    return x.getImageData(0, 0, w, h).data;
  }
  function clearTiles() {
    for (const t of tiles.values()) { t.over.destroy(); t.tex.destroy(true); t.base?.destroy({ texture: true }); t.res?.destroy({ texture: true }); t.dil?.destroy({ texture: true }); }
    tiles.clear(); dilShown = false;
    checker?.destroy(); checker = null;
    frame?.clear();
  }

  function makeChecker(): Texture {
    const c = document.createElement('canvas'); c.width = c.height = 16;
    const x = c.getContext('2d')!; x.fillStyle = '#ffffff'; x.fillRect(0, 0, 16, 16); x.fillStyle = '#f1ede4'; x.fillRect(0, 0, 8, 8); x.fillRect(8, 8, 8, 8);
    const t = Texture.from(c); t.source.style.addressMode = 'repeat'; return t;
  }

  // müsvedde: sayfanın altında sert mürekkep gölgesi ve ince çerçeve (ekran koordinatında, her ölçekte aynı kalınlık)
  function placeShadow() {
    if (!frame) return;
    frame.clear();
    if (!cur) return;
    const s = world.scale.x, w = cur.width * s, h = cur.height * s;
    frame.rect(world.x + 7, world.y + 7, w, h).fill({ color: 0x121212 });
    frame.rect(world.x - 1, world.y - 1, w + 2, h + 2).fill({ color: 0x121212 });
    app.view = { x: world.x, y: world.y, s };
  }

  async function openPage(p: Page) {
    const my = ++token;
    ready = false; undo = []; redo = []; app.canUndo = app.canRedo = false; dirty.clear();
    clearTiles();
    cur = p; cleanVer = 0; app.compare = false;
    nx = Math.ceil(p.width / TILE); ny = Math.ceil(p.height / TILE);
    checker = new TilingSprite({ texture: makeChecker(), width: p.width, height: p.height });
    baseC.addChildAt(checker, 0);
    for (let ty = 0; ty < ny; ty++) for (let tx = 0; tx < nx; tx++) {
      const w = Math.min(TILE, p.width - tx * TILE), h = Math.min(TILE, p.height - ty * TILE);
      const canvas = document.createElement('canvas'); canvas.width = w; canvas.height = h;
      const ctx = canvas.getContext('2d', { willReadFrequently: true })!;
      const img = ctx.createImageData(w, h);
      const tex = Texture.from(canvas);
      const over = new Sprite(tex); over.position.set(tx * TILE, ty * TILE); overC.addChild(over);
      tiles.set(`${tx},${ty}`, { key: `${tx},${ty}`, tx, ty, w, h, api: null, edit: new Uint8Array(TILE * TILE), canvas, ctx, img, tex, over, base: null, baseKey: '', baseLoading: false, res: null, resKey: '', resLoading: false, dil: null, dilKey: '', dilCv: null, dilTex: null });
    }
    fit(p.height / p.width > 1.8 ? 'width' : 'page');
    await loadMasks(p, my);
    if (my !== token) return;
    await probeClean(p);
    ready = true;
    applyLayers();
    scheduleVisible();
  }
  async function probeClean(p: Page) {
    app.hasClean = !!(await fetchTile(p.id, 'clean', 0, 0, 'probe' + Date.now()));
  }

  async function loadMasks(p: Page, my: number) {
    let any = false;
    await Promise.all([...tiles.values()].map(async (t) => {
      const [a, e] = await Promise.all([
        fetchTile(p.id, 'mask', t.tx, t.ty, p.rev + ':' + p.mask_conf + ':' + Date.now()),
        fetchTile(p.id, 'edit', t.tx, t.ty, 'e' + Date.now()),
      ]);
      if (my !== token) return;
      t.api = null;
      if (a) {
        any = true;
        const d = grayFromBitmap(a, t.w, t.h); const arr = new Uint8Array(TILE * TILE);
        for (let y = 0; y < t.h; y++) for (let x = 0; x < t.w; x++) arr[y * TILE + x] = d[(y * t.w + x) * 4] > 127 ? 1 : 0;
        t.api = arr;
      }
      t.edit.fill(0);
      if (e) {
        const d = grayFromBitmap(e, t.w, t.h);
        for (let y = 0; y < t.h; y++) for (let x = 0; x < t.w; x++) t.edit[y * TILE + x] = Math.min(2, d[(y * t.w + x) * 4]);
      }
      redraw(t);
    }));
    app.hasApiMask = any;
    maskVer++; scheduleDil();
  }

  function redraw(t: T, r?: [number, number, number, number]) {
    const [x0, y0, x1, y1] = r ?? [0, 0, t.w, t.h];
    const d = t.img.data, L = app.layers;
    const aA = Math.round(255 * L.api.op), aE = Math.round(255 * L.edit.op);
    const sa = L.api.vis, se = L.edit.vis;
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) {
      const e = t.edit[y * TILE + x]; const a = t.api ? t.api[y * TILE + x] : 0;
      const o = (y * t.w + x) * 4;
      if (e === 1 && se) { const hatch = ((x + y + t.tx * TILE + t.ty * TILE) % 6) < 3; d[o] = 47; d[o + 1] = 111; d[o + 2] = 214; d[o + 3] = hatch ? aE : aE * 0.35; }
      else if (e !== 2 && a && sa) { d[o] = 225; d[o + 1] = 52; d[o + 2] = 30; d[o + 3] = aA; }
      else d[o + 3] = 0;
    }
    t.ctx.putImageData(t.img, 0, 0, x0, y0, x1 - x0, y1 - y0);
    t.tex.source.update();
  }

  // ---------- görünüm ----------
  function fit(mode: 'width' | 'page') {
    if (!cur || !px) return;
    const W = host.clientWidth, H = host.clientHeight;
    const PX = 84, TOP = 30, BOT = 72; // araç kutusu ve alt gezinme için boşluk
    const s = mode === 'width' ? Math.min((W - PX * 2) / cur.width, 4) : Math.min((W - PX * 2) / cur.width, (H - TOP - BOT) / cur.height, 4);
    world.scale.set(s);
    world.position.set((W - cur.width * s) / 2, cur.height * s > H - TOP - BOT ? TOP : TOP + (H - TOP - BOT - cur.height * s) / 2);
    afterTransform();
  }
  function zoomTo(z: number) {
    if (!cur || !px) return;
    const W = host.clientWidth, H = host.clientHeight;
    const [ix, iy] = toImg(W / 2, H / 2);
    world.scale.set(z); world.position.set(W / 2 - ix * z, H / 2 - iy * z); afterTransform();
  }
  function afterTransform() { app.zoomPct = Math.round(world.scale.x * 100); placeShadow(); scheduleVisible(); }
  let visRaf = 0;
  function scheduleVisible() { if (!visRaf) visRaf = requestAnimationFrame(() => { visRaf = 0; updateVisible(); }); }

  function updateVisible() {
    if (!px || !cur) return;
    scheduleDil();
    const s = world.scale.x, W = host.clientWidth, H = host.clientHeight;
    const x0 = -world.x / s, y0 = -world.y / s, x1 = x0 + W / s, y1 = y0 + H / s;
    const [a, b, c, d] = [Math.floor(x0 / TILE), Math.floor(y0 / TILE), Math.floor(x1 / TILE), Math.floor(y1 / TILE)];
    const needRes = resultShown() || app.layers.result.vis && app.compare;
    for (const t of tiles.values()) {
      const inView = t.tx >= a && t.tx <= c && t.ty >= b && t.ty <= d;
      if (inView) {
        if (t.baseKey !== 'orig' && !t.baseLoading) loadLayer(t, 'orig');
        if (needRes && app.hasClean && t.resKey !== 'clean:' + cleanVer && !t.resLoading) loadLayer(t, 'clean');
      } else if (t.tx < a - 1 || t.tx > c + 1 || t.ty < b - 1 || t.ty > d + 1) {
        if (t.base) { t.base.destroy({ texture: true }); t.base = null; t.baseKey = ''; }
        if (t.res) { t.res.destroy({ texture: true }); t.res = null; t.resKey = ''; }
      }
    }
  }
  async function loadLayer(t: T, layer: 'orig' | 'clean') {
    if (!cur) return;
    const my = token, p = cur;
    const isRes = layer === 'clean';
    if (isRes) t.resLoading = true; else t.baseLoading = true;
    const bm = await fetchTile(p.id, layer, t.tx, t.ty, isRes ? cleanVer : 'o');
    if (isRes) t.resLoading = false; else t.baseLoading = false;
    if (my !== token || !tiles.has(t.key)) return;
    const key = isRes ? 'clean:' + cleanVer : 'orig';
    if (!bm) { if (isRes) t.resKey = key; else t.baseKey = key; return; }
    const tex = Texture.from(bm);
    const cont = isRes ? resC : baseC;
    let sp = isRes ? t.res : t.base;
    if (sp) { const old = sp.texture; sp.texture = tex; old.destroy(true); }
    else { sp = new Sprite(tex); sp.position.set(t.tx * TILE, t.ty * TILE); cont.addChild(sp); if (isRes) t.res = sp; else t.base = sp; }
    if (isRes) t.resKey = key; else t.baseKey = key;
  }

  function applyLayers() {
    if (!baseC) return;
    const L = app.layers, showRes = resultShown();
    baseC.visible = L.orig.vis; baseC.alpha = L.orig.op;
    resC.visible = showRes; resC.alpha = L.result.op;
    overC.visible = !showRes || app.split;
    applySplit();
    for (const t of tiles.values()) redraw(t);
    scheduleVisible();
    scheduleDil();
  }
  // önce | sonra: sonuç çizginin sağında, maske/orijinal solunda
  const splitOn = $derived(app.compare && app.split && app.hasClean && app.layers.result.vis && !!app.page);
  function applySplit() {
    if (!resC || !cur) return;
    if (!(resultShown() && app.split)) {
      // maske kaldırılırken çizimleri de silinmeli: maskeler `world` içinde durur, silinmezse beyaz dikdörtgen olarak tüm sayfayı örter
      resC.mask = null; overC.mask = null; maskR.clear(); maskO.clear();
      return;
    }
    const x = app.splitAt * cur.width;
    maskR.clear().rect(x, 0, cur.width - x, cur.height).fill(0xffffff);
    maskO.clear().rect(0, 0, x, cur.height).fill(0xffffff);
    resC.mask = maskR; overC.mask = maskO;
  }
  $effect(() => { app.splitAt; untrack(applySplit); });
  let splitDrag = false;
  const divX = $derived(app.view.x + app.splitAt * (app.page?.width ?? 0) * app.view.s);
  const divTop = $derived(Math.max(0, app.view.y));
  const divBot = $derived(Math.min(hostH, app.view.y + (app.page?.height ?? 0) * app.view.s));
  function splitMove(e: PointerEvent) {
    if (!splitDrag || !cur) return;
    const [sx] = local(e);
    app.splitAt = Math.min(1, Math.max(0, (sx - world.x) / world.scale.x / cur.width));
  }
  $effect(() => {
    const L = app.layers;
    app.split;
    L.orig.vis; L.orig.op; L.api.vis; L.api.op; L.edit.vis; L.edit.op; L.result.vis; L.result.op; app.compare;
    untrack(applyLayers);
  });

  // sayfa değişimi
  $effect(() => {
    const p = app.page;
    untrack(() => {
      if (!px) return;
      if (!p) { clearTiles(); cur = null; ready = false; return; }
      if (cur && cur.id === p.id) return;
      openPage(p);
    });
  });
  // iş bitti -> tazele
  let lastTick = app.tick;
  $effect(() => {
    const t = app.tick;
    if (t === lastTick) return;
    lastTick = t;
    untrack(async () => {
      if (!cur) return;
      const p = await api.getPage(cur.id); if (!p) return;
      cur = p; cleanVer++;
      await loadMasks(p, token);
      await probeClean(p);
      for (const tt of tiles.values()) tt.resKey = '';
      if (p.status === 'cleaned' && app.hasClean) app.compare = true;
      applyLayers();
    });
  });

  // ---------- çizim ----------
  const tileAt = (tx: number, ty: number) => tiles.get(`${tx},${ty}`);
  function snapshot(t: T) { if (stroke && !stroke.before.has(t.key)) stroke.before.set(t.key, t.edit.slice()); }
  function stamp(cx: number, cy: number, value: number) {
    const r = Math.max(1, app.brushSize / 2), r2 = r * r;
    const x0 = Math.floor(cx - r), x1 = Math.ceil(cx + r), y0 = Math.floor(cy - r), y1 = Math.ceil(cy + r);
    for (let ty = Math.max(0, Math.floor(y0 / TILE)); ty <= Math.min(ny - 1, Math.floor(y1 / TILE)); ty++)
      for (let tx = Math.max(0, Math.floor(x0 / TILE)); tx <= Math.min(nx - 1, Math.floor(x1 / TILE)); tx++) {
        const t = tileAt(tx, ty)!;
        const lx0 = Math.max(0, x0 - tx * TILE), lx1 = Math.min(t.w, x1 - tx * TILE + 1);
        const ly0 = Math.max(0, y0 - ty * TILE), ly1 = Math.min(t.h, y1 - ty * TILE + 1);
        if (lx0 >= lx1 || ly0 >= ly1) continue;
        let changed = false;
        for (let y = ly0; y < ly1; y++) for (let x = lx0; x < lx1; x++) {
          const dx = x + tx * TILE + 0.5 - cx, dy = y + ty * TILE + 0.5 - cy;
          if (dx * dx + dy * dy <= r2 && t.edit[y * TILE + x] !== value) {
            if (!changed) { snapshot(t); changed = true; }
            t.edit[y * TILE + x] = value;
          }
        }
        if (changed) { dirty.add(t.key); redraw(t, [lx0, ly0, lx1, ly1]); maskVer++; scheduleDil(); }
      }
  }
  function strokeTo(ix: number, iy: number) {
    if (!stroke) return;
    const last = stroke.last;
    if (!last) stamp(ix, iy, stroke.value);
    else {
      const dx = ix - last[0], dy = iy - last[1], dist = Math.hypot(dx, dy);
      const step = Math.max(1, app.brushSize / 6), n = Math.max(1, Math.ceil(dist / step));
      for (let i = 1; i <= n; i++) stamp(last[0] + (dx * i) / n, last[1] + (dy * i) / n, stroke.value);
    }
    stroke.last = [ix, iy];
  }
  function endStroke() {
    if (!stroke) return;
    const after = new Map<string, Uint8Array>();
    for (const k of stroke.before.keys()) after.set(k, tiles.get(k)!.edit.slice());
    if (stroke.before.size) {
      undo.push({ before: stroke.before, after }); if (undo.length > MAXUNDO) undo.shift();
      redo = []; app.canUndo = true; app.canRedo = false; scheduleSave();
    }
    stroke = null;
  }
  function fillLasso(pts: [number, number][], value: number) {
    if (pts.length < 3 || !cur) return;
    let minx = Infinity, miny = Infinity, maxx = -Infinity, maxy = -Infinity;
    for (const [x, y] of pts) { minx = Math.min(minx, x); miny = Math.min(miny, y); maxx = Math.max(maxx, x); maxy = Math.max(maxy, y); }
    const bx = Math.max(0, Math.floor(minx)), by = Math.max(0, Math.floor(miny));
    const bw = Math.min(cur.width, Math.ceil(maxx)) - bx, bh = Math.min(cur.height, Math.ceil(maxy)) - by;
    if (bw <= 0 || bh <= 0) return;
    const c = document.createElement('canvas'); c.width = bw; c.height = bh;
    const x = c.getContext('2d', { willReadFrequently: true })!;
    x.fillStyle = '#fff'; x.beginPath(); pts.forEach(([px_, py_], i) => (i ? x.lineTo(px_ - bx, py_ - by) : x.moveTo(px_ - bx, py_ - by))); x.closePath(); x.fill();
    const data = x.getImageData(0, 0, bw, bh).data;
    stroke = { before: new Map(), last: null, value };
    const touched = new Set<T>();
    for (let y = 0; y < bh; y++) for (let xx = 0; xx < bw; xx++) {
      if (data[(y * bw + xx) * 4 + 3] < 128) continue;
      const gx = bx + xx, gy = by + y;
      const t = tileAt(Math.floor(gx / TILE), Math.floor(gy / TILE)); if (!t) continue;
      const i = (gy % TILE) * TILE + (gx % TILE);
      if (t.edit[i] !== value) { snapshot(t); t.edit[i] = value; touched.add(t); dirty.add(t.key); }
    }
    for (const t of touched) redraw(t);
    maskVer++; scheduleDil();
    endStroke();
  }
  function applyEntry(m: Map<string, Uint8Array>) {
    for (const [k, v] of m) { const t = tiles.get(k); if (!t) continue; t.edit.set(v); dirty.add(k); redraw(t); }
    maskVer++; scheduleDil();
    scheduleSave();
  }
  function doUndo() { const e = undo.pop(); if (!e) return; applyEntry(e.before); redo.push(e); app.canUndo = undo.length > 0; app.canRedo = true; }
  function doRedo() { const e = redo.pop(); if (!e) return; applyEntry(e.after); undo.push(e); app.canRedo = redo.length > 0; app.canUndo = true; }

  function scheduleSave() { clearTimeout(saveTimer); saveTimer = setTimeout(flush, 700); }
  async function flush() {
    clearTimeout(saveTimer);
    if (!cur || !dirty.size) return;
    const pid = cur.id;
    const keys = [...dirty]; dirty.clear(); app.saving = true;
    const parts: Uint8Array[] = [];
    for (const k of keys) {
      const t = tiles.get(k); if (!t) continue;
      const head = new Uint8Array(8); const dv = new DataView(head.buffer); dv.setUint32(0, t.tx, true); dv.setUint32(4, t.ty, true);
      parts.push(head, t.edit.slice());
    }
    const body = new Uint8Array(parts.reduce((n, p) => n + p.length, 0)); let o = 0;
    for (const p of parts) { body.set(p, o); o += p.length; }
    try { await api.saveEdit(pid, body); } catch (e) { keys.forEach((k) => dirty.add(k)); app.toast(i18('canvas.save_failed', { err: String(e) }), 'err'); }
    app.saving = false;
  }


  // ---------- genişletme (dilation) önizlemesi ----------
  // Denetçideki "Genişlet" üstündeyken ya da değişirken, son maskenin (API ∪ mavi − silinen) genişletilmiş dış çizgisi çizilir.
  // Arka uçtaki gibi disk yapı elemanı, yalnızca kenar piksellerine basılır; yalnızca görünen karolar, kısa gecikmeyle (debounce).
  let dilC: Container;
  let dilTimer: any = null;
  let maskVer = 0;
  let dilShown = false;
  function scheduleDil() {
    if (!app.dilOn && !dilShown) return;
    clearTimeout(dilTimer);
    dilTimer = setTimeout(updateDilation, 60);
  }
  function dropDil(t: T) {
    if (t.dil) { t.dil.destroy({ texture: true }); t.dil = null; t.dilTex = null; t.dilCv = null; }
    t.dilKey = '';
  }
  /** t karosunu r piksel genişletip çizim tuvaline yazar (komşu karolardan r+1 piksellik pay okunur). */
  function dilateTile(t: T, r: number) {
    const pad = r + 1, W = t.w + 2 * pad, H = t.h + 2 * pad;
    const M = new Uint8Array(W * H);
    const gx0 = t.tx * TILE - pad, gy0 = t.ty * TILE - pad;
    let any = false;
    for (let ty = Math.max(0, Math.floor(gy0 / TILE)); ty <= Math.min(ny - 1, Math.floor((gy0 + H - 1) / TILE)); ty++)
      for (let tx = Math.max(0, Math.floor(gx0 / TILE)); tx <= Math.min(nx - 1, Math.floor((gx0 + W - 1) / TILE)); tx++) {
        const n = tileAt(tx, ty); if (!n) continue;
        const x0 = Math.max(gx0, tx * TILE), x1 = Math.min(gx0 + W, tx * TILE + n.w), y0 = Math.max(gy0, ty * TILE), y1 = Math.min(gy0 + H, ty * TILE + n.h);
        for (let y = y0; y < y1; y++) {
          const ro = (y - ty * TILE) * TILE - tx * TILE, mo = (y - gy0) * W - gx0;
          for (let x = x0; x < x1; x++) {
            const e = n.edit[ro + x], v = e === 1 ? 1 : e === 2 ? 0 : n.api ? n.api[ro + x] : 0;
            if (v) { M[mo + x] = 1; any = true; }
          }
        }
      }
    if (!any) { dropDil(t); return; }
    const D = M.slice();
    const spans: number[] = [];
    for (let dy = -r; dy <= r; dy++) spans.push(Math.floor(Math.sqrt(r * r - dy * dy + 0.25)));
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
      const i = y * W + x;
      if (!M[i]) continue;
      if (x > 0 && y > 0 && x < W - 1 && y < H - 1 && M[i - 1] && M[i + 1] && M[i - W] && M[i + W]) continue; // iç piksel
      for (let dy = -r; dy <= r; dy++) {
        const yy = y + dy; if (yy < 0 || yy >= H) continue;
        const hx = spans[dy + r], a = Math.max(0, x - hx), b = Math.min(W - 1, x + hx);
        D.fill(1, yy * W + a, yy * W + b + 1);
      }
    }
    if (!t.dilCv) {
      t.dilCv = document.createElement('canvas'); t.dilCv.width = t.w; t.dilCv.height = t.h;
      t.dilTex = Texture.from(t.dilCv);
      t.dil = new Sprite(t.dilTex); t.dil.position.set(t.tx * TILE, t.ty * TILE); dilC.addChild(t.dil);
    }
    const ctx = t.dilCv.getContext('2d')!, img = ctx.createImageData(t.w, t.h), o = img.data;
    for (let y = 0; y < t.h; y++) for (let x = 0; x < t.w; x++) {
      const g = (y + pad) * W + x + pad;
      if (!D[g]) continue;
      const k = (y * t.w + x) * 4;
      const edge = !(D[g - 1] && D[g + 1] && D[g - W] && D[g + W]);
      if (edge) { o[k] = 225; o[k + 1] = 52; o[k + 2] = 30; o[k + 3] = 255; }
      else if (!M[g]) { o[k] = 225; o[k + 1] = 52; o[k + 2] = 30; o[k + 3] = ((x + y) % 6) < 3 ? 120 : 45; }
    }
    ctx.putImageData(img, 0, 0);
    t.dilTex!.source.update();
  }
  function updateDilation() {
    if (!px || !cur || !ready) return;
    const on = app.dilOn && overC.visible;
    if (!on) {
      if (dilShown) { for (const t of tiles.values()) dropDil(t); dilShown = false; }
      dilC.visible = false;
      return;
    }
    const r = app.opts.dilation, s = world.scale.x, W = host.clientWidth, H = host.clientHeight;
    const [a, b, c, d] = [Math.floor((-world.x / s) / TILE), Math.floor((-world.y / s) / TILE), Math.floor((-world.x / s + W / s) / TILE), Math.floor((-world.y / s + H / s) / TILE)];
    const key = `${r}:${maskVer}`;
    dilC.visible = true; dilShown = true;
    for (const t of tiles.values()) {
      if (t.tx < a || t.tx > c || t.ty < b || t.ty > d) { dropDil(t); continue; }
      if (t.dilKey === key) continue;
      dilateTile(t, r);
      t.dilKey = key;
    }
  }
  $effect(() => { app.dilOn; app.opts.dilation; app.layers.api.vis; app.layers.edit.vis; untrack(scheduleDil); });

  // ---------- olaylar ----------
  function local(e: { clientX: number; clientY: number }): [number, number] {
    const r = host.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }
  const toImg = (sx: number, sy: number): [number, number] => [(sx - world.x) / world.scale.x, (sy - world.y) / world.scale.x];


  // fırça halkası: işaretçi tuvalden çıkınca (başka öğe, pencere dışı, odak kaybı), araç değişince ya da bir pencere açılınca kaybolur
  let lastSx = -1, lastSy = -1, inside = false;
  function hideRing() { app.cursor = null; inside = false; if (!panning && !stroke && !lasso) ring?.clear(); }
  function onWindowBlur() { hideRing(); }
  function onDocLeave() { hideRing(); }
  $effect(() => {
    app.tool;
    untrack(() => { ring?.clear(); if (inside && lastSx >= 0) drawRing(lastSx, lastSy); });
  });
  $effect(() => { if (app.dialog || app.help || app.asking || app.menu) untrack(hideRing); });

  function drawRing(sx: number, sy: number) {
    ring.clear();
    if (lasso && lasso.length > 1) {
      ring.moveTo(world.x + lasso[0][0] * world.scale.x, world.y + lasso[0][1] * world.scale.x);
      for (const [x, y] of lasso) ring.lineTo(world.x + x * world.scale.x, world.y + y * world.scale.x);
      ring.stroke({ width: 2, color: app.lassoMode === 'add' ? 0x2f6fd6 : 0x121212, alpha: 1 });
    }
    if (app.tool === 'brush' || app.tool === 'eraser')
    {
      const r = Math.max(2, (app.brushSize / 2) * world.scale.x);
      ring.circle(sx, sy, r + 1.5).stroke({ width: 1.5, color: 0xffffff, alpha: 0.9 });
      ring.circle(sx, sy, r).stroke({ width: 2, color: app.tool === 'brush' ? 0x2f6fd6 : 0x121212, alpha: 1 });
      ring.circle(sx, sy, 1.6).fill({ color: 0x121212 });
    }
  }

  function onDown(e: PointerEvent) {
    if (!ready) return;
    host.setPointerCapture(e.pointerId);
    const [sx, sy] = local(e);
    if (e.button === 1 || e.button === 2 || space || (e.button === 0 && app.tool === 'hand')) { panning = { x: sx, y: sy, wx: world.x, wy: world.y }; return; }
    if (e.button !== 0) return;
    const [ix, iy] = toImg(sx, sy);
    if (app.tool === 'zoom') {
      const k = e.altKey ? 0.5 : 2, s1 = Math.min(24, Math.max(0.02, world.scale.x * k));
      world.scale.set(s1); world.position.set(sx - ix * s1, sy - iy * s1); afterTransform(); return;
    }
    if (app.tool === 'lasso') { lasso = [[ix, iy]]; return; }
    stroke = { before: new Map(), last: null, value: app.tool === 'brush' ? 1 : 2 };
    strokeTo(ix, iy);
  }
  function onMove(e: PointerEvent) {
    const [sx, sy] = local(e);
    lastSx = sx; lastSy = sy; inside = true;
    const [ix, iy] = toImg(sx, sy);
    app.cursor = cur ? { x: Math.round(ix), y: Math.round(iy) } : null;
    if (panning) { world.position.set(panning.wx + sx - panning.x, panning.wy + sy - panning.y); afterTransform(); drawRing(-100, -100); return; }
    if (lasso) lasso.push([ix, iy]);
    if (stroke) strokeTo(ix, iy);
    drawRing(sx, sy);
  }
  function onUp(e: PointerEvent) {
    panning = null;
    if (lasso) { const l = lasso; lasso = null; fillLasso(l, (app.lassoMode === 'add') !== e.altKey ? 1 : 2); }
    endStroke(); ring.clear();
    // bırakıldığı yer hâlâ tuvalin içindeyse halka yerinde kalsın; dışındaysa kapalı
    const [sx, sy] = local(e);
    inside = sx >= 0 && sy >= 0 && sx <= host.clientWidth && sy <= host.clientHeight;
    if (inside) drawRing(sx, sy); else app.cursor = null;
  }
  function onLeave() { hideRing(); }
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const [sx, sy] = local(e);
    if (e.ctrlKey && (app.tool === 'brush' || app.tool === 'eraser')) { app.brushSize = Math.max(2, Math.min(400, Math.round(app.brushSize * (e.deltaY < 0 ? 1.12 : 0.89)))); drawRing(sx, sy); return; }
    const k = Math.exp(-e.deltaY * 0.0016);
    const s0 = world.scale.x, s1 = Math.min(24, Math.max(0.02, s0 * k));
    const [ix, iy] = toImg(sx, sy);
    world.scale.set(s1); world.position.set(sx - ix * s1, sy - iy * s1); afterTransform();
  }
  function onKeyDown(e: KeyboardEvent) {
    if (e.code === 'Space' && !(e.target as HTMLElement)?.closest?.('input,textarea,select')) { space = true; host.style.cursor = 'grab'; e.preventDefault(); }
    if (e.key === '\\' && !peek) { peek = true; applyLayers(); }
  }
  function onKeyUp(e: KeyboardEvent) {
    if (e.code === 'Space') { space = false; setCursor(); }
    if (e.key === '\\') { peek = false; applyLayers(); }
  }
  function setCursor() {
    if (!host) return;
    host.style.cursor = app.tool === 'hand' ? 'grab' : app.tool === 'zoom' ? 'zoom-in' : app.tool === 'lasso' ? 'crosshair' : 'none';
  }
  $effect(() => { app.tool; untrack(setCursor); });

  onMount(async () => {
    px = new Application();
    await px.init({ resizeTo: host, backgroundAlpha: 0, antialias: true, resolution: window.devicePixelRatio || 1, autoDensity: true, preference: 'webgl' });
    host.appendChild(px.canvas);
    world = new Container(); baseC = new Container(); resC = new Container(); overC = new Container(); dilC = new Container(); dilC.visible = false;
    maskR = new Graphics(); maskO = new Graphics();
    world.addChild(baseC, resC, overC, dilC, maskR, maskO);
    frame = new Graphics();
    px.stage.addChild(frame, world);
    ring = new Graphics(); px.stage.addChild(ring);
    px.renderer.on('resize', () => { placeShadow(); scheduleVisible(); });
    window.addEventListener('keydown', onKeyDown); window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onWindowBlur); document.documentElement.addEventListener('mouseleave', onDocLeave);
    app.cv = { fit, zoomTo, undo: doUndo, redo: doRedo, flush, reloadAll: () => { app.tick++; } };
    setCursor();
    if (app.page) openPage(app.page);
  });
  onDestroy(() => {
    window.removeEventListener('keydown', onKeyDown); window.removeEventListener('keyup', onKeyUp);
    window.removeEventListener('blur', onWindowBlur); document.documentElement.removeEventListener('mouseleave', onDocLeave);
    clearTimeout(dilTimer);
    token++; clearTiles(); app.cv = null;
    try { px?.destroy(true, { children: true, texture: true }); } catch {}
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="absolute inset-0 overflow-hidden"
  bind:this={host}
  bind:clientHeight={hostH}
  onpointerdown={onDown}
  onpointermove={onMove}
  onpointerup={onUp}
  onpointercancel={onUp}
  onpointerleave={onLeave}
  onlostpointercapture={onLeave}
  onwheel={onWheel}
  oncontextmenu={(e) => e.preventDefault()}
  style="touch-action:none"
>
  {#if splitOn && ready}
    <div class="absolute z-10 pointer-events-none" style="left:{divX - 1}px; top:{divTop}px; height:{Math.max(0, divBot - divTop)}px; width:2px; background:var(--color-ink); box-shadow:0 0 0 1px #fff"></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="absolute z-20 w-[26px] h-[44px] -ml-[13px] bg-ink text-paper grid place-items-center cursor-ew-resize shadow-[3px_3px_0_#fff] border-2 border-paper"
      style="left:{divX}px; top:{Math.min(Math.max(divTop + 20, hostH / 2 - 22), Math.max(divTop + 20, divBot - 64))}px"
      onpointerdown={(e) => { e.stopPropagation(); splitDrag = true; (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); }}
      onpointermove={splitMove} onpointerup={() => (splitDrag = false)} onpointercancel={() => (splitDrag = false)}>
      <span class="mono text-[13px] font-bold leading-none">◂▸</span>
    </div>
    <span class="absolute z-10 tag tag-ink pointer-events-none" style="left:{Math.max(8, divX - 62)}px; top:{divTop + 8}px">{i18('canvas.before')}</span>
    <span class="absolute z-10 tag bg-sheet pointer-events-none" style="left:{divX + 8}px; top:{divTop + 8}px">{i18('canvas.after')}</span>
  {/if}
  {#if app.page && !ready}<div class="absolute inset-0 grid place-items-center pointer-events-none"><span class="mono font-semibold bg-sheet border-[1.5px] border-ink px-2 py-1">{i18('canvas.loading')}</span></div>{/if}
  {#if !app.page && app.project}<div class="absolute inset-0 grid place-items-center pointer-events-none"><span class="bg-sheet border-[1.5px] border-ink px-3 py-2 font-medium">{i18('canvas.empty')}</span></div>{/if}
</div>
