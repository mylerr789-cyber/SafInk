<script lang="ts">
  import { Eye, EyeOff, RefreshCw, Download, ScanText } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  import { STATUS, fmtBytes } from '../lib/api';
  import Section from '../ui/Section.svelte';
  import Scrub from '../ui/Scrub.svelte';
  import Select from '../ui/Select.svelte';
  import Segmented from '../ui/Segmented.svelte';

  const save = () => app.saveOpts();
  const model = $derived(app.models.find((m) => m.id === app.opts.model_id));
  const fitTxt: Record<string, string> = { fits: '', tight: 'Biraz zorlanabilir', no: 'Ekran kartın yetmeyebilir', slow: 'Ekran kartı yok, yavaş çalışır', unknown: '' };
  const modelOpts = $derived(app.models.slice()
    .sort((a, b) => (a.kind === b.kind ? 0 : a.kind === 'inpaint' ? -1 : 1))
    .map((m) => ({
      value: m.id, label: m.name, group: m.kind === 'redraw' ? 'Yeniden çizim' : 'Hızlı silme',
      hint: `${fmtBytes(m.total_bytes)}${m.installed ? '' : ' · kurulu değil'}`,
      mark: (m.installed ? 'ok' : 'off') as 'ok' | 'off',
    })));
  // ön ayarlar kısa adlarla çip olarak
  const presetShort = (name: string) => name.split(/[–-]/)[0].trim();
  const presetOpts = $derived((app.settings?.presets ?? []).map((p) => ({ value: p.id, label: presetShort(p.name), tip: p.name })));
  const custom = () => { if (app.opts.preset_id !== 'custom') app.opts.preset_id = 'custom'; save(); };

  const st = $derived(app.page ? app.statusOf(app.page) : 'new');
  const err = $derived(app.page ? app.pageStatus[app.page.id]?.error || app.page.error : '');
  const layerRows = [
    { id: 'orig', name: 'Orijinal', hint: 'kaynak sayfa', sw: 'background:#fff' },
    { id: 'api', name: 'Kırmızı kalem', hint: 'otomatik maske', sw: 'background:var(--color-red)' },
    { id: 'edit', name: 'Mavi kalem', hint: 'elle eklenen', sw: 'background:repeating-linear-gradient(45deg,var(--color-blue) 0 2px,#fff 2px 4px)' },
    { id: 'result', name: 'Daksil', hint: 'temiz sonuç', sw: 'background:#fff;background-image:radial-gradient(circle,#12121240 .9px,transparent 1.1px);background-size:4px 4px' },
  ] as const;
  const blocked = $derived(!app.page || app.jobActive || !!(model && !model.installed));
</script>

<aside class="koma flex flex-col overflow-hidden">
  <div class="flex-1 min-h-0 overflow-y-auto">
    {#if app.page}
      <div class="px-3 py-2.5 border-b-[2.5px] border-ink">
        <div class="flex items-center gap-2">
          <span class="mono font-semibold truncate flex-1" title={app.page.path}>{app.page.name}</span>
          <span class="tag {STATUS[st]?.tag}">{STATUS[st]?.label}</span>
        </div>
        <div class="mono text-[11px] muted mt-0.5">{app.page.width}×{app.page.height}{#if app.page.detections}{` · ${app.page.detections} yazı`}{/if}</div>
        {#if err}<div class="mt-2 border-[1.5px] border-red text-red px-2 py-1.5 text-[12px] leading-4 select-text">{err}</div>{/if}
      </div>
    {/if}

    <Section title="Maske" id="mask">
      {#snippet aside()}<span class="mono text-[11px] font-semibold">{app.page?.detections ? `${app.page.detections} yazı` : '—'}</span>{/snippet}
      <div class="flex flex-col gap-2.5">
        <Scrub bind:value={app.opts.confidence} min={0.05} max={0.95} step={0.05} decimals={2} label="Güven eşiği" onchange={custom} tip={'düşük: daha çok yazı yakalar\nyüksek: daha az yanlış işaret'} />
        <div class="grid grid-cols-2 gap-2.5">
          <button class="btn" disabled={!app.page || app.jobActive} onclick={() => app.run('fetch', 'page')}><RefreshCw size={14} strokeWidth={2.25} /> Yeniden al</button>
          <button class="btn" disabled={!app.page || app.jobActive} onclick={() => app.run('fetch', 'chapter')}><ScanText size={14} strokeWidth={2.25} /> Tüm bölüm</button>
        </div>
      </div>
    </Section>

    <Section title="Temizleme" id="clean">
      {#snippet aside()}<span class="mono text-[11px] font-semibold">{model ? (model.kind === 'redraw' ? 'yeniden çizim' : 'hızlı silme') : ''}</span>{/snippet}
      <div class="flex flex-col gap-2.5">
        <Segmented value={app.opts.preset_id} full compact options={presetOpts} onchange={(v) => app.applyPreset(v as string)} />
        <Select bind:value={app.opts.model_id} options={modelOpts} onchange={custom} menuWidth="300px" align="right" aside={model ? fmtBytes(model.total_bytes) : ''}>
          {#snippet footer(close)}
            <button class="pop-item font-semibold" onclick={() => { close(); app.prefsTab = 'models'; app.dialog = 'prefs'; }}><Download size={14} /> Modelleri göster…</button>
          {/snippet}
        </Select>
        {#if model && (!model.installed || fitTxt[model.fit])}
          <div class="flex items-center gap-2 -mt-1 text-[12px]">
            {#if fitTxt[model.fit]}<span class="mono text-[11px] {model.fit === 'fits' ? '' : 'text-red'}">{fitTxt[model.fit]}</span>{/if}
            <span class="flex-1"></span>
            {#if !model.installed}<button class="font-semibold underline decoration-red decoration-2 underline-offset-2" onclick={() => { app.prefsTab = 'models'; app.dialog = 'prefs'; }}>İndir</button>{/if}
          </div>
        {/if}
        <div class="grid grid-cols-2 gap-2.5">
          <Scrub bind:value={app.opts.dilation} min={0} max={16} label="Genişlet" unit="px" onchange={custom} tip={'maskeyi her yöne büyütür\nyazı kenarı kalıyorsa artır'} />
          <Scrub bind:value={app.opts.feather} min={0} max={16} label="Yumuşat" unit="px" onchange={custom} tip="temizlenen alanın kenar geçişi" />
        </div>
      </div>
    </Section>

    <Section title="Katmanlar" id="layers" last>
      {#snippet aside()}<span class="mono text-[11px] font-semibold">4</span>{/snippet}
      <div class="flex flex-col">
        {#each layerRows as r}
          {@const l = app.layers[r.id]}
          {@const dis = r.id === 'result' && !app.hasClean}
          <div class="flex items-center gap-2.5 h-[34px] {dis ? 'opacity-35 pointer-events-none' : ''}">
            <button class="w-7 h-7 grid place-items-center hover:bg-ink hover:text-paper" aria-label="Görünürlük" onclick={() => { l.vis = !l.vis; save(); }}>
              {#if l.vis}<Eye size={16} strokeWidth={2} />{:else}<EyeOff size={16} strokeWidth={2} class="text-ink3" />{/if}
            </button>
            <span class="w-[22px] h-[14px] border-[1.5px] border-ink shrink-0" style={r.sw}></span>
            <span class="flex-1 truncate {l.vis ? '' : 'muted line-through'}">{r.name} <span class="mono text-[10.5px] muted no-underline">{r.hint}</span></span>
            <Scrub bind:value={l.op} min={0.05} max={1} step={0.05} percent width="58px" bare onchange={save} tip="opaklık" />
          </div>
        {/each}
      </div>
    </Section>
  </div>

  <div class="shrink-0 border-t-[2.5px] border-ink bg-paper p-3 flex flex-col gap-2.5">
    <button class="btn print !h-[54px] !justify-start !px-4 !gap-3" disabled={blocked} onclick={() => app.run('clean', 'page')}>
      <span class="whiteout"></span><span class="disp text-[22px] leading-none font-normal">Sayfayı temizle</span><span class="kbd ml-auto">CTRL ↵</span>
    </button>
    <button class="btn" disabled={blocked} onclick={() => app.run('clean', 'chapter')}>Bölümün tamamı <span class="mono font-medium opacity-60">· {app.pages.length} sayfa</span></button>
    {#if model && !model.installed}<div class="text-[12px] text-red text-center font-medium">Seçili model kurulu değil</div>{/if}
  </div>
</aside>

<style>
  /* daksil şişesi işareti: beyaz kare, kırmızı düzeltme çizgisi */
  .whiteout { width: 26px; height: 26px; border: 2px solid currentColor; background: #fff; position: relative; flex-shrink: 0; }
  .whiteout::after { content: ''; position: absolute; left: 3px; right: 3px; top: 9px; height: 4px; background: var(--color-red); transform: rotate(-20deg); }
</style>
