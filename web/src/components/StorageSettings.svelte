<script>
  // Settings > Linked storage: Google Drive as a mirror of your files or as
  // extra space, and where new files go first. Only ciphertext goes there.
  import { onMount } from 'svelte';
  import { t } from '../lib/i18n.svelte.js';
  import { storage, loadStorage, linkDrive, moveFiles, stopMoving } from '../lib/storage.svelte.js';
  import { setStorageMode, setStoragePrefer, unlinkStorage } from '../lib/cloud.svelte.js';
  import { toast, toastError } from '../lib/ui.svelte.js';
  import { formatSize } from '../lib/format.js';
  import { inApp } from '../lib/server.svelte.js';
  import { slide } from '../lib/motion.js';
  import Icon from './Icon.svelte';
  import ConfirmDialog from './dialogs/ConfirmDialog.svelte';

  let linking = $state(null); // 'mirror' | 'extra' while the popup is open
  let unlinking = $state(null); // the account being confirmed
  let busy = $state(false);

  onMount(loadStorage);

  const info = $derived(storage.info);
  const moving = $derived(storage.moving);
  const hasExtra = $derived(info?.accounts.some((a) => a.mode === 'extra') ?? false);

  async function link(mode) {
    linking = mode;
    try {
      const outcome = await linkDrive(mode);
      if (outcome === 'linked') toast(t('Google Drive is linked'), { kind: 'success' });
      else if (outcome === 'denied') toast(t("Google Drive wasn't linked: access wasn't allowed"), { kind: 'error' });
      else if (outcome !== 'closed') toast(t("Linking Google Drive didn't work. Try again."), { kind: 'error' });
    } catch (e) {
      if (e?.code === 'popup_blocked') toast(t('Allow pop-ups for this site to link Google Drive'), { kind: 'error' });
      else toastError(e);
    } finally {
      linking = null;
    }
  }

  async function changeMode(a, mode) {
    try {
      await setStorageMode(a.id, mode);
      await loadStorage();
    } catch (e) {
      toastError(e);
    }
  }

  async function changePrefer(prefer) {
    try {
      await setStoragePrefer(prefer);
      await loadStorage();
    } catch (e) {
      toastError(e);
    }
  }

  async function move(a, to) {
    try {
      const outcome = await moveFiles(a.id, to);
      if (outcome === 'done') {
        toast(to === 'server' ? t('Your files are back on this server') : t('Your files are in Google Drive now'), { kind: 'success' });
      } else if (outcome === 'full') {
        toast(to === 'server' ? t("This server doesn't have room for the rest of your files.") : t("This Google Drive doesn't have room for the rest of your files."), { kind: 'error' });
      }
    } catch (e) {
      toastError(e);
    }
  }

  // Files kept only in the account come back first; then it's unlinked.
  async function unlink() {
    busy = true;
    try {
      if (unlinking.only_there > 0) {
        const outcome = await moveFiles(unlinking.id, 'server');
        if (outcome === 'full') {
          throw new Error(t("This server doesn't have room for the rest of your files, so the Drive stays linked. Make some room here, then try again."));
        }
        if (outcome !== 'done') throw new Error(t('Moving stopped, so the Drive stays linked.'));
      }
      await unlinkStorage(unlinking.id);
      unlinking = null;
      await loadStorage();
      toast(t('Google Drive is unlinked'), { kind: 'success' });
    } catch (e) {
      if (e?.code === 'conflict') throw new Error(t("Some files are kept only in this Google Drive, so it can't be unlinked yet."));
      throw e;
    } finally {
      busy = false;
    }
  }

  const pct = (done, total) => (total > 0 ? Math.min(100, (done / total) * 100) : 100);
</script>

<section class="card overflow-hidden">
  <div class="grid grid-cols-1 gap-4 p-6">
    <div class="grid gap-1">
      <h2 class="text-base font-semibold tracking-tight">{t('Linked storage')}</h2>
      <p class="text-[13px] text-fg-muted">
        {t('Keep a copy of your files in your own Google Drive, or use its space for new files. Only encrypted pieces go there: Google sees their sizes and when they change, never names or contents.')}
      </p>
    </div>

    {#if info === null}
      <div class="skeleton h-12 w-full" aria-hidden="true"></div>
    {:else}
      {#if info.accounts.length}
        <ul class="divide-y divide-line rounded-md border border-line">
          {#each info.accounts as a (a.id)}
            <li class="grid gap-2 px-3 py-3" out:slide>
              <div class="flex items-center gap-3">
                <span class="size-2.5 shrink-0 rounded-full bg-place-google" aria-hidden="true"></span>
                <div class="min-w-0 flex-1">
                  <p class="flex items-center gap-2 text-sm">
                    <span class="font-medium">{t('Google Drive')}</span>
                    {#if a.label}<span class="truncate text-fg-muted">{a.label}</span>{/if}
                  </p>
                  <p class="text-xs text-fg-muted tabular-nums">
                    {#if a.free_bytes != null}
                      {t('{used} used here · {free} free', { used: formatSize(a.used_bytes), free: formatSize(a.free_bytes) })}
                    {:else}
                      {t('{used} used here', { used: formatSize(a.used_bytes) })}
                    {/if}
                  </p>
                </div>
                <select
                  class="input h-8 w-auto text-[13px]"
                  aria-label={t('What this Google Drive is for')}
                  value={a.mode}
                  onchange={(e) => changeMode(a, e.currentTarget.value)}>
                  <option value="mirror">{t('Mirror')}</option>
                  <option value="extra">{t('Extra space')}</option>
                </select>
                <button type="button" class="btn btn-secondary h-8 px-2.5 text-[13px]" onclick={() => (unlinking = a)}>{t('Unlink')}</button>
              </div>
              {#if a.broken}
                <p class="flex items-center gap-1.5 text-xs text-danger">
                  <Icon name="circle-alert" class="size-3.5 shrink-0" />
                  {t('Google stopped letting thencloud in. Unlink it and link it again.')}
                </p>
              {:else if a.mode === 'mirror' && a.mirror_total != null}
                {#if a.mirror_done >= a.mirror_total}
                  <p class="flex items-center gap-1.5 text-xs text-fg-muted"><Icon name="check" class="size-3.5" />{t('A copy of everything here is in this Drive.')}</p>
                {:else}
                  <div class="grid gap-1">
                    <p class="text-xs text-fg-muted tabular-nums">
                      {t('Copying: {done} of {total}', { done: formatSize(a.mirror_done), total: formatSize(a.mirror_total) })}
                    </p>
                    <div class="progress"><div class="bg-place-google" style:width="{pct(a.mirror_done, a.mirror_total)}%"></div></div>
                  </div>
                {/if}
              {:else if a.mode === 'extra'}
                <p class="text-xs text-fg-muted">{t('New files can be kept here instead of on this server.')}</p>
              {/if}
              {#if moving?.id === a.id}
                <div class="grid gap-1">
                  <div class="flex items-center gap-2">
                    <p class="flex-1 text-xs text-fg-muted tabular-nums">
                      {#if moving.to === 'server'}
                        {t('Moving to this server: {done} of {total}', { done: formatSize(moving.done), total: formatSize(moving.total) })}
                      {:else}
                        {t('Moving to Google Drive: {done} of {total}', { done: formatSize(moving.done), total: formatSize(moving.total) })}
                      {/if}
                    </p>
                    {#if !unlinking}
                      <button type="button" class="btn btn-ghost h-7 px-2 text-xs" onclick={stopMoving}>{t('Stop')}</button>
                    {/if}
                  </div>
                  <div class="progress"><div class="bg-place-google" style:width="{pct(moving.done, moving.total)}%"></div></div>
                </div>
              {:else if !a.broken && (a.only_there > 0 || (a.mode === 'extra' && info.server_used > 0))}
                <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
                  {#if a.only_there > 0}
                    <p class="text-xs text-fg-muted tabular-nums">{t('{size} of your files are kept only in this Drive.', { size: formatSize(a.only_there) })}</p>
                    <button type="button" class="btn btn-ghost h-7 px-2 text-xs" disabled={moving !== null} onclick={() => move(a, 'server')}>
                      <Icon name="arrow-down-to-line" class="size-3.5" />{t('Move to this server')}
                    </button>
                  {/if}
                  {#if a.mode === 'extra' && info.server_used > 0}
                    <button type="button" class="btn btn-ghost h-7 px-2 text-xs" disabled={moving !== null} onclick={() => move(a, 'linked')}>
                      <Icon name="arrow-up-from-line" class="size-3.5" />{t('Move everything from this server here')}
                    </button>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}

      {#if hasExtra}
        <fieldset class="grid gap-2">
          <legend class="mb-1 text-[13px] font-medium">{t('New files go to')}</legend>
          <label class="flex items-center gap-2 text-sm">
            <input type="radio" name="storage-prefer" value="server" checked={info.prefer === 'server'} onchange={() => changePrefer('server')} />
            {t("This server first, then Google Drive when it's full")}
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input type="radio" name="storage-prefer" value="linked" checked={info.prefer === 'linked'} onchange={() => changePrefer('linked')} />
            {t('Google Drive first, then this server')}
          </label>
        </fieldset>
      {/if}

      {#if !info.google}
        <p class="text-[13px] text-fg-muted">{t("This server can't link Google Drive yet. Whoever runs it can turn it on.")}</p>
      {:else if inApp}
        <p class="text-[13px] text-fg-muted">{t('Link a Google Drive from thencloud in a web browser; it then works here too.')}</p>
      {/if}
    {/if}
  </div>
  {#if info?.google && !inApp}
    <div class="flex flex-wrap items-center justify-end gap-3 border-t border-line bg-subtle px-6 py-3">
      <p class="mr-auto hidden text-xs text-fg-muted sm:block">{t('Google asks you which account to use.')}</p>
      <button type="button" class="btn btn-secondary" disabled={linking !== null} onclick={() => link('extra')}>
        {#if linking === 'extra'}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="plus" />{/if}
        {t('Add Google Drive as extra space')}
      </button>
      <button type="button" class="btn btn-primary" disabled={linking !== null} onclick={() => link('mirror')}>
        {#if linking === 'mirror'}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="cloud" />{/if}
        {t('Add Google Drive as a mirror')}
      </button>
    </div>
  {/if}
</section>

{#if unlinking}
  <ConfirmDialog
    title={t('Unlink Google Drive?')}
    description={unlinking.only_there > 0
      ? t('{size} of your files are kept only in this Drive. They are moved back to this server first, then thencloud loses access.', { size: formatSize(unlinking.only_there) })
      : unlinking.mode === 'mirror'
        ? t('The copies in it are deleted and thencloud loses access. Your files here stay as they are.')
        : t('thencloud loses access to it. Your files here stay as they are.')}
    confirmLabel={unlinking.only_there > 0 ? t('Move back and unlink') : t('Unlink')}
    danger
    disabled={busy}
    onconfirm={unlink}
    onclose={() => {
      if (!busy) unlinking = null;
    }} />
{/if}
