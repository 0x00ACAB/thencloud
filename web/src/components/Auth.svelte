<script>
  import { onMount } from 'svelte';
  import { login, loginWithPasskey, register, authOptions, recoverAccount, changeServer } from '../lib/cloud.svelte.js';
  import { inApp, serverOrigin, parseServer } from '../lib/server.svelte.js';
  import { passkeysSupported, cancelled } from '../lib/passkeys.js';
  import { errorMessage } from '../lib/ui.svelte.js';
  import Icon from './Icon.svelte';
  import { t, language, LANGUAGES } from '../lib/i18n.svelte.js';
  import { format, setFormat } from '../lib/locale.svelte.js';
  import { hasToken, takeToken, goToCheck, returned } from '../lib/turnstile.js';

  let mode = $state('signin'); // signin | signup | recover
  let recoveryKey = $state('');
  let remember = $state(false);
  let username = $state('');
  let password = $state('');
  let confirm = $state('');
  let showPassword = $state(false);
  let busy = $state(false);
  let error = $state('');
  // A password sign-in waiting for its second step (see `login`).
  let pending = $state(null);
  let code = $state('');
  const canPasskey = passkeysSupported();
  // Set by cloud.svelte.js when a session ends from elsewhere.
  let endedElsewhere = $state(false);
  try {
    endedElsewhere = sessionStorage.getItem('signedOut') === '1';
    sessionStorage.removeItem('signedOut');
  } catch {
    /* private mode */
  }

  // An invite link is /#invite=<token>. Read it once, then clear it from
  // the address bar so it isn't left in history.
  let invite = $state(null);
  function readInvite() {
    const m = location.hash.match(/^#invite=([\w-]+)$/);
    if (!m) return;
    history.replaceState(null, '', location.pathname);
    invite = m[1];
    switchMode('signup');
  }

  let registration = $state('open'); // open | invite | closed
  // Set when this server asks for a Turnstile check (see lib/turnstile.js).
  let turnstile = $state(null);
  let checked = $state(hasToken());
  // A new server: the first account needs the setup code from its log.
  let setup = $state(false);
  let setupCode = $state('');
  let adminName = $state(null);
  function applyOptions(o) {
    registration = o.registration;
    turnstile = o.turnstile ?? null;
    setup = !!o.setup;
    adminName = o.admin_username ?? null;
    if (setup) {
      switchMode('signup');
      if (adminName && !username) username = adminName;
    }
  }
  const loadOptions = () =>
    authOptions()
      .then(applyOptions)
      .catch(() => {});
  onMount(() => {
    // Back from /auth: the same tab, and the invite if there was one.
    const back = returned();
    if (back?.invite) invite = back.invite;
    if (back?.mode === 'signup' || back?.mode === 'signin') switchMode(back.mode);
    readInvite();
    if (serverOrigin()) loadOptions();
    // Also when the link is pasted into a tab that's already here.
    addEventListener('hashchange', readInvite);
    return () => removeEventListener('hashchange', readInvite);
  });
  // The apps ask which server to use first.
  let editingServer = $state(inApp && !serverOrigin());
  let serverInput = $state(serverOrigin() ?? '');
  const serverHost = $derived(serverOrigin() && new URL(serverOrigin()).host);

  function connect(e) {
    e.preventDefault();
    const origin = parseServer(serverInput);
    if (!origin) {
      error = t('Use an https:// address.');
      return;
    }
    run(async () => {
      let o = null;
      try {
        const res = await fetch(`${origin}/api/auth/options`, { cache: 'no-store', referrerPolicy: 'no-referrer' });
        o = res.ok ? await res.json() : null;
      } catch {
        /* unreachable, or not letting the app in */
      }
      if (!o?.registration) throw new Error(t("Couldn't find a thencloud server there."));
      await changeServer(origin);
      applyOptions(o);
      editingServer = false;
    });
  }

  const canSignUp = $derived(registration === 'open' || (registration === 'invite' && !!invite));

  const signup = $derived(mode === 'signup');
  const recover = $derived(mode === 'recover');
  // Whether this form needs the check first. Recovery keys and passkeys don't.
  const needsCheck = $derived(!!turnstile && (signup ? turnstile.register : !recover && turnstile.login));
  const checkFirst = $derived(needsCheck && !checked);
  // Both creating an account and resetting a password choose a new one.
  const choosing = $derived(signup || recover);

  function switchMode(m) {
    mode = m;
    pending = null;
    code = '';
    error = '';
    password = '';
    confirm = '';
  }

  function explain(err, overrides) {
    if (err?.code === 'sign_in_expired') pending = null;
    if (cancelled(err)) return '';
    return (
      {
        invalid_credentials: recover ? t("That recovery key doesn't match this account.") : t('Wrong username or password.'),
        invalid_second_factor: t("That didn't work. Codes change every 30 seconds; try the newest one."),
        sign_in_expired: t('That took too long. Sign in again.'),
        account_disabled: t('This account has been disabled. Ask the person who runs this server.'),
        invalid_invite: t('This invite link has already been used or has expired. Ask for a new one.'),
        registration_closed: t('New accounts are not being accepted on this server right now.'),
        turnstile_failed: t("The check that you're a person expired or didn't go through. Do it again, then try once more."),
        setup_required: adminName
          ? t("That setup code isn't right, or the name isn't {name}.", { name: adminName })
          : t("That setup code isn't right. It's in the server's log from when it started."),
        ...overrides,
      }[err?.code] ?? errorMessage(err)
    );
  }

  async function run(task, overrides = {}) {
    error = '';
    busy = true;
    try {
      await task();
    } catch (err) {
      error = explain(err, overrides);
    } finally {
      busy = false;
    }
  }

  function submit(e) {
    e.preventDefault();
    if (choosing && password !== confirm) {
      error = t("The passwords don't match.");
      return;
    }
    // A check token is good once: whatever happens next, it's used up.
    const token = needsCheck ? takeToken() : null;
    if (needsCheck) checked = false;
    if (needsCheck && !token) {
      error = t("The check that you're a person expired or didn't go through. Do it again, then try once more.");
      return;
    }
    run(async () => {
      if (signup) await register(username, password, invite, remember, token, setup ? setupCode : null);
      else if (recover) await recoverAccount(username.trim(), recoveryKey, password, remember);
      else pending = await login(username, password, remember, token);
    });
  }

  function submitCode(e) {
    e.preventDefault();
    run(() => pending.withCode(code));
  }
</script>

<main class="flex min-h-dvh flex-col items-center px-4 pt-[max(12vh,var(--safe-top))] pb-[calc(2rem+var(--safe-bottom))]">
  <img src="/img/logo.webp" alt="thencloud" width="715" height="349" class="h-auto w-56 select-none" draggable="false" />

  <div class="mt-8 w-full max-w-sm">
    {#if editingServer}
      <form class="grid gap-4" onsubmit={connect}>
        <div class="field">
          <label class="label" for="server">{t('Server address')}</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input id="server" class="input" bind:value={serverInput} placeholder="cloud.example.com" inputmode="url" autocapitalize="none" autocomplete="url" spellcheck="false" autofocus required />
          <p class="hint">{t('The address you open thencloud at in a browser.')}</p>
        </div>
        {#if error}
          <p class="flex items-center gap-2 text-[13px] text-danger" role="alert"><Icon name="circle-alert" />{error}</p>
        {/if}
        <button class="btn btn-primary btn-lg w-full" disabled={busy}>
          {#if busy}<Icon name="loader-circle" class="spinner" />{/if}{t('Connect')}
        </button>
        {#if serverOrigin()}
          <button type="button" class="btn btn-secondary btn-lg -mt-1 w-full" disabled={busy} onclick={() => ((editingServer = false), (error = ''))}>{t('Cancel')}</button>
        {/if}
      </form>
    {:else if pending}
      <div class="grid gap-1">
        <button type="button" class="flex w-fit cursor-pointer items-center gap-1 text-[13px] text-fg-muted hover:text-fg" onclick={() => switchMode('signin')}>
          <Icon name="arrow-left" class="size-3.5" /> {t('Back')}
        </button>
        <h1 class="mt-2 text-base font-semibold tracking-tight">{t("Confirm it's you")}</h1>
        <p class="text-[13px] text-fg-muted">
          {pending.totp && pending.passkey
            ? t('Your password was right. This account also asks for a passkey or a code from your authenticator app.')
            : pending.passkey
              ? t('Your password was right. This account also asks for a passkey.')
              : t('Your password was right. This account also asks for a code from your authenticator app.')}
        </p>
      </div>
      <div class="mt-6 grid gap-4">
        {#if pending.passkey && !canPasskey && !pending.totp}
          <p class="flex items-start gap-2 text-[13px] text-fg-muted"><Icon name="circle-alert" class="mt-0.5 size-4 shrink-0" />{t("The app can't use passkeys yet. Sign in from a browser, or use your recovery key.")}</p>
        {:else if pending.passkey && canPasskey}
          <button type="button" class="btn {pending.totp ? 'btn-secondary' : 'btn-primary'} btn-lg w-full" disabled={busy} onclick={() => run(pending.withPasskey)}>
            <Icon name="fingerprint" />{t('Use a passkey')}
          </button>
        {/if}
        {#if pending.totp}
          <form class="grid gap-4" onsubmit={submitCode}>
            <div class="field">
              <label class="label" for="totp-code">{t('Code from your authenticator app')}</label>
              <!-- svelte-ignore a11y_autofocus -->
              <input
                id="totp-code"
                class="input font-mono tracking-widest"
                bind:value={code}
                inputmode="numeric"
                autocomplete="one-time-code"
                pattern="[0-9 ]*"
                maxlength="7"
                placeholder="123456"
                autofocus
                required />
            </div>
            <button class="btn btn-primary btn-lg w-full" disabled={busy || code.replace(/\s/g, '').length !== 6}>
              {#if busy}<Icon name="loader-circle" class="spinner" />{t('Unlocking your files')}{:else}{t('Continue')}{/if}
            </button>
          </form>
        {/if}
        {#if error}
          <p class="flex items-center gap-2 text-[13px] text-danger" role="alert"><Icon name="circle-alert" />{error}</p>
        {/if}
        <p class="text-xs leading-5 text-fg-muted">{t('Lost them? A recovery key still resets your password without the second step.')}</p>
      </div>
    {:else}
    {#if inApp}
      <p class="mb-4 flex items-center justify-between gap-3 text-[13px] text-fg-muted">
        <span class="flex min-w-0 items-center gap-2"><Icon name="hard-drive" class="size-4 shrink-0" /><span class="truncate font-mono">{serverHost}</span></span>
        <button type="button" class="shrink-0 cursor-pointer text-xs hover:text-fg" onclick={() => ((editingServer = true), (error = ''))}>{t('Change server')}</button>
      </p>
    {/if}
    {#if recover}
      <div class="grid gap-1">
        <button type="button" class="flex w-fit cursor-pointer items-center gap-1 text-[13px] text-fg-muted hover:text-fg" onclick={() => switchMode('signin')}>
          <Icon name="arrow-left" class="size-3.5" /> {t('Back to sign in')}
        </button>
        <h1 class="mt-2 text-base font-semibold tracking-tight">{t('Reset your password')}</h1>
        <p class="text-[13px] text-fg-muted">
          {t("Use the recovery key you saved. Your files stay as they are; you'll be signed out on your other devices.")}
        </p>
      </div>
    {:else}
      <div class="grid grid-cols-2 rounded-lg border border-line bg-subtle p-1 text-sm" role="tablist">
        {#each [['signin', t('Sign in')], ['signup', t('Create account')]] as [m, label] (m)}
          <button
            type="button"
            role="tab"
            aria-selected={mode === m}
            class="h-8 cursor-pointer rounded-md font-medium transition-colors {mode === m
              ? 'bg-bg text-fg shadow-sm shadow-black/5 ring-1 ring-line'
              : 'text-fg-muted hover:text-fg'}"
            onclick={() => switchMode(m)}>{label}</button>
        {/each}
      </div>
    {/if}

    {#if signup && invite}
      <p class="mt-4 flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="user-round-plus" class="size-4" />{t("You've been invited to create an account here.")}</p>
    {:else if signup && !canSignUp}
      <p class="mt-4 flex items-start gap-2 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
        <Icon name="lock" class="mt-0.5 size-4 shrink-0" />
        {registration === 'invite' ? t('This server is invite-only. Open the invite link you were sent to create an account.') : t('This server is not accepting new accounts.')}
      </p>
    {/if}

    {#if checkFirst && inApp}
      <p class="mt-6 flex items-start gap-2 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
        <Icon name="shield-check" class="mt-0.5 size-4 shrink-0" />
        {t("This server asks for a check that you're a person, which the app can't show yet. Sign in from a browser, or use your recovery key.")}
      </p>
    {:else if checkFirst}
      <div class="mt-6 grid gap-4">
        <p class="flex items-start gap-2 text-[13px] leading-5 text-fg-muted">
          <Icon name="shield-check" class="mt-0.5 size-4 shrink-0" />
          {t("Before you continue, this server asks for a quick check that you're a person. It's run by Cloudflare on a page of its own, which never sees your password.")}
        </p>
        {#if error}
          <p class="flex items-center gap-2 text-[13px] text-danger" role="alert"><Icon name="circle-alert" />{error}</p>
        {/if}
        <button class="btn btn-primary btn-lg w-full" onclick={() => goToCheck({ mode, invite })} disabled={signup && !canSignUp}>{t('Continue')}</button>
        {#if mode === 'signin' && canPasskey}
          <button type="button" class="btn btn-secondary btn-lg -mt-1 w-full" disabled={busy} onclick={() => run(() => loginWithPasskey(remember), { invalid_credentials: t("That passkey isn't set up on this account.") })}>
            <Icon name="fingerprint" />{t('Sign in with a passkey')}
          </button>
          <button type="button" class="w-fit cursor-pointer text-xs text-fg-muted hover:text-fg" onclick={() => switchMode('recover')}>{t('Forgot your password?')}</button>
        {/if}
      </div>
    {:else}
    <form class="mt-6 grid gap-4" onsubmit={submit}>
      {#if signup && setup}
        <div class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
          <Icon name="key-round" class="mt-0.5 size-4 shrink-0 text-fg" />
          <p>
            <span class="font-medium text-fg">{t('This server is new.')}</span>
            {adminName
              ? t('The first account is its admin, and is called {name}. To make it, enter the setup code the server printed in its log when it started (it is also in the file setup-code in its data directory).', { name: adminName })
              : t('The first account is its admin. To make it, enter the setup code the server printed in its log when it started (it is also in the file setup-code in its data directory).')}
          </p>
        </div>
        <div class="field">
          <label class="label" for="setup-code">{t('Setup code')}</label>
          <input id="setup-code" class="input font-mono" bind:value={setupCode} autocomplete="off" autocapitalize="characters" spellcheck="false" placeholder="XXXXX-XXXXX-XXXXX-XXXXX-XXXXX" required />
        </div>
      {/if}
      <div class="field">
        <label class="label" for="username">{t('Username')}</label>
        <input id="username" class="input" bind:value={username} autocomplete="username" autocapitalize="none" spellcheck="false" required />
      </div>
      {#if recover}
        <div class="field">
          <label class="label" for="recovery-key">{t('Recovery key')}</label>
          <textarea
            id="recovery-key"
            class="input h-auto min-h-18 resize-none py-2 font-mono text-[13px] leading-5"
            bind:value={recoveryKey}
            autocomplete="off"
            autocapitalize="characters"
            spellcheck="false"
            placeholder="XXXXX-XXXXX-XXXXX-..."
            required></textarea>
        </div>
      {/if}
      <div class="field">
        <div class="flex items-baseline justify-between">
          <label class="label" for="password">{recover ? t('New password') : t('Password')}</label>
          {#if mode === 'signin'}
            <button type="button" class="cursor-pointer text-xs text-fg-muted hover:text-fg" onclick={() => switchMode('recover')}>{t('Forgot your password?')}</button>
          {/if}
        </div>
        <div class="relative">
          <input
            id="password"
            class="input pr-9"
            type={showPassword ? 'text' : 'password'}
            bind:value={password}
            autocomplete={choosing ? 'new-password' : 'current-password'}
            minlength={choosing ? 10 : undefined}
            required />
          <button
            type="button"
            class="absolute inset-y-0 right-0 grid w-9 cursor-pointer place-items-center text-fg-faint hover:text-fg"
            aria-label={showPassword ? t('Hide password') : t('Show password')}
            onclick={() => (showPassword = !showPassword)}>
            <Icon name={showPassword ? 'eye-off' : 'eye'} />
          </button>
        </div>
        {#if choosing}<p class="hint">{t('At least 10 characters. A few random words works well.')}</p>{/if}
      </div>
      {#if choosing}
        <div class="field">
          <label class="label" for="confirm">{t('Confirm password')}</label>
          <input id="confirm" class="input" type={showPassword ? 'text' : 'password'} bind:value={confirm} autocomplete="new-password" required />
        </div>
      {/if}
      {#if signup}
        <div class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
          <Icon name="key-round" class="mt-0.5 size-4 shrink-0 text-fg" />
          <p>
            <span class="font-medium text-fg">{t('There is no password reset.')}</span>
            {t('Your password is the only way to decrypt your files, and it never leaves this device. If you forget it, nobody can recover your data, unless you create a recovery key in Settings.')}
          </p>
        </div>
      {/if}

      <div class="grid gap-1.5">
        <label class="flex w-fit cursor-pointer items-center gap-2 text-[13px]">
          <input type="checkbox" class="size-4 accent-accent" bind:checked={remember} />
          {t('Keep me signed in on this browser')}
        </label>
        {#if remember}
          <p class="text-xs leading-5 text-fg-muted">
            {t("Your keys are saved in this browser, locked with a key it won't give to any website, so you won't need your password again here. Anyone who can use this computer account can open your files, so only tick this on your own device, ideally with an encrypted disk. Signing out removes them.")}
          </p>
        {/if}
      </div>

      {#if endedElsewhere && !error}
        <p class="flex items-center gap-2 text-[13px] text-fg-muted" role="status"><Icon name="log-out" />{t('You were signed out, maybe from another device. Sign in again to continue.')}</p>
      {/if}
      {#if error}
        <p class="flex items-center gap-2 text-[13px] text-danger" role="alert"><Icon name="circle-alert" />{error}</p>
      {/if}

      <button class="btn btn-primary btn-lg w-full" disabled={busy || (signup && !canSignUp)}>
        {#if busy}
          <Icon name="loader-circle" class="spinner" />
          {signup ? t('Generating your keys') : recover ? t('Setting your new password') : t('Unlocking your files')}
        {:else}
          {signup ? t('Create account') : recover ? t('Set new password') : t('Sign in')}
        {/if}
      </button>
      {#if mode === 'signin' && canPasskey}
        <button type="button" class="btn btn-secondary btn-lg -mt-1 w-full" disabled={busy} onclick={() => run(() => loginWithPasskey(remember), { invalid_credentials: t("That passkey isn't set up on this account.") })}>
          <Icon name="fingerprint" />{t('Sign in with a passkey')}
        </button>
      {/if}
    </form>
    {/if}
    {/if}

    <p class="mt-8 flex items-start gap-2 text-[13px] leading-5 text-fg-muted">
      <Icon name="shield-check" class="mt-0.5 size-4 shrink-0" />
      {t('Files are encrypted in your browser before upload. The server stores only ciphertext and never sees your password, your keys or your file names.')}
    </p>
  </div>

  <footer class="mt-auto flex items-center gap-3 pt-12 text-xs text-fg-faint">
    <span>{t('Open source')} · AGPL-3.0</span>
    <label class="flex items-center gap-1.5">
      <Icon name="globe" class="size-3.5" />
      <span class="sr-only">{t('Language')}</span>
      <select class="cursor-pointer bg-transparent text-fg-muted hover:text-fg" value={format.language === 'auto' ? language() : format.language} onchange={(e) => setFormat({ language: e.currentTarget.value })}>
        {#each LANGUAGES as [code, name] (code)}<option value={code}>{name}</option>{/each}
      </select>
    </label>
  </footer>
</main>
