<script lang="ts">
  import { X } from 'lucide-svelte';
  import { app } from '../lib/store.svelte';
  const groups: { title: string; rows: [string, string][] }[] = [
    { title: 'Araçlar', rows: [['B', 'Fırça'], ['E', 'Silgi'], ['L', 'Kement'], ['H', 'El'], ['Z', 'Büyüteç'], ['[  ]', 'Fırça boyutu'], ['Ctrl + tekerlek', 'Fırça boyutu']] },
    { title: 'Görünüm', rows: [['M', 'Kalem izleri'], ['C', 'Maske / sonuç'], ['\\ basılı', 'Orijinali göster'], ['F · Ctrl 0', 'Sayfaya sığdır'], ['Ctrl 1', 'Genişliğe sığdır'], ['Ctrl 2', 'Gerçek boyut'], ['Boşluk + sürükle', 'Kaydır']] },
    { title: 'Sayfalar', rows: [['← →', 'Önceki / sonraki'], ['Ctrl A', 'Hepsini seç'], ['Ctrl tık · Shift tık', 'Çoklu seçim'], ['Del', 'Sayfayı kaldır'], ['Esc', 'Seçimi bırak'], ['Ctrl [', 'Sayfa paneli'], ['Tab', 'Denetçi']] },
    { title: 'İş', rows: [['Ctrl ↵', 'Sayfayı temizle'], ['Ctrl ⇧ ↵', 'Bölümü temizle'], ['Ctrl Z · Ctrl Y', 'Geri al / yinele'], ['Ctrl E', 'Dışa aktar'], ['Ctrl O', 'Kütüphane'], ['Ctrl ,', 'Tercihler'], ['?', 'Bu pencere']] },
  ];
  const close = () => (app.help = false);
</script>

<svelte:window onkeydown={(e) => { if (app.help && e.key === 'Escape') close(); }} />
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-[85] bg-paper/75 grid place-items-center fade-in" onclick={close}>
  <div class="slidein w-[860px] max-w-[94vw] max-h-[92vh] koma shadow-[8px_8px_0_var(--color-ink)] flex flex-col overflow-hidden" onclick={(e) => e.stopPropagation()}>
    <div class="h-[56px] shrink-0 bg-ink text-paper flex items-center px-5">
      <span class="disp text-[22px] leading-none">Kısayollar</span><div class="flex-1"></div>
      <button class="btn btn-icon btn-sm btn-bare !text-paper hover:!bg-paper hover:!text-ink" aria-label="Kapat" onclick={close}><X size={16} strokeWidth={2.5} /></button>
    </div>
    <div class="overflow-y-auto grid grid-cols-2 gap-[1.5px] bg-ink">
      {#each groups as g}
        <section class="bg-sheet px-5 py-4">
          <div class="disp text-[17px] leading-none mb-2.5">{g.title}</div>
          {#each g.rows as [k, t]}
            <div class="flex items-center gap-3 h-[28px] border-b border-dashed border-ink/20"><span class="kbd shrink-0">{k}</span><span class="ml-auto text-[12.5px]">{t}</span></div>
          {/each}
        </section>
      {/each}
    </div>
  </div>
</div>
