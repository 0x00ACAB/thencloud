<script>
  // Full-window file preview. Files are decrypted in the browser, exactly
  // like a download, and shown from a blob: URL that is revoked when you
  // move on. ← and → step through the other files in the folder.
  import { onMount, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import TextView from './preview/TextView.svelte';
  import MarkdownView from './preview/MarkdownView.svelte';
  import PdfView from './preview/PdfView.svelte';
  import { saveBlob } from '../lib/crypto.js';
  import { previewKind, readText, MAX_PREVIEW, MAX_TEXT } from '../lib/preview.js';
  import { formatSize, formatWhen, fileIcon } from '../lib/format.js';
  import { errorMessage } from '../lib/ui.svelte.js';
  import { fade } from '../lib/motion.js';

  /** @type {{ entries: any[], start: number, fetch: (entry: any, onProgress: (p: number) => void) => Promise<{ blob: Blob }>, ondownload: (entry: any) => void, onclose: () => void }} */
  let { entries, start, fetch, ondownload, onclose } = $props();

  let dlg;
  let index = $state(untrack(() => start));
  let loaded = $state({ id: null }); // what load() produced, for the file with this id
  let showSource = $state(false);
  let zoomed = $state(false);

  const entry = $derived(entries[index]);
  const kind = $derived(previewKind(entry.meta));
  // Until load() catches up with a step to another file, show it as loading.
  const view = $derived(loaded.id === entry.node.id ? loaded : { status: 'loading', progress: 0 });

  let seq = 0;
  let url = null;

  function release() {
    if (url) URL.revokeObjectURL(url);
    url = null;
  }

  async function load(e) {
    const my = ++seq;
    const id = e.node.id;
    const set = (v) => (loaded = { id, ...v });
    release();
    zoomed = false;
    const k = previewKind(e.meta);
    if (!k) return set({ status: 'unsupported' });
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
      } else if (k.kind === 'pdf') {
        set({ status: 'ready', blob });
      } else {
        url = URL.createObjectURL(blob);
        set({ status: 'ready', blob, url });
      }
    } catch (err) {
      if (my === seq) set({ status: 'error', message: errorMessage(err) });
    }
  }

  $effect(() => {
    load(entry);
  });

  onMount(() => {
    dlg.showModal();
    return release;
  });

  function step(d) {
    const next = index + d;
    if (next >= 0 && next < entries.length) index = next;
  }

  function onkeydown(e) {
    if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.target instanceof HTMLElement && e.target.closest('input, textarea, video, audio')) return;
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

<svelte:window {onkeydown} />

<dialog bind:this={dlg} class="preview" aria-label="Preview of {entry.meta.name}" onclose={() => onclose()}>
  <header class="flex h-14 shrink-0 items-center gap-3 border-b border-line px-4">
    <Icon name={fileIcon(entry.meta)} class="size-4 shrink-0 text-fg-muted" />
    <div class="min-w-0 flex-1">
      <h2 class="truncate text-sm font-medium">{entry.meta.name}</h2>
      <p class="truncate text-xs text-fg-muted">
        {formatSize(entry.meta.size)}{entry.node.updated_at ? ` · ${formatWhen(entry.node.updated_at * 1000)}` : ''}
      </p>
    </div>

    {#if kind?.kind === 'markdown' && view.status === 'ready'}
      <div class="hidden rounded-md border border-line p-0.5 sm:flex" role="radiogroup" aria-label="Show">
        {#each [[false, 'book-open', 'Preview'], [true, 'code', 'Source']] as [value, icon, label] (label)}
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

    {#if entries.length > 1}
      <div class="flex items-center gap-1">
        <button type="button" class="btn btn-ghost btn-icon" aria-label="Previous file" title="Previous (←)" disabled={index === 0} onclick={() => step(-1)}>
          <Icon name="chevron-left" />
        </button>
        <span class="hidden min-w-12 text-center text-xs text-fg-muted tabular-nums sm:inline">{index + 1} of {entries.length}</span>
        <button type="button" class="btn btn-ghost btn-icon" aria-label="Next file" title="Next (→)" disabled={index === entries.length - 1} onclick={() => step(1)}>
          <Icon name="chevron-right" />
        </button>
      </div>
      <span class="h-5 w-px bg-line" aria-hidden="true"></span>
    {/if}

    <button type="button" class="btn btn-secondary" onclick={download}>
      <Icon name="download" /><span class="hidden sm:inline">Download</span>
    </button>
    <button type="button" class="btn btn-ghost btn-icon" aria-label="Close preview" title="Close (Esc)" onclick={() => dlg.close()}>
      <Icon name="x" />
    </button>
  </header>

  <div class="relative min-h-0 flex-1">
    {#key entry.node.id}
      {#if view.status === 'loading'}
        <div class="absolute inset-0 grid place-items-center" in:fade={{ delay: 150 }}>
          <div class="grid w-56 gap-3 text-center">
            <p class="text-[13px] text-fg-muted">Decrypting</p>
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
                aria-label={zoomed ? 'Fit to window' : 'Show actual size'}
                onclick={() => (zoomed = !zoomed)}>
                <img
                  src={view.url}
                  alt={entry.meta.name}
                  class="checkerboard rounded border border-line {zoomed ? 'max-w-none' : 'max-h-full max-w-full object-contain'}"
                  onload={imageLoaded}
                  onerror={() => (loaded = { id: entry.node.id, status: 'error', message: "This image couldn't be displayed. It may be damaged, or in a format your browser doesn't support." })} />
              </button>
            </div>
          {:else if kind.kind === 'video'}
            <div class="grid h-full place-items-center p-6">
              <!-- svelte-ignore a11y_media_has_caption -->
              <video src={view.url} controls class="max-h-full max-w-full rounded-md bg-black"></video>
            </div>
          {:else if kind.kind === 'audio'}
            <div class="grid h-full place-items-center p-6">
              <div class="card grid w-full max-w-md justify-items-center gap-4 p-8">
                <div class="grid size-14 place-items-center rounded-xl border border-line bg-subtle"><Icon name="file-audio" class="size-6 text-fg-muted" strokeWidth={1.5} /></div>
                <p class="max-w-full truncate font-medium">{entry.meta.name}</p>
                <audio src={view.url} controls class="w-full"></audio>
              </div>
            </div>
          {:else if kind.kind === 'pdf'}
            <PdfView blob={view.blob} />
          {:else if kind.kind === 'markdown' && !showSource}
            <MarkdownView text={view.text} />
          {:else}
            <TextView text={view.text} name={kind.kind === 'markdown' ? 'source.md' : entry.meta.name} />
          {/if}
        </div>
      {:else}
        {@const notice = {
          unsupported: ['No preview for this type of file', 'Download it to open it with an app on your device.'],
          large: ['Too large to preview', `Previews are decrypted in memory, so they're limited to ${formatSize(kind?.kind === 'text' || kind?.kind === 'markdown' ? MAX_TEXT : MAX_PREVIEW)}. Download it instead.`],
          binary: ["This doesn't look like text", 'It has binary content, so there is nothing to show here. Download it instead.'],
          error: ["Couldn't open this file", view.message],
        }[view.status]}
        <div class="absolute inset-0 grid place-items-center p-6 animate-enter">
          <div class="grid max-w-sm justify-items-center gap-1 text-center">
            <div class="mb-3 grid size-14 place-items-center rounded-xl border border-line bg-subtle">
              <Icon name={view.status === 'error' ? 'circle-alert' : fileIcon(entry.meta)} class="size-6 {view.status === 'error' ? 'text-danger' : 'text-fg-muted'}" strokeWidth={1.5} />
            </div>
            <p class="font-medium">{notice[0]}</p>
            <p class="text-[13px] text-fg-muted">{notice[1]}</p>
            <button type="button" class="btn btn-secondary mt-4" onclick={download}><Icon name="download" /> Download</button>
          </div>
        </div>
      {/if}
    {/key}
  </div>
</dialog>
