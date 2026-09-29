<script>
  import { t } from '../../lib/i18n.svelte.js';
  // Favourites and Recent: node ids from the encrypted app data, resolved
  // and decrypted here. Items that are gone are quietly dropped.
  import { places, loadPlaces, resolvePlaces, forgetPlaces, toggleFavourite, allTags, taggedWith } from '../../lib/places.svelte.js';
  import { toastError } from '../../lib/ui.svelte.js';
  import { formatSize, changedAt } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import Time from '../Time.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';

  let { mode, go } = $props(); // mode: 'favourites' | 'recent' | 'tags'

  let items = $state(null);
  // Tags: which one is picked. Kept here, not in the address: only ids go there.
  let tag = $state(null);
  const tags = $derived(mode === 'tags' ? allTags() : []);
  const picked = $derived(tag && tags.some((x) => x.tag === tag) ? tag : (tags[0]?.tag ?? null));

  const ids = $derived(
    mode === 'favourites' ? [...places.favourites].reverse() : mode === 'tags' ? (picked ? taggedWith(picked) : []) : places.recent.map((r) => r.id),
  );
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

  const title = $derived(mode === 'favourites' ? t('Favourites') : mode === 'tags' ? t('Tags') : t('Recent'));
</script>

<div>
  <h1 class="text-xl font-semibold tracking-tight">{title}</h1>
  <p class="mt-1 text-[13px] text-fg-muted">
    {mode === 'favourites' ? t('Files and folders you starred.') : mode === 'tags' ? t('Files and folders by the tags you gave them. Only you see your tags.') : t('Files you opened lately, on any device.')}
  </p>
</div>

{#if mode === 'tags' && tags.length}
  <div class="mt-5 flex flex-wrap gap-1.5" role="radiogroup" aria-label={t('Tags')}>
    {#each tags as x (x.tag)}
      <button
        type="button"
        role="radio"
        aria-checked={picked === x.tag}
        class="flex h-7 cursor-pointer items-center gap-1.5 rounded-md border px-2.5 text-[13px] transition-colors {picked === x.tag
          ? 'border-accent bg-accent-soft font-medium text-accent-text'
          : 'border-line text-fg-muted hover:bg-subtle hover:text-fg'}"
        onclick={() => (tag = x.tag)}>{x.tag}<span class="text-xs tabular-nums opacity-80">{x.n}</span></button>
    {/each}
  </div>
{/if}

<div class="card mt-6 overflow-hidden">
  {#if items === null}
    <div class="grid h-48 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
  {:else if !items.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center">
      <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle">
        <Icon name={mode === 'favourites' ? 'star' : mode === 'tags' ? 'tag' : 'clock'} class="size-5 text-fg-muted" />
      </div>
      <p class="font-medium">{mode === 'favourites' ? t('No favourites yet') : mode === 'tags' ? t('No tags yet') : t('Nothing opened yet')}</p>
      <p class="text-[13px] text-fg-muted">
        {mode === 'favourites' ? t('Use "Add to favourites" in the menu on any file or folder.') : mode === 'tags' ? t('Use "Tags" in the menu on any file or folder.') : t('Files you preview or download show up here.')}
      </p>
    </div>
  {:else}
    <table class="table">
      <thead>
        <tr>
          <th>{t('Name')}</th>
          <th class="hidden md:table-cell">{t('Location')}</th>
          <th class="hidden w-28 text-right sm:table-cell">{t('Size')}</th>
          <th class="hidden w-36 lg:table-cell">{mode === 'recent' ? t('Opened') : t('Modified')}</th>
          {#if mode === 'favourites'}<th class="w-12"><span class="sr-only">{t('Actions')}</span></th>{/if}
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
              {#if mode === 'recent' && openedAt.get(x.id)}<Time ms={openedAt.get(x.id)} relative />{:else}<Time ms={changedAt(x.entry)} relative />{/if}
            </td>
            {#if mode === 'favourites'}
              <td class="text-right">
                <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Remove {name} from favourites', { name: x.entry.meta.name })} title={t('Remove from favourites')} onclick={() => unstar(x)}>
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
