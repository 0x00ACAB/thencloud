<script>
  import { incomingShares, deleteShare, download } from '../../lib/cloud.svelte.js';
  import { toast, toastError, trackTransfer, errorMessage } from '../../lib/ui.svelte.js';
  import { formatSize } from '../../lib/format.js';
  import Icon from '../Icon.svelte';
  import Time from '../Time.svelte';
  import FileIcon from '../FileIcon.svelte';
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
    const t = trackTransfer('download', s.entry.meta.name, s.entry.meta.size);
    try {
      await download(s.entry, (p) => (t.progress = p));
      t.status = 'done';
    } catch (e) {
      t.status = 'error';
      t.error = errorMessage(e);
    }
  }
</script>

<div>
  <h1 class="text-xl font-semibold tracking-tight">Shared with me</h1>
  <p class="mt-1 text-[13px] text-fg-muted">Files and folders other people have shared with you. Compare fingerprints with the owner to be sure a share really came from them.</p>
</div>

<div class="card mt-6 overflow-hidden">
  {#if shares === null}
    <div class="grid h-48 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
  {:else if !shares.length}
    <div class="grid place-items-center gap-1 px-6 py-20 text-center">
      <img src="/img/logo.webp" alt="" width="715" height="349" class="mb-4 h-auto w-32 opacity-90 select-none" draggable="false" />
      <p class="font-medium">Nothing shared with you yet</p>
      <p class="max-w-sm text-[13px] text-fg-muted">
        When someone shares a file or folder with you, it shows up here. Tell them your username, and read them your key fingerprint from Settings so they can check it.
      </p>
    </div>
  {:else}
    <table class="table">
      <thead>
        <tr>
          <th>Name</th>
          <th class="hidden md:table-cell">Owner</th>
          <th class="hidden w-28 sm:table-cell">Access</th>
          <th class="hidden w-32 lg:table-cell">Shared</th>
          <th class="w-12"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each shares as s (s.id)}
          {@const folder = s.node.kind === 'folder'}
          <tr class="group">
            <td class="max-w-0">
              {#if s.error}
                <span class="flex items-center gap-3 text-fg-muted"><Icon name="circle-alert" class="size-4 text-danger" />Couldn't decrypt this share</span>
              {:else}
                <button type="button" class="flex max-w-full cursor-pointer items-center gap-3 text-left" onclick={() => open(s)}>
                  {#if folder}<Icon name="folder" class="size-4 shrink-0 text-accent-text" />{:else}<FileIcon meta={s.entry.meta} />{/if}
                  <span class="truncate font-medium group-hover:underline group-hover:decoration-line-strong group-hover:underline-offset-4">{s.entry.meta.name}</span>
                  {#if !folder}<span class="hidden text-fg-faint sm:inline">{formatSize(s.entry.meta.size)}</span>{/if}
                </button>
              {/if}
            </td>
            <td class="hidden md:table-cell">
              <p class="font-medium">{s.owner}</p>
              {#if s.ownerFingerprint}<p class="font-mono text-[11px] text-fg-faint" title="Owner's key fingerprint">{s.ownerFingerprint}</p>{/if}
            </td>
            <td class="hidden sm:table-cell">
              <span class="badge {s.permission === 'write' ? 'badge-accent' : ''}">{s.permission === 'write' ? 'Can edit' : 'View only'}</span>
            </td>
            <td class="hidden text-fg-muted lg:table-cell"><Time ms={s.created_at * 1000} /></td>
            <td class="text-right">
              <Menu
                label="Actions"
                items={[
                  ...(s.error ? [] : [folder ? { label: 'Open', icon: 'folder-open', onclick: () => open(s) } : { label: 'Download', icon: 'download', onclick: () => open(s) }]),
                  { label: 'Remove from my list', icon: 'door-open', danger: true, onclick: () => (leaving = s) },
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
    title="Leave this share?"
    description="You'll lose access to {leaving.entry?.meta.name ?? 'this item'}. {leaving.owner} can share it with you again."
    confirmLabel="Leave"
    danger
    onconfirm={async () => {
      await deleteShare(leaving.id);
      shares = shares.filter((x) => x.id !== leaving.id);
      toast('Removed from your shares');
    }}
    onclose={() => (leaving = null)} />
{/if}
