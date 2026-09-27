<script>
  import { session, resolvePath } from '../../lib/cloud.svelte.js';
  import { music, library, openLibrary, setMusicRoot, scanLibrary, loadCover, covers, info, albumInfo, albumTracks, saved, playlistTracks, renamePlaylist, deletePlaylist, createPlaylist, removeFromPlaylist, moveInPlaylist, play, playNext, enqueue, player, current } from '../../lib/music.svelte.js';
  import { toast, toastError } from '../../lib/ui.svelte.js';
  import { plural } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import Menu from '../Menu.svelte';
  import FolderPickDialog from '../dialogs/FolderPickDialog.svelte';
  import AddToPlaylistDialog from '../dialogs/AddToPlaylistDialog.svelte';
  import TrackEditDialog from '../dialogs/TrackEditDialog.svelte';
  import AlbumEditDialog from '../dialogs/AlbumEditDialog.svelte';
  import NameDialog from '../dialogs/NameDialog.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';

  let { album: albumId = null, playlist: playlistId = null, go } = $props();

  let tab = $state('albums'); // albums | tracks | playlists
  let dialog = $state.raw(null); // { type, ... }
  let query = $state('');
  let shown = $state(300);
  let picking = $state(null); // My files root entry while the folder dialog is open

  openLibrary();

  const lib = $derived(library.value);
  const album = $derived(albumId && lib?.albums.find((a) => a.id === albumId));
  const playlist = $derived(playlistId && saved.value.playlists.find((p) => p.id === playlistId));
  const playlistList = $derived(playlist && lib ? playlistTracks(playlist) : []);
  const q = $derived(query.trim().toLowerCase());
  const matches = (t) => {
    const i = info(t);
    return `${i.title} ${i.artist} ${i.album}`.toLowerCase().includes(q);
  };
  const albumText = (a) => {
    const i = albumInfo(a);
    return `${i.name} ${i.artist ?? ''}`.toLowerCase();
  };
  const albums = $derived(!lib ? [] : q ? lib.albums.filter((a) => albumText(a).includes(q) || a.tracks.some(matches)) : lib.albums);
  const playlists = $derived(q ? saved.value.playlists.filter((p) => p.name.toLowerCase().includes(q)) : saved.value.playlists);
  const tracks = $derived(!lib ? [] : q ? lib.tracks.filter(matches) : lib.tracks);
  const playingId = $derived(current()?.id);

  async function choose() {
    try {
      picking = (await resolvePath(session.me.keys.root_node_id)).items[0];
    } catch (e) {
      toastError(e);
    }
  }

  const openAlbum = (a) => go({ name: 'music', album: a.id });
  const openPlaylist = (p) => go({ name: 'music', playlist: p.id });

  /** The first cover among a playlist's tracks' albums. */
  function playlistCover(p) {
    if (!lib) return null;
    for (const t of playlistTracks(p)) {
      const a = lib.albums.find((a) => a.id === t.albumId);
      if (a?.cover) return a;
    }
    return null;
  }

  async function act(fn) {
    try {
      await fn();
    } catch (e) {
      toastError(e);
    }
  }

  /** Svelte action: call `fn` once, when the element first scrolls into view. */
  function onVisible(node, fn) {
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && (fn(), io.disconnect()), { rootMargin: '200px' });
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }

  function trackMenu(t, i) {
    const inList = playlist
      ? [
          'sep',
          ...(i > 0 ? [{ label: 'Move up', icon: 'arrow-up', onclick: () => act(() => moveInPlaylist(playlist.id, t, playlistList[i - 1])) }] : []),
          ...(i < playlistList.length - 1 ? [{ label: 'Move down', icon: 'arrow-down', onclick: () => act(() => moveInPlaylist(playlist.id, t, playlistList[i + 1])) }] : []),
          { label: 'Remove from playlist', icon: 'x', onclick: () => act(() => removeFromPlaylist(playlist.id, t)) },
        ]
      : [];
    return [
      { label: 'Play next', icon: 'list-start', onclick: () => (playNext([t]), toast(`${info(t).title} plays next`)) },
      { label: 'Add to queue', icon: 'list-end', onclick: () => (enqueue([t]), toast(`Added ${info(t).title} to the queue`)) },
      { label: 'Add to playlist', icon: 'list-plus', onclick: () => (dialog = { type: 'add', tracks: [t] }) },
      ...inList,
      'sep',
      { label: 'Edit details', icon: 'pencil', onclick: () => (dialog = { type: 'track', track: t }) },
      { label: 'Show in files', icon: 'folder-open', onclick: () => go({ name: 'files', folderId: t.entry.parentId }) },
    ];
  }
</script>

{#snippet cover(a, cls = '')}
  {@const url = covers.get(a.id)}
  <div class="grid aspect-square place-items-center overflow-hidden rounded-md border border-line bg-muted text-fg-faint {cls}" use:onVisible={() => loadCover(a)}>
    {#if url}<img src={url} alt="" class="size-full object-cover" draggable="false" />{:else}<Icon name="disc-3" class="size-1/3" strokeWidth={1.5} />{/if}
  </div>
{/snippet}

{#snippet trackTable(list, numbered)}
  <div class="card overflow-hidden">
    <table class="table table-fixed">
      <thead>
        <tr>
          <th class="w-12 text-right">#</th>
          <th>Title</th>
          {#if !numbered}<th class="hidden md:table-cell">Album</th>{/if}
          <th class="w-12"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each list.slice(0, shown) as t, i (t.id)}
          {@const it = info(t)}
          {@const now = t.id === playingId}
          <tr class="group cursor-pointer" onclick={() => play(list, i)}>
            <td class="text-right text-xs text-fg-muted tabular-nums">
              {#if now}<Icon name={player.playing ? 'audio-lines' : 'music'} class="ml-auto size-4 text-accent-text" />{:else}{numbered ? (it.trackNo ?? i + 1) : i + 1}{/if}
            </td>
            <td class="min-w-0">
              <button type="button" class="block w-full min-w-0 cursor-pointer text-left" onclick={(e) => (e.stopPropagation(), play(list, i))}>
                <span class="block truncate {now ? 'font-medium text-accent-text' : ''}">{it.title}</span>
                <span class="block truncate text-xs text-fg-muted">{it.artist}</span>
              </button>
            </td>
            {#if !numbered}<td class="hidden truncate text-fg-muted md:table-cell">{it.album}</td>{/if}
            <td onclick={(e) => e.stopPropagation()}>
              <Menu label="Track actions" items={trackMenu(t, i)} />
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if list.length > shown}
      <div class="border-t border-line p-2 text-center">
        <button type="button" class="btn btn-ghost" onclick={() => (shown += 300)}>Show more ({list.length - shown} left)</button>
      </div>
    {/if}
  </div>
{/snippet}

{#snippet header(kind, title, sub, list, menu, art)}
  {@const cls = 'w-40 shrink-0 sm:w-48'}
  <button type="button" class="btn btn-ghost -ml-3 mb-4" onclick={() => history.back()}><Icon name="arrow-left" />Music</button>
  <div class="flex flex-col gap-5 sm:flex-row sm:items-end">
    {#if kind === 'Playlist'}{@render stack(art, cls)}{:else}{@render cover(art, cls)}{/if}
    <div class="grid min-w-0 gap-1">
      <p class="text-xs font-medium text-fg-muted">{kind}</p>
      <h1 class="truncate text-2xl font-semibold tracking-tight">{title}</h1>
      <p class="text-[13px] text-fg-muted">{sub}</p>
      <div class="mt-3 flex items-center gap-2">
        <button type="button" class="btn btn-accent" disabled={!list.length} onclick={() => play(list, 0, { shuffle: false })}><Icon name="play" />Play</button>
        <button type="button" class="btn btn-secondary" disabled={!list.length} onclick={() => play(list, 0, { shuffle: true })}><Icon name="shuffle" />Shuffle</button>
        <Menu label="{kind} actions" items={menu} />
      </div>
    </div>
  </div>
{/snippet}

{#snippet stack(p, cls = '')}
  {@const a = playlistCover(p)}
  {#if a}{@render cover(a, cls)}{:else}
    <div class="grid aspect-square place-items-center rounded-md border border-line bg-muted text-fg-faint {cls}"><Icon name="list-music" class="size-1/3" strokeWidth={1.5} /></div>
  {/if}
{/snippet}

{#if album}
  {@const ai = albumInfo(album)}
  {@const list = albumTracks(album)}
  {@render header(
    'Album',
    ai.name,
    `${ai.artist ?? 'Unknown artist'} · ${plural(album.tracks.length, 'track')}`,
    list,
    [
      { label: 'Add to playlist', icon: 'list-plus', onclick: () => (dialog = { type: 'add', tracks: list }) },
      { label: 'Add to queue', icon: 'list-end', onclick: () => (enqueue(list), toast(`Added ${ai.name} to the queue`)) },
      'sep',
      { label: 'Edit album', icon: 'pencil', onclick: () => (dialog = { type: 'album', album }) },
      { label: 'Show in files', icon: 'folder-open', onclick: () => go({ name: 'files', folderId: album.id }) },
    ],
    album,
  )}
  <div class="mt-6">{@render trackTable(list, true)}</div>
{:else if playlist}
  {@render header(
    'Playlist',
    playlist.name,
    plural(playlistList.length, 'track'),
    playlistList,
    [
      { label: 'Add to queue', icon: 'list-end', onclick: () => (enqueue(playlistList), toast(`Added ${playlist.name} to the queue`)) },
      'sep',
      { label: 'Rename', icon: 'pencil', onclick: () => (dialog = { type: 'rename', playlist }) },
      { label: 'Delete playlist', icon: 'trash-2', danger: true, onclick: () => (dialog = { type: 'delete', playlist }) },
    ],
    playlist,
  )}
  {#if !lib}
    <p class="mt-6 flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="loader-circle" class="spinner" />Looking through folders</p>
  {:else if playlistList.length}
    <div class="mt-6">{@render trackTable(playlistList, false)}</div>
  {:else}
    <div class="card mt-6 grid place-items-center gap-1 px-6 py-16 text-center">
      <Icon name="list-music" class="mb-2 size-6 text-fg-muted" />
      <p class="font-medium">Nothing in this playlist yet</p>
      <p class="max-w-sm text-[13px] text-fg-muted">Add tracks from an album or the track list with Add to playlist in their menu.</p>
    </div>
  {/if}
{:else}
  <div class="flex flex-wrap items-start gap-3">
    <div class="mr-auto min-w-0">
      <h1 class="text-xl font-semibold tracking-tight">Music</h1>
      <p class="mt-1 truncate text-[13px] text-fg-muted">
        {#if lib}{music.rootName} · {plural(lib.albums.length, 'album')} · {plural(lib.tracks.length, 'track')}{:else}Play the music in your files. Names and tags are read in this browser.{/if}
      </p>
    </div>
    {#if music.rootId}
      <div class="flex items-center gap-2">
        {#if lib?.tracks.length}
          <button type="button" class="btn btn-accent" onclick={() => play(lib.tracks, 0, { shuffle: true })}><Icon name="shuffle" />Shuffle all</button>
        {/if}
        <Menu
          label="Music options"
          items={[
            { label: 'New playlist', icon: 'list-plus', onclick: () => (dialog = { type: 'new' }) },
            { label: 'Choose another folder', icon: 'folder-open', onclick: choose },
            { label: 'Scan again', icon: 'refresh-cw', onclick: scanLibrary },
          ]} />
      </div>
    {/if}
  </div>

  {#if !music.rootId}
    <div class="card mt-6 grid place-items-center gap-1 px-6 py-20 text-center">
      <img src="/img/logo.webp" alt="" width="715" height="349" class="mb-4 h-auto w-32 opacity-90 select-none" draggable="false" />
      <p class="font-medium">Pick your music folder</p>
      <p class="max-w-sm text-[13px] text-fg-muted">
        Albums are made from its folders, like Artist/Album/01 Song.mp3. MP3, M4A, AAC, Ogg, Opus, FLAC and WAV play here, decrypted as they stream.
      </p>
      <button type="button" class="btn btn-primary mt-4" onclick={choose}><Icon name="folder-open" />Choose folder</button>
    </div>
  {:else if music.error && !lib}
    <div class="card mt-6 grid place-items-center gap-1 px-6 py-16 text-center">
      <Icon name="circle-alert" class="mb-2 size-6 text-fg-muted" />
      <p class="font-medium">Could not open the music folder</p>
      <p class="max-w-sm text-[13px] text-fg-muted">{music.error}. It may have been moved to the trash, or its share ended.</p>
      <div class="mt-4 flex gap-2">
        <button type="button" class="btn btn-secondary" onclick={scanLibrary}>Try again</button>
        <button type="button" class="btn btn-primary" onclick={choose}>Choose folder</button>
      </div>
    </div>
  {:else if !lib}
    <p class="mt-6 flex items-center gap-2 text-[13px] text-fg-muted" aria-live="polite">
      <Icon name="loader-circle" class="spinner" />Looking through folders{music.found ? `: ${plural(music.found, 'track')} so far` : ''}
    </p>
    <div class="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
      {#each { length: 8 } as _, i (i)}
        <div class="grid gap-2"><div class="skeleton aspect-square rounded-md"></div><div class="skeleton h-3 w-2/3 rounded"></div></div>
      {/each}
    </div>
  {:else if !lib.tracks.length}
    <div class="card mt-6 grid place-items-center gap-1 px-6 py-16 text-center">
      <Icon name="music" class="mb-2 size-6 text-fg-muted" />
      <p class="font-medium">No music in {music.rootName}</p>
      <p class="max-w-sm text-[13px] text-fg-muted">Upload MP3, M4A, AAC, Ogg, Opus, FLAC or WAV files anywhere under it, then scan again.</p>
    </div>
  {:else}
    <div class="mt-6 flex flex-wrap items-center gap-3">
      <div class="flex items-center rounded-md border border-line p-0.5 text-sm" role="tablist" aria-label="Show">
        {#each [['albums', 'Albums'], ['tracks', 'Tracks'], ['playlists', 'Playlists']] as [id, label] (id)}
          <button
            type="button"
            role="tab"
            aria-selected={tab === id}
            class="h-7 cursor-pointer rounded px-3 font-medium transition-colors {tab === id ? 'bg-muted text-fg' : 'text-fg-muted hover:text-fg'}"
            onclick={() => ((tab = id), (shown = 300))}>{label}</button>
        {/each}
      </div>
      {#if music.scanning}<Icon name="loader-circle" class="spinner text-fg-muted" />{/if}
      <label class="relative ml-auto w-full sm:w-64">
        <Icon name="search" class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fg-faint" />
        <input class="input pl-9" type="search" placeholder="Search music" aria-label="Search music" bind:value={query} oninput={() => (shown = 300)} />
      </label>
    </div>

    {#if tab === 'albums'}
      {#if albums.length}
        <div class="mt-4 grid grid-cols-2 gap-x-4 gap-y-5 sm:grid-cols-3 lg:grid-cols-4">
          {#each albums.slice(0, shown) as a (a.id)}
            {@const ai = albumInfo(a)}
            <div class="group min-w-0">
              <div class="relative">
                <button type="button" class="block w-full cursor-pointer" aria-label={ai.name} onclick={() => openAlbum(a)}>
                  {@render cover(a, 'transition-colors group-hover:border-line-strong')}
                </button>
                <button
                  type="button"
                  class="btn btn-accent btn-icon absolute right-2 bottom-2 size-9 rounded-full opacity-0 shadow-md transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 pointer-coarse:hidden"
                  aria-label="Play {ai.name}"
                  onclick={() => play(albumTracks(a), 0, { shuffle: false })}><Icon name="play" /></button>
              </div>
              <button type="button" class="mt-2 block w-full cursor-pointer text-left" tabindex="-1" onclick={() => openAlbum(a)}>
                <span class="block truncate text-sm font-medium">{ai.name}</span>
                <span class="block truncate text-xs text-fg-muted">{ai.artist ?? 'Unknown artist'}</span>
              </button>
            </div>
          {/each}
        </div>
        {#if albums.length > shown}
          <div class="mt-4 text-center"><button type="button" class="btn btn-ghost" onclick={() => (shown += 300)}>Show more</button></div>
        {/if}
      {:else}
        <p class="mt-10 text-center text-[13px] text-fg-muted">No albums match "{query}"</p>
      {/if}
    {:else if tab === 'playlists'}
      {#if music.dataError}
        <p class="mt-10 text-center text-[13px] text-danger">{music.dataError}</p>
      {:else}
        <div class="mt-4 grid grid-cols-2 gap-x-4 gap-y-5 sm:grid-cols-3 lg:grid-cols-4">
          {#each playlists as p (p.id)}
            {@const n = playlistTracks(p).length}
            <div class="group min-w-0">
              <div class="relative">
                <button type="button" class="block w-full cursor-pointer" aria-label={p.name} onclick={() => openPlaylist(p)}>
                  {@render stack(p, 'transition-colors group-hover:border-line-strong')}
                </button>
                {#if n}
                  <button
                    type="button"
                    class="btn btn-accent btn-icon absolute right-2 bottom-2 size-9 rounded-full opacity-0 shadow-md transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 pointer-coarse:hidden"
                    aria-label="Play {p.name}"
                    onclick={() => play(playlistTracks(p), 0, { shuffle: false })}><Icon name="play" /></button>
                {/if}
              </div>
              <button type="button" class="mt-2 block w-full cursor-pointer text-left" tabindex="-1" onclick={() => openPlaylist(p)}>
                <span class="block truncate text-sm font-medium">{p.name}</span>
                <span class="block truncate text-xs text-fg-muted">{plural(n, 'track')}</span>
              </button>
            </div>
          {/each}
          {#if !q}
            <button type="button" class="grid aspect-square cursor-pointer place-items-center rounded-md border border-dashed border-line text-fg-muted transition-colors hover:border-line-strong hover:text-fg" onclick={() => (dialog = { type: 'new' })}>
              <span class="grid justify-items-center gap-2 text-sm"><Icon name="plus" class="size-5" />New playlist</span>
            </button>
          {/if}
        </div>
        {#if q && !playlists.length}
          <p class="mt-10 text-center text-[13px] text-fg-muted">No playlists match "{query}"</p>
        {/if}
      {/if}
    {:else if tracks.length}
      <div class="mt-4">{@render trackTable(tracks, false)}</div>
    {:else}
      <p class="mt-10 text-center text-[13px] text-fg-muted">No tracks match "{query}"</p>
    {/if}
  {/if}
{/if}

{#if picking}
  <FolderPickDialog
    root={picking}
    title="Music folder"
    description="Everything under this folder shows up in Music, grouped into albums by folder."
    onpick={setMusicRoot}
    onclose={() => (picking = null)} />
{/if}

{#if dialog?.type === 'add'}
  <AddToPlaylistDialog tracks={dialog.tracks} onclose={() => (dialog = null)} />
{:else if dialog?.type === 'track'}
  <TrackEditDialog track={dialog.track} onclose={() => (dialog = null)} />
{:else if dialog?.type === 'album'}
  <AlbumEditDialog album={dialog.album} onclose={() => (dialog = null)} />
{:else if dialog?.type === 'new'}
  <NameDialog title="New playlist" initial="" confirmLabel="Create" create onsave={async (n) => openPlaylist({ id: await createPlaylist(n) })} onclose={() => (dialog = null)} />
{:else if dialog?.type === 'rename'}
  <NameDialog title="Rename playlist" initial={dialog.playlist.name} onsave={(n) => renamePlaylist(dialog.playlist.id, n)} onclose={() => (dialog = null)} />
{:else if dialog?.type === 'delete'}
  <ConfirmDialog
    title="Delete {dialog.playlist.name}?"
    description="The playlist goes away; the music stays in your files."
    confirmLabel="Delete"
    danger
    onconfirm={async () => {
      const open = !!playlist;
      await deletePlaylist(dialog.playlist.id);
      if (open) history.back();
    }}
    onclose={() => (dialog = null)} />
{/if}
