<script>
  // The Health module (optional, turned on in Settings): measurements with
  // charts, and a mood meter. Everything is in the encrypted "health" app
  // data and decrypted here; the server keeps one opaque blob.
  import { onMount } from 'svelte';
  import { health, loadHealth, KINDS, addMeasure, removeMeasure, addMood, removeMood, putBack, unitOf, toDisplay, formatMeasure, bmi, bmiBand } from '../../lib/health.svelte.js';
  import { quadrant } from '../../lib/mood.js';
  import { moodWord } from '../../lib/moodword.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';
  import { toast, toastError } from '../../lib/ui.svelte.js';
  import Icon from '../Icon.svelte';
  import { formatDateTime, formatNumber } from '../../lib/locale.svelte.js';
  import LineChart from '../health/LineChart.svelte';
  import MoodMeter from '../health/MoodMeter.svelte';

  let loadError = $state(null);
  onMount(() => loadHealth().catch((e) => (loadError = e?.message ?? String(e))));

  let tab = $state('measures'); // measures | mood
  let range = $state('3m');
  const RANGES = $derived([
    ['1m', '1M', 30],
    ['3m', '3M', 91],
    ['1y', '1Y', 365],
    ['all', t('All'), null],
  ]);
  // Kept current, so the chart's right edge moves on while the page is open.
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(t);
  });
  function span(list) {
    const days = RANGES.find((r) => r[0] === range)[2];
    if (days) return { from: now - days * 86400e3, to: now };
    const first = list[0]?.at ?? now - 30 * 86400e3;
    return { from: Math.min(first, now - 86400e3), to: now };
  }

  // "2026-09-28T21:30" in local time, for datetime-local inputs.
  const localInput = (ms) => {
    const d = new Date(ms);
    d.setMinutes(d.getMinutes() - d.getTimezoneOffset());
    return d.toISOString().slice(0, 16);
  };
  const when = (ms) => formatDateTime(ms);
  const round1 = (v) => formatNumber(Math.round(v * 10) / 10, { maximumFractionDigits: 1 });

  // ------------------------------------------------------- measurements

  let kind = $state('weight');
  const k = $derived(KINDS[kind]);
  const ofKind = (id) => health.measures.filter((m) => m.kind === id);
  const logged = $derived(Object.keys(KINDS).filter((id) => health.measures.some((m) => m.kind === id)));
  const latest = (id) => ofKind(id).at(-1);
  const list = $derived(ofKind(kind));

  const latestBmi = $derived.by(() => {
    const w = latest('weight');
    const h = latest('height');
    const v = w && h ? bmi(w.value, h.value) : null;
    return v ? { value: v, band: bmiBand(v) } : null;
  });

  /** The change since the entry before, in display units. */
  function trend(id) {
    const l = ofKind(id);
    if (l.length < 2 || KINDS[id].pair) return null;
    const d = toDisplay(id, l.at(-1).value) - toDisplay(id, l.at(-2).value);
    return Math.abs(d) < 0.05 ? null : d;
  }

  const series = $derived.by(() => {
    if (k.pair)
      return [
        { label: k.pair[0], color: 'var(--mood-red)', points: list.map((m) => ({ t: m.at, v: m.value })) },
        { label: k.pair[1], color: 'var(--mood-blue)', points: list.map((m) => ({ t: m.at, v: m.value2 })) },
      ];
    return [{ label: k.label, color: 'var(--accent)', points: list.map((m) => ({ t: m.at, v: toDisplay(kind, m.value) })) }];
  });

  let form = $state({ value: '', value2: '', at: localInput(Date.now()), note: '' });
  let adding = $state(false);
  const valid = (v) => v !== '' && v !== null && Number.isFinite(+v);

  async function add(e) {
    e.preventDefault();
    if (!valid(form.value) || (k.pair && !valid(form.value2))) return;
    adding = true;
    try {
      await addMeasure({ kind, at: new Date(form.at).getTime() || Date.now(), value: +form.value, value2: k.pair ? +form.value2 : null, note: form.note });
      form = { value: '', value2: '', at: localInput(Date.now()), note: '' };
      toast(`${k.short ?? k.label} logged`, { kind: 'success' });
    } catch (err) {
      toastError(err);
    } finally {
      adding = false;
    }
  }

  async function drop(listName, item, remove) {
    try {
      await remove(item.id);
      toast(t('Entry deleted'), { action: { label: t('Undo'), onclick: () => putBack(listName, item).catch(toastError) } });
    } catch (err) {
      toastError(err);
    }
  }

  // --------------------------------------------------------------- mood

  let mood = $state(null); // { x, y, word }
  let moodNote = $state('');
  let moodAt = $state(localInput(Date.now()));
  let logging = $state(false);
  const recentMoods = $derived(health.moods.filter((m) => m.at > now - 14 * 86400e3));

  async function logMood(e) {
    e.preventDefault();
    if (!mood) return;
    logging = true;
    try {
      await addMood({ ...mood, at: new Date(moodAt).getTime() || Date.now(), note: moodNote });
      toast(t('Logged: {word}', { word: moodWord(mood.word) }), { kind: 'success' });
      mood = null;
      moodNote = '';
      moodAt = localInput(Date.now());
    } catch (err) {
      toastError(err);
    } finally {
      logging = false;
    }
  }

  const moodSpan = $derived(span(health.moods));
  const moodSeries = $derived([
    { label: t('Pleasantness'), color: 'var(--mood-green)', points: health.moods.map((m) => ({ t: m.at, v: m.x })) },
    { label: t('Energy'), color: 'var(--mood-yellow)', points: health.moods.map((m) => ({ t: m.at, v: m.y })) },
  ]);
  const moodTick = (v) => (v === 1 ? t('High') : v === -1 ? t('Low') : v === 0 ? '0' : '');
  const moodTip = (v) => `${v > 0 ? '+' : ''}${v.toFixed(2)}`;

  // Share of each corner in the range.
  const QUADS = $derived([
    ['yellow', t('Pleasant, lots of energy')],
    ['green', t('Pleasant, calm')],
    ['blue', t('Unpleasant, low energy')],
    ['red', t('Unpleasant, lots of energy')],
  ]);
  const moodMix = $derived.by(() => {
    const inRange = health.moods.filter((m) => m.at >= moodSpan.from);
    const n = { red: 0, yellow: 0, blue: 0, green: 0 };
    for (const m of inRange) n[quadrant(m.x, m.y)]++;
    return { n, total: inRange.length };
  });
</script>

<div class="flex flex-wrap items-end justify-between gap-3">
  <div>
    <h1 class="text-xl font-semibold tracking-tight">{t('Health')}</h1>
    <p class="mt-1 text-[13px] text-fg-muted">{t("Encrypted in this browser like your files. The server stores it but can't read any of it.")}</p>
  </div>
  <div class="flex items-center gap-2">
    <div class="flex rounded-md border border-line p-0.5" role="tablist" aria-label={t('Health sections')}>
      {#each [['measures', t('Measurements')], ['mood', t('Mood')]] as [value, label] (value)}
        <button type="button" role="tab" aria-selected={tab === value} class="h-7 cursor-pointer rounded px-3 text-[13px] {tab === value ? 'bg-muted font-medium text-fg' : 'text-fg-muted hover:text-fg'}" onclick={() => (tab = value)}>{label}</button>
      {/each}
    </div>
  </div>
</div>

{#snippet rangePicker()}
  <div class="flex rounded-md border border-line p-0.5" role="radiogroup" aria-label={t('Period')}>
    {#each RANGES as [value, label] (value)}
      <button type="button" role="radio" aria-checked={range === value} class="h-6 cursor-pointer rounded px-2 text-xs tabular-nums {range === value ? 'bg-muted font-medium text-fg' : 'text-fg-muted hover:text-fg'}" onclick={() => (range = value)}>{label}</button>
    {/each}
  </div>
{/snippet}

{#if loadError}
  <p class="mt-6 flex items-center gap-2 text-[13px] text-danger"><Icon name="circle-alert" class="size-4" />{loadError}</p>
{:else if !health.loaded}
  <div class="mt-6 grid gap-3" aria-hidden="true">
    <div class="skeleton h-20 w-full"></div>
    <div class="skeleton h-56 w-full"></div>
  </div>
{:else if tab === 'measures'}
  <!-- Latest of each, and BMI once there's a height and a weight. -->
  {#if logged.length}
    <div class="mt-6 grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-4">
      {#each logged as id (id)}
        {@const m = latest(id)}
        {@const d = trend(id)}
        <button
          type="button"
          class="grid cursor-pointer gap-1 rounded-lg border p-3 text-left transition-colors {kind === id ? 'border-accent bg-accent-soft' : 'border-line hover:bg-subtle'}"
          aria-pressed={kind === id}
          onclick={() => (kind = id)}>
          <span class="flex items-center gap-1.5 text-xs text-fg-muted"><Icon name={KINDS[id].icon} class="size-3.5" />{KINDS[id].short ?? KINDS[id].label}</span>
          <span class="text-lg font-semibold tracking-tight tabular-nums">{formatMeasure(m)}</span>
          <span class="text-xs text-fg-muted tabular-nums">{d === null ? when(m.at) : t('{change} since last', { change: `${d > 0 ? '+' : ''}${round1(d)} ${unitOf(id)}` })}</span>
        </button>
      {/each}
      {#if latestBmi}
        <div class="grid gap-1 rounded-lg border border-line p-3">
          <span class="flex items-center gap-1.5 text-xs text-fg-muted"><Icon name="activity" class="size-3.5" />BMI</span>
          <span class="text-lg font-semibold tracking-tight tabular-nums">{round1(latestBmi.value)}</span>
          <span class="text-xs text-fg-muted">{latestBmi.band}</span>
        </div>
      {/if}
    </div>
  {/if}

  <section class="mt-6 rounded-lg border border-line">
    <div class="flex flex-wrap items-center justify-between gap-3 border-b border-line px-4 py-3">
      <label class="flex items-center gap-2 text-sm font-medium">
        <span class="sr-only">{t('Measurement')}</span>
        <select class="input h-8 w-auto text-[13px]" bind:value={kind}>
          {#each Object.entries(KINDS) as [id, def] (id)}<option value={id}>{def.label}</option>{/each}
        </select>
      </label>
      {@render rangePicker()}
    </div>
    <div class="px-2 py-3 sm:px-4">
      {#key kind}
        {@const s = span(list)}
        <LineChart {series} from={s.from} to={s.to} label={t('{name} over time', { name: k.label })} format={(v) => round1(v)} tipFormat={(v) => `${round1(v)} ${unitOf(kind)}`} />
      {/key}
    </div>

    <form class="grid gap-3 border-t border-line bg-subtle px-4 py-4 sm:grid-cols-[auto_auto_1fr_auto] sm:items-end" onsubmit={add}>
      <div class="grid gap-1.5">
        <label class="label" for="h-value">{k.pair ? `${k.pair[0]} / ${k.pair[1]}` : k.label} ({unitOf(kind)})</label>
        <div class="flex items-center gap-1.5">
          <input id="h-value" class="input h-9 w-24 tabular-nums" type="number" inputmode="decimal" step={k.step} min={k.min} max={k.dim ? undefined : k.max} bind:value={form.value} required />
          {#if k.pair}
            <span class="text-fg-muted">/</span>
            <input class="input h-9 w-24 tabular-nums" type="number" inputmode="decimal" aria-label={k.pair[1]} step={k.step} min={k.min} max={k.max} bind:value={form.value2} required />
          {/if}
        </div>
      </div>
      <div class="grid gap-1.5">
        <label class="label" for="h-at">{t('When')}</label>
        <input id="h-at" class="input h-9" type="datetime-local" bind:value={form.at} max={localInput(now + 60_000)} />
      </div>
      <div class="grid gap-1.5">
        <label class="label" for="h-note">{t('Note')} <span class="font-normal text-fg-muted">{t('(optional)')}</span></label>
        <input id="h-note" class="input h-9" bind:value={form.note} maxlength="500" placeholder={t('After a run, before breakfast...')} />
      </div>
      <button class="btn btn-primary h-9" disabled={adding}>
        {#if adding}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="plus" />{/if}
        {t('Log')}
      </button>
    </form>

    {#if list.length}
      <ul class="divide-y divide-line border-t border-line">
        {#each list.toReversed().slice(0, 50) as m (m.id)}
          <li class="flex items-center gap-3 px-4 py-2 text-sm">
            <span class="w-28 shrink-0 font-medium tabular-nums">{formatMeasure(m)}</span>
            <span class="min-w-0 flex-1 truncate text-fg-muted">{when(m.at)}{#if m.note}<span class="text-fg"> · {m.note}</span>{/if}</span>
            <button type="button" class="btn btn-ghost btn-icon size-7" aria-label={t('Delete this entry')} onclick={() => drop('measures', m, removeMeasure)}><Icon name="trash-2" class="size-3.5" /></button>
          </li>
        {/each}
      </ul>
      {#if list.length > 50}<p class="border-t border-line px-4 py-2 text-xs text-fg-muted">{t('The 50 most recent of {count}. The chart shows them all.', { count: list.length })}</p>{/if}
    {/if}
  </section>
{:else}
  <div class="mt-6 grid grid-cols-[minmax(0,1fr)] gap-6 lg:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)]">
    <section class="rounded-lg border border-line p-3 sm:p-4">
      <h2 class="text-sm font-medium">{t('How are you feeling?')}</h2>
      <p class="mt-1 text-xs text-fg-muted">{t('Pick the word closest to it: how pleasant it is across, how much energy up and down.')}</p>
      <div class="mt-4"><MoodMeter bind:value={mood} recent={recentMoods} /></div>
      <form class="mt-4 grid gap-3" onsubmit={logMood}>
        <div class="grid gap-1.5">
          <label class="label" for="m-note">{t("What's behind it?")} <span class="font-normal text-fg-muted">{t('(optional)')}</span></label>
          <textarea id="m-note" class="input min-h-16 py-2" bind:value={moodNote} maxlength="1000"></textarea>
        </div>
        <div class="flex flex-wrap items-end justify-between gap-3">
          <div class="grid gap-1.5">
            <label class="label" for="m-at">{t('When')}</label>
            <input id="m-at" class="input h-9" type="datetime-local" bind:value={moodAt} max={localInput(now + 60_000)} />
          </div>
          <button class="btn btn-primary h-9" disabled={!mood || logging}>
            {#if logging}<Icon name="loader-circle" class="spinner" />{/if}
            {mood ? t('Log "{word}"', { word: moodWord(mood.word) }) : t('Pick a word first')}
          </button>
        </div>
      </form>
    </section>

    <section class="grid content-start gap-4 rounded-lg border border-line p-3 sm:p-4">
      <div class="flex items-center justify-between gap-3">
        <h2 class="text-sm font-medium">{t('Over time')}</h2>
        {@render rangePicker()}
      </div>
      <LineChart series={moodSeries} from={moodSpan.from} to={moodSpan.to} domain={[-1, 1]} height={180} label={t('Mood over time')} format={moodTick} tipFormat={moodTip} />
      {#if moodMix.total}
        <div class="grid gap-2">
          <div class="flex h-2 overflow-hidden rounded-full bg-muted" aria-hidden="true">
            {#each QUADS as [q] (q)}
              {#if moodMix.n[q]}<span class="h-full" style:width="{(moodMix.n[q] / moodMix.total) * 100}%" style:background-color="var(--mood-{q})"></span>{/if}
            {/each}
          </div>
          <ul class="grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-fg-muted">
            {#each QUADS as [q, label] (q)}
              <li class="flex items-center gap-1.5"><span class="size-2 shrink-0 rounded-full" style:background-color="var(--mood-{q})"></span>{label}<span class="ml-auto tabular-nums">{moodMix.n[q]}</span></li>
            {/each}
          </ul>
        </div>
      {/if}
    </section>
  </div>

  {#if health.moods.length}
    <section class="mt-6 rounded-lg border border-line">
      <h2 class="border-b border-line px-4 py-3 text-sm font-medium">{t('Recent')}</h2>
      <ul class="divide-y divide-line">
        {#each health.moods.toReversed().slice(0, 30) as m (m.id)}
          <li class="flex items-start gap-3 px-4 py-2.5 text-sm">
            <span class="mt-1.5 size-2.5 shrink-0 rounded-full" style:background-color="var(--mood-{quadrant(m.x, m.y)})" aria-hidden="true"></span>
            <div class="min-w-0 flex-1">
              <p><span class="font-medium">{moodWord(m.word)}</span> <span class="text-xs text-fg-muted">{when(m.at)}</span></p>
              {#if m.note}<p class="mt-0.5 text-[13px] break-words whitespace-pre-wrap text-fg-muted">{m.note}</p>{/if}
            </div>
            <button type="button" class="btn btn-ghost btn-icon size-7" aria-label={t('Delete this entry')} onclick={() => drop('moods', m, removeMood)}><Icon name="trash-2" class="size-3.5" /></button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
{/if}
