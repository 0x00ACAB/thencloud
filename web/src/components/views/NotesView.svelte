<script>
  // A folder of Markdown files as a notebook: the notes on the left (pinned
  // first, then the most recently changed), the open note on the right,
  // saved as you type. Everything is decrypted here; search reads notes'
  // text in the browser too.
  import { onMount, untrack } from 'svelte';
  import { session, resolvePath, trash } from '../../lib/cloud.svelte.js';
  import { notes, openNotes, setNotesRoot, scanNotes, readNote, writeNote, refreshNote, searchNotes, createNote, isPinned, togglePin } from '../../lib/notes.svelte.js';
  import { toast, toastError, errorMessage } from '../../lib/ui.svelte.js';
  import Icon from '../Icon.svelte';
  import Time from '../Time.svelte';
  import Menu from '../Menu.svelte';
  import FolderPickDialog from '../dialogs/FolderPickDialog.svelte';
  import MarkdownEditor from '../preview/MarkdownEditor.svelte';

  let { go } = $props();

  onMount(openNotes);

  let picking = $state(null);
  async function choose() {
    try {
      picking = (await resolvePath(session.me.keys.root_node_id)).items[0];
    } catch (e) {
      toastError(e);
    }
  }

  // --------------------------------------------------------------- list

  let query = $state('');
  let found = $state(null); // search results: [{ entry, snippet }]
  const pinned = $derived(new Set(notes.pinned));
  const sorted = $derived(
    (notes.list ?? []).toSorted((a, b) => (pinned.has(b.node.id) ? 1 : 0) - (pinned.has(a.node.id) ? 1 : 0) || b.node.updated_at - a.node.updated_at),
  );
  const shown = $derived(found ?? sorted.map((entry) => ({ entry, snippet: '' })));

  $effect(() => {
    const q = query.trim();
    found = null;
    if (!q) return;
    const ctl = new AbortController();
    const t = setTimeout(async () => {
      const r = await searchNotes(q, { signal: ctl.signal });
      if (!ctl.signal.aborted) found = r;
    }, 200);
    return () => (clearTimeout(t), ctl.abort());
  });

  const title = (entry) => entry.meta.name.replace(/\.(md|markdown)$/i, '');

  // --------------------------------------------------------------- editor

  let open = $state(null); // the note being edited
  let text = $state(null); // its text as loaded (the editor's starting point)
  let editorKey = $state(0);
  let pending = null; // unsaved text
  let timer = null;
  let status = $state(''); // '', 'saving', 'saved', 'unsaved', 'conflict', or an error
  let loadError = $state('');

  async function select(entry) {
    if (open?.node.id === entry.node.id) return;
    await flush();
    open = entry;
    text = null;
    loadError = '';
    status = '';
    try {
      const t = await readNote(entry);
      if (open?.node.id !== entry.node.id) return;
      text = t;
      editorKey++;
    } catch (e) {
      loadError = errorMessage(e);
    }
  }

  function onchange(md) {
    if (md === text && pending === null) return;
    pending = md;
    status = 'unsaved';
    clearTimeout(timer);
    timer = setTimeout(flush, 1200);
  }

  async function flush() {
    clearTimeout(timer);
    if (pending === null || !open || status === 'conflict') return;
    const md = pending;
    pending = null;
    status = 'saving';
    try {
      open = await writeNote(open, md);
      text = md;
      if (pending === null) status = 'saved';
    } catch (e) {
      pending ??= md;
      status = e?.status === 409 ? 'conflict' : errorMessage(e);
    }
  }

  // Changed elsewhere since it was opened: keep these edits, or take the other.
  async function keepMine() {
    try {
      open = await refreshNote(open);
      status = 'unsaved';
      await flush();
    } catch (e) {
      status = errorMessage(e);
    }
  }

  async function takeTheirs() {
    pending = null;
    const entry = await refreshNote(open);
    open = null;
    await select(entry);
  }

  async function newNote() {
    try {
      await select(await createNote());
    } catch (e) {
      toastError(e);
    }
  }

  async function remove(entry) {
    try {
      if (open?.node.id === entry.node.id) {
        pending = null;
        open = null;
      }
      await trash(entry);
      notes.list = notes.list.filter((n) => n.node.id !== entry.node.id);
      toast(`Moved ${entry.meta.name} to the trash`, { icon: 'trash-2' });
    } catch (e) {
      toastError(e);
    }
  }

  async function pin(entry) {
    try {
      await togglePin(entry.node.id);
    } catch (e) {
      toastError(e);
    }
  }

  // Leaving the view, or the page, with edits not yet saved.
  $effect(() => () => untrack(flush));
  const onbeforeunload = (e) => pending !== null && e.preventDefault();

  function onkeydown(e) {
    if ((e.ctrlKey || e.metaKey) && e.key === 's' && open) {
      e.preventDefault();
      flush();
    }
  }

  const statusText = $derived(
    { '': '', saving: 'Encrypting and saving', saved: 'Saved', unsaved: 'Unsaved changes', conflict: 'Changed on another device' }[status] ?? status,
  );
</script>

<svelte:window {onbeforeunload} {onkeydown} />

<div class="flex flex-wrap items-center justify-between gap-3">
  <div>
    <h1 class="text-xl font-semibold tracking-tight">Notes</h1>
    {#if notes.rootId}
      <p class="mt-1 text-[13px] text-fg-muted">
        Markdown files in <button type="button" class="link" onclick={() => go({ name: 'files', folderId: notes.rootId })}>{notes.rootName || 'your notes folder'}</button>, encrypted like everything else.
      </p>
    {/if}
  </div>
  {#if notes.rootId}
    <div class="flex gap-2">
      <Menu
        label="Notes folder"
        items={[
          { label: 'Choose another folder', icon: 'folder-open', onclick: choose },
          { label: 'Look for new notes', icon: 'refresh-cw', onclick: scanNotes },
        ]} />
      <button type="button" class="btn btn-primary" disabled={!notes.list} onclick={newNote}><Icon name="file-plus" /> New note</button>
    </div>
  {/if}
</div>

{#if !notes.rootId}
  <div class="card mt-6 grid place-items-center gap-1 px-6 py-20 text-center">
    <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle"><Icon name="notebook-pen" class="size-5 text-fg-muted" /></div>
    <p class="font-medium">Pick a folder for your notes</p>
    <p class="max-w-sm text-[13px] text-fg-muted">Its Markdown files become a notebook you can search and write in. New notes are saved there too.</p>
    <button type="button" class="btn btn-primary mt-4" onclick={choose}><Icon name="folder-open" /> Choose a folder</button>
  </div>
{:else if notes.error}
  <div class="card mt-6 grid place-items-center gap-3 px-6 py-16 text-center">
    <Icon name="circle-alert" class="size-6 text-danger" />
    <p class="text-fg-muted">{notes.error}</p>
    <div class="flex gap-2">
      <button type="button" class="btn btn-secondary" onclick={scanNotes}><Icon name="refresh-cw" /> Try again</button>
      <button type="button" class="btn btn-ghost" onclick={choose}>Choose another folder</button>
    </div>
  </div>
{:else}
  <div class="card mt-6 grid min-h-[70dvh] overflow-hidden md:grid-cols-[18rem_1fr]">
    <!-- On phones the list and the note take turns. -->
    <aside class="flex min-h-0 flex-col border-line md:border-r {open ? 'max-md:hidden' : ''}">
      <label class="relative block border-b border-line p-2">
        <span class="sr-only">Search notes</span>
        <Icon name="search" class="pointer-events-none absolute top-1/2 left-4.5 size-4 -translate-y-1/2 text-fg-faint" />
        <input bind:value={query} type="search" class="input h-8 w-full pl-8" placeholder="Search notes" autocomplete="off" spellcheck="false" />
      </label>
      <ul class="min-h-0 flex-1 overflow-y-auto p-1">
        {#if !notes.list}
          <li class="grid h-32 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner" /></li>
        {:else if query.trim() && found === null}
          <li class="px-3 py-6 text-center text-[13px] text-fg-muted"><Icon name="loader-circle" class="spinner mr-1.5 inline size-4 align-[-3px]" />Searching</li>
        {:else if !shown.length}
          <li class="px-3 py-6 text-center text-[13px] text-fg-muted">{query.trim() ? `No note mentions "${query.trim()}".` : 'No notes yet. Use New note to start one.'}</li>
        {/if}
        {#each shown as { entry, snippet } (entry.node.id)}
          <li>
            <button
              type="button"
              class="grid w-full cursor-pointer gap-0.5 rounded-md px-3 py-2 text-left {open?.node.id === entry.node.id ? 'bg-accent-soft' : 'hover:bg-muted'}"
              onclick={() => select(entry)}>
              <span class="flex items-center gap-1.5">
                {#if pinned.has(entry.node.id)}<Icon name="pin" class="size-3.5 shrink-0 text-fg-muted" />{/if}
                <span class="truncate text-sm font-medium {open?.node.id === entry.node.id ? 'text-accent-text' : ''}">{title(entry)}</span>
              </span>
              <span class="truncate text-xs text-fg-muted">
                {#if snippet}{snippet}{:else}{#if entry.location.length}{entry.location.join(' / ')} · {/if}<Time ms={entry.node.updated_at * 1000} relative />{/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </aside>

    <section class="flex min-h-0 min-w-0 flex-col {open ? '' : 'max-md:hidden'}">
      {#if !open}
        <div class="grid flex-1 place-items-center p-6 text-center text-[13px] text-fg-muted">Pick a note, or start a new one.</div>
      {:else}
        <header class="flex h-12 shrink-0 items-center gap-2 border-b border-line px-3">
          <button type="button" class="btn btn-ghost btn-icon md:hidden" aria-label="Back to the list" onclick={() => (flush(), (open = null))}><Icon name="arrow-left" /></button>
          <h2 class="min-w-0 flex-1 truncate text-sm font-medium">{title(open)}</h2>
          <p class="hidden truncate text-xs sm:block {status === 'conflict' || !['', 'saving', 'saved', 'unsaved'].includes(status) ? 'text-danger' : 'text-fg-muted'}" role="status">{statusText}</p>
          <Menu
            label="Note actions"
            items={[
              isPinned(open.node.id) ? { label: 'Unpin', icon: 'pin-off', onclick: () => pin(open) } : { label: 'Pin to the top', icon: 'pin', onclick: () => pin(open) },
              { label: 'Show in My files', icon: 'folder-open', onclick: () => go({ name: 'files', folderId: open.parentId, open: open.node.id }) },
              'sep',
              { label: 'Move to trash', icon: 'trash-2', danger: true, onclick: () => remove(open) },
            ]} />
        </header>
        {#if status === 'conflict'}
          <div class="flex flex-wrap items-center gap-3 border-b border-line bg-subtle px-4 py-2 text-[13px]" role="alert">
            <Icon name="circle-alert" class="size-4 shrink-0 text-fg-muted" />
            <p class="min-w-0 flex-1">This note was changed somewhere else since you opened it.</p>
            <button type="button" class="btn btn-ghost h-7 px-2.5 text-[13px]" onclick={takeTheirs}>Use theirs</button>
            <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={keepMine}>Keep mine</button>
          </div>
        {/if}
        <div class="relative min-h-0 flex-1">
          {#if loadError}
            <div class="grid h-full place-items-center p-6 text-center text-[13px] text-danger">{loadError}</div>
          {:else if text === null}
            <div class="grid h-full place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner" /></div>
          {:else}
            {#key editorKey}<div class="absolute inset-0"><MarkdownEditor {text} {onchange} /></div>{/key}
          {/if}
        </div>
      {/if}
    </section>
  </div>
{/if}

{#if picking}
  <FolderPickDialog
    root={picking}
    title="Notes folder"
    description="Its Markdown files, in it and in folders below it, become your notebook."
    onpick={(id) => {
      open = null;
      setNotesRoot(id);
    }}
    onclose={() => (picking = null)} />
{/if}
