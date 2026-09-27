<script>
  import { onMount } from 'svelte';
  import { login, register, authOptions, recoverAccount } from '../lib/cloud.svelte.js';
  import { errorMessage } from '../lib/ui.svelte.js';
  import Icon from './Icon.svelte';

  let mode = $state('signin'); // signin | signup | recover
  let recoveryKey = $state('');
  let username = $state('');
  let password = $state('');
  let confirm = $state('');
  let showPassword = $state(false);
  let busy = $state(false);
  let error = $state('');
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
  onMount(() => {
    readInvite();
    authOptions()
      .then((o) => (registration = o.registration))
      .catch(() => {});
    // Also when the link is pasted into a tab that's already here.
    addEventListener('hashchange', readInvite);
    return () => removeEventListener('hashchange', readInvite);
  });
  const canSignUp = $derived(registration === 'open' || (registration === 'invite' && !!invite));

  const signup = $derived(mode === 'signup');
  const recover = $derived(mode === 'recover');
  // Both creating an account and resetting a password choose a new one.
  const choosing = $derived(signup || recover);

  function switchMode(m) {
    mode = m;
    error = '';
    password = '';
    confirm = '';
  }

  async function submit(e) {
    e.preventDefault();
    error = '';
    if (choosing && password !== confirm) {
      error = "The passwords don't match.";
      return;
    }
    busy = true;
    try {
      if (signup) await register(username, password, invite);
      else if (recover) await recoverAccount(username.trim(), recoveryKey, password);
      else await login(username, password);
    } catch (err) {
      error =
        {
          invalid_credentials: recover ? "That recovery key doesn't match this account." : 'Wrong username or password.',
          account_disabled: 'This account has been disabled. Ask the person who runs this server.',
          invalid_invite: 'This invite link has already been used or has expired. Ask for a new one.',
          registration_closed: 'New accounts are not being accepted on this server right now.',
        }[err?.code] ?? errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<main class="flex min-h-dvh flex-col items-center px-4 pt-[12vh] pb-8">
  <img src="/img/logo.webp" alt="thencloud" width="715" height="349" class="h-auto w-56 select-none" draggable="false" />

  <div class="mt-8 w-full max-w-sm">
    {#if recover}
      <div class="grid gap-1">
        <button type="button" class="flex w-fit cursor-pointer items-center gap-1 text-[13px] text-fg-muted hover:text-fg" onclick={() => switchMode('signin')}>
          <Icon name="arrow-left" class="size-3.5" /> Back to sign in
        </button>
        <h1 class="mt-2 text-base font-semibold tracking-tight">Reset your password</h1>
        <p class="text-[13px] text-fg-muted">
          Use the recovery key you saved. Your files stay as they are; you'll be signed out on your other devices.
        </p>
      </div>
    {:else}
      <div class="grid grid-cols-2 rounded-lg border border-line bg-subtle p-1 text-sm" role="tablist">
        {#each [['signin', 'Sign in'], ['signup', 'Create account']] as [m, label] (m)}
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
      <p class="mt-4 flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="user-round-plus" class="size-4" />You've been invited to create an account here.</p>
    {:else if signup && !canSignUp}
      <p class="mt-4 flex items-start gap-2 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
        <Icon name="lock" class="mt-0.5 size-4 shrink-0" />
        {registration === 'invite' ? 'This server is invite-only. Open the invite link you were sent to create an account.' : 'This server is not accepting new accounts.'}
      </p>
    {/if}

    <form class="mt-6 grid gap-4" onsubmit={submit}>
      <div class="field">
        <label class="label" for="username">Username</label>
        <input id="username" class="input" bind:value={username} autocomplete="username" autocapitalize="none" spellcheck="false" required />
      </div>
      {#if recover}
        <div class="field">
          <label class="label" for="recovery-key">Recovery key</label>
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
          <label class="label" for="password">{recover ? 'New password' : 'Password'}</label>
          {#if mode === 'signin'}
            <button type="button" class="cursor-pointer text-xs text-fg-muted hover:text-fg" onclick={() => switchMode('recover')}>Forgot your password?</button>
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
            aria-label={showPassword ? 'Hide password' : 'Show password'}
            onclick={() => (showPassword = !showPassword)}>
            <Icon name={showPassword ? 'eye-off' : 'eye'} />
          </button>
        </div>
        {#if choosing}<p class="hint">At least 10 characters. A few random words works well.</p>{/if}
      </div>
      {#if choosing}
        <div class="field">
          <label class="label" for="confirm">Confirm password</label>
          <input id="confirm" class="input" type={showPassword ? 'text' : 'password'} bind:value={confirm} autocomplete="new-password" required />
        </div>
      {/if}
      {#if signup}
        <div class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
          <Icon name="key-round" class="mt-0.5 size-4 shrink-0 text-fg" />
          <p>
            <span class="font-medium text-fg">There is no password reset.</span>
            Your password is the only way to decrypt your files, and it never leaves this device. If you forget it, nobody can recover your data, unless you create a recovery key in Settings.
          </p>
        </div>
      {/if}

      {#if endedElsewhere && !error}
        <p class="flex items-center gap-2 text-[13px] text-fg-muted" role="status"><Icon name="log-out" />You were signed out, maybe from another device. Sign in again to continue.</p>
      {/if}
      {#if error}
        <p class="flex items-center gap-2 text-[13px] text-danger" role="alert"><Icon name="circle-alert" />{error}</p>
      {/if}

      <button class="btn btn-primary btn-lg w-full" disabled={busy || (signup && !canSignUp)}>
        {#if busy}
          <Icon name="loader-circle" class="spinner" />
          {signup ? 'Generating your keys' : recover ? 'Setting your new password' : 'Unlocking your files'}
        {:else}
          {signup ? 'Create account' : recover ? 'Set new password' : 'Sign in'}
        {/if}
      </button>
    </form>

    <p class="mt-8 flex items-start gap-2 text-[13px] leading-5 text-fg-muted">
      <Icon name="shield-check" class="mt-0.5 size-4 shrink-0" />
      Files are encrypted in your browser before upload. The server stores only ciphertext and never sees your password, your keys or your file names.
    </p>
  </div>

  <footer class="mt-auto pt-12 text-xs text-fg-faint">Open source · AGPL-3.0</footer>
</main>
