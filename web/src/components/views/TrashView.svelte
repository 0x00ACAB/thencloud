<script>
  import { t } from '../../lib/i18n.svelte.js';
  import { onMount } from 'svelte';
  import { session, trashItems, restoreFromTrash, purgeFromTrash, emptyTrash, refreshMe } from '../../lib/cloud.svelte.js';
  import { toast, toastError } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, fullDate } from '../../lib/format.js';
  import { fade, flip, flipParams } from '../../lib/motion.js';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import Menu from '../Menu.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';

  let { go } = $props();

  let items = $state(null);
  let dialog = $state(null); // { type: 'purge', item } | { type: 'empty' }

  async function load() {
    try {
      items = await trashItems();
    } catch (e) {
      toastError(e);
      items = [];
    }
  }

  onMount(load);

  const daysLeft = (it) => Math.max(0, Math.ceil((it.trashed_at + session.me.trash_days * 86400 - Date.now() / 1000) / 86400));

  async function restore(it) {
    try {
      const into = await restoreFromTrash(it);
      items = items.filter((x) => x.node.id !== it.node.id);
      const name = it.entry.meta.name;
      toast(into ? t('Restored {name} to {folder}, because its folder is in the trash', { name, folder: into }) : t('Restored {name}', { name }), {
        kind: 'success',
        action: { label: t('Show'), onclick: () => go({ name: 'files', folderId: it.entry.node.parent_id && !into ? it.entry.node.parent_id : session.me.keys.root_node_id }) },
      });
    } catch (e) {
      toastError(e);
    }
  }
</script>

<div class="flex flex-wrap items-start gap-x-4 gap-y-3">
  <div class="min-w-0 flex-1">
    <h1 class="text-xl font-semibold tracking-tight">{t('Trash')}</h1>
    <p class="mt-1 text-[13px] text-fg-muted">
      {t("Deleted items stay here for {count} days, then they're gone for good. They still count toward your storage until then.", { count: session.me.trash_days })}
    </p>
  </div>
  {#if items?.length}
    <button type="button" class="btn btn-secondary" onclick={() => (dialog = { type: 'empty' })}>
      <Icon name="trash-2" /> {t('Empty trash')}
    </button>
  {/if}
</div>

<div class="card mt-6 overflow-hidden">
  {#if items === null}
    <div aria-hidden="true">
      {#each [0, 1, 2] as i (i)}
        <div class="flex h-12 items-center gap-3 border-b border-line px-4 last:border-b-0">
          <div class="skeleton size-4"></div>
          <div class="skeleton h-3.5 w-48"></div>
        </div>
      {/each}
    </div>
  {:else if !items.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center animate-enter">
      <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle"><Icon name="trash-2" class="size-5 text-fg-muted" /></div>
      <p class="font-medium">{t('Trash is empty')}</p>
      <p class="text-[13px] text-fg-muted">{t('Things you delete show up here, in case you change your mind.')}</p>
    </div>
  {:else}
    <table class="table animate-enter">
      <thead>
        <tr>
          <th>{t('Name')}</th>
          <th class="hidden md:table-cell">{t('Was in')}</th>
          <th class="hidden w-36 sm:table-cell">{t('Deleted')}</th>
          <th class="w-28"><span class="sr-only">{t('Actions')}</span></th>
        </tr>
      </thead>
      <tbody>
        {#each items as it (it.node.id)}
          {@const folder = it.node.kind === 'folder'}
          <tr out:fade={{ duration: 120 }} animate:flip={flipParams()}>
            <td class="max-w-0">
              {#if it.error}
                <span class="flex items-center gap-3 text-fg-muted"><Icon name="circle-alert" class="size-4 text-danger" />{t("Couldn't decrypt this item")}</span>
              {:else}
                <span class="flex items-center gap-3">
                  {#if folder}<Icon name="folder" class="size-4 shrink-0 text-fg-faint" />{:else}<FileIcon meta={it.entry.meta} minimalClass="text-fg-faint" />{/if}
                  <span class="truncate font-medium">{it.entry.meta.name}</span>
                  {#if !folder}<span class="hidden text-fg-faint sm:inline">{formatSize(it.entry.meta.size)}</span>{/if}
                </span>
              {/if}
            </td>
            <td class="hidden max-w-0 truncate text-fg-muted md:table-cell">{it.location?.join(' / ') ?? ''}</td>
            <td class="hidden sm:table-cell">
              <p class="text-fg-muted" title={fullDate(it.trashed_at * 1000)}>{formatWhen(it.trashed_at * 1000)}</p>
              <p class="text-xs text-fg-faint">
                {t('{count} days left', { count: daysLeft(it) })}{it.trashed_by && it.trashed_by !== session.me.username ? ` · ${t('by {name}', { name: it.trashed_by })}` : ''}
              </p>
            </td>
            <td class="text-right whitespace-nowrap">
              <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" disabled={!!it.error} onclick={() => restore(it)}>{t('Restore')}</button>
              <Menu
                label={t('More actions')}
                items={[{ label: t('Delete permanently'), icon: 'trash-2', danger: true, onclick: () => (dialog = { type: 'purge', item: it }) }]} />
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

{#if items?.length}
  <p class="mt-3 px-1 text-xs text-fg-faint">{t('{count} items', { count: items.length })}</p>
{/if}

{#if dialog?.type === 'purge'}
  <ConfirmDialog
    title={dialog.item.entry ? t('Delete {name} permanently?', { name: dialog.item.entry.meta.name }) : t('Delete this item permanently?')}
    description={dialog.item.node.kind === 'folder' ? t("This can't be undone. Everything in the folder goes with it.") : t("This can't be undone.")}
    confirmLabel={t('Delete permanently')}
    danger
    onconfirm={async () => {
      await purgeFromTrash(dialog.item.node.id);
      items = items.filter((x) => x.node.id !== dialog.item.node.id);
      refreshMe().catch(() => {});
    }}
    onclose={() => (dialog = null)} />
{:else if dialog?.type === 'empty'}
  <ConfirmDialog
    title={t('Empty the trash?')}
    description={t("{count} items will be deleted permanently. This can't be undone.", { count: items.length })}
    confirmLabel={t('Empty trash')}
    danger
    onconfirm={async () => {
      await emptyTrash();
      items = [];
      refreshMe().catch(() => {});
      toast(t('Trash emptied'));
    }}
    onclose={() => (dialog = null)} />
{/if}
