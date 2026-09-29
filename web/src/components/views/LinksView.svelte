<script>
  import { t } from '../../lib/i18n.svelte.js';
  import { links, deleteLink } from '../../lib/cloud.svelte.js';
  import { toast, toastError, copyText } from '../../lib/ui.svelte.js';
  import Icon from '../Icon.svelte';
  import Time from '../Time.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';

  let { go } = $props();

  let list = $state(null);
  let deleting = $state(null);

  $effect(() => {
    links()
      .then((l) => (list = l))
      .catch((e) => {
        toastError(e);
        list = [];
      });
  });

  function open(l) {
    const n = l.entry?.node;
    if (!n) return;
    go({ name: 'files', folderId: n.kind === 'folder' ? n.id : n.parent_id });
  }
</script>

<div>
  <h1 class="text-xl font-semibold tracking-tight">{t('Public links')}</h1>
  <p class="mt-1 text-[13px] text-fg-muted">
    Anyone with one of these links can open what it points to. The key is in the part after <code class="font-mono text-fg">#</code>, which never reaches the server.
  </p>
</div>

<div class="card mt-6 overflow-hidden">
  {#if list === null}
    <div class="grid h-48 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
  {:else if !list.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center">
      <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle"><Icon name="link" class="size-5 text-fg-muted" /></div>
      <p class="font-medium">{t('No public links')}</p>
      <p class="text-[13px] text-fg-muted">{t('Create one from the menu on any file or folder.')}</p>
    </div>
  {:else}
    <table class="table">
      <thead>
        <tr>
          <th>{t('Name')}</th>
          <th class="hidden sm:table-cell">{t('Protection')}</th>
          <th class="hidden w-32 md:table-cell">{t('Created')}</th>
          <th class="w-24"><span class="sr-only">{t('Actions')}</span></th>
        </tr>
      </thead>
      <tbody>
        {#each list as l (l.id)}
          {@const folder = l.entry?.node.kind === 'folder'}
          <tr class="group">
            <td class="max-w-0">
              <button type="button" class="flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => open(l)}>
                {#if folder}<FolderIcon name={l.entry?.meta.name} />{:else if l.entry}<FileIcon meta={l.entry.meta} />{:else}<Icon name="file" class="size-4 shrink-0 text-fg-muted" />{/if}
                <span class="truncate font-medium group-hover:underline group-hover:decoration-line-strong group-hover:underline-offset-4">{l.entry?.meta.name ?? t('Unavailable')}</span>
              </button>
            </td>
            <td class="hidden sm:table-cell">
              <span class="flex flex-wrap gap-1.5">
                {#if l.upload_only}<span class="badge"><Icon name="inbox" />{t('File drop')}</span>{/if}
                {#if l.has_password}<span class="badge"><Icon name="lock" />{t('Password')}</span>{/if}
                {#if l.expires_at}<span class="badge"><Time ms={l.expires_at * 1000} prefix={t('Expires') + ' '} /></span>{/if}
                {#if l.max_opens}<span class="badge {l.opens >= l.max_opens ? 'text-fg-faint' : ''}"><Icon name="eye" />{l.opens >= l.max_opens ? t('Used up') : t('{count} of {max} opens', { count: l.opens, max: l.max_opens })}</span>{/if}
                {#if !l.has_password && !l.expires_at && !l.upload_only && !l.max_opens}<span class="text-[13px] text-fg-faint">{t('None')}</span>{/if}
              </span>
            </td>
            <td class="hidden text-fg-muted md:table-cell"><Time ms={l.created_at * 1000} /></td>
            <td class="text-right whitespace-nowrap">
              <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Copy link')} title={t('Copy link')} disabled={!l.url} onclick={() => copyText(l.url, t('Link copied'))}>
                <Icon name="copy" />
              </button>
              <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Delete link')} title={t('Delete link')} onclick={() => (deleting = l)}>
                <Icon name="trash-2" />
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

{#if deleting}
  <ConfirmDialog
    title={t('Delete this link?')}
    description={deleting.entry?.node.kind === 'folder' ? t('The link stops working immediately. The folder itself is not affected.') : t('The link stops working immediately. The file itself is not affected.')}
    confirmLabel={t('Delete link')}
    danger
    onconfirm={async () => {
      await deleteLink(deleting.id);
      list = list.filter((x) => x.id !== deleting.id);
      toast(t('Link deleted'));
    }}
    onclose={() => (deleting = null)} />
{/if}
