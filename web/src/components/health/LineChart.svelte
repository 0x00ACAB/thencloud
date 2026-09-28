<script>
  // A small time chart, drawn by hand in SVG. `series` is a list of
  // { label, color, points: [{ t, v }] } (colour as a CSS value, points
  // sorted by t); series that share times (blood pressure's two values, the
  // mood's two axes) show together on hover. The entries list next to it is
  // the text version, so this is decoration for screen readers.
  let { series, from, to, format = (v) => String(v), tipFormat = format, height = 220, domain = null, label = 'Chart' } = $props();

  let width = $state(600);
  let hover = $state(null); // index into series[0].points

  const PAD = { top: 12, right: 12, bottom: 26, left: 44 };
  const inRange = $derived(series.map((s) => ({ ...s, points: s.points.filter((p) => p.t >= from && p.t <= to) })));
  const all = $derived(inRange.flatMap((s) => s.points));

  import { formatDateTime } from '../../lib/locale.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';

  // "Nice" ticks: 1, 2, 2.5 or 5 times a power of ten.
  function ticks(lo, hi, n = 4) {
    if (hi - lo < 1e-9) [lo, hi] = [lo - 1, hi + 1];
    const raw = (hi - lo) / n;
    const pow = 10 ** Math.floor(Math.log10(raw));
    const step = [1, 2, 2.5, 5, 10].map((m) => m * pow).find((s) => s >= raw);
    const out = [];
    // From the step at or below the lowest value to the one at or above the highest.
    for (let i = Math.floor(lo / step + 1e-9); ; i++) {
      out.push(Math.round(i * step * 1e6) / 1e6);
      if (i * step >= hi - 1e-9) break;
    }
    return out;
  }

  const yTicks = $derived.by(() => {
    if (domain) return ticks(domain[0], domain[1]);
    const vs = all.map((p) => p.v);
    return vs.length ? ticks(Math.min(...vs), Math.max(...vs)) : [];
  });
  const yLo = $derived(domain ? domain[0] : yTicks[0]);
  const yHi = $derived(domain ? domain[1] : yTicks[yTicks.length - 1]);
  const plotW = $derived(Math.max(10, width - PAD.left - PAD.right));
  const plotH = $derived(height - PAD.top - PAD.bottom);
  const x = (t) => PAD.left + ((t - from) / Math.max(1, to - from)) * plotW;
  const y = (v) => PAD.top + (1 - (v - yLo) / Math.max(1e-9, yHi - yLo)) * plotH;

  const xTicks = $derived.by(() => {
    const span = to - from;
    const n = Math.max(2, Math.min(6, Math.floor(plotW / 110)));
    const opts = span > 400 * 86400e3 ? { month: 'short', year: 'numeric' } : { month: 'short', day: 'numeric' };
    return Array.from({ length: n }, (_, i) => {
      const t = from + (span * i) / (n - 1);
      return { t, text: formatDateTime(t, opts) };
    });
  });

  const path = (pts) => pts.map((p, i) => `${i ? 'L' : 'M'}${x(p.t).toFixed(1)} ${y(p.v).toFixed(1)}`).join('');

  function onmove(e) {
    const pts = inRange[0]?.points;
    if (!pts?.length) return;
    const r = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - r.left;
    let best = 0;
    for (let i = 1; i < pts.length; i++) if (Math.abs(x(pts[i].t) - px) < Math.abs(x(pts[best].t) - px)) best = i;
    hover = best;
  }

  const tip = $derived(hover === null ? null : inRange[0]?.points[hover]);
</script>

<div class="relative min-w-0" bind:clientWidth={width}>
  {#if !all.length}
    <div class="grid place-items-center rounded-md border border-dashed border-line text-[13px] text-fg-muted" style:height="{height}px">{t('Nothing logged in this period.')}</div>
  {:else}
    <svg width="100%" {height} role="img" aria-label={label} class="block touch-none select-none" onpointermove={onmove} onpointerleave={() => (hover = null)}>
      {#each yTicks as v (v)}
        <line x1={PAD.left} x2={PAD.left + plotW} y1={y(v)} y2={y(v)} class="stroke-line" stroke-width="1" />
        <text x={PAD.left - 8} y={y(v)} text-anchor="end" dominant-baseline="middle" class="fill-fg-muted text-[11px] tabular-nums">{format(v)}</text>
      {/each}
      {#each xTicks as tk, i (i)}
        <text x={x(tk.t)} y={height - 6} text-anchor={i === 0 ? 'start' : i === xTicks.length - 1 ? 'end' : 'middle'} class="fill-fg-muted text-[11px]">{tk.text}</text>
      {/each}
      {#each inRange as s, si (si)}
        {#if s.points.length > 1}
          <path d={path(s.points)} fill="none" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" style:stroke={s.color} />
        {/if}
        {#if s.points.length <= 90}
          {#each s.points as p, i (i)}
            <circle cx={x(p.t)} cy={y(p.v)} r={hover === i ? 4 : 2.5} class="stroke-bg" stroke-width="1.5" style:fill={s.color} />
          {/each}
        {/if}
      {/each}
      {#if tip}
        <line x1={x(tip.t)} x2={x(tip.t)} y1={PAD.top} y2={PAD.top + plotH} class="stroke-line-strong" stroke-width="1" stroke-dasharray="3 3" />
      {/if}
    </svg>
    {#if tip}
      <div
        class="pointer-events-none absolute top-1 z-10 rounded-md border border-line bg-bg px-2.5 py-1.5 text-xs shadow-sm"
        style:left="{x(tip.t) + 170 < width ? x(tip.t) + 10 : Math.max(0, x(tip.t) - 170)}px">
        <p class="text-fg-muted">{formatDateTime(tip.t)}</p>
        {#each inRange as s, si (si)}
          {#if s.points[hover]}
            <p class="flex items-center gap-1.5 font-medium tabular-nums">
              <span class="size-2 rounded-full" style:background-color={s.color}></span>{#if inRange.length > 1}<span class="font-normal text-fg-muted">{s.label}</span>{/if}
              {tipFormat(s.points[hover].v)}
            </p>
          {/if}
        {/each}
      </div>
    {/if}
  {/if}
</div>
