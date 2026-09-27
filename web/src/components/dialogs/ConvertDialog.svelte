<script>
  // "Convert": pick a format, then download the result or save it next
  // to the original. Decrypting, converting and re-encrypting all happen in
  // this browser.
  import { onMount, untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { targetsFor, convert, convertedName, sourceKind, MAX_IMAGE, MAX_MEDIA } from '../../lib/convert.js';
  import { saveBlob } from '../../lib/crypto.js';
  import { formatSize } from '../../lib/format.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  /** fetch(entry, onProgress) -> { blob }; save(file) uploads it next to the original (null if you can't write here). */
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

  onMount(async () => {
    groups = await targetsFor(entry.meta);
    target = groups[0]?.targets[0] ?? null;
  });

  const stageText = {
    decrypting: 'Decrypting',
    loading: 'Loading the converter (about 10 MB, only the first time)',
    converting: 'Converting',
    saving: 'Encrypting and saving',
  };

  async function run() {
    if (phase === 'done') return onclose();
    if (!target || phase === 'working') return;
    controller = new AbortController();
    const { signal } = controller;
    phase = 'working';
    error = '';
    try {
      stage = 'decrypting';
      progress = 0;
      const { blob } = await fetch(entry, (p) => (progress = p));
      if (signal.aborted) throw new DOMException('Cancelled', 'AbortError');
      progress = 0;
      stage = 'converting';
      const out = await convert(blob, entry.meta, target, {
        quality: quality / 100,
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
  description="It's converted in this browser. The server never sees the file, and a saved copy is encrypted like any upload."
  onclose={() => (controller?.abort(), onclose())}
  onsubmit={run}
  class="max-w-lg">
  {#if tooBig}
    <p class="flex items-start gap-2 text-[13px] text-fg-muted">
      <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
      This file is {formatSize(entry.meta.size)}. Conversion happens in memory in your browser, so it's limited to {formatSize(limit)} for {kind === 'image' ? 'images' : 'video and audio'}.
    </p>
  {:else if groups === null}
    <div class="skeleton h-24" aria-hidden="true"></div>
  {:else if !groups.length || !groups.some((g) => g.targets.length)}
    <p class="text-[13px] text-fg-muted">There's nothing this file can be converted to here.</p>
  {:else if phase === 'choose' || phase === 'error'}
    {#each groups as group (group.title)}
      {#if group.targets.length}
        <fieldset class="grid gap-2">
          <legend class="mb-2 text-xs font-medium text-fg-muted">{group.title}</legend>
          <div class="grid grid-cols-3 gap-2 sm:grid-cols-4">
            {#each group.targets as t (t.id)}
              <label
                class="flex h-9 cursor-pointer items-center justify-center rounded-md border text-sm transition-colors has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring {target === t
                  ? 'border-accent bg-accent-soft font-medium text-accent-text'
                  : 'border-line hover:bg-subtle'}">
                <input type="radio" class="sr-only" name="target" value={t.id} checked={target === t} onchange={() => (target = t)} />
                {t.label}
              </label>
            {/each}
          </div>
        </fieldset>
      {/if}
    {/each}

    {#if target?.quality}
      <div class="field">
        <label class="label flex justify-between" for="quality"><span>Quality</span><span class="text-fg-muted tabular-nums">{quality}</span></label>
        <input id="quality" type="range" min="30" max="100" step="1" bind:value={quality} class="w-full accent-accent" />
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
      <button class="btn btn-primary" disabled={!target || tooBig || phase === 'working'}>
        {#if phase === 'working'}<Icon name="loader-circle" class="spinner" />{/if}
        Convert{target ? ` to ${target.label}` : ''}
      </button>
    {/if}
  {/snippet}
</Modal>
