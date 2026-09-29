<script>
  // Set up an authenticator app: confirm the password, scan the code (or
  // type the secret), then enter a code from the app to turn it on.
  import { t } from '../../lib/i18n.svelte.js';
  import { serverOrigin } from '../../lib/server.svelte.js';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import QrCode from '../QrCode.svelte';
  import { session, startTotp, enableTotp } from '../../lib/cloud.svelte.js';
  import { copyText, errorMessage } from '../../lib/ui.svelte.js';

  let { onclose, ondone } = $props();

  let password = $state('');
  let setup = $state(null); // { setup_id, secret }
  let code = $state('');
  let busy = $state(false);
  let error = $state('');

  const uri = $derived.by(() => {
    if (!setup) return '';
    const issuer = `thencloud (${new URL(serverOrigin()).host})`;
    const label = encodeURIComponent(`${issuer}:${session.me.username}`);
    return `otpauth://totp/${label}?secret=${setup.secret}&issuer=${encodeURIComponent(issuer)}&algorithm=SHA1&digits=6&period=30`;
  });
  const grouped = $derived(setup?.secret.match(/.{1,4}/g).join(' ') ?? '');

  async function submit() {
    busy = true;
    error = '';
    try {
      if (!setup) {
        setup = await startTotp(password);
        password = '';
      } else {
        await enableTotp(setup.setup_id, code);
        ondone?.();
        onclose();
      }
    } catch (e) {
      error =
        {
          invalid_credentials: t('That password is wrong.'),
          invalid_second_factor: t("That code didn't match. Check the time on your phone is right, and use the newest code."),
          sign_in_expired: t('That took too long. Close this and start again.'),
        }[e?.code] ?? errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  title={t('Set up an authenticator app')}
  description={setup
    ? t('Scan this with an app like Aegis, 2FAS, Google Authenticator or 1Password, then enter the code it shows.')
    : t('After this, signing in with your password also asks for a six-digit code from the app.')}
  {onclose}
  onsubmit={submit}
  class="max-w-md">
  {#if !setup}
    <div class="field">
      <label class="label" for="totp-password">{t('Your password')}</label>
      <input id="totp-password" class="input" type="password" bind:value={password} autocomplete="current-password" required />
    </div>
    <p class="hint">{t("The code is checked by the server before it lets you in. It doesn't encrypt anything, so losing the app never costs you files.")}</p>
  {:else}
    <div class="grid justify-items-center gap-3">
      <QrCode text={uri} label={t('Authenticator app setup code')} class="size-44 rounded-md border border-line" />
      <div class="flex items-center gap-1">
        <code class="font-mono text-[13px] tracking-wide text-fg-muted select-all">{grouped}</code>
        <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Copy the secret')} title={t('Copy')} onclick={() => copyText(setup.secret, t('Secret copied'))}>
          <Icon name="copy" />
        </button>
      </div>
    </div>
    <div class="field">
      <label class="label" for="totp-confirm">{t('Code from the app')}</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        id="totp-confirm"
        class="input font-mono tracking-widest"
        bind:value={code}
        inputmode="numeric"
        autocomplete="one-time-code"
        maxlength="7"
        placeholder="123456"
        autofocus
        required />
    </div>
  {/if}
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
    <button class="btn btn-primary" disabled={busy || (setup ? code.replace(/\s/g, '').length !== 6 : !password)}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {setup ? t('Turn on') : t('Continue')}
    </button>
  {/snippet}
</Modal>
