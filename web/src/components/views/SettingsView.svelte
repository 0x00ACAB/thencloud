<script>
  import { onMount } from 'svelte';
  import { session, changePassword, listSessions, revokeSession, revokeOtherSessions, removeRecoveryKey } from '../../lib/cloud.svelte.js';
  import RecoveryKeyDialog from '../dialogs/RecoveryKeyDialog.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';
  import { theme, setTheme, toast, toastError, errorMessage, copyText, accent, setAccent, ACCENT_PRESETS, DEFAULT_ACCENT, contrast, accentForeground, iconPack, setIconPack } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, formatDate, fullDate } from '../../lib/format.js';
  import { slide } from '../../lib/motion.js';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import { ICON_PACKS } from '../../lib/file-icons.svelte.js';

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

  let recoveryDialog = $state(null); // 'create' | 'remove'
  let removePassword = $state('');

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
    <div class="grid gap-4 p-6">
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

<div class="mt-6 grid gap-6">
  {#snippet accountBody()}
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
  {@render section('Password', 'Changing it signs out your other devices. There is still no way to reset it if you forget it.', passwordBody, passwordFooter)}

  {#snippet recoveryBody()}
    {#if session.me.recovery_created_at}
      <p class="flex items-center gap-2 text-sm"><Icon name="shield-check" class="size-4 text-success" />Set up on {formatDate(session.me.recovery_created_at * 1000)}</p>
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
              </p>
              <p class="truncate text-xs text-fg-muted">
                Signed in <span title={fullDate(d.created_at * 1000)}>{formatDate(d.created_at * 1000)}</span>
                {#if !d.current}· active <span title={fullDate(d.last_seen * 1000)}>{formatWhen(d.last_seen * 1000)}</span>{/if}
              </p>
            </div>
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
      {#if lowContrast}
        <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="circle-alert" class="size-4" />This colour is hard to read on one of the themes; text uses an adjusted shade.</p>
      {/if}
    </div>
  {/snippet}
  {#snippet accentFooter()}
    <p class="mr-auto text-xs text-fg-muted">Saved on this device only.</p>
    <button type="button" class="btn btn-secondary" disabled={accent.value === DEFAULT_ACCENT} onclick={() => setAccent(DEFAULT_ACCENT)}>Reset to blue</button>
  {/snippet}
  {@render section('Accent colour', 'Used for buttons, links, folders and highlights.', accentBody, accentFooter)}

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
  {/snippet}
  {@render section('File icons', 'Icons for files by type. Folders keep the same icon either way.', iconsBody)}

  {#snippet aboutBody()}
    <div class="grid gap-2 text-[13px] text-fg-muted">
      <p>
        thencloud is free software under the AGPL-3.0. Your files are encrypted in this browser with keys derived from your password; the server stores
        ciphertext and can see only sizes, dates and who shares with whom.
      </p>
      <p>Parts of thencloud are written with the help of AI. The source is public so you can check exactly what it does.</p>
    </div>
  {/snippet}
  {@render section('About', null, aboutBody)}
</div>

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
