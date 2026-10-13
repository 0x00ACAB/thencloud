<script>
  // "Share to thencloud" from another app's share sheet (Android only): the
  // plugin already has the file(s) queued (shareImport.svelte.js); this asks
  // where to put them, then hands them to the normal upload pipeline, same
  // as any file picked or dropped in Files.
  import { t } from '../../lib/i18n.svelte.js';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderPickDialog from './FolderPickDialog.svelte';
  import { shareImport } from '../../lib/shareImport.svelte.js';
  import { importShare, cancelShare } from '../../lib/native.js';
  import { session, resolvePath, keyOf } from '../../lib/cloud.svelte.js';
  import { checkPhotos, runUploadJobs } from '../../lib/uploadJobs.js';
  import { toastError } from '../../lib/ui.svelte.js';
  import { formatSize } from '../../lib/format.js';

  let { onclose } = $props();

  let picking = $state(null); // My files root entry, while the folder dialog is open
  let preparing = $state(false);
  let photoDialog = $state(null);

  const pickDescription = $derived(
    shareImport.items.length > 1
      ? t('Choose where to save {count} shared files.', { count: shareImport.items.length })
      : t('Choose where to save {name}.', { name: shareImport.items[0]?.name ?? '' }),
  );

  function declineAll() {
    for (const item of shareImport.items) cancelShare(item.id);
    shareImport.items = [];
    onclose();
  }

  async function choosePlace() {
    picking = (await resolvePath(session.me.keys.root_node_id)).items[0];
  }

  function askPhotoDetails(located) {
    return new Promise((resolve) => (photoDialog = { located, resolve }));
  }

  async function importInto(parentId) {
    picking = null;
    preparing = true;
    const items = shareImport.items;
    shareImport.items = [];
    const parentKey = await keyOf(parentId);
    const jobs = [];
    for (const item of items) {
      try {
        jobs.push({ file: await importShare(item), dest: { parentId, parentKey }, label: item.name });
      } catch (e) {
        toastError(e);
      }
    }
    preparing = false;
    // The ask-about-location step (if any) still shows here; once it's
    // answered the upload itself goes on in the transfer tray, so this
    // dialog closes without waiting for it to finish.
    if (await checkPhotos(jobs, askPhotoDetails)) runUploadJobs(jobs);
    onclose();
  }
</script>

{#if preparing}
  <Modal title={t('Getting the shared files ready')} onclose={() => {}} class="max-w-sm">
    <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="loader-circle" class="spinner" />{t('This only takes a moment.')}</p>
  </Modal>
{:else if picking}
  <FolderPickDialog root={picking} title={t('Save to')} description={pickDescription} onpick={importInto} onclose={() => (picking = null)} />
{:else if photoDialog}
  <Modal
    title={t('{count} photos have a location', { count: photoDialog.located })}
    description={t('Photos from phones and cameras often record where they were taken, and on what. Anyone you share them with could read it.')}
    onclose={() => photoDialog.resolve('cancel')}
    onsubmit={() => photoDialog.resolve('remove')}>
    <p class="text-[13px] text-fg-muted">{t("Removing it also drops the camera details. The pictures themselves don't change. You can choose what happens every time in Settings.")}</p>
    {#snippet footer()}
      <button type="button" class="btn btn-secondary mr-auto" onclick={() => photoDialog.resolve('cancel')}>{t('Cancel')}</button>
      <button type="button" class="btn btn-secondary" onclick={() => photoDialog.resolve('keep')}>{t('Keep it')}</button>
      <button class="btn btn-primary">{t('Remove location')}</button>
    {/snippet}
  </Modal>
{:else}
  <Modal
    title={t('Shared with thencloud')}
    description={t('Choose where to save what was shared from another app.')}
    onclose={declineAll}
    onsubmit={choosePlace}
    class="max-w-md">
    <ul class="max-h-80 divide-y divide-line overflow-y-auto rounded-md border border-line">
      {#each shareImport.items as item (item.id)}
        <li class="flex items-center gap-3 px-3 py-2.5">
          <FileIcon meta={{ name: item.name, mime: item.mime }} />
          <div class="min-w-0 flex-1">
            <p class="truncate text-sm font-medium">{item.name}</p>
            {#if item.size > 0}<p class="truncate text-xs text-fg-muted">{formatSize(item.size)}</p>{/if}
          </div>
        </li>
      {/each}
    </ul>
    {#snippet footer()}
      <button type="button" class="btn btn-secondary mr-auto" onclick={declineAll}>{t('Decline')}</button>
      <button class="btn btn-primary">{t('Choose a folder')}</button>
    {/snippet}
  </Modal>
{/if}
