<script>
  // A folder of photos as a timeline by date taken, and as albums by folder.
  // Tiles are the encrypted thumbnails, decrypted here; a photo opens in the
  // preview, which steps through the ones shown.
  import { onMount } from 'svelte';
  import { session, resolvePath, fetchEntry, openEntry, download } from '../../lib/cloud.svelte.js';
  import { photos, openPhotos, setPhotosRoot, scanPhotos, byMonth, albums } from '../../lib/photos.svelte.js';
  import { toastError, trackTransfer, errorMessage } from '../../lib/ui.svelte.js';
  import { plural } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import Menu from '../Menu.svelte';
  import Thumb from '../Thumb.svelte';
  import Preview from '../Preview.svelte';
  import FolderPickDialog from '../dialogs/FolderPickDialog.svelte';

  let { go } = $props();

  onMount(openPhotos);

  let picking = $state(null);
  async function choose() {
    try {
      picking = (await resolvePath(session.me.keys.root_node_id)).items[0];
    } catch (e) {
      toastError(e);
    }
  }

  let tab = $state('timeline'); // timeline | albums
  let album = $state(null); // an album's key, when one is open

  const albumList = $derived(photos.list ? albums(photos.list) : []);
  const openAlbum = $derived(album === null ? null : albumList.find((a) => a.key === album));
  // What's on screen, in order: the preview steps through exactly these.
  const shown = $derived(openAlbum ? openAlbum.items : (photos.list ?? []));
  const months = $derived(byMonth(shown));

  let viewing = $state(null); // index into `shown`

  async function downloadEntry(entry) {
    const t = trackTransfer('download', entry.meta.name, entry.meta.size);
    try {
      await download(entry, (p) => (t.progress = p));
      t.status = 'done';
    } catch (e) {
      t.status = 'error';
      t.error = errorMessage(e);
    }
  }
</script>

<div class="flex flex-wrap items-center justify-between gap-3">
  <div class="min-w-0">
    {#if openAlbum}
      <button type="button" class="mb-1 inline-flex cursor-pointer items-center gap-1 text-[13px] text-fg-muted hover:text-fg" onclick={() => (album = null)}>
        <Icon name="arrow-left" class="size-3.5" /> Albums
      </button>
      <h1 class="truncate text-xl font-semibold tracking-tight">{openAlbum.name}</h1>
      <p class="mt-1 text-[13px] text-fg-muted">{plural(openAlbum.items.length, 'photo')}{openAlbum.location.length > 1 ? ` · ${openAlbum.location.slice(0, -1).join(' / ')}` : ''}</p>
    {:else}
      <h1 class="text-xl font-semibold tracking-tight">Photos</h1>
      {#if photos.rootId}
        <p class="mt-1 text-[13px] text-fg-muted">
          Images in <button type="button" class="link" onclick={() => go({ name: 'files', folderId: photos.rootId })}>{photos.rootName || 'your photos folder'}</button>, by the date they were taken.
        </p>
      {/if}
    {/if}
  </div>
  {#if photos.rootId && !openAlbum}
    <div class="flex gap-2">
      <div class="flex h-8 rounded-md border border-line p-0.5" role="radiogroup" aria-label="Show">
        {#each [['timeline', 'Timeline'], ['albums', 'Albums']] as [value, label] (value)}
          <button
            type="button"
            role="radio"
            aria-checked={tab === value}
            class="cursor-pointer rounded px-2.5 text-xs font-medium transition-colors {tab === value ? 'bg-muted text-fg' : 'text-fg-muted hover:text-fg'}"
            onclick={() => (tab = value)}>{label}</button>
        {/each}
      </div>
      <Menu
        label="Photos folder"
        items={[
          { label: 'Choose another folder', icon: 'folder-open', onclick: choose },
          { label: 'Look for new photos', icon: 'refresh-cw', onclick: scanPhotos },
        ]} />
    </div>
  {/if}
</div>

{#snippet grid(items)}
  <ul class="grid grid-cols-[repeat(auto-fill,minmax(7.5rem,1fr))] gap-1">
    {#each items as p (p.node.id)}
      <li>
        <button
          type="button"
          class="grid aspect-square w-full cursor-pointer place-items-center overflow-hidden rounded-md bg-subtle outline-offset-2 transition-opacity hover:opacity-90"
          title={p.meta.name}
          aria-label={p.meta.name}
          onclick={() => (viewing = shown.indexOf(p))}>
          <Thumb entry={p} iconClass="size-8" />
        </button>
      </li>
    {/each}
  </ul>
{/snippet}

{#if !photos.rootId}
  <div class="card mt-6 grid place-items-center gap-1 px-6 py-20 text-center">
    <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle"><Icon name="image" class="size-5 text-fg-muted" /></div>
    <p class="font-medium">Pick a folder for your photos</p>
    <p class="max-w-sm text-[13px] text-fg-muted">Its images, and those in folders below it, become a timeline by date taken, with each folder as an album.</p>
    <button type="button" class="btn btn-primary mt-4" onclick={choose}><Icon name="folder-open" /> Choose a folder</button>
  </div>
{:else if photos.error}
  <div class="card mt-6 grid place-items-center gap-3 px-6 py-16 text-center">
    <Icon name="circle-alert" class="size-6 text-danger" />
    <p class="text-fg-muted">{photos.error}</p>
    <div class="flex gap-2">
      <button type="button" class="btn btn-secondary" onclick={scanPhotos}><Icon name="refresh-cw" /> Try again</button>
      <button type="button" class="btn btn-ghost" onclick={choose}>Choose another folder</button>
    </div>
  </div>
{:else if !photos.list}
  <div class="mt-6 grid grid-cols-[repeat(auto-fill,minmax(7.5rem,1fr))] gap-1" aria-busy="true" aria-label="Loading">
    {#each Array(12) as _, i (i)}<div class="skeleton aspect-square rounded-md"></div>{/each}
  </div>
{:else if !photos.list.length}
  <div class="card mt-6 grid place-items-center gap-1 px-6 py-20 text-center">
    <p class="font-medium">No photos here yet</p>
    <p class="max-w-sm text-[13px] text-fg-muted">Upload JPEG, PNG, WebP or other images into {photos.rootName}, or any folder in it.</p>
  </div>
{:else if tab === 'albums' && !openAlbum}
  <ul class="mt-6 grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-4">
    {#each albumList as a (a.key)}
      <li>
        <button type="button" class="group grid w-full cursor-pointer gap-2 text-left" onclick={() => (album = a.key)}>
          <span class="grid aspect-square w-full place-items-center overflow-hidden rounded-lg border border-line bg-subtle transition-opacity group-hover:opacity-90">
            <Thumb entry={a.items[0]} iconClass="size-10" />
          </span>
          <span class="grid min-w-0 px-0.5">
            <span class="truncate text-[13px] font-medium">{a.name}</span>
            <span class="truncate text-xs text-fg-muted">{plural(a.items.length, 'photo')}</span>
          </span>
        </button>
      </li>
    {/each}
  </ul>
{:else}
  <div class="mt-6 grid gap-6">
    {#each months as m (m.key)}
      <section class="grid gap-2">
        <h2 class="text-[13px] font-medium text-fg-muted">{m.label}</h2>
        {@render grid(m.items)}
      </section>
    {/each}
  </div>
{/if}

{#if photos.scanning && photos.list}
  <p class="mt-4 flex items-center gap-2 text-xs text-fg-faint" role="status"><Icon name="loader-circle" class="spinner size-3.5" /> Looking for new photos</p>
{/if}

{#if viewing !== null && shown[viewing]}
  <Preview entries={shown} start={viewing} fetch={fetchEntry} open={openEntry} ondownload={downloadEntry} onclose={() => (viewing = null)} />
{/if}

{#if picking}
  <FolderPickDialog
    root={picking}
    title="Photos folder"
    description="Its images, in it and in folders below it, become your photos."
    onpick={(id) => {
      album = null;
      setPhotosRoot(id);
    }}
    onclose={() => (picking = null)} />
{/if}
