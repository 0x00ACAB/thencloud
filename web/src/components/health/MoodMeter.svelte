<script>
  // Pick a feeling on the mood meter: pleasantness left to right, energy
  // bottom to top, a word per cell. Arrow keys move between cells. Each cell
  // counts how often it was picked among `recent` moods.
  import { WORDS, SIZE, cellPoint, pointCell, quadrant, strength } from '../../lib/mood.js';

  let { value = $bindable(null), recent = [] } = $props();

  const cells = WORDS.flatMap((row, r) =>
    row.map((word, c) => {
      const { x, y } = cellPoint(r, c);
      return { r, c, word, x, y, q: quadrant(x, y), s: strength(x, y) };
    }),
  );
  const tint = (cell) => `color-mix(in oklab, var(--mood-${cell.q}) ${Math.round(14 + cell.s * 30)}%, var(--bg))`;

  let grid = $state();
  let focus = $state({ r: 2, c: 3 });

  function pick(cell) {
    value = { x: cell.x, y: cell.y, word: cell.word };
    focus = { r: cell.r, c: cell.c };
  }

  function onkeydown(e) {
    const d = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
    if (!d) return;
    e.preventDefault();
    const r = Math.max(0, Math.min(SIZE - 1, focus.r + d[0]));
    const c = Math.max(0, Math.min(SIZE - 1, focus.c + d[1]));
    focus = { r, c };
    grid.querySelector(`[data-cell="${r}-${c}"]`)?.focus();
  }

  const isPicked = (cell) => value && value.word === cell.word;
  const counts = $derived.by(() => {
    const n = {};
    for (const m of recent) {
      const { row, col } = pointCell(m.x, m.y);
      n[`${row}-${col}`] = (n[`${row}-${col}`] ?? 0) + 1;
    }
    return n;
  });
</script>

<div class="grid grid-cols-[auto_minmax(0,1fr)] grid-rows-[1fr_auto] gap-2">
  <div class="flex rotate-180 items-center justify-between py-1 text-[11px] text-fg-muted [writing-mode:vertical-rl]">
    <span>Less energy</span><span>More energy</span>
  </div>
  <div class="relative">
    <div bind:this={grid} class="grid grid-cols-6 gap-1" role="grid" aria-label="Mood meter: pleasantness across, energy up" tabindex="-1" {onkeydown}>
      {#each WORDS as row, r (r)}
        <div class="contents" role="row">
          {#each cells.slice(r * SIZE, r * SIZE + SIZE) as cell (cell.word)}
            <button
              type="button"
              role="gridcell"
              data-cell="{cell.r}-{cell.c}"
              tabindex={focus.r === cell.r && focus.c === cell.c ? 0 : -1}
              aria-selected={isPicked(cell)}
              class="relative grid aspect-[5/3] min-w-0 cursor-pointer place-items-center rounded-md px-0.5 text-center text-[10px] leading-tight font-medium sm:px-1 transition-shadow hover:ring-1 hover:ring-line-strong min-[480px]:text-[11px] sm:text-xs {isPicked(cell) ? 'ring-2 ring-fg' : ''}"
              style:background-color={tint(cell)}
              onclick={() => pick(cell)}>
              <span class="break-words hyphens-auto">{cell.word}</span>
              {#if counts[`${cell.r}-${cell.c}`]}<span class="absolute top-0.5 right-1 text-[10px] font-normal tabular-nums opacity-70" aria-label="picked {counts[`${cell.r}-${cell.c}`]} times lately">{counts[`${cell.r}-${cell.c}`]}</span>{/if}
            </button>
          {/each}
        </div>
      {/each}
    </div>
  </div>
  <span></span>
  <div class="flex justify-between text-[11px] text-fg-muted"><span>Unpleasant</span><span>Pleasant</span></div>
</div>
