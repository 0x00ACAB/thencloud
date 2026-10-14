<script>
  // A password or short note, shared as a one-time public link
  // (lib/cloud.svelte.js: sendSecurely). Kept as an encrypted file in a
  // "Secure Send" folder in My files, same as anything else uploaded; the
  // link stops working once it's been opened (or sooner, if you choose).
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { sendSecurely } from '../../lib/cloud.svelte.js';
  import { copyText, errorMessage } from '../../lib/ui.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';

  let { onclose } = $props();

  const MAX_LENGTH = 20_000;
  let text = $state('');
  let password = $state('');
  let expiry = $state('7');
  let opens = $state('1');
  let busy = $state(false);
  let error = $state('');
  let created = $state(null);

  const expiries = [
    ['never', t('Never')],
    ['1', t('In {count} days', { count: 1 })],
    ['7', t('In {count} days', { count: 7 })],
    ['30', t('In {count} days', { count: 30 })],
  ];
  const openLimits = [
    ['1', t('After the first open')],
    ['5', t('After {count} opens', { count: 5 })],
    ['25', t('After {count} opens', { count: 25 })],
    ['any', t('No limit')],
  ];

  // Text with the # of the link shown as code: the translation keeps {hash}.
  const hashParts = (s) => s.split('{hash}');

  async function submit() {
    if (!text.trim() || busy) return;
    busy = true;
    error = '';
    try {
      const expiresAt = expiry === 'never' ? null : Math.floor(Date.now() / 1000) + Number(expiry) * 86400;
      const maxOpens = opens === 'any' ? null : Number(opens);
      created = await sendSecurely(text, { password, expiresAt, maxOpens });
      text = '';
      password = '';
      copyText(created.url, t('Link created and copied'));
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  title={t('Secure Send')}
  description={t('Share a password or a short note as a one-time link, without it sitting in a chat or an email.')}
  {onclose}
  onsubmit={submit}
  class="max-w-lg">
  {#if created}
    <div class="grid gap-2">
      <p class="flex gap-2.5 rounded-md border border-line bg-subtle p-3 text-[13px] text-fg-muted">
        <Icon name="key-round" class="mt-0.5 size-4 shrink-0 text-fg" />
        <span>
          {#each hashParts(t("The decryption key is the part of the link after {hash}. Browsers never send that part to the server, so thencloud can't read what you share. Treat the whole link like a password.")) as part, i (i)}{#if i}<code class="font-mono text-fg">#</code>{/if}{part}{/each}
        </span>
      </p>
      <div class="flex items-center gap-2 rounded-md border border-line p-2">
        <code class="min-w-0 flex-1 truncate font-mono text-[13px]">{created.url}</code>
        <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Copy link')} title={t('Copy link')} onclick={() => copyText(created.url, t('Link copied'))}>
          <Icon name="copy" />
        </button>
      </div>
      <button type="button" class="btn btn-secondary" onclick={() => (created = null)}><Icon name="plus" />{t('Send another')}</button>
    </div>
  {:else}
    <div class="field">
      <label class="label" for="secure-text">{t('Message')}</label>
      <textarea id="secure-text" class="input" rows="4" maxlength={MAX_LENGTH} bind:value={text} placeholder={t('A password, a code, a short note…')}></textarea>
    </div>
    <div class="grid gap-3 sm:grid-cols-2">
      <div class="field">
        <label class="label" for="secure-password">{t('Password')} <span class="font-normal text-fg-muted">{t('(optional)')}</span></label>
        <input id="secure-password" class="input" type="password" bind:value={password} autocomplete="new-password" />
      </div>
      <div class="field">
        <label class="label" for="secure-expiry">{t('Expires')}</label>
        <select id="secure-expiry" class="input" bind:value={expiry}>
          {#each expiries as [value, label] (value)}<option {value}>{label}</option>{/each}
        </select>
      </div>
      <div class="field sm:col-span-2">
        <label class="label" for="secure-opens">{t('Stops working')}</label>
        <select id="secure-opens" class="input" bind:value={opens}>
          {#each openLimits as [value, label] (value)}<option {value}>{label}</option>{/each}
        </select>
      </div>
    </div>
    {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {/if}
  {#snippet footer()}
    {#if created}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Done')}</button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
      <button class="btn btn-primary" disabled={busy || !text.trim()}>
        {#if busy}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="link" />{/if}
        {t('Create link')}
      </button>
    {/if}
  {/snippet}
</Modal>
