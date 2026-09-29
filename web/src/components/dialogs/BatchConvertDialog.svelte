<script>
  // Convert several files to one format, one after another, all in this
  // browser. Images can also be scaled down to a longest side.
  import { onMount, untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import { t } from '../../lib/i18n.svelte.js';
  import Icon from '../Icon.svelte';
  import { targetsFor, convert, convertedName, sourceKind, MAX_IMAGE, MAX_MEDIA } from '../../lib/convert.js';
  import { saveBlob } from '../../lib/crypto.js';
  import { extension } from '../../lib/preview.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  /** entries: files that sourceKind() accepts. fetch/save as for ConvertDialog. */
  let { entries, fetch, save = null, onclose } = $props();

  const kinds = untrack(() => new Set(entries.map((e) => sourceKind(e.meta))));
  const onlyImages = kinds.size === 1 && kinds.has('image');

  let targets = $state(null);
  let target = $state(null);
  let quality = $state(90);
  let maxSide = $state('');
  let destination = $state(untrack(() => (save ? 'save' : 'download')));
  let phase = $state('choose'); // choose | working | done
  let stage = $state('');
  let progress = $state(0);
  let current = $state({ index: 0, name: '' });
  let results = $state([]); // { name, ok, note }
  let controller = null;

  // Formats every selected file can become.
  onMount(async () => {
    const lists = await Promise.all([...kinds].map((k) => targetsFor(entries.find((e) => sourceKind(e.meta) === k).meta)));
    const flat = lists.map((groups) => groups.flatMap((g) => g.targets));
    targets = flat[0].filter((opt) => flat.every((l) => l.some((x) => x.id === opt.id))).map((opt) => ({ ...opt, same: false }));
    target = targets[0] ?? null;
  });

  const side = $derived(maxSide.trim() ? Math.round(Number(maxSide)) : null);
  const sideError = $derived(side !== null && !(side >= 16 && side <= 16384) ? t('Use a number of pixels between 16 and 16384.') : '');

  const stageText = $derived({ decrypting: t('Decrypting'), loading: t('Loading the converter'), converting: t('Converting'), saving: t('Saving') });

  async function imageSize(blob) {
    const b = await createImageBitmap(blob);
    const size = { w: b.width, h: b.height };
    b.close();
    return size;
  }

  async function run() {
    if (phase === 'done') return onclose();
    if (!target || sideError) return;
    controller = new AbortController();
    const { signal } = controller;
    phase = 'working';
    results = [];
    for (const [i, entry] of entries.entries()) {
      if (signal.aborted) break;
      current = { index: i + 1, name: entry.meta.name };
      const kind = sourceKind(entry.meta);
      const same = extension(entry.meta.name) === target.ext || (target.id === 'jpeg' && extension(entry.meta.name) === 'jpeg');
      try {
        if (entry.meta.size > (kind === 'image' ? MAX_IMAGE : MAX_MEDIA)) throw new Error(t('Too large to convert in the browser'));
        stage = 'decrypting';
        progress = 0;
        const { blob } = await fetch(entry, (p) => (progress = p));
        let size;
        if (kind === 'image' && side) {
          const s = await imageSize(blob);
          const f = Math.min(1, side / Math.max(s.w, s.h));
          if (f < 1) size = { w: Math.max(1, Math.round(s.w * f)), h: Math.max(1, Math.round(s.h * f)) };
        }
        if (same && !size) {
          results.push({ name: entry.meta.name, ok: true, note: t('Already {format}', { format: target.label }) });
          continue;
        }
        stage = 'converting';
        progress = 0;
        const out = await convert(blob, entry.meta, target, {
          quality: quality / 100,
          size,
          signal,
          onStage: (s) => ((stage = s), (progress = 0)),
          onProgress: (p) => (progress = p),
        });
        const name = convertedName(entry.meta.name, target);
        if (destination === 'save') {
          stage = 'saving';
          progress = 0;
          await save(new File([out], name, { type: target.type, lastModified: Date.now() }), (p) => (progress = p));
        } else {
          saveBlob(out, name);
        }
        results.push({ name, ok: true });
      } catch (e) {
        if (e?.name === 'AbortError') break;
        results.push({ name: entry.meta.name, ok: false, note: errorMessage(e) });
      }
    }
    controller = null;
    phase = 'done';
  }

  const failed = $derived(results.filter((r) => !r.ok));
</script>

<Modal
  title={t('Convert {count} files', { count: entries.length })}
  description={t('One after another, in this browser.')}
  onclose={() => (controller?.abort(), onclose())}
  onsubmit={run}
  class="max-w-lg">
  {#if targets === null}
    <div class="skeleton h-24" aria-hidden="true"></div>
  {:else if !targets.length}
    <p class="text-[13px] text-fg-muted">{t('These files have no format in common to convert to. Select only images, or only video and audio.')}</p>
  {:else if phase === 'choose'}
    <fieldset class="grid gap-2">
      <legend class="mb-2 text-xs font-medium text-fg-muted">{t('To')}</legend>
      <div class="grid grid-cols-3 gap-2 sm:grid-cols-6">
        {#each targets as opt (opt.id)}
          <label
            class="flex h-10 cursor-pointer items-center justify-center rounded-md border text-sm transition-colors has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring {target === opt
              ? 'border-accent bg-accent-soft font-medium text-accent-text'
              : 'border-line hover:bg-subtle'}">
            <input type="radio" class="sr-only" name="batch-target" value={opt.id} checked={target === opt} onchange={() => (target = opt)} />
            {opt.label}
          </label>
        {/each}
      </div>
    </fieldset>
    {#if target?.quality}
      <div class="field">
        <label class="label flex justify-between" for="b-quality"><span>{t('Quality')}</span><span class="text-fg-muted tabular-nums">{quality}</span></label>
        <input id="b-quality" type="range" min="30" max="100" step="1" bind:value={quality} class="w-full accent-accent" />
      </div>
    {/if}
    {#if onlyImages}
      <div class="field">
        <label class="label" for="b-side">{t('Longest side at most')} <span class="font-normal text-fg-muted">{t('(optional, pixels)')}</span></label>
        <input id="b-side" class="input h-8 w-32 tabular-nums" inputmode="numeric" placeholder="as is" bind:value={maxSide} />
        {#if sideError}<p class="text-[13px] text-danger">{sideError}</p>{/if}
      </div>
    {/if}
    {#if save}
      <div class="grid gap-2">
        <p class="text-xs font-medium text-fg-muted">{t('Then')}</p>
        <div class="flex flex-wrap gap-4 text-sm">
          <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={destination} value="save" />{t('Save them in this folder')}</label>
          <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={destination} value="download" />{t('Download them')}</label>
        </div>
      </div>
    {/if}
  {:else if phase === 'working'}
    <div class="grid gap-3 py-2" role="status">
      <p class="truncate text-xs text-fg-muted">{t('File {n} of {total}: {name}', { n: current.index, total: entries.length, name: current.name })}</p>
      <p class="flex items-center gap-2 text-sm"><Icon name="loader-circle" class="spinner" />{stageText[stage] ?? t('Working')}</p>
      <div class="progress"><div style:width="{Math.round(progress * 100)}%"></div></div>
    </div>
  {:else}
    <p class="flex items-center gap-2 py-1 text-sm font-medium">
      <Icon name="check" class="size-4 text-success" />
      {results.length < entries.length ? t('{done} of {total} done before you stopped', { done: results.length - failed.length, total: entries.length }) : t('{done} of {total} done', { done: results.length - failed.length, total: entries.length })}
    </p>
    {#if results.some((r) => r.note)}
      <ul class="max-h-48 divide-y divide-line overflow-y-auto rounded-md border border-line text-[13px]">
        {#each results.filter((r) => r.note) as r, i (i)}
          <li class="grid gap-0.5 px-3 py-2">
            <span class="truncate font-medium">{r.name}</span>
            <span class="text-xs {r.ok ? 'text-fg-muted' : 'text-danger'}">{r.note}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}

  {#snippet footer()}
    {#if phase === 'done'}
      <button class="btn btn-primary">{t('Done')}</button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={() => (controller ? controller.abort() : onclose())}>{phase === 'working' ? t('Stop') : t('Cancel')}</button>
      <button class="btn btn-primary" disabled={!target || !!sideError || phase === 'working'}>
        {#if phase === 'working'}<Icon name="loader-circle" class="spinner" />{/if}
        {target ? t('Convert to {format}', { format: target.label }) : t('Convert')}
      </button>
    {/if}
  {/snippet}
</Modal>
