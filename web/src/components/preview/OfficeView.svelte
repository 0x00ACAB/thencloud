<script>
  // Word, OpenDocument text, Excel and PowerPoint files, read by
  // lib/office.js into plain data and shown here as text: nothing from the
  // document reaches the page as markup. A reading preview, not the layout.
  import { onMount } from 'svelte';
  import { t } from '../../lib/i18n.svelte.js';
  import { formatNumber } from '../../lib/locale.svelte.js';
  import Icon from '../Icon.svelte';

  let { blob, format } = $props();

  let doc = $state(null);
  let error = $state('');
  let sheet = $state(0);
  let shown = $state(500);

  onMount(() => {
    let live = true;
    let opened = null;
    (async () => {
      try {
        const { openOffice } = await import('../../lib/office.js');
        opened = await openOffice(new Uint8Array(await blob.arrayBuffer()), format);
        if (live) doc = opened;
        else for (const u of opened.urls ?? []) URL.revokeObjectURL(u);
      } catch (e) {
        // The reader's own messages aren't translated; say it plainly.
        if (live) error = t("This file couldn't be opened.");
      }
    })();
    return () => {
      live = false;
      for (const u of opened?.urls ?? []) URL.revokeObjectURL(u);
    };
  });

  /** A1-style column letters for column `i`. */
  function letters(i) {
    let s = '';
    for (let n = i + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s;
    return s;
  }

  const current = $derived(doc?.kind === 'sheets' ? doc.sheets[sheet] : null);
  const width = $derived(current ? Math.max(0, ...current.rows.map((r) => r.length)) : 0);

  const headings = {
    h1: 'mt-6 mb-3 text-2xl font-semibold tracking-tight first:mt-0',
    h2: 'mt-6 mb-2 text-xl font-semibold tracking-tight first:mt-0',
    h3: 'mt-5 mb-2 text-lg font-semibold first:mt-0',
    h4: 'mt-4 mb-1.5 text-base font-semibold first:mt-0',
    h5: 'mt-4 mb-1 text-sm font-semibold first:mt-0',
    h6: 'mt-4 mb-1 text-sm font-semibold text-fg-muted first:mt-0',
  };
</script>

{#snippet runs(list)}
  {#each list as r, i (i)}
    {@const cls = [r.b && 'font-semibold', r.i && 'italic', r.u && 'underline', r.s && 'line-through'].filter(Boolean).join(' ')}
    {#if r.href}
      <a class="text-accent underline underline-offset-2 {cls}" href={r.href} target="_blank" rel="noopener noreferrer">{r.text}</a>
    {:else if cls}
      <span class={cls}>{r.text}</span>
    {:else}{r.text}{/if}
  {/each}
{/snippet}

{#snippet blocks(list)}
  {#each list as b, i (i)}
    {#if b.t === 'table'}
      <div class="my-4 overflow-x-auto">
        <table class="border-collapse text-[13px] [&_td]:border [&_td]:border-line [&_td]:px-2.5 [&_td]:py-1 [&_td]:align-top [&_td_p]:my-0">
          <tbody>
            {#each b.rows as row, r (r)}
              <tr>{#each row as cell, c (c)}<td>{@render blocks(cell)}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if b.t === 'img'}
      <img src={b.url} alt="" class="my-3 h-auto max-w-full rounded-md" loading="lazy" />
    {:else if b.style === 'li'}
      <p class="my-0.5 flex gap-2 whitespace-pre-wrap" style:padding-left="{b.level * 1.25}rem">
        <span class="shrink-0 text-fg-faint select-none" aria-hidden="true">{b.ordered ? `${b.n}.` : '•'}</span><span>{@render runs(b.runs)}</span>
      </p>
    {:else if headings[b.style]}
      <p class={headings[b.style]} role="heading" aria-level={Number(b.style[1])}>{@render runs(b.runs)}</p>
    {:else}
      <p class="my-2 min-h-[1lh] whitespace-pre-wrap">{@render runs(b.runs)}</p>
    {/if}
  {/each}
{/snippet}

<div class="flex h-full flex-col">
  {#if error}
    <p class="grid flex-1 place-items-center px-6 text-center text-[13px] text-fg-muted">{error}</p>
  {:else if !doc}
    <p class="grid flex-1 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></p>
  {:else if doc.kind === 'sheets'}
    {#if doc.sheets.length > 1}
      <div class="flex shrink-0 gap-1 overflow-x-auto border-b border-line px-3 py-1.5" role="tablist">
        {#each doc.sheets as s, i (i)}
          <button
            type="button"
            role="tab"
            aria-selected={sheet === i}
            class="h-7 shrink-0 cursor-pointer rounded-md px-2.5 text-[13px] {sheet === i ? 'bg-subtle font-medium text-fg ring-1 ring-line' : 'text-fg-muted hover:text-fg'}"
            onclick={() => {
              sheet = i;
              shown = 500;
            }}>{s.name}</button>
        {/each}
      </div>
    {/if}
    {#if !current.rows.length}
      <p class="grid flex-1 place-items-center text-[13px] text-fg-muted">{t('This sheet is empty.')}</p>
    {:else}
      <div class="min-h-0 flex-1 overflow-auto">
        <table class="min-w-full border-separate border-spacing-0 text-[13px] [&_td]:border-b [&_td]:border-line [&_td]:px-3 [&_td]:py-1.5 [&_td]:whitespace-nowrap [&_td+td]:border-l [&_th]:sticky [&_th]:top-0 [&_th]:z-[1] [&_th]:border-b [&_th]:border-line [&_th]:bg-subtle [&_th]:px-3 [&_th]:py-1.5 [&_th]:text-left [&_th]:font-medium [&_th]:text-fg-muted [&_th+th]:border-l">
          <thead>
            <tr>
              <th class="w-px text-right text-fg-faint"><span class="sr-only">{t('Row')}</span></th>
              {#each { length: width } as _, c (c)}<th>{letters(c)}</th>{/each}
            </tr>
          </thead>
          <tbody>
            {#each current.rows.slice(0, shown) as row, r (r)}
              <tr>
                <td class="text-right text-fg-faint tabular-nums select-none">{r + 1}</td>
                {#each { length: width } as _, c (c)}
                  <td class="max-w-96 truncate" title={row[c]?.length > 40 ? row[c] : undefined}>{row[c] ?? ''}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
        {#if current.rows.length > shown}
          <button class="btn btn-secondary m-3" onclick={() => (shown += 500)}>{t('Show more rows')}</button>
        {/if}
      </div>
    {/if}
    <p class="shrink-0 border-t border-line px-4 py-2 text-xs text-fg-faint">
      {#if current.truncated}
        {t('Only the first {rows} rows and {cols} columns are shown.', { rows: formatNumber(5000), cols: formatNumber(200) })}
      {:else}
        {t("Values as last saved. Number formats and charts aren't shown.")}
      {/if}
    </p>
  {:else}
    <div class="min-h-0 flex-1 overflow-auto">
      <article class="mx-auto max-w-3xl px-6 py-8 text-[15px] leading-7 text-fg">
        {#if doc.kind === 'slides'}
          {#each doc.slides as s, i (i)}
            <section class="mb-6 rounded-lg border border-line p-6" aria-label={t('Slide {n}', { n: i + 1 })}>
              <p class="mb-3 text-xs text-fg-faint">{t('Slide {n}', { n: i + 1 })}</p>
              {#if s.blocks.length}{@render blocks(s.blocks)}{:else}<p class="text-[13px] text-fg-muted">{t('No text on this slide.')}</p>{/if}
            </section>
          {/each}
        {:else if doc.blocks.length}
          {@render blocks(doc.blocks)}
        {:else}
          <p class="text-[13px] text-fg-muted">{t('This document has no text.')}</p>
        {/if}
        {#if doc.truncated}
          <p class="mt-6 border-t border-line pt-4 text-xs text-fg-faint">{t("This is as far as the preview goes. Download the file to see the rest.")}</p>
        {/if}
      </article>
    </div>
    <p class="shrink-0 border-t border-line px-4 py-2 text-xs text-fg-faint">{t('A simple preview: the text, tables and pictures, without the original layout.')}</p>
  {/if}
</div>
