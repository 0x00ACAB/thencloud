<script>
  import { login, register } from '../lib/cloud.svelte.js';
  import { errorMessage } from '../lib/ui.svelte.js';
  import Icon from './Icon.svelte';

  let mode = $state('signin');
  let username = $state('');
  let password = $state('');
  let confirm = $state('');
  let showPassword = $state(false);
  let busy = $state(false);
  let error = $state('');

  const signup = $derived(mode === 'signup');

  function switchMode(m) {
    mode = m;
    error = '';
    password = '';
    confirm = '';
  }

  async function submit(e) {
    e.preventDefault();
    error = '';
    if (signup && password !== confirm) {
      error = "The passwords don't match.";
      return;
    }
    busy = true;
    try {
      if (signup) await register(username, password);
      else await login(username, password);
    } catch (err) {
      error = err?.code === 'invalid_credentials' ? 'Wrong username or password.' : errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<main class="flex min-h-dvh flex-col items-center px-4 pt-[12vh] pb-8">
  <img src="/img/logo.webp" alt="thencloud" width="715" height="349" class="h-auto w-56 select-none" draggable="false" />

  <div class="mt-8 w-full max-w-sm">
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

    <form class="mt-6 grid gap-4" onsubmit={submit}>
      <div class="field">
        <label class="label" for="username">Username</label>
        <input id="username" class="input" bind:value={username} autocomplete="username" autocapitalize="none" spellcheck="false" required />
      </div>
      <div class="field">
        <label class="label" for="password">Password</label>
        <div class="relative">
          <input
            id="password"
            class="input pr-9"
            type={showPassword ? 'text' : 'password'}
            bind:value={password}
            autocomplete={signup ? 'new-password' : 'current-password'}
            minlength={signup ? 10 : undefined}
            required />
          <button
            type="button"
            class="absolute inset-y-0 right-0 grid w-9 cursor-pointer place-items-center text-fg-faint hover:text-fg"
            aria-label={showPassword ? 'Hide password' : 'Show password'}
            onclick={() => (showPassword = !showPassword)}>
            <Icon name={showPassword ? 'eye-off' : 'eye'} />
          </button>
        </div>
        {#if signup}<p class="hint">At least 10 characters. A few random words works well.</p>{/if}
      </div>
      {#if signup}
        <div class="field">
          <label class="label" for="confirm">Confirm password</label>
          <input id="confirm" class="input" type={showPassword ? 'text' : 'password'} bind:value={confirm} autocomplete="new-password" required />
        </div>
        <div class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
          <Icon name="key-round" class="mt-0.5 size-4 shrink-0 text-fg" />
          <p>
            <span class="font-medium text-fg">There is no password reset.</span>
            Your password is the only way to decrypt your files, and it never leaves this device. If you forget it, nobody can recover your data.
          </p>
        </div>
      {/if}

      {#if error}
        <p class="flex items-center gap-2 text-[13px] text-danger" role="alert"><Icon name="circle-alert" />{error}</p>
      {/if}

      <button class="btn btn-primary btn-lg w-full" disabled={busy}>
        {#if busy}
          <Icon name="loader-circle" class="spinner" />
          {signup ? 'Generating your keys' : 'Unlocking your files'}
        {:else}
          {signup ? 'Create account' : 'Sign in'}
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
