<script>
  // CSV and TSV as a table: the first row is the header, and clicking a
  // header sorts by that column (numbers by value). Very long files show
  // their first rows only; the Source view still has everything.
  import Icon from '../Icon.svelte';
  import { parseCsv, compareCells } from '../../lib/csv.js';

  let { text, separator } = $props();

  const MAX_ROWS = 5000;

  const parsed = $derived(parseCsv(text, separator, MAX_ROWS + 1));
  const header = $derived(parsed.rows[0] ?? []);
  const body = $derived(parsed.rows.slice(1, MAX_ROWS + 1));
  const truncated = $derived(parsed.truncated || parsed.rows.length > MAX_ROWS + 1);
  const width = $derived(Math.max(header.length, ...body.slice(0, 200).map((r) => r.length)));

  let sortCol = $state(null);
  let sortDir = $state(1);

  const rows = $derived.by(() => {
    const list = body.map((cells, i) => ({ cells, n: i + 1 }));
    if (sortCol === null) return list;
    const c = sortCol;
    return list.sort((a, b) => {
      const x = a.cells[c] ?? '';
      const y = b.cells[c] ?? '';
      // Empty cells stay at the bottom either way round.
      if (!x !== !y) return x ? -1 : 1;
      return sortDir * compareCells(x, y) || a.n - b.n;
    });
  });

  function sortBy(i) {
    if (sortCol !== i) {
      sortCol = i;
      sortDir = 1;
    } else if (sortDir === 1) sortDir = -1;
    else sortCol = null;
  }
</script>

<div class="flex h-full flex-col">
  {#if !parsed.rows.length}
    <p class="grid flex-1 place-items-center text-[13px] text-fg-muted">This file is empty.</p>
  {:else}
    <div class="min-h-0 flex-1 overflow-auto">
      <table class="min-w-full border-separate border-spacing-0 text-[13px] [&_td]:border-b [&_td]:border-line [&_td]:px-3 [&_td]:py-1.5 [&_td]:whitespace-nowrap [&_td+td]:border-l [&_th]:sticky [&_th]:top-0 [&_th]:z-[1] [&_th]:border-b [&_th]:border-line [&_th]:bg-subtle [&_th]:px-3 [&_th]:py-1.5 [&_th]:text-left [&_th]:font-medium [&_th]:whitespace-nowrap [&_th]:text-fg-muted [&_th+th]:border-l [&_tbody_tr:hover]:bg-subtle">
        <thead>
          <tr>
            <th class="w-px text-right text-fg-faint"><span class="sr-only">Row</span></th>
            {#each { length: width } as _, i (i)}
              <th aria-sort={sortCol === i ? (sortDir === 1 ? 'ascending' : 'descending') : 'none'}>
                <button type="button" class="inline-flex max-w-80 cursor-pointer items-center gap-1 text-left hover:text-fg {sortCol === i ? 'text-fg' : ''}" onclick={() => sortBy(i)}>
                  <span class="truncate">{header[i] ?? ''}</span>
                  {#if sortCol === i}<Icon name={sortDir === 1 ? 'arrow-up' : 'arrow-down'} class="size-3 shrink-0" />{/if}
                </button>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each rows as row (row.n)}
            <tr>
              <td class="text-right text-fg-faint tabular-nums select-none">{row.n}</td>
              {#each { length: width } as _, i (i)}
                <td class="max-w-96 truncate" title={row.cells[i]?.length > 40 ? row.cells[i] : undefined}>{row.cells[i] ?? ''}</td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="shrink-0 border-t border-line px-4 py-2 text-xs text-fg-faint">
      {body.length.toLocaleString()} {body.length === 1 ? 'row' : 'rows'}{truncated ? ` shown, the file has more. Use Source to see all of it.` : ''}
    </p>
  {/if}
</div>

