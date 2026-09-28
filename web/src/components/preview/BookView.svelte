<script>
  // EPUB and comic (CBZ) books. A comic is a page at a time; an EPUB a
  // chapter at a time, sanitised like Markdown, with its images decrypted
  // from inside the book. Where you were is kept through `progress`
  // (encrypted app data) when it's given; public links don't have it.
  import { onMount } from 'svelte';
  import Icon from '../Icon.svelte';

  let { blob, entry, format, progress = null } = $props();

  let book = $state(null);
  let at = $state(0);
  let error = $state('');
  let pageUrl = $state(null);
  let article = $state();
  let scroller = $state();
  let urls = []; // blob: URLs of the current chapter's images
  let chapterPath = '';
  let restoreFrac = 0;
  let seq = 0;

  onMount(() => {
    let live = true;
    (async () => {
      try {
        const lib = await import('../../lib/books.js');
        const bytes = new Uint8Array(await blob.arrayBuffer());
        const b = format === 'cbz' ? lib.openCbz(bytes) : await lib.openEpub(bytes);
        const pos = await progress?.load(entry).catch(() => null);
        if (!live) return;
        if (pos && pos.at >= 0 && pos.at < b.count) {
          at = pos.at;
          restoreFrac = pos.frac ?? 0;
        }
        book = b;
      } catch (e) {
        if (live) error = e?.message || "This book couldn't be opened.";
      }
    })();
    return () => {
      live = false;
      drop();
    };
  });

  function drop() {
    for (const u of urls) URL.revokeObjectURL(u);
    urls = [];
    if (pageUrl) URL.revokeObjectURL(pageUrl);
    pageUrl = null;
  }

  // Show the page or chapter `at` whenever it changes.
  $effect(() => {
    if (!book) return;
    const i = at;
    const my = ++seq;
    (async () => {
      try {
        if (book.kind === 'cbz') {
          const url = await book.page(i);
          if (my !== seq) return url && URL.revokeObjectURL(url);
          drop();
          pageUrl = url;
          if (!url) error = "This page isn't an image this browser can show.";
        } else {
          const c = await book.chapter(i);
          if (my !== seq) return c.urls.forEach((u) => URL.revokeObjectURL(u));
          drop();
          urls = c.urls;
          chapterPath = c.path;
          article.replaceChildren(c.fragment);
          const frac = restoreFrac;
          restoreFrac = 0;
          requestAnimationFrame(() => scroller && (scroller.scrollTop = frac * (scroller.scrollHeight - scroller.clientHeight)));
        }
        error = '';
      } catch (e) {
        if (my === seq) error = e?.message || "This part of the book couldn't be read.";
      }
    })();
  });

  function go(i) {
    if (!book || i < 0 || i >= book.count) return;
    at = i;
    progress?.save(entry, { at: i, frac: 0 });
    if (scroller) scroller.scrollTop = 0;
  }

  function onscroll() {
    if (book?.kind !== 'epub' || !scroller) return;
    const range = scroller.scrollHeight - scroller.clientHeight;
    progress?.save(entry, { at, frac: range > 0 ? scroller.scrollTop / range : 0 });
  }

  // Links to other chapters move within the book; others open in a new tab.
  function onclick(e) {
    const a = e.target.closest?.('a[data-book-href]');
    if (!a || book?.kind !== 'epub') return;
    e.preventDefault();
    const i = book.chapterOf(chapterPath, a.getAttribute('data-book-href'));
    if (i >= 0) go(i);
  }

  // Taken before the preview's own arrow keys, which step between files.
  function onkeydown(e) {
    if (!book || e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.target instanceof HTMLElement && e.target.closest('input, textarea, [contenteditable]')) return;
    const back = e.key === 'ArrowLeft' || e.key === 'PageUp';
    const on = e.key === 'ArrowRight' || e.key === 'PageDown' || (e.key === ' ' && book.kind === 'cbz');
    if (!back && !on) return;
    // In a chapter, Page Up/Down scroll; only the arrows turn chapters.
    if (book.kind === 'epub' && (e.key === 'PageUp' || e.key === 'PageDown')) return;
    e.preventDefault();
    e.stopPropagation();
    go(at + (on ? 1 : -1));
  }

  onMount(() => {
    window.addEventListener('keydown', onkeydown, true);
    return () => window.removeEventListener('keydown', onkeydown, true);
  });
</script>

<div class="flex h-full flex-col">
  {#if error && !book}
    <div class="grid h-full place-items-center p-6">
      <p class="max-w-sm text-center text-[13px] text-fg-muted">{error}</p>
    </div>
  {:else if !book}
    <div class="grid h-full place-items-center"><Icon name="loader-circle" class="spinner size-5 text-fg-muted" /></div>
  {:else}
    {#if book.kind === 'cbz'}
      <div class="relative min-h-0 flex-1">
        {#if pageUrl}
          <img src={pageUrl} alt="Page {at + 1}" class="absolute inset-0 m-auto max-h-full max-w-full object-contain p-4 select-none" draggable="false" />
        {:else if error}
          <p class="grid h-full place-items-center text-[13px] text-fg-muted">{error}</p>
        {/if}
        <!-- Tap either side to turn the page. -->
        <button type="button" class="absolute inset-y-0 left-0 w-1/3 cursor-w-resize" aria-label="Previous page" disabled={at === 0} onclick={() => go(at - 1)}></button>
        <button type="button" class="absolute inset-y-0 right-0 w-1/3 cursor-e-resize" aria-label="Next page" disabled={at === book.count - 1} onclick={() => go(at + 1)}></button>
      </div>
    {:else}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="min-h-0 flex-1 overflow-auto" bind:this={scroller} {onscroll} {onclick}>
        {#if error}<p class="p-6 text-center text-[13px] text-fg-muted">{error}</p>{/if}
        <article bind:this={article} class="prose book mx-auto max-w-2xl px-6 py-10"></article>
      </div>
    {/if}
    <div class="flex items-center justify-center gap-3 border-t border-line px-4 py-2 text-[13px] text-fg-muted">
      <button type="button" class="btn btn-ghost btn-icon" aria-label={book.kind === 'cbz' ? 'Previous page' : 'Previous chapter'} disabled={at === 0} onclick={() => go(at - 1)}>
        <Icon name="chevron-left" />
      </button>
      <span class="tabular-nums">{book.kind === 'cbz' ? 'Page' : 'Chapter'} {at + 1} of {book.count}</span>
      <button type="button" class="btn btn-ghost btn-icon" aria-label={book.kind === 'cbz' ? 'Next page' : 'Next chapter'} disabled={at === book.count - 1} onclick={() => go(at + 1)}>
        <Icon name="chevron-right" />
      </button>
    </div>
  {/if}
</div>
