<script lang="ts">
  import { Download, Check, X, ChevronRight, Trash2, ExternalLink, RotateCcw } from 'lucide-svelte';
  import { api, fmtBytes, modelDesc, type Model } from '../lib/api';
  import { app } from '../lib/store.svelte';
  import { t, i18n } from '../lib/i18n/index.svelte';

  const groups = $derived([
    { kind: 'inpaint', title: t('models.g_inpaint'), sub: t('models.g_inpaint_sub') },
    { kind: 'redraw', title: t('models.g_redraw'), sub: t('models.g_redraw_sub') },
  ]);
  const fit = $derived<Record<string, string>>({
    fits: t('fit.fits'), tight: t('fit.tight'), no: t('fit.no'), slow: t('fit.slow'), lowvram: t('fit.lowvram'), more: t('fit.more'),
  });
  let open = $state<Record<string, boolean>>({});

  const dls = (m: Model) => app.dl[m.id];
  const busy = (m: Model) => { const d = dls(m); return !!d && ['downloading', 'verifying', 'extracting'].includes(d.stage); };
  const failed = (m: Model) => dls(m)?.stage === 'error' && !m.installed;
  const pct = (m: Model) => { const d = dls(m); return d && d.total ? Math.min(99, Math.round((d.done / d.total) * 100)) : 0; };
  const isSel = (m: Model) => app.settings?.model_id === m.id;

  async function install(m: Model) {
    try {
      app.dl[m.id] = { stage: 'downloading', done: 0, total: 1, file: '', phase: '' };
      await api.installModel(m.id);
    } catch (e) { delete app.dl[m.id]; app.toast(String(e), 'err'); }
  }
  async function remove(m: Model) {
    if (!(await app.ask(t('models.ask_remove', { name: m.name, size: fmtBytes(m.total_bytes) })))) return;
    try { await api.removeModel(m.id); delete app.dl[m.id]; await app.reloadModels(); } catch (e) { app.toast(String(e), 'err'); }
  }
  async function use(m: Model) {
    if (!app.settings) return;
    app.settings.model_id = m.id;
    await api.saveSettings($state.snapshot(app.settings));
    app.opts.model_id = m.id; app.saveOpts();
    app.toast(t('models.using', { name: m.name }), 'ok');
  }
  // kartta görünen teknik künye: motor, bu bilgisayarda nerede çalışır, nicemleme, bellek, adım, hız/kalite
  const gpu = $derived(app.hw?.gpu_kind ?? 'none');
  const quantName = (q: string) => (i18n.locale === 'en' ? q.replace('dinamik', 'dynamic') : q);
  function specs(m: Model): [string, string][] {
    const p: any = (m as any).params ?? {};
    const vram = m.min_vram_gb ? `≥ ${m.min_vram_gb} GB` : '—';
    const ram = m.min_ram_gb ? `≥ ${m.min_ram_gb} GB` : '—';
    if (m.backend === 'lama') {
      const onGpu = gpu === 'nvidia';
      return [
        [t('spec.engine'), onGpu ? 'PyTorch · CUDA' : 'ONNX Runtime'],
        [t('spec.on_pc'), onGpu ? t('spec.gpu') : t('spec.cpu')],
        [t('spec.precision'), 'FP32'],
        [t('spec.operation'), t('spec.window')],
        ['VRAM', onGpu ? vram : '—'],
        ['RAM', ram],
      ];
    }
    if (m.backend === 'sdcpp') {
      const where = gpu === 'nvidia' ? 'GPU · CUDA' : gpu === 'other' ? 'GPU · Vulkan' : t('spec.cpu_slow');
      return [
        [t('spec.engine'), 'sd.cpp'],
        [t('spec.on_pc'), where],
        [t('spec.quant'), p.quant ?? 'GGUF Q4_K_M'],
        [t('spec.steps'), String(p.steps ?? '—')],
        ['VRAM', vram],
        ['RAM', ram],
      ];
    }
    if (m.backend === 'sdnq') {
      return [
        [t('spec.engine'), 'SDNQ · diffusers'],
        [t('spec.on_pc'), 'GPU · CUDA'],
        [t('spec.quant'), quantName(p.quant ?? '4-bit dinamik')],
        [t('spec.steps'), String(p.steps ?? '—')],
        ['VRAM', vram],
        ['RAM', ram],
      ];
    }
    return [];
  }

  const usedBytes = $derived(app.models.filter((m) => m.installed).reduce((n, m) => n + m.total_bytes, 0));
  const gpuLine = $derived(
    !app.hw ? '' : app.hw.gpu_kind === 'none' ? t('models.no_gpu') : app.hw.gpu_name,
  );
</script>

<div class="flex flex-col gap-8">
  <div class="flex items-center gap-3 flex-wrap">
    {#if gpuLine}<span class="tag tag-ink !h-[24px] !px-2 mono">{gpuLine}</span>{/if}
    {#if usedBytes}<span class="mono text-[11.5px] muted">{t('models.on_disk', { size: fmtBytes(usedBytes) })}</span>{/if}
  </div>

  {#each groups as g}
    {@const list = app.models.filter((m) => m.kind === g.kind)}
    {#if list.length}
      <section>
        <div class="flex items-baseline gap-3 mb-3">
          <span class="disp text-[22px] leading-none">{g.title}</span>
          <span class="mono text-[11px] muted truncate">{g.sub}</span>
          <span class="flex-1 border-t-[2px] border-ink self-center"></span>
        </div>
        <div class="flex flex-col gap-4">
          {#each list as m (m.id)}
            <div class="bg-sheet border-[2.5px] border-ink px-4 py-3.5 {isSel(m) && m.installed ? 'shadow-[5px_5px_0_var(--color-ink)]' : ''}">
              <div class="flex items-center gap-5">
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2 flex-wrap">
                    <span class="disp text-[21px] leading-none">{m.name}</span>
                    {#if m.recommended && !m.installed}<span class="tag tag-red">{t('models.recommended')}</span>{/if}
                  </div>
                  <div class="text-[13.5px] mt-1.5 leading-[1.4]">{modelDesc(m)}</div>
                  <div class="flex items-center gap-x-3 flex-wrap mono text-[11.5px] mt-2">
                    <span class="font-semibold">{m.installed ? t('models.on_disk', { size: fmtBytes(m.total_bytes) }) : t('models.to_download', { size: fmtBytes(m.download_bytes || m.total_bytes) })}</span>
                    {#if fit[m.fit]}<span class="{m.fit === 'fits' || m.fit === 'lowvram' ? 'muted' : 'text-red'}">{fit[m.fit]}</span>{/if}
                    <button class="muted hover:text-ink inline-flex items-center" onclick={() => (open[m.id] = !open[m.id])}>
                      <ChevronRight size={12} class="transition-transform {open[m.id] ? 'rotate-90' : ''}" />{t('models.details')}
                    </button>
                  </div>
                </div>

                <div class="shrink-0 w-[190px] flex flex-col items-stretch gap-1.5">
                  {#if busy(m)}
                    <div class="flex justify-between mono text-[12px] font-semibold"><span>{t('models.installing')}</span><span>%{pct(m)}</span></div>
                    <div class="h-[14px] border-2 border-ink relative"><span class="tone absolute inset-y-0 left-0 transition-[width] duration-300" style="width:{pct(m)}%"></span></div>
                    <div class="flex items-center justify-between">
                      <span class="mono text-[10.5px] muted truncate">{dls(m)?.phase}</span>
                      <button class="mono text-[11px] underline hover:text-red inline-flex items-center gap-0.5" onclick={() => api.cancelInstall(m.id)}><X size={11} />{t('models.cancel')}</button>
                    </div>
                  {:else if m.installed}
                    {#if isSel(m)}
                      <div class="btn !h-[48px] !cursor-default !bg-ink !text-paper justify-center pointer-events-none"><Check size={16} strokeWidth={3} /> <span class="disp text-[19px] leading-none font-normal">{t('models.selected')}</span></div>
                    {:else}
                      <button class="btn print !h-[48px] justify-center" onclick={() => use(m)}><span class="disp text-[19px] leading-none font-normal">{t('models.use')}</span></button>
                    {/if}
                  {:else if failed(m)}
                    <button class="btn print btn-ink !h-[48px] justify-center" onclick={() => install(m)}><RotateCcw size={15} strokeWidth={2.5} /> <span class="disp text-[19px] leading-none font-normal">{t('models.retry')}</span></button>
                  {:else}
                    <button class="btn print btn-ink !h-[48px] justify-center" onclick={() => install(m)}><Download size={16} strokeWidth={2.5} /> <span class="disp text-[19px] leading-none font-normal">{t('common.download')}</span></button>
                  {/if}
                </div>
              </div>

              {#if specs(m).length}
                <div class="mt-3 grid grid-cols-3 gap-[1.5px] bg-ink border-[1.5px] border-ink">
                  {#each specs(m) as [k, v]}
                    <div class="bg-sheet px-2.5 py-1.5 flex items-baseline gap-2">
                      <span class="eyebrow !text-[9.5px] w-[68px] shrink-0">{k}</span>
                      <span class="mono text-[11.5px] font-semibold truncate" title={v}>{v}</span>
                    </div>
                  {/each}
                </div>
              {/if}

              {#if failed(m)}<div class="text-[12px] text-red mt-2 leading-4 select-text">{t('models.install_failed', { msg: dls(m)?.file ?? '' })}</div>{/if}

              {#if open[m.id]}
                <div class="mt-3 border-[1.5px] border-ink bg-paper px-3 py-2 mono text-[11px] leading-[1.7] flex items-center gap-4 flex-wrap">
                  {#if m.license}<a class="hover:text-ink underline inline-flex items-center gap-1" href={m.license_url} target="_blank" rel="noreferrer">{t('models.license', { name: m.license })}<ExternalLink size={10} /></a>{/if}
                  <span class="flex-1"></span>
                  {#if m.installed}<button class="btn btn-sm btn-danger" onclick={() => remove(m)}><Trash2 size={13} /> {t('models.remove_disk')}</button>{/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      </section>
    {/if}
  {/each}
</div>
