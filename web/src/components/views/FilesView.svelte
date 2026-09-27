<script>
  import { session, resolvePath, listFolder, createFolder, rename, trash, untrash, download, fetchEntry, upload, saveText, refreshMe } from '../../lib/cloud.svelte.js';
  import { toast, toastError, trackTransfer, errorMessage, sort, sortBy } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, fullDate, plural, sortEntries } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import Menu from '../Menu.svelte';
  import NameDialog from '../dialogs/NameDialog.svelte';
  import VersionsDialog from '../dialogs/VersionsDialog.svelte';
  import { fade, flip, flipParams } from '../../lib/motion.js';
  import MoveDialog from '../dialogs/MoveDialog.svelte';
  import ShareDialog from '../dialogs/ShareDialog.svelte';
  import LinkDialog from '../dialogs/LinkDialog.svelte';
  import Preview from '../Preview.svelte';
  import ShortcutsDialog from '../dialogs/ShortcutsDialog.svelte';

  let { folderId, go, inShare = $bindable(false) } = $props();

  let path = $state([]); // [{ node, key, meta }] from the tree root down
  let share = $state(null); // set when browsing a folder shared with us
  let rows = $state([]);
  let loading = $state(true);
  let loadError = $state('');
  let dialog = $state(null); // { type, entry }
  let dragging = $state(false);
  let fileInput;
  let searchInput = $state();
  let tbody = $state();
  let query = $state('');
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
    query = '';
    load(folderId);
  });

  // What the table shows: the folder filtered by the search box (names are
  // decrypted here, so this never involves the server), in the chosen order.
  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return sortEntries(
      rows.filter((r) => !q || r.meta.name.toLowerCase().includes(q)),
      sort,
    );
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

  const files = $derived(visible.filter((r) => r.node.kind === 'file'));

  function activate(entry) {
    if (entry.node.kind === 'folder') open(entry.node.id);
    else preview(entry);
  }

  // The list is fixed when the preview opens, so a reload behind it doesn't shift ← and →.
  const preview = (entry, edit = false) => (dialog = { type: 'preview', entries: files, start: files.indexOf(entry), edit });

  function untitledName() {
    const taken = new Set(rows.map((r) => r.meta.name.toLowerCase()));
    for (let i = 1; ; i++) {
      const name = i === 1 ? 'Untitled.md' : `Untitled ${i}.md`;
      if (!taken.has(name.toLowerCase())) return name;
    }
  }

  async function newNote(name) {
    const file = new File([''], /\.(md|markdown)$/i.test(name) ? name : `${name}.md`, { type: 'text/markdown' });
    const created = await upload(file, { parentId: here.node.id, parentKey: here.key });
    await load();
    const entry = rows.find((r) => r.node.id === created.node.id);
    if (entry) preview(entry, true);
  }

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

  // ---------------------------------------------------------- keyboard

  const rowButtons = () => [...(tbody?.querySelectorAll('td:first-child > button') ?? [])];

  function focusRow(delta) {
    const list = rowButtons();
    if (!list.length) return;
    const i = list.indexOf(document.activeElement);
    const next = i === -1 ? (delta > 0 ? 0 : list.length - 1) : Math.max(0, Math.min(list.length - 1, i + delta));
    list[next].focus();
    list[next].scrollIntoView({ block: 'nearest' });
  }

  function focusedEntry() {
    const i = rowButtons().indexOf(document.activeElement);
    return i === -1 ? null : visible[i];
  }

  function onkeydown(e) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    if (dialog || document.querySelector('dialog[open]')) return;
    const t = e.target;
    const typing = t instanceof HTMLElement && (t.isContentEditable || !!t.closest('input, textarea, select'));
    if (t === searchInput) {
      if (e.key === 'Escape') {
        query = '';
        searchInput.blur();
      } else if (e.key === 'ArrowDown') focusRow(1);
      else return;
      e.preventDefault();
      return;
    }
    if (typing) return;
    const key = e.key;
    if (key === '/') searchInput?.focus();
    else if (key === 'j' || key === 'ArrowDown') focusRow(1);
    else if (key === 'k' || key === 'ArrowUp') focusRow(-1);
    else if (key === 'Backspace' && path.length > 1) open(path[path.length - 2].node.id);
    else if (key === 'n' && canWrite && here) dialog = { type: 'mkdir' };
    else if (key === 'u' && canWrite && here) fileInput.click();
    else if (key === 'Delete' && canWrite && focusedEntry()) {
      const entry = focusedEntry();
      const list = rowButtons();
      const i = list.indexOf(document.activeElement);
      moveToTrash(entry).then(() => rowButtons()[Math.min(i, rowButtons().length - 1)]?.focus());
    } else if (key === '?') dialog = { type: 'shortcuts' };
    else return;
    e.preventDefault();
  }

  const sortLabel = { name: 'Name', size: 'Size', modified: 'Modified' };

  const close = () => (dialog = null);
  const reload = () => load();
</script>

<svelte:window {onkeydown} ondragenter={onDragEnter} ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop} />

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
  <div class="flex flex-wrap gap-2">
    {#if rows.length}
      <label class="relative block">
        <span class="sr-only">Search this folder</span>
        <Icon name="search" class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-fg-faint" />
        <input
          bind:this={searchInput}
          bind:value={query}
          type="search"
          class="input h-8 w-44 pr-8 pl-8 sm:w-56"
          placeholder="Search"
          autocomplete="off"
          spellcheck="false" />
        {#if !query}<kbd class="kbd pointer-events-none absolute top-1/2 right-2 -translate-y-1/2">/</kbd>{/if}
      </label>
    {/if}
    {#if canWrite}
      <button type="button" class="btn btn-secondary" disabled={!here} onclick={() => (dialog = { type: 'note' })}>
        <Icon name="file-plus" /> New note
      </button>
      <button type="button" class="btn btn-secondary" disabled={!here} onclick={() => (dialog = { type: 'mkdir' })}>
        <Icon name="folder-plus" /> New folder
      </button>
      <button type="button" class="btn btn-primary" disabled={!here} onclick={() => fileInput.click()}>
        <Icon name="upload" /> Upload
      </button>
    {/if}
  </div>
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
          {#snippet sortHeader(key, cls = '')}
            <th class={cls} aria-sort={sort.key === key ? (sort.dir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button type="button" class="inline-flex cursor-pointer items-center gap-1 hover:text-fg {sort.key === key ? 'text-fg' : ''}" onclick={() => sortBy(key)}>
                {sortLabel[key]}
                {#if sort.key === key}<Icon name={sort.dir === 'asc' ? 'arrow-up' : 'arrow-down'} class="size-3" />{/if}
              </button>
            </th>
          {/snippet}
          {@render sortHeader('name')}
          {@render sortHeader('size', 'hidden w-28 text-right sm:table-cell')}
          {@render sortHeader('modified', 'hidden w-40 md:table-cell')}
          <th class="w-12"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody bind:this={tbody}>
        {#if !visible.length}
          <tr>
            <td colspan="4" class="h-24 text-center text-[13px] text-fg-muted">
              Nothing in this folder matches "{query.trim()}". <button type="button" class="link" onclick={() => (query = '')}>Clear search</button>
            </td>
          </tr>
        {/if}
        {#each visible as entry (entry.node.id)}
          {@const folder = entry.node.kind === 'folder'}
          <tr class="group" in:fade out:fade={{ duration: 120 }} animate:flip={flipParams()}>
            <td class="max-w-0">
              <button type="button" class="flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => activate(entry)}>
                {#if folder}<Icon name="folder" class="size-4 shrink-0 text-accent-text" />{:else}<FileIcon meta={entry.meta} />{/if}
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
  <div class="mt-3 flex items-center justify-between gap-4 px-1 text-xs text-fg-faint">
    <p>
      {#if query.trim()}{visible.length} of {rows.length} shown ·{/if}
      {plural(rows.filter((r) => r.node.kind === 'folder').length, 'folder')}, {plural(rows.filter((r) => r.node.kind === 'file').length, 'file')}
    </p>
    <button type="button" class="hidden cursor-pointer items-center gap-1.5 hover:text-fg-muted sm:flex" onclick={() => (dialog = { type: 'shortcuts' })}>
      <Icon name="keyboard" class="size-3.5" /> Press <kbd class="kbd">?</kbd> for shortcuts
    </button>
  </div>
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
{:else if dialog?.type === 'shortcuts'}
  <ShortcutsDialog onclose={close} />
{:else if dialog?.type === 'note'}
  <NameDialog
    title="New note"
    initial={untitledName()}
    confirmLabel="Create"
    create
    onsave={newNote}
    onclose={() => dialog?.type === 'note' && close()} />
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
  <Preview
    entries={dialog.entries}
    start={dialog.start}
    edit={dialog.edit}
    fetch={fetchEntry}
    save={canWrite ? saveText : null}
    onsaved={() => (load(), refreshMe().catch(() => {}))}
    ondownload={downloadEntry}
    onclose={close} />
{:else if dialog?.type === 'share'}
  <ShareDialog entry={dialog.entry} onclose={close} />
{:else if dialog?.type === 'link'}
  <LinkDialog entry={dialog.entry} onclose={close} />
{/if}
