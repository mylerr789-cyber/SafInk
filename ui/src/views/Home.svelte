<script lang="ts">
  import { X, FolderOpen, Images, Plus, Trash2, Settings, Boxes, FolderInput, ArrowRight, Pencil, Download } from 'lucide-svelte';
  import { api, thumbUrl, type Manga, type Chapter } from '../lib/api';
  import { app } from '../lib/store.svelte';
  import { importChapterFolder, quickFromPicker } from '../lib/actions';
  import Logo from '../ui/Logo.svelte';
  import Stamp from '../ui/Stamp.svelte';

  interface Card { ch: Chapter; manga: string; thumb: number | null }
  let mangas = $state<Manga[]>([]);
  let sel = $state<number | 'recent'>('recent');
  let cards = $state<Card[]>([]);
  let newName = $state('');
  let adding = $state(false);
  let renId = $state<number | null>(null);
  let renDraft = $state('');
  const noModel = $derived(app.models.length > 0 && !app.models.some((m) => m.installed));
  async function commitRename(c: Card) {
    if (renId !== c.ch.id) return;
    renId = null;
    const n = renDraft.trim();
    if (n && n !== c.ch.name && (await app.renameChapter(c.ch.id, n))) { c.ch.name = n; if (app.project?.chapterId === c.ch.id) app.project.name = n; }
  }

  async function loadMangas() { mangas = (await api.listManga()).filter((m) => m.name !== '__quick__'); }
  async function loadCards() {
    let list: { ch: Chapter; manga: string }[] = [];
    if (sel === 'recent') {
      for (const m of mangas) for (const c of await api.listChapters(m.id)) list.push({ ch: c, manga: m.name });
      list.sort((a, b) => b.ch.id - a.ch.id); list = list.slice(0, 12);
    } else {
      const m = mangas.find((x) => x.id === sel);
      if (m) list = (await api.listChapters(m.id)).map((c) => ({ ch: c, manga: m.name }));
    }
    cards = list.map((x) => ({ ...x, thumb: null }));
    cards.forEach((c, i) => api.listPages(c.ch.id).then((p) => { if (cards[i]) cards[i].thumb = p[0]?.id ?? null; }));
  }
  $effect(() => { app.tick; loadMangas(); });
  $effect(() => { sel; mangas; loadCards(); });

  async function addManga() {
    if (!newName.trim()) return;
    const id = await api.createManga(newName.trim());
    newName = ''; adding = false; await loadMangas(); sel = id;
  }
  async function delChapter(c: Card, e: Event) {
    e.stopPropagation();
    if (!(await app.ask(`"${c.ch.name}" kütüphaneden kaldırılsın? Orijinal dosyalara dokunulmaz.`))) return;
    if (app.project?.chapterId === c.ch.id) { app.project = null; app.pages = []; }
    await api.deleteChapter(c.ch.id); await loadMangas(); await loadCards();
  }
  async function delManga(m: Manga) {
    if (!(await app.ask(`"${m.name}" ve tüm bölümleri kütüphaneden kaldırılsın? Orijinal dosyalara dokunulmaz.`))) return;
    await api.deleteManga(m.id); if (sel === m.id) sel = 'recent'; await loadMangas();
  }
  const curManga = $derived(typeof sel === 'number' ? mangas.find((m) => m.id === sel) : undefined);
  function close() { if (app.project) app.dialog = null; }
  const pad = (n: number) => String(n).padStart(2, '0');
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && close()} />

<div class="fixed inset-0 z-[75] bg-paper p-[10px] grid gap-[10px] fade-in" style="grid-template-columns:250px minmax(0,1fr); grid-template-rows:minmax(0,1fr)">
  <!-- kütüphane rafı -->
  <aside class="koma flex flex-col overflow-hidden">
    <div class="h-[64px] shrink-0 bg-ink text-paper flex items-center px-4" data-tauri-drag-region><Logo size={30} /></div>
    <button class="h-[44px] shrink-0 px-3.5 flex items-center gap-2 border-y-[2.5px] border-ink text-left {sel === 'recent' ? 'bg-ink text-paper' : 'hover:bg-desk'}" onclick={() => (sel = 'recent')}>
      <span class="disp text-[18px] leading-none">Masa</span><span class="mono text-[11px] opacity-60 ml-auto">son bölümler</span>
    </button>
    <div class="h-[40px] shrink-0 px-3.5 flex items-center tone-light border-b-[1.5px] border-ink">
      <span class="disp text-[17px] leading-none bg-sheet px-1.5 py-1 -ml-1.5">Raf</span>
      <span class="mono text-[11px] bg-sheet px-1 ml-2">{pad(mangas.length)}</span>
      <div class="flex-1"></div>
      <button class="btn btn-sm btn-icon !h-6 !w-6" data-tip="Yeni manga" aria-label="Yeni manga" onclick={() => (adding = !adding)}><Plus size={14} strokeWidth={2.5} /></button>
    </div>
    {#if adding}
      <div class="p-2.5 border-b-[1.5px] border-ink slidein">
        <!-- svelte-ignore a11y_autofocus -->
        <input class="field" placeholder="Manga adı, Enter" bind:value={newName} autofocus onkeydown={(e) => { if (e.key === 'Enter') addManga(); if (e.key === 'Escape') { adding = false; e.stopPropagation(); } }} />
      </div>
    {/if}
    <div class="flex-1 min-h-0 overflow-y-auto">
      {#each mangas as m (m.id)}
        <div class="group flex items-stretch border-b-[1.5px] border-ink/15 {sel === m.id ? 'bg-ink text-paper' : 'hover:bg-desk'}">
          <button class="flex-1 min-w-0 py-2.5 pl-3.5 pr-2 text-left" onclick={() => (sel = m.id)}>
            <div class="font-semibold truncate leading-tight">{m.name}</div>
            <div class="mono text-[10.5px] opacity-60 mt-0.5">{pad(m.chapters)} bölüm</div>
          </button>
          <button class="hidden group-hover:grid place-items-center w-9 hover:text-red" aria-label="Kaldır" data-tip="Raftan kaldır" onclick={() => delManga(m)}><Trash2 size={14} /></button>
        </div>
      {/each}
      {#if !mangas.length}<div class="text-[12.5px] muted p-3.5 leading-5">Raf boş. Bir bölüm klasörü içe aktarın; manga adı klasörden alınır.</div>{/if}
    </div>
    <div class="shrink-0 border-t-[2.5px] border-ink grid grid-cols-2">
      <button class="h-[44px] flex items-center justify-center gap-2 font-semibold border-r-[1.5px] border-ink hover:bg-ink hover:text-paper" onclick={() => { app.prefsTab = 'models'; app.dialog = 'prefs'; }}><Boxes size={15} /> Modeller</button>
      <button class="relative h-[44px] flex items-center justify-center gap-2 font-semibold hover:bg-ink hover:text-paper" onclick={() => { app.prefsTab = 'clean'; app.dialog = 'prefs'; }}><Settings size={15} /> Tercihler</button>
    </div>
    {#if app.hw?.gpu_name}
      <div class="shrink-0 border-t-[1.5px] border-ink px-3.5 py-2 mono text-[10.5px] leading-4 bg-paper">
        <div class="truncate font-semibold">{app.hw.gpu_name}</div>
        <div class="muted">{app.hw.vram_gb.toFixed(0)} GB VRAM · {app.hw.ram_gb.toFixed(0)} GB RAM</div>
      </div>
    {/if}
  </aside>

  <!-- içerik -->
  <main class="koma overflow-y-auto bg-desk">
    {#if app.project}
      <button class="btn btn-icon print absolute top-4 right-4 z-10 !h-10 !w-10" aria-label="Kapat" data-tip="Masaya dön  Esc" onclick={close}><X size={18} strokeWidth={2.5} /></button>
    {/if}
    <div class="max-w-[1120px] px-8 pt-7 pb-12">
      {#if noModel}
        <div class="koma !border-red mb-[18px] flex items-center gap-4 pl-5 pr-3 h-[68px] shadow-[5px_5px_0_var(--color-red)]">
          <span class="tag tag-red">İLK ADIM</span>
          <span class="font-semibold text-[14px]">Temizlemek için önce bir model indir.</span>
          <div class="flex-1"></div>
          <button class="btn print btn-ink" onclick={() => { app.prefsTab = 'models'; app.dialog = 'prefs'; }}><Download size={15} strokeWidth={2.5} /> Model indir</button>
        </div>
      {/if}
      {#if sel === 'recent'}
        <!-- iki giriş karesi: manga sayfasındaki iki koma gibi -->
        <div class="grid grid-cols-[1.4fr_1fr] gap-[10px] mb-9">
          <div class="koma p-5 {app.dragging ? '!border-dashed' : ''}">
            <div class="absolute inset-y-0 right-0 w-[38%] tone-light border-l-[1.5px] border-ink"></div>
            <div class="relative">
              <div class="disp text-[34px] leading-[0.95]">Hızlı<br />temizlik</div>
              <p class="mt-3 max-w-[300px] leading-5">Sayfaları ya da bir klasörü pencereye bırakın. Kütüphaneye eklemeden doğrudan masaya açılır.</p>
              <div class="flex gap-3 mt-5">
                <button class="btn print btn-ink" onclick={() => quickFromPicker('files')}><Images size={16} /> Sayfa seç</button>
                <button class="btn print" onclick={() => quickFromPicker('folder')}><FolderOpen size={16} /> Klasör seç</button>
              </div>
            </div>
            <div class="absolute right-6 top-1/2 -translate-y-1/2 disp text-[15px] bg-sheet border-[2.5px] border-ink px-3 py-2 rotate-[4deg] shadow-[4px_4px_0_var(--color-ink)]">Sürükle → bırak</div>
          </div>
          <button class="koma p-5 text-left flex flex-col group hover:bg-ink hover:text-paper transition-colors" onclick={() => importChapterFolder()}>
            <FolderInput size={26} strokeWidth={2} />
            <div class="disp text-[34px] leading-[0.95] mt-3">Bölüm<br />içe aktar</div>
            <p class="mt-3 leading-5 opacity-80">Bölüm klasörü ya da bölümleri içeren manga klasörü. Raf'a eklenir.</p>
            <div class="mt-auto pt-3 font-semibold flex items-center gap-1.5">Klasör seç <ArrowRight size={15} class="transition-transform group-hover:translate-x-1" /></div>
          </button>
        </div>
      {:else if curManga}
        <div class="flex items-end gap-4 mb-7">
          <div class="min-w-0">
            <div class="eyebrow mb-1.5">Raf / {pad(curManga.chapters)} bölüm</div>
            <h1 class="disp text-[42px] leading-[0.95] truncate">{curManga.name}</h1>
          </div>
          <div class="flex-1"></div>
          <button class="btn print shrink-0" onclick={() => importChapterFolder(curManga.id, curManga.name)}><FolderInput size={16} /> Bölüm ekle</button>
        </div>
      {/if}

      <div class="flex items-center gap-3 mb-4">
        <h2 class="disp text-[22px] leading-none">{sel === 'recent' ? 'Masadaki bölümler' : 'Bölümler'}</h2>
        <span class="mono text-[12px]">{pad(cards.length)}</span>
        <span class="flex-1 border-t-[2.5px] border-ink"></span>
      </div>
      {#if !cards.length}
        <div class="border-[2.5px] border-dashed border-ink/50 py-12 text-center">
          <div class="font-semibold">Burada henüz bölüm yok.</div>
          <div class="mono text-[11.5px] muted mt-1">bölüm içe aktarın ya da sayfaları pencereye bırakın</div>
        </div>
      {/if}
      <div class="grid gap-x-6 gap-y-7" style="grid-template-columns:repeat(auto-fill,minmax(150px,1fr))">
        {#each cards as c (c.ch.id)}
          {@const done = c.ch.pages > 0 && c.ch.cleaned === c.ch.pages}
          <div class="group relative text-left cursor-pointer" role="button" tabindex="0"
            onclick={() => app.openChapter(c.ch.id, c.ch.name, c.manga)} onkeydown={(e) => e.key === 'Enter' && app.openChapter(c.ch.id, c.ch.name, c.manga)}>
            <div class="relative bg-white border-[2px] border-ink aspect-[3/4] overflow-hidden shadow-[5px_5px_0_var(--color-ink)] transition-[transform,box-shadow] duration-100 group-hover:-translate-x-0.5 group-hover:-translate-y-0.5 group-hover:shadow-[8px_8px_0_var(--color-ink)]">
              {#if c.thumb}<img src={thumbUrl(c.thumb, 1)} alt="" class="w-full h-full object-cover object-top" draggable="false" />{/if}
              <button class="absolute top-0 left-0 w-8 h-8 grid place-items-center bg-ink text-paper hover:bg-red opacity-0 group-hover:opacity-100" aria-label="Kaldır" data-tip="Raftan kaldır" onclick={(e) => delChapter(c, e)}><Trash2 size={14} /></button>
              <button class="absolute top-0 left-8 w-8 h-8 grid place-items-center bg-ink text-paper hover:bg-blue opacity-0 group-hover:opacity-100 border-l border-paper/30" aria-label="Adını değiştir" data-tip="Adını değiştir" onclick={(e) => { e.stopPropagation(); renId = c.ch.id; renDraft = c.ch.name; }}><Pencil size={13} /></button>
            </div>
            {#if done}<span class="absolute -right-3 top-3"><Stamp size={54} /></span>{/if}
            <!-- ilerleme: temiz sayfa sayısı, mürekkep dolgulu şerit -->
            <div class="mt-3 h-[9px] border-[1.5px] border-ink bg-sheet relative"><span class="absolute inset-y-0 left-0 {done ? 'bg-ink' : 'tone'}" style="width:{c.ch.pages ? (c.ch.cleaned / c.ch.pages) * 100 : 0}%"></span></div>
            <div class="mt-1.5 flex items-baseline gap-2">
              {#if renId === c.ch.id}
                <!-- svelte-ignore a11y_autofocus, a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <input class="field !h-[24px] !px-1.5 flex-1 min-w-0 font-semibold" bind:value={renDraft} autofocus onfocus={(e) => (e.currentTarget as HTMLInputElement).select()} onclick={(e) => e.stopPropagation()} onblur={() => commitRename(c)}
                  onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Enter') commitRename(c); if (e.key === 'Escape') renId = null; }} />
              {:else}
                <span class="font-semibold truncate flex-1">{c.ch.name}</span>
              {/if}
              <span class="mono text-[11px] shrink-0">{c.ch.cleaned}/{c.ch.pages}</span>
            </div>
            {#if sel === 'recent'}<div class="text-[11.5px] muted truncate">{c.manga}</div>{/if}
          </div>
        {/each}
      </div>
    </div>
  </main>
</div>
