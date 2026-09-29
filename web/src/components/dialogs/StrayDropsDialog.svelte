<script>
  // Files dropped through a link that no longer exists (or whose key doesn't
  // open). They aren't taken in automatically, since anyone with the old
  // link, or the server itself, could have put them there.
  import { t } from '../../lib/i18n.svelte.js';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import { strayDrops, keepStrayDrop, deleteStrayDrop } from '../../lib/cloud.svelte.js';
  import { formatSize } from '../../lib/format.js';
  import { toastError } from '../../lib/ui.svelte.js';

  let { onclose, onchanged } = $props();
  let busy = $state(null);

  async function act(item, fn) {
    busy = item;
    try {
      await fn(item);
      onchanged?.();
      if (!strayDrops.list.length) onclose();
    } catch (e) {
      toastError(e);
    } finally {
      busy = null;
    }
  }
</script>

<Modal
  title={t('Dropped files waiting for you')}
  description={t('These came in through a file drop link that has since expired or been deleted. Keep only files you expect: anyone with the old link could have added them.')}
  {onclose}
  class="max-w-lg">
  <ul class="max-h-80 divide-y divide-line overflow-y-auto rounded-md border border-line">
    {#each strayDrops.list as item (item.drop.node.id)}
      <li class="flex items-center gap-3 px-3 py-2.5">
        {#if item.meta}<FileIcon meta={item.meta} />{:else}<Icon name="circle-alert" class="size-4 shrink-0 text-danger" />{/if}
        <div class="min-w-0 flex-1">
          <p class="truncate text-sm font-medium">{item.meta?.name ?? t("Can't be opened")}</p>
          <p class="truncate text-xs text-fg-muted">{item.meta ? formatSize(item.meta.size) : t('Its key is damaged or not meant for you.')}</p>
        </div>
        {#if item.meta && item.folder}
          <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" disabled={busy !== null} onclick={() => act(item, keepStrayDrop)}>
            {#if busy === item}<Icon name="loader-circle" class="spinner" />{/if}{t('Keep')}
          </button>
        {/if}
        <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Delete')} title={t('Delete')} disabled={busy !== null} onclick={() => act(item, deleteStrayDrop)}>
          <Icon name="trash-2" />
        </button>
      </li>
    {/each}
  </ul>
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Close')}</button>
  {/snippet}
</Modal>
