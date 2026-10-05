<script>
  // Full-screen video playback. Picks up where you left off, keeps your
  // place as you watch, and offers the next episode when one ends.
  import { onMount, onDestroy, untrack } from 'svelte';
  import { t } from '../lib/i18n.svelte.js';
  import Icon from './Icon.svelte';
  import { fade, portal } from '../lib/motion.js';
  import { details, openVideo, saveProgress, resumeAt, seriesOf, subtitlesFor } from '../lib/videos.svelte.js';
  import { fetchEntry } from '../lib/cloud.svelte.js';
  import { loadSubtitles, release as releaseSubtitles } from '../lib/subtitles.js';
  import SubtitlePicker from './SubtitlePicker.svelte';
  import { player as music, toggle as toggleMusic } from '../lib/music.svelte.js';
  import { errorMessage } from '../lib/ui.svelte.js';
  import { inAndroidApp } from '../lib/native.js';

  let { video, onclose } = $props();

  let v = $state.raw(untrack(() => video));
  let src = $state(null);
  let error = $state('');
  let el = $state();
  let countdown = $state(null); // seconds until the next episode plays
  let served = null;
  let seq = 0;
  let lastSave = 0;

  const d = $derived(details(v));
  const series = $derived(seriesOf(v));
  const nextVideo = $derived(series?.episodes[series.episodes.indexOf(v) + 1] ?? null);

  onMount(() => {
    if (music.playing) toggleMusic();
  });

  $effect(() => {
    const cur = v;
    untrack(() => load(cur));
  });

  // Subtitles named after the video, next to it.
  let subtitles = $state([]);
  $effect(() => {
    const matched = subtitlesFor(v);
    if (!matched.length) return;
    let live = true;
    let got = [];
    loadSubtitles(matched, fetchEntry).then((subs) => {
      got = subs;
      if (live) subtitles = subs;
      else releaseSubtitles(subs);
    });
    return () => {
      live = false;
      releaseSubtitles(got);
      subtitles = [];
    };
  });

  async function load(cur) {
    const my = ++seq;
    served?.close();
    served = null;
    src = null;
    error = '';
    countdown = null;
    try {
      const s = await openVideo(cur);
      if (my !== seq) return s.close();
      served = s;
      src = s.url;
    } catch (e) {
      if (my === seq) error = errorMessage(e);
    }
  }

  function save() {
    if (el) saveProgress(v, el.currentTime, el.duration);
  }

  function go(next) {
    save();
    v = next;
  }

  onDestroy(() => {
    save();
    seq++;
    served?.close();
  });

  function ontimeupdate() {
    if (Date.now() - lastSave > 60_000) {
      lastSave = Date.now();
      save();
    }
  }

  function onloadedmetadata() {
    const at = resumeAt(v);
    if (at) el.currentTime = at;
  }

  function onended() {
    saveProgress(v, el.duration, el.duration);
    if (nextVideo) countdown = 8;
  }

  $effect(() => {
    if (countdown === null) return;
    if (countdown <= 0) return untrack(() => go(nextVideo));
    const tick = setTimeout(() => (countdown -= 1), 1000);
    return () => clearTimeout(tick);
  });

  $effect(() => {
    const key = (e) => e.key === 'Escape' && !document.fullscreenElement && onclose();
    addEventListener('keydown', key);
    return () => removeEventListener('keydown', key);
  });
</script>

<div class="fixed inset-0 z-50 flex flex-col bg-bg" role="dialog" data-escape aria-label={d.label} use:portal transition:fade>
  <div class="flex items-center gap-3 border-b border-line px-3 pt-[max(var(--safe-top),0.75rem)] pb-3 md:px-4">
    <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Close')} onclick={onclose}><Icon name="arrow-left" /></button>
    <div class="grid min-w-0">
      <span class="truncate text-sm font-medium">{d.label}</span>
      {#if d.series}<span class="truncate text-xs text-fg-muted">{d.series}</span>{/if}
    </div>
    <SubtitlePicker tracks={subtitles} video={el} class="ml-auto shrink-0" />
    {#if nextVideo}
      <button type="button" class="btn btn-ghost shrink-0 {subtitles.length ? '' : 'ml-auto'}" onclick={() => go(nextVideo)}>
        <Icon name="skip-forward" /><span class="hidden sm:inline">{t('Next episode')}</span>
      </button>
    {/if}
  </div>
  <div class="relative grid min-h-0 flex-1 place-items-center bg-black pb-[var(--safe-bottom)]">
    {#if error}
      <div class="card grid max-w-sm place-items-center gap-1 px-6 py-8 text-center">
        <Icon name="circle-alert" class="mb-1 size-6 text-fg-muted" />
        <p class="font-medium">{t("This video can't be played here")}</p>
        <p class="text-[13px] text-fg-muted">{error}</p>
      </div>
    {:else if src}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video bind:this={el} {src} controls controlslist={inAndroidApp ? 'nofullscreen' : undefined} autoplay playsinline class="size-full object-contain" {ontimeupdate} {onloadedmetadata} {onended} onpause={save}>
        {#each subtitles as sub (sub.url)}<track kind="subtitles" src={sub.url} srclang={sub.lang || undefined} label={sub.label} />{/each}
      </video>
    {:else}
      <Icon name="loader-circle" class="spinner size-6 text-fg-faint" />
    {/if}

    {#if countdown !== null && nextVideo}
      <div class="card absolute right-4 bottom-20 grid w-72 gap-3 p-4" transition:fade>
        <div class="grid min-w-0 gap-0.5">
          <p class="text-xs text-fg-muted">{t('Up next in {count}', { count: countdown })}</p>
          <p class="truncate text-sm font-medium">{details(nextVideo).label}</p>
        </div>
        <div class="flex gap-2">
          <button type="button" class="btn btn-accent flex-1" onclick={() => go(nextVideo)}><Icon name="play" />{t('Play now')}</button>
          <button type="button" class="btn btn-secondary" onclick={() => (countdown = null)}>{t('Cancel')}</button>
        </div>
      </div>
    {/if}
  </div>
</div>
