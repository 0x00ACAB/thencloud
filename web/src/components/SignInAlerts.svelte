<script>
  // A notice for each new sign-in to this account (lib/signins.svelte.js),
  // with a way to sign it out. A sign-in with the recovery key is called out:
  // it also changed the password.
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import Sentence from './Sentence.svelte';
  import { signIns, watchSignIns, dismiss, signOut } from '../lib/signins.svelte.js';
  import { formatWhen, fullDate } from '../lib/format.js';
  import { toast, toastError } from '../lib/ui.svelte.js';
  import { slide } from '../lib/motion.js';
  import { t } from '../lib/i18n.svelte.js';

  onMount(watchSignIns);

  let busy = $state(null);

  const methodText = (s) =>
    ({
      password: t('with your password'),
      'password+code': t('with your password and a code'),
      'password+passkey': t('with your password and a passkey'),
      passkey: t('with a passkey'),
      recovery: t('with your recovery key, which also set a new password'),
      app_password: t('with an app password'),
    })[s.method] ?? '';

  async function run(s, action) {
    busy = s.id;
    try {
      await action(s);
      if (action === signOut) toast(t('Signed out {device}', { device: s.device_name }), { kind: 'success' });
    } catch (e) {
      toastError(e);
    } finally {
      busy = null;
    }
  }
</script>

{#if signIns.alerts.length}
  <div class="mb-6 grid gap-2" role="region" aria-label={t('New sign-ins')}>
    {#each signIns.alerts as s (s.id)}
      <div class="card flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3 {s.method === 'recovery' ? 'border-danger' : ''}" transition:slide>
        <Icon name="shield-check" class="size-5 shrink-0 {s.method === 'recovery' ? 'text-danger' : 'text-fg-muted'}" />
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium">{t('New sign-in to your account')}</p>
          <p class="text-[13px] text-fg-muted">
            <Sentence text={methodText(s) ? t('{device}, {how}, {when}.') : t('{device}, {when}.')}>
              {#snippet device()}<span class="font-medium text-fg">{s.device_name}</span>{/snippet}
              {#snippet how()}{methodText(s)}{/snippet}
              {#snippet when()}<span title={fullDate(s.created_at * 1000)}>{formatWhen(s.created_at * 1000)}</span>{/snippet}
            </Sentence>
            {t("If it wasn't you, sign it out and change your password.")}
          </p>
        </div>
        <div class="flex shrink-0 gap-2">
          <button type="button" class="btn btn-secondary h-8" disabled={busy === s.id} onclick={() => run(s, dismiss)}>{t('It was me')}</button>
          <button type="button" class="btn btn-danger h-8" disabled={busy === s.id} onclick={() => run(s, signOut)}>
            {#if busy === s.id}<Icon name="loader-circle" class="spinner" />{/if}{t('Sign it out')}
          </button>
        </div>
      </div>
    {/each}
  </div>
{/if}
