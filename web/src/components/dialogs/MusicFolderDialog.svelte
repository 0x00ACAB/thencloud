<script>
  // Pick the folder the music library is read from.
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { listFolder } from '../../lib/cloud.svelte.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  /** `onpick(id)` */
  let { root, onpick, onclose } = $props();

  let trail = $state(untrack(() => [{ id: root.node.id, key: root.key, name: root.meta.name }]));
  let folders = $state([]);
  let loading = $state(true);
  let error = $state('');

  const here = $derived(trail[trail.length - 1]);

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

  function submit() {
    onpick(here.id);
    onclose();
  }
</script>

<Modal title="Music folder" description="Everything under this folder shows up in Music, grouped into albums by folder." {onclose} onsubmit={submit}>
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
          <li>
            <button
              type="button"
              class="flex h-8 w-full cursor-pointer items-center gap-2 rounded-md px-2 text-left text-sm hover:bg-muted"
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
    <button class="btn btn-primary" disabled={loading}>Use {here.name}</button>
  {/snippet}
</Modal>
