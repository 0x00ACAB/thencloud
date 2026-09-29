<script>
  // Edit how a track shows up in Music. Saved in your encrypted library
  // data; the file itself is left alone.
  import { untrack } from 'svelte';
  import { t } from '../../lib/i18n.svelte.js';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { info, editTrack, resetTrack, isEdited } from '../../lib/music.svelte.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  let { track, onclose } = $props();

  const now = untrack(() => info(track));
  let title = $state(now.title);
  let artist = $state(now.artist === t('Unknown artist') ? '' : now.artist);
  let album = $state(now.album);
  let trackNo = $state(now.trackNo ?? '');
  let busy = $state(false);
  let error = $state('');

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

  const submit = () =>
    run(() => editTrack(track, { title: title.trim(), artist: artist.trim(), album: album.trim(), trackNo: trackNo === '' || trackNo == null ? null : +trackNo }));
</script>

<Modal title={t('Edit track')} description={t("Changes are kept in your library. The file isn't changed.")} {onclose} onsubmit={submit}>
  <div class="field">
    <label class="label" for="track-title">{t('Title')}</label>
    <input id="track-title" class="input" bind:value={title} autocomplete="off" />
  </div>
  <div class="field">
    <label class="label" for="track-artist">{t('Artist')}</label>
    <input id="track-artist" class="input" bind:value={artist} autocomplete="off" />
  </div>
  <div class="grid grid-cols-[1fr_6rem] gap-3">
    <div class="field">
      <label class="label" for="track-album">{t('Album')}</label>
      <input id="track-album" class="input" bind:value={album} autocomplete="off" />
    </div>
    <div class="field">
      <label class="label" for="track-no">{t('Track')}</label>
      <input id="track-no" class="input" type="number" min="0" max="999" bind:value={trackNo} />
    </div>
  </div>
  <p class="hint">{t('File: {name}', { name: track.entry.meta.name })}</p>
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    {#if isEdited(track)}
      <button type="button" class="btn btn-ghost mr-auto" disabled={busy} onclick={() => run(() => resetTrack(track))}>{t('Undo edits')}</button>
    {/if}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
    <button class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {t('Save')}
    </button>
  {/snippet}
</Modal>
