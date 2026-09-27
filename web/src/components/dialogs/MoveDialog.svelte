<script>
  // Folder picker for moving an item within the same tree (your own files,
  // or one shared folder). Moving re-wraps the item's key for the new parent.
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { listFolder, move } from '../../lib/cloud.svelte.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  let { entry, root, currentFolderId, onmoved, onclose } = $props();

  // Breadcrumb of folders being browsed: [{ id, key, name }].
  let trail = $state(untrack(() => [{ id: root.node.id, key: root.key, name: root.meta.name }]));
  let folders = $state([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');

  const here = $derived(trail[trail.length - 1]);
  const sameFolder = $derived(here.id === currentFolderId);

  $effect(() => {
    const { id, key } = here;
    loading = true;
    error = '';
    listFolder(id, key)
      .then((rows) => {
        if (here.id === id) folders = rows.filter((r) => r.node.kind === 'folder');
      })
      .catch((e) => (error = errorMessage(e)))
      .finally(() => (loading = false));
  });

  async function submit() {
    busy = true;
    error = '';
    try {
      await move(entry, here.id, here.key);
      onmoved(here.name);
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Move {entry.meta.name}" description="Choose a destination folder." {onclose} onsubmit={submit}>
  <div class="overflow-hidden rounded-md border border-line">
    <div class="flex h-9 items-center gap-1 overflow-x-auto border-b border-line bg-subtle px-2 text-[13px]">
      {#each trail as crumb, i (crumb.id)}
        {#if i}<Icon name="chevron-right" class="size-3.5 shrink-0 text-fg-faint" />{/if}
        <button
          type="button"
          class="shrink-0 cursor-pointer rounded px-1 py-0.5 {i === trail.length - 1 ? 'font-medium text-fg' : 'text-fg-muted hover:text-fg'}"
          onclick={() => (trail = trail.slice(0, i + 1))}>{crumb.name}</button>
      {/each}
    </div>
    <ul class="h-56 overflow-y-auto p-1">
      {#if loading}
        <li class="grid h-full place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner" /></li>
      {:else if !folders.length}
        <li class="grid h-full place-items-center text-[13px] text-fg-muted">No folders in here</li>
      {:else}
        {#each folders as f (f.node.id)}
          {@const self = f.node.id === entry.node.id}
          <li>
            <button
              type="button"
              class="flex h-8 w-full cursor-pointer items-center gap-2 rounded-md px-2 text-left text-sm hover:bg-muted disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent"
              disabled={self}
              title={self ? "You can't move a folder into itself" : undefined}
              onclick={() => (trail = [...trail, { id: f.node.id, key: f.key, name: f.meta.name }])}>
              <Icon name="folder" class="size-4 text-fg-muted" />
              <span class="flex-1 truncate">{f.meta.name}</span>
              <Icon name="chevron-right" class="size-4 text-fg-faint" />
            </button>
          </li>
        {/each}
      {/if}
    </ul>
  </div>
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
    <button class="btn btn-primary" disabled={busy || sameFolder || loading}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      Move here
    </button>
  {/snippet}
</Modal>
