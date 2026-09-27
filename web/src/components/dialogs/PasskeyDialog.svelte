<script>
  // Add a passkey: name it, confirm the password, then the browser's prompt.
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { addPasskey } from '../../lib/cloud.svelte.js';
  import { cancelled } from '../../lib/passkeys.js';
  import { errorMessage } from '../../lib/ui.svelte.js';

  let { onclose, onadded } = $props();

  const guess = /iPhone|iPad/.test(navigator.userAgent) ? 'iPhone' : /Android/.test(navigator.userAgent) ? 'Android phone' : /Mac OS X/.test(navigator.userAgent) ? 'Mac' : '';
  let name = $state(guess);
  let password = $state('');
  let busy = $state(false);
  let error = $state('');

  async function submit() {
    busy = true;
    error = '';
    try {
      const p = await addPasskey(name.trim(), password);
      onadded?.(p);
      onclose();
    } catch (e) {
      error = cancelled(e)
        ? ''
        : e?.name === 'InvalidStateError'
          ? 'That passkey is already added.'
          : ({ invalid_credentials: 'That password is wrong.', sign_in_expired: 'That took too long. Try again.' }[e?.code] ?? errorMessage(e));
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Add a passkey" description="Your phone, computer or security key confirms it's you with a fingerprint, face or PIN." {onclose} onsubmit={submit} class="max-w-md">
  <div class="field">
    <label class="label" for="pk-name">Name</label>
    <input id="pk-name" class="input" bind:value={name} placeholder="Laptop, YubiKey..." maxlength="100" required />
  </div>
  <div class="field">
    <label class="label" for="pk-password">Your password</label>
    <input id="pk-password" class="input" type="password" bind:value={password} autocomplete="current-password" required />
  </div>
  <p class="hint">
    If your passkey supports it, it also gets its own locked copy of your master key, so it can sign you in without your password. The browser unlocks
    that copy; the server can't.
  </p>
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
    <button class="btn btn-primary" disabled={busy || !password || !name.trim()}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      Continue
    </button>
  {/snippet}
</Modal>
