<script>
  // Looking inside a .zip, .7z, .tar or .tar.gz: a zip is read with the same
  // bounds-checked reader as EPUB/CBZ and office documents (lib/unzip.js),
  // so the same entry, size and zip-bomb caps apply; a 7z with lib/un7z.js
  // (unpacking a block only when an entry in it is downloaded); a tar with
  // lib/untar.js, gunzipped first (up to its cap) when it's compressed.
  // Which one it is comes from the bytes, not the name. Nothing is
  // extracted until a single entry is downloaded.
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n.svelte.js';
  import { formatSize } from '../../lib/format.js';
  import { saveBlob } from '../../lib/crypto.js';
  import { toastError } from '../../lib/ui.svelte.js';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';

  let { blob } = $props();

  let error = $state('');
  let root = $state(null); // { folders: Map<string, node>, files: [{name, path, size}] }
  let path = $state([]); // breadcrumb segments into `root`
  let saving = $state(new Set()); // entry paths currently being read for download

  function buildTree(entries) {
    const top = { folders: new Map(), files: [] };
    for (const { name, size } of entries) {
      const parts = name.split('/');
      let node = top;
      for (let i = 0; i < parts.length - 1; i++) {
        const seg = parts[i];
        if (!seg) continue;
        if (!node.folders.has(seg)) node.folders.set(seg, { folders: new Map(), files: [] });
        node = node.folders.get(seg);
      }
      const base = parts[parts.length - 1];
      if (base) node.files.push({ name: base, path: name, size });
    }
    return top;
  }

  let archive = null;
  onMount(() => {
    (async () => {
      try {
        let bytes = new Uint8Array(await blob.arrayBuffer());
        const { is7z, open7z } = await import('../../lib/un7z.js');
        const { openTar, isTar, isGzip, gunzip } = await import('../../lib/untar.js');
        if (isGzip(bytes)) bytes = await gunzip(bytes);
        if (is7z(bytes)) {
          archive = await open7z(bytes);
        } else if (isTar(bytes)) {
          archive = openTar(bytes);
        } else {
          const { openZip } = await import('../../lib/unzip.js');
          archive = openZip(bytes);
        }
        root = buildTree(archive.list());
      } catch (e) {
        error = e?.reason === 'password' ? t("This archive is protected with a password, so it can't be opened here.") : t("This file couldn't be opened.");
      }
    })();
  });

  const current = $derived.by(() => {
    let node = root;
    for (const seg of path) node = node?.folders.get(seg);
    return node;
  });
  const folders = $derived([...(current?.folders.keys() ?? [])].sort((a, b) => a.localeCompare(b)));
  const files = $derived([...(current?.files ?? [])].sort((a, b) => a.name.localeCompare(b.name)));

  async function downloadEntry(entry) {
    if (saving.has(entry.path)) return;
    saving = new Set(saving).add(entry.path);
    try {
      const bytes = await archive.read(entry.path);
      saveBlob(new Blob([bytes], { type: 'application/octet-stream' }), entry.name);
    } catch (e) {
      if (e?.reason === 'password') toastError(new Error(t("This archive is protected with a password, so it can't be opened here.")));
      else if (e?.reason === 'method') toastError(new Error(t("This file is packed in a way the preview can't unpack. Download the whole archive and open it with 7-Zip.")));
      else toastError(e);
    } finally {
      saving = new Set(saving);
      saving.delete(entry.path);
    }
  }
</script>

<div class="flex h-full flex-col">
  {#if error}
    <p class="grid flex-1 place-items-center px-6 text-center text-[13px] text-fg-muted">{error}</p>
  {:else if !root}
    <p class="grid flex-1 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></p>
  {:else}
    <div class="flex min-w-0 shrink-0 items-center gap-1 overflow-x-auto border-b border-line px-3 py-2 text-[13px]">
      <button type="button" class="btn btn-ghost h-7 shrink-0 px-2" disabled={!path.length} onclick={() => (path = [])}>
        <Icon name="house" class="size-3.5" />
      </button>
      {#each path as seg, i (i)}
        <Icon name="chevron-right" class="size-3.5 shrink-0 text-fg-faint" />
        <button type="button" class="btn btn-ghost h-7 shrink-0 truncate px-2" onclick={() => (path = path.slice(0, i + 1))}>{seg}</button>
      {/each}
    </div>
    {#if !folders.length && !files.length}
      <p class="grid flex-1 place-items-center text-[13px] text-fg-muted">{t('This folder is empty.')}</p>
    {:else}
      <div class="min-h-0 flex-1 overflow-auto p-2">
        {#each folders as seg (seg)}
          <button type="button" class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-[13px] hover:bg-subtle" onclick={() => (path = [...path, seg])}>
            <FolderIcon name={seg} class="size-4 shrink-0" />
            <span class="min-w-0 flex-1 truncate">{seg}</span>
          </button>
        {/each}
        {#each files as entry (entry.path)}
          <div class="flex items-center gap-2.5 rounded-md px-2.5 py-2 text-[13px] hover:bg-subtle">
            <FileIcon meta={{ name: entry.name }} class="size-4 shrink-0" />
            <span class="min-w-0 flex-1 truncate">{entry.name}</span>
            <span class="shrink-0 text-xs text-fg-faint tabular-nums">{formatSize(entry.size)}</span>
            <button
              type="button"
              class="btn btn-ghost btn-icon shrink-0"
              aria-label={t('Download {name}', { name: entry.name })}
              title={t('Download')}
              disabled={saving.has(entry.path)}
              onclick={() => downloadEntry(entry)}>
              {#if saving.has(entry.path)}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="download" />{/if}
            </button>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>
