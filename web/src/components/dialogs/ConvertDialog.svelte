<script>
  // "Convert": pick a format, optionally crop and resize (images) or trim
  // (video and audio), then download the result or save it next to the
  // original. Decrypting, converting and re-encrypting all happen in this
  // browser.
  import { onMount, onDestroy, untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import ImageCrop from '../ImageCrop.svelte';
  import { targetsFor, convert, convertedName, sourceKind, parseTime, MAX_IMAGE, MAX_MEDIA } from '../../lib/convert.js';
  import { saveBlob } from '../../lib/crypto.js';
  import { formatSize } from '../../lib/format.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  /** fetch(entry, onProgress) -> { blob }; save(file, onProgress) uploads it next to the original (null if you can't write here). */
  let { entry, fetch, save = null, onclose } = $props();

  const kind = untrack(() => sourceKind(entry.meta));
  const limit = kind === 'image' ? MAX_IMAGE : MAX_MEDIA;
  const tooBig = untrack(() => entry.meta.size > limit);

  let groups = $state(null);
  let target = $state(null);
  let quality = $state(90);
  let destination = $state(untrack(() => (save ? 'save' : 'download')));
  let phase = $state('choose'); // choose | working | done | error
  let stage = $state(''); // decrypting | loading | converting | saving
  let progress = $state(0);
  let error = $state('');
  let result = $state(null); // { name, size, saved }
  let controller = null;

  // Images: decrypted up front, to show the crop editor and real size.
  let image = $state(null); // { blob, url, width, height }
  let imageError = $state('');
  let crop = $state(null); // { x, y, w, h } in source pixels
  let aspectChoice = $state(null); // null (free), 'original' or width / height
  let outW = $state(0);
  let outH = $state(0);
  let lock = $state(true);

  // Video and audio: optional trim, as typed times.
  let trimFrom = $state('');
  let trimTo = $state('');

  onMount(async () => {
    groups = await targetsFor(entry.meta);
    target = groups[0]?.targets.find((t) => !t.same) ?? groups[0]?.targets[0] ?? null;
    if (kind === 'image' && !tooBig) {
      try {
        const { blob } = await fetch(entry, () => {});
        const bitmap = await createImageBitmap(blob);
        image = { blob, url: URL.createObjectURL(blob), width: bitmap.width, height: bitmap.height };
        bitmap.close();
        resetEdits();
      } catch {
        imageError = "This image couldn't be read, so it can't be converted here.";
      }
    }
  });

  onDestroy(() => image && URL.revokeObjectURL(image.url));

  function resetEdits() {
    aspectChoice = null;
    lastCrop = null;
    crop = { x: 0, y: 0, w: image.width, h: image.height };
    outW = image.width;
    outH = image.height;
  }

  const ASPECTS = [
    ['Free', null],
    ['Original', 'original'],
    ['1:1', 1],
    ['4:3', 4 / 3],
    ['3:2', 3 / 2],
    ['16:9', 16 / 9],
  ];
  const aspect = $derived(image && aspectChoice ? (aspectChoice === 'original' ? image.width / image.height : aspectChoice) : null);

  // Keep the output size in step with the crop, at the same scale as before.
  let lastCrop = null;
  $effect(() => {
    if (!crop) return;
    const c = crop;
    untrack(() => {
      if (lastCrop && (lastCrop.w !== c.w || lastCrop.h !== c.h)) {
        const scale = outW / lastCrop.w;
        outW = Math.max(1, Math.round(c.w * scale));
        outH = Math.max(1, Math.round(c.h * scale));
      }
      lastCrop = c;
    });
  });

  function setWidth(v) {
    outW = Math.max(1, Math.round(Number(v) || 1));
    if (lock && crop) outH = Math.max(1, Math.round((outW * crop.h) / crop.w));
  }
  function setHeight(v) {
    outH = Math.max(1, Math.round(Number(v) || 1));
    if (lock && crop) outW = Math.max(1, Math.round((outH * crop.w) / crop.h));
  }
  function scaleTo(f) {
    outW = Math.max(1, Math.round(crop.w * f));
    outH = Math.max(1, Math.round(crop.h * f));
  }

  const edited = $derived(
    !!image && !!crop && (crop.x !== 0 || crop.y !== 0 || crop.w !== image.width || crop.h !== image.height || outW !== crop.w || outH !== crop.h),
  );
  const MAX_SIDE = 16384;
  const sizeError = $derived(image && (outW > MAX_SIDE || outH > MAX_SIDE) ? `Keep each side under ${MAX_SIDE} pixels.` : '');

  const trim = $derived({ start: parseTime(trimFrom), end: parseTime(trimTo) });
  const trimError = $derived(
    Number.isNaN(trim.start) || Number.isNaN(trim.end)
      ? 'Use times like 1:05, 0:01:05 or 65.'
      : trim.start != null && trim.end != null && trim.end <= trim.start
        ? 'The end has to be after the start.'
        : '',
  );
  const trimmed = $derived(!trimError && (trim.start != null || trim.end != null));

  // Converting to the same format only makes sense with an edit.
  const pointless = $derived(!!target?.same && !(kind === 'image' ? edited : trimmed));
  const canRun = $derived(!!target && !tooBig && !pointless && !sizeError && !trimError && (kind !== 'image' || !!image) && phase !== 'working');

  const stageText = {
    decrypting: 'Decrypting',
    loading: 'Loading the converter (about 10 MB, only the first time)',
    converting: 'Converting',
    saving: 'Saving',
  };

  async function run() {
    if (phase === 'done') return onclose();
    if (!canRun) return;
    controller = new AbortController();
    const { signal } = controller;
    phase = 'working';
    error = '';
    try {
      let blob = image?.blob;
      if (!blob) {
        stage = 'decrypting';
        progress = 0;
        ({ blob } = await fetch(entry, (p) => (progress = p)));
        if (signal.aborted) throw new DOMException('Cancelled', 'AbortError');
      }
      progress = 0;
      stage = 'converting';
      const out = await convert(blob, entry.meta, target, {
        quality: quality / 100,
        crop: image ? $state.snapshot(crop) : undefined,
        size: image ? { w: outW, h: outH } : undefined,
        trim: trimmed ? trim : undefined,
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
      result = { name, size: out.size, saved: destination === 'save' };
      phase = 'done';
    } catch (e) {
      if (e?.name === 'AbortError') {
        phase = 'choose';
        return;
      }
      error = errorMessage(e);
      phase = 'error';
    } finally {
      controller = null;
    }
  }

  function cancel() {
    if (controller) controller.abort();
    else onclose();
  }
</script>

<Modal
  title="Convert {entry.meta.name}"
  description="It's done in this browser."
  onclose={() => (controller?.abort(), onclose())}
  onsubmit={run}
  class={kind === 'image' ? 'max-w-2xl' : 'max-w-lg'}>
  {#if tooBig}
    <p class="flex items-start gap-2 text-[13px] text-fg-muted">
      <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
      This file is {formatSize(entry.meta.size)}. Conversion happens in memory in your browser, so it's limited to {formatSize(limit)} for {kind === 'image' ? 'images' : 'video and audio'}.
    </p>
  {:else if groups === null || (kind === 'image' && !image && !imageError)}
    <div class="skeleton h-40" aria-hidden="true"></div>
  {:else if imageError}
    <p class="text-[13px] text-danger">{imageError}</p>
  {:else if phase === 'choose' || phase === 'error'}
    {#each groups as group (group.title)}
      <fieldset class="grid gap-2">
        <legend class="mb-2 text-xs font-medium text-fg-muted">{group.title}</legend>
        <div class="grid grid-cols-3 gap-2 sm:grid-cols-6">
          {#each group.targets as t (t.id)}
            <label
              class="flex h-10 cursor-pointer flex-col items-center justify-center rounded-md border text-sm leading-tight transition-colors has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring {target === t
                ? 'border-accent bg-accent-soft font-medium text-accent-text'
                : 'border-line hover:bg-subtle'}">
              <input type="radio" class="sr-only" name="target" value={t.id} checked={target === t} onchange={() => (target = t)} />
              {t.label}
              {#if t.same}<span class="text-[10px] font-normal text-fg-muted">same format</span>{/if}
            </label>
          {/each}
        </div>
      </fieldset>
    {/each}

    {#if image}
      <div class="grid gap-3">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <p class="text-xs font-medium text-fg-muted">Crop</p>
          <div class="flex flex-wrap items-center gap-1" role="radiogroup" aria-label="Crop shape">
            {#each ASPECTS as [label, value] (label)}
              <button
                type="button"
                role="radio"
                aria-checked={aspectChoice === value}
                class="h-6 cursor-pointer rounded px-2 text-xs transition-colors {aspectChoice === value ? 'bg-muted font-medium text-fg' : 'text-fg-muted hover:text-fg'}"
                onclick={() => (aspectChoice = value)}>{label}</button>
            {/each}
            <span class="mx-1 h-4 w-px bg-line max-sm:hidden" aria-hidden="true"></span>
            <button type="button" class="h-6 cursor-pointer rounded px-2 text-xs text-fg-muted hover:text-fg disabled:cursor-default disabled:opacity-40" disabled={!edited} onclick={resetEdits}>Reset</button>
          </div>
        </div>
        <ImageCrop src={image.url} width={image.width} height={image.height} bind:crop {aspect} />
        <div class="flex flex-wrap items-end gap-3">
          <div class="field">
            <label class="label" for="out-w">Width</label>
            <input id="out-w" class="input h-8 w-24 tabular-nums" inputmode="numeric" value={outW} onchange={(e) => setWidth(e.currentTarget.value)} />
          </div>
          <button
            type="button"
            class="btn btn-ghost btn-icon mb-0.5 {lock ? 'text-accent-text' : ''}"
            aria-pressed={lock}
            aria-label="Keep proportions"
            title="Keep proportions"
            onclick={() => ((lock = !lock), lock && setWidth(outW))}><Icon name={lock ? 'lock' : 'lock-open'} /></button>
          <div class="field">
            <label class="label" for="out-h">Height</label>
            <input id="out-h" class="input h-8 w-24 tabular-nums" inputmode="numeric" value={outH} onchange={(e) => setHeight(e.currentTarget.value)} />
          </div>
          <div class="mb-1 flex gap-1">
            {#each [[1, '100%'], [0.5, '50%'], [0.25, '25%']] as [f, label] (label)}
              <button type="button" class="h-6 cursor-pointer rounded border border-line px-2 text-xs text-fg-muted hover:text-fg" onclick={() => scaleTo(f)}>{label}</button>
            {/each}
          </div>
          <p class="mb-1.5 ml-auto text-xs text-fg-muted tabular-nums">Original {image.width} x {image.height}</p>
        </div>
        {#if sizeError}<p class="text-[13px] text-danger">{sizeError}</p>{/if}
      </div>
    {/if}

    {#if target?.quality}
      <div class="field">
        <label class="label flex justify-between" for="quality"><span>Quality</span><span class="text-fg-muted tabular-nums">{quality}</span></label>
        <input id="quality" type="range" min="30" max="100" step="1" bind:value={quality} class="w-full accent-accent" />
      </div>
    {/if}

    {#if kind === 'video' || kind === 'audio'}
      <div class="grid gap-2">
        <p class="text-xs font-medium text-fg-muted">Trim (optional)</p>
        <div class="flex flex-wrap items-end gap-3">
          <div class="field">
            <label class="label" for="trim-from">From</label>
            <input id="trim-from" class="input h-8 w-28 tabular-nums" placeholder="0:00" bind:value={trimFrom} autocomplete="off" />
          </div>
          <div class="field">
            <label class="label" for="trim-to">To</label>
            <input id="trim-to" class="input h-8 w-28 tabular-nums" placeholder="the end" bind:value={trimTo} autocomplete="off" />
          </div>
        </div>
        {#if trimError}<p class="text-[13px] text-danger">{trimError}</p>{/if}
      </div>
    {/if}

    {#if save}
      <div class="grid gap-2">
        <p class="text-xs font-medium text-fg-muted">Then</p>
        <div class="flex flex-wrap gap-4 text-sm">
          <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={destination} value="save" />Save it in this folder</label>
          <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={destination} value="download" />Download it</label>
        </div>
      </div>
    {/if}

    {#if kind !== 'image'}
      <p class="hint">Video and audio are converted by ffmpeg in your browser, single-threaded, so long files take a while. Changing only the container (say MOV to MP4) is usually quick.</p>
    {/if}
    {#if pointless}<p class="hint">It's already in this format. Pick another, or {kind === 'image' ? 'crop or resize it' : 'trim it'} first.</p>{/if}
    {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {:else if phase === 'working'}
    <div class="grid gap-3 py-2" role="status">
      <p class="flex items-center gap-2 text-sm"><Icon name="loader-circle" class="spinner" />{stageText[stage]}</p>
      <div class="progress"><div style:width="{Math.round(progress * 100)}%"></div></div>
      <p class="text-xs text-fg-muted tabular-nums">{Math.round(progress * 100)}%</p>
    </div>
  {:else if phase === 'done'}
    <div class="grid gap-1 py-2">
      <p class="flex items-center gap-2 text-sm font-medium"><Icon name="check" class="size-4 text-success" />{result.saved ? `Saved as ${result.name}` : `Downloaded ${result.name}`}</p>
      <p class="text-[13px] text-fg-muted tabular-nums">{formatSize(entry.meta.size)} to {formatSize(result.size)}</p>
    </div>
  {/if}

  {#snippet footer()}
    {#if phase === 'done'}
      <button class="btn btn-primary">Done</button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={cancel}>{phase === 'working' ? 'Stop' : 'Cancel'}</button>
      <button class="btn btn-primary" disabled={!canRun}>
        {#if phase === 'working'}<Icon name="loader-circle" class="spinner" />{/if}
        {target?.same ? 'Save changes' : `Convert${target ? ` to ${target.label}` : ''}`}
      </button>
    {/if}
  {/snippet}
</Modal>
