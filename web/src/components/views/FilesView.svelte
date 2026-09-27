<script>
  import { session, resolvePath, listFolder, createFolder, rename, trash, untrash, download, fetchEntry, upload, refreshMe } from '../../lib/cloud.svelte.js';
  import { toast, toastError, trackTransfer, errorMessage } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, fullDate, fileIcon, plural } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import Menu from '../Menu.svelte';
  import NameDialog from '../dialogs/NameDialog.svelte';
  import VersionsDialog from '../dialogs/VersionsDialog.svelte';
  import { fade, flip, flipParams } from '../../lib/motion.js';
  import MoveDialog from '../dialogs/MoveDialog.svelte';
  import ShareDialog from '../dialogs/ShareDialog.svelte';
  import LinkDialog from '../dialogs/LinkDialog.svelte';
  import Preview from '../Preview.svelte';

  let { folderId, go, inShare = $bindable(false) } = $props();

  let path = $state([]); // [{ node, key, meta }] from the tree root down
  let share = $state(null); // set when browsing a folder shared with us
  let rows = $state([]);
  let loading = $state(true);
  let loadError = $state('');
  let dialog = $state(null); // { type, entry }
  let dragging = $state(false);
  let fileInput;
  let versionInput;
  let versionTarget = null;

  const here = $derived(path[path.length - 1]);
  const canWrite = $derived(!share || share.permission === 'write');
  const isOwner = $derived(!share);

  async function load(id = folderId) {
    loadError = '';
    try {
      const p = await resolvePath(id);
      const list = await listFolder(id, p.items[p.items.length - 1].key);
      if (id !== folderId) return; // navigated away meanwhile
      path = p.items;
      share = p.share;
      inShare = !!p.share;
      rows = list;
    } catch (e) {
      loadError = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    loading = true;
    rows = [];
    load(folderId);
  });

  const open = (id) => go({ name: 'files', folderId: id });

  // ------------------------------------------------------------------ uploads

  async function uploadFiles(files, existing = null) {
    const list = [...files];
    if (!list.length) return;
    const target = here;
    const one = async (file) => {
      const t = trackTransfer('upload', existing ? existing.meta.name : file.name, file.size);
      try {
        const dest = existing ? { existing } : { parentId: target.node.id, parentKey: target.key };
        await upload(file, dest, (p) => (t.progress = p));
        t.status = 'done';
      } catch (e) {
        t.status = 'error';
        t.error = e?.code === 'quota_exceeded' ? 'Not enough storage left' : errorMessage(e);
      }
    };
    // Three files at a time; each file's chunks go up sequentially.
    let next = 0;
    const worker = async () => {
      while (next < list.length) await one(list[next++]);
    };
    await Promise.all(Array.from({ length: Math.min(3, list.length) }, worker));
    if (target.node.id === folderId) await load();
    refreshMe().catch(() => {});
  }

  function onPick(e) {
    uploadFiles(e.currentTarget.files);
    e.currentTarget.value = '';
  }

  function onPickVersion(e) {
    const f = e.currentTarget.files;
    if (f.length && versionTarget) uploadFiles([f[0]], versionTarget);
    e.currentTarget.value = '';
  }

  // Drag and drop from the desktop. Only files, not folders.
  let dragDepth = 0;
  const hasFiles = (e) => [...(e.dataTransfer?.types || [])].includes('Files');

  function onDragEnter(e) {
    if (!hasFiles(e) || !canWrite) return;
    e.preventDefault();
    dragDepth++;
    dragging = true;
  }
  function onDragOver(e) {
    if (!hasFiles(e) || !canWrite) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
  }
  function onDragLeave(e) {
    if (!hasFiles(e)) return;
    dragDepth = Math.max(0, dragDepth - 1);
    if (!dragDepth) dragging = false;
  }
  function onDrop(e) {
    if (!hasFiles(e) || !canWrite) return;
    e.preventDefault();
    dragDepth = 0;
    dragging = false;
    const items = [...e.dataTransfer.items].filter((i) => i.kind === 'file');
    const files = items.filter((i) => !i.webkitGetAsEntry?.()?.isDirectory).map((i) => i.getAsFile());
    if (files.length < items.length) toast("Folders can't be uploaded yet, only files.");
    uploadFiles(files);
  }

  // ---------------------------------------------------------------- actions

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

  async function moveToTrash(entry) {
    try {
      await trash(entry);
      rows = rows.filter((r) => r.node.id !== entry.node.id);
      const name = entry.meta.name;
      if (isOwner) {
        toast(`Moved ${name} to the trash`, {
          icon: 'trash-2',
          action: {
            label: 'Undo',
            onclick: async () => {
              try {
                await untrash(entry);
                await load();
              } catch (e) {
                toastError(e);
              }
            },
          },
        });
      } else {
        toast(`Deleted ${name}. ${here.node.owner} can restore it from their trash.`, { icon: 'trash-2' });
      }
    } catch (e) {
      toastError(e);
      load();
    }
  }

  const files = $derived(rows.filter((r) => r.node.kind === 'file'));

  function activate(entry) {
    if (entry.node.kind === 'folder') open(entry.node.id);
    else preview(entry);
  }

  // The list is fixed when the preview opens, so a reload behind it doesn't shift ← and →.
  const preview = (entry) => (dialog = { type: 'preview', entries: files, start: files.indexOf(entry) });

  function menuFor(entry) {
    const folder = entry.node.kind === 'folder';
    return [
      folder
        ? { label: 'Open', icon: 'folder-open', onclick: () => open(entry.node.id) }
        : { label: 'Preview', icon: 'eye', onclick: () => preview(entry) },
      ...(!folder ? [{ label: 'Download', icon: 'download', onclick: () => downloadEntry(entry) }] : []),
      ...(isOwner
        ? [
            { label: 'Share', icon: 'share-2', onclick: () => (dialog = { type: 'share', entry }) },
            { label: 'Public link', icon: 'link', onclick: () => (dialog = { type: 'link', entry }) },
          ]
        : []),
      ...(canWrite
        ? [
            'sep',
            { label: 'Rename', icon: 'pencil', onclick: () => (dialog = { type: 'rename', entry }) },
            { label: 'Move', icon: 'move', onclick: () => (dialog = { type: 'move', entry }) },
            ...(!folder
              ? [{ label: 'Upload new version', icon: 'file-up', onclick: () => ((versionTarget = entry), versionInput.click()) }]
              : []),
          ]
        : []),
      ...(!folder ? [{ label: 'Version history', icon: 'refresh-cw', onclick: () => (dialog = { type: 'versions', entry }) }] : []),
      ...(canWrite ? ['sep', { label: 'Move to trash', icon: 'trash-2', danger: true, onclick: () => moveToTrash(entry) }] : []),
    ];
  }

  const close = () => (dialog = null);
  const reload = () => load();
</script>

<svelte:window ondragenter={onDragEnter} ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop} />

<input bind:this={fileInput} type="file" multiple hidden onchange={onPick} />
<input bind:this={versionInput} type="file" hidden onchange={onPickVersion} />

<div class="flex flex-wrap items-start gap-x-4 gap-y-3">
  <div class="min-w-0 flex-1">
    <nav class="flex min-h-8 flex-wrap items-center gap-1 text-sm" aria-label="Folder path">
      {#if share}
        <button type="button" class="cursor-pointer rounded px-1 text-fg-muted hover:text-fg" onclick={() => go({ name: 'shared-with-me' })}>Shared with me</button>
        <Icon name="chevron-right" class="size-4 text-fg-faint" />
      {/if}
      {#each path as crumb, i (crumb.node.id)}
        {#if i}<Icon name="chevron-right" class="size-4 text-fg-faint" />{/if}
        {#if i === path.length - 1}
          <h1 class="truncate px-1 text-xl font-semibold tracking-tight">{crumb.meta.name}</h1>
        {:else}
          <button type="button" class="max-w-48 cursor-pointer truncate rounded px-1 text-fg-muted hover:text-fg" onclick={() => open(crumb.node.id)}>{crumb.meta.name}</button>
        {/if}
      {/each}
    </nav>
    {#if share}
      <p class="mt-1 flex flex-wrap items-center gap-2 px-1 text-[13px] text-fg-muted">
        <span>Shared by <span class="font-medium text-fg">{here?.node.owner}</span></span>
        <span class="badge {share.permission === 'write' ? 'badge-accent' : ''}">{share.permission === 'write' ? 'Can edit' : 'View only'}</span>
      </p>
    {/if}
  </div>
  {#if canWrite}
    <div class="flex gap-2">
      <button type="button" class="btn btn-secondary" disabled={!here} onclick={() => (dialog = { type: 'mkdir' })}>
        <Icon name="folder-plus" /> New folder
      </button>
      <button type="button" class="btn btn-primary" disabled={!here} onclick={() => fileInput.click()}>
        <Icon name="upload" /> Upload
      </button>
    </div>
  {/if}
</div>

<div class="card relative mt-6 overflow-hidden">
  {#if loading}
    <div aria-busy="true" aria-label="Loading">
      <div class="h-9 border-b border-line"></div>
      {#each [44, 32, 56, 38] as w, i (i)}
        <div class="flex h-12 items-center gap-3 border-b border-line px-4 last:border-b-0">
          <div class="skeleton size-4 shrink-0"></div>
          <div class="skeleton h-3.5" style:width="{w}%"></div>
        </div>
      {/each}
    </div>
  {:else if loadError}
    <div class="grid place-items-center gap-3 px-6 py-16 text-center">
      <Icon name="circle-alert" class="size-6 text-danger" />
      <p class="text-fg-muted">{loadError}</p>
      <div class="flex gap-2">
        <button type="button" class="btn btn-secondary" onclick={reload}><Icon name="refresh-cw" /> Try again</button>
        <button type="button" class="btn btn-ghost" onclick={() => open(session.me.keys.root_node_id)}>Back to my files</button>
      </div>
    </div>
  {:else if !rows.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center animate-enter">
      <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle">
        <Icon name={canWrite ? 'upload' : 'folder-open'} class="size-5 text-fg-muted" />
      </div>
      <p class="font-medium">This folder is empty</p>
      <p class="text-[13px] text-fg-muted">
        {canWrite ? 'Drop files anywhere on this page, or use Upload. They are encrypted before they leave your device.' : 'Nothing has been added here yet.'}
      </p>
    </div>
  {:else}
    <table class="table animate-enter">
      <thead>
        <tr>
          <th>Name</th>
          <th class="hidden w-28 text-right sm:table-cell">Size</th>
          <th class="hidden w-40 md:table-cell">Modified</th>
          <th class="w-12"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each rows as entry (entry.node.id)}
          {@const folder = entry.node.kind === 'folder'}
          <tr class="group" in:fade out:fade={{ duration: 120 }} animate:flip={flipParams()}>
            <td class="max-w-0">
              <button type="button" class="flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => activate(entry)}>
                <Icon name={folder ? 'folder' : fileIcon(entry.meta)} class="size-4 shrink-0 {folder ? 'text-accent-text' : 'text-fg-muted'}" />
                <span class="truncate font-medium group-hover:underline group-hover:underline-offset-4 group-hover:decoration-line-strong">{entry.meta.name}</span>
              </button>
            </td>
            <td class="hidden text-right text-fg-muted tabular-nums sm:table-cell">{folder ? '' : formatSize(entry.meta.size)}</td>
            <td class="hidden text-fg-muted md:table-cell" title={fullDate(entry.node.updated_at * 1000)}>{formatWhen(entry.node.updated_at * 1000)}</td>
            <td class="text-right"><Menu items={menuFor(entry)} label="Actions for {entry.meta.name}" /></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  {#if dragging}
    <div class="pointer-events-none absolute inset-0 grid place-items-center rounded-lg border-2 border-dashed border-accent bg-accent-soft/80">
      <p class="flex items-center gap-2 font-medium text-accent-text"><Icon name="upload" /> Drop to encrypt and upload to {here?.meta.name}</p>
    </div>
  {/if}
</div>

{#if rows.length}
  <p class="mt-3 px-1 text-xs text-fg-faint">
    {plural(rows.filter((r) => r.node.kind === 'folder').length, 'folder')}, {plural(files.length, 'file')}
  </p>
{/if}

{#if dialog?.type === 'mkdir'}
  <NameDialog
    title="New folder"
    confirmLabel="Create"
    onsave={async (name) => {
      await createFolder(here.node.id, here.key, name);
      await load();
    }}
    onclose={close} />
{:else if dialog?.type === 'rename'}
  <NameDialog
    title="Rename"
    initial={dialog.entry.meta.name}
    confirmLabel="Rename"
    onsave={async (name) => {
      await rename(dialog.entry, name);
      await load();
    }}
    onclose={close} />
{:else if dialog?.type === 'move'}
  <MoveDialog
    entry={dialog.entry}
    root={path[0]}
    currentFolderId={folderId}
    onmoved={(dest) => {
      toast(`Moved to ${dest}`, { kind: 'success' });
      load();
    }}
    onclose={close} />
{:else if dialog?.type === 'versions'}
  <VersionsDialog entry={dialog.entry} {canWrite} onchanged={() => (load(), refreshMe().catch(() => {}))} onclose={close} />
{:else if dialog?.type === 'preview'}
  <Preview entries={dialog.entries} start={dialog.start} fetch={fetchEntry} ondownload={downloadEntry} onclose={close} />
{:else if dialog?.type === 'share'}
  <ShareDialog entry={dialog.entry} onclose={close} />
{:else if dialog?.type === 'link'}
  <LinkDialog entry={dialog.entry} onclose={close} />
{/if}
