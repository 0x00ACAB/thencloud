<script>
  // Add tracks to one of your playlists, or to a new one.
  import { t } from '../../lib/i18n.svelte.js';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { saved, addToPlaylist, createPlaylist } from '../../lib/music.svelte.js';
  import { toast, errorMessage } from '../../lib/ui.svelte.js';

  let { tracks, onclose } = $props();

  let name = $state('');
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

  const add = (p) =>
    run(async () => {
      const n = await addToPlaylist(p.id, tracks);
      toast(n ? t('Added {count} tracks to {name}', { count: n, name: p.name }) : t('Already in {name}', { name: p.name }));
    });

  function create() {
    const n = name.trim();
    if (!n) return;
    run(async () => {
      await createPlaylist(n, tracks);
      toast(t('Made the playlist {name}', { name: n }));
    });
  }
</script>

<Modal title={t('Add to playlist')} description={tracks.length === 1 ? undefined : t('{count} tracks', { count: tracks.length })} {onclose} onsubmit={create}>
  {#if saved.value.playlists.length}
    <ul class="max-h-56 overflow-y-auto rounded-md border border-line p-1">
      {#each saved.value.playlists as p (p.id)}
        <li>
          <button type="button" class="flex h-8 w-full cursor-pointer items-center gap-2 rounded-md px-2 text-left text-sm hover:bg-muted" disabled={busy} onclick={() => add(p)}>
            <Icon name="list-music" class="size-4 text-fg-muted" />
            <span class="flex-1 truncate">{p.name}</span>
            <span class="text-xs text-fg-muted tabular-nums">{p.tracks.length}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <div class="field">
    <label class="label" for="playlist-name">{t('New playlist')}</label>
    <input id="playlist-name" class="input" bind:value={name} placeholder={t('Name')} autocomplete="off" />
  </div>
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
    <button class="btn btn-primary" disabled={busy || !name.trim()}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {t('Create')}
    </button>
  {/snippet}
</Modal>
