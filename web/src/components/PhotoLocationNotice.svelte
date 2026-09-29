<script>
  // Before sharing a photo: say so if it records where it was taken, and
  // offer to remove that. Removing uploads a clean copy as a new version and
  // deletes the older versions, which people it's shared with could open.
  import { t } from '../lib/i18n.svelte.js';
  import Icon from './Icon.svelte';
  import { fetchEntry, upload, versions, deleteVersion } from '../lib/cloud.svelte.js';
  import { photoInfo, maybePhoto } from '../lib/exif.js';
  import { toast, errorMessage } from '../lib/ui.svelte.js';

  let { entry } = $props();

  const MAX = 64 * 1024 * 1024;
  let found = $state(null); // { bytes, info } when the photo has a location
  let busy = $state(false);
  let error = $state('');
  let file = $derived(entry.node.kind === 'file' ? entry : null);

  $effect(() => {
    const e = file;
    found = null;
    if (!e || !maybePhoto({ name: e.meta.name, type: e.meta.mime || '' }) || e.meta.size > MAX) return;
    let live = true;
    fetchEntry(e, () => {})
      .then(async ({ blob }) => {
        const bytes = new Uint8Array(await blob.arrayBuffer());
        const info = photoInfo(bytes);
        if (live && info?.gps) found = { info };
      })
      .catch(() => {});
    return () => (live = false);
  });

  async function remove() {
    busy = true;
    error = '';
    try {
      const clean = new File([found.info.strip()], entry.meta.name, { type: entry.meta.mime || '' });
      await upload(clean, { existing: entry });
      for (const v of await versions(entry)) if (!v.current) await deleteVersion(entry, v);
      found = null;
      toast(t('Location removed. Older versions were deleted too.'), { kind: 'success' });
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if found}
  <div class="grid gap-2 rounded-md border border-line bg-subtle p-3 text-[13px]" role="status">
    <p class="flex items-center gap-2 font-medium"><Icon name="circle-alert" class="size-4 shrink-0" />{t('This photo records where it was taken')}</p>
    <p class="text-fg-muted">{found.info.camera ? t('Anyone you share it with could read the location and the camera it was taken on.') : t('Anyone you share it with could read the location.')}</p>
    {#if error}<p class="text-danger">{error}</p>{/if}
    <div>
      <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" disabled={busy} onclick={remove}>
        {#if busy}<Icon name="loader-circle" class="spinner" />{/if}{t('Remove location')}
      </button>
    </div>
  </div>
{/if}
