<script>
  // Download a video from a link through the server (yt-dlp). Unlike
  // everything else, the server sees the link and the video here, so the
  // dialog says so. It keeps neither: the video is streamed to this browser,
  // then encrypted and uploaded like any other file (or just saved).
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import { t } from '../../lib/i18n.svelte.js';
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
  let quality = $state('1080');
  // Playlists: which entries to get, and how far along it is.
  let picked = $state(new Set());
  let batch = $state({ index: 0, total: 0, done: 0, failed: [] });

  const playlist = $derived(!!info?.entries?.length);
  const option = $derived(
    !info || playlist ? null : kind === 'video' ? (info.qualities?.find((q) => q.quality === quality) ?? info.video) : info.audio,
  );
  const QUALITIES = $derived([
    ['480', '480p'],
    ['720', '720p'],
    ['1080', '1080p'],
    ['best', t('Best')],
  ]);
  const CAP = { 480: 480, 720: 720, 1080: 1080, best: Infinity };

  /** For one playlist video: the closest match to the chosen quality. */
  function qualityFor(entryInfo) {
    const opts = entryInfo.qualities ?? [];
    const fit = opts.filter((o) => (o.height ?? 0) <= CAP[quality]);
    return (fit[fit.length - 1] ?? opts[0])?.quality ?? '1080';
  }

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
      kind = info.video || info.entries?.length ? 'video' : 'audio';
      quality = info.video?.quality ?? '1080';
      picked = new Set(info.entries?.map((e) => e.url) ?? []);
      phase = 'choose';
    } catch (e) {
      error = e?.code === 'busy' ? t('You already have a download running. Wait for it to finish.') : errorMessage(e);
      phase = 'link';
    }
  }

  /** Download (and save) one video; returns { name, size }. */
  async function fetchOne(link, title, opt, q, signal) {
    received = 0;
    stage = 'downloading';
    const blob = await downloadVideo(link, kind, { quality: q, signal, onProgress: (n) => (received = n) });
    const name = fileName(title, opt?.ext ?? (kind === 'video' ? 'mp4' : 'm4a'));
    const type = kind === 'video' ? 'video/mp4' : 'audio/mp4';
    if (destination === 'save') {
      stage = 'saving';
      progress = 0;
      await save(new File([blob], name, { type, lastModified: Date.now() }), (p) => (progress = p));
    } else {
      await saveBlob(new Blob([blob], { type: 'application/octet-stream' }), name);
    }
    return { name, size: blob.size };
  }

  async function startPlaylist() {
    controller = new AbortController();
    const list = info.entries.filter((e) => picked.has(e.url));
    batch = { index: 0, total: list.length, done: 0, failed: [], current: '' };
    phase = 'working';
    error = '';
    for (const [i, e] of list.entries()) {
      if (controller.signal.aborted) break;
      batch.index = i + 1;
      batch.current = e.title;
      try {
        const one = await videoInfo(e.url);
        const opt = kind === 'video' ? (one.qualities?.find((q) => q.quality === qualityFor(one)) ?? one.video) : one.audio;
        if (!opt) throw new Error(kind === 'video' ? t('No video to download') : t('No audio to download'));
        if (maxBytes && opt.size > maxBytes) throw new Error(t('Too large for this server'));
        batchOption = opt;
        await fetchOne(e.url, one.title, opt, kind === 'video' ? opt.quality : undefined, controller.signal);
        batch.done++;
      } catch (err) {
        if (err?.name === 'AbortError') break;
        batch.failed.push({ title: e.title, error: errorMessage(err) });
      }
    }
    const stopped = controller.signal.aborted;
    controller = null;
    batchOption = null;
    result = { playlist: true, done: batch.done, failed: batch.failed, stopped, saved: destination === 'save' };
    phase = 'done';
  }
  let batchOption = $state(null);
  const shown = $derived(batchOption ?? option);

  async function start() {
    if (playlist) return startPlaylist();
    controller = new AbortController();
    phase = 'working';
    stage = 'downloading';
    received = 0;
    error = '';
    try {
      const r = await fetchOne(url.trim(), info.title, option, kind === 'video' ? option.quality : undefined, controller.signal);
      result = { ...r, saved: destination === 'save' };
      phase = 'done';
    } catch (e) {
      phase = 'choose';
      if (e?.name !== 'AbortError') error = e?.code === 'busy' ? t('You already have a download running. Wait for it to finish.') : errorMessage(e);
    } finally {
      controller = null;
    }
  }

  function submit() {
    if (phase === 'link') return url.trim() && lookUp();
    if (phase === 'choose') return (playlist ? picked.size : option) && start();
    if (phase === 'done') return onclose();
  }

  function cancel() {
    if (controller) controller.abort();
    else onclose();
  }
</script>

<Modal title={t('Download from a video link')} {onclose} onsubmit={submit} class="max-w-lg">
  <div class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
    <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0 text-fg" />
    <p>
      <span class="font-medium text-fg">{t('This server downloads it for you, so it sees the link and the video.')}</span>
      {t('Nothing is kept there: the video comes straight to this browser, which encrypts it like any upload.')}
    </p>
  </div>

  {#if phase === 'link' || phase === 'looking'}
    <div class="field">
      <label class="label" for="video-url">{t('Link')}</label>
      <div class="flex gap-2">
        <!-- svelte-ignore a11y_autofocus -->
        <input id="video-url" class="input" type="url" bind:value={url} placeholder="https://www.youtube.com/watch?v=..." autocomplete="off" spellcheck="false" required autofocus />
        <button class="btn btn-primary h-9" disabled={phase === 'looking' || !url.trim()}>
          {#if phase === 'looking'}<Icon name="loader-circle" class="spinner" />{/if}
          {t('Look up')}
        </button>
      </div>
      <p class="hint">{t('Links to a video or a playlist on YouTube, Vimeo and most other video sites work.')}</p>
    </div>
  {:else if info}
    <div class="grid gap-1 rounded-md border border-line p-3">
      <p class="truncate font-medium" title={info.title}>{info.title}</p>
      <p class="truncate text-xs text-fg-muted">
        {[info.site, info.uploader, playlist ? t('{count} videos', { count: info.entries.length }) : duration(info.duration)].filter(Boolean).join(' · ')}
      </p>
    </div>

    {#if phase === 'choose' && playlist}
      <div class="grid gap-2">
        <div class="flex items-center justify-between text-xs text-fg-muted">
          <span>{t('{n} of {total} selected', { n: picked.size, total: info.entries.length })}</span>
          <button
            type="button"
            class="cursor-pointer hover:text-fg"
            onclick={() => (picked = picked.size === info.entries.length ? new Set() : new Set(info.entries.map((e) => e.url)))}>
            {picked.size === info.entries.length ? t('Select none') : t('Select all')}
          </button>
        </div>
        <ul class="max-h-56 divide-y divide-line overflow-y-auto rounded-md border border-line">
          {#each info.entries as e (e.url)}
            <li>
              <label class="flex cursor-pointer items-center gap-3 px-3 py-2 text-sm hover:bg-subtle">
                <input
                  type="checkbox"
                  class="size-4 accent-accent"
                  checked={picked.has(e.url)}
                  onchange={(ev) => {
                    const next = new Set(picked);
                    if (ev.currentTarget.checked) next.add(e.url);
                    else next.delete(e.url);
                    picked = next;
                  }} />
                <span class="min-w-0 flex-1 truncate">{e.title}</span>
                {#if e.duration}<span class="text-xs text-fg-muted tabular-nums">{duration(e.duration)}</span>{/if}
              </label>
            </li>
          {/each}
        </ul>
      </div>
      <div class="flex flex-wrap items-start gap-4">
        <div class="field">
          <span class="label">{t('Get')}</span>
          <div class="flex gap-4 text-sm">
            <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={kind} value="video" />{t('Video')}</label>
            <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={kind} value="audio" />{t('Audio only')}</label>
          </div>
        </div>
        {#if kind === 'video'}
          <div class="field">
            <label class="label" for="pl-quality">{t('Up to')}</label>
            <select id="pl-quality" class="input h-8 w-auto" bind:value={quality}>
              {#each QUALITIES as [value, label] (value)}<option {value}>{label}</option>{/each}
            </select>
          </div>
        {/if}
      </div>
      {#if save}
        <div class="grid gap-2">
          <p class="text-xs font-medium text-fg-muted">Then</p>
          <div class="flex flex-wrap gap-4 text-sm">
            <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={destination} value="save" />{t('Save them in this folder')}</label>
            <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={destination} value="download" />{t('Download them')}</label>
          </div>
        </div>
      {/if}
      <p class="hint">{t('Videos are fetched one at a time. Any that fail are skipped and listed at the end.')}</p>
    {:else if phase === 'choose'}
      <fieldset class="grid gap-2">
        <legend class="mb-2 text-xs font-medium text-fg-muted">{t('Get')}</legend>
        {#each [['video', t('Video'), info.video], ['audio', t('Audio only'), info.audio]] as [value, label, opt] (value)}
          <label class="flex cursor-pointer items-center gap-3 rounded-md border px-3 py-2 text-sm {kind === value ? 'border-accent bg-accent-soft' : 'border-line hover:bg-subtle'} {opt ? '' : 'pointer-events-none opacity-50'}">
            <input type="radio" class="accent-accent" bind:group={kind} {value} disabled={!opt} />
            <span class="font-medium {kind === value ? 'text-accent-text' : ''}">{label}</span>
            <span class="ml-auto text-xs text-fg-muted tabular-nums">
              {#if value === 'video' && opt}{@const o = option ?? opt}
                {o.ext.toUpperCase()}{o.height ? ` · ${o.height}p` : ''}{o.size ? ` · ${t('about {size}', { size: formatSize(o.size) })}` : ''}
              {:else if opt}
                {opt.ext.toUpperCase()}{opt.size ? ` · ${t('about {size}', { size: formatSize(opt.size) })}` : ''}
              {:else}
                {canMerge ? t('Not available') : t('Not offered as one file')}
              {/if}
            </span>
          </label>
        {/each}
      </fieldset>
      {#if kind === 'video' && info.qualities?.length > 1}
        <div class="flex flex-wrap gap-2" role="radiogroup" aria-label={t('Quality')}>
          {#each info.qualities as q (q.quality)}
            <button
              type="button"
              role="radio"
              aria-checked={quality === q.quality}
              class="h-8 cursor-pointer rounded-md border px-3 text-[13px] tabular-nums transition-colors {quality === q.quality
                ? 'border-accent bg-accent-soft font-medium text-accent-text'
                : 'border-line text-fg-muted hover:bg-subtle hover:text-fg'}"
              onclick={() => (quality = q.quality)}>{q.height ? `${q.height}p` : t('Default')}</button>
          {/each}
        </div>
      {/if}
      {#if option?.size && maxBytes && option.size > maxBytes}
        <p class="text-[13px] text-danger">{t("That's larger than this server allows ({size}).", { size: formatSize(maxBytes) })}</p>
      {/if}

      {#if save}
        <div class="grid gap-2">
          <p class="text-xs font-medium text-fg-muted">Then</p>
          <div class="flex flex-wrap gap-4 text-sm">
            <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={destination} value="save" />{t('Save it in this folder')}</label>
            <label class="flex cursor-pointer items-center gap-2"><input type="radio" class="accent-accent" bind:group={destination} value="download" />{t('Download it')}</label>
          </div>
        </div>
      {/if}
      <p class="hint">
        {canMerge
          ? option?.height > 1080
            ? t('Video is saved as MP4, put together as it streams, so nothing is written to the server. Above 1080p it is usually VP9 or AV1, which not every player can open.')
            : t('Video is saved as MP4, put together as it streams, so nothing is written to the server.')
          : t('This server can only save videos a site offers as one file, and YouTube rarely does. Audio only usually works.')}
      </p>
    {:else if phase === 'working'}
      <div class="grid gap-3 py-1" role="status">
        {#if playlist}
          <p class="truncate text-xs text-fg-muted">{t('Video {n} of {total}: {name}', { n: batch.index, total: batch.total, name: batch.current })}</p>
        {/if}
        <p class="flex items-center gap-2 text-sm">
          <Icon name="loader-circle" class="spinner" />
          {stage === 'saving' ? t('Saving') : shown?.size ? t('Downloading {size} of about {total}', { size: formatSize(received), total: formatSize(shown.size) }) : t('Downloading {size}', { size: formatSize(received) })}
        </p>
        {#if stage === 'saving' || shown?.size}
          <div class="progress"><div style:width="{Math.round(Math.min(1, stage === 'saving' ? progress : received / shown.size) * 100)}%"></div></div>
        {/if}
      </div>
    {:else if phase === 'done' && result.playlist}
      <p class="flex items-center gap-2 py-1 text-sm font-medium">
        <Icon name="check" class="size-4 text-success" />
        {result.saved
          ? result.stopped
            ? t('Saved {n} of {total} videos before you stopped', { n: result.done, total: batch.total })
            : t('Saved {n} of {total} videos', { n: result.done, total: batch.total })
          : result.stopped
            ? t('Downloaded {n} of {total} videos before you stopped', { n: result.done, total: batch.total })
            : t('Downloaded {n} of {total} videos', { n: result.done, total: batch.total })}
      </p>
      {#if result.failed.length}
        <ul class="max-h-40 divide-y divide-line overflow-y-auto rounded-md border border-line text-[13px]">
          {#each result.failed as f, i (i)}
            <li class="grid gap-0.5 px-3 py-2"><span class="truncate font-medium">{f.title}</span><span class="text-xs text-danger">{f.error}</span></li>
          {/each}
        </ul>
      {/if}
    {:else if phase === 'done'}
      <p class="flex items-center gap-2 py-1 text-sm font-medium">
        <Icon name="check" class="size-4 text-success" />{result.saved ? t('Saved as {name}', { name: result.name }) : t('Downloaded {name}', { name: result.name })} ({formatSize(result.size)})
      </p>
    {/if}
  {/if}

  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}

  {#snippet footer()}
    {#if phase === 'done'}
      <button class="btn btn-primary">{t('Done')}</button>
    {:else if phase === 'choose' || phase === 'working'}
      <button type="button" class="btn btn-secondary mr-auto" disabled={phase === 'working'} onclick={() => ((phase = 'link'), (info = null), (error = ''))}>{t('Back')}</button>
      <button type="button" class="btn btn-secondary" onclick={cancel}>{phase === 'working' ? t('Stop') : t('Cancel')}</button>
      <button class="btn btn-primary" disabled={phase === 'working' || (playlist ? !picked.size : !option || (maxBytes && option.size > maxBytes))}>
        {#if phase === 'working'}<Icon name="loader-circle" class="spinner" />{/if}
        {playlist
          ? destination === 'save'
            ? t('Download and save {count}', { count: picked.size })
            : t('Download {count}', { count: picked.size })
          : destination === 'save'
            ? t('Download and save')
            : t('Download')}
      </button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
    {/if}
  {/snippet}
</Modal>
