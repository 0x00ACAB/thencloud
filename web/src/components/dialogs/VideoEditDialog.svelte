<script>
  // Edit how a video shows up in Videos: its title, and the series, season
  // and episode it belongs to (none for a movie). Kept in your encrypted
  // library data; the file isn't changed.
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { details, editVideo, resetVideo, isEdited } from '../../lib/videos.svelte.js';
  import { episodeLabel } from '../../lib/episodes.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  let { video, onclose } = $props();

  const now = untrack(() => details(video));
  let title = $state(now.title);
  let series = $state(now.series);
  let season = $state(now.season ?? '');
  let episode = $state(now.episode ?? '');
  let busy = $state(false);
  let error = $state('');

  const num = (v) => (v === '' || v == null ? null : +v);
  const fields = $derived({ title: title.trim(), series: series.trim(), season: num(season) ?? (num(episode) == null ? null : 1), episode: num(episode) });
  const shown = $derived(fields.episode == null ? fields.title : episodeLabel(fields));

  async function run(fn) {
    busy = true;
    error = '';
    try {
      await fn();
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Edit video" description="Changes are kept in your library, encrypted. The file isn't changed." {onclose} onsubmit={() => run(() => editVideo(video, fields))}>
  <div class="field">
    <label class="label" for="video-title">{fields.episode == null ? 'Title' : 'Episode name'}</label>
    <input id="video-title" class="input" bind:value={title} autocomplete="off" />
  </div>
  <div class="field">
    <label class="label" for="video-series">Series</label>
    <input id="video-series" class="input" bind:value={series} placeholder="None, it's a movie" autocomplete="off" />
  </div>
  <div class="grid grid-cols-2 gap-3">
    <div class="field">
      <label class="label" for="video-season">Season</label>
      <input id="video-season" class="input" type="number" min="0" max="999" bind:value={season} placeholder="1" />
    </div>
    <div class="field">
      <label class="label" for="video-episode">Episode</label>
      <input id="video-episode" class="input" type="number" min="0" max="9999" bind:value={episode} />
    </div>
  </div>
  <p class="hint">Leave the episode empty for a movie. Shown as <span class="font-medium text-fg">{shown || video.entry.meta.name}</span></p>
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    {#if isEdited(video)}
      <button type="button" class="btn btn-ghost mr-auto" disabled={busy} onclick={() => run(() => resetVideo(video))}>Undo edits</button>
    {/if}
    <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
    <button class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      Save
    </button>
  {/snippet}
</Modal>
