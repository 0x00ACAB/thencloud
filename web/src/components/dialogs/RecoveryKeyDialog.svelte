<script>
  // Create or replace the recovery key: confirm the password, then show the
  // key once. It's made in this browser and never sent to the server.
  import { formatDateTime } from '../../lib/locale.svelte.js';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { session, createRecoveryKey } from '../../lib/cloud.svelte.js';
  import { saveBlob } from '../../lib/crypto.js';
  import { copyText, errorMessage } from '../../lib/ui.svelte.js';

  let { onclose } = $props();

  const replacing = !!session.me.recovery_created_at;
  let password = $state('');
  let busy = $state(false);
  let error = $state('');
  let key = $state(null);
  let stored = $state(false);

  async function submit() {
    if (key) return stored && onclose();
    busy = true;
    error = '';
    try {
      key = await createRecoveryKey(password);
      password = '';
    } catch (e) {
      error = e?.code === 'invalid_credentials' ? 'That password is wrong.' : errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function saveFile() {
    const text = [
      'thencloud recovery key',
      '',
      `Account: ${session.me.username}`,
      `Server:  ${location.origin}`,
      `Created: ${formatDateTime(Date.now())}`,
      '',
      key,
      '',
      'With this key you can set a new password if you forget yours.',
      'Anyone who has it and your username can do the same, so keep it private.',
      '',
    ].join('\n');
    saveBlob(new Blob([text], { type: 'text/plain' }), 'thencloud-recovery-key.txt');
  }
</script>

<Modal
  title={key ? 'Your recovery key' : replacing ? 'Replace your recovery key' : 'Create a recovery key'}
  description={key
    ? "Write it down or save it somewhere safe, away from this device. It won't be shown again."
    : replacing
      ? 'The old key stops working as soon as the new one is made.'
      : 'If you forget your password, this key lets you set a new one without losing your files.'}
  {onclose}
  onsubmit={submit}
  class="max-w-lg">
  {#if !key}
    <div class="field">
      <label class="label" for="rk-password">Your password</label>
      <input id="rk-password" class="input" type="password" bind:value={password} autocomplete="current-password" required />
    </div>
    <p class="hint">
      The key is made in this browser. The server only stores your master key locked with it, so it can't use the key or see it.
    </p>
  {:else}
    <div class="grid gap-3">
      <p class="rounded-md border border-line bg-subtle px-4 py-3 text-center font-mono text-[15px] leading-7 tracking-wide break-all select-all">{key}</p>
      <div class="flex flex-wrap gap-2">
        <button type="button" class="btn btn-secondary" onclick={() => copyText(key, 'Recovery key copied')}><Icon name="copy" /> Copy</button>
        <button type="button" class="btn btn-secondary" onclick={saveFile}><Icon name="download" /> Save as a text file</button>
      </div>
      <p class="hint">Anyone with this key and your username can take over your account, so keep it as private as your password.</p>
      <label class="flex cursor-pointer items-center gap-2 text-sm">
        <input type="checkbox" class="size-4 accent-accent" bind:checked={stored} />
        I've stored it somewhere safe
      </label>
    </div>
  {/if}
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    {#if !key}
      <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
      <button class="btn btn-primary" disabled={busy || !password}>
        {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
        {replacing ? 'Replace key' : 'Create key'}
      </button>
    {:else}
      <button class="btn btn-primary" disabled={!stored}>Done</button>
    {/if}
  {/snippet}
</Modal>
