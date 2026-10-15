<script>
  import { onMount, untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { versions, downloadVersion, restoreVersion, deleteVersion, pinVersion, session } from '../../lib/cloud.svelte.js';
  import { loadPlaces, versionName, setVersionName, MAX_VERSION_NAME } from '../../lib/places.svelte.js';
  import { toast, toastError, trackTransfer, errorMessage } from '../../lib/ui.svelte.js';
  import { formatSize, formatWhen, fullDate } from '../../lib/format.js';
  import { fly, slide } from '../../lib/motion.js';
  import { previewKind } from '../../lib/preview.js';
  import DiffDialog from './DiffDialog.svelte';
  import { t } from '../../lib/i18n.svelte.js';

  let { entry, canWrite, onchanged, onclose } = $props();

  // Exact from the version's encrypted metadata; the server's is to the hour.
  const versionAt = (v) => v.meta?.changed ?? v.created_at * 1000;

  // Local copy, kept current after a restore so revision checks pass.
  let file = $state(untrack(() => entry));
  let list = $state(null);
  let busy = $state(null); // version id with an action in flight
  // The last one or two versions picked to compare, oldest click dropped
  // once a third comes in; two picked versions opens DiffDialog.
  let picked = $state([]);
  const comparing = $derived.by(() => {
    if (picked.length !== 2) return null;
    const [a, b] = picked.map((id) => list.find((v) => v.id === id));
    return versionAt(a) <= versionAt(b) ? { before: a, after: b } : { before: b, after: a };
  });
  function pick(v) {
    if (picked.includes(v.id)) picked = picked.filter((id) => id !== v.id);
    else picked = picked.length < 2 ? [...picked, v.id] : [picked[1], v.id];
  }
  // Text, Markdown, CSV and code can show what each version changed.
  const textual = $derived(['text', 'markdown'].includes(previewKind(file.meta)?.kind));

  async function load() {
    picked = [];
    try {
      list = await versions(file);
    } catch (e) {
      toastError(e);
      list = [];
    }
  }

  onMount(() => {
    load();
    loadPlaces().catch(() => {});
  });

  // Pinning asks for an optional name first; unpinning doesn't ask.
  let naming = $state(null); // the version being pinned
  let pinName = $state('');

  async function setPinned(v, pinned, name = '') {
    busy = v.id;
    try {
      await pinVersion(file, v, pinned);
      list = list.map((x) => (x.id === v.id ? { ...x, pinned } : x));
      // The name is only ever in your encrypted app data.
      if (pinned ? name.trim() : versionName(v.id)) await setVersionName(v.id, pinned ? name : '');
    } catch (e) {
      toastError(e);
    } finally {
      busy = null;
    }
  }

  function startPin(v) {
    naming = v;
    pinName = '';
  }

  async function confirmPin() {
    const v = naming;
    naming = null;
    await setPinned(v, true, pinName);
  }

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
  description={`${t('{name}. Up to {count} versions are kept, and the oldest go first when you run out of space.', { name: entry.meta.name, count: session.me.max_versions })} ${t('Pinned versions are always kept.')}`}
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
      {#each list as v, i (v.id)}
        <li class="flex items-center gap-3 px-3 py-2.5" out:slide>
          <div class="min-w-0 flex-1">
            <p class="flex min-w-0 items-center gap-2 text-sm">
              {#if v.pinned && versionName(v.id)}
                <span class="truncate font-medium">{versionName(v.id)}</span>
                <span class="shrink-0 text-xs text-fg-muted" title={fullDate(versionAt(v))}>{formatWhen(versionAt(v))}</span>
              {:else}
                <span class="font-medium" title={fullDate(versionAt(v))}>{formatWhen(versionAt(v))}</span>
              {/if}
              {#if v.current}<span class="badge badge-accent shrink-0">{t('Current')}</span>{/if}
              {#if v.pinned}<span class="badge shrink-0"><Icon name="pin" />{t('Pinned')}</span>{/if}
            </p>
            <p class="mt-0.5 truncate text-xs text-fg-muted">
              {v.meta ? formatSize(v.meta.size) : t("Can't read this version's details")}{v.created_by ? ` · ${t('uploaded by {name}', { name: v.created_by })}` : ''}
            </p>
          </div>
          {#if textual && v.meta}
            <button
              type="button"
              class="btn btn-ghost btn-icon {picked.includes(v.id) ? 'text-accent-text' : ''}"
              aria-label={t('Pick to compare with another version')}
              aria-pressed={picked.includes(v.id)}
              title={t('Compare')}
              onclick={() => pick(v)}>
              <Icon name="file-diff" />
            </button>
          {/if}
          <button type="button" class="btn btn-ghost btn-icon" aria-label={t('Download this version')} title={t('Download')} disabled={!v.meta} onclick={() => download(v)}>
            <Icon name="download" />
          </button>
          {#if canWrite}
            <button
              type="button"
              class="btn btn-ghost btn-icon {v.pinned ? 'text-accent-text' : ''}"
              aria-label={v.pinned ? t('Unpin this version') : t('Pin this version')}
              aria-pressed={!!v.pinned}
              title={v.pinned ? t('Unpin') : t('Pin')}
              disabled={busy !== null}
              onclick={() => (v.pinned ? setPinned(v, false) : startPin(v))}>
              <Icon name={v.pinned ? 'pin-off' : 'pin'} />
            </button>
          {/if}
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
    {:else if textual && picked.length === 1}
      <p class="hint">{t('Pick one more version to compare it with.')}</p>
    {:else if textual}
      <p class="hint">{t('Pick two versions to see what changed between them.')}</p>
    {/if}
  {/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Done')}</button>
  {/snippet}
</Modal>

{#if comparing}
  <DiffDialog entry={file} before={comparing.before} after={comparing.after} at={versionAt} onclose={() => (picked = [])} />
{/if}

{#if naming}
  <Modal
    title={t('Pin this version')}
    description={t('A pinned version is kept however many newer ones there are, and isn\'t removed to make space. It still counts toward your storage.')}
    onclose={() => (naming = null)}
    onsubmit={confirmPin}
    class="max-w-sm">
    <div class="field">
      <label class="label" for="pin-name">{t('Name (optional)')}</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input id="pin-name" class="input" bind:value={pinName} maxlength={MAX_VERSION_NAME} placeholder={t('Final draft')} autocomplete="off" autofocus />
      <p class="hint">{t('Only you see the name. It\'s kept encrypted with your other library data.')}</p>
    </div>
    {#snippet footer()}
      <button type="button" class="btn btn-secondary" onclick={() => (naming = null)}>{t('Cancel')}</button>
      <button class="btn btn-primary"><Icon name="pin" />{t('Pin')}</button>
    {/snippet}
  </Modal>
{/if}
