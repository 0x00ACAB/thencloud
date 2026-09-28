<script>
  // Favourites and Recent: node ids from the encrypted app data, resolved
  // and decrypted here. Items that are gone are quietly dropped.
  import { places, loadPlaces, resolvePlaces, forgetPlaces, toggleFavourite } from '../../lib/places.svelte.js';
  import { toastError } from '../../lib/ui.svelte.js';
  import { formatSize, modifiedAt } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import Time from '../Time.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';

  let { mode, go } = $props(); // mode: 'favourites' | 'recent'

  let items = $state(null);

  const ids = $derived(mode === 'favourites' ? [...places.favourites].reverse() : places.recent.map((r) => r.id));
  const openedAt = $derived(new Map(places.recent.map((r) => [r.id, r.at])));

  $effect(() => {
    const wanted = ids;
    let live = true;
    loadPlaces()
      .then(() => resolvePlaces(wanted))
      .then((list) => {
        if (!live) return;
        items = list.filter((x) => x.entry);
        forgetPlaces(list.filter((x) => x.missing).map((x) => x.id));
      })
      .catch((e) => {
        toastError(e);
        items = [];
      });
    return () => (live = false);
  });

  function open(x) {
    if (x.entry.node.kind === 'folder') go({ name: 'files', folderId: x.id });
    else if (x.parentId) go({ name: 'files', folderId: x.parentId, open: x.id });
  }

  async function unstar(x) {
    try {
      await toggleFavourite(x.id);
    } catch (e) {
      toastError(e);
    }
  }

  const title = $derived(mode === 'favourites' ? 'Favourites' : 'Recent');
</script>

<div>
  <h1 class="text-xl font-semibold tracking-tight">{title}</h1>
  <p class="mt-1 text-[13px] text-fg-muted">
    {mode === 'favourites' ? 'Files and folders you starred.' : 'Files you opened lately, on any device.'} The list is encrypted, so the server can't tell which they are.
  </p>
</div>

<div class="card mt-6 overflow-hidden">
  {#if items === null}
    <div class="grid h-48 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
  {:else if !items.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center">
      <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle">
        <Icon name={mode === 'favourites' ? 'star' : 'clock'} class="size-5 text-fg-muted" />
      </div>
      <p class="font-medium">{mode === 'favourites' ? 'No favourites yet' : 'Nothing opened yet'}</p>
      <p class="text-[13px] text-fg-muted">
        {mode === 'favourites' ? 'Use "Add to favourites" in the menu on any file or folder.' : 'Files you preview or download show up here.'}
      </p>
    </div>
  {:else}
    <table class="table">
      <thead>
        <tr>
          <th>Name</th>
          <th class="hidden md:table-cell">Location</th>
          <th class="hidden w-28 text-right sm:table-cell">Size</th>
          <th class="hidden w-36 lg:table-cell">{mode === 'recent' ? 'Opened' : 'Modified'}</th>
          {#if mode === 'favourites'}<th class="w-12"><span class="sr-only">Actions</span></th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each items as x (x.id)}
          {@const folder = x.entry.node.kind === 'folder'}
          <tr class="group">
            <td class="max-w-0">
              <button type="button" class="flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => open(x)}>
                {#if folder}<FolderIcon name={x.entry.meta.name} />{:else}<FileIcon meta={x.entry.meta} />{/if}
                <span class="truncate font-medium group-hover:underline group-hover:decoration-line-strong group-hover:underline-offset-4">{x.entry.meta.name}</span>
              </button>
            </td>
            <td class="hidden max-w-0 md:table-cell">
              {#if x.parentId}
                <button type="button" class="block max-w-full cursor-pointer truncate text-fg-muted hover:text-fg" title={x.location.join(' / ')} onclick={() => go({ name: 'files', folderId: x.parentId })}>
                  {x.location.join(' / ')}
                </button>
              {/if}
            </td>
            <td class="hidden text-right text-fg-muted tabular-nums sm:table-cell">{folder ? '' : formatSize(x.entry.meta.size)}</td>
            <td class="hidden text-fg-muted lg:table-cell">
              {#if mode === 'recent' && openedAt.get(x.id)}<Time ms={openedAt.get(x.id)} relative />{:else}<Time ms={modifiedAt(x.entry)} relative />{/if}
            </td>
            {#if mode === 'favourites'}
              <td class="text-right">
                <button type="button" class="btn btn-ghost btn-icon" aria-label="Remove {x.entry.meta.name} from favourites" title="Remove from favourites" onclick={() => unstar(x)}>
                  <Icon name="star-off" />
                </button>
              </td>
            {/if}
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
