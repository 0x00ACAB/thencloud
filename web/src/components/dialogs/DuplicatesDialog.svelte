<script>
  // Finds files under My files with identical content (lib/duplicates.js)
  // and offers to move all but one copy of each to the trash. Nothing
  // changes until "Move to trash" is pressed.
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import Time from '../Time.svelte';
  import { session, resolvePath, trash } from '../../lib/cloud.svelte.js';
  import { findDuplicates } from '../../lib/duplicates.js';
  import { formatSize } from '../../lib/format.js';
  import { toast, toastError, errorMessage } from '../../lib/ui.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';

  let { onclose } = $props();

  let status = $state('scanning'); // scanning | ready | error | removing
  let progress = $state({ done: 0, total: 0 });
  let groups = $state([]); // [{ items: [entry, ...], keep: node id }]
  let message = $state('');
  let ctl = null;

  onMount(() => {
    run();
    return () => ctl?.abort();
  });

  async function run() {
    status = 'scanning';
    progress = { done: 0, total: 0 };
    ctl = new AbortController();
    try {
      const { items } = await resolvePath(session.me.keys.root_node_id);
      const found = await findDuplicates(items[0], {
        signal: ctl.signal,
        onProgress: (done, total) => (progress = { done, total }),
      });
      if (ctl.signal.aborted) return;
      groups = found.map((items) => ({ items, keep: items[0].node.id }));
      status = 'ready';
    } catch (e) {
      message = errorMessage(e);
      status = 'error';
    }
  }

  const toTrash = $derived(groups.flatMap((g) => g.items.filter((it) => it.node.id !== g.keep)));
  const reclaimed = $derived(toTrash.reduce((n, it) => n + it.meta.size, 0));

  async function cleanUp() {
    status = 'removing';
    let done = 0;
    for (const it of toTrash) {
      try {
        await trash(it);
        done++;
      } catch (e) {
        toastError(e);
      }
    }
    if (done) toast(t('Moved {count} duplicates to the trash', { count: done }), { kind: 'success' });
    onclose();
  }
</script>

<Modal
  title={t('Duplicate files')}
  description={t('Files under My files with exactly the same content. Same-sized files are read and compared here; nothing is sent anywhere.')}
  onclose={() => (status === 'removing' ? null : onclose())}
  class="max-w-2xl">
  {#if status === 'scanning'}
    <div class="grid gap-2">
      <div class="progress">{#if progress.total}<div style:width="{(progress.done / progress.total) * 100}%"></div>{/if}</div>
      <p class="text-[13px] text-fg-muted" role="status">
        {progress.total ? t('Comparing {done} of {total} same-sized files…', { done: progress.done, total: progress.total }) : t('Looking through My files…')}
      </p>
    </div>
  {:else if status === 'error'}
    <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="circle-alert" class="size-4 shrink-0 text-danger" />{message}</p>
  {:else if !groups.length}
    <p class="text-[13px] text-fg-muted">{t('No duplicate files found.')}</p>
  {:else}
    <p class="text-[13px] text-fg-muted">{t("Keeping the oldest copy of each by default; pick another if you'd rather. {size} would be freed.", { size: formatSize(reclaimed) })}</p>
    <ul class="max-h-96 divide-y divide-line overflow-y-auto rounded-md border border-line">
      {#each groups as g, gi (g.items[0].node.id)}
        <li class="grid gap-1.5 px-3 py-2.5">
          <p class="text-xs text-fg-muted">{t('{count} copies, {size} each', { count: g.items.length, size: formatSize(g.items[0].meta.size) })}</p>
          {#each g.items as it (it.node.id)}
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" name="keep-{gi}" class="shrink-0" checked={g.keep === it.node.id} onchange={() => (g.keep = it.node.id)} />
              <FileIcon meta={it.meta} />
              <span class="min-w-0 flex-1 truncate">{it.location.join(' / ')} / <span class="font-medium">{it.meta.name}</span></span>
              <span class="shrink-0 text-xs text-fg-muted"><Time ms={it.meta.mtime} /></span>
            </label>
          {/each}
        </li>
      {/each}
    </ul>
  {/if}
  {#snippet footer()}
    {#if status === 'scanning'}
      <button type="button" class="btn btn-secondary" onclick={() => ctl?.abort()}>{t('Stop')}</button>
    {:else if status === 'removing'}
      <button type="button" class="btn btn-primary" disabled>
        <Icon name="loader-circle" class="spinner" />{t('Moving to the trash…')}
      </button>
    {:else if status === 'ready' && groups.length}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
      <button type="button" class="btn btn-primary" disabled={!toTrash.length} onclick={cleanUp}>
        {t('Move {count} to the trash', { count: toTrash.length })}
      </button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Close')}</button>
    {/if}
  {/snippet}
</Modal>
