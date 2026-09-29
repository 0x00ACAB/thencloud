<script>
  // Which subtitle track a <video> shows. The tracks are its <track>
  // children, in the same order as `tracks`; the browser's own captions
  // menu, where there is one, stays in step with this.
  import Icon from './Icon.svelte';
  import { preferredTrack } from '../lib/subtitles.js';
  import { t } from '../lib/i18n.svelte.js';

  let { tracks, video, class: cls = '' } = $props();

  let selected = $state(-1);

  // A new set of tracks starts in the reader's language, or off.
  $effect(() => {
    selected = preferredTrack(tracks);
  });

  $effect(() => {
    if (!video) return;
    const list = video.textTracks;
    const apply = () => {
      for (let i = 0; i < list.length; i++) list[i].mode = i === selected ? 'showing' : 'disabled';
    };
    apply();
    // Tracks can arrive after this runs.
    list.addEventListener('addtrack', apply);
    const sync = () => {
      const i = [...list].findIndex((t) => t.mode === 'showing');
      if (i !== selected) selected = i;
    };
    list.addEventListener('change', sync);
    return () => {
      list.removeEventListener('addtrack', apply);
      list.removeEventListener('change', sync);
    };
  });
</script>

{#if tracks.length}
  <label class="relative flex items-center {cls}">
    <span class="sr-only">{t('Subtitles')}</span>
    <Icon name="captions" class="pointer-events-none absolute left-2.5 size-4 text-fg-muted" />
    <select class="input h-8 w-auto max-w-44 pl-8 text-[13px]" bind:value={selected} title={t('Subtitles')}>
      <option value={-1}>{t('Subtitles off')}</option>
      {#each tracks as t, i (t.url)}<option value={i}>{t.label}</option>{/each}
    </select>
  </label>
{/if}
