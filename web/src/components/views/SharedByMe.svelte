<script>
  import { outgoingShares, setSharePermission, deleteShare } from '../../lib/cloud.svelte.js';
  import { toast, toastError } from '../../lib/ui.svelte.js';
  import Icon from '../Icon.svelte';
  import Avatar from '../Avatar.svelte';
  import PersonName from '../PersonName.svelte';
  import Time from '../Time.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';

  let { go } = $props();

  let shares = $state(null);
  let revoking = $state(null);

  $effect(() => {
    outgoingShares()
      .then((s) => (shares = s))
      .catch((e) => {
        toastError(e);
        shares = [];
      });
  });

  async function changePermission(s, value) {
    try {
      await setSharePermission(s.id, value);
      s.permission = value;
      toast(`${s.recipient} can now ${value === 'write' ? 'edit' : 'only view'}`);
    } catch (e) {
      toastError(e);
    }
  }

  function open(s) {
    if (!s.entry) return;
    if (s.entry.node.kind === 'folder') go({ name: 'files', folderId: s.entry.node.id });
    else if (s.entry.node.parent_id) go({ name: 'files', folderId: s.entry.node.parent_id });
  }
</script>

<div>
  <h1 class="text-xl font-semibold tracking-tight">Shared by me</h1>
  <p class="mt-1 text-[13px] text-fg-muted">Everyone who can decrypt something of yours.</p>
</div>

<div class="card mt-6 overflow-hidden">
  {#if shares === null}
    <div class="grid h-48 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
  {:else if !shares.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center">
      <div class="mb-3 grid size-11 place-items-center rounded-lg border border-line bg-subtle"><Icon name="users" class="size-5 text-fg-muted" /></div>
      <p class="font-medium">You haven't shared anything</p>
      <p class="text-[13px] text-fg-muted">Use Share on a file or folder to give someone access.</p>
    </div>
  {:else}
    <table class="table">
      <thead>
        <tr>
          <th>Name</th>
          <th>Shared with</th>
          <th class="w-36">Access</th>
          <th class="hidden w-32 md:table-cell">Since</th>
          <th class="w-12"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each shares as s (s.id)}
          {@const folder = s.entry?.node.kind === 'folder'}
          <tr class="group">
            <td class="max-w-0">
              <button type="button" class="flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => open(s)}>
                {#if folder}<FolderIcon name={s.entry?.meta.name} />{:else if s.entry}<FileIcon meta={s.entry.meta} />{:else}<Icon name="file" class="size-4 shrink-0 text-fg-muted" />{/if}
                <span class="truncate font-medium group-hover:underline group-hover:decoration-line-strong group-hover:underline-offset-4">{s.entry?.meta.name ?? 'Unavailable'}</span>
              </button>
            </td>
            <td>
              <span class="flex items-center gap-2">
                <Avatar username={s.recipient} class="size-6 text-[11px]" />
                <PersonName username={s.recipient} class="truncate" />
                {#if s.expires_at}<span class="shrink-0 text-xs text-fg-muted"><Time ms={s.expires_at * 1000} prefix="until " /></span>{/if}
              </span>
            </td>
            <td>
              <select class="input h-8 text-[13px]" aria-label="Access for {s.recipient}" value={s.permission} onchange={(e) => changePermission(s, e.currentTarget.value)}>
                <option value="read">Can view</option>
                <option value="write">Can edit</option>
              </select>
            </td>
            <td class="hidden text-fg-muted md:table-cell"><Time ms={s.created_at * 1000} /></td>
            <td class="text-right">
              <button type="button" class="btn btn-ghost btn-icon" aria-label="Revoke access for {s.recipient}" title="Revoke access" onclick={() => (revoking = s)}>
                <Icon name="x" />
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

{#if revoking}
  <ConfirmDialog
    title="Revoke access?"
    confirmLabel="Revoke"
    danger
    onconfirm={async () => {
      await deleteShare(revoking.id);
      shares = shares.filter((x) => x.id !== revoking.id);
      toast(`${revoking.recipient} no longer has access`);
    }}
    onclose={() => (revoking = null)}>
    <p class="text-[13px] text-fg-muted">
      {revoking.recipient} won't be able to open {revoking.entry?.meta.name ?? 'this item'} through thencloud anymore. Anything they already downloaded
      stays with them, and since the item isn't re-encrypted, a copy of its key they kept could still decrypt it.
    </p>
  </ConfirmDialog>
{/if}
