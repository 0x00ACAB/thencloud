<script>
  // PDF pages drawn with pdf.js, fitted to the window width (up to a
  // readable maximum) and rendered only as they scroll into view, with
  // selectable text and clickable links over each.
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n.svelte.js';
  import Icon from '../Icon.svelte';

  let { blob } = $props();

  let scroller;
  let pages = $state([]); // [{ n, ratio, w }]: height / width, and width at scale 1
  let width = $state(0);
  let error = $state('');
  let zoom = $state(1);

  const fit = $derived(Math.min(width - 48, 900));
  const pageWidth = $derived(Math.max(200, Math.round(fit * zoom)));

  let doc;
  let loading; // pdf.js loading task; destroying it frees the document and worker
  const drawn = new Map(); // page number -> { task } of its latest render

  onMount(() => {
    let live = true;
    const ro = new ResizeObserver(([e]) => (width = e.contentRect.width));
    ro.observe(scroller);
    (async () => {
      try {
        const { openPdf } = await import('../../lib/pdf.js');
        const bytes = new Uint8Array(await blob.arrayBuffer());
        if (!live) return;
        loading = openPdf(bytes);
        doc = await loading.promise;
        if (!live) return;
        const list = [];
        for (let n = 1; n <= doc.numPages; n++) {
          const vp = (await doc.getPage(n)).getViewport({ scale: 1 });
          list.push({ n, ratio: vp.height / vp.width, w: vp.width });
        }
        if (live) pages = list;
      } catch (e) {
        if (live) error = e?.name === 'PasswordException' ? t("This PDF is password protected, which the preview doesn't support yet.") : t("This PDF couldn't be read. It may be damaged.");
      }
    })();
    return () => {
      live = false;
      ro.disconnect();
      loading?.destroy();
    };
  });

  // Svelte action: draw a page when it comes near the viewport, and redraw
  // it at the new size after a resize or zoom.
  function page(canvas, params) {
    let { n, w: target } = params;
    const io = new IntersectionObserver(([e]) => e.isIntersecting && draw(), { root: scroller, rootMargin: '600px 0px' });
    io.observe(canvas);
    // pdf.js refuses two renders on one canvas at once, so draws for a page
    // run one after another; a newer size cancels the render in flight.
    let queue = Promise.resolve();
    let wanted = 0;
    function draw() {
      if (!doc || wanted === target) return;
      const w = (wanted = target);
      drawn.get(n)?.task.cancel();
      queue = queue.then(async () => {
        if (!doc || w !== wanted) return;
        const { drawPage } = await import('../../lib/pdf.js');
        const task = drawPage(await doc.getPage(n), canvas, w);
        drawn.set(n, { task });
        await task.promise;
      }).catch(() => {}); // cancelled, or the document was closed
    }
    return {
      update: (next) => {
        target = next.w;
        const r = canvas.getBoundingClientRect();
        if (r.bottom > -600 && r.top < innerHeight + 600) draw();
      },
      destroy: () => {
        io.disconnect();
        wanted = -1;
        drawn.get(n)?.task.cancel();
        drawn.delete(n);
      },
    };
  }

  // Svelte action: the text and links over a page, made once when it first
  // comes near the viewport (they scale with the page through CSS).
  function layers(node, n) {
    const io = new IntersectionObserver(
      async ([e]) => {
        if (!e.isIntersecting || !doc) return;
        io.disconnect();
        try {
          const { drawText, pageLinks } = await import('../../lib/pdf.js');
          const page = await doc.getPage(n);
          const text = document.createElement('div');
          text.className = 'pdf-text';
          node.append(text);
          await drawText(page, text);
          const links = document.createElement('div');
          links.className = 'pdf-links';
          for (const l of await pageLinks(doc, page)) {
            const a = document.createElement('a');
            Object.assign(a.style, { left: `${l.left}%`, top: `${l.top}%`, width: `${l.width}%`, height: `${l.height}%` });
            if (l.url) {
              a.href = l.url;
              a.target = '_blank';
              a.rel = 'noopener noreferrer';
              a.title = l.url;
            } else {
              a.href = '#';
              a.title = t('Go to page {n}', { n: l.page });
              a.onclick = (ev) => {
                ev.preventDefault();
                scroller.querySelector(`[data-page="${l.page}"]`)?.scrollIntoView({ block: 'start' });
              };
            }
            links.append(a);
          }
          node.append(links);
        } catch {
          /* the page still shows; it just can't be selected */
        }
      },
      { root: scroller, rootMargin: '600px 0px' },
    );
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }
</script>

<div bind:this={scroller} class="h-full overflow-auto bg-subtle">
  {#if error}
    <p class="grid h-full place-items-center p-6 text-[13px] text-fg-muted">{error}</p>
  {:else if !pages.length}
    <div class="grid h-full place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
  {:else}
    <div class="grid justify-items-center gap-4 px-6 pt-6 pb-2">
      {#each pages as p (p.n)}
        <div
          use:layers={p.n}
          data-page={p.n}
          class="relative scroll-mt-6 overflow-hidden rounded-sm bg-muted shadow-sm ring-1 ring-line"
          style:width="{pageWidth}px"
          style:height="{Math.round(pageWidth * p.ratio)}px"
          style:--total-scale-factor={pageWidth / p.w}>
          <canvas use:page={{ n: p.n, w: pageWidth }} class="block size-full" aria-label={t('Page {n} of {total}', { n: p.n, total: pages.length })}></canvas>
        </div>
      {/each}
    </div>
    <div class="sticky bottom-4 flex justify-center">
      <div class="flex items-center gap-1 rounded-lg border border-line bg-bg p-1 shadow-sm">
        <button type="button" class="btn btn-ghost btn-icon h-7" aria-label={t('Zoom out')} disabled={zoom <= 0.5} onclick={() => (zoom = Math.max(0.5, zoom - 0.25))}><Icon name="zoom-out" /></button>
        <button type="button" class="h-7 min-w-12 cursor-pointer rounded px-1 text-xs text-fg-muted tabular-nums hover:text-fg" title={t('Fit to width')} onclick={() => (zoom = 1)}>{Math.round(zoom * 100)}%</button>
        <button type="button" class="btn btn-ghost btn-icon h-7" aria-label={t('Zoom in')} disabled={zoom >= 3} onclick={() => (zoom = Math.min(3, zoom + 0.25))}><Icon name="zoom-in" /></button>
        <span class="mx-1 h-4 w-px bg-line" aria-hidden="true"></span>
        <span class="px-1 text-xs text-fg-muted">{t('{count} pages', { count: pages.length })}</span>
      </div>
    </div>
  {/if}
</div>
