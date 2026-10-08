<script>
  // How hard a new password would be to guess, worked out in this browser
  // (lib/strength.js); nothing is sent anywhere.
  import { passwordScore } from '../lib/strength.js';
  import { t } from '../lib/i18n.svelte.js';

  /** `inputs`: words the password shouldn't lean on, like the username. */
  let { password, inputs = [], minLength = 10 } = $props();

  let score = $state(null);
  let asked = 0;

  $effect(() => {
    const value = password;
    const words = [...inputs];
    const n = ++asked;
    if (!value) {
      score = null;
      return;
    }
    passwordScore(value, words)
      .then((s) => {
        if (n === asked) score = s;
      })
      .catch(() => {
        if (n === asked) score = null;
      });
  });

  const short = $derived(password.length < minLength);
  const shown = $derived(short ? 0 : score);
  const label = $derived(
    short
      ? t('Too short')
      : [t('Very weak'), t('Weak'), t('Fair'), t('Strong'), t('Very strong')][shown],
  );
  const fill = $derived(shown <= 1 ? 'bg-danger' : shown === 2 ? 'bg-warning' : 'bg-success');
</script>

{#if password && shown != null}
  <div class="grid gap-1.5" role="meter" aria-label={t('Password strength')} aria-valuemin="0" aria-valuemax="4" aria-valuenow={shown} aria-valuetext={label}>
    <div class="grid grid-cols-4 gap-1" aria-hidden="true">
      {#each [1, 2, 3, 4] as step (step)}
        <span class="h-1 rounded-full {step <= Math.max(shown, 1) ? fill : 'bg-line-strong'}"></span>
      {/each}
    </div>
    <p class="text-xs text-fg-muted">{t('Strength: {label}', { label })}</p>
  </div>
{/if}
