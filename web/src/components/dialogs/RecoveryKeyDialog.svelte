<script>
  // Create or replace the recovery key: confirm the password, then show the
  // key once. It's made in this browser and never sent to the server.
  import { formatDateTime } from '../../lib/locale.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';
  import { serverOrigin } from '../../lib/server.svelte.js';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { session, createRecoveryKey, avatar } from '../../lib/cloud.svelte.js';
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
      error = e?.code === 'invalid_credentials' ? t('That password is wrong.') : errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function saveFile() {
    const text = [
      t('thencloud recovery key'),
      '',
      t('Account: {name}', { name: session.me.username }),
      t('Server: {address}', { address: serverOrigin() }),
      t('Created: {when}', { when: formatDateTime(Date.now()) }),
      '',
      key,
      '',
      t('With this key you can set a new password if you forget yours.'),
      t('Anyone who has it and your username can do the same, so keep it private.'),
      '',
    ].join('\n');
    saveBlob(new Blob([text], { type: 'text/plain' }), 'thencloud-recovery-key.txt');
  }
</script>

<Modal
  title={key ? t('Your recovery key') : replacing ? t('Replace your recovery key') : t('Create a recovery key')}
  description={key
    ? t("Write it down or save it somewhere safe, away from this device. It won't be shown again.")
    : replacing
      ? t('The old key stops working as soon as the new one is made.')
      : t('If you forget your password, this key lets you set a new one without losing your files.')}
  {onclose}
  onsubmit={submit}
  class="max-w-lg">
  {#if !key}
    <div class="field">
      <label class="label" for="rk-password">{t('Your password')}</label>
      <input id="rk-password" class="input" type="password" bind:value={password} autocomplete="current-password" required />
    </div>
    <p class="hint">
      {t("The key is made in this browser. The server only stores your master key locked with it, so it can't use the key or see it.")}
    </p>
  {:else}
    <div class="grid gap-3">
      <p class="rounded-md border border-line bg-subtle px-4 py-3 text-center font-mono text-[15px] leading-7 tracking-wide break-all select-all">{key}</p>
      <div class="flex flex-wrap gap-2">
        <button type="button" class="btn btn-secondary" onclick={() => copyText(key, t('Recovery key copied'))}><Icon name="copy" /> {t('Copy')}</button>
        <button type="button" class="btn btn-secondary" onclick={saveFile}><Icon name="download" /> {t('Save as a text file')}</button>
      </div>
      <p class="hint">{t('Anyone with this key and your username can take over your account, so keep it as private as your password.')}</p>
      <label class="flex cursor-pointer items-center gap-2 text-sm">
        <input type="checkbox" class="size-4 accent-accent" bind:checked={stored} />
        {t("I've stored it somewhere safe", { gender: avatar.details?.gender })}
      </label>
    </div>
  {/if}
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    {#if !key}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
      <button class="btn btn-primary" disabled={busy || !password}>
        {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
        {replacing ? t('Replace key') : t('Create key')}
      </button>
    {:else}
      <button class="btn btn-primary" disabled={!stored}>{t('Done')}</button>
    {/if}
  {/snippet}
</Modal>
