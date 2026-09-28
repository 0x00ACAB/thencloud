<script>
  import { t } from '../../lib/i18n.svelte.js';
  // Folder picker for moving items within the same tree (your own files,
  // or one shared folder). Moving re-wraps each item's key for the new parent.
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { listFolder, move, session } from '../../lib/cloud.svelte.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  /** `onmoved(destinationName, movedCount)` */
  let { entries, root, currentFolderId, onmoved, onclose } = $props();

  const moving = $derived(new Set(entries.map((e) => e.node.id)));
  const title = $derived(entries.length === 1 ? t('Move {name}', { name: entries[0].meta.name }) : t('Move {count} items', { count: entries.length }));

  // Breadcrumb of folders being browsed: [{ id, key, name }].
  let trail = $state(untrack(() => [{ id: root.node.id, key: root.key, name: root.node.id === session.me.keys.root_node_id ? t('My files') : root.meta.name }]));
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
    let done = 0;
    const failed = [];
    for (const entry of entries) {
      try {
        await move(entry, here.id, here.key);
        done++;
      } catch (e) {
        failed.push(`${entry.meta.name}: ${errorMessage(e)}`);
      }
    }
    busy = false;
    if (done) onmoved(here.name, done);
    if (!failed.length) return onclose();
    error = done ? `${t('Moved {done} of {total}.', { done, total: entries.length })} ${failed.join(' ')}` : failed.join(' ');
  }
</script>

<Modal {title} description={t('Choose a destination folder.')} {onclose} onsubmit={submit}>
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
        <li class="grid h-full place-items-center text-[13px] text-fg-muted">{t('No folders in here')}</li>
      {:else}
        {#each folders as f (f.node.id)}
          {@const self = moving.has(f.node.id)}
          <li>
            <button
              type="button"
              class="flex h-8 w-full cursor-pointer items-center gap-2 rounded-md px-2 text-left text-sm hover:bg-muted disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent"
              disabled={self}
              title={self ? t("You can't move a folder into itself") : undefined}
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
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
    <button class="btn btn-primary" disabled={busy || sameFolder || loading}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {t('Move here')}
    </button>
  {/snippet}
</Modal>
