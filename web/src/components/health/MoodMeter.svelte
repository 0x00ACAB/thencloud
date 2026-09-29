<script>
  // Pick a feeling on the mood meter: pleasantness left to right, energy
  // bottom to top, a word per cell. Arrow keys move between cells. Each cell
  // counts how often it was picked among `recent` moods.
  import { WORDS, SIZE, cellPoint, pointCell, quadrant, strength } from '../../lib/mood.js';
  import { moodWord } from '../../lib/moodword.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';

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

  // A font size that fits the longest word of `text` in the cell: about
  // 0.58em per letter, at most the cell's normal size.
  const fit = (text) => {
    const longest = Math.max(...text.split(' ').map((w) => w.length));
    return longest > 8 ? `min(1em, calc(100cqw / ${(longest * 0.58).toFixed(2)}))` : null;
  };

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
    <span>{t('Less energy')}</span><span>{t('More energy')}</span>
  </div>
  <div class="relative">
    <div bind:this={grid} class="grid grid-cols-6 gap-1" role="grid" aria-label={t('Mood meter: pleasantness across, energy up')} tabindex="-1" {onkeydown}>
      {#each WORDS as row, r (r)}
        <div class="contents" role="row">
          {#each cells.slice(r * SIZE, r * SIZE + SIZE) as cell (cell.word)}
            <button
              type="button"
              role="gridcell"
              data-cell="{cell.r}-{cell.c}"
              tabindex={focus.r === cell.r && focus.c === cell.c ? 0 : -1}
              aria-selected={isPicked(cell)}
              class="@container relative grid aspect-[5/3] min-w-0 cursor-pointer place-items-center rounded-md px-0.5 text-center text-[10px] leading-tight font-medium sm:px-1 transition-shadow hover:ring-1 hover:ring-line-strong min-[480px]:text-[11px] sm:text-xs {isPicked(cell) ? 'ring-2 ring-fg' : ''}"
              style:background-color={tint(cell)}
              onclick={() => pick(cell)}>
              <!-- Long words (some languages have many) shrink to fit the cell rather than break. -->
              <span class="max-w-full" style:font-size={fit(moodWord(cell.word))}>{moodWord(cell.word)}</span>
              {#if counts[`${cell.r}-${cell.c}`]}<span class="absolute top-0.5 right-1 text-[10px] font-normal tabular-nums opacity-70" aria-label={t('picked {count} times lately', { count: counts[`${cell.r}-${cell.c}`] })}>{counts[`${cell.r}-${cell.c}`]}</span>{/if}
            </button>
          {/each}
        </div>
      {/each}
    </div>
  </div>
  <span></span>
  <div class="flex justify-between text-[11px] text-fg-muted"><span>{t('Less pleasant')}</span><span>{t('More pleasant')}</span></div>
</div>
