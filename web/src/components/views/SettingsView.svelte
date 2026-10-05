<script>
  import { MODULES, modules, loadModules, setModule } from '../../lib/modules.svelte.js';
  import { t, LANGUAGES, language } from '../../lib/i18n.svelte.js';
  import { format, setFormat, REGIONS, autoRegionTag, unitSystem, formatDateTime, formatNumber } from '../../lib/locale.svelte.js';
  import { onMount } from 'svelte';
  import { session, changePassword, listSessions, revokeSession, revokeOtherSessions, removeRecoveryKey, listAppPasswords, deleteAppPassword, avatar, loadMyAvatar, setAvatar, removeAvatar, setDisplayName, cleanDisplayName, setPersonDetails, cleanPronoun, listContacts, forgetContact, forgetThisBrowser, myTransfer, listPasskeys, removePasskey, disableTotp, deleteAccount, verifyTree, resolvePath, exportAccount, saveAppData } from '../../lib/cloud.svelte.js';
  import { passkeysSupported } from '../../lib/passkeys.js';
  import TotpDialog from '../dialogs/TotpDialog.svelte';
  import PasskeyDialog from '../dialogs/PasskeyDialog.svelte';
  import RecoveryKeyDialog from '../dialogs/RecoveryKeyDialog.svelte';
  import ConfirmDialog from '../dialogs/ConfirmDialog.svelte';
  import AppPasswordDialog from '../dialogs/AppPasswordDialog.svelte';
  import { theme, setTheme, toast, toastError, errorMessage, copyText, accent, setAccent, ACCENT_PRESETS, DEFAULT_ACCENT, contrast, accentForeground, iconPack, setIconPack, folderIcons, setFolderIcons, tint, setTint, photoDetails, setPhotoDetails } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, formatDate, fullDate } from '../../lib/format.js';
  import { slide } from '../../lib/motion.js';
  import Icon from '../Icon.svelte';
  import Avatar from '../Avatar.svelte';
  import PersonName from '../PersonName.svelte';
  import Time from '../Time.svelte';
  import Sentence from '../Sentence.svelte';
  import { serverOrigin, inApp } from '../../lib/server.svelte.js';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';
  import { ICON_PACKS, hasFolderIcons } from '../../lib/file-icons.svelte.js';

  // Preset colour and icon pack names, in the chosen language.
  const accentNames = $derived({
    Blue: t('Blue'),
    Teal: t('Teal'),
    Green: t('Green'),
    Amber: t('Amber'),
    Orange: t('Orange'),
    Red: t('Red'),
    Pink: t('Pink'),
    Violet: t('Violet'),
    Slate: t('Slate'),
  });
  const packNames = $derived({ minimal: t('Minimal'), documents: t('Documents') });

  // Checking that everything decrypts.
  // The release this page is (from the build) and how to check a server sends it.
  const appVersion = document.querySelector('meta[name="thencloud-version"]')?.getAttribute('content') || 'dev';
  const verifyCommand = $derived(`thencloud verify-web ${serverOrigin() ?? location.origin} --manifest thencloud-web-${appVersion}.json`);
  const watched = typeof navigator !== 'undefined' && !!navigator.serviceWorker?.controller;
  let checkRun = $state(null); // { files, folders, bytes, problems, done, stopped }
  let checkCtl = null;
  onMount(() => () => checkCtl?.abort());

  async function checkFiles() {
    checkCtl?.abort();
    const ctl = (checkCtl = new AbortController());
    const run = (checkRun = { files: 0, folders: 0, bytes: 0, problems: [], done: false, stopped: false });
    try {
      const { items } = await resolvePath(session.me.keys.root_node_id);
      await verifyTree(items[0], {
        signal: ctl.signal,
        onProgress: (p) => Object.assign(run, p),
        onProblem: (p) => run.problems.push(p),
      });
    } catch (e) {
      run.problems.push({ location: [], name: null, id: '', error: errorMessage(e) });
    }
    run.done = true;
    run.stopped = ctl.signal.aborted;
  }

  // Everything decrypted into one zip.
  let exporting = $state(null); // { progress } while running

  async function exportData() {
    const run = (exporting = { progress: 0 });
    try {
      await exportAccount((p) => (run.progress = p));
      toast('Export finished', { kind: 'success' });
    } catch (e) {
      toastError(e);
    } finally {
      exporting = null;
    }
  }

  let deleting = $state(false);
  let deletePassword = $state('');
  let deleteConfirm = $state('');

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
    if (next.length < 10) return (error = t('Use at least 10 characters.'));
    busy = true;
    try {
      await changePassword(current, next);
      current = next = confirm = '';
      toast('Password changed. Other devices were signed out.', { kind: 'success' });
    } catch (err) {
      error = err?.code === 'invalid_credentials' ? t('Your current password is wrong.') : errorMessage(err);
    } finally {
      busy = false;
    }
  }

  // Daily transfer limits, shown only when an admin set some.
  let transfer = $state(null);
  onMount(() => myTransfer().then((info) => (transfer = info)).catch(() => {}));

  // Optional modules (and the language and region settings, in the same app data).
  onMount(() => loadModules().catch(() => {}));

  // Language and region.
  // What "Automatic" means right now, named in the interface's language.
  const browserRegion = $derived.by(() => {
    format.language;
    const tag = autoRegionTag();
    try {
      return new Intl.DisplayNames([language()], { type: 'language' }).of(tag) ?? tag;
    } catch {
      return tag;
    }
  });
  function changeFormat(change) {
    setFormat(change, (f) => saveAppData('prefs', f))?.catch(toastError);
  }
  const example = $derived.by(() => {
    format.region;
    format.clock;
    format.units;
    const metric = unitSystem() === 'metric';
    return `${formatDateTime(Date.UTC(2026, 8, 28, 21, 30), { dateStyle: 'medium', timeStyle: 'short' })} · ${formatNumber(metric ? 72.4 : 159.6)} ${metric ? 'kg' : 'lb'} · ${formatNumber(1234567.89)}`;
  });
  async function toggleModule(m, on) {
    try {
      await setModule(m.name, on);
      toast(on ? t("{name} is on. It's in the sidebar.", { name: m.label }) : t('{name} is off', { name: m.label }), { kind: on ? 'success' : 'info' });
    } catch (err) {
      toastError(err);
    }
  }

  // Profile picture and display name.
  let avatarBusy = $state(false);
  let nameDraft = $state('');
  let nameBusy = $state(false);
  onMount(() =>
    loadMyAvatar()
      .then((a) => (nameDraft = a.name ?? ''))
      .catch(() => {}),
  );
  // Pronouns (English) and grammatical gender, filled in like the example
  // sentences read.
  let pro = $state({ subject: '', object: '', possessive: '', gender: null });
  let proBusy = $state(false);
  const fromDetails = (d = {}) => ({ subject: d.subject ?? '', object: d.object ?? '', possessive: d.possessive ?? '', gender: d.gender ?? null });
  onMount(() =>
    loadMyAvatar()
      .then((a) => (pro = fromDetails(a.details)))
      .catch(() => {}),
  );
  const proValid = $derived(['subject', 'object', 'possessive'].every((k) => cleanPronoun(pro[k]) !== null));
  const proChanged = $derived.by(() => {
    const now = fromDetails(avatar.details);
    return ['subject', 'object', 'possessive'].some((k) => (cleanPronoun(pro[k]) ?? pro[k]) !== now[k]) || pro.gender !== now.gender;
  });
  async function savePronouns(e) {
    e.preventDefault();
    if (!proValid || !proChanged) return;
    proBusy = true;
    try {
      await setPersonDetails(pro);
      pro = fromDetails(avatar.details);
      toast(t('Pronouns saved'), { kind: 'success' });
    } catch (err) {
      toastError(err);
    } finally {
      proBusy = false;
    }
  }

  const nameValid = $derived(!nameDraft.trim() || cleanDisplayName(nameDraft) !== null);
  const nameChanged = $derived((nameDraft.trim() ? cleanDisplayName(nameDraft) : null) !== avatar.name);

  async function saveName(e) {
    e.preventDefault();
    if (!nameValid || !nameChanged) return;
    nameBusy = true;
    try {
      await setDisplayName(nameDraft);
      nameDraft = avatar.name ?? '';
      toast(avatar.name ? t('Display name saved') : t('Display name removed'), { kind: 'success' });
    } catch (err) {
      toastError(err);
    } finally {
      nameBusy = false;
    }
  }

  async function pickAvatar(e) {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = '';
    if (!file) return;
    avatarBusy = true;
    try {
      await setAvatar(file);
      toast('Profile picture updated', { kind: 'success' });
    } catch (err) {
      toastError(err?.name === 'InvalidStateError' || err?.name === 'EncodingError' ? new Error(t("That image couldn't be read.")) : err);
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
      toast(t("Forgot {name}'s key. You'll be asked to check it next time you share.", { name: c.username }));
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
  const wrongPassword = (e) => (e?.code === 'invalid_credentials' ? new Error(t('That password is wrong.')) : e);

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
      toast(id === 'others' ? t('Signed out your other devices') : t('Signed out'), { kind: 'success' });
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

  const themes = $derived([
    ['system', 'monitor', t('System')],
    ['light', 'sun', t('Light')],
    ['dark', 'moon', t('Dark')],
  ]);

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

<h1 class="text-xl font-semibold tracking-tight">{t('Settings')}</h1>

<div class="mt-6 grid grid-cols-1 gap-6">
  {#snippet accountBody()}
    <div class="flex flex-wrap items-center gap-4">
      {#if avatar.url}
        <img src={avatar.url} alt={t('You')} class="size-16 rounded-full object-cover" />
      {:else}
        <span class="grid size-16 place-items-center rounded-full bg-muted text-xl font-semibold uppercase" aria-hidden="true">{[...(avatar.name ?? session.me.username)][0]}</span>
      {/if}
      <div class="grid gap-2">
        <div class="flex flex-wrap gap-2">
          <label class="btn btn-secondary {avatarBusy ? 'pointer-events-none opacity-60' : ''}">
            {#if avatarBusy}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="upload" />{/if}
            {avatar.url ? t('Change picture') : t('Add a picture')}
            <input type="file" accept="image/*" class="sr-only" onchange={pickAvatar} />
          </label>
          {#if avatar.url}<button type="button" class="btn btn-ghost" disabled={avatarBusy} onclick={dropAvatar}>{t('Remove')}</button>{/if}
        </div>
        <p class="max-w-md text-xs text-fg-muted">{t('Only people you share with, or who share with you, can see it.')}</p>
      </div>
    </div>
    <dl class="grid gap-3 text-sm sm:grid-cols-[10rem_1fr]">
      <dt class="text-fg-muted"><label for="display-name">{t('Display name')}</label></dt>
      <dd class="grid max-w-sm gap-1.5">
        <form class="flex gap-2" onsubmit={saveName}>
          <input id="display-name" class="input" bind:value={nameDraft} maxlength="64" autocomplete="name" placeholder={t('None')} aria-invalid={!nameValid} aria-describedby="display-name-hint" />
          <button class="btn btn-secondary shrink-0" disabled={nameBusy || !nameValid || !nameChanged}>
            {#if nameBusy}<Icon name="loader-circle" class="spinner" />{/if}
            {t('Save')}
          </button>
        </form>
        <p id="display-name-hint" class="text-xs {nameValid ? 'text-fg-muted' : 'text-danger'}">
          {nameValid ? t('Any script, up to 64 characters. Shown next to your username, to the same people who see your picture.') : t("That name has characters that can't be used.")}
        </p>
      </dd>
      <dt class="text-fg-muted">{t('Pronouns')}</dt>
      <dd>
        {#snippet blank(key, placeholder)}
          <input
            class="input inline-block h-7 w-24 px-2 align-baseline"
            bind:value={pro[key]}
            {placeholder}
            maxlength="24"
            autocomplete="off"
            spellcheck="false"
            aria-label={{ subject: t('Pronoun as the subject, like "they"'), object: t('Pronoun as the object, like "them"'), possessive: t('Possessive pronoun, like "their"') }[key]} />
        {/snippet}
        <form class="grid max-w-md gap-3" onsubmit={savePronouns}>
          <p class="text-xs text-fg-muted">{t('How thencloud refers to you when it talks about you to others in English. Fill in the blanks the way you like; empty ones read they, them and their.')}</p>
          <div class="grid gap-2 rounded-md border border-line bg-subtle p-3 text-sm leading-7" lang="en">
            <p>Yesterday, {@render blank('subject', 'they')} shared a folder with you.</p>
            <p>Send {@render blank('object', 'them')} a file.</p>
            <p>Ask {avatar.name ?? session.me.username} to read you {@render blank('possessive', 'their')} key fingerprint.</p>
          </div>
          <div class="grid gap-1.5">
            <p class="text-[13px] font-medium" id="gender-label">{t('Grammatical gender')}</p>
            <div class="flex w-fit flex-wrap rounded-md border border-line p-0.5" role="radiogroup" aria-labelledby="gender-label">
              {#each [[null, t('Not set')], ['feminine', t('Feminine')], ['masculine', t('Masculine')], ['neuter', t('Neuter')]] as [value, label] (value ?? 'none')}
                <button type="button" role="radio" aria-checked={pro.gender === value} class="h-7 cursor-pointer rounded px-3 text-[13px] {pro.gender === value ? 'bg-muted font-medium text-fg' : 'text-fg-muted hover:text-fg'}" onclick={() => (pro.gender = value)}>{label}</button>
              {/each}
            </div>
            <p class="text-xs text-fg-muted">{t('For languages whose words change with it, like Polish "przeczytała" or "przeczytał". Not set keeps the wording neutral.')}</p>
          </div>
          {#if !proValid}<p class="text-xs text-danger">{t("Those pronouns have characters that can't be used.")}</p>{/if}
          <div>
            <button class="btn btn-secondary" disabled={proBusy || !proValid || !proChanged}>
              {#if proBusy}<Icon name="loader-circle" class="spinner" />{/if}
              {t('Save')}
            </button>
          </div>
          <p class="text-xs text-fg-muted">{t('Shown only to the people who see your picture and display name.')}</p>
        </form>
      </dd>
      <dt class="text-fg-muted">{t('Username')}</dt>
      <dd class="font-medium">{session.me.username}{#if session.me.is_admin}<span class="badge ml-2">{t('Admin')}</span>{/if}</dd>
      <dt class="text-fg-muted">{t('Storage')}</dt>
      <dd class="grid max-w-sm gap-2">
        <span class="tabular-nums">{t('{used} of {quota} used', { used: formatSize(session.me.used_bytes), quota: formatSize(session.me.quota_bytes) })}</span>
        <div class="progress"><div style:width="{usedPct}%"></div></div>
      </dd>
      {#if transfer && (transfer.daily_download_limit || transfer.daily_upload_limit)}
        <dt class="text-fg-muted">{t("Today's transfers")}</dt>
        <dd class="grid max-w-sm gap-0.5 text-[13px] tabular-nums">
          {#if transfer.daily_download_limit}<span>{t('{used} of {limit} downloaded', { used: formatSize(transfer.downloaded_today), limit: formatSize(transfer.daily_download_limit) })}</span>{/if}
          {#if transfer.daily_upload_limit}<span>{t('{used} of {limit} uploaded', { used: formatSize(transfer.uploaded_today), limit: formatSize(transfer.daily_upload_limit) })}</span>{/if}
          <span class="text-xs text-fg-muted">{t("Limits set by your server's admin. They start again at midnight UTC.")}</span>
        </dd>
      {/if}
    </dl>
  {/snippet}
  {@render section(t('Account'), null, accountBody)}

  {#snippet keyBody()}
    <div class="flex flex-wrap items-center gap-2">
      <code class="fingerprint rounded-md border border-line bg-subtle px-3 py-2 select-all">{session.fingerprint}</code>
      <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Copy fingerprint')} title={t('Copy')} onclick={() => copyText(session.fingerprint, t('Fingerprint copied'))}>
        <Icon name="copy" />
      </button>
    </div>
  {/snippet}
  {@render section(
    t('Your key fingerprint'),
    t('When someone shares with you, they see this fingerprint. Read it to them over a call or in person so they can check it matches. That proves the server gave them your real key.'),
    keyBody,
  )}

  {#snippet contactsBody()}
    {#if contacts === null}
      <div class="skeleton h-12 w-full" aria-hidden="true"></div>
    {:else if contactsError}
      <p class="flex items-center gap-2 text-[13px] text-danger"><Icon name="circle-alert" class="size-4" />{contactsError}</p>
    {:else if !contacts.length}
      <p class="text-[13px] text-fg-muted">{t("None yet. When you share with someone and confirm their fingerprint, they're added here.")}</p>
    {:else}
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each contacts as c (c.username)}
          <li class="flex items-center gap-3 px-3 py-2.5" out:slide>
            <Avatar username={c.username} class="size-7 text-xs" />
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm font-medium"><PersonName username={c.username} /></p>
              <p class="fingerprint truncate text-xs text-fg-muted">{c.fingerprint}</p>
            </div>
            <span class="hidden text-xs text-fg-faint sm:inline"><Sentence text={t('Checked {when}')}>{#snippet when()}<Time ms={c.verifiedAt} relative />{/snippet}</Sentence></span>
            <button type="button" class="btn btn-ghost h-7 px-2.5 text-[13px]" onclick={() => forget(c)}>{t('Forget')}</button>
          </li>
        {/each}
      </ul>
    {/if}
  {/snippet}
  {@render section(
    t('Verified contacts'),
    t("Keys you've checked by fingerprint. If the server ever gives you a different key for one of these people, sharing with them is stopped until you check again."),
    contactsBody,
  )}

  {#snippet passwordBody()}
    <form id="password-form" class="grid max-w-sm gap-4" onsubmit={submit}>
      <div class="field">
        <label class="label" for="pw-current">{t('Current password')}</label>
        <input id="pw-current" class="input" type="password" bind:value={current} autocomplete="current-password" required />
      </div>
      <div class="field">
        <label class="label" for="pw-new">{t('New password')}</label>
        <input id="pw-new" class="input" type="password" bind:value={next} autocomplete="new-password" minlength="10" required />
      </div>
      <div class="field">
        <label class="label" for="pw-confirm">{t('Confirm new password')}</label>
        <input id="pw-confirm" class="input" type="password" bind:value={confirm} autocomplete="new-password" required />
      </div>
      {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
    </form>
  {/snippet}
  {#snippet passwordFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">{t('Your files stay as they are; only the key protecting them is re-wrapped.')}</p>
    <button form="password-form" class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {t('Change password')}
    </button>
  {/snippet}
  {@render section(t('Password'), t('Changing it signs out your other devices. If you forget it, only your recovery key can get you back in.'), passwordBody, passwordFooter)}

  {#snippet recoveryBody()}
    {#if session.me.recovery_created_at}
      <p class="flex items-center gap-2 text-sm"><Icon name="shield-check" class="size-4 text-success" /><span><Sentence text={t('Set up on {date}')}>{#snippet date()}<Time ms={session.me.recovery_created_at * 1000} />{/snippet}</Sentence></span></p>
    {:else}
      <p class="flex items-start gap-2 text-sm text-fg-muted">
        <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
        {t("Not set up. If you forget your password, your files can't be recovered by anyone.")}
      </p>
    {/if}
  {/snippet}
  {#snippet recoveryFooter()}
    {#if session.me.recovery_created_at}
      <button type="button" class="btn btn-ghost" onclick={() => ((removePassword = ''), (recoveryDialog = 'remove'))}>{t('Remove')}</button>
    {/if}
    <button type="button" class="btn btn-secondary" onclick={() => (recoveryDialog = 'create')}>
      <Icon name="key-round" />{session.me.recovery_created_at ? t('Replace key') : t('Create recovery key')}
    </button>
  {/snippet}
  {@render section(
    t('Recovery key'),
    t('A printable key that lets you set a new password if you forget yours, without losing your files. Keep it somewhere safe, away from this device.'),
    recoveryBody,
    recoveryFooter,
  )}

  {#snippet twoStepBody()}
    <div class="grid gap-4">
      <div class="flex flex-wrap items-center gap-3">
        <Icon name="smartphone" class="size-4 shrink-0 text-fg-muted" />
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium">{t('Authenticator app')}</p>
          <p class="text-xs text-fg-muted">
            {#if session.me.totp_created_at}<Sentence text={t('On since {date}')}>{#snippet date()}<Time ms={session.me.totp_created_at * 1000} />{/snippet}</Sentence>{:else}{t('Six-digit codes from an app on your phone')}{/if}
          </p>
        </div>
        {#if session.me.totp_created_at}
          <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => openTwoStep('totp-off')}>{t('Turn off')}</button>
        {:else}
          <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => openTwoStep('totp')}>{t('Set up')}</button>
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
                  <span class="badge" title={p.unlock ? t('Signs you in without your password') : t('Its authenticator has no PRF support, so it only confirms a password sign-in')}>
                    {p.unlock ? t('Signs in on its own') : t('Second step only')}
                  </span>
                </p>
                <p class="truncate text-xs text-fg-muted">
                  <Sentence text={t('Added {date}')}>{#snippet date()}<span title={fullDate(p.created_at * 1000)}>{formatDate(p.created_at * 1000)}</span>{/snippet}</Sentence>
                  · {#if p.last_used_at}<Sentence text={t('last used {when}')}>{#snippet when()}<span title={fullDate(p.last_used_at * 1000)}>{formatWhen(p.last_used_at * 1000)}</span>{/snippet}</Sentence>{:else}{t('never used')}{/if}
                </p>
              </div>
              <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => openTwoStep({ passkey: p })}>{t('Remove')}</button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if twoStepOn && !session.me.recovery_created_at}
        <p class="flex items-start gap-2 text-[13px] text-fg-muted">
          <Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />
          {t("Create a recovery key too. If you lose your phone and passkeys, it's the only way back in.")}
        </p>
      {/if}
    </div>
  {/snippet}
  {#snippet twoStepFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">{t('App passwords and your recovery key skip this step.')}</p>
    {#if passkeysSupported()}
      <button type="button" class="btn btn-secondary" onclick={() => openTwoStep('passkey')}><Icon name="plus" />{t('Add a passkey')}</button>
    {/if}
  {/snippet}
  {@render section(
    t('Two-step sign-in'),
    t("With an authenticator app or a passkey set up, signing in with your password also asks for one of them, so a leaked password alone isn't enough."),
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
                {#if d.current}<span class="badge badge-accent">{t('This device')}</span>{/if}
                {#if d.app_password}<span class="badge"><Icon name="key-round" />{d.app_password}</span>{/if}
                {#if d.current && session.remembered}<span class="badge">{t('Kept signed in')}</span>{/if}
              </p>
              <p class="truncate text-xs text-fg-muted">
                <Sentence text={t('Signed in {date}')}>{#snippet date()}<span title={fullDate(d.created_at * 1000)}>{formatDate(d.created_at * 1000)}</span>{/snippet}</Sentence>
                {#if !d.current}· <Sentence text={t('active {when}')}>{#snippet when()}<span title={fullDate(d.last_seen * 1000)}>{formatWhen(d.last_seen * 1000)}</span>{/snippet}</Sentence>{/if}
              </p>
            </div>
            {#if d.current && session.remembered}
              <button
                type="button"
                class="btn btn-secondary h-7 px-2.5 text-[13px]"
                title={t("Remove the saved keys from this browser; you'll need your password next time")}
                onclick={async () => {
                  await forgetThisBrowser();
                  toast(t("This browser won't keep you signed in any more"), { kind: 'success' });
                }}>{t('Stop keeping signed in')}</button>
            {/if}
            {#if !d.current}
              <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" disabled={revoking !== null} onclick={() => signOut(d.id)}>
                {#if revoking === d.id}<Icon name="loader-circle" class="spinner" />{/if}
                {t('Sign out')}
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/snippet}
  {#snippet devicesFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">{t('Only the device name and times are kept, not IP addresses.')}</p>
    <button type="button" class="btn btn-secondary" disabled={revoking !== null || !devices || devices.length < 2} onclick={() => signOut('others')}>
      {#if revoking === 'others'}<Icon name="loader-circle" class="spinner" />{/if}
      {t('Sign out other devices')}
    </button>
  {/snippet}
  {@render section(
    t('Devices'),
    t('Where you are signed in. Signing a device out ends its session, and its keys are gone from memory the next time it tries to do anything.'),
    devicesBody,
    devicesFooter,
  )}

  {#snippet appBody()}
    {#if appPasswords === null}
      <div class="skeleton h-12 w-full" aria-hidden="true"></div>
    {:else if !appPasswords.length}
      <p class="text-[13px] text-fg-muted">{t('None yet.')}</p>
    {:else}
      <ul class="divide-y divide-line rounded-md border border-line">
        {#each appPasswords as a (a.id)}
          <li class="flex items-center gap-3 px-3 py-2.5" out:slide>
            <Icon name="key-round" class="size-4 shrink-0 text-fg-muted" />
            <div class="min-w-0 flex-1">
              <p class="flex items-center gap-2 text-sm">
                <span class="truncate font-medium">{a.name}</span>
                <span class="badge">{a.scope === 'read' ? t('Read only') : t('Full access')}</span>
              </p>
              <p class="truncate text-xs text-fg-muted">
                <Sentence text={t('Created {date}')}>{#snippet date()}<span title={fullDate(a.created_at * 1000)}>{formatDate(a.created_at * 1000)}</span>{/snippet}</Sentence>
                · {#if a.last_used_at}<Sentence text={t('last used {when}')}>{#snippet when()}<span title={fullDate(a.last_used_at * 1000)}>{formatWhen(a.last_used_at * 1000)}</span>{/snippet}</Sentence>{:else}{t('never used')}{/if}
              </p>
            </div>
            <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => (revokingApp = a)}>{t('Revoke')}</button>
          </li>
        {/each}
      </ul>
    {/if}
  {/snippet}
  {#snippet appFooter()}
    <p class="mr-auto hidden text-xs text-fg-muted sm:block">{t("Your password changing doesn't affect them.")}</p>
    <button type="button" class="btn btn-secondary" onclick={() => (appDialog = true)}><Icon name="plus" />{t('New app password')}</button>
  {/snippet}
  {@render section(
    t('App passwords'),
    t('Sign in sync clients and other devices without giving them your account password. Each one can be read only, and revoking it signs that device out.'),
    appBody,
    appFooter,
  )}

  {#snippet themeBody()}
    <div class="grid max-w-md grid-cols-3 gap-2" role="radiogroup" aria-label={t('Theme')}>
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
  {#snippet regionBody()}
    <dl class="grid gap-4 text-sm sm:grid-cols-[10rem_1fr] sm:items-center">
      <dt class="text-fg-muted"><label for="ui-language">{t('Language')}</label></dt>
      <dd>
        <select id="ui-language" class="input h-9 max-w-sm" value={format.language} onchange={(e) => changeFormat({ language: e.currentTarget.value })}>
          <option value="auto">{t('Automatic ({name})', { name: LANGUAGES.find(([c]) => c === language())?.[1] ?? 'English' })}</option>
          {#each LANGUAGES as [code, name] (code)}<option value={code}>{name}</option>{/each}
        </select>
      </dd>
      <dt class="text-fg-muted"><label for="region-format">{t('Dates and numbers')}</label></dt>
      <dd>
        <select id="region-format" class="input h-9 max-w-sm" value={format.region} onchange={(e) => changeFormat({ region: e.currentTarget.value })}>
          <option value="auto">{t('Automatic ({name})', { name: browserRegion })}</option>
          {#each REGIONS as [tag, label] (tag)}<option value={tag}>{label}</option>{/each}
        </select>
      </dd>
      {#snippet choice(key, options, label)}
        <div class="flex w-fit rounded-md border border-line p-0.5" role="radiogroup" aria-label={label}>
          {#each options as [value, text] (value)}
            <button type="button" role="radio" aria-checked={format[key] === value} class="h-7 cursor-pointer rounded px-3 text-[13px] {format[key] === value ? 'bg-muted font-medium text-fg' : 'text-fg-muted hover:text-fg'}" onclick={() => changeFormat({ [key]: value })}>{text}</button>
          {/each}
        </div>
      {/snippet}
      <dt class="text-fg-muted">{t('Time')}</dt>
      <dd>{@render choice('clock', [['auto', t('Automatic')], ['12', t('12-hour')], ['24', t('24-hour')]], t('Time format'))}</dd>
      <dt class="text-fg-muted">{t('Units')}</dt>
      <dd>{@render choice('units', [['auto', t('Automatic')], ['metric', t('Metric')], ['imperial', t('Imperial')]], t('Units'))}</dd>
      <dt class="text-fg-muted">{t('Looks like')}</dt>
      <dd class="text-fg-muted tabular-nums">{example}</dd>
    </dl>
  {/snippet}
  {@render section(t('Language and region'), t('The language of thencloud, and how dates, times, numbers and measurements are shown. Saved with your account, so your other devices use it too.'), regionBody)}

  {@render section(t('Appearance'), null, themeBody)}

  {#snippet accentBody()}
    <div class="grid gap-4">
      <div class="flex flex-wrap items-center gap-2" role="radiogroup" aria-label={t('Accent colour')}>
        {#each ACCENT_PRESETS as [hex, name] (hex)}
          <button
            type="button"
            role="radio"
            aria-checked={accent.value === hex}
            aria-label={accentNames[name] ?? name}
            title={accentNames[name] ?? name}
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
          title={t('Custom colour')}
          style:background-color={isPreset ? null : accent.value}>
          <Icon name="plus" class="size-4 {isPreset ? '' : 'opacity-0'}" />
          <input type="color" class="absolute inset-0 cursor-pointer opacity-0" value={accent.value} aria-label={t('Custom accent colour')} oninput={(e) => setAccent(e.currentTarget.value)} />
        </label>
      </div>
      <div class="flex flex-wrap items-center gap-3 rounded-md border border-line bg-subtle p-4">
        <button type="button" class="btn btn-accent" tabindex="-1">{t('Primary action')}</button>
        <span class="badge badge-accent">{t('Can edit')}</span>
        <span class="link text-sm">{t('A link')}</span>
        <span class="flex items-center gap-1.5 text-sm"><Icon name="folder" class="size-4 text-accent-text" />{t('Folder')}</span>
      </div>
      <label class="flex cursor-pointer items-start gap-3 text-sm">
        <input type="checkbox" class="mt-0.5 size-4 accent-accent" checked={tint.on} onchange={(e) => setTint(e.currentTarget.checked)} />
        <span class="grid gap-0.5">
          <span>{t('Tint the whole theme')}</span>
          <span class="text-xs text-fg-muted">{t("Backgrounds, borders and grey text take on a hint of the accent's hue.")}</span>
        </span>
      </label>
      {#if lowContrast}
        <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="circle-alert" class="size-4" />{t('This colour is hard to read on one of the themes; text uses an adjusted shade.')}</p>
      {/if}
    </div>
  {/snippet}
  {#snippet accentFooter()}
    <p class="mr-auto text-xs text-fg-muted">{t('Saved on this device only.')}</p>
    <button type="button" class="btn btn-secondary" disabled={accent.value === DEFAULT_ACCENT} onclick={() => setAccent(DEFAULT_ACCENT)}>{t('Reset to blue')}</button>
  {/snippet}
  {@render section(t('Accent colour'), t('Used for buttons, links, folders and highlights, and optionally to tint everything else.'), accentBody, accentFooter)}

  {#snippet iconsBody()}
    <div class="grid gap-2 sm:grid-cols-2" role="radiogroup" aria-label={t('File icons')}>
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
            <span class="text-sm font-medium {iconPack.value === pack.id ? 'text-accent-text' : ''}">{packNames[pack.id] ?? pack.name}</span>
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
      <span>{t('Folder icons by name')}</span>
      <span class="flex gap-2" aria-hidden="true">
        {#each ['src', 'images', 'docs', 'music'] as name (name)}<FolderIcon {name} named={hasFolderIcons(iconPack.value)} class="size-5" />{/each}
      </span>
    </label>
    {#if !hasFolderIcons(iconPack.value)}<p class="-mt-2 text-xs text-fg-muted">{t('Material and Symbols have icons for common folder names.')}</p>{/if}
  {/snippet}
  {@render section(t('File icons'), t('Icons for files by type, and optionally for folders by name.'), iconsBody)}

  {#snippet photosBody()}
    <div class="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label={t('Location and camera details in photos')}>
      {#each [['ask', t('Ask me'), t('When a photo records where it was taken.')], ['remove', t('Always remove'), t('From every photo, before it is uploaded.')], ['keep', t('Keep them'), t('Upload photos exactly as they are.')]] as [value, label, text] (value)}
        <button
          type="button"
          role="radio"
          aria-checked={photoDetails.value === value}
          class="grid cursor-pointer gap-1 rounded-md border p-3 text-left transition-colors {photoDetails.value === value ? 'border-accent bg-accent-soft' : 'border-line hover:bg-subtle'}"
          onclick={() => setPhotoDetails(value)}>
          <span class="text-sm font-medium {photoDetails.value === value ? 'text-accent-text' : ''}">{label}</span>
          <span class="text-xs text-fg-muted">{text}</span>
        </button>
      {/each}
    </div>
  {/snippet}
  {@render section(t('Location in photos'), t('JPEG, PNG and WebP photos often record where they were taken and on what camera. This is checked on this device when you upload; the orientation is always kept.'), photosBody)}


  {#snippet exportBody()}
    <p class="text-[13px] text-fg-muted">
      <Sentence text={t("Everything in My files in one zip, with your playlists, pinned notes and verified contacts as JSON in a {folder} folder. The zip is not encrypted, so keep it somewhere safe. Items others shared with you aren't included.")}>
        {#snippet folder()}<code class="font-mono text-fg">thencloud-data</code>{/snippet}
      </Sentence>
    </p>
    {#if exporting}
      <div class="progress"><div style:width="{Math.round(exporting.progress * 100)}%"></div></div>
    {/if}
  {/snippet}
  {#snippet exportFooter()}
    <button type="button" class="btn btn-secondary" onclick={exportData} disabled={!!exporting}>
      {#if exporting}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="download" />{/if} {t('Export')}
    </button>
  {/snippet}
  {#snippet modulesBody()}
    <ul class="grid gap-2">
      {#each MODULES as m (m.name)}
        <li>
          <label class="flex cursor-pointer items-start gap-3 rounded-md border border-line p-3 hover:bg-subtle">
            <input type="checkbox" class="mt-0.5 size-4 accent-accent" checked={modules[m.name]} disabled={!modules.loaded} onchange={(e) => toggleModule(m, e.currentTarget.checked)} />
            <span class="grid gap-0.5">
              <span class="flex items-center gap-1.5 text-sm font-medium"><Icon name={m.icon} class="size-4" />{m.label}</span>
              <span class="text-xs text-fg-muted">{m.description}</span>
            </span>
          </label>
        </li>
      {/each}
    </ul>
    <p class="text-xs text-fg-muted">{t('Turning one off only hides it; what you logged stays until you turn it on again.')}</p>
  {/snippet}
  {@render section(t('Modules'), t('Optional parts of thencloud. Which ones are on is saved with your account, so it follows you to other devices.'), modulesBody)}

  {@render section(t('Export your data'), null, exportBody, exportFooter)}

  {#snippet checkBody()}
    <p class="text-[13px] text-fg-muted">
      {t('Downloads everything in My files and decrypts it here, to make sure nothing is damaged or missing. Nothing is saved. With a lot of data this takes a while.')}
    </p>
    {#if checkRun}
      {@const pct = Math.min(100, (checkRun.bytes / Math.max(1, session.me.used_bytes)) * 100)}
      <div class="grid gap-2">
        {#if !checkRun.done}
          <div class="progress"><div style:width="{pct}%"></div></div>
        {/if}
        <p class="text-[13px] {checkRun.done && !checkRun.problems.length && !checkRun.stopped ? 'text-success' : 'text-fg-muted'}" role="status">
          {#if !checkRun.done}
            {t('Checked {files} in {folders}, {size}', { files: t('{count} files', { count: checkRun.files }), folders: t('{count} folders', { count: checkRun.folders + 1 }), size: formatSize(checkRun.bytes) })}
          {:else if checkRun.stopped}
            {t('Stopped after {count} files.', { count: checkRun.files })}
          {:else if !checkRun.problems.length}
            {t('Everything decrypts correctly: {files} and {folders}.', { files: t('{count} files', { count: checkRun.files }), folders: t('{count} folders', { count: checkRun.folders }) })}
          {:else}
            {t("{count} items can't be read.", { count: checkRun.problems.length })} {t('The other {count} files are fine.', { count: checkRun.files })}
          {/if}
        </p>
        {#if checkRun.problems.length}
          <ul class="divide-y divide-line rounded-md border border-danger/40 text-[13px]">
            {#each checkRun.problems as p, i (i)}
              <li class="grid gap-0.5 px-3 py-2">
                <span class="truncate font-medium">{[...p.location, p.name ?? t("An item that can't be named")].join(' / ')}</span>
                <span class="text-fg-muted">{p.error}</span>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  {/snippet}
  {#snippet checkFooter()}
    {#if checkRun && !checkRun.done}
      <button type="button" class="btn btn-secondary" onclick={() => checkCtl?.abort()}>{t('Stop')}</button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={checkFiles}><Icon name="shield-check" /> {t('Check files')}</button>
    {/if}
  {/snippet}
  {@render section(t('Check your files'), null, checkBody, checkFooter)}

  {#snippet deleteBody()}
    <p class="text-[13px] text-fg-muted">
      {t("Your files, folders, versions, links and shares are deleted from the server straight away. There is no undo, and an admin can't bring them back. Files you added to folders other people shared with you belong to them and stay.")}
    </p>
  {/snippet}
  {#snippet deleteFooter()}
    <button type="button" class="btn btn-danger" onclick={() => ((deleting = true), (deletePassword = ''), (deleteConfirm = ''))}>
      <Icon name="trash-2" /> {t('Delete account')}
    </button>
  {/snippet}
  {@render section(t('Delete account'), null, deleteBody, deleteFooter)}

  {#snippet aboutBody()}
    <div class="grid gap-2 text-[13px] text-fg-muted">
      <p>
        {t('thencloud is free software under the AGPL-3.0. Your files are encrypted in this browser with keys derived from your password; the server stores ciphertext and can see only sizes, dates and who shares with whom.')}
      </p>
      <p>
        {#if inApp}
          {t('This app carries its own copy of the web app, so the server sends it only data, never code.')}
        {:else}
          <Sentence text={t("This page is code the server sends you. Each release is built reproducibly with a signed list of every file's hash, and {command} checks that a server sends exactly that.")}>
            {#snippet command()}<code class="font-mono text-fg">thencloud verify-web</code>{/snippet}
          </Sentence>
        {/if}
      </p>
      <p>
        <Sentence text={t('This is version {version} of the web app.')}>
          {#snippet version()}<code class="font-mono text-fg">{appVersion}</code>{/snippet}
        </Sentence>
        {#if !inApp}
          {watched
            ? t("This browser remembers the app it was sent and tells you before opening a changed one; that isn't proof against a server set on changing it, so check it too.")
            : t("This browser isn't watching the app for changes right now (it needs a service worker, which some private windows don't allow).")}
        {/if}
      </p>
      {#if appVersion !== 'dev' && !inApp}
        <div class="flex items-center gap-2">
          <code class="min-w-0 flex-1 truncate rounded-md border border-line bg-subtle px-2.5 py-1.5 font-mono text-xs text-fg">{verifyCommand}</code>
          <button type="button" class="btn btn-secondary btn-sm" onclick={() => copyText(verifyCommand)}><Icon name="copy" />{t('Copy')}</button>
        </div>
      {/if}
    </div>
  {/snippet}
  {@render section(t('About'), null, aboutBody)}
</div>

{#if deleting}
  <ConfirmDialog
    title={t('Delete your account?')}
    description={t('The account {name} and everything in it are deleted for good. Download anything you want to keep first.', { name: session.me.username })}
    confirmLabel={t('Delete account')}
    danger
    disabled={!deletePassword || deleteConfirm.trim().toLowerCase() !== session.me.username}
    onconfirm={async () => {
      try {
        await deleteAccount(deletePassword);
      } catch (e) {
        throw wrongPassword(e);
      }
    }}
    onclose={() => (deleting = false)}>
    <div class="field">
      <label class="label" for="delete-password">{t('Your password')}</label>
      <input id="delete-password" class="input" type="password" bind:value={deletePassword} autocomplete="current-password" />
    </div>
    <div class="field">
      <label class="label" for="delete-confirm"><Sentence text={t('Type {name} to confirm')}>{#snippet name()}<span class="font-mono">{session.me.username}</span>{/snippet}</Sentence></label>
      <input id="delete-confirm" class="input" bind:value={deleteConfirm} autocomplete="off" autocapitalize="none" spellcheck="false" />
    </div>
  </ConfirmDialog>
{/if}

{#if appDialog}
  <AppPasswordDialog onclose={() => (appDialog = false)} oncreated={loadAppPasswords} />
{/if}

{#if revokingApp}
  <ConfirmDialog
    title={t('Revoke {name}?', { name: revokingApp.name })}
    description={t('It stops working straight away, and anything signed in with it is signed out.')}
    confirmLabel={t('Revoke')}
    danger
    onconfirm={async () => {
      await deleteAppPassword(revokingApp.id);
      appPasswords = appPasswords.filter((x) => x.id !== revokingApp.id);
      loadDevices();
      toast(t('App password revoked'));
    }}
    onclose={() => (revokingApp = null)} />
{/if}

{#if twoStepDialog === 'totp'}
  <TotpDialog onclose={() => (twoStepDialog = null)} ondone={() => toast(t('Authenticator app turned on'), { kind: 'success' })} />
{:else if twoStepDialog === 'passkey'}
  <PasskeyDialog
    onclose={() => (twoStepDialog = null)}
    onadded={(p) => {
      passkeys = [...(passkeys ?? []), p];
      toast(p.unlock ? t('Passkey added. It can sign you in on its own.') : t('Passkey added as a second step.'), { kind: 'success' });
    }} />
{:else if twoStepDialog}
  {@const passkey = twoStepDialog.passkey}
  <ConfirmDialog
    title={passkey ? t('Remove {name}?', { name: passkey.name }) : t('Turn off the authenticator app?')}
    description={passkey ? t('It stops working for this account straight away.') : t('Signing in will no longer ask for its codes.')}
    confirmLabel={passkey ? t('Remove') : t('Turn off')}
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
      toast(passkey ? t('Passkey removed') : t('Authenticator app turned off'));
    }}
    onclose={() => (twoStepDialog = null)}>
    <div class="field">
      <label class="label" for="two-step-password">{t('Your password')}</label>
      <input id="two-step-password" class="input" type="password" bind:value={twoStepPassword} autocomplete="current-password" />
    </div>
  </ConfirmDialog>
{/if}

{#if recoveryDialog === 'create'}
  <RecoveryKeyDialog onclose={() => (recoveryDialog = null)} />
{:else if recoveryDialog === 'remove'}
  <ConfirmDialog
    title={t('Remove your recovery key?')}
    description={t("The key stops working straight away. If you then forget your password, your files can't be recovered.")}
    confirmLabel={t('Remove key')}
    danger
    disabled={!removePassword}
    onconfirm={async () => {
      try {
        await removeRecoveryKey(removePassword);
      } catch (e) {
        throw e?.code === 'invalid_credentials' ? new Error(t('That password is wrong.')) : e;
      }
      toast(t('Recovery key removed'));
    }}
    onclose={() => (recoveryDialog = null)}>
    <div class="field">
      <label class="label" for="rm-password">{t('Your password')}</label>
      <input id="rm-password" class="input" type="password" bind:value={removePassword} autocomplete="current-password" />
    </div>
  </ConfirmDialog>
{/if}
