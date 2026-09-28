<script>
  import { t } from '../../lib/i18n.svelte.js';
  import { incomingShares, deleteShare, download } from '../../lib/cloud.svelte.js';
  import { toast, toastError, trackTransfer, errorMessage } from '../../lib/ui.svelte.js';
  import { formatSize } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import Avatar from '../Avatar.svelte';
  import PersonName from '../PersonName.svelte';
  import Time from '../Time.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';
  import Menu from '../Menu.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';

  let { go } = $props();

  let shares = $state(null);
  let leaving = $state(null);

  async function load() {
    try {
      shares = await incomingShares();
    } catch (e) {
      toastError(e);
      shares = [];
    }
  }

  $effect(() => {
    load();
  });

  async function open(s) {
    if (s.node.kind === 'folder') return go({ name: 'files', folderId: s.node.id });
    const job = trackTransfer('download', s.entry.meta.name, s.entry.meta.size);
    try {
      await download(s.entry, (p) => (job.progress = p));
      job.status = 'done';
    } catch (e) {
      job.status = 'error';
      job.error = errorMessage(e);
    }
  }
</script>

<div>
  <h1 class="text-xl font-semibold tracking-tight">{t('Shared with me')}</h1>
  <p class="mt-1 text-[13px] text-fg-muted">{t('Files and folders other people have shared with you. Compare fingerprints with the owner to be sure a share really came from them.')}</p>
</div>

<div class="card mt-6 overflow-hidden">
  {#if shares === null}
    <div class="grid h-48 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
  {:else if !shares.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center">
      <img src="/img/logo.webp" alt="" width="715" height="349" class="mb-4 h-auto w-32 opacity-90 select-none" draggable="false" />
      <p class="font-medium">{t('Nothing shared with you yet')}</p>
      <p class="max-w-sm text-[13px] text-fg-muted">
        {t('When someone shares a file or folder with you, it shows up here. Tell them your username, and read them your key fingerprint from Settings so they can check it.')}
      </p>
    </div>
  {:else}
    <table class="table">
      <thead>
        <tr>
          <th>{t('Name')}</th>
          <th class="hidden md:table-cell">{t('Owner')}</th>
          <th class="hidden w-28 sm:table-cell">{t('Access')}</th>
          <th class="hidden w-32 lg:table-cell">{t('Shared')}</th>
          <th class="w-12"><span class="sr-only">{t('Actions')}</span></th>
        </tr>
      </thead>
      <tbody>
        {#each shares as s (s.id)}
          {@const folder = s.node.kind === 'folder'}
          <tr class="group">
            <td class="max-w-0">
              {#if s.error}
                <span class="flex items-center gap-3 text-fg-muted"><Icon name="circle-alert" class="size-4 text-danger" />{t("Couldn't decrypt this share")}</span>
              {:else}
                <button type="button" class="flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => open(s)}>
                  {#if folder}<FolderIcon name={s.entry.meta.name} />{:else}<FileIcon meta={s.entry.meta} />{/if}
                  <span class="truncate font-medium group-hover:underline group-hover:decoration-line-strong group-hover:underline-offset-4">{s.entry.meta.name}</span>
                  {#if !folder}<span class="hidden text-fg-faint sm:inline">{formatSize(s.entry.meta.size)}</span>{/if}
                </button>
              {/if}
            </td>
            <td class="hidden md:table-cell">
              <div class="flex items-center gap-2.5">
                <Avatar username={s.owner} class="size-7 text-xs" />
                <div class="min-w-0">
                  <p class="font-medium"><PersonName username={s.owner} /></p>
                  {#if s.ownerFingerprint}<p class="font-mono text-[11px] text-fg-faint" title={t("Owner's key fingerprint")}>{s.ownerFingerprint}</p>{/if}
                </div>
              </div>
            </td>
            <td class="hidden sm:table-cell">
              <span class="flex flex-wrap gap-1.5">
                <span class="badge {s.permission === 'write' ? 'badge-accent' : ''}">{s.permission === 'write' ? t('Can edit') : t('View only')}</span>
                {#if s.expires_at}<span class="badge"><Time ms={s.expires_at * 1000} prefix={t('Until') + ' '} /></span>{/if}
              </span>
            </td>
            <td class="hidden text-fg-muted lg:table-cell"><Time ms={s.created_at * 1000} /></td>
            <td class="text-right">
              <Menu
                label={t('Actions')}
                items={[
                  ...(s.error ? [] : [folder ? { label: t('Open'), icon: 'folder-open', onclick: () => open(s) } : { label: t('Download'), icon: 'download', onclick: () => open(s) }]),
                  { label: t('Remove from my list'), icon: 'door-open', danger: true, onclick: () => (leaving = s) },
                ]} />
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

{#if leaving}
  <ConfirmDialog
    title={t('Leave this share?')}
    description={t("You'll lose access to {name}. {owner} can share it with you again.", { name: leaving.entry?.meta.name ?? t('this item'), owner: leaving.owner })}
    confirmLabel={t('Leave')}
    danger
    onconfirm={async () => {
      await deleteShare(leaving.id);
      shares = shares.filter((x) => x.id !== leaving.id);
      toast(t('Removed from your shares'));
    }}
    onclose={() => (leaving = null)} />
{/if}
