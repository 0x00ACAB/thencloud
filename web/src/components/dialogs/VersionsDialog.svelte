<script>
  import { onMount, untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { versions, downloadVersion, restoreVersion, deleteVersion, session } from '../../lib/cloud.svelte.js';
  import { toast, toastError, trackTransfer, errorMessage } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, fullDate } from '../../lib/format.js';
  import { fly, slide } from '../../lib/motion.js';
  import { t } from '../../lib/i18n.svelte.js';

  let { entry, canWrite, onchanged, onclose } = $props();

  // Exact from the version's encrypted metadata; the server's is to the hour.
  const versionAt = (v) => v.meta?.changed ?? v.created_at * 1000;

  // Local copy, kept current after a restore so revision checks pass.
  let file = $state(untrack(() => entry));
  let list = $state(null);
  let busy = $state(null); // version id with an action in flight

  async function load() {
    try {
      list = await versions(file);
    } catch (e) {
      toastError(e);
      list = [];
    }
  }

  onMount(load);

  async function download(v) {
    const job = trackTransfer('download', v.meta?.name ?? file.meta.name, v.meta?.size ?? v.size);
    try {
      await downloadVersion(file, v, (p) => (job.progress = p));
      job.status = 'done';
    } catch (e) {
      job.status = 'error';
      job.error = errorMessage(e);
    }
  }

  async function restore(v) {
    busy = v.id;
    try {
      const node = await restoreVersion(file, v);
      file = { ...file, node, meta: { ...file.meta, size: v.meta.size, mtime: v.meta.mtime } };
      toast(t('Restored the version from {when}', { when: formatWhen(versionAt(v)) }), { kind: 'success' });
      onchanged?.();
      await load();
    } catch (e) {
      toastError(e);
    } finally {
      busy = null;
    }
  }

  async function remove(v) {
    busy = v.id;
    try {
      await deleteVersion(file, v);
      list = list.filter((x) => x.id !== v.id);
      onchanged?.();
    } catch (e) {
      toastError(e);
    } finally {
      busy = null;
    }
  }
</script>

<Modal
  title={t('Version history')}
  description={t('{name}. Up to {count} versions are kept, and the oldest go first when you run out of space.', { name: entry.meta.name, count: session.me.max_versions })}
  {onclose}
  class="max-w-lg">
  {#if list === null}
    <ul class="divide-y divide-line rounded-md border border-line" aria-hidden="true">
      {#each [0, 1, 2] as i (i)}
        <li class="flex items-center gap-3 px-3 py-3">
          <div class="grid flex-1 gap-1.5"><div class="skeleton h-3.5 w-32"></div><div class="skeleton h-3 w-48"></div></div>
        </li>
      {/each}
    </ul>
  {:else}
    <ul class="max-h-80 divide-y divide-line overflow-y-auto rounded-md border border-line" in:fly>
      {#each list as v (v.id)}
        <li class="flex items-center gap-3 px-3 py-2.5" out:slide>
          <div class="min-w-0 flex-1">
            <p class="flex items-center gap-2 text-sm">
              <span class="font-medium" title={fullDate(versionAt(v))}>{formatWhen(versionAt(v))}</span>
              {#if v.current}<span class="badge badge-accent">{t('Current')}</span>{/if}
            </p>
            <p class="mt-0.5 truncate text-xs text-fg-muted">
              {v.meta ? formatSize(v.meta.size) : t("Can't read this version's details")}{v.created_by ? ` · ${t('uploaded by {name}', { name: v.created_by })}` : ''}
            </p>
          </div>
          <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Download this version')} title={t('Download')} disabled={!v.meta} onclick={() => download(v)}>
            <Icon name="download" />
          </button>
          {#if canWrite && !v.current}
            <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" disabled={busy !== null || !v.meta} onclick={() => restore(v)}>
              {#if busy === v.id}<Icon name="loader-circle" class="spinner" />{/if}
              {t('Restore')}
            </button>
            <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Delete this version')} title={t('Delete')} disabled={busy !== null} onclick={() => remove(v)}>
              <Icon name="trash-2" />
            </button>
          {/if}
        </li>
      {/each}
    </ul>
    {#if list.length === 1}
      <p class="hint">{t('This is the only version so far. Uploading a new version keeps this one here.')}</p>
    {/if}
  {/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Done')}</button>
  {/snippet}
</Modal>
