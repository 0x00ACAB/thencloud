<script>
  // A date for a file or folder to go to the trash on its own. The server
  // has to know the date to act on it while none of your devices is online,
  // so the dialog says so; it never learns the name.
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { session, setTrashAt } from '../../lib/cloud.svelte.js';
  import { formatDateTime } from '../../lib/locale.svelte.js';
  import { toast, toastError } from '../../lib/ui.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';

  let { entry, onclose, onsaved } = $props();

  const DAY = 86400_000;
  const current = untrack(() => entry.node.trash_at);
  const isoDay = (ms) => new Date(ms - new Date(ms).getTimezoneOffset() * 60_000).toISOString().slice(0, 10);

  let choice = $state(current ? 'date' : '7');
  let date = $state(isoDay(current ? current * 1000 : Date.now() + 7 * DAY));
  let busy = $state(false);

  const presets = $derived([
    ['1', t('In 1 day')],
    ['7', t('In a week')],
    ['30', t('In a month')],
    ['90', t('In three months')],
    ['date', t('On a date')],
  ]);

  // The moment it goes: a preset from now, or the start of the chosen day.
  const at = $derived.by(() => {
    if (choice === 'date') {
      const ms = new Date(`${date}T00:00`).getTime();
      return Number.isFinite(ms) ? ms : null;
    }
    return Date.now() + Number(choice) * DAY;
  });
  const valid = $derived(at !== null && at > Date.now());

  async function save(trashAt) {
    busy = true;
    try {
      await setTrashAt(entry, trashAt === null ? null : Math.floor(trashAt / 1000));
      toast(trashAt === null ? t("{name} won't be deleted automatically", { name: entry.meta.name }) : t('{name} goes to the trash on {date}', { name: entry.meta.name, date: formatDateTime(trashAt, { dateStyle: 'medium' }) }), { kind: 'success' });
      onsaved?.();
      onclose();
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={t('Delete automatically')} description={entry.meta.name} {onclose} onsubmit={() => valid && save(at)} class="max-w-md">
  <div class="grid gap-1.5" role="radiogroup" aria-label={t('When')}>
    {#each presets as [value, label] (value)}
      <label class="flex cursor-pointer items-center gap-2 text-sm">
        <input type="radio" name="when" {value} bind:group={choice} />
        {label}
      </label>
    {/each}
    {#if choice === 'date'}
      <input type="date" class="input ml-6 w-auto" bind:value={date} min={isoDay(Date.now() + DAY)} aria-label={t('Date')} />
    {/if}
  </div>
  <p class="hint">
    {t('It goes to the trash then, where it stays for {count} days like anything you delete. To do this while none of your devices is online, the server is told the date; it never learns the name.', { count: session.me.trash_days })}
  </p>
  {#snippet footer()}
    {#if current}
      <button type="button" class="btn btn-secondary mr-auto" disabled={busy} onclick={() => save(null)}>{t("Don't delete it")}</button>
    {/if}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
    <button class="btn btn-primary" disabled={busy || !valid}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {t('Save')}
    </button>
  {/snippet}
</Modal>
