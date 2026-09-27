<script>
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { createLink, links, deleteLink } from '../../lib/cloud.svelte.js';
  import { copyText, errorMessage, toastError } from '../../lib/ui.svelte.js';
  import { formatDate } from '../../lib/format.js';

  let { entry, onclose } = $props();

  let existing = $state(null);
  let password = $state('');
  let expiry = $state('never');
  let busy = $state(false);
  let error = $state('');
  let created = $state(null);

  const expiries = [
    ['never', 'Never'],
    ['1', 'In 1 day'],
    ['7', 'In 7 days'],
    ['30', 'In 30 days'],
  ];

  async function load() {
    try {
      existing = await links(entry.node.id);
    } catch (e) {
      toastError(e);
      existing = [];
    }
  }

  $effect(() => {
    load();
  });

  async function submit() {
    busy = true;
    error = '';
    try {
      const expiresAt = expiry === 'never' ? null : Math.floor(Date.now() / 1000) + Number(expiry) * 86400;
      created = await createLink(entry, { password, expiresAt });
      password = '';
      await load();
      copyText(created.url, 'Link created and copied');
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function remove(l) {
    try {
      await deleteLink(l.id);
      existing = existing.filter((x) => x.id !== l.id);
      if (created?.id === l.id) created = null;
    } catch (e) {
      toastError(e);
    }
  }
</script>

<Modal title="Public link" description="Anyone with the link can open {entry.meta.name}, no account needed." {onclose} onsubmit={submit} class="max-w-lg">
  <div class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
    <Icon name="key-round" class="mt-0.5 size-4 shrink-0 text-fg" />
    <p>
      The decryption key is the part of the link after <code class="font-mono text-fg">#</code>. Browsers never send that part to the server, so thencloud
      can't read what you share. Treat the whole link like a password.
    </p>
  </div>

  {#if existing?.length}
    <div class="space-y-2">
      <h3 class="text-[13px] font-medium">Active links</h3>
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each existing as l (l.id)}
          <li class="flex items-center gap-2 px-3 py-2">
            <div class="min-w-0 flex-1">
              <p class="truncate font-mono text-[13px] {created?.id === l.id ? 'text-accent-text' : ''}">{l.url}</p>
              <p class="mt-0.5 flex flex-wrap gap-x-3 text-xs text-fg-muted">
                <span>Created {formatDate(l.created_at * 1000)}</span>
                {#if l.has_password}<span class="inline-flex items-center gap-1"><Icon name="lock" class="size-3" />Password</span>{/if}
                <span>{l.expires_at ? `Expires ${formatDate(l.expires_at * 1000)}` : 'No expiry'}</span>
              </p>
            </div>
            <button type="button" class="btn btn-ghost btn-icon" aria-label="Copy link" title="Copy link" onclick={() => copyText(l.url, 'Link copied')}>
              <Icon name="copy" />
            </button>
            <button type="button" class="btn btn-ghost btn-icon" aria-label="Delete link" title="Delete link" onclick={() => remove(l)}>
              <Icon name="trash-2" />
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <div class="grid gap-3 sm:grid-cols-2">
    <div class="field">
      <label class="label" for="link-password">Password <span class="font-normal text-fg-muted">(optional)</span></label>
      <input id="link-password" class="input" type="password" bind:value={password} autocomplete="new-password" />
    </div>
    <div class="field">
      <label class="label" for="link-expiry">Expires</label>
      <select id="link-expiry" class="input" bind:value={expiry}>
        {#each expiries as [value, label] (value)}<option {value}>{label}</option>{/each}
      </select>
    </div>
  </div>
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}

  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>Done</button>
    <button class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="link" />{/if}
      Create link
    </button>
  {/snippet}
</Modal>
