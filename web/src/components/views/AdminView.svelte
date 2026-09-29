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
    adminAudit,
    inviteUrl,
    resetToolsInfo,
    refreshMe,
  } from '../../lib/cloud.svelte.js';
  import { toast, toastError, copyText } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, formatDate, fullDate } from '../../lib/format.js';
  import { t } from '../../lib/i18n.svelte.js';
  import { slide } from '../../lib/motion.js';
  import Icon from '../Icon.svelte';
  import Time from '../Time.svelte';
  import Sentence from '../Sentence.svelte';
  import Menu from '../Menu.svelte';
  import Modal from '../Modal.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';

  let users = $state(null);
  let settings = $state(null);
  let invites = $state(null);
  let stats = $state(null);
  let audit = $state(null); // { entries, more }
  let auditLoading = $state(false);
  let dialog = $state(null); // { type: 'quota' | 'delete', user }
  let created = $state(null); // the invite just made: { url, expires_at }
  let inviteDays = $state(7);
  let busy = $state(false);

  async function load() {
    try {
      [users, settings, invites, stats, audit] = await Promise.all([adminUsers(), adminSettings(), adminInvites(), adminStats(), adminAudit()]);
    } catch (e) {
      toastError(e);
    }
  }
  onMount(load);

  // The first page again, after this admin did something.
  const refreshAudit = () => adminAudit().then((a) => (audit = a), () => {});

  async function olderAudit() {
    auditLoading = true;
    try {
      const page = await adminAudit(audit.entries.at(-1).id);
      audit = { entries: [...audit.entries, ...page.entries], more: page.more };
    } catch (e) {
      toastError(e);
    } finally {
      auditLoading = false;
    }
  }

  const bytes = (v) => formatSize(Number(v));
  /** One entry as a sentence. Values are as the server stored them (bytes, a mode, days). */
  function describe(e) {
    const p = { actor: e.actor, target: e.target ?? '' };
    switch (e.action) {
      case 'quota':
        return t("{actor} set {target}'s storage to {value}", { ...p, value: bytes(e.detail) });
      case 'download_limit':
        return Number(e.detail) ? t("{actor} limited {target}'s downloads to {value} a day", { ...p, value: bytes(e.detail) }) : t("{actor} removed {target}'s download limit", p);
      case 'upload_limit':
        return Number(e.detail) ? t("{actor} limited {target}'s uploads to {value} a day", { ...p, value: bytes(e.detail) }) : t("{actor} removed {target}'s upload limit", p);
      case 'admin_granted':
        return t('{actor} made {target} an admin', p);
      case 'admin_removed':
        return t("{actor} took {target}'s admin rights away", p);
      case 'disabled':
        return t("{actor} disabled {target}'s account", p);
      case 'enabled':
        return t("{actor} enabled {target}'s account again", p);
      case 'deleted':
        return t("{actor} deleted {target}'s account", p);
      case 'registration':
        return t('{actor} set registration to {value}', { ...p, value: modes.find((m) => m[0] === e.detail)?.[1] ?? e.detail });
      case 'downloader':
        return t('{actor} set the video downloader to {value}', { ...p, value: downloaderModes.find((m) => m[0] === e.detail)?.[1] ?? e.detail });
      case 'invite_created':
        return t('{actor} made an invite link valid for {count} days', { actor: e.actor, count: Number(e.detail) });
      case 'invite_deleted':
        return t('{actor} deleted an invite link', p);
      default:
        return `${e.actor}: ${e.action}`;
    }
  }

  const modes = $derived([
    ['open', t('Open to anyone'), t('Anyone who can reach this server can create an account.')],
    ['invite', t('Invite only'), t('People need an invite link from an admin.')],
    ['closed', t('Closed'), t('No new accounts, not even with an invite.')],
  ]);

  async function setRegistration(registration) {
    if (settings.registration === registration) return;
    try {
      settings = await adminUpdateSettings({ registration });
      refreshAudit();
    } catch (e) {
      toastError(e);
    }
  }

  const downloaderModes = $derived([
    ['off', t('Off'), t('Nobody can use it.')],
    ['admins', t('Admins only'), t('Only admins see "From a video link".')],
    ['everyone', t('Everyone'), t('Every account can use it.')],
  ]);

  async function setDownloader(downloader) {
    if (settings.downloader === downloader) return;
    try {
      settings = await adminUpdateSettings({ downloader });
      refreshAudit();
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
      refreshAudit();
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
      refreshAudit();
    } catch (e) {
      toastError(e);
    }
  }

  // used_by goes back to null if that account is deleted later; used_at stays.
  const inviteState = (i) =>
    i.used_at
      ? i.used_by
        ? t('Used by {name}', { name: i.used_by })
        : t('Used by an account that was deleted')
      : i.expires_at * 1000 < Date.now()
        ? t('Expired')
        : t('Expires {date}', { date: formatDate(i.expires_at * 1000) });

  async function update(user, body, message) {
    try {
      const next = await adminUpdateUser(user.id, body);
      users = users.map((u) => (u.id === next.id ? next : u));
      if (message) toast(message, { kind: 'success' });
      if (user.id === session.me.user_id) refreshMe().catch(() => {});
      refreshAudit();
      stats = await adminStats();
    } catch (e) {
      toastError(e);
    }
  }

  function menuFor(u) {
    const self = u.id === session.me.user_id;
    return [
      { label: t('Change quota'), icon: 'hard-drive', onclick: () => (dialog = { type: 'quota', user: u }) },
      { label: t('Transfer limits'), icon: 'refresh-cw', onclick: () => (dialog = { type: 'transfer', user: u }) },
      ...(!self
        ? [
            u.is_admin
              ? { label: t('Remove admin rights'), icon: 'user', onclick: () => update(u, { is_admin: false }, t('{name} is no longer an admin', { name: u.username })) }
              : { label: t('Make admin'), icon: 'key-round', onclick: () => update(u, { is_admin: true }, t('{name} is now an admin', { name: u.username })) },
            u.disabled
              ? { label: t('Enable account'), icon: 'door-open', onclick: () => update(u, { disabled: false }, t('{name} can sign in again', { name: u.username })) }
              : { label: t('Disable account'), icon: 'lock', onclick: () => update(u, { disabled: true }, t("{name} was signed out and can't sign in", { name: u.username })) },
            'sep',
            { label: t('Delete account'), icon: 'trash-2', danger: true, onclick: () => (dialog = { type: 'delete', user: u, typed: '' }) },
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
    if (!Number.isFinite(gb) || gb < 0) return (quotaError = t('Enter a number of GB, like 10 or 0.5.'));
    const u = dialog.user;
    dialog = null;
    await update(u, { quota_bytes: Math.round(gb * 1024 ** 3) }, t('{name} now has {size}', { name: u.username, size: formatSize(Math.round(gb * 1024 ** 3)) }));
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
    if (![down, up].every((b) => Number.isFinite(b) && b >= 0)) return (transferError = t('Enter a number of GB, like 5 or 0.5, or leave it empty for no limit.'));
    const u = dialog.user;
    dialog = null;
    await update(u, { daily_download_limit: down, daily_upload_limit: up }, down || up ? t('Limits set for {name}', { name: u.username }) : t('{name} has no transfer limits', { name: u.username }));
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

<h1 class="text-xl font-semibold tracking-tight">{t('Admin')}</h1>
<p class="mt-1 text-[13px] text-fg-muted">
  {t("Accounts and server settings. Admins can't see anyone's files, file names or keys; the numbers below are plain counts.")}
</p>

{#if !users || !stats || !settings || !invites || !audit}
  <div class="mt-6 grid gap-4" aria-hidden="true">
    <div class="grid grid-cols-2 gap-3 md:grid-cols-4">{#each [0, 1, 2, 3] as i (i)}<div class="skeleton h-24"></div>{/each}</div>
    <div class="skeleton h-64"></div>
  </div>
{:else}
  <div class="mt-6 grid grid-cols-2 gap-3 md:grid-cols-4 animate-enter">
    {@render stat(t('Accounts'), stats.users, stats.disabled_users ? t('{count} disabled', { count: stats.disabled_users }) : t('{count} active sessions', { count: stats.active_sessions }))}
    {@render stat(t('Storage used'), formatSize(stats.used_bytes), t('of {size} handed out', { size: formatSize(stats.quota_bytes) }))}
    {@render stat(t('Files'), stats.files, `${t('{count} folders', { count: stats.folders })} · ${t('{count} stored versions', { count: stats.versions })}`)}
    {@render stat(t('Sharing'), stats.shares, t('{count} public links', { count: stats.public_links }))}
  </div>

  <section class="card mt-6 overflow-hidden">
    <div class="grid gap-4 p-6">
      <div class="grid gap-1">
        <h2 class="text-base font-semibold tracking-tight">{t('Registration')}</h2>
        <p class="text-[13px] text-fg-muted">{t('Who can create an account on this server.')}</p>
      </div>
      <div class="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label={t('Registration')}>
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
          <h3 class="text-sm font-semibold">{t('Invite links')}</h3>
          <p class="text-[13px] text-fg-muted">
            {settings.registration !== 'invite' ? t('Each link creates one account. They only work while registration is Invite only.') : t('Each link creates one account.')}
          </p>
        </div>
        <div class="flex items-center gap-2">
          <label class="sr-only" for="invite-days">{t('Valid for')}</label>
          <select id="invite-days" class="input h-8 w-auto" bind:value={inviteDays}>
            {#each [1, 7, 30, 90] as d (d)}<option value={d}>{t('{count} days', { count: d })}</option>{/each}
          </select>
          <button type="button" class="btn btn-secondary" disabled={busy} onclick={createInvite}>
            {#if busy}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="user-round-plus" />{/if}
            {t('Create invite')}
          </button>
        </div>
      </div>

      {#if created}
        <div class="grid gap-2 rounded-md border border-accent bg-accent-soft p-3" transition:slide>
          <p class="text-[13px] font-medium text-accent-text">{t("New invite link. Copy it now; it can't be shown again.")}</p>
          <div class="flex items-center gap-2">
            <code class="min-w-0 flex-1 truncate rounded border border-line bg-bg px-2 py-1.5 font-mono text-xs select-all">{created.url}</code>
            <button type="button" class="btn btn-secondary" onclick={() => copyText(created.url, t('Invite link copied'))}><Icon name="copy" /> {t('Copy')}</button>
            <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Dismiss')} onclick={() => (created = null)}><Icon name="x" /></button>
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
                <span class="text-xs text-fg-faint"> · <Sentence text={t('created by {name} {when}')}>{#snippet name()}{inv.created_by}{/snippet}{#snippet when()}<Time ms={inv.created_at * 1000} relative />{/snippet}</Sentence></span>
              </span>
              {#if !inv.used_at}
                <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Delete invite')} title={t('Delete invite')} onclick={() => removeInvite(inv)}><Icon name="trash-2" /></button>
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
        <h2 class="text-base font-semibold tracking-tight">{t('Video downloader')}</h2>
        <p class="text-[13px] text-fg-muted">
          {t("Lets people save videos from YouTube and other sites into their files, using yt-dlp on this server. It's the one feature where the server sees what's being handled: the link and the video pass through it on their way to the browser, which then encrypts them. Nothing is stored.")}
        </p>
      </div>
      {#if settings.yt_dlp_version}
        <p class="flex items-center gap-2 text-[13px] text-fg-muted">
          <Icon name="check" class="size-4 text-success" />{t('yt-dlp {version} found · up to {size} per video', { version: settings.yt_dlp_version, size: formatSize(settings.downloader_max_bytes) })}
        </p>
        {#if !settings.downloader_can_merge}
          <p class="flex items-start gap-2 text-[13px] text-fg-muted">
            <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
            <span>{t("ffmpeg isn't installed, so only videos a site offers as one file can be saved. YouTube rarely does any more; install ffmpeg for video up to 1080p.")}</span>
          </p>
        {/if}
      {:else}
        <p class="flex items-start gap-2 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
          <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
          <span><Sentence text={t("yt-dlp isn't installed on this server, or isn't on its PATH. Install it (or point {flag} at it) and restart the server to use this.")}>{#snippet flag()}<code class="font-mono text-xs">--yt-dlp</code>{/snippet}</Sentence></span>
        </p>
      {/if}
      <div class="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label={t('Video downloader')}>
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
      <h2 class="text-base font-semibold tracking-tight">{t('Accounts')}</h2>
    </div>
    <table class="table">
      <thead>
        <tr>
          <th>{t('Username')}</th>
          <th class="hidden w-56 sm:table-cell">{t('Storage')}</th>
          <th class="hidden w-36 md:table-cell">{t('Last active')}</th>
          <th class="w-12"><span class="sr-only">{t('Actions')}</span></th>
        </tr>
      </thead>
      <tbody>
        {#each users as u (u.id)}
          <tr>
            <td class="max-w-0">
              <span class="flex items-center gap-2">
                <span class="truncate font-medium {u.disabled ? 'text-fg-muted line-through' : ''}">{u.username}</span>
                {#if u.id === session.me.user_id}<span class="badge">{t('You')}</span>{/if}
                {#if u.is_admin}<span class="badge badge-accent">{t('Admin')}</span>{/if}
                {#if u.disabled}<span class="badge text-danger">{t('Disabled')}</span>{/if}
              </span>
              <span class="text-xs text-fg-faint" title={fullDate(u.created_at * 1000)}>{t('Joined {date}', { date: formatDate(u.created_at * 1000) })}</span>
            </td>
            <td class="hidden sm:table-cell">
              <div class="grid gap-1.5">
                <span class="text-xs text-fg-muted tabular-nums">{t('{used} of {total}', { used: formatSize(u.used_bytes), total: formatSize(u.quota_bytes) })}</span>
                <div class="progress"><div style:width="{pct(u)}%"></div></div>
              </div>
            </td>
            <td class="hidden text-fg-muted md:table-cell" title={u.last_seen ? fullDate(u.last_seen * 1000) : ''}>
              {u.last_seen ? formatWhen(u.last_seen * 1000) : t('Not signed in')}
            </td>
            <td class="text-right"><Menu items={menuFor(u)} label={t('Actions for {name}', { name: u.username })} /></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>

  <section class="card mt-6 overflow-hidden">
    <div class="grid gap-1 p-6 pb-4">
      <h2 class="text-base font-semibold tracking-tight">{t('Admin activity')}</h2>
      <p class="text-[13px] text-fg-muted">{t('What admins changed on this server, kept for a year.')}</p>
    </div>
    {#if !audit.entries.length}
      <p class="px-6 pb-6 text-[13px] text-fg-muted">{t('Nothing yet.')}</p>
    {:else}
      <ul class="divide-y divide-line border-t border-line">
        {#each audit.entries as e (e.id)}
          <li class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-0.5 px-6 py-2.5 text-[13px]">
            <span class="min-w-0 break-words">{describe(e)}</span>
            <span class="text-xs text-fg-muted"><Time ms={e.at * 1000} relative /></span>
          </li>
        {/each}
      </ul>
      {#if audit.more}
        <div class="border-t border-line p-2 text-center">
          <button type="button" class="btn btn-ghost" disabled={auditLoading} onclick={olderAudit}>
            {#if auditLoading}<Icon name="loader-circle" class="spinner" />{/if}
            {t('Show older')}
          </button>
        </div>
      {/if}
    {/if}
  </section>
{/if}

{#if dialog?.type === 'quota'}
  <Modal title={t('Storage for {name}', { name: dialog.user.username })} description={t('Using {size} now. Lowering the quota below that blocks new uploads but deletes nothing.', { size: formatSize(dialog.user.used_bytes) })} onclose={() => (dialog = null)} onsubmit={saveQuota}>
    <div class="field">
      <label class="label" for="quota">{t('Quota in GB')}</label>
      <input id="quota" class="input" inputmode="decimal" bind:value={quotaGb} />
      {#if quotaError}<p class="text-[13px] text-danger">{quotaError}</p>{/if}
    </div>
    {#snippet footer()}
      <button type="button" class="btn btn-secondary" onclick={() => (dialog = null)}>{t('Cancel')}</button>
      <button class="btn btn-primary">{t('Save')}</button>
    {/snippet}
  </Modal>
{:else if dialog?.type === 'transfer'}
  <Modal
    title={t('Transfer limits for {name}', { name: dialog.user.username })}
    description={t('How much they can download and upload each day (UTC), counting what goes through their public links. Today so far: {down} down, {up} up.', { down: formatSize(dialog.user.downloaded_today), up: formatSize(dialog.user.uploaded_today) })}
    onclose={() => (dialog = null)}
    onsubmit={saveTransfer}>
    <div class="grid gap-3 sm:grid-cols-2">
      <div class="field">
        <label class="label" for="down-limit">{t('Downloads, GB a day')}</label>
        <input id="down-limit" class="input" inputmode="decimal" placeholder={t('No limit')} bind:value={downGb} />
      </div>
      <div class="field">
        <label class="label" for="up-limit">{t('Uploads, GB a day')}</label>
        <input id="up-limit" class="input" inputmode="decimal" placeholder={t('No limit')} bind:value={upGb} />
      </div>
    </div>
    {#if transferError}<p class="text-[13px] text-danger">{transferError}</p>{/if}
    {#snippet footer()}
      <button type="button" class="btn btn-secondary" onclick={() => (dialog = null)}>{t('Cancel')}</button>
      <button class="btn btn-primary">{t('Save')}</button>
    {/snippet}
  </Modal>
{:else if dialog?.type === 'delete'}
  <ConfirmDialog
    title={t('Delete {name}?', { name: dialog.user.username })}
    description={t("Their files, folders, versions, shares and links are deleted for good, and the storage is freed. Files they added to other people's shared folders stay with those people. This can't be undone.")}
    confirmLabel={t('Delete account')}
    danger
    disabled={dialog.typed !== dialog.user.username}
    onconfirm={async () => {
      const u = dialog.user;
      await adminDeleteUser(u.id);
      users = users.filter((x) => x.id !== u.id);
      refreshAudit();
      stats = await adminStats();
      toast(t('Deleted {name}', { name: u.username }), { kind: 'success' });
    }}
    onclose={() => (dialog = null)}>
    <div class="field">
      <label class="label" for="confirm-name"><Sentence text={t('Type {name} to confirm')}>{#snippet name()}<span class="font-mono">{dialog.user.username}</span>{/snippet}</Sentence></label>
      <input id="confirm-name" class="input" bind:value={dialog.typed} autocomplete="off" spellcheck="false" />
    </div>
  </ConfirmDialog>
{/if}
