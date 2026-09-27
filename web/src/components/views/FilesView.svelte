<script>
  import { session, resolvePath, listFolder, createFolder, rename, trash, untrash, download, downloadZip, fetchEntry, upload, saveText, refreshMe } from '../../lib/cloud.svelte.js';
  import { toast, toastError, trackTransfer, errorMessage, sort, sortBy } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, fullDate, plural, sortEntries, nameError } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import Menu from '../Menu.svelte';
  import NameDialog from '../dialogs/NameDialog.svelte';
  import VersionsDialog from '../dialogs/VersionsDialog.svelte';
  import { fade, fly, flip, flipParams } from '../../lib/motion.js';
  import { SvelteSet } from 'svelte/reactivity';
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
  let folderInput;
  let searchInput = $state();
  let tbody = $state();
  let query = $state('');
  const selected = new SvelteSet(); // node ids
  let anchor = null; // last row clicked, for shift-click ranges
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
    selected.clear();
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

  function uploadFiles(files, existing = null) {
    const target = here;
    const dest = existing ? { existing } : { parentId: target.node.id, parentKey: target.key };
    return runUploads(
      [...files].map((file) => ({ file, dest, label: existing ? existing.meta.name : file.name })),
      target,
    );
  }

  // Files the OS adds to folders that nobody means to upload.
  const JUNK = new Set(['.ds_store', 'thumbs.db', 'desktop.ini']);

  /**
   * Upload a folder tree into the current folder. `items` is a list of
   * { dirs, file } where dirs is the folder path (["Photos", "2024"]) and
   * file may be null for an empty folder. Folders are created first, then
   * the files go up like any other upload.
   */
  async function uploadTree(items) {
    const target = here;
    const made = new Map([['', { id: target.node.id, key: target.key }]]);
    const taken = new Set(rows.map((r) => r.meta.name.toLowerCase()));
    const topName = new Map(); // a dropped folder's name -> the name it gets here
    const folderFor = async (dirs) => {
      let path = '';
      for (const [i, dir] of dirs.entries()) {
        const next = path ? `${path}/${dir}` : dir;
        if (!made.has(next)) {
          let name = dir;
          if (i === 0) {
            // Don't mix into an existing folder of the same name.
            if (!topName.has(dir)) {
              let n = dir;
              for (let k = 2; taken.has(n.toLowerCase()); k++) n = `${dir} (${k})`;
              taken.add(n.toLowerCase());
              topName.set(dir, n);
            }
            name = topName.get(dir);
          }
          const parent = made.get(path);
          made.set(next, await createFolder(parent.id, parent.key, name));
        }
        path = next;
      }
      return made.get(path);
    };
    const jobs = [];
    try {
      for (const { dirs, file } of items) {
        if (file && JUNK.has(file.name.toLowerCase())) continue;
        const parent = await folderFor(dirs);
        if (file) jobs.push({ file, dest: { parentId: parent.id, parentKey: parent.key }, label: [...dirs, file.name].join('/') });
      }
    } catch (e) {
      toastError(e);
    }
    if (target.node.id === folderId) await load();
    await runUploads(jobs, target);
  }

  async function runUploads(jobs, target) {
    if (!jobs.length) return;
    const one = async ({ file, dest, label }) => {
      const t = trackTransfer('upload', label, file.size);
      try {
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
      while (next < jobs.length) await one(jobs[next++]);
    };
    await Promise.all(Array.from({ length: Math.min(3, jobs.length) }, worker));
    if (target.node.id === folderId) await load();
    refreshMe().catch(() => {});
  }

  function onPick(e) {
    uploadFiles(e.currentTarget.files);
    e.currentTarget.value = '';
  }

  function onPickFolder(e) {
    // Each file carries its path inside the picked folder ("Photos/2024/a.jpg").
    const items = [...e.currentTarget.files].map((file) => ({ dirs: file.webkitRelativePath.split('/').slice(0, -1), file }));
    e.currentTarget.value = '';
    if (items.length) uploadTree(items);
  }

  function onPickVersion(e) {
    const f = e.currentTarget.files;
    if (f.length && versionTarget) uploadFiles([f[0]], versionTarget);
    e.currentTarget.value = '';
  }

  // Drag and drop from the desktop: files and whole folders.
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
    // Entries must be taken now; the drop's data is gone once this returns.
    const entries = [...e.dataTransfer.items].filter((i) => i.kind === 'file').map((i) => i.webkitGetAsEntry?.() ?? i.getAsFile());
    const files = [...e.dataTransfer.files];
    if (entries.some((x) => x?.isDirectory)) readDropped(entries).then(uploadTree, toastError);
    else uploadFiles(files);
  }

  /** Walk dropped files and folders into { dirs, file } items. */
  async function readDropped(entries) {
    const out = [];
    const fileOf = (entry) => new Promise((resolve, reject) => entry.file(resolve, reject));
    async function walk(entry, dirs) {
      if (entry instanceof File) return out.push({ dirs, file: entry });
      if (entry.isFile) return out.push({ dirs, file: await fileOf(entry) });
      const here = [...dirs, entry.name];
      const reader = entry.createReader();
      let any = false;
      // readEntries returns at most ~100 at a time; call until it's empty.
      for (;;) {
        const batch = await new Promise((resolve, reject) => reader.readEntries(resolve, reject));
        if (!batch.length) break;
        any = true;
        for (const child of batch) await walk(child, here);
      }
      if (!any) out.push({ dirs: here, file: null });
    }
    for (const entry of entries) if (entry) await walk(entry, []);
    return out;
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

  // ------------------------------------------------------------ selection

  // Only rows still in the folder count (a reload may have removed some).
  const chosen = $derived(rows.filter((r) => selected.has(r.node.id)));
  const allVisibleSelected = $derived(visible.length > 0 && visible.every((r) => selected.has(r.node.id)));

  function toggle(entry, e) {
    const id = entry.node.id;
    const i = visible.indexOf(entry);
    if (e?.shiftKey && anchor !== null) {
      const a = visible.findIndex((r) => r.node.id === anchor);
      if (a !== -1) {
        const on = !selected.has(id);
        for (const r of visible.slice(Math.min(a, i), Math.max(a, i) + 1)) on ? selected.add(r.node.id) : selected.delete(r.node.id);
        anchor = id;
        return;
      }
    }
    selected.has(id) ? selected.delete(id) : selected.add(id);
    anchor = id;
  }

  function toggleAll() {
    if (allVisibleSelected) for (const r of visible) selected.delete(r.node.id);
    else for (const r of visible) selected.add(r.node.id);
  }

  function downloadChosen() {
    const list = [...chosen];
    if (list.length === 1 && list[0].node.kind === 'file') return downloadEntry(list[0]);
    return zipEntries(list, `${here.meta.name}.zip`);
  }

  async function zipEntries(list, name) {
    const t = trackTransfer('download', name, null);
    try {
      await downloadZip(list, name, (p) => (t.progress = p));
      t.status = 'done';
    } catch (e) {
      t.status = 'error';
      t.error = errorMessage(e);
    }
  }

  async function trashChosen() {
    const list = [...chosen];
    const done = [];
    for (const entry of list) {
      try {
        await trash(entry);
        done.push(entry);
      } catch (e) {
        toastError(e);
      }
    }
    selected.clear();
    const gone = new Set(done.map((d) => d.node.id));
    rows = rows.filter((r) => !gone.has(r.node.id));
    if (!done.length) return;
    const what = done.length === 1 ? done[0].meta.name : plural(done.length, 'item');
    if (!isOwner) return toast(`Deleted ${what}. ${here.node.owner} can restore them from their trash.`, { icon: 'trash-2' });
    toast(`Moved ${what} to the trash`, {
      icon: 'trash-2',
      action: {
        label: 'Undo',
        onclick: async () => {
          try {
            for (const entry of done) await untrash(entry);
          } catch (e) {
            toastError(e);
          }
          await load();
        },
      },
    });
  }

  // ---------------------------------------------------------- inline rename

  let renaming = $state(null); // node id being renamed in place
  let renameValue = $state('');
  let renameBusy = false;

  function startRename(entry) {
    renaming = entry.node.id;
    renameValue = entry.meta.name;
  }

  /** Svelte action: focus the field and select `name` without its extension. */
  function selectName(input, name) {
    // After bind:value has filled the field in.
    queueMicrotask(() => {
      input.focus();
      const dot = name.lastIndexOf('.');
      input.setSelectionRange(0, dot > 0 ? dot : name.length);
    });
  }

  async function finishRename(entry, save = true) {
    if (renaming !== entry.node.id || renameBusy) return;
    const name = renameValue.trim();
    if (!save || name === entry.meta.name) return (renaming = null);
    const err = nameError(name);
    if (err) return toast(err);
    renameBusy = true;
    try {
      await rename(entry, name);
      renaming = null;
      await load();
      rowButtons().find((b) => b.textContent.trim() === name)?.focus();
    } catch (e) {
      toastError(e);
    } finally {
      renameBusy = false;
    }
  }

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
      folder
        ? { label: 'Download as zip', icon: 'download', onclick: () => zipEntries([entry], `${entry.meta.name}.zip`) }
        : { label: 'Download', icon: 'download', onclick: () => downloadEntry(entry) },
      ...(isOwner
        ? [
            { label: 'Share', icon: 'share-2', onclick: () => (dialog = { type: 'share', entry }) },
            { label: 'Public link', icon: 'link', onclick: () => (dialog = { type: 'link', entry }) },
          ]
        : []),
      ...(canWrite
        ? [
            'sep',
            { label: 'Rename', icon: 'pencil', onclick: () => startRename(entry) },
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

  const rowButtons = () => [...(tbody?.querySelectorAll('button.row-open') ?? [])];

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
    if (e.key === 'Escape' && selected.size && !typing) {
      selected.clear();
      e.preventDefault();
      return;
    }
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
    else if (key === 'Delete' && canWrite && selected.size) trashChosen();
    else if (key === 'Delete' && canWrite && focusedEntry()) {
      const entry = focusedEntry();
      const list = rowButtons();
      const i = list.indexOf(document.activeElement);
      moveToTrash(entry).then(() => rowButtons()[Math.min(i, rowButtons().length - 1)]?.focus());
    } else if (key === 'x' && focusedEntry()) toggle(focusedEntry());
    else if (key === 'F2' && canWrite && focusedEntry()) startRename(focusedEntry());
    else if (key === '?') dialog = { type: 'shortcuts' };
    else return;
    e.preventDefault();
  }

  const sortLabel = { name: 'Name', size: 'Size', modified: 'Modified' };

  const close = () => (dialog = null);
  const reload = () => load();
</script>

<svelte:window {onkeydown} ondragenter={onDragEnter} ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop} />

<input bind:this={fileInput} type="file" multiple hidden onchange={onPick} />
<input bind:this={folderInput} type="file" webkitdirectory hidden onchange={onPickFolder} />
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
      <Menu
        label="Upload"
        buttonClass="btn btn-primary"
        items={[
          { label: 'Files', icon: 'file-up', onclick: () => fileInput.click() },
          { label: 'Folder', icon: 'folder-up', onclick: () => folderInput.click() },
        ]}>
        {#snippet trigger()}<Icon name="upload" /> Upload<Icon name="chevron-down" class="-mr-1 size-3.5 opacity-70" />{/snippet}
      </Menu>
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
      {#if path.length === 1 && !share}
        <!-- A brand moment: the very first, empty "My files". -->
        <img src="/img/logo.webp" alt="" width="715" height="349" class="mb-4 h-auto w-40 select-none" draggable="false" />
        <p class="font-medium">Nothing here yet</p>
        <p class="max-w-sm text-[13px] text-fg-muted">
          Drop files or whole folders anywhere on this page, or use Upload. Everything is encrypted before it leaves your device.
        </p>
      {:else}
        <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle">
          <Icon name={canWrite ? 'upload' : 'folder-open'} class="size-5 text-fg-muted" />
        </div>
        <p class="font-medium">This folder is empty</p>
        <p class="text-[13px] text-fg-muted">
          {canWrite ? 'Drop files anywhere on this page, or use Upload. They are encrypted before they leave your device.' : 'Nothing has been added here yet.'}
        </p>
      {/if}
    </div>
  {:else}
    <table class="table animate-enter">
      <thead>
        <tr>
          <th class="w-10 !pr-0">
            <input
              type="checkbox"
              class="size-4 cursor-pointer align-middle accent-accent"
              aria-label="Select all"
              checked={allVisibleSelected}
              indeterminate={!allVisibleSelected && visible.some((r) => selected.has(r.node.id))}
              onchange={toggleAll} />
          </th>
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
            <td colspan="5" class="h-24 text-center text-[13px] text-fg-muted">
              Nothing in this folder matches "{query.trim()}". <button type="button" class="link" onclick={() => (query = '')}>Clear search</button>
            </td>
          </tr>
        {/if}
        {#each visible as entry (entry.node.id)}
          {@const folder = entry.node.kind === 'folder'}
          {@const isSelected = selected.has(entry.node.id)}
          <tr class="group {isSelected ? 'bg-accent-soft/60 hover:bg-accent-soft/60' : ''}" aria-selected={isSelected} in:fade out:fade={{ duration: 120 }} animate:flip={flipParams()}>
            <td class="w-10 !pr-0">
              <input
                type="checkbox"
                class="size-4 cursor-pointer align-middle accent-accent transition-opacity focus-visible:opacity-100 {selected.size ? '' : 'opacity-0 group-hover:opacity-100'}"
                aria-label="Select {entry.meta.name}"
                checked={isSelected}
                onclick={(e) => {
                  e.preventDefault();
                  toggle(entry, e);
                }} />
            </td>
            <td class="max-w-0">
              {#if renaming === entry.node.id}
                <form
                  class="flex items-center gap-3"
                  onsubmit={(e) => {
                    e.preventDefault();
                    finishRename(entry);
                  }}>
                  {#if folder}<Icon name="folder" class="size-4 shrink-0 text-accent-text" />{:else}<FileIcon meta={entry.meta} />{/if}
                  <input
                    use:selectName={entry.meta.name}
                    class="input h-7 max-w-md px-2 font-medium"
                    aria-label="New name for {entry.meta.name}"
                    bind:value={renameValue}
                    spellcheck="false"
                    onkeydown={(e) => e.key === 'Escape' && (e.preventDefault(), e.stopPropagation(), finishRename(entry, false))}
                    onblur={() => finishRename(entry)} />
                </form>
              {:else}
                <button type="button" class="row-open flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => activate(entry)}>
                  {#if folder}<Icon name="folder" class="size-4 shrink-0 text-accent-text" />{:else}<FileIcon meta={entry.meta} />{/if}
                  <span class="truncate font-medium group-hover:underline group-hover:underline-offset-4 group-hover:decoration-line-strong">{entry.meta.name}</span>
                </button>
              {/if}
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

{#if selected.size && chosen.length}
  <div class="fixed inset-x-0 bottom-6 z-40 flex justify-center px-4" transition:fly={{ y: 12 }}>
    <div class="flex items-center gap-1 rounded-lg border border-line bg-bg p-1.5 pl-3 shadow-lg shadow-black/5 dark:shadow-black/40" role="toolbar" aria-label="Selection">
      <span class="mr-2 text-sm font-medium tabular-nums">{chosen.length} selected</span>
      <button type="button" class="btn btn-ghost" onclick={downloadChosen} disabled={!chosen.some((r) => r.node.kind === 'file')}>
        <Icon name="download" /><span class="hidden sm:inline">Download</span>
      </button>
      {#if canWrite}
        <button type="button" class="btn btn-ghost" onclick={() => (dialog = { type: 'move', entries: [...chosen] })}>
          <Icon name="move" /><span class="hidden sm:inline">Move</span>
        </button>
        <button type="button" class="btn btn-ghost text-danger hover:text-danger" onclick={trashChosen}>
          <Icon name="trash-2" /><span class="hidden sm:inline">Move to trash</span>
        </button>
      {/if}
      <span class="mx-1 h-5 w-px bg-line" aria-hidden="true"></span>
      <button type="button" class="btn btn-ghost btn-icon" aria-label="Clear selection" title="Clear selection (Esc)" onclick={() => selected.clear()}>
        <Icon name="x" />
      </button>
    </div>
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
{:else if dialog?.type === 'move'}
  <MoveDialog
    entries={dialog.entries ?? [dialog.entry]}
    root={path[0]}
    currentFolderId={folderId}
    onmoved={(dest, count) => {
      toast(count > 1 ? `Moved ${count} items to ${dest}` : `Moved to ${dest}`, { kind: 'success' });
      selected.clear();
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
