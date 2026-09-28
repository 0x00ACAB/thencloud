<script>
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import PhotoLocationNotice from '../PhotoLocationNotice.svelte';
  import Time from '../Time.svelte';
  import { createLink, links, deleteLink } from '../../lib/cloud.svelte.js';
  import { copyText, errorMessage, toastError } from '../../lib/ui.svelte.js';

  let { entry, onclose } = $props();

  let existing = $state(null);
  let password = $state('');
  let expiry = $state('never');
  let opens = $state('any');
  let busy = $state(false);
  let error = $state('');
  let created = $state(null);
  let kind = $state('view');
  const folder = $derived(entry.node.kind === 'folder');

  const kinds = [
    ['view', 'View and download', 'Visitors can open everything in the folder.'],
    ['drop', 'File drop', "Visitors can add files but can't see what's in it."],
  ];

  const expiries = [
    ['never', 'Never'],
    ['1', 'In 1 day'],
    ['7', 'In 7 days'],
    ['30', 'In 30 days'],
  ];

  // An open is one visit to the link page; that visit can then browse and
  // download everything.
  const openLimits = [
    ['any', 'No limit'],
    ['1', 'After the first open'],
    ['5', 'After 5 opens'],
    ['25', 'After 25 opens'],
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
      const drop = folder && kind === 'drop';
      const maxOpens = drop || opens === 'any' ? null : Number(opens);
      created = await createLink(entry, { password, expiresAt, uploadOnly: drop, maxOpens });
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

<Modal title="Public link" description="Anyone with the link can use {entry.meta.name}, no account needed." {onclose} onsubmit={submit} class="max-w-lg">
  <div class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
    <Icon name="key-round" class="mt-0.5 size-4 shrink-0 text-fg" />
    {#if kind === 'drop'}
      <p>
        A file drop link carries your public key after the <code class="font-mono text-fg">#</code>. Files are encrypted to you in the visitor's browser,
        and only you can open them. Anyone with the link can add files, so they count toward your storage.
      </p>
    {:else}
      <p>
        The decryption key is the part of the link after <code class="font-mono text-fg">#</code>. Browsers never send that part to the server, so thencloud
        can't read what you share. Treat the whole link like a password.
      </p>
    {/if}
  </div>

  <PhotoLocationNotice {entry} />

  {#if existing?.length}
    <div class="space-y-2">
      <h3 class="text-[13px] font-medium">Active links</h3>
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each existing as l (l.id)}
          <li class="flex items-center gap-2 px-3 py-2">
            <div class="min-w-0 flex-1">
              {#if l.url}
                <p class="truncate font-mono text-[13px] {created?.id === l.id ? 'text-accent-text' : ''}">{l.url}</p>
              {:else}
                <p class="text-[13px] text-fg-muted">Made before link passwords were part of the key. It can't be opened; delete it and make a new one.</p>
              {/if}
              <p class="mt-0.5 flex flex-wrap gap-x-3 text-xs text-fg-muted">
                <span><Time ms={l.created_at * 1000} prefix="Created " /></span>
                {#if l.upload_only}<span class="inline-flex items-center gap-1"><Icon name="inbox" class="size-3" />File drop</span>{/if}
                {#if l.has_password}<span class="inline-flex items-center gap-1"><Icon name="lock" class="size-3" />Password</span>{/if}
                <span>{#if l.expires_at}<Time ms={l.expires_at * 1000} prefix="Expires " />{:else}No expiry{/if}</span>
                {#if l.max_opens}<span>{l.opens} of {l.max_opens} {l.max_opens === 1 ? 'open' : 'opens'} used</span>{/if}
              </p>
            </div>
            {#if l.url}
              <button type="button" class="btn btn-ghost btn-icon" aria-label="Copy link" title="Copy link" onclick={() => copyText(l.url, 'Link copied')}>
                <Icon name="copy" />
              </button>
            {/if}
            <button type="button" class="btn btn-ghost btn-icon" aria-label="Delete link" title="Delete link" onclick={() => remove(l)}>
              <Icon name="trash-2" />
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if folder}
    <div class="grid gap-2 sm:grid-cols-2" role="radiogroup" aria-label="Link type">
      {#each kinds as [value, label, text] (value)}
        <button
          type="button"
          role="radio"
          aria-checked={kind === value}
          class="grid cursor-pointer gap-1 rounded-md border p-3 text-left transition-colors {kind === value ? 'border-accent bg-accent-soft' : 'border-line hover:bg-subtle'}"
          onclick={() => (kind = value)}>
          <span class="text-sm font-medium {kind === value ? 'text-accent-text' : ''}">{label}</span>
          <span class="text-xs text-fg-muted">{text}</span>
        </button>
      {/each}
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
    {#if kind !== 'drop'}
      <div class="field sm:col-span-2">
        <label class="label" for="link-opens">Stops working</label>
        <select id="link-opens" class="input" bind:value={opens}>
          {#each openLimits as [value, label] (value)}<option {value}>{label}</option>{/each}
        </select>
        <p class="text-xs text-fg-muted">Each visit to the link counts once, however many files are downloaded during it.</p>
      </div>
    {/if}
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
