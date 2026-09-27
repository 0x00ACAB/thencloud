<script>
  import { onMount } from 'svelte';
  import { session, changePassword, listSessions, revokeSession, revokeOtherSessions, removeRecoveryKey, listAppPasswords, deleteAppPassword, avatar, loadMyAvatar, setAvatar, removeAvatar, listContacts, forgetContact, forgetThisBrowser, listPasskeys, removePasskey, disableTotp } from '../../lib/cloud.svelte.js';
  import { passkeysSupported } from '../../lib/passkeys.js';
  import TotpDialog from '../dialogs/TotpDialog.svelte';
  import PasskeyDialog from '../dialogs/PasskeyDialog.svelte';
  import RecoveryKeyDialog from '../dialogs/RecoveryKeyDialog.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';
  import AppPasswordDialog from '../dialogs/AppPasswordDialog.svelte';
  import { theme, setTheme, toast, toastError, errorMessage, copyText, accent, setAccent, ACCENT_PRESETS, DEFAULT_ACCENT, contrast, accentForeground, iconPack, setIconPack, folderIcons, setFolderIcons, tint, setTint } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, formatDate, fullDate } from '../../lib/format.js';
  import { slide } from '../../lib/motion.js';
  import Icon from '../Icon.svelte';
  import Avatar from '../Avatar.svelte';
  import Time from '../Time.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';
  import { ICON_PACKS, hasFolderIcons } from '../../lib/file-icons.svelte.js';

  let current = $state('');
  let next = $state('');
  let confirm = $state('');
  let busy = $state(false);
  let error = $state('');

  const usedPct = $derived(Math.min(100, (session.me.used_bytes / Math.max(1, session.me.quota_bytes)) * 100));

  async function submit(e) {
    e.preventDefault();
    error = '';
    if (next !== confirm) return (error = "The new passwords don't match.");
    if (next.length < 10) return (error = 'Use at least 10 characters.');
    busy = true;
    try {
      await changePassword(current, next);
      current = next = confirm = '';
      toast('Password changed. Other devices were signed out.', { kind: 'success' });
    } catch (err) {
      error = err?.code === 'invalid_credentials' ? 'Your current password is wrong.' : errorMessage(err);
    } finally {
      busy = false;
    }
  }

  // Profile picture.
  let avatarBusy = $state(false);
  onMount(() => loadMyAvatar().catch(() => {}));

  async function pickAvatar(e) {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = '';
    if (!file) return;
    avatarBusy = true;
    try {
      await setAvatar(file);
      toast('Profile picture updated', { kind: 'success' });
    } catch (err) {
      toastError(err?.name === 'InvalidStateError' || err?.name === 'EncodingError' ? new Error("That image couldn't be read.") : err);
    } finally {
      avatarBusy = false;
    }
  }

  async function dropAvatar() {
    avatarBusy = true;
    try {
      await removeAvatar();
      toast('Profile picture removed');
    } catch (err) {
      toastError(err);
    } finally {
      avatarBusy = false;
    }
  }

  // Verified contacts.
  let contacts = $state(null);
  let contactsError = $state('');
  onMount(() => {
    listContacts()
      .then((c) => (contacts = c))
      .catch((e) => ((contactsError = errorMessage(e)), (contacts = [])));
  });

  async function forget(c) {
    try {
      await forgetContact(c.username);
      contacts = contacts.filter((x) => x.username !== c.username);
      toast(`Forgot ${c.username}'s key. You'll be asked to check it next time you share.`);
    } catch (e) {
      toastError(e);
    }
  }

  let recoveryDialog = $state(null); // 'create' | 'remove'
  let removePassword = $state('');

  // Two-step sign-in.
  let passkeys = $state(null);
  let twoStepDialog = $state(null); // 'totp' | 'totp-off' | 'passkey' | { passkey }
  let twoStepPassword = $state('');
  const twoStepOn = $derived(!!session.me.totp_created_at || !!passkeys?.length);
  onMount(() => {
    listPasskeys()
      .then((p) => (passkeys = p))
      .catch(() => (passkeys = []));
  });
  const openTwoStep = (d) => ((twoStepPassword = ''), (twoStepDialog = d));
  const wrongPassword = (e) => (e?.code === 'invalid_credentials' ? new Error('That password is wrong.') : e);

  // Signed-in devices.
  let devices = $state(null);
  let revoking = $state(null); // session id, or 'others'

  async function loadDevices() {
    try {
      devices = await listSessions();
    } catch (e) {
      toastError(e);
      devices = [];
    }
  }
  onMount(loadDevices);

  async function signOut(id) {
    revoking = id;
    try {
      if (id === 'others') await revokeOtherSessions();
      else await revokeSession(id);
      devices = devices.filter((d) => d.current || (id !== 'others' && d.id !== id));
      toast(id === 'others' ? 'Signed out your other devices' : 'Signed out', { kind: 'success' });
    } catch (e) {
      toastError(e);
    } finally {
      revoking = null;
    }
  }

  // App passwords.
  let appPasswords = $state(null);
  let appDialog = $state(false);
  let revokingApp = $state(null);

  async function loadAppPasswords() {
    try {
      appPasswords = await listAppPasswords();
    } catch (e) {
      toastError(e);
      appPasswords = [];
    }
  }
  onMount(loadAppPasswords);

  const deviceIcon = (name) => (/Android|iOS/.test(name) ? 'smartphone' : 'laptop');

  const themes = [
    ['system', 'monitor', 'System'],
    ['light', 'sun', 'Light'],
    ['dark', 'moon', 'Dark'],
  ];

  const SAMPLES = ['main.rs', 'index.ts', 'README.md', 'package.json', 'photo.jpg', 'report.pdf', 'backup.zip', 'song.mp3'].map((name) => ({
    name,
    mime: '',
  }));

  const isPreset = $derived(ACCENT_PRESETS.some(([hex]) => hex === accent.value));
  // Accent used as text/links needs to read on both backgrounds.
  const lowContrast = $derived(Math.min(contrast(accent.value, '#ffffff'), contrast(accent.value, '#0a0a0a')) < 2.2);
</script>

{#snippet section(title, description, body, footer)}
  <section class="card overflow-hidden">
    <div class="grid grid-cols-1 gap-4 p-6">
      <div class="grid gap-1">
        <h2 class="text-base font-semibold tracking-tight">{title}</h2>
        {#if description}<p class="text-[13px] text-fg-muted">{description}</p>{/if}
      </div>
      {@render body()}
    </div>
    {#if footer}
      <div class="flex items-center justify-end gap-3 border-t border-line bg-subtle px-6 py-3">{@render footer()}</div>
    {/if}
  </section>
{/snippet}

<h1 class="text-xl font-semibold tracking-tight">Settings</h1>

<div class="mt-6 grid grid-cols-1 gap-6">
  {#snippet accountBody()}
    <div class="flex flex-wrap items-center gap-4">
      {#if avatar.url}
        <img src={avatar.url} alt="You" class="size-16 rounded-full object-cover" />
      {:else}
        <span class="grid size-16 place-items-center rounded-full bg-muted text-xl font-semibold uppercase" aria-hidden="true">{session.me.username.slice(0, 1)}</span>
      {/if}
      <div class="grid gap-2">
        <div class="flex flex-wrap gap-2">
          <label class="btn btn-secondary {avatarBusy ? 'pointer-events-none opacity-60' : ''}">
            {#if avatarBusy}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="upload" />{/if}
            {avatar.url ? 'Change picture' : 'Add a picture'}
            <input type="file" accept="image/*" class="sr-only" onchange={pickAvatar} />
          </label>
          {#if avatar.url}<button type="button" class="btn btn-ghost" disabled={avatarBusy} onclick={dropAvatar}>Remove</button>{/if}
        </div>
        <p class="max-w-md text-xs text-fg-muted">Encrypted in this browser. Only people you share with, or who share with you, can see it; the server can't.</p>
      </div>
    </div>
    <dl class="grid gap-3 text-sm sm:grid-cols-[10rem_1fr]">
      <dt class="text-fg-muted">Username</dt>
      <dd class="font-medium">{session.me.username}{#if session.me.is_admin}<span class="badge ml-2">Admin</span>{/if}</dd>
      <dt class="text-fg-muted">Storage</dt>
      <dd class="grid max-w-sm gap-2">
        <span class="tabular-nums">{formatSize(session.me.used_bytes)} of {formatSize(session.me.quota_bytes)} used</span>
        <div class="progress"><div style:width="{usedPct}%"></div></div>
      </dd>
    </dl>
  {/snippet}
  {@render section('Account', null, accountBody)}

  {#snippet keyBody()}
    <div class="flex flex-wrap items-center gap-2">
      <code class="fingerprint rounded-md border border-line bg-subtle px-3 py-2 select-all">{session.fingerprint}</code>
      <button type="button" class="btn btn-ghost btn-icon" aria-label="Copy fingerprint" title="Copy" onclick={() => copyText(session.fingerprint, 'Fingerprint copied')}>
        <Icon name="copy" />
      </button>
    </div>
  {/snippet}
  {@render section(
    'Your key fingerprint',
    'When someone shares with you, they see this fingerprint. Read it to them over a call or in person so they can check it matches. That proves the server gave them your real key.',
    keyBody,
  )}

  {#snippet contactsBody()}
    {#if contacts === null}
      <div class="skeleton h-12 w-full" aria-hidden="true"></div>
    {:else if contactsError}
      <p class="flex items-center gap-2 text-[13px] text-danger"><Icon name="circle-alert" class="size-4" />{contactsError}</p>
    {:else if !contacts.length}
      <p class="text-[13px] text-fg-muted">None yet. When you share with someone and confirm their fingerprint, they're added here.</p>
    {:else}
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each contacts as c (c.username)}
          <li class="flex items-center gap-3 px-3 py-2.5" out:slide>
            <Avatar username={c.username} class="size-7 text-xs" />
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm font-medium">{c.username}</p>
              <p class="fingerprint truncate text-xs text-fg-muted">{c.fingerprint}</p>
            </div>
            <span class="hidden text-xs text-fg-faint sm:inline">Checked <Time ms={c.verifiedAt} relative /></span>
            <button type="button" class="btn btn-ghost h-7 px-2.5 text-[13px]" onclick={() => forget(c)}>Forget</button>
          </li>
        {/each}
      </ul>
    {/if}
  {/snippet}
  {@render section(
    'Verified contacts',
    "Keys you've checked by fingerprint. If the server ever gives you a different key for one of these people, sharing with them is stopped until you check again. This list is encrypted; the server can't read or change it.",
    contactsBody,
  )}

  {#snippet passwordBody()}
    <form id="password-form" class="grid max-w-sm gap-4" onsubmit={submit}>
      <div class="field">
        <label class="label" for="pw-current">Current password</label>
        <input id="pw-current" class="input" type="password" bind:value={current} autocomplete="current-password" required />
      </div>
      <div class="field">
        <label class="label" for="pw-new">New password</label>
        <input id="pw-new" class="input" type="password" bind:value={next} autocomplete="new-password" minlength="10" required />
      </div>
      <div class="field">
        <label class="label" for="pw-confirm">Confirm new password</label>
        <input id="pw-confirm" class="input" type="password" bind:value={confirm} autocomplete="new-password" required />
      </div>
      {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
    </form>
  {/snippet}
  {#snippet passwordFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">Your files stay as they are; only the key protecting them is re-wrapped.</p>
    <button form="password-form" class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      Change password
    </button>
  {/snippet}
  {@render section('Password', 'Changing it signs out your other devices. If you forget it, only your recovery key can get you back in.', passwordBody, passwordFooter)}

  {#snippet recoveryBody()}
    {#if session.me.recovery_created_at}
      <p class="flex items-center gap-2 text-sm"><Icon name="shield-check" class="size-4 text-success" />Set up on&nbsp;<Time ms={session.me.recovery_created_at * 1000} /></p>
    {:else}
      <p class="flex items-start gap-2 text-sm text-fg-muted">
        <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
        Not set up. If you forget your password, your files can't be recovered by anyone.
      </p>
    {/if}
  {/snippet}
  {#snippet recoveryFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">Made in this browser; the server never sees it.</p>
    {#if session.me.recovery_created_at}
      <button type="button" class="btn btn-ghost" onclick={() => ((removePassword = ''), (recoveryDialog = 'remove'))}>Remove</button>
    {/if}
    <button type="button" class="btn btn-secondary" onclick={() => (recoveryDialog = 'create')}>
      <Icon name="key-round" />{session.me.recovery_created_at ? 'Replace key' : 'Create recovery key'}
    </button>
  {/snippet}
  {@render section(
    'Recovery key',
    'A printable key that lets you set a new password if you forget yours, without losing your files. Keep it somewhere safe, away from this device.',
    recoveryBody,
    recoveryFooter,
  )}

  {#snippet twoStepBody()}
    <div class="grid gap-4">
      <div class="flex flex-wrap items-center gap-3">
        <Icon name="smartphone" class="size-4 shrink-0 text-fg-muted" />
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium">Authenticator app</p>
          <p class="text-xs text-fg-muted">
            {#if session.me.totp_created_at}On since <Time ms={session.me.totp_created_at * 1000} />{:else}Six-digit codes from an app on your phone{/if}
          </p>
        </div>
        {#if session.me.totp_created_at}
          <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => openTwoStep('totp-off')}>Turn off</button>
        {:else}
          <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => openTwoStep('totp')}>Set up</button>
        {/if}
      </div>
      {#if passkeys === null}
        <div class="skeleton h-12 w-full" aria-hidden="true"></div>
      {:else if passkeys.length}
        <ul class="divide-y divide-line rounded-md border border-line">
          {#each passkeys as p (p.id)}
            <li class="flex items-center gap-3 px-3 py-2.5" out:slide>
              <Icon name="fingerprint" class="size-4 shrink-0 text-fg-muted" />
              <div class="min-w-0 flex-1">
                <p class="flex items-center gap-2 text-sm">
                  <span class="truncate font-medium">{p.name}</span>
                  <span class="badge" title={p.unlock ? 'Signs you in without your password' : 'Its authenticator has no PRF support, so it only confirms a password sign-in'}>
                    {p.unlock ? 'Signs in on its own' : 'Second step only'}
                  </span>
                </p>
                <p class="truncate text-xs text-fg-muted">
                  Added <span title={fullDate(p.created_at * 1000)}>{formatDate(p.created_at * 1000)}</span>
                  · {#if p.last_used_at}last used <span title={fullDate(p.last_used_at * 1000)}>{formatWhen(p.last_used_at * 1000)}</span>{:else}never used{/if}
                </p>
              </div>
              <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => openTwoStep({ passkey: p })}>Remove</button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if twoStepOn && !session.me.recovery_created_at}
        <p class="flex items-start gap-2 text-[13px] text-fg-muted">
          <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
          Create a recovery key too. If you lose your phone and passkeys, it's the only way back in.
        </p>
      {/if}
    </div>
  {/snippet}
  {#snippet twoStepFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">App passwords and your recovery key skip this step.</p>
    {#if passkeysSupported()}
      <button type="button" class="btn btn-secondary" onclick={() => openTwoStep('passkey')}><Icon name="plus" />Add a passkey</button>
    {/if}
  {/snippet}
  {@render section(
    'Two-step sign-in',
    'With an authenticator app or a passkey set up, signing in with your password also asks for one of them, so a leaked password alone isn\'t enough.',
    twoStepBody,
    twoStepFooter,
  )}

  {#snippet devicesBody()}
    {#if devices === null}
      <div class="grid gap-2" aria-hidden="true">
        {#each [0, 1] as i (i)}<div class="skeleton h-12 w-full"></div>{/each}
      </div>
    {:else}
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each devices as d (d.id)}
          <li class="flex items-center gap-3 px-3 py-2.5" out:slide>
            <Icon name={deviceIcon(d.device_name)} class="size-4 shrink-0 text-fg-muted" />
            <div class="min-w-0 flex-1">
              <p class="flex items-center gap-2 text-sm">
                <span class="truncate font-medium">{d.device_name}</span>
                {#if d.current}<span class="badge badge-accent">This device</span>{/if}
                {#if d.app_password}<span class="badge"><Icon name="key-round" />{d.app_password}</span>{/if}
                {#if d.current && session.remembered}<span class="badge">Kept signed in</span>{/if}
              </p>
              <p class="truncate text-xs text-fg-muted">
                Signed in <span title={fullDate(d.created_at * 1000)}>{formatDate(d.created_at * 1000)}</span>
                {#if !d.current}· active <span title={fullDate(d.last_seen * 1000)}>{formatWhen(d.last_seen * 1000)}</span>{/if}
              </p>
            </div>
            {#if d.current && session.remembered}
              <button
                type="button"
                class="btn btn-secondary h-7 px-2.5 text-[13px]"
                title="Remove the saved keys from this browser; you'll need your password next time"
                onclick={async () => {
                  await forgetThisBrowser();
                  toast("This browser won't keep you signed in any more", { kind: 'success' });
                }}>Stop keeping signed in</button>
            {/if}
            {#if !d.current}
              <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" disabled={revoking !== null} onclick={() => signOut(d.id)}>
                {#if revoking === d.id}<Icon name="loader-circle" class="spinner" />{/if}
                Sign out
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/snippet}
  {#snippet devicesFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">Only the device name and times are kept, not IP addresses.</p>
    <button type="button" class="btn btn-secondary" disabled={revoking !== null || !devices || devices.length < 2} onclick={() => signOut('others')}>
      {#if revoking === 'others'}<Icon name="loader-circle" class="spinner" />{/if}
      Sign out other devices
    </button>
  {/snippet}
  {@render section(
    'Devices',
    'Where you are signed in. Signing a device out ends its session, and its keys are gone from memory the next time it tries to do anything.',
    devicesBody,
    devicesFooter,
  )}

  {#snippet appBody()}
    {#if appPasswords === null}
      <div class="skeleton h-12 w-full" aria-hidden="true"></div>
    {:else if !appPasswords.length}
      <p class="text-[13px] text-fg-muted">None yet.</p>
    {:else}
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each appPasswords as a (a.id)}
          <li class="flex items-center gap-3 px-3 py-2.5" out:slide>
            <Icon name="key-round" class="size-4 shrink-0 text-fg-muted" />
            <div class="min-w-0 flex-1">
              <p class="flex items-center gap-2 text-sm">
                <span class="truncate font-medium">{a.name}</span>
                <span class="badge">{a.scope === 'read' ? 'Read only' : 'Full access'}</span>
              </p>
              <p class="truncate text-xs text-fg-muted">
                Created <span title={fullDate(a.created_at * 1000)}>{formatDate(a.created_at * 1000)}</span>
                · {#if a.last_used_at}last used <span title={fullDate(a.last_used_at * 1000)}>{formatWhen(a.last_used_at * 1000)}</span>{:else}never used{/if}
              </p>
            </div>
            <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => (revokingApp = a)}>Revoke</button>
          </li>
        {/each}
      </ul>
    {/if}
  {/snippet}
  {#snippet appFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">Your password changing doesn't affect them.</p>
    <button type="button" class="btn btn-secondary" onclick={() => (appDialog = true)}><Icon name="plus" />New app password</button>
  {/snippet}
  {@render section(
    'App passwords',
    'Sign in sync clients and other devices without giving them your account password. Each one can be read only, and revoking it signs that device out.',
    appBody,
    appFooter,
  )}

  {#snippet themeBody()}
    <div class="grid max-w-md grid-cols-3 gap-2" role="radiogroup" aria-label="Theme">
      {#each themes as [value, icon, label] (value)}
        <button
          type="button"
          role="radio"
          aria-checked={theme.pref === value}
          class="flex h-10 cursor-pointer items-center justify-center gap-2 rounded-md border text-sm transition-colors {theme.pref === value
            ? 'border-accent bg-accent-soft font-medium text-accent-text'
            : 'border-line text-fg-muted hover:bg-muted hover:text-fg'}"
          onclick={() => setTheme(value)}>
          <Icon name={icon} />{label}
        </button>
      {/each}
    </div>
  {/snippet}
  {@render section('Appearance', null, themeBody)}

  {#snippet accentBody()}
    <div class="grid gap-4">
      <div class="flex flex-wrap items-center gap-2" role="radiogroup" aria-label="Accent colour">
        {#each ACCENT_PRESETS as [hex, name] (hex)}
          <button
            type="button"
            role="radio"
            aria-checked={accent.value === hex}
            aria-label={name}
            title={name}
            class="grid size-8 cursor-pointer place-items-center rounded-full ring-offset-2 ring-offset-bg transition-[scale,box-shadow] duration-150 hover:scale-110 {accent.value === hex
              ? 'ring-2 ring-fg'
              : ''}"
            style:background-color={hex}
            style:color={accentForeground(hex)}
            onclick={() => setAccent(hex)}>
            {#if accent.value === hex}<Icon name="check" class="size-4" />{/if}
          </button>
        {/each}
        <span class="mx-1 h-6 w-px bg-line" aria-hidden="true"></span>
        <label
          class="relative grid size-8 cursor-pointer place-items-center rounded-full border border-dashed border-line-strong text-fg-muted hover:text-fg {!isPreset
            ? 'ring-2 ring-fg ring-offset-2 ring-offset-bg'
            : ''}"
          title="Custom colour"
          style:background-color={isPreset ? null : accent.value}>
          <Icon name="plus" class="size-4 {isPreset ? '' : 'opacity-0'}" />
          <input type="color" class="absolute inset-0 cursor-pointer opacity-0" value={accent.value} aria-label="Custom accent colour" oninput={(e) => setAccent(e.currentTarget.value)} />
        </label>
      </div>
      <div class="flex flex-wrap items-center gap-3 rounded-md border border-line bg-subtle p-4">
        <button type="button" class="btn btn-accent" tabindex="-1">Primary action</button>
        <span class="badge badge-accent">Can edit</span>
        <span class="link text-sm">A link</span>
        <span class="flex items-center gap-1.5 text-sm"><Icon name="folder" class="size-4 text-accent-text" />Folder</span>
      </div>
      <label class="flex cursor-pointer items-start gap-3 text-sm">
        <input type="checkbox" class="mt-0.5 size-4 accent-accent" checked={tint.on} onchange={(e) => setTint(e.currentTarget.checked)} />
        <span class="grid gap-0.5">
          <span>Tint the whole theme</span>
          <span class="text-xs text-fg-muted">Backgrounds, borders and grey text take on a hint of the accent's hue.</span>
        </span>
      </label>
      {#if lowContrast}
        <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="circle-alert" class="size-4" />This colour is hard to read on one of the themes; text uses an adjusted shade.</p>
      {/if}
    </div>
  {/snippet}
  {#snippet accentFooter()}
    <p class="mr-auto text-xs text-fg-muted">Saved on this device only.</p>
    <button type="button" class="btn btn-secondary" disabled={accent.value === DEFAULT_ACCENT} onclick={() => setAccent(DEFAULT_ACCENT)}>Reset to blue</button>
  {/snippet}
  {@render section('Accent colour', 'Used for buttons, links, folders and highlights, and optionally to tint everything else.', accentBody, accentFooter)}

  {#snippet iconsBody()}
    <div class="grid gap-2 sm:grid-cols-2" role="radiogroup" aria-label="File icons">
      {#each ICON_PACKS as pack (pack.id)}
        <button
          type="button"
          role="radio"
          aria-checked={iconPack.value === pack.id}
          class="grid cursor-pointer gap-3 rounded-md border p-3 text-left transition-colors {iconPack.value === pack.id
            ? 'border-accent bg-accent-soft'
            : 'border-line hover:bg-subtle'}"
          onclick={() => setIconPack(pack.id)}>
          <span class="flex items-baseline justify-between gap-2">
            <span class="text-sm font-medium {iconPack.value === pack.id ? 'text-accent-text' : ''}">{pack.name}</span>
            <span class="truncate text-xs text-fg-muted">{pack.credit}</span>
          </span>
          <span class="flex gap-2.5" aria-hidden="true">
            {#each SAMPLES as meta (meta.name)}<FileIcon {meta} pack={pack.id} class="size-5" />{/each}
          </span>
        </button>
      {/each}
    </div>
    <label class="flex cursor-pointer items-center gap-3 text-sm {hasFolderIcons(iconPack.value) ? '' : 'pointer-events-none opacity-50'}">
      <input
        type="checkbox"
        class="size-4 accent-accent"
        checked={folderIcons.named}
        disabled={!hasFolderIcons(iconPack.value)}
        onchange={(e) => setFolderIcons(e.currentTarget.checked)} />
      <span>Folder icons by name</span>
      <span class="flex gap-2" aria-hidden="true">
        {#each ['src', 'images', 'docs', 'music'] as name (name)}<FolderIcon {name} named={hasFolderIcons(iconPack.value)} class="size-5" />{/each}
      </span>
    </label>
    {#if !hasFolderIcons(iconPack.value)}<p class="-mt-2 text-xs text-fg-muted">Material and Symbols have icons for common folder names.</p>{/if}
  {/snippet}
  {@render section('File icons', 'Icons for files by type, and optionally for folders by name.', iconsBody)}

  {#snippet aboutBody()}
    <div class="grid gap-2 text-[13px] text-fg-muted">
      <p>
        thencloud is free software under the AGPL-3.0. Your files are encrypted in this browser with keys derived from your password; the server stores
        ciphertext and can see only sizes, dates and who shares with whom.
      </p>
      <p>
        This page is code the server sends you. Each release is built reproducibly with a signed list of every file's hash, and
        <code class="font-mono text-fg">thencloud verify-web</code> checks that a server sends exactly that.
      </p>
    </div>
  {/snippet}
  {@render section('About', null, aboutBody)}
</div>

{#if appDialog}
  <AppPasswordDialog onclose={() => (appDialog = false)} oncreated={loadAppPasswords} />
{/if}

{#if revokingApp}
  <ConfirmDialog
    title="Revoke {revokingApp.name}?"
    description="It stops working straight away, and anything signed in with it is signed out."
    confirmLabel="Revoke"
    danger
    onconfirm={async () => {
      await deleteAppPassword(revokingApp.id);
      appPasswords = appPasswords.filter((x) => x.id !== revokingApp.id);
      loadDevices();
      toast('App password revoked');
    }}
    onclose={() => (revokingApp = null)} />
{/if}

{#if twoStepDialog === 'totp'}
  <TotpDialog onclose={() => (twoStepDialog = null)} ondone={() => toast('Authenticator app turned on', { kind: 'success' })} />
{:else if twoStepDialog === 'passkey'}
  <PasskeyDialog
    onclose={() => (twoStepDialog = null)}
    onadded={(p) => {
      passkeys = [...(passkeys ?? []), p];
      toast(p.unlock ? 'Passkey added. It can sign you in on its own.' : 'Passkey added as a second step.', { kind: 'success' });
    }} />
{:else if twoStepDialog}
  {@const passkey = twoStepDialog.passkey}
  <ConfirmDialog
    title={passkey ? `Remove ${passkey.name}?` : 'Turn off the authenticator app?'}
    description={passkey ? 'It stops working for this account straight away.' : 'Signing in will no longer ask for its codes.'}
    confirmLabel={passkey ? 'Remove' : 'Turn off'}
    danger
    disabled={!twoStepPassword}
    onconfirm={async () => {
      try {
        if (passkey) {
          await removePasskey(passkey.id, twoStepPassword);
          passkeys = passkeys.filter((x) => x.id !== passkey.id);
        } else {
          await disableTotp(twoStepPassword);
        }
      } catch (e) {
        throw wrongPassword(e);
      }
      toast(passkey ? 'Passkey removed' : 'Authenticator app turned off');
    }}
    onclose={() => (twoStepDialog = null)}>
    <div class="field">
      <label class="label" for="two-step-password">Your password</label>
      <input id="two-step-password" class="input" type="password" bind:value={twoStepPassword} autocomplete="current-password" />
    </div>
  </ConfirmDialog>
{/if}

{#if recoveryDialog === 'create'}
  <RecoveryKeyDialog onclose={() => (recoveryDialog = null)} />
{:else if recoveryDialog === 'remove'}
  <ConfirmDialog
    title="Remove your recovery key?"
    description="The key stops working straight away. If you then forget your password, your files can't be recovered."
    confirmLabel="Remove key"
    danger
    disabled={!removePassword}
    onconfirm={async () => {
      try {
        await removeRecoveryKey(removePassword);
      } catch (e) {
        throw e?.code === 'invalid_credentials' ? new Error('That password is wrong.') : e;
      }
      toast('Recovery key removed');
    }}
    onclose={() => (recoveryDialog = null)}>
    <div class="field">
      <label class="label" for="rm-password">Your password</label>
      <input id="rm-password" class="input" type="password" bind:value={removePassword} autocomplete="current-password" />
    </div>
  </ConfirmDialog>
{/if}
