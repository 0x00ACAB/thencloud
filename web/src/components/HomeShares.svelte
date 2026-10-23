<script>
  // Folders shared with us that were picked to show in My files ("Show in
  // My files" in Shared with me). Kept apart from the folder's own rows, so
  // selecting, dragging and moving there never reach someone else's folder.
  import { t } from '../lib/i18n.svelte.js';
  import { incomingShares } from '../lib/cloud.svelte.js';
  import { places, toggleHome } from '../lib/places.svelte.js';
  import { toast, toastError } from '../lib/ui.svelte.js';
  import Icon from './Icon.svelte';
  import FolderIcon from './FolderIcon.svelte';
  import PersonName from './PersonName.svelte';
  import Menu from './Menu.svelte';
  import Sentence from './Sentence.svelte';

  let { go } = $props();

  let shares = $state([]);
  const asked = $derived(places.home.length > 0);

  $effect(() => {
    if (!asked) return;
    incomingShares().then(
      (list) => (shares = list.filter((s) => !s.error && s.node.kind === 'folder')),
      () => {},
    );
  });

  // In the order they were added; a share that's gone just isn't shown.
  const shown = $derived(places.home.map((id) => shares.find((s) => s.node.id === id)).filter(Boolean));

  const open = (s) => go({ name: 'files', folderId: s.node.id });

  async function unpin(s) {
    try {
      await toggleHome(s.node.id);
      toast(t('{name} no longer shows in My files', { name: s.entry.meta.name }), { icon: 'pin-off' });
    } catch (e) {
      toastError(e);
    }
  }
</script>

{#if shown.length}
  <section class="card mt-6 overflow-hidden" aria-label={t('Shared with you')}>
    <ul>
      {#each shown as s (s.node.id)}
        <li class="group flex h-12 items-center gap-4 border-b border-line pr-2 pl-4 last:border-b-0">
          <button type="button" class="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left" onclick={() => open(s)}>
            <FolderIcon name={s.entry.meta.name} />
            <span class="truncate font-medium group-hover:underline group-hover:decoration-line-strong group-hover:underline-offset-4">{s.entry.meta.name}</span>
          </button>
          <span class="flex min-w-0 shrink items-center gap-1.5 text-fg-muted"
            ><Icon name="users" class="size-3.5 shrink-0" /><span class="hidden truncate md:inline"
              ><Sentence text={t('Shared by {owner}')}>{#snippet owner()}<PersonName username={s.owner} />{/snippet}</Sentence></span
            ></span>
          <span class="badge hidden shrink-0 sm:inline-flex {s.permission === 'write' ? 'badge-accent' : ''}">{s.permission === 'write' ? t('Can edit') : t('View only')}</span>
          <Menu
            label={t('Actions for {name}', { name: s.entry.meta.name })}
            items={[
              { label: t('Open'), icon: 'folder-open', onclick: () => open(s) },
              { label: t('Remove from My files'), icon: 'pin-off', onclick: () => unpin(s) },
            ]} />
        </li>
      {/each}
    </ul>
  </section>
{/if}
