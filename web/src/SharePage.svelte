<script>
  // Public link viewer: /s/<token>#<key>
  //
  // Only <token> is sent to the server. The key after '#' is read from
  // location.hash here and used only for decryption in this page; it never
  // appears in any request URL, header or body. For a file drop (upload-only
  // link) the fragment holds the owner's public key instead, and files are
  // sealed to it here before upload. A link with a password holds `p.` and
  // a secret: the folder or file key comes from the server wrapped under the
  // secret and the password together, and the password itself is never sent
  // (only an auth key derived from it).
  import { request, allChildren } from './lib/api.js';
  import { tc, b64, unb64, encryptMeta, decryptMeta, decryptChildren, fetchFile, openFile, encryptPiece, saveBlob, deriveLinkKeys } from './lib/crypto.js';
  import { streamsAvailable, streamDownload } from './lib/stream.js';
  import { formatSize, sortEntries } from './lib/format.js';
  import { errorMessage, trackTransfer } from './lib/ui.svelte.js';
  import { t } from './lib/i18n.svelte.js';
  import Icon from './components/Icon.svelte';
  import Time from './components/Time.svelte';
  import Sentence from './components/Sentence.svelte';
  import FileIcon from './components/FileIcon.svelte';
  import FolderIcon from './components/FolderIcon.svelte';
  import Toasts from './components/Toasts.svelte';
  import TransferTray from './components/TransferTray.svelte';
  import Preview from './components/Preview.svelte';
  import { previewKind } from './lib/preview.js';

  const token = decodeURIComponent(location.pathname.split('/').filter(Boolean)[1] || '');
  const base = `/api/public/${encodeURIComponent(token)}`;

  let phase = $state('loading'); // loading | password | ready | drop | error
  let error = $state({ title: '', detail: '' });
  let linkToken = null;
  let expiresAt = $state(null);
  let trail = $state([]); // [{ node, key, meta }]
  let rows = $state([]);
  let listing = $state(false);

  let password = $state('');
  let unlocking = $state(false);
  let unlockError = $state('');

  const here = $derived(trail[trail.length - 1]);
  const opts = () => (linkToken ? { headers: { 'X-Link-Token': linkToken } } : {});

  function fail(title, detail) {
    error = { title, detail };
    phase = 'error';
  }

  // With `p.`, the link's secret; the key is then unwrapped after unlocking.
  const withPassword = location.hash.startsWith('#p.');
  let linkKek = null;

  function readKey() {
    const fragment = location.hash.slice(withPassword ? 3 : 1);
    if (!token || !fragment) return null;
    try {
      const k = unb64(fragment);
      // A folder key or a link's secret, or for a file drop the owner's key
      // (32 bytes) or its X25519 half and the hash of its ML-KEM half (64).
      if (withPassword) return k.length === 32 ? k : null;
      return k.length === 32 || k.length === 64 ? k : null;
    } catch {
      return null;
    }
  }

  const rootKey = readKey();

  async function load() {
    if (!rootKey) {
      return fail(
        t('This link is incomplete'),
        t('The part after the # is missing or damaged. It holds the decryption key, so ask the sender for the full link.'),
      );
    }
    let info;
    try {
      info = await request('GET', base, opts());
    } catch (e) {
      if (e.code === 'password_required') return (phase = 'password');
      if (e.status === 404) return fail(t('This link has expired or was removed'), t('Ask the person who shared it for a new link.'));
      return fail(t("Couldn't open this link"), errorMessage(e));
    }
    expiresAt = info.expires_at;
    // A link with limited opens counted this visit; its token covers the rest.
    if (info.link_token) linkToken = info.link_token;
    if (info.upload_only) {
      if (rootKey.length === 64) {
        const pq = info.owner_pq_public_key ? unb64(info.owner_pq_public_key) : new Uint8Array();
        // The server supplies the ML-KEM key; the link says which one it must be.
        if (b64(tc.identity(rootKey.subarray(0, 32), pq)) !== b64(rootKey)) {
          return fail(
            t("This link doesn't match its owner's key"),
            t("The server gave a different key than the link names, so nothing can be sent safely. Don't upload anything; let the owner know."),
          );
        }
        dropKey = new Uint8Array([...rootKey.subarray(0, 32), ...pq]);
      } else {
        dropKey = rootKey;
      }
      owner = info.owner;
      dropFolder = info.folder_id;
      phase = 'drop';
      return;
    }
    let nodeKey = rootKey;
    if (withPassword) {
      // A server that doesn't ask for the password has nothing we can open.
      if (!linkKek || !info.enc_link_key) return (phase = 'password');
      try {
        nodeKey = tc.unwrap_link_key(linkKek, unb64(info.enc_link_key), info.node.id);
      } catch {
        return fail(t('The key in this link is wrong'), t('The password was accepted, but it and the link together do not open this. Make sure you copied the whole link.'));
      }
    }
    let meta;
    try {
      meta = decryptMeta(nodeKey, info.node);
    } catch {
      return fail(t('The key in this link is wrong'), t('The part after the # does not match. Make sure you copied the whole link.'));
    }
    trail = [{ node: info.node, key: nodeKey, meta }];
    phase = 'ready';
  }

  async function unlock(e) {
    e.preventDefault();
    unlocking = true;
    unlockError = '';
    try {
      const k = await deriveLinkKeys(rootKey, password);
      const r = await request('POST', `${base}/unlock`, { body: { auth: b64(k.auth) } });
      linkToken = r.link_token;
      linkKek = k.kek;
      password = '';
      await load();
    } catch (err) {
      unlockError = err?.code === 'invalid_credentials' ? t('That password is not right.') : errorMessage(err);
    } finally {
      unlocking = false;
    }
  }

  $effect(() => {
    if (!here || here.node.kind !== 'folder') return;
    const { node, key } = here;
    listing = true;
    allChildren((p) => request('GET', p, opts()), `${base}/nodes/${node.id}/children`)
      .then((nodes) => {
        if (here?.node.id === node.id) rows = sortEntries(decryptChildren(key, nodes));
      })
      .catch((e) => fail(t("Couldn't list this folder"), errorMessage(e)))
      .finally(() => (listing = false));
  });

  const getChunk = (entry) => (i) => request('GET', `${base}/nodes/${entry.node.id}/chunks/${i}`, opts());
  const fetchEntry = (entry, onProgress) => fetchFile(entry.node, entry.key, getChunk(entry), onProgress);
  const openEntry = (entry) => openFile(entry.node, entry.key, getChunk(entry));

  let preview = $state(null); // { entries, start }
  const files = $derived(rows.filter((r) => r.node.kind === 'file'));

  const listFolder = async (entry) => sortEntries(decryptChildren(entry.key, await allChildren((p) => request('GET', p, opts()), `${base}/nodes/${entry.node.id}/children`)));

  /** Everything in the folder being viewed, as one zip. */
  async function downloadAll() {
    const name = `${here.meta.name}.zip`;
    const job = trackTransfer('download', name, null);
    try {
      const { saveZip } = await import('./lib/zip.js');
      await saveZip(rows, name, { list: listFolder, open: openEntry, onProgress: (p) => (job.progress = p) });
      job.status = 'done';
    } catch (e) {
      t.status = 'error';
      t.error = errorMessage(e);
    }
  }

  async function downloadEntry(entry) {
    const job = trackTransfer('download', entry.meta.name, entry.meta.size);
    try {
      if (entry.meta.size > 16 * 1024 * 1024 && (await streamsAvailable())) {
        await streamDownload(openEntry(entry), (p) => (job.progress = p));
      } else {
        const { blob, meta } = await fetchEntry(entry, (p) => (job.progress = p));
        saveBlob(blob, meta.name);
      }
      t.status = 'done';
    } catch (e) {
      t.status = 'error';
      t.error = errorMessage(e);
    }
  }

  // --- File drop -----------------------------------------------------------

  let owner = $state('');
  let dropKey = null; // what dropped files' keys are sealed to
  let sent = $state([]); // { id, name, size, progress, status, error }
  let dragging = $state(false);
  let picker = $state();
  const fingerprintText = $derived(phase === 'drop' ? tc.fingerprint(rootKey) : '');

  /** Encrypt `file` under a fresh key sealed to the owner, and upload it. */
  async function dropFile(file) {
    const item = { id: tc.new_id(), name: file.name, size: file.size, progress: 0, status: 'active', error: '' };
    sent.push(item);
    const row = sent[sent.length - 1];
    const folderId = dropFolder;
    const nodeId = item.id;
    const nodeKey = tc.random_key();
    const versionId = tc.new_id();
    const contentKey = tc.random_key();
    const padded = tc.padded_size(file.size);
    const chunkCount = tc.chunk_count(padded);
    const meta = { name: file.name, mime: file.type || null, size: file.size, mtime: file.lastModified || Date.now() };
    let upId = null;
    try {
      const up = await request('POST', `${base}/uploads`, {
        ...opts(),
        body: {
          node_id: nodeId,
          parent_id: folderId,
          enc_key: b64(tc.seal_drop_key(dropKey, nodeKey, nodeId, folderId)),
          enc_metadata: encryptMeta(nodeKey, nodeId, meta),
          version_id: versionId,
          enc_content_key: b64(tc.wrap_content_key(nodeKey, contentKey, nodeId, versionId)),
          chunk_count: chunkCount,
        },
      });
      upId = up.upload_id;
      for (let i = 0; i < chunkCount; i++) {
        const enc = await encryptPiece(file, i, padded, contentKey, versionId, chunkCount);
        await request('PUT', `${base}/uploads/${up.upload_id}/chunks/${i}`, { ...opts(), raw: enc });
        row.progress = (i + 1) / chunkCount;
      }
      await request('POST', `${base}/uploads/${up.upload_id}/finish`, opts());
      row.status = 'done';
    } catch (e) {
      // Free the space it took straight away.
      if (upId) request('DELETE', `${base}/uploads/${upId}`, opts()).catch(() => {});
      row.status = 'error';
      row.error = e?.status === 507 ? t("There's no room left in this folder.") : errorMessage(e);
    }
  }

  // The folder id is bound into each sealed key, so the owner's client
  // takes the file into the folder it was meant for.
  let dropFolder = null;

  async function sendFiles(list) {
    for (const f of list) await dropFile(f);
  }

  function onDrop(e) {
    e.preventDefault();
    dragging = false;
    if (phase !== 'drop') return;
    const list = [...(e.dataTransfer?.files ?? [])];
    if (list.length) sendFiles(list);
  }

  function onDragOver(e) {
    if (phase !== 'drop' || !e.dataTransfer?.types.includes('Files')) return;
    e.preventDefault();
    dragging = true;
  }

  load();
</script>

<svelte:window ondragover={onDragOver} ondragleave={(e) => !e.relatedTarget && (dragging = false)} ondrop={onDrop} />

<div class="flex min-h-dvh flex-col">
  <header class="border-b border-line">
    <div class="mx-auto flex h-16 max-w-4xl items-center justify-between gap-4 px-4">
      <img src="/img/logo.webp" alt="thencloud" width="715" height="349" class="h-10 w-auto select-none" draggable="false" />
      <span class="badge"><Icon name="lock" />{t('End-to-end encrypted')}</span>
    </div>
  </header>

  <main class="mx-auto w-full max-w-4xl flex-1 px-4 py-10">
    {#if phase === 'loading'}
      <div class="grid h-64 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
    {:else if phase === 'error'}
      <div class="card mx-auto grid max-w-md gap-2 p-8 text-center">
        <div class="mx-auto mb-2 grid size-11 place-items-center rounded-lg border border-line bg-subtle"><Icon name="circle-alert" class="size-5 text-fg-muted" /></div>
        <h1 class="text-base font-semibold">{error.title}</h1>
        <p class="text-[13px] text-fg-muted">{error.detail}</p>
      </div>
    {:else if phase === 'password'}
      <form class="card mx-auto grid max-w-sm gap-4 p-6" onsubmit={unlock}>
        <div class="grid gap-1">
          <div class="mb-2 grid size-10 place-items-center rounded-lg border border-line bg-subtle"><Icon name="lock" class="size-5 text-fg-muted" /></div>
          <h1 class="text-base font-semibold">{t('This link is password protected')}</h1>
          <p class="text-[13px] text-fg-muted">{t('Enter the password the sender gave you.')}</p>
        </div>
        <input class="input" type="password" bind:value={password} aria-label={t('Password')} autocomplete="off" required />
        {#if unlockError}<p class="text-[13px] text-danger">{unlockError}</p>{/if}
        <button class="btn btn-primary w-full" disabled={unlocking}>
          {#if unlocking}<Icon name="loader-circle" class="spinner" />{/if}
          {t('Unlock')}
        </button>
      </form>
    {:else if phase === 'drop'}
      <div class="mx-auto grid max-w-lg gap-6">
        <div class="grid gap-1">
          <h1 class="text-xl font-semibold tracking-tight">{t('Send files to {name}', { name: owner })}</h1>
          <p class="text-[13px] text-fg-muted">
            {t("Only {name} can open the files you send. You can't see what's already in this folder.", { name: owner })}
          </p>
        </div>
        <button
          type="button"
          class="grid cursor-pointer justify-items-center gap-2 rounded-lg border border-dashed p-10 text-center transition-colors {dragging ? 'border-accent bg-accent-soft' : 'border-line-strong hover:bg-subtle'}"
          onclick={() => picker.click()}>
          <Icon name="upload" class="size-5 text-fg-muted" />
          <span class="text-sm font-medium">{t('Drop files here or choose them')}</span>
        </button>
        <input bind:this={picker} type="file" multiple class="hidden" onchange={(e) => (sendFiles([...e.currentTarget.files]), (e.currentTarget.value = ''))} />
        {#if sent.length}
          <ul class="card divide-y divide-line">
            {#each sent as f (f.id)}
              <li class="flex items-center gap-3 px-4 py-2.5">
                <FileIcon meta={{ name: f.name }} />
                <div class="min-w-0 flex-1">
                  <p class="truncate text-[13px] font-medium">{f.name}</p>
                  <p class="text-xs {f.status === 'error' ? 'text-danger' : 'text-fg-muted'}">
                    {#if f.status === 'error'}{f.error}{:else if f.status === 'done'}{t('Sent')} · {formatSize(f.size)}{:else}{t('{percent}% of {size}', { percent: Math.round(f.progress * 100), size: formatSize(f.size) })}{/if}
                  </p>
                </div>
                {#if f.status === 'done'}<Icon name="check" class="size-4 text-accent-text" />{:else if f.status === 'active'}<Icon name="loader-circle" class="spinner size-4 text-fg-muted" />{/if}
              </li>
            {/each}
          </ul>
        {/if}
        <p class="text-xs text-fg-muted">
          <Sentence text={t("{name}'s key fingerprint is {fingerprint}. If it matters who can read these files, check it with them.", { name: owner })}>{#snippet fingerprint()}<span class="font-mono text-fg">{fingerprintText}</span>{/snippet}</Sentence>
        </p>
      </div>
    {:else if here.node.kind === 'file'}
      <div class="card mx-auto grid max-w-md justify-items-center gap-1 p-8 text-center">
        <div class="mb-3 grid size-14 place-items-center rounded-xl border border-line bg-subtle">
          <FileIcon meta={here.meta} class="size-6" strokeWidth={1.5} />
        </div>
        <h1 class="max-w-full truncate text-base font-semibold">{here.meta.name}</h1>
        <p class="text-[13px] text-fg-muted">{formatSize(here.meta.size)}{#if here.meta.mtime}, <Sentence text={t('modified {date}')}>{#snippet date()}<Time ms={here.meta.mtime} />{/snippet}</Sentence>{/if}</p>
        <div class="mt-5 grid w-full gap-2">
          <button type="button" class="btn btn-accent btn-lg w-full" onclick={() => downloadEntry(here)}>
            <Icon name="download" /> {t('Download')}
          </button>
          {#if previewKind(here.meta)}
            <button type="button" class="btn btn-secondary btn-lg w-full" onclick={() => (preview = { entries: [here], start: 0 })}>
              <Icon name="eye" /> {t('Preview')}
            </button>
          {/if}
        </div>
      </div>
    {:else}
      <div class="flex flex-wrap items-center justify-between gap-3">
        <nav class="flex flex-wrap items-center gap-1 text-sm" aria-label={t('Folder path')}>
          {#each trail as crumb, i (crumb.node.id)}
            {#if i}<Icon name="chevron-right" class="size-4 text-fg-faint" />{/if}
            {#if i === trail.length - 1}
              <h1 class="truncate px-1 text-xl font-semibold tracking-tight">{crumb.meta.name}</h1>
            {:else}
              <button type="button" class="cursor-pointer rounded px-1 text-fg-muted hover:text-fg" onclick={() => ((rows = []), (trail = trail.slice(0, i + 1)))}>{crumb.meta.name}</button>
            {/if}
          {/each}
        </nav>
        {#if rows.length}
          <button type="button" class="btn btn-secondary" onclick={downloadAll}><Icon name="download" /> {t('Download all')}</button>
        {/if}
      </div>
      <div class="card mt-6 overflow-hidden">
        {#if listing && !rows.length}
          <div class="grid h-48 place-items-center text-fg-muted"><Icon name="loader-circle" class="spinner size-5" /></div>
        {:else if !rows.length}
          <p class="px-6 py-16 text-center text-[13px] text-fg-muted">{t('This folder is empty.')}</p>
        {:else}
          <table class="table">
            <thead>
              <tr>
                <th>{t('Name')}</th>
                <th class="hidden w-28 text-right sm:table-cell">{t('Size')}</th>
                <th class="w-28"><span class="sr-only">{t('Actions')}</span></th>
              </tr>
            </thead>
            <tbody>
              {#each rows as entry (entry.node.id)}
                {@const folder = entry.node.kind === 'folder'}
                <tr class="group">
                  <td class="max-w-0">
                    <button
                      type="button"
                      class="flex max-w-full cursor-pointer items-center gap-3 text-left"
                      onclick={() => (folder ? ((rows = []), (trail = [...trail, entry])) : (preview = { entries: files, start: files.indexOf(entry) }))}>
                      {#if folder}<FolderIcon name={entry.meta.name} />{:else}<FileIcon meta={entry.meta} />{/if}
                      <span class="truncate font-medium group-hover:underline group-hover:decoration-line-strong group-hover:underline-offset-4">{entry.meta.name}</span>
                    </button>
                  </td>
                  <td class="hidden text-right text-fg-muted tabular-nums sm:table-cell">{folder ? '' : formatSize(entry.meta.size)}</td>
                  <td class="text-right">
                    {#if !folder}
                      <button type="button" class="btn btn-secondary h-7 px-2.5 text-[13px]" onclick={() => downloadEntry(entry)}>
                        <Icon name="download" /> {t('Download')}
                      </button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {/if}
  </main>

  <footer class="border-t border-line">
    <div class="mx-auto flex max-w-4xl flex-wrap items-center justify-between gap-2 px-4 py-5 text-xs text-fg-muted">
      <p class="flex items-center gap-1.5">
        <Icon name="shield-check" class="size-3.5" />
        {#if phase === 'drop'}{t('Encrypted in your browser before upload.')}{:else}{t('Decrypted in your browser. The key is never sent to the server.')}{/if}
      </p>
      {#if expiresAt}<p><Sentence text={t('Link expires {date}')}>{#snippet date()}<Time ms={expiresAt * 1000} />{/snippet}</Sentence></p>{/if}
    </div>
  </footer>
</div>

{#if preview}
  <Preview
    entries={preview.entries}
    start={preview.start}
    fetch={fetchEntry}
    open={openEntry}
    trail={here?.node.kind === 'folder' ? trail : null}
    list={listFolder}
    ondownload={downloadEntry}
    onclose={() => (preview = null)} />
{/if}

<TransferTray />
<Toasts />
