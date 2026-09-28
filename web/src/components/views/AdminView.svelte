<script>
  // Server administration. Admins manage accounts, never content: nothing
  // here can read anyone's files, names or keys, and the numbers are counts.
  import { onMount } from 'svelte';
  import {
    session,
    adminUsers,
    adminUpdateUser,
    adminDeleteUser,
    adminSettings,
    adminUpdateSettings,
    adminInvites,
    adminCreateInvite,
    adminDeleteInvite,
    adminStats,
    inviteUrl,
    resetToolsInfo,
    refreshMe,
  } from '../../lib/cloud.svelte.js';
  import { toast, toastError, copyText } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, formatDate, fullDate, plural } from '../../lib/format.js';
  import { slide } from '../../lib/motion.js';
  import Icon from '../Icon.svelte';
  import Time from '../Time.svelte';
  import Menu from '../Menu.svelte';
  import Modal from '../Modal.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';

  let users = $state(null);
  let settings = $state(null);
  let invites = $state(null);
  let stats = $state(null);
  let dialog = $state(null); // { type: 'quota' | 'delete', user }
  let created = $state(null); // the invite just made: { url, expires_at }
  let inviteDays = $state(7);
  let busy = $state(false);

  async function load() {
    try {
      [users, settings, invites, stats] = await Promise.all([adminUsers(), adminSettings(), adminInvites(), adminStats()]);
    } catch (e) {
      toastError(e);
    }
  }
  onMount(load);

  const modes = [
    ['open', 'Open', 'Anyone who can reach this server can create an account.'],
    ['invite', 'Invite only', 'People need an invite link from an admin.'],
    ['closed', 'Closed', 'No new accounts, not even with an invite.'],
  ];

  async function setRegistration(registration) {
    if (settings.registration === registration) return;
    try {
      settings = await adminUpdateSettings({ registration });
    } catch (e) {
      toastError(e);
    }
  }

  const downloaderModes = [
    ['off', 'Off', 'Nobody can use it.'],
    ['admins', 'Admins only', 'Only admins see "From a video link".'],
    ['everyone', 'Everyone', 'Every account can use it.'],
  ];

  async function setDownloader(downloader) {
    if (settings.downloader === downloader) return;
    try {
      settings = await adminUpdateSettings({ downloader });
      resetToolsInfo();
    } catch (e) {
      toastError(e);
    }
  }

  async function createInvite() {
    busy = true;
    try {
      const r = await adminCreateInvite(inviteDays);
      created = { url: inviteUrl(r.token), expires_at: r.invite.expires_at };
      invites = [r.invite, ...invites];
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }

  async function removeInvite(inv) {
    try {
      await adminDeleteInvite(inv.id);
      invites = invites.filter((i) => i.id !== inv.id);
    } catch (e) {
      toastError(e);
    }
  }

  // used_by goes back to null if that account is deleted later; used_at stays.
  const inviteState = (i) =>
    i.used_at ? `Used by ${i.used_by ?? 'a deleted account'}` : i.expires_at * 1000 < Date.now() ? 'Expired' : `Expires ${formatDate(i.expires_at * 1000)}`;

  async function update(user, body, message) {
    try {
      const next = await adminUpdateUser(user.id, body);
      users = users.map((u) => (u.id === next.id ? next : u));
      if (message) toast(message, { kind: 'success' });
      if (user.id === session.me.user_id) refreshMe().catch(() => {});
      stats = await adminStats();
    } catch (e) {
      toastError(e);
    }
  }

  function menuFor(u) {
    const self = u.id === session.me.user_id;
    return [
      { label: 'Change quota', icon: 'hard-drive', onclick: () => (dialog = { type: 'quota', user: u }) },
      { label: 'Transfer limits', icon: 'refresh-cw', onclick: () => (dialog = { type: 'transfer', user: u }) },
      ...(!self
        ? [
            u.is_admin
              ? { label: 'Remove admin rights', icon: 'user', onclick: () => update(u, { is_admin: false }, `${u.username} is no longer an admin`) }
              : { label: 'Make admin', icon: 'key-round', onclick: () => update(u, { is_admin: true }, `${u.username} is now an admin`) },
            u.disabled
              ? { label: 'Enable account', icon: 'door-open', onclick: () => update(u, { disabled: false }, `${u.username} can sign in again`) }
              : { label: 'Disable account', icon: 'lock', onclick: () => update(u, { disabled: true }, `${u.username} was signed out and can't sign in`) },
            'sep',
            { label: 'Delete account', icon: 'trash-2', danger: true, onclick: () => (dialog = { type: 'delete', user: u, typed: '' }) },
          ]
        : []),
    ];
  }

  // Quota dialog, in GB.
  let quotaGb = $state('');
  let quotaError = $state('');
  $effect(() => {
    if (dialog?.type === 'quota') {
      quotaGb = String(+(dialog.user.quota_bytes / 1024 ** 3).toFixed(2));
      quotaError = '';
    }
  });

  async function saveQuota() {
    const gb = Number(quotaGb);
    if (!Number.isFinite(gb) || gb < 0) return (quotaError = 'Enter a number of GB, like 10 or 0.5.');
    const u = dialog.user;
    dialog = null;
    await update(u, { quota_bytes: Math.round(gb * 1024 ** 3) }, `${u.username} now has ${formatSize(Math.round(gb * 1024 ** 3))}`);
  }

  // Daily transfer limits, in GB (blank: none).
  let downGb = $state('');
  let upGb = $state('');
  let transferError = $state('');
  $effect(() => {
    if (dialog?.type === 'transfer') {
      const gb = (v) => (v ? String(+(v / 1024 ** 3).toFixed(2)) : '');
      downGb = gb(dialog.user.daily_download_limit);
      upGb = gb(dialog.user.daily_upload_limit);
      transferError = '';
    }
  });

  async function saveTransfer() {
    const bytes = (v) => (String(v).trim() === '' ? 0 : Math.round(Number(v) * 1024 ** 3));
    const [down, up] = [bytes(downGb), bytes(upGb)];
    if (![down, up].every((b) => Number.isFinite(b) && b >= 0)) return (transferError = 'Enter a number of GB, like 5 or 0.5, or leave it empty for no limit.');
    const u = dialog.user;
    dialog = null;
    await update(u, { daily_download_limit: down, daily_upload_limit: up }, down || up ? `Limits set for ${u.username}` : `${u.username} has no transfer limits`);
  }

  const pct = (u) => Math.min(100, (u.used_bytes / Math.max(1, u.quota_bytes)) * 100);
</script>

{#snippet stat(label, value, detail = '')}
  <div class="card grid gap-1 p-4">
    <p class="text-xs text-fg-muted">{label}</p>
    <p class="text-xl font-semibold tracking-tight tabular-nums">{value}</p>
    {#if detail}<p class="truncate text-xs text-fg-faint">{detail}</p>{/if}
  </div>
{/snippet}

<h1 class="text-xl font-semibold tracking-tight">Admin</h1>
<p class="mt-1 text-[13px] text-fg-muted">
  Accounts and server settings. Admins can't see anyone's files, file names or keys; the numbers below are plain counts.
</p>

{#if !users || !stats || !settings || !invites}
  <div class="mt-6 grid gap-4" aria-hidden="true">
    <div class="grid grid-cols-2 gap-3 md:grid-cols-4">{#each [0, 1, 2, 3] as i (i)}<div class="skeleton h-24"></div>{/each}</div>
    <div class="skeleton h-64"></div>
  </div>
{:else}
  <div class="mt-6 grid grid-cols-2 gap-3 md:grid-cols-4 animate-enter">
    {@render stat('Accounts', stats.users, stats.disabled_users ? `${stats.disabled_users} disabled` : `${plural(stats.active_sessions, 'active session')}`)}
    {@render stat('Storage used', formatSize(stats.used_bytes), `of ${formatSize(stats.quota_bytes)} handed out`)}
    {@render stat('Files', stats.files, `${plural(stats.folders, 'folder')} · ${plural(stats.versions, 'stored version')}`)}
    {@render stat('Sharing', stats.shares, `${plural(stats.public_links, 'public link')}`)}
  </div>

  <section class="card mt-6 overflow-hidden">
    <div class="grid gap-4 p-6">
      <div class="grid gap-1">
        <h2 class="text-base font-semibold tracking-tight">Registration</h2>
        <p class="text-[13px] text-fg-muted">Who can create an account on this server.</p>
      </div>
      <div class="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label="Registration">
        {#each modes as [value, label, text] (value)}
          <button
            type="button"
            role="radio"
            aria-checked={settings.registration === value}
            class="grid cursor-pointer gap-1 rounded-md border p-3 text-left transition-colors {settings.registration === value
              ? 'border-accent bg-accent-soft'
              : 'border-line hover:bg-subtle'}"
            onclick={() => setRegistration(value)}>
            <span class="text-sm font-medium {settings.registration === value ? 'text-accent-text' : ''}">{label}</span>
            <span class="text-xs text-fg-muted">{text}</span>
          </button>
        {/each}
      </div>
    </div>

    <div class="grid grid-cols-1 gap-4 border-t border-line p-6">
      <div class="flex flex-wrap items-end justify-between gap-3">
        <div class="grid gap-1">
          <h3 class="text-sm font-semibold">Invite links</h3>
          <p class="text-[13px] text-fg-muted">
            Each link creates one account.{settings.registration !== 'invite' ? ' They only work while registration is Invite only.' : ''}
          </p>
        </div>
        <div class="flex items-center gap-2">
          <label class="sr-only" for="invite-days">Valid for</label>
          <select id="invite-days" class="input h-8 w-auto" bind:value={inviteDays}>
            {#each [[1, '1 day'], [7, '7 days'], [30, '30 days'], [90, '90 days']] as [d, l] (d)}<option value={d}>{l}</option>{/each}
          </select>
          <button type="button" class="btn btn-secondary" disabled={busy} onclick={createInvite}>
            {#if busy}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="user-round-plus" />{/if}
            Create invite
          </button>
        </div>
      </div>

      {#if created}
        <div class="grid gap-2 rounded-md border border-accent bg-accent-soft p-3" transition:slide>
          <p class="text-[13px] font-medium text-accent-text">New invite link. Copy it now; it can't be shown again.</p>
          <div class="flex items-center gap-2">
            <code class="min-w-0 flex-1 truncate rounded border border-line bg-bg px-2 py-1.5 font-mono text-xs select-all">{created.url}</code>
            <button type="button" class="btn btn-secondary" onclick={() => copyText(created.url, 'Invite link copied')}><Icon name="copy" /> Copy</button>
            <button type="button" class="btn btn-ghost btn-icon" aria-label="Dismiss" onclick={() => (created = null)}><Icon name="x" /></button>
          </div>
        </div>
      {/if}

      {#if invites.length}
        <ul class="divide-y divide-line rounded-md border border-line">
          {#each invites as inv (inv.id)}
            <li class="flex items-center gap-3 px-3 py-2 text-sm" out:slide>
              <Icon name="link" class="size-4 shrink-0 text-fg-muted" />
              <span class="min-w-0 flex-1 truncate">
                <span class={inv.used_at ? 'text-fg' : 'text-fg-muted'}>{inviteState(inv)}</span>
                <span class="text-xs text-fg-faint"> · created by {inv.created_by} <Time ms={inv.created_at * 1000} relative /></span>
              </span>
              {#if !inv.used_at}
                <button type="button" class="btn btn-ghost btn-icon" aria-label="Delete invite" title="Delete invite" onclick={() => removeInvite(inv)}><Icon name="trash-2" /></button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </section>

  <section class="card mt-6 overflow-hidden">
    <div class="grid gap-4 p-6">
      <div class="grid gap-1">
        <h2 class="text-base font-semibold tracking-tight">Video downloader</h2>
        <p class="text-[13px] text-fg-muted">
          Lets people save videos from YouTube and other sites into their files, using yt-dlp on this server. It's the one feature where the server
          sees what's being handled: the link and the video pass through it on their way to the browser, which then encrypts them. Nothing is stored.
        </p>
      </div>
      {#if settings.yt_dlp_version}
        <p class="flex items-center gap-2 text-[13px] text-fg-muted">
          <Icon name="check" class="size-4 text-success" />yt-dlp {settings.yt_dlp_version} found · up to {formatSize(settings.downloader_max_bytes)} per video
        </p>
        {#if !settings.downloader_can_merge}
          <p class="flex items-start gap-2 text-[13px] text-fg-muted">
            <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
            <span>ffmpeg isn't installed, so only videos a site offers as one file can be saved. YouTube rarely does any more; install ffmpeg for video up to 1080p.</span>
          </p>
        {/if}
      {:else}
        <p class="flex items-start gap-2 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
          <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
          <span>yt-dlp isn't installed on this server, or isn't on its PATH. Install it (or point <code class="font-mono text-xs">--yt-dlp</code> at it) and restart the server to use this.</span>
        </p>
      {/if}
      <div class="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label="Video downloader">
        {#each downloaderModes as [value, label, text] (value)}
          <button
            type="button"
            role="radio"
            aria-checked={settings.downloader === value}
            disabled={!settings.yt_dlp_version && value !== 'off'}
            class="grid cursor-pointer gap-1 rounded-md border p-3 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-50 {settings.downloader === value
              ? 'border-accent bg-accent-soft'
              : 'border-line hover:bg-subtle disabled:hover:bg-transparent'}"
            onclick={() => setDownloader(value)}>
            <span class="text-sm font-medium {settings.downloader === value ? 'text-accent-text' : ''}">{label}</span>
            <span class="text-xs text-fg-muted">{text}</span>
          </button>
        {/each}
      </div>
    </div>
  </section>

  <section class="card mt-6 overflow-hidden">
    <div class="p-6 pb-4">
      <h2 class="text-base font-semibold tracking-tight">Accounts</h2>
    </div>
    <table class="table">
      <thead>
        <tr>
          <th>Username</th>
          <th class="hidden w-56 sm:table-cell">Storage</th>
          <th class="hidden w-36 md:table-cell">Last active</th>
          <th class="w-12"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each users as u (u.id)}
          <tr>
            <td class="max-w-0">
              <span class="flex items-center gap-2">
                <span class="truncate font-medium {u.disabled ? 'text-fg-muted line-through' : ''}">{u.username}</span>
                {#if u.id === session.me.user_id}<span class="badge">You</span>{/if}
                {#if u.is_admin}<span class="badge badge-accent">Admin</span>{/if}
                {#if u.disabled}<span class="badge text-danger">Disabled</span>{/if}
              </span>
              <span class="text-xs text-fg-faint" title={fullDate(u.created_at * 1000)}>Joined {formatDate(u.created_at * 1000)}</span>
            </td>
            <td class="hidden sm:table-cell">
              <div class="grid gap-1.5">
                <span class="text-xs text-fg-muted tabular-nums">{formatSize(u.used_bytes)} of {formatSize(u.quota_bytes)}</span>
                <div class="progress"><div style:width="{pct(u)}%"></div></div>
              </div>
            </td>
            <td class="hidden text-fg-muted md:table-cell" title={u.last_seen ? fullDate(u.last_seen * 1000) : ''}>
              {u.last_seen ? formatWhen(u.last_seen * 1000) : 'Not signed in'}
            </td>
            <td class="text-right"><Menu items={menuFor(u)} label="Actions for {u.username}" /></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{/if}

{#if dialog?.type === 'quota'}
  <Modal title="Storage for {dialog.user.username}" description="Using {formatSize(dialog.user.used_bytes)} now. Lowering the quota below that blocks new uploads but deletes nothing." onclose={() => (dialog = null)} onsubmit={saveQuota}>
    <div class="field">
      <label class="label" for="quota">Quota in GB</label>
      <input id="quota" class="input" inputmode="decimal" bind:value={quotaGb} />
      {#if quotaError}<p class="text-[13px] text-danger">{quotaError}</p>{/if}
    </div>
    {#snippet footer()}
      <button type="button" class="btn btn-secondary" onclick={() => (dialog = null)}>Cancel</button>
      <button class="btn btn-primary">Save</button>
    {/snippet}
  </Modal>
{:else if dialog?.type === 'transfer'}
  <Modal
    title="Transfer limits for {dialog.user.username}"
    description="How much they can download and upload each day (UTC), counting what goes through their public links. Today so far: {formatSize(dialog.user.downloaded_today)} down, {formatSize(dialog.user.uploaded_today)} up."
    onclose={() => (dialog = null)}
    onsubmit={saveTransfer}>
    <div class="grid gap-3 sm:grid-cols-2">
      <div class="field">
        <label class="label" for="down-limit">Downloads, GB a day</label>
        <input id="down-limit" class="input" inputmode="decimal" placeholder="No limit" bind:value={downGb} />
      </div>
      <div class="field">
        <label class="label" for="up-limit">Uploads, GB a day</label>
        <input id="up-limit" class="input" inputmode="decimal" placeholder="No limit" bind:value={upGb} />
      </div>
    </div>
    {#if transferError}<p class="text-[13px] text-danger">{transferError}</p>{/if}
    {#snippet footer()}
      <button type="button" class="btn btn-secondary" onclick={() => (dialog = null)}>Cancel</button>
      <button class="btn btn-primary">Save</button>
    {/snippet}
  </Modal>
{:else if dialog?.type === 'delete'}
  <ConfirmDialog
    title="Delete {dialog.user.username}?"
    description="Their files, folders, versions, shares and links are deleted for good, and the storage is freed. Files they added to other people's shared folders stay with those people. This can't be undone."
    confirmLabel="Delete account"
    danger
    disabled={dialog.typed !== dialog.user.username}
    onconfirm={async () => {
      const u = dialog.user;
      await adminDeleteUser(u.id);
      users = users.filter((x) => x.id !== u.id);
      stats = await adminStats();
      toast(`Deleted ${u.username}`, { kind: 'success' });
    }}
    onclose={() => (dialog = null)}>
    <div class="field">
      <label class="label" for="confirm-name">Type <span class="font-mono">{dialog.user.username}</span> to confirm</label>
      <input id="confirm-name" class="input" bind:value={dialog.typed} autocomplete="off" spellcheck="false" />
    </div>
  </ConfirmDialog>
{/if}
