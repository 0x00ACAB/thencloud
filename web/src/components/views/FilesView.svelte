<script>
  import { session, resolvePath, listFolder, createFolder, rename, move, trash, untrash, download, downloadZip, fetchEntry, upload, saveText, refreshMe, toolsInfo, loadDraft, storeDraft, dropDraft, searchTree, searchContents, openEntry, strayDrops, bookProgress, watchFolder } from '../../lib/cloud.svelte.js';
  import { toast, toastError, trackTransfer, errorMessage, sort, sortBy, photoDetails, fileView, setFileView } from '../../lib/ui.svelte.js';
  import { fileInfo, hasDetails, stripFile } from '../../lib/exif.js';
  import Modal from '../Modal.svelte';
  import { formatSize, formatWhen, fullDate, sortEntries, nameError, changedAt } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';
  import Thumb from '../Thumb.svelte';
  import Menu from '../Menu.svelte';
  import NameDialog from '../dialogs/NameDialog.svelte';
  import VersionsDialog from '../dialogs/VersionsDialog.svelte';
  import CommentsDialog from '../dialogs/CommentsDialog.svelte';
  import ActivityDialog from '../dialogs/ActivityDialog.svelte';
  import { fade, fly, flip, flipParams } from '../../lib/motion.js';
  import { SvelteSet } from 'svelte/reactivity';
  import MoveDialog from '../dialogs/MoveDialog.svelte';
  import ShareDialog from '../dialogs/ShareDialog.svelte';
  import LinkDialog from '../dialogs/LinkDialog.svelte';
  import Preview from '../Preview.svelte';
  import ShortcutsDialog from '../dialogs/ShortcutsDialog.svelte';
  import ConvertDialog from '../dialogs/ConvertDialog.svelte';
  import BatchConvertDialog from '../dialogs/BatchConvertDialog.svelte';
  import StrayDropsDialog from '../dialogs/StrayDropsDialog.svelte';
  import VideoDownloadDialog from '../dialogs/VideoDownloadDialog.svelte';
  import { onMount, tick, untrack } from 'svelte';
  import { sourceKind } from '../../lib/convert.js';
  import { t } from '../../lib/i18n.svelte.js';
  import PdfToolsDialog from '../dialogs/PdfToolsDialog.svelte';
  // Kept here: pdfedit.js pulls in pdf-lib, which loads only with the dialog.
  const isPdf = (meta) => meta.mime === 'application/pdf' || /\.pdf$/i.test(meta.name);
  import { previewKind } from '../../lib/preview.js';
  import { play, enqueue, makeTrack } from '../../lib/music.svelte.js';
  import { isFavourite, toggleFavourite, noteRecent } from '../../lib/places.svelte.js';

  let { folderId, openId = null, go, inShare = $bindable(false) } = $props();

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

  /** `reopen`: show the file named in the URL, as on the first load. */
  async function load(id = folderId, reopen = true) {
    loadError = '';
    try {
      const p = await resolvePath(id);
      const list = await listFolder(id, p.items[p.items.length - 1].key);
      if (id !== folderId) return; // navigated away meanwhile
      path = p.items;
      share = p.share;
      inShare = !!p.share;
      rows = list;
      // Opened from a search result: show that file.
      const wanted = openId && rows.find((r) => r.node.id === openId && r.node.kind === 'file');
      if (reopen && wanted && !dialog) untrack(() => preview(wanted));
    } catch (e) {
      loadError = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  // Live updates: someone else (or another tab) changed something here.
  // Reloaded quietly a moment later, and not while a name is being edited.
  $effect(() => {
    const id = folderId;
    let timer = null;
    const stop = watchFolder(id, () => {
      clearTimeout(timer);
      timer = setTimeout(function again() {
        if (renaming || document.hidden) return (timer = setTimeout(again, 1000));
        load(id, false);
      }, 600);
    });
    return () => (stop(), clearTimeout(timer));
  });

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
  // The root folder's own name was set when the account was made; show it in the current language.
  const folderName = (e) => (e?.node.id === session.me.keys.root_node_id ? t('My files') : e?.meta.name);

  // Big folders. The list keeps only the rows near the screen in the page,
  // with a spacer above and below standing in for the rest, and the grid
  // adds tiles as you scroll down, so 100,000 items stay as quick as 100.
  // Smaller folders render in full, with their animations.
  const WINDOWED = 400;
  const windowed = $derived(visible.length > WINDOWED);
  let rowH = $state(49);
  let win = $state({ start: 0, end: WINDOWED });
  const shown = $derived(windowed ? visible.slice(win.start, win.end) : visible);
  const GRID_STEP = 300;
  let gridLimit = $state(GRID_STEP);
  const gridShown = $derived(visible.length > gridLimit ? visible.slice(0, gridLimit) : visible);
  // No fading or sliding while rows come and go by scrolling.
  const motion = (d) => (windowed ? { duration: 0 } : d);

  let frame = 0;
  function updateWindow() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (!windowed || !tbody || fileView.value === 'grid') return;
      const row = tbody.querySelector('tr[data-row]');
      if (row?.offsetHeight) rowH = row.offsetHeight;
      const top = tbody.getBoundingClientRect().top;
      const start = Math.max(0, Math.floor(-top / rowH) - 30);
      const end = Math.min(visible.length, start + Math.ceil(innerHeight / rowH) + 60);
      if (start !== win.start || end !== win.end) win = { start, end };
    });
  }
  $effect(() => {
    // Again whenever the rows or the layout change.
    visible;
    fileView.value;
    tbody;
    updateWindow();
  });
  $effect(() => {
    folderId;
    gridLimit = GRID_STEP;
    win = { start: 0, end: WINDOWED };
  });

  /** Calls `more` when the element comes near the screen. */
  function nearScreen(node, more) {
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && more(), { rootMargin: '800px' });
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }

  /** Make sure row `i` of `visible` is in the page (scrolling to it if the list is windowed). */
  async function revealRow(i) {
    if (!windowed || (i >= win.start && i < win.end)) return;
    const top = tbody.getBoundingClientRect().top + scrollY;
    scrollTo({ top: top + i * rowH - innerHeight / 2 });
    const start = Math.max(0, i - 60);
    win = { start, end: Math.min(visible.length, start + Math.ceil(innerHeight / rowH) + 120) };
    await tick();
  }

  // Search everywhere below the top of this tree (My files, or the shared
  // folder we're in), not just this folder; "Inside files" also looks in
  // text, Markdown and PDF files' words (decrypted here, see fulltext.js).
  let scope = $state('folder'); // folder | all | contents
  let found = $state([]);
  let searching = $state(false);
  let reading = $state(null); // { done, total } while files are read to search inside them
  let searchRun = null;
  $effect(() => {
    const q = query.trim();
    const top = path[0];
    searchRun?.abort();
    found = [];
    reading = null;
    if (scope === 'folder' || !q || !top) return;
    const run = (searchRun = new AbortController());
    const timer = setTimeout(async () => {
      searching = true;
      const hits = new Map();
      const onResult = (r) => {
        if (hits.has(r.node.id)) return;
        hits.set(r.node.id, r);
        if (hits.size <= 500) found = sortEntries([...hits.values()], sort);
      };
      await searchTree(top, q, { signal: run.signal, onResult });
      if (scope === 'contents' && !run.signal.aborted) {
        await searchContents(top, q, {
          signal: run.signal,
          onResult,
          onProgress: (done, total) => !run.signal.aborted && (reading = done < total ? { done, total } : null),
        }).catch((e) => toastError(e));
      }
      if (!run.signal.aborted) searching = false;
    }, 300);
    return () => {
      clearTimeout(timer);
      run.abort();
      searching = false;
    };
  });

  function openFound(r) {
    if (r.node.kind === 'folder') return open(r.node.id);
    if (r.parentId === folderId) {
      const hit = rows.find((x) => x.node.id === r.node.id);
      return hit && preview(hit);
    }
    go({ name: 'files', folderId: r.parentId, open: r.node.id });
  }

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

  /**
   * Location and camera details in photos: removed, kept, or (by default)
   * asked about when a photo has a location. Marks the jobs to clean; the
   * cleaning happens as each one uploads. Resolves to false if cancelled.
   */
  async function checkPhotos(jobs) {
    if (photoDetails.value === 'keep') return true;
    const found = [];
    for (const job of jobs) {
      const info = await fileInfo(job.file).catch(() => null);
      if (hasDetails(info)) found.push({ job, info });
    }
    if (!found.length) return true;
    if (photoDetails.value === 'ask') {
      const located = found.filter((f) => f.info.gps).length;
      if (!located) return true;
      const choice = await new Promise((resolve) => (dialog = { type: 'photo-details', located, resolve }));
      dialog = null;
      if (choice === 'cancel') return false;
      if (choice === 'keep') return true;
    }
    for (const f of found) f.job.clean = true;
    return true;
  }

  async function runUploads(jobs, target) {
    if (!jobs.length) return;
    if (!(await checkPhotos(jobs))) return;
    const one = async ({ file, dest, label, clean }) => {
      const job = trackTransfer('upload', label, file.size);
      try {
        if (clean) {
          file = await stripFile(file);
          job.size = file.size;
        }
        await upload(file, dest, (p) => (job.progress = p));
        job.status = 'done';
      } catch (e) {
        job.status = 'error';
        job.error = e?.code === 'quota_exceeded' ? t('Not enough storage left') : errorMessage(e);
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

  // Paste to upload: a screenshot or files copied in the file manager.
  function onPaste(e) {
    if (!canWrite || !here || dialog || document.querySelector('dialog[open]')) return;
    const target = e.target;
    if (target instanceof HTMLElement && (target.isContentEditable || target.closest('input, textarea, select'))) return;
    const files = [...(e.clipboardData?.files ?? [])];
    if (!files.length) return;
    e.preventDefault();
    uploadFiles(files.map(pastedName));
  }

  /** Browsers call every pasted screenshot "image.png"; give it a date instead. */
  function pastedName(file) {
    if (!/^image\.\w+$/i.test(file.name)) return file;
    const d = new Date();
    const pad = (n) => String(n).padStart(2, '0');
    const stamp = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}.${pad(d.getMinutes())}.${pad(d.getSeconds())}`;
    return new File([file], `${t('Pasted image')} ${stamp}${file.name.slice(file.name.lastIndexOf('.'))}`, { type: file.type, lastModified: file.lastModified });
  }

  // ---------------------------------------------------------- drag to move

  // Rows dragged onto a folder row or a folder in the path are moved there.
  // The drag carries nothing but a type; what's being moved stays here.
  const NODES = 'application/x-thencloud-nodes';
  let dragged = null; // entries being dragged
  let dropTarget = $state(null); // node id of the folder under the pointer

  function rowDragStart(e, entry) {
    if (!canWrite || renaming) return e.preventDefault();
    dragged = selected.has(entry.node.id) ? [...chosen] : [entry];
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData(NODES, '');
  }

  function rowDragEnd() {
    dragged = null;
    dropTarget = null;
  }

  function dragOverFolder(e, id) {
    if (!dragged || id === folderId || dragged.some((d) => d.node.id === id)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    dropTarget = id;
  }

  function dragLeaveFolder(e, id) {
    if (dropTarget === id && !e.currentTarget.contains(e.relatedTarget)) dropTarget = null;
  }

  async function dropOnFolder(e, target) {
    if (!dragged || target.node.id === folderId) return;
    e.preventDefault();
    const list = dragged.filter((d) => d.node.id !== target.node.id);
    rowDragEnd();
    let done = 0;
    for (const entry of list) {
      try {
        await move(entry, target.node.id, target.key);
        done++;
      } catch (err) {
        toast(t("Couldn't move {name}: {error}", { name: entry.meta.name, error: errorMessage(err) }), { kind: 'error' });
      }
    }
    if (done) toast(done > 1 ? t('Moved {count} items to {folder}', { count: done, folder: target.meta.name }) : t('Moved {name} to {folder}', { name: list[0].meta.name, folder: target.meta.name }), { kind: 'success' });
    selected.clear();
    load();
  }

  // ---------------------------------------------------------------- actions

  async function downloadEntry(entry) {
    noteRecent(entry.node.id);
    const job = trackTransfer('download', entry.meta.name, entry.meta.size);
    try {
      await download(entry, (p) => (job.progress = p));
      job.status = 'done';
    } catch (e) {
      job.status = 'error';
      job.error = errorMessage(e);
    }
  }

  async function moveToTrash(entry) {
    try {
      await trash(entry);
      rows = rows.filter((r) => r.node.id !== entry.node.id);
      const name = entry.meta.name;
      if (isOwner) {
        toast(t('Moved {name} to the trash', { name }), {
          icon: 'trash-2',
          action: {
            label: t('Undo'),
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
        toast(t('Deleted {name}. {owner} can restore it from their trash.', { name, owner: here.node.owner }), { icon: 'trash-2' });
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

  // Touch: long-press a row to select it; while something is selected, a
  // tap toggles a row instead of opening it (there's no hover to reveal the
  // checkboxes).
  const touch = typeof matchMedia === 'function' && matchMedia('(pointer: coarse)').matches;
  let press = null; // { timer, x, y, fired }

  function pressStart(e, entry) {
    if (e.pointerType === 'mouse') return;
    const p = { x: e.clientX, y: e.clientY, fired: false };
    p.timer = setTimeout(() => {
      p.fired = true;
      toggle(entry);
      navigator.vibrate?.(10);
    }, 450);
    press = p;
  }
  function pressMove(e) {
    if (press && Math.hypot(e.clientX - press.x, e.clientY - press.y) > 10) pressEnd();
  }
  function pressEnd() {
    if (press) clearTimeout(press.timer);
  }

  function rowTap(entry) {
    const long = press?.fired;
    press = null;
    if (long) return; // the long press already selected it
    if (touch && selected.size) return toggle(entry);
    activate(entry);
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
    const job = trackTransfer('download', name, null);
    try {
      await downloadZip(list, name, (p) => (job.progress = p));
      job.status = 'done';
    } catch (e) {
      job.status = 'error';
      job.error = errorMessage(e);
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
    const what = done.length === 1 ? done[0].meta.name : t('{count} items', { count: done.length });
    if (!isOwner) return toast(t('Deleted {name}. {owner} can restore it from their trash.', { name: what, owner: here.node.owner }), { icon: 'trash-2' });
    toast(t('Moved {name} to the trash', { name: what }), {
      icon: 'trash-2',
      action: {
        label: t('Undo'),
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
  function preview(entry, edit = false) {
    noteRecent(entry.node.id);
    dialog = { type: 'preview', entries: files, start: files.indexOf(entry), edit };
  }

  async function star(entry) {
    try {
      const on = await toggleFavourite(entry.node.id);
      toast(on ? t('Added {name} to favourites', { name: entry.meta.name }) : t('Removed {name} from favourites', { name: entry.meta.name }), { icon: on ? 'star' : 'star-off' });
    } catch (e) {
      toastError(e);
    }
  }

  // Server-side tools this user may use (the video downloader is opt-in).
  let tools = $state({ video_downloader: false });
  onMount(() => toolsInfo().then((info) => (tools = info)));

  /** Upload a new file (converted, or downloaded) here, under a name that's free. */
  async function saveNewFile(file, onProgress) {
    const taken = new Set(rows.map((r) => r.meta.name.toLowerCase()));
    let name = file.name;
    const dot = name.lastIndexOf('.');
    for (let i = 2; taken.has(name.toLowerCase()); i++) name = `${file.name.slice(0, dot)} (${i})${file.name.slice(dot)}`;
    const named = name === file.name ? file : new File([file], name, { type: file.type, lastModified: file.lastModified });
    const target = here;
    await upload(named, { parentId: target.node.id, parentKey: target.key }, onProgress);
    if (target.node.id === folderId) await load();
    refreshMe().catch(() => {});
  }

  function untitledName() {
    const taken = new Set(rows.map((r) => r.meta.name.toLowerCase()));
    for (let i = 1; ; i++) {
      const name = i === 1 ? `${t('Untitled')}.md` : `${t('Untitled')} ${i}.md`;
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

  // Audio files play in the music player, with the rest of the folder's
  // audio queued after them.
  const isAudio = (e) => previewKind(e.meta)?.kind === 'audio';
  const asTrack = (e) => {
    const parentId = e.parentId ?? here.node.id;
    return makeTrack({ ...e, parentId }, { albumId: parentId, album: e.location?.at(-1) ?? here.meta.name });
  };

  function playFrom(entry) {
    const list = files.filter(isAudio);
    const i = list.indexOf(entry);
    if (i < 0) play([asTrack(entry)]);
    else play(list.map(asTrack), i, { shuffle: false });
  }

  function menuFor(entry) {
    const folder = entry.node.kind === 'folder';
    return [
      folder
        ? { label: t('Open'), icon: 'folder-open', onclick: () => open(entry.node.id) }
        : { label: t('Preview'), icon: 'eye', onclick: () => preview(entry) },
      ...(isAudio(entry)
        ? [
            { label: t('Play'), icon: 'play', onclick: () => playFrom(entry) },
            { label: t('Add to queue'), icon: 'list-end', onclick: () => (enqueue([asTrack(entry)]), toast(t('Added {name} to the queue', { name: entry.meta.name }))) },
          ]
        : []),
      folder
        ? { label: t('Download as zip'), icon: 'download', onclick: () => zipEntries([entry], `${entry.meta.name}.zip`) }
        : { label: t('Download'), icon: 'download', onclick: () => downloadEntry(entry) },
      isFavourite(entry.node.id)
        ? { label: t('Remove from favourites'), icon: 'star-off', onclick: () => star(entry) }
        : { label: t('Add to favourites'), icon: 'star', onclick: () => star(entry) },
      ...(isOwner
        ? [
            { label: t('Share'), icon: 'share-2', onclick: () => (dialog = { type: 'share', entry }) },
            { label: t('Public link'), icon: 'link', onclick: () => (dialog = { type: 'link', entry }) },
          ]
        : []),
      ...(canWrite
        ? [
            'sep',
            { label: t('Rename'), icon: 'pencil', onclick: () => startRename(entry) },
            { label: t('Move'), icon: 'move', onclick: () => (dialog = { type: 'move', entry }) },
            ...(!folder
              ? [{ label: t('Upload new version'), icon: 'file-up', onclick: () => ((versionTarget = entry), versionInput.click()) }]
              : []),
          ]
        : []),
      ...(!folder && sourceKind(entry.meta) ? [{ label: t('Convert'), icon: 'file-cog', onclick: () => (dialog = { type: 'convert', entry }) }] : []),
      ...(!folder && isPdf(entry.meta) ? [{ label: t('PDF tools'), icon: 'file-stack', onclick: () => (dialog = { type: 'pdf', entries: [entry] }) }] : []),
      ...(!folder ? [{ label: t('Version history'), icon: 'refresh-cw', onclick: () => (dialog = { type: 'versions', entry }) }] : []),
      { label: t('Comments'), icon: 'message-square', onclick: () => (dialog = { type: 'comments', entry }) },
      ...(folder ? [{ label: t('Activity'), icon: 'history', onclick: () => (dialog = { type: 'activity', entry }) }] : []),
      ...(canWrite ? ['sep', { label: t('Move to trash'), icon: 'trash-2', danger: true, onclick: () => moveToTrash(entry) }] : []),
    ];
  }

  // ---------------------------------------------------------- keyboard

  const rowButtons = () => [...(tbody?.querySelectorAll('button.row-open') ?? [])];

  // The first row in the page is this one in `visible` (not 0 when the
  // list is windowed).
  const firstInPage = () => (windowed && fileView.value !== 'grid' ? win.start : 0);

  async function focusRow(delta) {
    const list = rowButtons();
    if (!list.length) return;
    const i = list.indexOf(document.activeElement);
    const last = fileView.value === 'grid' ? list.length - 1 : visible.length - 1;
    const next = i === -1 ? firstInPage() + (delta > 0 ? 0 : list.length - 1) : Math.max(0, Math.min(last, firstInPage() + i + delta));
    await revealRow(next);
    const b = rowButtons()[next - firstInPage()];
    b?.focus();
    b?.scrollIntoView({ block: 'nearest' });
  }

  function focusedEntry() {
    const i = rowButtons().indexOf(document.activeElement);
    return i === -1 ? null : visible[firstInPage() + i];
  }

  function onkeydown(e) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    if (dialog || document.querySelector('dialog[open]')) return;
    const target = e.target;
    const typing = target instanceof HTMLElement && (target.isContentEditable || !!target.closest('input, textarea, select'));
    if (e.key === 'Escape' && selected.size && !typing) {
      selected.clear();
      e.preventDefault();
      return;
    }
    if (target === searchInput) {
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

  const sortLabel = $derived({ name: t('Name'), size: t('Size'), modified: t('Modified') });

  const close = () => (dialog = null);
  const reload = () => load();
</script>

<svelte:window {onkeydown} onscroll={updateWindow} onresize={updateWindow} onpaste={onPaste} ondragenter={onDragEnter} ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop} />

<input bind:this={fileInput} type="file" multiple hidden onchange={onPick} />
<input bind:this={folderInput} type="file" webkitdirectory hidden onchange={onPickFolder} />
<input bind:this={versionInput} type="file" hidden onchange={onPickVersion} />

<div class="flex flex-wrap items-start gap-x-4 gap-y-3">
  <!-- Wide enough for a name; past that the toolbar wraps below instead. -->
  <div class="min-w-[min(100%,12rem)] flex-1">
    <nav class="flex min-h-8 flex-wrap items-center gap-1 text-sm" aria-label={t('Folder path')}>
      {#if share}
        <button type="button" class="cursor-pointer rounded px-1 text-fg-muted hover:text-fg" onclick={() => go({ name: 'shared-with-me' })}>{t('Shared with me')}</button>
        <Icon name="chevron-right" class="size-4 text-fg-faint" />
      {/if}
      {#each path as crumb, i (crumb.node.id)}
        {#if i}<Icon name="chevron-right" class="size-4 text-fg-faint" />{/if}
        {#if i === path.length - 1}
          <h1 class="truncate px-1 text-xl font-semibold tracking-tight">{folderName(crumb)}</h1>
        {:else}
          <button
            type="button"
            class="max-w-48 cursor-pointer truncate rounded px-1 text-fg-muted hover:text-fg {dropTarget === crumb.node.id ? 'bg-accent-soft text-accent-text ring-1 ring-accent' : ''}"
            onclick={() => open(crumb.node.id)}
            ondragover={(e) => dragOverFolder(e, crumb.node.id)}
            ondragleave={(e) => dragLeaveFolder(e, crumb.node.id)}
            ondrop={(e) => dropOnFolder(e, crumb)}>{folderName(crumb)}</button>
        {/if}
      {/each}
    </nav>
    {#if share}
      <p class="mt-1 flex flex-wrap items-center gap-2 px-1 text-[13px] text-fg-muted">
        <span>{t('Shared by')} <span class="font-medium text-fg">{here?.node.owner}</span></span>
        <span class="badge {share.permission === 'write' ? 'badge-accent' : ''}">{share.permission === 'write' ? t('Can edit') : t('View only')}</span>
      </p>
    {/if}
  </div>
  <div class="flex w-full flex-wrap gap-2 md:w-auto">
    {#if rows.length}
      <label class="relative block flex-1 md:flex-none">
        <span class="sr-only">{t('Search this folder')}</span>
        <Icon name="search" class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-fg-faint" />
        <input
          bind:this={searchInput}
          bind:value={query}
          type="search"
          class="input h-8 w-full pr-8 pl-8 md:w-56"
          placeholder={t('Search')}
          autocomplete="off"
          spellcheck="false" />
        {#if !query}<kbd class="kbd pointer-events-none absolute top-1/2 right-2 -translate-y-1/2">/</kbd>{/if}
      </label>
      {#if query.trim() && path.length}
        <div class="flex h-8 rounded-md border border-line p-0.5" role="radiogroup" aria-label={t('Search in')}>
          {#each [['folder', t('This folder')], ['all', share ? t('Whole share') : t('Everywhere')], ['contents', t('Inside files')]] as [value, label] (value)}
            <button
              type="button"
              role="radio"
              aria-checked={scope === value}
              class="cursor-pointer rounded px-2 text-xs font-medium transition-colors {scope === value ? 'bg-muted text-fg' : 'text-fg-muted hover:text-fg'}"
              onclick={() => (scope = value)}>{label}</button>
          {/each}
        </div>
      {/if}
    {/if}
    <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Activity in this folder')} title={t('Activity')} disabled={!here} onclick={() => (dialog = { type: 'activity', entry: here })}>
      <Icon name="history" />
    </button>
    <div class="flex h-8 rounded-md border border-line p-0.5" role="radiogroup" aria-label={t('Show files as')}>
      {#each [['list', 'list', t('List')], ['grid', 'layout-grid', t('Grid')]] as [value, icon, label] (value)}
        <button
          type="button"
          role="radio"
          aria-checked={fileView.value === value}
          aria-label={label}
          title={label}
          class="grid w-7 cursor-pointer place-items-center rounded transition-colors {fileView.value === value ? 'bg-muted text-fg' : 'text-fg-muted hover:text-fg'}"
          onclick={() => setFileView(value)}><Icon name={icon} class="size-4" /></button>
      {/each}
    </div>
    {#if canWrite}
      <!-- On phones these live in the + button instead. -->
      <div class="hidden gap-2 md:flex">
        <button type="button" class="btn btn-secondary" disabled={!here} onclick={() => (dialog = { type: 'note' })}>
          <Icon name="file-plus" /> {t('New note')}
        </button>
        <button type="button" class="btn btn-secondary" disabled={!here} onclick={() => (dialog = { type: 'mkdir' })}>
          <Icon name="folder-plus" /> {t('New folder')}
        </button>
        <Menu
          label={t('Upload')}
          buttonClass="btn btn-primary"
          items={[
            { label: t('Files'), icon: 'file-up', onclick: () => fileInput.click() },
            { label: t('Folder'), icon: 'folder-up', onclick: () => folderInput.click() },
            ...(tools.video_downloader ? ['sep', { label: t('From a video link'), icon: 'link', onclick: () => (dialog = { type: 'video' }) }] : []),
          ]}>
          {#snippet trigger()}<Icon name="upload" /> {t('Upload')}<Icon name="chevron-down" class="-mr-1 size-3.5 opacity-70" />{/snippet}
        </Menu>
      </div>
    {/if}
  </div>
</div>

{#if strayDrops.list.length && isOwner}
  <div class="mt-4 flex flex-wrap items-center gap-3 rounded-md border border-line bg-subtle px-4 py-2.5 text-[13px]" role="status">
    <Icon name="inbox" class="size-4 shrink-0 text-fg-muted" />
    <p class="min-w-0 flex-1">
      {t('{count} files were dropped through a link that no longer exists.', { count: strayDrops.list.length })}
    </p>
    <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => (dialog = { type: 'stray-drops' })}>{t('Review')}</button>
  </div>
{/if}

<div class="card relative mt-6 overflow-hidden">
  {#if loading}
    <div aria-busy="true" aria-label={t('Loading')}>
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
        <button type="button" class="btn btn-secondary" onclick={reload}><Icon name="refresh-cw" /> {t('Try again')}</button>
        <button type="button" class="btn btn-ghost" onclick={() => open(session.me.keys.root_node_id)}>{t('Back to my files')}</button>
      </div>
    </div>
  {:else if scope !== 'folder' && query.trim()}
    <table class="table animate-enter">
      <thead>
        <tr>
          <th>{t('Name')}</th>
          <th class="hidden md:table-cell">{t('Location')}</th>
          <th class="hidden w-28 text-right sm:table-cell">{t('Size')}</th>
        </tr>
      </thead>
      <tbody>
        {#if !found.length}
          <tr>
            <td colspan="3" class="h-24 text-center text-[13px] text-fg-muted">
              {#if reading}<Icon name="loader-circle" class="spinner mr-1.5 inline size-4 align-[-3px]" />Reading files to search inside them ({reading.done} of {reading.total}){:else if searching}<Icon name="loader-circle" class="spinner mr-1.5 inline size-4 align-[-3px]" />Looking through your folders{:else}Nothing matches "{query.trim()}".{/if}
            </td>
          </tr>
        {/if}
        {#each found as r (r.node.id)}
          {@const folder = r.node.kind === 'folder'}
          <tr class="group">
            <td class="max-w-0">
              <button type="button" class="flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => openFound(r)}>
                {#if folder}<FolderIcon name={r.meta.name} />{:else}<FileIcon meta={r.meta} />{/if}
                <span class="truncate font-medium group-hover:underline group-hover:decoration-line-strong group-hover:underline-offset-4">{r.meta.name}</span>
              </button>
            </td>
            <td class="hidden max-w-0 md:table-cell">
              <button type="button" class="block max-w-full cursor-pointer truncate text-fg-muted hover:text-fg" title={r.location.join(' / ')} onclick={() => open(r.parentId)}>
                {r.location.join(' / ')}
              </button>
            </td>
            <td class="hidden text-right text-fg-muted tabular-nums sm:table-cell">{folder ? '' : formatSize(r.meta.size)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else if !rows.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center animate-enter">
      {#if path.length === 1 && !share}
        <!-- A brand moment: the very first, empty "My files". -->
        <img src="/img/logo.webp" alt="" width="715" height="349" class="mb-4 h-auto w-40 select-none" draggable="false" />
        <p class="font-medium">{t('Nothing here yet')}</p>
        <p class="max-w-sm text-[13px] text-fg-muted">
          {t('Drop files or whole folders anywhere on this page, or use Upload.')}
        </p>
      {:else}
        <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle">
          <Icon name={canWrite ? 'upload' : 'folder-open'} class="size-5 text-fg-muted" />
        </div>
        <p class="font-medium">{t('This folder is empty')}</p>
        <p class="text-[13px] text-fg-muted">
          {canWrite ? t('Drop files anywhere on this page, or use Upload.') : t('Nothing has been added here yet.')}
        </p>
      {/if}
    </div>
  {:else if fileView.value === 'grid'}
    <ul class="grid animate-enter grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-1 p-2" bind:this={tbody}>
      {#if !visible.length}
        <li class="col-span-full grid h-24 place-items-center text-[13px] text-fg-muted">
          <span>{t('Nothing in this folder matches "{query}".', { query: query.trim() })} <button type="button" class="link" onclick={() => (query = '')}>{t('Clear search')}</button></span>
        </li>
      {/if}
      {#each gridShown as entry (entry.node.id)}
        {@const folder = entry.node.kind === 'folder'}
        {@const isSelected = selected.has(entry.node.id)}
        <li
          class="group relative rounded-lg transition-colors {isSelected ? 'bg-accent-soft/60 ring-1 ring-accent/40' : 'hover:bg-subtle'} {dropTarget === entry.node.id ? 'bg-accent-soft ring-1 ring-accent' : ''}"
          draggable={canWrite && !touch && renaming !== entry.node.id}
          ondragstart={(e) => rowDragStart(e, entry)}
          ondragend={rowDragEnd}
          ondragover={folder ? (e) => dragOverFolder(e, entry.node.id) : undefined}
          ondragleave={folder ? (e) => dragLeaveFolder(e, entry.node.id) : undefined}
          ondrop={folder ? (e) => dropOnFolder(e, entry) : undefined}
          in:fade
          out:fade={{ duration: 120 }}
          animate:flip={flipParams()}>
          {#snippet tile()}
            <span class="grid aspect-square w-full place-items-center overflow-hidden rounded-md border border-line bg-subtle">
              {#if folder}<FolderIcon name={entry.meta.name} class="size-12" />{:else}<Thumb {entry} />{/if}
            </span>
          {/snippet}
          {#if renaming === entry.node.id}
            <form
              class="grid gap-2 p-2"
              onsubmit={(e) => {
                e.preventDefault();
                finishRename(entry);
              }}>
              {@render tile()}
              <input
                use:selectName={entry.meta.name}
                class="input h-7 px-2 text-[13px] font-medium"
                aria-label={t('New name for {name}', { name: entry.meta.name })}
                bind:value={renameValue}
                spellcheck="false"
                onkeydown={(e) => e.key === 'Escape' && (e.preventDefault(), e.stopPropagation(), finishRename(entry, false))}
                onblur={() => finishRename(entry)} />
            </form>
          {:else}
            <button
              type="button"
              class="row-open grid w-full cursor-pointer gap-2 p-2 text-left select-none"
              title={entry.meta.name}
              onclick={() => rowTap(entry)}
              onpointerdown={(e) => pressStart(e, entry)}
              onpointerup={pressEnd}
              onpointercancel={pressEnd}
              onpointermove={pressMove}
              oncontextmenu={(e) => e.pointerType !== 'mouse' && touch && e.preventDefault()}>
              {@render tile()}
              <span class="grid min-w-0 px-0.5">
                <span class="truncate text-[13px] font-medium">{entry.meta.name}</span>
                <span class="truncate text-xs text-fg-muted">{#if !folder}{formatSize(entry.meta.size)}{' · '}{/if}{formatWhen(changedAt(entry))}</span>
              </span>
            </button>
          {/if}
          <input
            type="checkbox"
            class="absolute top-3.5 left-3.5 size-4 cursor-pointer accent-accent transition-opacity focus-visible:opacity-100 {selected.size ? '' : 'opacity-0 group-hover:opacity-100'}"
            aria-label={t('Select {name}', { name: entry.meta.name })}
            checked={isSelected}
            onclick={(e) => {
              e.preventDefault();
              toggle(entry, e);
            }} />
          <div class="absolute top-2.5 right-2.5 rounded-md bg-bg/90 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100 max-md:opacity-100">
            <Menu items={menuFor(entry)} label={t('Actions for {name}', { name: entry.meta.name })} />
          </div>
        </li>
      {/each}
      {#if gridShown.length < visible.length}
        {#key gridLimit}<li class="col-span-full h-px" aria-hidden="true" use:nearScreen={() => (gridLimit += GRID_STEP)}></li>{/key}
      {/if}
    </ul>
  {:else}
    <table class="table animate-enter">
      <thead>
        <tr>
          <!-- On phones the checkboxes only appear while selecting (long press). -->
          <th class="w-10 !pr-0 {selected.size ? '' : 'max-md:hidden'}">
            <input
              type="checkbox"
              class="size-4 cursor-pointer align-middle accent-accent"
              aria-label={t('Select all')}
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
          <th class="w-12"><span class="sr-only">{t('Actions')}</span></th>
        </tr>
      </thead>
      <tbody bind:this={tbody}>
        {#if !visible.length}
          <tr>
            <td colspan="5" class="h-24 text-center text-[13px] text-fg-muted">
              {t('Nothing in this folder matches "{query}".', { query: query.trim() })} <button type="button" class="link" onclick={() => (query = '')}>{t('Clear search')}</button>
            </td>
          </tr>
        {/if}
        {#if windowed && win.start}
          <tr aria-hidden="true" class="!bg-transparent"><td colspan="5" class="!border-0 !p-0" style:height="{win.start * rowH}px"></td></tr>
        {/if}
        {#each shown as entry (entry.node.id)}
          {@const folder = entry.node.kind === 'folder'}
          {@const isSelected = selected.has(entry.node.id)}
          <tr
            data-row
            class="group {isSelected ? 'bg-accent-soft/60 hover:bg-accent-soft/60' : ''} {dropTarget === entry.node.id ? 'drop-target' : ''}"
            aria-selected={isSelected}
            draggable={canWrite && !touch && renaming !== entry.node.id}
            ondragstart={(e) => rowDragStart(e, entry)}
            ondragend={rowDragEnd}
            ondragover={folder ? (e) => dragOverFolder(e, entry.node.id) : undefined}
            ondragleave={folder ? (e) => dragLeaveFolder(e, entry.node.id) : undefined}
            ondrop={folder ? (e) => dropOnFolder(e, entry) : undefined}
            in:fade={motion({})}
            out:fade={motion({ duration: 120 })}
            animate:flip={motion(flipParams())}>
            <td class="w-10 !pr-0 {selected.size ? '' : 'max-md:hidden'}">
              <input
                type="checkbox"
                class="size-4 cursor-pointer align-middle accent-accent transition-opacity focus-visible:opacity-100 {selected.size ? '' : 'opacity-0 group-hover:opacity-100'}"
                aria-label={t('Select {name}', { name: entry.meta.name })}
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
                  {#if folder}<FolderIcon name={entry.meta.name} />{:else}<FileIcon meta={entry.meta} />{/if}
                  <input
                    use:selectName={entry.meta.name}
                    class="input h-7 max-w-md px-2 font-medium"
                    aria-label={t('New name for {name}', { name: entry.meta.name })}
                    bind:value={renameValue}
                    spellcheck="false"
                    onkeydown={(e) => e.key === 'Escape' && (e.preventDefault(), e.stopPropagation(), finishRename(entry, false))}
                    onblur={() => finishRename(entry)} />
                </form>
              {:else}
                <button
                  type="button"
                  class="row-open flex max-w-full cursor-pointer items-center gap-3 text-left select-none md:select-auto"
                  onclick={() => rowTap(entry)}
                  onpointerdown={(e) => pressStart(e, entry)}
                  onpointerup={pressEnd}
                  onpointercancel={pressEnd}
                  onpointermove={pressMove}
                  oncontextmenu={(e) => e.pointerType !== 'mouse' && touch && e.preventDefault()}>
                  {#if folder}<FolderIcon name={entry.meta.name} />{:else}<FileIcon meta={entry.meta} />{/if}
                  <span class="grid min-w-0">
                    <span class="truncate font-medium group-hover:underline group-hover:underline-offset-4 group-hover:decoration-line-strong">{entry.meta.name}</span>
                    <span class="truncate text-xs text-fg-muted md:hidden">
                      {#if !folder}<span class="sm:hidden">{formatSize(entry.meta.size)}{' · '}</span>{/if}{formatWhen(changedAt(entry))}
                    </span>
                  </span>
                </button>
              {/if}
            </td>
            <td class="hidden text-right text-fg-muted tabular-nums sm:table-cell">{folder ? '' : formatSize(entry.meta.size)}</td>
            <td class="hidden text-fg-muted md:table-cell" title={fullDate(changedAt(entry))}>{formatWhen(changedAt(entry))}</td>
            <td class="text-right"><Menu items={menuFor(entry)} label={t('Actions for {name}', { name: entry.meta.name })} /></td>
          </tr>
        {/each}
        {#if windowed && win.end < visible.length}
          <tr aria-hidden="true" class="!bg-transparent"><td colspan="5" class="!border-0 !p-0" style:height="{(visible.length - win.end) * rowH}px"></td></tr>
        {/if}
      </tbody>
    </table>
  {/if}

  {#if dragging}
    <div class="pointer-events-none absolute inset-0 grid place-items-center rounded-lg border-2 border-dashed border-accent bg-accent-soft/80">
      <p class="flex items-center gap-2 font-medium text-accent-text"><Icon name="upload" /> {t('Drop to upload to {folder}', { folder: folderName(here) })}</p>
    </div>
  {/if}
</div>

{#if rows.length}
  <div class="mt-3 flex items-center justify-between gap-4 px-1 text-xs text-fg-faint">
    <p>
      {#if query.trim() && scope !== 'folder'}{reading ? t('{count} found, reading {done} of {total} files', { count: found.length, done: reading.done, total: reading.total }) : searching ? t('{count} found so far', { count: found.length }) : t('{count} found', { count: found.length })} ·{:else if query.trim()}{t('{shown} of {total} shown', { shown: visible.length, total: rows.length })} ·{/if}
      {t('{count} folders', { count: rows.filter((r) => r.node.kind === 'folder').length })}, {t('{count} files', { count: rows.filter((r) => r.node.kind === 'file').length })}
    </p>
    <button type="button" class="hidden cursor-pointer items-center gap-1.5 hover:text-fg-muted sm:flex" onclick={() => (dialog = { type: 'shortcuts' })}>
      <Icon name="keyboard" class="size-3.5" /> {t('Keyboard shortcuts')} <kbd class="kbd">?</kbd>
    </button>
  </div>
{/if}

{#if selected.size && chosen.length}
  <div class="fixed inset-x-0 bottom-[calc(var(--bottom-bar)+1rem)] z-40 flex justify-center px-4 md:bottom-[calc(var(--bottom-bar)+1.5rem)]" transition:fly={{ y: 12 }}>
    <div class="flex items-center gap-1 rounded-lg border border-line bg-bg p-1.5 pl-3 shadow-lg shadow-black/5 dark:shadow-black/40" role="toolbar" aria-label={t('Selection')}>
      <span class="mr-2 text-sm font-medium tabular-nums">{t('{count} selected', { count: chosen.length })}</span>
      <button type="button" class="btn btn-ghost" onclick={downloadChosen} disabled={!chosen.some((r) => r.node.kind === 'file')}>
        <Icon name="download" /><span class="hidden sm:inline">{t('Download')}</span>
      </button>
      {#if chosen.length > 1 && chosen.every((r) => r.node.kind === 'file' && sourceKind(r.meta))}
        <button type="button" class="btn btn-ghost" onclick={() => (dialog = { type: 'convert-many', entries: [...chosen] })}>
          <Icon name="file-cog" /><span class="hidden sm:inline">{t('Convert')}</span>
        </button>
      {/if}
      {#if chosen.length > 1 && chosen.every((r) => r.node.kind === 'file' && isPdf(r.meta))}
        <button type="button" class="btn btn-ghost" onclick={() => (dialog = { type: 'pdf', entries: [...chosen] })}>
          <Icon name="file-stack" /><span class="hidden sm:inline">{t('Merge PDFs')}</span>
        </button>
      {/if}
      {#if canWrite}
        <button type="button" class="btn btn-ghost" onclick={() => (dialog = { type: 'move', entries: [...chosen] })}>
          <Icon name="move" /><span class="hidden sm:inline">{t('Move')}</span>
        </button>
        <button type="button" class="btn btn-ghost text-danger hover:text-danger" onclick={trashChosen}>
          <Icon name="trash-2" /><span class="hidden sm:inline">{t('Move to trash')}</span>
        </button>
      {/if}
      <span class="mx-1 h-5 w-px bg-line" aria-hidden="true"></span>
      <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Clear selection')} title={t('Clear selection (Esc)')} onclick={() => selected.clear()}>
        <Icon name="x" />
      </button>
    </div>
  </div>
{/if}

<!-- Phones: one + button for everything that adds files. -->
{#if canWrite && here && !selected.size}
  <div class="fixed right-4 bottom-[calc(var(--bottom-bar)+1rem)] z-30 md:hidden" transition:fly={{ y: 12 }}>
    <Menu
      label={t('Add')}
      buttonClass="grid size-14 cursor-pointer place-items-center rounded-full bg-accent text-accent-fg shadow-lg shadow-black/25 transition-transform active:scale-95"
      items={[
        { label: t('Upload files'), icon: 'file-up', onclick: () => fileInput.click() },
        { label: t('Upload a folder'), icon: 'folder-up', onclick: () => folderInput.click() },
        ...(tools.video_downloader ? [{ label: t('From a video link'), icon: 'link', onclick: () => (dialog = { type: 'video' }) }] : []),
        'sep',
        { label: t('New folder'), icon: 'folder-plus', onclick: () => (dialog = { type: 'mkdir' }) },
        { label: t('New note'), icon: 'file-plus', onclick: () => (dialog = { type: 'note' }) },
      ]}>
      {#snippet trigger()}<Icon name="plus" class="size-6" />{/snippet}
    </Menu>
  </div>
{/if}

{#if dialog?.type === 'photo-details'}
  {@const answer = dialog.resolve}
  <Modal
    title={t('{count} photos have a location', { count: dialog.located })}
    description={t('Photos from phones and cameras often record where they were taken, and on what. Anyone you share them with could read it.')}
    onclose={() => answer('cancel')}
    onsubmit={() => answer('remove')}>
    <p class="text-[13px] text-fg-muted">{t("Removing it also drops the camera details. The pictures themselves don't change. You can choose what happens every time in Settings.")}</p>
    {#snippet footer()}
      <button type="button" class="btn btn-secondary mr-auto" onclick={() => answer('cancel')}>{t('Cancel')}</button>
      <button type="button" class="btn btn-secondary" onclick={() => answer('keep')}>{t('Keep it')}</button>
      <button class="btn btn-primary">{t('Remove location')}</button>
    {/snippet}
  </Modal>
{:else if dialog?.type === 'mkdir'}
  <NameDialog
    title={t('New folder')}
    confirmLabel={t('Create')}
    onsave={async (name) => {
      await createFolder(here.node.id, here.key, name);
      await load();
    }}
    onclose={close} />
{:else if dialog?.type === 'convert'}
  <ConvertDialog
    entry={dialog.entry}
    fetch={fetchEntry}
    save={canWrite ? saveNewFile : null}
    onclose={close} />
{:else if dialog?.type === 'pdf'}
  <PdfToolsDialog entries={dialog.entries} fetch={fetchEntry} save={canWrite ? saveNewFile : null} onclose={close} />
{:else if dialog?.type === 'stray-drops'}
  <StrayDropsDialog onclose={close} onchanged={() => load()} />
{:else if dialog?.type === 'convert-many'}
  <BatchConvertDialog entries={dialog.entries} fetch={fetchEntry} save={canWrite ? saveNewFile : null} onclose={close} />
{:else if dialog?.type === 'video'}
  <VideoDownloadDialog
    save={canWrite ? saveNewFile : null}
    maxBytes={tools.downloader_max_bytes}
    canMerge={tools.downloader_can_merge}
    onclose={close} />
{:else if dialog?.type === 'shortcuts'}
  <ShortcutsDialog onclose={close} />
{:else if dialog?.type === 'note'}
  <NameDialog
    title={t('New note')}
    initial={untitledName()}
    confirmLabel={t('Create')}
    create
    onsave={newNote}
    onclose={() => dialog?.type === 'note' && close()} />
{:else if dialog?.type === 'move'}
  <MoveDialog
    entries={dialog.entries ?? [dialog.entry]}
    root={path[0]}
    currentFolderId={folderId}
    onmoved={(dest, count) => {
      toast(count > 1 ? t('Moved {count} items to {folder}', { count, folder: dest }) : t('Moved to {folder}', { folder: dest }), { kind: 'success' });
      selected.clear();
      load();
    }}
    onclose={close} />
{:else if dialog?.type === 'activity'}
  <ActivityDialog
    entry={dialog.entry}
    onopen={(e) => {
      close();
      open(e.folder ? e.node_id : e.parentId);
    }}
    onclose={close} />
{:else if dialog?.type === 'comments'}
  <CommentsDialog entry={dialog.entry} {isOwner} onclose={close} />
{:else if dialog?.type === 'versions'}
  <VersionsDialog entry={dialog.entry} {canWrite} onchanged={() => (load(), refreshMe().catch(() => {}))} onclose={close} />
{:else if dialog?.type === 'preview'}
  <Preview
    entries={dialog.entries}
    start={dialog.start}
    edit={dialog.edit}
    fetch={fetchEntry}
    open={openEntry}
    {bookProgress}
    trail={path}
    list={(f) => listFolder(f.node.id, f.key)}
    save={canWrite ? saveText : null}
    drafts={canWrite ? { load: loadDraft, store: storeDraft, drop: dropDraft } : null}
    onsaved={() => (load(), refreshMe().catch(() => {}))}
    ondownload={downloadEntry}
    onclose={close} />
{:else if dialog?.type === 'share'}
  <ShareDialog entry={dialog.entry} onclose={close} />
{:else if dialog?.type === 'link'}
  <LinkDialog entry={dialog.entry} onclose={close} />
{/if}
