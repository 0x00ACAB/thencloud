<script>
  // PDF tools: one PDF (rotate, reorder, remove or extract pages) or several
  // (merge them). The files are decrypted here, edited with pdf-lib and the
  // result is saved as a new encrypted file next to them, or downloaded.
  // Thumbnails are drawn by pdf.js as they come into view.
  import { onMount, onDestroy, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import Modal from '../Modal.svelte';
  import { t } from '../../lib/i18n.svelte.js';
  import Icon from '../Icon.svelte';
  import { saveBlob } from '../../lib/crypto.js';
  import { formatSize } from '../../lib/format.js';
  import { toast, errorMessage } from '../../lib/ui.svelte.js';

  /** fetch(entry, onProgress) -> { blob }; save(file, onProgress) uploads next to the originals (null if you can't write here). */
  let { entries, fetch, save = null, onclose } = $props();

  const merging = untrack(() => entries.length > 1);
  const base = (n) => n.replace(/\.pdf$/i, '');
  let name = $state(untrack(() => (merging ? t('{name} and {count} more', { name: base(entries[0].meta.name), count: entries.length - 1 }) + '.pdf' : t('{name} (edited)', { name: base(entries[0].meta.name) }) + '.pdf')));

  let phase = $state('loading'); // loading | ready | saving | error
  let error = $state('');
  let loaded = $state(0);
  let tools = null; // { loadPdf, buildPdf, rotations, openPdf, drawPage, MAX_PDF }
  let sources = []; // [{ doc (pdf-lib), task (pdf.js loading task), view (its document) }]
  /** The pages of the result, in order. */
  let pages = $state([]); // [{ key, src, page, rotate }]
  const selected = new SvelteSet();
  let nextKey = 0;

  onMount(async () => {
    try {
      const [edit, pdf] = await Promise.all([import('../../lib/pdfedit.js'), import('../../lib/pdf.js')]);
      tools = { ...edit, ...pdf };
      const list = [];
      for (const [src, entry] of entries.entries()) {
        if (entry.meta.size > edit.MAX_PDF) throw new Error(t('{name} is too big for the PDF tools (over {size}).', { name: entry.meta.name, size: formatSize(edit.MAX_PDF) }));
        const { blob } = await fetch(entry, () => {});
        const bytes = new Uint8Array(await blob.arrayBuffer());
        let doc;
        try {
          doc = await edit.loadPdf(bytes);
        } catch (e) {
          throw new Error(`${entry.meta.name}: ${pdfError(e)}`);
        }
        // pdf.js takes its copy to its worker.
        const task = pdf.openPdf(bytes.slice());
        sources.push({ doc, task, view: await task.promise });
        for (let page = 0; page < doc.getPageCount(); page++) list.push({ key: nextKey++, src, page, rotate: 0 });
        loaded = src + 1;
      }
      pages = list;
      phase = 'ready';
    } catch (e) {
      error = errorMessage(e);
      phase = 'error';
    }
  });

  onDestroy(() => {
    for (const s of sources) Promise.resolve().then(() => s.task.destroy()).catch(() => {});
  });

  const BOX = 112;

  /** Draw a page's thumbnail once it's near the screen. */
  function thumb(canvas, p) {
    const io = new IntersectionObserver(
      async (es) => {
        if (!es.some((e) => e.isIntersecting)) return;
        io.disconnect();
        try {
          const page = await sources[p.src].view.getPage(p.page + 1);
          // Fit a square box, keeping the page's shape (also when turned).
          const { width, height } = page.getViewport({ scale: 1 });
          const w = Math.round(BOX * Math.min(1, width / height));
          await tools.drawPage(page, canvas, w).promise;
          canvas.style.width = `${w}px`;
          canvas.style.height = `${Math.round((w * height) / width)}px`;
        } catch {
          /* the tile stays blank */
        }
      },
      { rootMargin: '300px' },
    );
    io.observe(canvas);
    return { destroy: () => io.disconnect() };
  }

  const rotate = (p, by) => (p.rotate = (p.rotate + by + 360) % 360);
  function move(i, by) {
    const j = i + by;
    if (j < 0 || j >= pages.length) return;
    const next = [...pages];
    [next[i], next[j]] = [next[j], next[i]];
    pages = next;
  }
  function remove(p) {
    pages = pages.filter((x) => x !== p);
    selected.delete(p.key);
  }
  const toggle = (p) => (selected.has(p.key) ? selected.delete(p.key) : selected.add(p.key));

  // Drag a page onto another to put it there.
  let dragging = $state(null);
  let over = $state(null);
  function drop(target) {
    if (dragging === null || dragging === target.key) return;
    const from = pages.findIndex((p) => p.key === dragging);
    const next = [...pages];
    const [moved] = next.splice(from, 1);
    next.splice(next.findIndex((p) => p.key === target.key) + (from < pages.indexOf(target) ? 1 : 0), 0, moved);
    pages = next;
    dragging = over = null;
  }

  async function finish(which, fileName) {
    phase = 'saving';
    try {
      const bytes = await tools.buildPdf(
        sources.map((s) => s.doc),
        which.map(({ src, page, rotate }) => ({ src, page, rotate })),
      );
      const out = fileName.trim().toLowerCase().endsWith('.pdf') ? fileName.trim() : `${fileName.trim() || t('Document')}.pdf`;
      const file = new File([bytes], out, { type: 'application/pdf', lastModified: Date.now() });
      if (save) {
        await save(file, () => {});
        toast(t('Saved {name}', { name: out }), { kind: 'success' });
      } else await saveBlob(file, out);
      onclose();
    } catch (e) {
      error = pdfError(e);
      phase = 'ready';
    }
  }

  function extract() {
    const chosen = pages.filter((p) => selected.has(p.key));
    const numbers = pages.flatMap((p, i) => (selected.has(p.key) ? [i + 1] : []));
    const which = numbers.length === 1 ? t('page {n}', { n: numbers[0] }) : numbers.length <= 5 ? t('pages {list}', { list: numbers.join(', ') }) : t('{count} pages', { count: numbers.length });
    const from = merging ? base(name) : base(entries[0].meta.name);
    finish(chosen, `${from} (${which}).pdf`);
  }
  const label = (p) => (merging ? t('{name}, page {n}', { name: entries[p.src].meta.name, n: p.page + 1 }) : t('Page {n}', { n: p.page + 1 }));

  // pdfedit.js stays free of the UI (it's fuzzed in node), so its few errors are translated here.
  const pdfError = (e) =>
    ({
      "This PDF is protected with a password, so its pages can't be copied.": t("This PDF is protected with a password, so its pages can't be copied."),
      "This file couldn't be read as a PDF.": t("This file couldn't be read as a PDF."),
      'There are no pages to save.': t('There are no pages to save.'),
    })[e?.message] ?? errorMessage(e);
</script>

<Modal
  title={merging ? t('Merge {count} PDFs', { count: entries.length }) : t('Edit {name}', { name: entries[0].meta.name })}
  description={merging ? t('Rotate, reorder or remove pages.') : t('Rotate, reorder or remove pages, or pick some to save on their own.')}
  class="max-w-4xl"
  onclose={onclose}>
  {#if phase === 'loading'}
    <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="loader-circle" class="spinner" />{merging ? t('Reading {n} of {total} files', { n: loaded, total: entries.length }) : t('Reading the file')}</p>
  {:else if phase === 'error' && !pages.length}
    <p class="flex items-center gap-2 text-[13px] text-danger"><Icon name="circle-alert" class="size-4" />{error}</p>
  {:else}
    <ol class="grid max-h-[55vh] grid-cols-[repeat(auto-fill,minmax(8.5rem,1fr))] gap-3 overflow-y-auto p-1" aria-label={t('Pages')}>
      {#each pages as p, i (p.key)}
        <li
          class="group relative grid gap-1.5 rounded-lg border p-2 transition-colors {selected.has(p.key) ? 'border-accent bg-accent-soft' : 'border-line'} {over === p.key ? 'ring-2 ring-accent' : ''}"
          draggable="true"
          ondragstart={(e) => ((dragging = p.key), e.dataTransfer.setData('text/plain', ''))}
          ondragend={() => (dragging = over = null)}
          ondragover={(e) => (e.preventDefault(), (over = p.key))}
          ondragleave={() => over === p.key && (over = null)}
          ondrop={(e) => (e.preventDefault(), drop(p))}>
          <div class="grid aspect-square place-items-center overflow-hidden rounded bg-subtle">
            <canvas use:thumb={p} class="bg-white shadow-sm transition-transform" style:rotate="{p.rotate}deg" aria-hidden="true"></canvas>
          </div>
          <label class="flex min-w-0 items-center gap-1.5 text-xs">
            <input type="checkbox" class="size-3.5 accent-accent" checked={selected.has(p.key)} onchange={() => toggle(p)} />
            <span class="truncate" title={label(p)}><span class="font-medium tabular-nums">{i + 1}</span>{#if merging || p.page !== i}<span class="text-fg-muted"> · {merging ? t('{name} p. {n}', { name: base(entries[p.src].meta.name), n: p.page + 1 }) : t('was {n}', { n: p.page + 1 })}</span>{/if}</span>
          </label>
          <div class="flex justify-between opacity-100 transition-opacity sm:opacity-0 sm:group-focus-within:opacity-100 sm:group-hover:opacity-100">
            <button type="button" class="btn btn-ghost btn-icon size-6" aria-label={t('Move {page} earlier', { page: label(p) })} disabled={i === 0} onclick={() => move(i, -1)}><Icon name="chevron-left" class="size-3.5" /></button>
            <button type="button" class="btn btn-ghost btn-icon size-6" aria-label={t('Rotate {page} left', { page: label(p) })} onclick={() => rotate(p, -90)}><Icon name="undo-2" class="size-3.5" /></button>
            <button type="button" class="btn btn-ghost btn-icon size-6" aria-label={t('Rotate {page} right', { page: label(p) })} onclick={() => rotate(p, 90)}><Icon name="redo-2" class="size-3.5" /></button>
            <button type="button" class="btn btn-ghost btn-icon size-6 text-danger hover:text-danger" aria-label={t('Remove {page}', { page: label(p) })} disabled={pages.length === 1} onclick={() => remove(p)}><Icon name="trash-2" class="size-3.5" /></button>
            <button type="button" class="btn btn-ghost btn-icon size-6" aria-label={t('Move {page} later', { page: label(p) })} disabled={i === pages.length - 1} onclick={() => move(i, 1)}><Icon name="chevron-right" class="size-3.5" /></button>
          </div>
        </li>
      {/each}
    </ol>
    <div class="grid gap-1.5">
      <label class="label" for="pdf-name">{t('Save as')}</label>
      <input id="pdf-name" class="input" bind:value={name} spellcheck="false" />
    </div>
    {#if error}<p class="flex items-center gap-2 text-[13px] text-danger"><Icon name="circle-alert" class="size-4" />{error}</p>{/if}
  {/if}

  {#snippet footer()}
    <span class="mr-auto text-xs text-fg-muted tabular-nums">{t('{count} pages', { count: pages.length })}{selected.size ? ` · ${t('{count} selected', { count: selected.size })}` : ''}</span>
    <button type="button" class="btn btn-ghost" onclick={onclose}>{t('Cancel')}</button>
    {#if selected.size && phase === 'ready'}
      <button type="button" class="btn btn-secondary" disabled={phase !== 'ready'} onclick={extract}>
        {save ? t('Save {count} pages on their own', { count: selected.size }) : t('Download {count} pages on their own', { count: selected.size })}
      </button>
    {/if}
    <button type="button" class="btn btn-primary" disabled={phase !== 'ready' || !pages.length} onclick={() => finish(pages, name)}>
      {#if phase === 'saving'}<Icon name="loader-circle" class="spinner" />{/if}
      {save ? t('Save as new file') : t('Download')}
    </button>
  {/snippet}
</Modal>
