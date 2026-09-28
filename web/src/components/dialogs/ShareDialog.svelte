<script>
  // Share with another user. Their public key is fetched from the server,
  // so we show its fingerprint and ask the user to compare it out of band
  // before the item's key is sealed to it. A checked key is remembered
  // (encrypted, in verified contacts): next time it's shown as verified, and
  // if the server ever hands out a different key, sharing is blocked until
  // it's checked again.
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import PhotoLocationNotice from '../PhotoLocationNotice.svelte';
  import Avatar from '../Avatar.svelte';
  import PersonName from '../PersonName.svelte';
  import { lookupUser, share, outgoingShares, setSharePermission, deleteShare, session, contactStatus, verifyContact } from '../../lib/cloud.svelte.js';
  import Time from '../Time.svelte';
  import { errorMessage, toast, toastError } from '../../lib/ui.svelte.js';
  import { formatDate } from '../../lib/format.js';
  import { t } from '../../lib/i18n.svelte.js';

  let { entry, onclose } = $props();

  let username = $state('');
  let user = $state(null); // { username, publicKey, fingerprint }
  let verified = $state(false);
  let status = $state(null); // { state: 'new' | 'verified' | 'changed', verifiedAt, pinnedFingerprint }
  let permission = $state('read');
  let ends = $state('never');
  const endings = $derived([
    ['never', t('Never')],
    ['1', t('In {count} days', { count: 1 })],
    ['7', t('In {count} days', { count: 7 })],
    ['30', t('In {count} days', { count: 30 })],
  ]);
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
        if (name === session.me.username) throw new Error(t("That's you. Pick someone else to share with."));
        const found = await lookupUser(name);
        status = await contactStatus(found);
        verified = status.state === 'verified';
        user = found;
      } else {
        // Remember a key once it's been checked (or re-checked after a change).
        if (status.state !== 'verified') await verifyContact(user);
        const expiresAt = ends === 'never' ? null : Math.floor(Date.now() / 1000) + Number(ends) * 86400;
        await share(entry, user, permission, expiresAt);
        toast(t('Shared with {name}', { name: user.username }), { kind: 'success' });
        user = null;
        status = null;
        username = '';
        verified = false;
        await loadPeople();
      }
    } catch (e) {
      error = e?.status === 404 ? t('There\'s no user called "{name}".', { name: username.trim() }) : errorMessage(e);
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
      toast(t('{name} no longer has access', { name: p.recipient }));
    } catch (e) {
      toastError(e);
    }
  }
</script>

<Modal
  title={t('Share {name}', { name: entry.meta.name })}
  description={isFolder ? t('People you share with can decrypt this folder and everything in it.') : t('People you share with can decrypt this file.')}
  {onclose}
  onsubmit={submit}
  class="max-w-lg">
  {#if !user}
    <div class="field">
      <label class="label" for="share-user">{t('Username')}</label>
      <div class="flex gap-2">
        <input id="share-user" bind:this={input} class="input" bind:value={username} autocomplete="off" autocapitalize="none" spellcheck="false" placeholder={t('e.g. alex')} required />
        <button class="btn btn-primary h-9" disabled={busy || !username.trim()}>
          {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
          {t('Continue')}
        </button>
      </div>
    </div>
  {:else}
    <div class="grid gap-3 rounded-md border border-line p-4">
      <div class="flex items-center gap-3">
        <Avatar username={user.username} class="size-8 text-xs" />
        <div class="min-w-0 flex-1">
          <p class="font-medium"><PersonName username={user.username} /></p>
          <p class="hint">{t('Their key fingerprint')}</p>
        </div>
      </div>
      <p class="fingerprint rounded-md bg-subtle px-3 py-2 text-center select-all">{user.fingerprint}</p>
      {#if status.state === 'verified'}
        <p class="flex items-center gap-2 text-[13px] text-success">
          <Icon name="shield-check" class="size-4" />{t('You checked this key on {date}.', { date: formatDate(status.verifiedAt) })}
        </p>
      {:else if status.state === 'changed'}
        <div class="grid gap-2 rounded-md border border-danger/40 bg-danger-soft p-3 text-[13px]" role="alert">
          <p class="flex items-center gap-2 font-medium text-danger"><Icon name="circle-alert" class="size-4" />{t('This key is not the one you checked')}</p>
          <p class="text-fg-muted">
            {t('You verified a different key for {name} on {date}:', { name: user.username, date: formatDate(status.verifiedAt) })}
            <span class="fingerprint mt-1 block text-fg-muted">{status.pinnedFingerprint}</span>
          </p>
          <p class="text-fg-muted">
            {t("That can happen if they made a new account with the same name. It's also what an attack would look like. Don't share until {name} reads you the new fingerprint above.", { name: user.username })}
          </p>
        </div>
        <label class="flex cursor-pointer items-center gap-2 text-[13px]">
          <input type="checkbox" bind:checked={verified} class="size-4 accent-accent" />
          {t('{name} read me the new fingerprint and it matches', { name: user.username })}
        </label>
      {:else}
        <p class="hint">
          {t("Ask {name} to open Settings and read you their fingerprint. If it doesn't match exactly, don't share: someone may be intercepting. Once you've checked, it's remembered.", { name: user.username })}
        </p>
        <label class="flex cursor-pointer items-center gap-2 text-[13px]">
          <input type="checkbox" bind:checked={verified} class="size-4 accent-accent" />
          {t('The fingerprints match')}
        </label>
      {/if}
      <div class="grid gap-3 sm:grid-cols-[1fr_auto]">
        <div class="field">
          <label class="label" for="share-perm">{t('Access')}</label>
          <select id="share-perm" class="input" bind:value={permission}>
            <option value="read">{t('Can view and download')}</option>
            <option value="write">{isFolder ? t('Can edit, upload and delete inside') : t('Can edit')}</option>
          </select>
        </div>
        <div class="field">
          <label class="label" for="share-ends">{t('Access ends')}</label>
          <select id="share-ends" class="input" bind:value={ends}>
            {#each endings as [value, label] (value)}<option {value}>{label}</option>{/each}
          </select>
        </div>
      </div>
    </div>
  {/if}

  <PhotoLocationNotice {entry} />

  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}

  <div class="space-y-2">
    <h3 class="text-[13px] font-medium">{t('People with access')}</h3>
    {#if people === null}
      <p class="hint">{t('Loading')}</p>
    {:else if !people.length}
      <p class="hint">{t('Only you.')}</p>
    {:else}
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each people as p (p.id)}
          <li class="flex items-center gap-3 px-3 py-2">
            <Avatar username={p.recipient} class="size-7 text-xs" />
            <span class="grid min-w-0 flex-1">
              <PersonName username={p.recipient} class="truncate text-sm" />
              {#if p.expires_at}<span class="text-xs text-fg-muted"><Time ms={p.expires_at * 1000} prefix={t('Until') + ' '} /></span>{/if}
            </span>
            <select
              class="input h-8 w-auto text-[13px]"
              aria-label={t('Access for {name}', { name: p.recipient })}
              value={p.permission}
              onchange={(e) => changePermission(p, e.currentTarget.value)}>
              <option value="read">{t('Can view')}</option>
              <option value="write">{t('Can edit')}</option>
            </select>
            <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Remove {name}', { name: p.recipient })} title={t('Remove access')} onclick={() => revoke(p)}>
              <Icon name="x" />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  {#snippet footer()}
    {#if user}
      <button type="button" class="btn btn-secondary mr-auto" onclick={() => ((user = null), (status = null), (verified = false))}>{t('Back')}</button>
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
      <button class="btn btn-accent" disabled={busy || !verified}>
        {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
        {t('Share')}
      </button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Done')}</button>
    {/if}
  {/snippet}
</Modal>
