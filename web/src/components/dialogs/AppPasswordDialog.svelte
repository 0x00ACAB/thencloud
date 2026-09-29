<script>
  // Create an app password: name it, pick its access, confirm the account
  // password, then show it once. It's made in this browser.
  import { t } from '../../lib/i18n.svelte.js';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { createAppPassword } from '../../lib/cloud.svelte.js';
  import { copyText, errorMessage } from '../../lib/ui.svelte.js';

  let { onclose, oncreated } = $props();

  let name = $state('');
  let scope = $state('full');
  let password = $state('');
  let busy = $state(false);
  let error = $state('');
  let secret = $state(null);

  const scopes = $derived([
    ['full', t('Full access'), t('Read, upload, change and delete files.')],
    ['read', t('Read only'), t('Browse and download. Good for backups.')],
  ]);

  async function submit() {
    if (secret) return onclose();
    busy = true;
    error = '';
    try {
      secret = await createAppPassword(password, name.trim(), scope);
      password = '';
      oncreated?.();
    } catch (e) {
      error = e?.code === 'invalid_credentials' ? t('That password is wrong.') : errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  title={secret ? t('Your app password') : t('New app password')}
  description={secret
    ? t("Enter it in {name} to sign in. It won't be shown again.", { name: name.trim() })
    : t('For a sync client or another device, so it never needs your account password.')}
  {onclose}
  onsubmit={submit}
  class="max-w-lg">
  {#if !secret}
    <div class="field">
      <label class="label" for="ap-name">{t('Name')}</label>
      <input id="ap-name" class="input" bind:value={name} placeholder={t('Laptop sync')} maxlength="100" required />
    </div>
    <div class="grid gap-2 sm:grid-cols-2" role="radiogroup" aria-label={t('Access')}>
      {#each scopes as [value, label, text] (value)}
        <button
          type="button"
          role="radio"
          aria-checked={scope === value}
          class="grid cursor-pointer gap-1 rounded-md border p-3 text-left transition-colors {scope === value ? 'border-accent bg-accent-soft' : 'border-line hover:bg-subtle'}"
          onclick={() => (scope = value)}>
          <span class="text-sm font-medium {scope === value ? 'text-accent-text' : ''}">{label}</span>
          <span class="text-xs text-fg-muted">{text}</span>
        </button>
      {/each}
    </div>
    <div class="field">
      <label class="label" for="ap-password">{t('Your account password')}</label>
      <input id="ap-password" class="input" type="password" bind:value={password} autocomplete="current-password" required />
    </div>
    <p class="hint">{t('The app password is made in this browser. The server keeps your master key locked with it and a hash to recognise it, never the password itself.')}</p>
  {:else}
    <div class="grid gap-3">
      <p class="rounded-md border border-line bg-subtle px-4 py-3 text-center font-mono text-[15px] leading-7 tracking-wide break-all select-all">{secret}</p>
      <div><button type="button" class="btn btn-secondary" onclick={() => copyText(secret, t('App password copied'))}><Icon name="copy" /> {t('Copy')}</button></div>
      <p class="hint">{scope === 'read' ? t('It opens your files (read only), so treat it like a password. You can revoke it here at any time.') : t('It opens your files, so treat it like a password. You can revoke it here at any time.')}</p>
    </div>
  {/if}
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    {#if !secret}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
      <button class="btn btn-primary" disabled={busy || !password || !name.trim()}>
        {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
        {t('Create')}
      </button>
    {:else}
      <button class="btn btn-primary">{t('Done')}</button>
    {/if}
  {/snippet}
</Modal>
