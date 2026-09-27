<script>
  // Edit an album's name, artist and cover. The name and artist are kept in
  // your encrypted library data; the cover goes into the album's folder as
  // cover.jpg, encrypted like any file.
  import { untrack, onDestroy } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { albumInfo, editAlbum, setAlbumCover, covers } from '../../lib/music.svelte.js';
  import { imageError } from '../../lib/cover.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  let { album, onclose } = $props();

  const now = untrack(() => albumInfo(album));
  let name = $state(now.name);
  let artist = $state(now.artist ?? '');
  let file = $state(null);
  let picked = $state(null); // blob: URL of the new cover
  let busy = $state(false);
  let error = $state('');

  const shown = $derived(picked ?? covers.get(album.id));

  function pick(e) {
    const f = e.currentTarget.files?.[0];
    e.currentTarget.value = '';
    if (!f) return;
    if (picked) URL.revokeObjectURL(picked);
    file = f;
    picked = URL.createObjectURL(f);
  }

  onDestroy(() => picked && URL.revokeObjectURL(picked));

  async function submit() {
    busy = true;
    error = '';
    try {
      if (file) await setAlbumCover(album, file);
      await editAlbum(album, { name: name.trim(), artist: artist.trim() });
      onclose();
    } catch (e) {
      error = errorMessage(imageError(e));
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Edit album" description="The name and artist are kept in your library, encrypted. A new cover is saved as cover.jpg in the album's folder." {onclose} onsubmit={submit}>
  <div class="flex items-center gap-4">
    <div class="grid size-24 shrink-0 place-items-center overflow-hidden rounded-md border border-line bg-muted text-fg-faint">
      {#if shown}<img src={shown} alt="" class="size-full object-cover" />{:else}<Icon name="disc-3" class="size-1/3" strokeWidth={1.5} />{/if}
    </div>
    <label class="btn btn-secondary cursor-pointer">
      <Icon name="image" />Choose cover
      <input type="file" accept="image/*" class="sr-only" onchange={pick} />
    </label>
  </div>
  <div class="field">
    <label class="label" for="album-name">Name</label>
    <input id="album-name" class="input" bind:value={name} placeholder={album.name} autocomplete="off" />
  </div>
  <div class="field">
    <label class="label" for="album-artist">Artist</label>
    <input id="album-artist" class="input" bind:value={artist} placeholder="Unknown artist" autocomplete="off" />
  </div>
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
    <button class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      Save
    </button>
  {/snippet}
</Modal>
