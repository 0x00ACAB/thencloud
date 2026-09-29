<script>
  // Full-window file preview. Files are decrypted in the browser, exactly
  // like a download, and shown from a blob: URL that is revoked when you
  // move on. ← and → step through the other files in the folder.
  //
  // Markdown and text files can be edited when `save` is given (the viewer
  // can write): each save uploads the text as a new encrypted version.
  // With `drafts` ({ load, store, drop }), unsaved edits are kept encrypted
  // on the server as you type and offered back next time.
  import { onMount, untrack } from 'svelte';
  import { t, language } from '../lib/i18n.svelte.js';
  import Sentence from './Sentence.svelte';
  import Icon from './Icon.svelte';
  import Time from './Time.svelte';
  import FileIcon from './FileIcon.svelte';
  import TextView from './preview/TextView.svelte';
  import MarkdownView from './preview/MarkdownView.svelte';
  import PdfView from './preview/PdfView.svelte';
  import BookView from './preview/BookView.svelte';
  import TableView from './preview/TableView.svelte';
  import SubtitlePicker from './SubtitlePicker.svelte';
  import { matchSubtitles, loadSubtitles, release as releaseSubtitles } from '../lib/subtitles.js';
  import MarkdownEditor from './preview/MarkdownEditor.svelte';
  import TextEditor from './preview/TextEditor.svelte';
  import ConfirmDialog from './dialogs/ConfirmDialog.svelte';
  import { saveBlob } from '../lib/crypto.js';
  import { previewKind, readText, MAX_PREVIEW, MAX_TEXT } from '../lib/preview.js';
  import { formatSize, changedAt } from '../lib/format.js';
  import { separatorFor } from '../lib/csv.js';
  import { errorMessage, toastError } from '../lib/ui.svelte.js';
  import { fade } from '../lib/motion.js';

  /** @type {{ entries: any[], start: number, fetch: (entry: any, onProgress: (p: number) => void) => Promise<{ blob: Blob }>, ondownload: (entry: any) => void, onclose: () => void, save?: ((entry: any, text: string) => Promise<any>) | null, onsaved?: (entry: any) => void, edit?: boolean, trail?: any[] | null, list?: ((folder: any) => Promise<any[]>) | null, drafts?: any, open?: ((entry: any) => any) | null }} */
  let { entries, start, fetch, ondownload, onclose, save = null, onsaved, edit = false, trail = null, list = null, drafts = null, open = null, bookProgress = null } = $props();

  let dlg;
  let index = $state(untrack(() => start));
  let loaded = $state({ id: null }); // what load() produced, for the file with this id
  let showSource = $state(false);
  let zoomed = $state(false);

  let updated = $state({}); // node id -> entry after a save here
  const entry = $derived(updated[entries[index].node.id] ?? entries[index]);
  const kind = $derived(previewKind(entry.meta));
  // Until load() catches up with a step to another file, show it as loading.
  const view = $derived(loaded.id === entry.node.id ? loaded : { status: 'loading', progress: 0 });

  let seq = 0;
  let url = null;
  let served = null; // a stream from lib/stream.js, for video and audio

  // Images in Markdown by relative path: found by decrypted name from the
  // folder the file is in (`trail` ends there), decrypted like any preview.
  const MAX_IMAGE = 32 * 1024 * 1024;
  const listed = new Map();
  const cachedList = (folder) => {
    if (!listed.has(folder.node.id)) listed.set(folder.node.id, list(folder));
    return listed.get(folder.node.id);
  };
  async function loadImage(path) {
    const { findRelative } = await import('../lib/relpath.js');
    const target = await findRelative(trail, path, cachedList);
    const k = target && previewKind(target.meta);
    if (k?.kind !== 'image' || target.meta.size > MAX_IMAGE) return null;
    const { blob } = await fetch(target, () => {});
    return URL.createObjectURL(new Blob([blob], { type: k.type }));
  }

  function release() {
    if (url) URL.revokeObjectURL(url);
    url = null;
    served?.close();
    served = null;
  }

  async function load(e) {
    const my = ++seq;
    const id = e.node.id;
    const set = (v) => (loaded = { id, ...v });
    release();
    zoomed = false;
    const k = previewKind(e.meta);
    if (!k) return set({ status: 'unsupported' });
    // Video and audio play as they're decrypted, through the stream worker,
    // so their size doesn't matter.
    if ((k.kind === 'video' || k.kind === 'audio') && open) {
      const { streamsAvailable, serveFile } = await import('../lib/stream.js');
      if (await streamsAvailable()) {
        if (my !== seq) return;
        try {
          const file = open(e);
          served = serveFile({ size: file.size, chunkSize: file.chunkSize, type: k.type, read: (i) => file.read(i) });
          return set({ status: 'ready', url: served.url });
        } catch (err) {
          return set({ status: 'error', message: errorMessage(err) });
        }
      }
    }
    if (e.meta.size > MAX_PREVIEW || ((k.kind === 'text' || k.kind === 'markdown') && e.meta.size > MAX_TEXT)) {
      return set({ status: 'large' });
    }
    set({ status: 'loading', progress: 0 });
    try {
      const { blob: raw } = await fetch(e, (p) => my === seq && (loaded.progress = p));
      if (my !== seq) return;
      // Re-type the bytes with the type we chose, never the stored one.
      const blob = new Blob([raw], { type: k.type });
      if (k.kind === 'text' || k.kind === 'markdown') {
        const text = await readText(blob);
        if (my !== seq) return;
        set(text === null ? { status: 'binary', blob } : { status: 'ready', blob, text });
      } else if (k.kind === 'pdf' || k.kind === 'book') {
        set({ status: 'ready', blob });
      } else {
        url = URL.createObjectURL(blob);
        set({ status: 'ready', blob, url });
      }
    } catch (err) {
      if (my === seq) set({ status: 'error', message: errorMessage(err) });
    }
  }

  // Subtitles: .srt and .vtt files in the same folder named after the video.
  let subtitles = $state([]);
  let videoEl = $state();
  $effect(() => {
    const e = entry;
    if (previewKind(e.meta)?.kind !== 'video') return;
    const matched = matchSubtitles(e.meta.name, entries, { locale: language(), unnamed: t('Subtitles') });
    if (!matched.length) return;
    let live = true;
    let got = [];
    loadSubtitles(matched, fetch).then((subs) => {
      got = subs;
      if (live) subtitles = subs;
      else releaseSubtitles(subs);
    });
    return () => {
      live = false;
      releaseSubtitles(got);
      subtitles = [];
    };
  });

  // Load when stepping to another file; a save here updates `entry` but
  // already has the text, so it must not trigger a reload.
  $effect(() => {
    index;
    untrack(() => load(entry));
  });

  // ------------------------------------------------------------ editing

  let editing = $state(false);
  let draft = $state(null); // current text while editing
  let editorText = $state(''); // what the editor starts from
  let editorKey = $state(0); // bumped to start the editor again (restoring a draft)
  let offer = $state(null); // a draft from earlier: { text, baseRevision, updatedAt }
  let draftTimer = null;
  let saving = $state(false);
  let saveError = $state('');
  let confirm = $state(null); // { title, description, label, then } before discarding changes
  const canEdit = $derived(!!save && (kind?.kind === 'markdown' || kind?.kind === 'text') && view.status === 'ready' && view.text !== undefined);
  const dirty = $derived(editing && draft !== null && draft !== view.text);

  // `edit` opens straight into the editor (for a new note).
  $effect(() => {
    if (edit && canEdit && untrack(() => !editing && index === start)) startEditing();
  });

  function startEditing() {
    draft = null;
    saveError = '';
    editorText = view.text;
    offer = null;
    editing = true;
    const e = entry;
    drafts
      ?.load(e)
      .then((d) => {
        if (d && editing && e === entry && d.text !== view.text) offer = d;
      })
      .catch(() => {});
  }

  function stopEditing() {
    clearTimeout(draftTimer);
    if (drafts && (dirty || offer)) drafts.drop(entry);
    editing = false;
    draft = null;
    offer = null;
    saveError = '';
  }

  /** Run `then` now, or after confirming if there are unsaved changes. */
  function guard(then) {
    if (!dirty) return then();
    confirm = { then };
  }

  function onEdit(text) {
    draft = text;
    if (!drafts) return;
    clearTimeout(draftTimer);
    const e = entry;
    draftTimer = setTimeout(() => {
      if (editing && e === entry && dirty) drafts.store(e, draft).catch(() => {});
    }, 1500);
  }

  function restoreDraft() {
    editorText = offer.text;
    draft = offer.text;
    offer = null;
    editorKey++;
  }

  async function saveDraft() {
    if (!dirty || saving) return;
    await saveText(draft);
  }

  // Ticking a task in the rendered view saves the file straight away.
  async function toggleTask(i, checked) {
    const { setTask } = await import('../lib/tasks.js');
    const text = setTask(view.text, i, checked);
    if (text === null || saving) return;
    await saveText(text);
    if (saveError) toastError(new Error(saveError));
  }

  async function saveText(text) {
    saving = true;
    saveError = '';
    try {
      const next = await save(entry, text);
      clearTimeout(draftTimer);
      drafts?.drop(entry);
      offer = null;
      updated[next.node.id] = { ...entry, ...next };
      loaded = { id: next.node.id, status: 'ready', text, blob: new Blob([text], { type: 'text/plain' }) };
      onsaved?.(updated[next.node.id]);
      if (draft === text) draft = null;
    } catch (e) {
      saveError =
        e?.code === 'conflict'
          ? t('Someone else changed this file since you opened it. Copy your changes, then reopen the file.')
          : e?.code === 'quota_exceeded'
            ? t('Not enough storage left to save.')
            : errorMessage(e);
    } finally {
      saving = false;
    }
  }

  function requestClose() {
    guard(() => dlg.close());
  }

  function oncancel(e) {
    // Esc: keep unsaved edits unless confirmed.
    if (dirty) {
      e.preventDefault();
      requestClose();
    }
  }

  function onbeforeunload(e) {
    if (dirty) e.preventDefault();
  }

  onMount(() => {
    dlg.showModal();
    return release;
  });

  function step(d) {
    const next = index + d;
    if (next >= 0 && next < entries.length) index = next;
  }

  function onkeydown(e) {
    if (confirm) return;
    if (editing && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      saveDraft();
      return;
    }
    if (editing || e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.target instanceof HTMLElement && e.target.closest('input, textarea, video, audio, [contenteditable]')) return;
    if (e.key === 'ArrowLeft') step(-1);
    else if (e.key === 'ArrowRight') step(1);
    else return;
    e.preventDefault();
  }

  function download() {
    if (view.blob) saveBlob(view.blob, entry.meta.name);
    else ondownload(entry);
  }

  // An image's blob URL is only needed until it's decoded. Revoking it right
  // away also means "open image in new tab" can't turn an SVG into a page.
  function imageLoaded() {
    release();
  }
</script>

<svelte:window {onkeydown} {onbeforeunload} />

<dialog bind:this={dlg} class="preview" aria-label={t('Preview of {name}', { name: entry.meta.name })} onclose={() => onclose()} {oncancel}>
  <header class="flex h-14 shrink-0 items-center gap-3 border-b border-line px-4">
    <FileIcon meta={entry.meta} />
    <div class="min-w-0 flex-1">
      <h2 class="truncate text-sm font-medium">{entry.meta.name}</h2>
      <p class="truncate text-xs text-fg-muted">
        {formatSize(entry.meta.size)}{#if entry.node.updated_at}{' · '}<Time ms={changedAt(entry)} relative />{/if}
      </p>
    </div>

    {#if editing}
      <p class="hidden truncate text-xs sm:block {saveError ? 'text-danger' : 'text-fg-muted'}" role="status" title={saveError}>
        {saveError || (saving ? t('Saving') : dirty ? t('Unsaved changes') : t('All changes saved'))}
      </p>
      <button type="button" class="btn btn-secondary" onclick={() => guard(stopEditing)}>{t('Done')}</button>
      <button type="button" class="btn btn-primary" disabled={!dirty || saving} title={t('Save (Ctrl+S)')} onclick={saveDraft}>
        {#if saving}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="save" />{/if}
        {t('Save')}
      </button>
    {:else if (kind?.kind === 'markdown' || kind?.table) && view.status === 'ready'}
      <div class="hidden rounded-md border border-line p-0.5 sm:flex" role="radiogroup" aria-label={t('Show')}>
        {#each [[false, kind.table ? 'table' : 'book-open', kind.table ? t('Table') : t('Preview')], [true, 'code', t('Source')]] as [value, icon, label] (value)}
          <button
            type="button"
            role="radio"
            aria-checked={showSource === value}
            class="flex h-6 cursor-pointer items-center gap-1.5 rounded px-2 text-xs font-medium transition-colors {showSource === value
              ? 'bg-muted text-fg'
              : 'text-fg-muted hover:text-fg'}"
            onclick={() => (showSource = value)}>
            <Icon name={icon} class="size-3.5" />{label}
          </button>
        {/each}
      </div>
    {/if}

    {#if kind?.kind === 'video' && view.status === 'ready'}
      <SubtitlePicker tracks={subtitles} video={videoEl} class="hidden sm:flex" />
    {/if}

    {#if canEdit && !editing}
      <button type="button" class="btn btn-secondary" onclick={startEditing}><Icon name="pencil" /><span class="hidden sm:inline">{t('Edit')}</span></button>
    {/if}

    {#if entries.length > 1 && !editing}
      <div class="flex items-center gap-1">
        <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Previous file')} title={t('Previous (←)')} disabled={index === 0} onclick={() => step(-1)}>
          <Icon name="chevron-left" />
        </button>
        <span class="hidden min-w-12 text-center text-xs text-fg-muted tabular-nums sm:inline">{t('{n} of {total}', { n: index + 1, total: entries.length })}</span>
        <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Next file')} title={t('Next (→)')} disabled={index === entries.length - 1} onclick={() => step(1)}>
          <Icon name="chevron-right" />
        </button>
      </div>
      <span class="h-5 w-px bg-line" aria-hidden="true"></span>
    {/if}

    {#if !editing}
      <button type="button" class="btn btn-secondary" onclick={download}>
        <Icon name="download" /><span class="hidden sm:inline">{t('Download')}</span>
      </button>
    {/if}
    <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Close preview')} title={t('Close (Esc)')} onclick={requestClose}>
      <Icon name="x" />
    </button>
  </header>

  {#if editing && offer}
    <div class="flex flex-wrap items-center gap-3 border-b border-line bg-subtle px-4 py-2 text-[13px]" role="status">
      <Icon name="circle-alert" class="size-4 shrink-0 text-fg-muted" />
      <p class="min-w-0 flex-1">
        <Sentence text={t('You have unsaved changes from {when}.')}>{#snippet when()}<Time ms={offer.updatedAt * 1000} relative />{/snippet}</Sentence>
        {#if offer.baseRevision !== entry.node.revision}<span class="text-fg-muted">{t('The file has changed since, so restoring replaces those changes when you save.')}</span>{/if}
      </p>
      <button type="button" class="btn btn-ghost h-7 px-2.5 text-[13px]" onclick={() => (drafts.drop(entry), (offer = null))}>{t('Discard')}</button>
      <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={restoreDraft}>{t('Restore')}</button>
    </div>
  {/if}

  <div class="relative min-h-0 flex-1">
    {#key entry.node.id}
      {#if view.status === 'loading'}
        <div class="absolute inset-0 grid place-items-center" in:fade={{ delay: 150 }}>
          <div class="grid w-56 gap-3 text-center">
            <p class="text-[13px] text-fg-muted">{t('Decrypting')}</p>
            <div class="progress"><div style:width="{Math.round(view.progress * 100)}%"></div></div>
          </div>
        </div>
      {:else if view.status === 'ready'}
        <div class="absolute inset-0 animate-enter">
          {#if kind.kind === 'image'}
            <div class="h-full overflow-auto">
              <button
                type="button"
                class={zoomed ? 'grid min-h-full w-max min-w-full cursor-zoom-out place-items-center p-6' : 'absolute inset-0 flex cursor-zoom-in items-center justify-center p-6'}
                aria-label={zoomed ? t('Fit to window') : t('Show actual size')}
                onclick={() => (zoomed = !zoomed)}>
                <img
                  src={view.url}
                  alt={entry.meta.name}
                  class="checkerboard rounded border border-line {zoomed ? 'max-w-none' : 'max-h-full max-w-full object-contain'}"
                  onload={imageLoaded}
                  onerror={() => (loaded = { id: entry.node.id, status: 'error', message: t("This image couldn't be displayed. It may be damaged, or in a format your browser doesn't support.") })} />
              </button>
            </div>
          {:else if kind.kind === 'video'}
            <div class="grid h-full place-items-center p-6">
              <!-- svelte-ignore a11y_media_has_caption -->
              <video bind:this={videoEl} src={view.url} controls class="max-h-full max-w-full rounded-md bg-black">
                {#each subtitles as sub (sub.url)}<track kind="subtitles" src={sub.url} srclang={sub.lang || undefined} label={sub.label} />{/each}
              </video>
            </div>
          {:else if kind.kind === 'audio'}
            <div class="grid h-full place-items-center p-6">
              <div class="card grid w-full max-w-md justify-items-center gap-4 p-8">
                <div class="grid size-14 place-items-center rounded-xl border border-line bg-subtle"><FileIcon meta={entry.meta} class="size-6" strokeWidth={1.5} /></div>
                <p class="max-w-full truncate font-medium">{entry.meta.name}</p>
                <audio src={view.url} controls class="w-full"></audio>
              </div>
            </div>
          {:else if kind.kind === 'pdf'}
            <PdfView blob={view.blob} />
          {:else if kind.kind === 'book'}
            {#key entry.node.id}<BookView blob={view.blob} {entry} format={kind.format} progress={bookProgress} />{/key}
          {:else if kind.kind === 'markdown' && editing}
            {#key editorKey}<MarkdownEditor text={editorText} onchange={onEdit} />{/key}
          {:else if kind.kind === 'text' && editing}
            {#key editorKey}<TextEditor text={editorText} onchange={onEdit} />{/key}
          {:else if kind.table && !showSource}
            <TableView text={view.text} separator={separatorFor(entry.meta.name)} />
          {:else if kind.kind === 'markdown' && !showSource}
            <MarkdownView text={view.text} loadImage={trail && list ? loadImage : null} ontoggle={save ? toggleTask : null} />
          {:else}
            <TextView text={view.text} name={kind.kind === 'markdown' ? 'source.md' : entry.meta.name} />
          {/if}
        </div>
      {:else}
        {@const notice = {
          unsupported: [t('No preview for this type of file'), t('Download it to open it with an app on your device.')],
          large: [t('Too large to preview'), t("Previews are made in memory, so they're limited to {size}. Download it instead.", { size: formatSize(kind?.kind === 'text' || kind?.kind === 'markdown' ? MAX_TEXT : MAX_PREVIEW) })],
          binary: [t("This doesn't look like text"), t('It has binary content, so there is nothing to show here. Download it instead.')],
          error: [t("Couldn't open this file"), view.message],
        }[view.status]}
        <div class="absolute inset-0 grid place-items-center p-6 animate-enter">
          <div class="grid max-w-sm justify-items-center gap-1 text-center">
            <div class="mb-3 grid size-14 place-items-center rounded-xl border border-line bg-subtle">
              {#if view.status === 'error'}<Icon name="circle-alert" class="size-6 text-danger" strokeWidth={1.5} />{:else}<FileIcon meta={entry.meta} class="size-6" strokeWidth={1.5} />{/if}
            </div>
            <p class="font-medium">{notice[0]}</p>
            <p class="text-[13px] text-fg-muted">{notice[1]}</p>
            <button type="button" class="btn btn-secondary mt-4" onclick={download}><Icon name="download" /> {t('Download')}</button>
          </div>
        </div>
      {/if}
    {/key}
  </div>
  {#if confirm}
    <ConfirmDialog
      title={t('Discard your changes?')}
      description={t("Your edits to {name} haven't been saved.", { name: entry.meta.name })}
      confirmLabel={t('Discard')}
      danger
      onconfirm={() => {
        const then = confirm.then;
        stopEditing();
        then();
      }}
      onclose={() => (confirm = null)} />
  {/if}
</dialog>
