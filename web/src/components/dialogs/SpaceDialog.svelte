<script>
  // What takes space under My files (lib/space.js): a treemap of the
  // current folder's contents by folder, or of everything below it by file
  // type. Sizes are decrypted from the metadata here; nothing is
  // downloaded.
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import FileIcon from '../FileIcon.svelte';
  import FolderIcon from '../FolderIcon.svelte';
  import { session, resolvePath } from '../../lib/cloud.svelte.js';
  import { measureTree, byKind, KINDS } from '../../lib/space.js';
  import { squarify, topItems } from '../../lib/treemap.js';
  import { formatSize } from '../../lib/format.js';
  import { formatNumber } from '../../lib/locale.svelte.js';
  import { errorMessage } from '../../lib/ui.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';

  let { onclose } = $props();

  // At most this many tiles at once; the rest of a folder becomes one tile.
  const MAX_TILES = 80;

  let status = $state('scanning'); // scanning | ready | error
  let progress = $state({ files: 0, folders: 0 });
  let message = $state('');
  let root = $state.raw(null);
  let path = $state.raw([]); // folders from root down to the one shown
  let mode = $state('folder'); // folder | type
  let kind = $state(null); // in "By type", the kind opened
  let width = $state(0);
  let height = $state(0);
  let ctl = null;

  onMount(() => {
    run();
    return () => ctl?.abort();
  });

  async function run() {
    ctl = new AbortController();
    try {
      const { items } = await resolvePath(session.me.keys.root_node_id);
      const tree = await measureTree(items[0], { signal: ctl.signal, onProgress: (p) => (progress = p) });
      if (ctl.signal.aborted) return;
      root = tree;
      path = [tree];
      status = 'ready';
    } catch (e) {
      message = errorMessage(e);
      status = 'error';
    }
  }

  const kindLabels = $derived({
    image: t('Images'),
    video: t('Videos'),
    audio: t('Audio'),
    document: t('Documents'),
    archive: t('Archives'),
    text: t('Text and code'),
    other: t('Other files'),
  });

  const current = $derived(path.at(-1));
  const kinds = $derived(current && mode === 'type' ? byKind(current) : []);

  const fold = (rest) => ({ rest: true, id: 'rest', count: rest.length, size: rest.reduce((n, it) => n + it.size, 0) });

  // What the treemap shows: `{ id, name, size, folder?, kind?, entry?, group?, rest?, count? }`.
  const items = $derived.by(() => {
    if (!current) return [];
    if (mode === 'folder') {
      return topItems(
        current.children.map((c) => (c.children ? { ...c, folder: c } : c)),
        MAX_TILES,
        (it) => it.size,
        fold,
      );
    }
    if (!kind) return kinds.map((g) => ({ id: g.kind, name: kindLabels[g.kind], size: g.size, kind: g.kind, group: g }));
    const g = kinds.find((g) => g.kind === kind);
    return g ? topItems(g.files, MAX_TILES, (it) => it.size, fold) : [];
  });

  const total = $derived(mode === 'type' && kind ? (kinds.find((g) => g.kind === kind)?.size ?? 0) : (current?.size ?? 0));
  const tiles = $derived(width > 0 && height > 0 ? squarify(items, { x: 0, y: 0, w: width, h: height }, (it) => it.size) : []);
  const largest = $derived(items.filter((it) => !it.rest && it.size > 0).slice(0, 8));

  function open(it) {
    if (it.folder) {
      path = [...path, it.folder];
      kind = null;
    } else if (it.group) {
      kind = it.kind;
    }
  }

  function goTo(i) {
    path = path.slice(0, i + 1);
    kind = null;
  }

  function setMode(m) {
    mode = m;
    kind = null;
  }

  const pct = (n) => (total ? `${formatNumber((n / total) * 100, { maximumFractionDigits: n / total < 0.1 ? 1 : 0 })}%` : '');
  const opens = (it) => !!(it.folder || it.group);
</script>

<Modal
  title={t('What takes space')}
  description={t('Sizes are read from your files\' encrypted details here, in the browser. Nothing is downloaded, and the server learns nothing new.')}
  {onclose}
  class="max-w-4xl">
  {#if status === 'scanning'}
    <div class="grid gap-2">
      <div class="progress"></div>
      <p class="text-[13px] text-fg-muted tabular-nums" role="status">
        {t('Looking through My files: {count} items so far', { count: progress.files + progress.folders })}
      </p>
    </div>
  {:else if status === 'error'}
    <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="circle-alert" class="size-4 shrink-0 text-danger" />{message}</p>
  {:else}
    <div class="flex flex-wrap items-center justify-between gap-2">
      <nav aria-label={t('Folder')} class="flex min-w-0 flex-wrap items-center gap-0.5 text-[13px]">
        {#each path as f, i (f.id)}
          {#if i}<Icon name="chevron-right" class="size-3.5 shrink-0 text-fg-faint" />{/if}
          {#if i < path.length - 1 || (mode === 'type' && kind)}
            <button type="button" class="max-w-48 cursor-pointer truncate rounded px-1 text-fg-muted hover:bg-muted hover:text-fg" onclick={() => goTo(i)}>{i ? f.name : t('My files')}</button>
          {:else}
            <span class="max-w-48 truncate px-1 font-medium" aria-current="page">{i ? f.name : t('My files')}</span>
          {/if}
        {/each}
        {#if mode === 'type' && kind}
          <Icon name="chevron-right" class="size-3.5 shrink-0 text-fg-faint" />
          <span class="px-1 font-medium" aria-current="page">{kindLabels[kind]}</span>
        {/if}
      </nav>
      <div class="flex w-fit rounded-md border border-line p-0.5" role="radiogroup" aria-label={t('Group')}>
        {#each [['folder', t('By folder')], ['type', t('By type')]] as [value, label] (value)}
          <button type="button" role="radio" aria-checked={mode === value} class="h-7 cursor-pointer rounded px-3 text-[13px] {mode === value ? 'bg-muted font-medium text-fg' : 'text-fg-muted hover:text-fg'}" onclick={() => setMode(value)}>{label}</button>
        {/each}
      </div>
    </div>

    <p class="text-[13px] text-fg-muted tabular-nums">
      {t('{size} in {count} files', { size: formatSize(total), count: mode === 'type' && kind ? (kinds.find((g) => g.kind === kind)?.files.length ?? 0) : current.files })}
    </p>

    <div class="relative h-80 overflow-hidden rounded-md border border-line bg-bg sm:h-96" bind:clientWidth={width} bind:clientHeight={height}>
      {#if !tiles.length}
        <p class="grid h-full place-items-center text-[13px] text-fg-muted">{t('Nothing here takes any space.')}</p>
      {/if}
      {#each tiles as tile (tile.item.id)}
        {@const it = tile.item}
        {@const roomy = tile.w >= 64 && tile.h >= 38}
        {@const label = it.rest ? t('{count} smaller items', { count: it.count }) : it.name}
        {#snippet inner()}
          {#if it.kind && !it.rest}<span class="space-strip" style:background-color="var(--kind-{it.kind})"></span>{/if}
          {#if roomy}
            <span class="flex min-w-0 items-center gap-1.5">
              {#if it.folder}<FolderIcon name={it.name} class="size-3.5 shrink-0" />
              {:else if it.entry}<FileIcon meta={it.entry.meta} class="size-3.5 shrink-0" />{/if}
              <span class="truncate text-xs font-medium">{label}</span>
            </span>
            <span class="text-[11px] text-fg-muted tabular-nums">{formatSize(it.size)}</span>
          {/if}
        {/snippet}
        {#if opens(it)}
          <button
            type="button"
            class="space-tile cursor-pointer"
            style:left="{tile.x}px"
            style:top="{tile.y}px"
            style:width="{tile.w}px"
            style:height="{tile.h}px"
            style:--tile-kind={it.kind && !it.rest ? `var(--kind-${it.kind})` : null}
            title="{label}, {formatSize(it.size)}"
            aria-label="{label}, {formatSize(it.size)}"
            onclick={() => open(it)}>{@render inner()}</button>
        {:else}
          <div
            class="space-tile {it.rest ? 'space-rest' : ''}"
            style:left="{tile.x}px"
            style:top="{tile.y}px"
            style:width="{tile.w}px"
            style:height="{tile.h}px"
            style:--tile-kind={it.kind && !it.rest ? `var(--kind-${it.kind})` : null}
            title="{label}, {formatSize(it.size)}">{@render inner()}</div>
        {/if}
      {/each}
    </div>

    {#if mode === 'type' && !kind}
      <ul class="flex flex-wrap gap-x-4 gap-y-1 text-xs text-fg-muted" aria-label={t('File types')}>
        {#each KINDS.filter((k) => kinds.some((g) => g.kind === k)) as k (k)}
          <li class="flex items-center gap-1.5"><span class="space-swatch" style:background-color="var(--kind-{k})"></span>{kindLabels[k]}</li>
        {/each}
      </ul>
    {/if}

    {#if largest.length}
      <div class="grid gap-1">
        <h3 class="text-xs font-medium text-fg-muted">{t('Largest here')}</h3>
        <ul class="divide-y divide-line rounded-md border border-line text-[13px]">
          {#each largest as it (it.id)}
            <li class="flex items-center gap-2 px-3 py-1.5">
              {#if it.group}<span class="space-swatch shrink-0" style:background-color="var(--kind-{it.kind})"></span>
              {:else if it.folder}<FolderIcon name={it.name} class="size-4 shrink-0" />
              {:else}<FileIcon meta={it.entry.meta} />{/if}
              {#if opens(it)}
                <button type="button" class="min-w-0 flex-1 cursor-pointer truncate text-left hover:underline" onclick={() => open(it)}>{it.name}</button>
              {:else}
                <span class="min-w-0 flex-1 truncate">{it.name}</span>
              {/if}
              <span class="shrink-0 text-xs text-fg-muted tabular-nums">{pct(it.size)}</span>
              <span class="w-20 shrink-0 text-right tabular-nums">{formatSize(it.size)}</span>
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <p class="text-xs text-fg-muted">
      {t('Only the current version of each file under My files. Older versions and the trash count toward your storage too, so together with encryption they make up the rest of the {used} in use.', { used: formatSize(session.me.used_bytes) })}
    </p>
  {/if}
  {#snippet footer()}
    {#if status === 'scanning'}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Stop')}</button>
    {:else}
      <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Close')}</button>
    {/if}
  {/snippet}
</Modal>

