<script>
  // Shown over a video the browser can't play: offers to convert it for
  // viewing here (lib/playable.js). `onready(url)` gets a blob: URL of the
  // converted video; the caller plays it and revokes it.
  import { onDestroy } from 'svelte';
  import Icon from './Icon.svelte';
  import { convertForViewing, MAX_MEDIA } from '../lib/playable.js';
  import { formatSize } from '../lib/format.js';
  import { errorMessage } from '../lib/ui.svelte.js';
  import { t } from '../lib/i18n.svelte.js';

  let { entry, onready } = $props();

  let status = $state('idle'); // idle | busy | error
  let stage = $state('downloading');
  let progress = $state(0);
  let message = $state('');
  let ctl = null;

  const tooBig = $derived(entry.meta.size > MAX_MEDIA);
  const stageText = $derived(
    {
      downloading: t('Decrypting the video...'),
      loading: t('Loading the converter...'),
      remuxing: t('Repackaging the video...'),
      converting: t('Converting the video. This can take as long as the video itself, or longer.'),
    }[stage],
  );

  async function start() {
    status = 'busy';
    progress = 0;
    ctl = new AbortController();
    try {
      const url = await convertForViewing(entry, {
        signal: ctl.signal,
        onStage: (s) => ((stage = s), (progress = 0)),
        onProgress: (p) => (progress = p),
      });
      if (ctl.signal.aborted) return URL.revokeObjectURL(url);
      onready(url);
    } catch (e) {
      if (e?.name === 'AbortError') status = 'idle';
      else {
        message = errorMessage(e);
        status = 'error';
      }
    }
  }

  function cancel() {
    ctl?.abort();
    status = 'idle';
  }

  onDestroy(() => ctl?.abort());
</script>

<div class="card grid w-full max-w-md gap-3 p-5 text-left" role="status">
  <div class="flex items-start gap-3">
    <Icon name="film" class="mt-0.5 size-5 shrink-0 text-fg-muted" />
    <div class="grid gap-1">
      <p class="text-sm font-medium">{t("Your browser can't play this video's format.")}</p>
      <p class="text-[13px] text-fg-muted">
        {tooBig
          ? t('It could be converted here for watching, but at {size} it is too large to convert in the browser. Download it and use a video player instead.', { size: formatSize(entry.meta.size) })
          : t('It can be converted here, in your browser, into a format it plays. Nothing is uploaded or saved; the converted copy is gone when you close it.')}
      </p>
    </div>
  </div>
  {#if status === 'busy'}
    <div class="grid gap-1.5">
      <div class="progress"><div style:width="{Math.round(progress * 100)}%"></div></div>
      <p class="text-xs text-fg-muted">{stageText}</p>
    </div>
  {:else if status === 'error'}
    <p class="flex items-start gap-2 text-[13px] text-fg-muted"><Icon name="circle-alert" class="mt-0.5 size-4 shrink-0 text-danger" />{message}</p>
  {/if}
  {#if !tooBig}
    <div class="flex justify-end gap-2">
      {#if status === 'busy'}
        <button type="button" class="btn btn-secondary" onclick={cancel}>{t('Cancel')}</button>
      {:else}
        <button type="button" class="btn btn-primary" onclick={start}><Icon name="play" />{status === 'error' ? t('Try again') : t('Convert to watch')}</button>
      {/if}
    </div>
  {/if}
</div>
