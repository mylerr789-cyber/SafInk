<script lang="ts">
  import { Download, Check, X, ChevronRight, Trash2, ExternalLink, RotateCcw } from 'lucide-svelte';
  import { api, fmtBytes, type Model } from '../lib/api';
  import { app } from '../lib/store.svelte';

  const groups = [
    { kind: 'inpaint', title: 'Hızlı silme', sub: 'Saniyeler içinde; düz arka planlar için' },
    { kind: 'redraw', title: 'Yeniden çizim', sub: 'Yavaş ama desenli arka planı yeniden çizer' },
  ];
  const fit: Record<string, string> = {
    fits: 'Bilgisayarına uygun', tight: 'Biraz zorlanabilir', no: 'Ekran kartın yetmeyebilir', slow: 'Ekran kartı yok, yavaş çalışır',
  };
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
    if (!confirm(`"${m.name}" silinsin? ${fmtBytes(m.total_bytes)} yer açılır.`)) return;
    try { await api.removeModel(m.id); delete app.dl[m.id]; await app.reloadModels(); } catch (e) { app.toast(String(e), 'err'); }
  }
  async function use(m: Model) {
    if (!app.settings) return;
    app.settings.model_id = m.id;
    await api.saveSettings($state.snapshot(app.settings));
    app.opts.model_id = m.id; app.saveOpts();
    app.toast(`Kullanılacak model: ${m.name}`, 'ok');
  }
  const usedBytes = $derived(app.models.filter((m) => m.installed).reduce((n, m) => n + m.total_bytes, 0));
  const gpuLine = $derived(
    !app.hw ? '' : app.hw.gpu_kind === 'none' ? 'Ekran kartı bulunamadı: yeniden çizim yavaş olur, hızlı silme sorunsuz çalışır.' : app.hw.gpu_name,
  );
</script>

<div class="flex flex-col gap-8">
  <div class="flex items-center gap-3 flex-wrap">
    {#if gpuLine}<span class="tag tag-ink !h-[24px] !px-2 mono">{gpuLine}</span>{/if}
    {#if usedBytes}<span class="mono text-[11.5px] muted">{fmtBytes(usedBytes)} diskte</span>{/if}
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
                    {#if m.recommended && !m.installed}<span class="tag tag-red">ÖNERİLEN</span>{/if}
                  </div>
                  <div class="text-[13.5px] mt-1.5 leading-[1.4]">{m.description}</div>
                  <div class="flex items-center gap-x-3 flex-wrap mono text-[11.5px] mt-2">
                    <span class="font-semibold">{m.installed ? `${fmtBytes(m.total_bytes)} diskte` : `${fmtBytes(m.download_bytes || m.total_bytes)} indirilir`}</span>
                    {#if fit[m.fit]}<span class="{m.fit === 'fits' ? 'muted' : 'text-red'}">{fit[m.fit]}</span>{/if}
                    <button class="muted hover:text-ink inline-flex items-center" onclick={() => (open[m.id] = !open[m.id])}>
                      <ChevronRight size={12} class="transition-transform {open[m.id] ? 'rotate-90' : ''}" />ayrıntı
                    </button>
                  </div>
                </div>

                <div class="shrink-0 w-[190px] flex flex-col items-stretch gap-1.5">
                  {#if busy(m)}
                    <div class="flex justify-between mono text-[12px] font-semibold"><span>Kuruluyor</span><span>%{pct(m)}</span></div>
                    <div class="h-[14px] border-2 border-ink relative"><span class="tone absolute inset-y-0 left-0 transition-[width] duration-300" style="width:{pct(m)}%"></span></div>
                    <div class="flex items-center justify-between">
                      <span class="mono text-[10.5px] muted truncate">{dls(m)?.phase}</span>
                      <button class="mono text-[11px] underline hover:text-red inline-flex items-center gap-0.5" onclick={() => api.cancelInstall(m.id)}><X size={11} />iptal</button>
                    </div>
                  {:else if m.installed}
                    {#if isSel(m)}
                      <div class="btn !h-[48px] !cursor-default !bg-ink !text-paper justify-center pointer-events-none"><Check size={16} strokeWidth={3} /> <span class="disp text-[19px] leading-none font-normal">Seçili</span></div>
                    {:else}
                      <button class="btn print !h-[48px] justify-center" onclick={() => use(m)}><span class="disp text-[19px] leading-none font-normal">Kullan</span></button>
                    {/if}
                  {:else if failed(m)}
                    <button class="btn print btn-ink !h-[48px] justify-center" onclick={() => install(m)}><RotateCcw size={15} strokeWidth={2.5} /> <span class="disp text-[19px] leading-none font-normal">Tekrar dene</span></button>
                  {:else}
                    <button class="btn print btn-ink !h-[48px] justify-center" onclick={() => install(m)}><Download size={16} strokeWidth={2.5} /> <span class="disp text-[19px] leading-none font-normal">İndir</span></button>
                  {/if}
                </div>
              </div>

              {#if failed(m)}<div class="text-[12px] text-red mt-2 leading-4 select-text">Kurulum yapılamadı: {dls(m)?.file}</div>{/if}

              {#if open[m.id]}
                <div class="mt-3 border-[1.5px] border-ink bg-paper px-3 py-2 mono text-[11px] leading-[1.7] flex items-center gap-4 flex-wrap">
                  {#if m.license}<a class="hover:text-ink underline inline-flex items-center gap-1" href={m.license_url} target="_blank" rel="noreferrer">Lisans: {m.license}<ExternalLink size={10} /></a>{/if}
                  <span class="flex-1"></span>
                  {#if m.installed}<button class="btn btn-sm btn-danger" onclick={() => remove(m)}><Trash2 size={13} /> Diskten sil</button>{/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      </section>
    {/if}
  {/each}
</div>
