<script>
  // Where a file (or every file in a folder) is kept: this server or a
  // linked Google Drive, older versions included, and moving just these.
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { nodeStorage, moveNodeStorage } from '../../lib/cloud.svelte.js';
  import { storage, loadStorage } from '../../lib/storage.svelte.js';
  import { toast, toastError } from '../../lib/ui.svelte.js';
  import { formatSize } from '../../lib/format.js';
  import { t } from '../../lib/i18n.svelte.js';

  let { entry, onclose } = $props();

  let places = $state(null);
  let moving = $state(null); // { to, done, total }
  let stop = false;

  const folder = $derived(entry.node.kind === 'folder');
  const accounts = $derived(storage.info?.accounts ?? []);
  const accountOf = (id) => accounts.find((a) => a.id === id);
  const total = $derived((places ?? []).reduce((n, p) => n + p.bytes, 0));
  const onServer = $derived((places ?? []).find((p) => p.account_id == null)?.bytes ?? 0);
  // Extra space it could go to: not broken, and not already holding all of it.
  const targets = $derived(
    accounts.filter((a) => a.mode === 'extra' && !a.broken && (places ?? []).find((p) => p.account_id === a.id)?.bytes !== total),
  );

  async function load() {
    try {
      places = (await nodeStorage(entry.node.id)).places;
    } catch (e) {
      toastError(e);
      onclose();
    }
  }

  onMount(() => {
    load();
    if (!storage.info) loadStorage();
    return () => (stop = true);
  });

  // `to`: null for this server, or an account id.
  async function move(to) {
    moving = { to, done: 0, total: 0 };
    stop = false;
    try {
      for (;;) {
        const r = await moveNodeStorage(entry.node.id, to);
        moving.done += r.moved_bytes;
        moving.total = moving.done + r.left_bytes;
        if (r.left === 0) break;
        if (r.moved === 0 || stop) {
          if (r.full) toast(to == null ? t("This server doesn't have room for the rest of your files.") : t("This Google Drive doesn't have room for the rest of your files."), { kind: 'error' });
          break;
        }
      }
    } catch (e) {
      toastError(e);
    } finally {
      moving = null;
      await Promise.all([load(), loadStorage()]);
    }
  }

  const pct = (done, all) => (all > 0 ? Math.min(100, (done / all) * 100) : 100);
</script>

<Modal title={t('Where {name} is kept', { name: entry.meta.name })} {onclose}>
  {#if places === null}
    <div class="skeleton h-16 w-full" aria-hidden="true"></div>
  {:else if total === 0}
    <p class="text-sm text-fg-muted">{folder ? t('There are no files in this folder.') : t('This file is empty.')}</p>
  {:else}
    <p class="text-[13px] text-fg-muted">
      {folder ? t('Every file in this folder, older versions included.') : t('This file, older versions included.')}
    </p>
    <ul class="divide-y divide-line rounded-md border border-line">
      {#each places as p (p.account_id ?? 'server')}
        {@const a = p.account_id ? accountOf(p.account_id) : null}
        <li class="flex items-center gap-3 px-3 py-2.5 text-sm">
          <span class="size-2.5 shrink-0 rounded-full {p.account_id ? 'bg-place-google' : 'bg-accent'}" aria-hidden="true"></span>
          <span class="min-w-0 flex-1 truncate">
            {#if p.account_id}
              {t('Google Drive')}{#if a?.label}<span class="text-fg-muted"> · {a.label}</span>{/if}
            {:else}
              {t('This server')}
            {/if}
          </span>
          <span class="text-fg-muted tabular-nums">{formatSize(p.bytes)}</span>
        </li>
      {/each}
    </ul>
    {#if moving}
      <div class="grid gap-1">
        <div class="flex items-center gap-2">
          <p class="flex-1 text-xs text-fg-muted tabular-nums">
            {#if moving.to == null}
              {t('Moving to this server: {done} of {total}', { done: formatSize(moving.done), total: formatSize(moving.total) })}
            {:else}
              {t('Moving to Google Drive: {done} of {total}', { done: formatSize(moving.done), total: formatSize(moving.total) })}
            {/if}
          </p>
          <button type="button" class="btn btn-ghost h-7 px-2 text-xs" onclick={() => (stop = true)}>{t('Stop')}</button>
        </div>
        <div class="progress"><div class={moving.to == null ? 'bg-accent' : 'bg-place-google'} style:width="{pct(moving.done, moving.total)}%"></div></div>
      </div>
    {/if}
  {/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary mr-auto" onclick={onclose}>{t('Close')}</button>
    {#if places && total > 0}
      {#each targets as a (a.id)}
        <button type="button" class="btn btn-secondary" disabled={moving !== null} onclick={() => move(a.id)}>
          <Icon name="arrow-up-from-line" />{t('Move to Google Drive')}{#if targets.length > 1 && a.label}<span class="text-fg-muted">{a.label}</span>{/if}
        </button>
      {/each}
      {#if onServer < total}
        <button type="button" class="btn btn-secondary" disabled={moving !== null} onclick={() => move(null)}>
          <Icon name="arrow-down-to-line" />{t('Move to this server')}
        </button>
      {/if}
    {/if}
  {/snippet}
</Modal>
