<script>
  // Download a video from a link through the server (yt-dlp). Unlike
  // everything else, the server sees the link and the video here, so the
  // dialog says so. It keeps neither: the video is streamed to this browser,
  // then encrypted and uploaded like any other file (or just saved).
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { videoInfo, downloadVideo } from '../../lib/cloud.svelte.js';
  import { saveBlob } from '../../lib/crypto.js';
  import { formatSize } from '../../lib/format.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  /** save(file, onProgress) uploads it into the current folder (null if you can't write here). */
  let { save = null, maxBytes = 0, canMerge = false, onclose } = $props();

  let url = $state('');
  let info = $state(null); // VideoInfo from the server
  let kind = $state('video');
  let destination = $state(untrack(() => (save ? 'save' : 'download')));
  let phase = $state('link'); // link | looking | choose | working | done
  let stage = $state(''); // downloading | saving
  let received = $state(0);
  let progress = $state(0);
  let error = $state('');
  let result = $state(null);
  let controller = null;

  const option = $derived(info ? (kind === 'video' ? info.video : info.audio) : null);

  function duration(s) {
    if (s == null) return '';
    s = Math.round(s);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = String(s % 60).padStart(2, '0');
    return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
  }

  /** A file name from the video's title. */
  function fileName(title, ext) {
    const clean = title.replace(/[\u0000-\u001f\u007f/\\:*?"<>|]+/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 150) || 'video';
    return `${clean}.${ext}`;
  }

  async function lookUp() {
    error = '';
    phase = 'looking';
    try {
      info = await videoInfo(url.trim());
      kind = info.video ? 'video' : 'audio';
      phase = 'choose';
    } catch (e) {
      error = e?.code === 'busy' ? 'You already have a download running. Wait for it to finish.' : errorMessage(e);
      phase = 'link';
    }
  }

  async function start() {
    controller = new AbortController();
    phase = 'working';
    stage = 'downloading';
    received = 0;
    error = '';
    try {
      const blob = await downloadVideo(url.trim(), kind, { signal: controller.signal, onProgress: (n) => (received = n) });
      const name = fileName(info.title, option?.ext ?? (kind === 'video' ? 'mp4' : 'm4a'));
      const type = kind === 'video' ? 'video/mp4' : 'audio/mp4';
      if (destination === 'save') {
        stage = 'saving';
        progress = 0;
        await save(new File([blob], name, { type, lastModified: Date.now() }), (p) => (progress = p));
      } else {
        saveBlob(new Blob([blob], { type: 'application/octet-stream' }), name);
      }
      result = { name, size: blob.size, saved: destination === 'save' };
      phase = 'done';
    } catch (e) {
      phase = 'choose';
      if (e?.name !== 'AbortError') error = e?.code === 'busy' ? 'You already have a download running. Wait for it to finish.' : errorMessage(e);
    } finally {
      controller = null;
    }
  }

  function submit() {
    if (phase === 'link') return url.trim() && lookUp();
    if (phase === 'choose') return option && start();
    if (phase === 'done') return onclose();
  }

  function cancel() {
    if (controller) controller.abort();
    else onclose();
  }
</script>

<Modal title="Download from a video link" {onclose} onsubmit={submit} class="max-w-lg">
  <div class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
    <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0 text-fg" />
    <p>
      <span class="font-medium text-fg">This server downloads it for you, so it sees the link and the video.</span>
      Nothing is kept there: the video comes straight to this browser, which encrypts it like any upload.
    </p>
  </div>

  {#if phase === 'link' || phase === 'looking'}
    <div class="field">
      <label class="label" for="video-url">Link</label>
      <div class="flex gap-2">
        <!-- svelte-ignore a11y_autofocus -->
        <input id="video-url" class="input" type="url" bind:value={url} placeholder="https://www.youtube.com/watch?v=..." autocomplete="off" spellcheck="false" required autofocus />
        <button class="btn btn-primary h-9" disabled={phase === 'looking' || !url.trim()}>
          {#if phase === 'looking'}<Icon name="loader-circle" class="spinner" />{/if}
          Look up
        </button>
      </div>
      <p class="hint">Links to a single video on YouTube, Vimeo and most other video sites work.</p>
    </div>
  {:else if info}
    <div class="grid gap-1 rounded-md border border-line p-3">
      <p class="truncate font-medium" title={info.title}>{info.title}</p>
      <p class="truncate text-xs text-fg-muted">
        {[info.site, info.uploader, duration(info.duration)].filter(Boolean).join(' · ')}
      </p>
    </div>

    {#if phase === 'choose'}
      <fieldset class="grid gap-2">
        <legend class="mb-2 text-xs font-medium text-fg-muted">Get</legend>
        {#each [['video', 'Video', info.video], ['audio', 'Audio only', info.audio]] as [value, label, opt] (value)}
          <label class="flex cursor-pointer items-center gap-3 rounded-md border px-3 py-2 text-sm {kind === value ? 'border-accent bg-accent-soft' : 'border-line hover:bg-subtle'} {opt ? '' : 'pointer-events-none opacity-50'}">
            <input type="radio" class="accent-accent" bind:group={kind} {value} disabled={!opt} />
            <span class="font-medium {kind === value ? 'text-accent-text' : ''}">{label}</span>
            <span class="ml-auto text-xs text-fg-muted tabular-nums">
              {#if opt}
                {opt.ext.toUpperCase()}{opt.height ? ` · ${opt.height}p` : ''}{opt.size ? ` · about ${formatSize(opt.size)}` : ''}
              {:else}
                {canMerge ? 'Not available' : 'Not offered as one file'}
              {/if}
            </span>
          </label>
        {/each}
      </fieldset>
      {#if option?.size && maxBytes && option.size > maxBytes}
        <p class="text-[13px] text-danger">That's larger than this server allows ({formatSize(maxBytes)}).</p>
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
      <p class="hint">
        {canMerge
          ? 'Video is saved as MP4, up to 1080p. It is put together as it streams, so nothing is written to the server.'
          : "This server can only save videos a site offers as one file, and YouTube rarely does. Audio only usually works."}
      </p>
    {:else if phase === 'working'}
      <div class="grid gap-3 py-1" role="status">
        <p class="flex items-center gap-2 text-sm">
          <Icon name="loader-circle" class="spinner" />
          {stage === 'saving' ? 'Encrypting and saving' : `Downloading ${formatSize(received)}${option?.size ? ` of about ${formatSize(option.size)}` : ''}`}
        </p>
        {#if stage === 'saving' || option?.size}
          <div class="progress"><div style:width="{Math.round(Math.min(1, stage === 'saving' ? progress : received / option.size) * 100)}%"></div></div>
        {/if}
      </div>
    {:else if phase === 'done'}
      <p class="flex items-center gap-2 py-1 text-sm font-medium">
        <Icon name="check" class="size-4 text-success" />{result.saved ? `Saved as ${result.name}` : `Downloaded ${result.name}`} ({formatSize(result.size)})
      </p>
    {/if}
  {/if}

  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}

  {#snippet footer()}
    {#if phase === 'done'}
      <button class="btn btn-primary">Done</button>
    {:else if phase === 'choose' || phase === 'working'}
      <button type="button" class="btn btn-secondary mr-auto" disabled={phase === 'working'} onclick={() => ((phase = 'link'), (info = null), (error = ''))}>Back</button>
      <button type="button" class="btn btn-secondary" onclick={cancel}>{phase === 'working' ? 'Stop' : 'Cancel'}</button>
      <button class="btn btn-primary" disabled={phase === 'working' || !option || (maxBytes && option.size > maxBytes)}>
        {#if phase === 'working'}<Icon name="loader-circle" class="spinner" />{/if}
        {destination === 'save' ? 'Download and save' : 'Download'}
      </button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
    {/if}
  {/snippet}
</Modal>
