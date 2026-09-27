<script>
  import { session, resolvePath } from '../../lib/cloud.svelte.js';
  import {
    videos, catalogue, openVideos, setVideosRoot, scanVideos, readAllTags, stopReading, details, moviePoster, loadPoster, posters, setPoster,
    renameSeries, renameFiles, progressOf, isWatched, setWatched, continueWatching, nextUp, seriesOf,
  } from '../../lib/videos.svelte.js';
  import { imageError } from '../../lib/cover.js';
  import { pad } from '../../lib/episodes.js';
  import { toast, toastError } from '../../lib/ui.svelte.js';
  import { plural } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import Menu from '../Menu.svelte';
  import VideoPlayer from '../VideoPlayer.svelte';
  import FolderPickDialog from '../dialogs/FolderPickDialog.svelte';
  import VideoEditDialog from '../dialogs/VideoEditDialog.svelte';
  import NameDialog from '../dialogs/NameDialog.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';

  let { series: seriesKey = null, play: playId = null, go } = $props();

  let tab = $state('series'); // series | movies
  let query = $state('');
  let season = $state(null);
  let picking = $state(null);
  let dialog = $state.raw(null); // { type, ... }
  let posterFor = null;
  let posterInput;

  openVideos();

  const cat = $derived(catalogue.value);
  const series = $derived(seriesKey && cat?.series.find((s) => s.key === seriesKey));
  const playing = $derived(playId && cat?.items.find((v) => v.id === playId));
  const q = $derived(query.trim().toLowerCase());
  const matches = (v) => {
    const d = details(v);
    return `${d.label} ${d.series} ${v.entry.meta.name}`.toLowerCase().includes(q);
  };
  const seriesList = $derived(!cat ? [] : q ? cat.series.filter((s) => s.name.toLowerCase().includes(q) || s.episodes.some(matches)) : cat.series);
  const movies = $derived(!cat ? [] : q ? cat.movies.filter(matches) : cat.movies);
  const resume = $derived(cat && !q ? continueWatching() : []);
  const seasonShown = $derived(series && (series.seasons.includes(season) ? season : details(nextUp(series)).season));
  const episodes = $derived(series ? series.episodes.filter((v) => details(v).season === seasonShown) : []);

  async function choose() {
    try {
      picking = (await resolvePath(session.me.keys.root_node_id)).items[0];
    } catch (e) {
      toastError(e);
    }
  }

  const play = (v) => go({ name: 'videos', series: seriesKey, play: v.id });
  const openSeries = (s) => go({ name: 'videos', series: s.key });
  const seasonName = (n) => (n === 0 ? 'Specials' : `Season ${n}`);
  const code = (d) => `S${pad(d.season)}E${pad(d.episode)}`;

  function pickPoster(target) {
    posterFor = target;
    posterInput.click();
  }

  async function onPoster(e) {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = '';
    if (!file || !posterFor) return;
    try {
      await setPoster(posterFor, file);
      toast('Poster saved', { kind: 'success' });
    } catch (err) {
      toastError(imageError(err));
    }
  }

  async function act(fn) {
    try {
      await fn();
    } catch (e) {
      toastError(e);
    }
  }

  async function readTags() {
    try {
      await readAllTags();
      toast('Read the episode details in your videos');
    } catch (e) {
      toastError(e);
    }
  }

  function videoMenu(v) {
    const watched = isWatched(v);
    const s = seriesOf(v);
    return [
      { label: 'Play', icon: 'play', onclick: () => play(v) },
      { label: watched ? 'Mark as not watched' : 'Mark as watched', icon: watched ? 'eye-off' : 'eye', onclick: () => act(() => setWatched([v], !watched)) },
      'sep',
      { label: 'Edit details', icon: 'pencil', onclick: () => (dialog = { type: 'edit', video: v }) },
      ...(s ? [] : [{ label: 'Set poster', icon: 'image', onclick: () => pickPoster(v) }]),
      { label: 'Show in files', icon: 'folder-open', onclick: () => go({ name: 'files', folderId: v.parentId }) },
    ];
  }

  /** Svelte action: call `fn` once, when the element first scrolls into view. */
  function onVisible(node, fn) {
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && (fn(), io.disconnect()), { rootMargin: '200px' });
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }
</script>

{#snippet poster(entry, icon, cls = '')}
  {@const url = entry && posters.get(entry.node.id)}
  <div class="grid aspect-[2/3] place-items-center overflow-hidden rounded-md border border-line bg-muted text-fg-faint {cls}" use:onVisible={() => loadPoster(entry)}>
    {#if url}<img src={url} alt="" class="size-full object-cover" draggable="false" />{:else}<Icon name={icon} class="size-1/4" strokeWidth={1.5} />{/if}
  </div>
{/snippet}

{#snippet bar(v)}
  {@const p = progressOf(v)}
  {#if p > 0 && p < 1}<div class="progress absolute inset-x-2 bottom-2"><div style:width="{p * 100}%"></div></div>{/if}
{/snippet}

{#snippet card(title, sub, entry, icon, open, menu, v = null)}
  <div class="group min-w-0">
    <div class="relative">
      <button type="button" class="block w-full cursor-pointer" aria-label={title} onclick={open}>
        {@render poster(entry, icon, 'transition-colors group-hover:border-line-strong')}
      </button>
      {#if v}
        {@render bar(v)}
        {#if isWatched(v)}<span class="badge absolute top-2 right-2 bg-bg"><Icon name="check" class="size-3" />Watched</span>{/if}
      {/if}
    </div>
    <div class="mt-2 flex items-start gap-1">
      <button type="button" class="min-w-0 flex-1 cursor-pointer text-left" tabindex="-1" onclick={open}>
        <span class="block truncate text-sm font-medium">{title}</span>
        <span class="block truncate text-xs text-fg-muted">{sub}</span>
      </button>
      {#if menu}<Menu label="{title} actions" items={menu} buttonClass="btn btn-ghost btn-icon -mr-1 size-7" />{/if}
    </div>
  </div>
{/snippet}

<input bind:this={posterInput} type="file" accept="image/*" class="hidden" onchange={onPoster} />

{#if series}
  {@const up = nextUp(series)}
  {@const upD = details(up)}
  <button type="button" class="btn btn-ghost -ml-3 mb-4" onclick={() => history.back()}><Icon name="arrow-left" />Videos</button>
  <div class="flex flex-col gap-5 sm:flex-row sm:items-end">
    {@render poster(series.poster, 'tv', 'w-36 shrink-0 sm:w-44')}
    <div class="grid min-w-0 gap-1">
      <p class="text-xs font-medium text-fg-muted">Series</p>
      <h1 class="truncate text-2xl font-semibold tracking-tight">{series.name}</h1>
      <p class="text-[13px] text-fg-muted">{plural(series.seasons.length, 'season')} · {plural(series.episodes.length, 'episode')}</p>
      <div class="mt-3 flex items-center gap-2">
        <button type="button" class="btn btn-accent min-w-0" onclick={() => play(up)}>
          <Icon name="play" /><span class="truncate">{progressOf(up) > 0 && !isWatched(up) ? 'Resume' : 'Play'} {code(upD)}</span>
        </button>
        <Menu
          label="Series actions"
          items={[
            { label: 'Rename series', icon: 'pencil', onclick: () => (dialog = { type: 'rename', series }) },
            { label: 'Set poster', icon: 'image', onclick: () => pickPoster(series) },
            { label: 'Rename files', icon: 'file-video', onclick: () => (dialog = { type: 'files', series }) },
            'sep',
            { label: 'Mark all as watched', icon: 'eye', onclick: () => act(() => setWatched(series.episodes, true)) },
            { label: 'Mark all as not watched', icon: 'eye-off', onclick: () => act(() => setWatched(series.episodes, false)) },
            ...(series.folderId ? ['sep', { label: 'Show in files', icon: 'folder-open', onclick: () => go({ name: 'files', folderId: series.folderId }) }] : []),
          ]} />
      </div>
    </div>
  </div>

  {#if series.seasons.length > 1}
    <div class="mt-6 flex gap-1 overflow-x-auto" role="tablist" aria-label="Seasons">
      {#each series.seasons as n (n)}
        <button
          type="button"
          role="tab"
          aria-selected={seasonShown === n}
          class="h-8 shrink-0 cursor-pointer rounded-md px-3 text-sm font-medium transition-colors {seasonShown === n ? 'bg-muted text-fg' : 'text-fg-muted hover:text-fg'}"
          onclick={() => (season = n)}>{seasonName(n)}</button>
      {/each}
    </div>
  {/if}

  <div class="card mt-4 overflow-hidden">
    <ul>
      {#each episodes as v (v.id)}
        {@const d = details(v)}
        {@const p = progressOf(v)}
        <li class="group flex items-center gap-3 border-b border-line px-3 py-2.5 last:border-b-0 hover:bg-subtle">
          <button type="button" class="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left" onclick={() => play(v)}>
            <span class="grid size-8 shrink-0 place-items-center rounded-full border border-line text-fg-muted group-hover:border-line-strong group-hover:text-fg">
              {#if isWatched(v)}<Icon name="check" class="size-4" />{:else}<Icon name="play" class="size-3.5" />{/if}
            </span>
            <span class="grid min-w-0 flex-1 gap-1">
              <span class="truncate text-sm {isWatched(v) ? 'text-fg-muted' : ''}">{d.label}</span>
              {#if p > 0 && p < 1}<span class="progress block max-w-48"><span class="block h-full rounded-full bg-accent" style:width="{p * 100}%"></span></span>{/if}
            </span>
          </button>
          <Menu label="Episode actions" items={videoMenu(v)} />
        </li>
      {/each}
    </ul>
  </div>
{:else}
  <div class="flex flex-wrap items-start gap-3">
    <div class="mr-auto min-w-0">
      <h1 class="text-xl font-semibold tracking-tight">Videos</h1>
      <p class="mt-1 truncate text-[13px] text-fg-muted">
        {#if cat}{videos.rootName} · {cat.series.length} series · {plural(cat.movies.length, 'movie')}{:else}Watch the videos in your files. Names and details are read in this browser.{/if}
      </p>
    </div>
    {#if videos.rootId}
      <Menu
        label="Video options"
        items={[
          { label: 'Read episode details from files', icon: 'file-video', onclick: readTags },
          { label: 'Choose another folder', icon: 'folder-open', onclick: choose },
          { label: 'Scan again', icon: 'refresh-cw', onclick: scanVideos },
        ]} />
    {/if}
  </div>

  {#if videos.reading}
    <div class="card mt-4 flex items-center gap-3 px-4 py-3 text-[13px]" aria-live="polite">
      <Icon name="loader-circle" class="spinner text-fg-muted" />
      <span class="flex-1">Reading episode details: {videos.reading.done} of {videos.reading.total}</span>
      <button type="button" class="btn btn-ghost" onclick={stopReading}>Stop</button>
    </div>
  {/if}

  {#if !videos.rootId}
    <div class="card mt-6 grid place-items-center gap-1 px-6 py-20 text-center">
      <img src="/img/logo.webp" alt="" width="715" height="349" class="mb-4 h-auto w-32 opacity-90 select-none" draggable="false" />
      <p class="font-medium">Pick your videos folder</p>
      <p class="max-w-sm text-[13px] text-fg-muted">
        Episodes are grouped into series by their names, like Show S01E02.mp4, by folders like Show/Season 1, or by their tags. MP4, WebM and MOV play here, decrypted as they stream; MKV where your browser can.
      </p>
      <button type="button" class="btn btn-primary mt-4" onclick={choose}><Icon name="folder-open" />Choose folder</button>
    </div>
  {:else if videos.error && !cat}
    <div class="card mt-6 grid place-items-center gap-1 px-6 py-16 text-center">
      <Icon name="circle-alert" class="mb-2 size-6 text-fg-muted" />
      <p class="font-medium">Could not open the videos folder</p>
      <p class="max-w-sm text-[13px] text-fg-muted">{videos.error}. It may have been moved to the trash, or its share ended.</p>
      <div class="mt-4 flex gap-2">
        <button type="button" class="btn btn-secondary" onclick={scanVideos}>Try again</button>
        <button type="button" class="btn btn-primary" onclick={choose}>Choose folder</button>
      </div>
    </div>
  {:else if !cat}
    <p class="mt-6 flex items-center gap-2 text-[13px] text-fg-muted" aria-live="polite">
      <Icon name="loader-circle" class="spinner" />Looking through folders{videos.found ? `: ${plural(videos.found, 'video')} so far` : ''}
    </p>
    <div class="mt-4 grid grid-cols-3 gap-4 sm:grid-cols-4 lg:grid-cols-5">
      {#each { length: 10 } as _, i (i)}
        <div class="grid gap-2"><div class="skeleton aspect-[2/3] rounded-md"></div><div class="skeleton h-3 w-2/3 rounded"></div></div>
      {/each}
    </div>
  {:else if !cat.items.length}
    <div class="card mt-6 grid place-items-center gap-1 px-6 py-16 text-center">
      <Icon name="clapperboard" class="mb-2 size-6 text-fg-muted" />
      <p class="font-medium">No videos in {videos.rootName}</p>
      <p class="max-w-sm text-[13px] text-fg-muted">Upload MP4, WebM, MOV or MKV files anywhere under it, then scan again.</p>
    </div>
  {:else}
    {#if videos.dataError}<p class="mt-4 text-[13px] text-danger">{videos.dataError}</p>{/if}

    {#if resume.length}
      <h2 class="mt-6 text-sm font-medium">Continue watching</h2>
      <div class="mt-3 grid grid-cols-3 gap-x-4 gap-y-5 sm:grid-cols-4 lg:grid-cols-6">
        {#each resume as v (v.id)}
          {@const d = details(v)}
          {@const s = seriesOf(v)}
          {@render card(s ? d.label : d.title, s ? s.name : 'Movie', s ? s.poster : moviePoster(v), s ? 'tv' : 'film', () => play(v), videoMenu(v), v)}
        {/each}
      </div>
    {/if}

    <div class="mt-6 flex flex-wrap items-center gap-3">
      <div class="flex items-center rounded-md border border-line p-0.5 text-sm" role="tablist" aria-label="Show">
        {#each [['series', 'Series'], ['movies', 'Movies']] as [id, label] (id)}
          <button
            type="button"
            role="tab"
            aria-selected={tab === id}
            class="h-7 cursor-pointer rounded px-3 font-medium transition-colors {tab === id ? 'bg-muted text-fg' : 'text-fg-muted hover:text-fg'}"
            onclick={() => (tab = id)}>{label}</button>
        {/each}
      </div>
      {#if videos.scanning}<Icon name="loader-circle" class="spinner text-fg-muted" />{/if}
      <label class="relative ml-auto w-full sm:w-64">
        <Icon name="search" class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fg-faint" />
        <input class="input pl-9" type="search" placeholder="Search videos" aria-label="Search videos" bind:value={query} />
      </label>
    </div>

    {#if (tab === 'series' ? seriesList : movies).length}
      <div class="mt-4 grid grid-cols-3 gap-x-4 gap-y-5 sm:grid-cols-4 lg:grid-cols-5">
        {#if tab === 'series'}
          {#each seriesList as s (s.key)}
            {@render card(s.name, `${plural(s.seasons.length, 'season')} · ${plural(s.episodes.length, 'episode')}`, s.poster, 'tv', () => openSeries(s), null)}
          {/each}
        {:else}
          {#each movies as v (v.id)}
            {@render card(details(v).title, v.entry.meta.name, moviePoster(v), 'film', () => play(v), videoMenu(v), v)}
          {/each}
        {/if}
      </div>
    {:else if q}
      <p class="mt-10 text-center text-[13px] text-fg-muted">Nothing matches "{query}"</p>
    {:else}
      <p class="mt-10 text-center text-[13px] text-fg-muted">
        {tab === 'series' ? 'No series found. Name episodes like Show S01E02.mp4, or read their details from the files.' : 'Every video here is part of a series.'}
      </p>
    {/if}
  {/if}
{/if}

{#if playing}
  <VideoPlayer video={playing} onclose={() => history.back()} />
{/if}

{#if picking}
  <FolderPickDialog
    root={picking}
    title="Videos folder"
    description="Everything under this folder shows up in Videos. Episodes are grouped into series; the rest are movies."
    onpick={setVideosRoot}
    onclose={() => (picking = null)} />
{/if}

{#if dialog?.type === 'edit'}
  <VideoEditDialog video={dialog.video} onclose={() => (dialog = null)} />
{:else if dialog?.type === 'rename'}
  <NameDialog
    title="Rename series"
    initial={dialog.series.name}
    onsave={async (n) => {
      await renameSeries(dialog.series, n);
      go({ name: 'videos', series: n.toLowerCase() });
    }}
    onclose={() => (dialog = null)} />
{:else if dialog?.type === 'files'}
  <ConfirmDialog
    title="Rename {plural(dialog.series.episodes.length, 'file')}?"
    description="Each episode's file is renamed to its name, season and episode, like {details(dialog.series.episodes[0]).label}. Their details here stay the same."
    confirmLabel="Rename files"
    onconfirm={async () => {
      const n = await renameFiles(dialog.series.episodes);
      toast(n ? `Renamed ${plural(n, 'file')}` : 'The files already have these names');
    }}
    onclose={() => (dialog = null)} />
{/if}
