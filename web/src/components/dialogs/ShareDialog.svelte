<script>
  // Share with another user. Their public key is fetched from the server,
  // so we show its fingerprint and ask the user to compare it out of band
  // before the item's key is sealed to it.
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { lookupUser, share, outgoingShares, setSharePermission, deleteShare, session } from '../../lib/cloud.svelte.js';
  import { errorMessage, toast, toastError } from '../../lib/ui.svelte.js';

  let { entry, onclose } = $props();

  let username = $state('');
  let user = $state(null); // { username, publicKey, fingerprint }
  let verified = $state(false);
  let permission = $state('read');
  let busy = $state(false);
  let error = $state('');
  let people = $state(null);
  let input = $state();

  const isFolder = $derived(entry.node.kind === 'folder');

  async function loadPeople() {
    try {
      people = await outgoingShares(entry.node.id);
    } catch (e) {
      toastError(e);
      people = [];
    }
  }

  onMount(() => {
    loadPeople();
    input?.focus();
  });

  async function submit() {
    error = '';
    busy = true;
    try {
      if (!user) {
        const name = username.trim().toLowerCase();
        if (name === session.me.username) throw new Error("That's you. Pick someone else to share with.");
        user = await lookupUser(name);
      } else {
        await share(entry, user, permission);
        toast(`Shared with ${user.username}`, { kind: 'success' });
        user = null;
        username = '';
        verified = false;
        await loadPeople();
      }
    } catch (e) {
      error = e?.status === 404 ? `There's no user called "${username.trim()}".` : errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function changePermission(p, value) {
    try {
      await setSharePermission(p.id, value);
      p.permission = value;
    } catch (e) {
      toastError(e);
    }
  }

  async function revoke(p) {
    try {
      await deleteShare(p.id);
      people = people.filter((x) => x.id !== p.id);
      toast(`${p.recipient} no longer has access`);
    } catch (e) {
      toastError(e);
    }
  }
</script>

<Modal title="Share {entry.meta.name}" description="People you share with can decrypt this {isFolder ? 'folder and everything in it' : 'file'}." {onclose} onsubmit={submit} class="max-w-lg">
  {#if !user}
    <div class="field">
      <label class="label" for="share-user">Username</label>
      <div class="flex gap-2">
        <input id="share-user" bind:this={input} class="input" bind:value={username} autocomplete="off" autocapitalize="none" spellcheck="false" placeholder="e.g. alex" required />
        <button class="btn btn-primary h-9" disabled={busy || !username.trim()}>
          {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
          Continue
        </button>
      </div>
    </div>
  {:else}
    <div class="grid gap-3 rounded-md border border-line p-4">
      <div class="flex items-center gap-3">
        <span class="grid size-8 place-items-center rounded-full bg-muted text-xs font-semibold uppercase">{user.username.slice(0, 1)}</span>
        <div class="min-w-0 flex-1">
          <p class="font-medium">{user.username}</p>
          <p class="hint">Their key fingerprint</p>
        </div>
      </div>
      <p class="fingerprint rounded-md bg-subtle px-3 py-2 text-center select-all">{user.fingerprint}</p>
      <p class="hint">
        Ask {user.username} to open Settings and read you their fingerprint. If it doesn't match exactly, don't share: someone may be intercepting.
      </p>
      <label class="flex cursor-pointer items-center gap-2 text-[13px]">
        <input type="checkbox" bind:checked={verified} class="size-4 accent-accent" />
        The fingerprints match
      </label>
      <div class="field">
        <label class="label" for="share-perm">Access</label>
        <select id="share-perm" class="input" bind:value={permission}>
          <option value="read">Can view and download</option>
          <option value="write">Can edit{isFolder ? ', upload and delete inside' : ''}</option>
        </select>
      </div>
    </div>
  {/if}

  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}

  <div class="space-y-2">
    <h3 class="text-[13px] font-medium">People with access</h3>
    {#if people === null}
      <p class="hint">Loading</p>
    {:else if !people.length}
      <p class="hint">Only you.</p>
    {:else}
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each people as p (p.id)}
          <li class="flex items-center gap-3 px-3 py-2">
            <span class="grid size-7 shrink-0 place-items-center rounded-full bg-muted text-xs font-semibold uppercase">{p.recipient.slice(0, 1)}</span>
            <span class="min-w-0 flex-1 truncate text-sm">{p.recipient}</span>
            <select
              class="input h-8 w-auto text-[13px]"
              aria-label="Access for {p.recipient}"
              value={p.permission}
              onchange={(e) => changePermission(p, e.currentTarget.value)}>
              <option value="read">Can view</option>
              <option value="write">Can edit</option>
            </select>
            <button type="button" class="btn btn-ghost btn-icon" aria-label="Remove {p.recipient}" title="Remove access" onclick={() => revoke(p)}>
              <Icon name="x" />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  {#snippet footer()}
    {#if user}
      <button type="button" class="btn btn-secondary mr-auto" onclick={() => ((user = null), (verified = false))}>Back</button>
      <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
      <button class="btn btn-accent" disabled={busy || !verified}>
        {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
        Share
      </button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={onclose}>Done</button>
    {/if}
  {/snippet}
</Modal>
