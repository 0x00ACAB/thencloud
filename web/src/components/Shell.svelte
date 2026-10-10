<script>
  import { session, logout, avatar, loadMyAvatar } from '../lib/cloud.svelte.js';
  import { theme, setTheme } from '../lib/ui.svelte.js';
  import { formatSize } from '../lib/format.js';
  import Icon from './Icon.svelte';
  import Menu from './Menu.svelte';
  import Wordmark from './Wordmark.svelte';
  import TransferTray from './TransferTray.svelte';
  import FilesView from './views/FilesView.svelte';
  import SharedWithMe from './views/SharedWithMe.svelte';
  import SharedByMe from './views/SharedByMe.svelte';
  import LinksView from './views/LinksView.svelte';
  import SettingsView from './views/SettingsView.svelte';
  import TrashView from './views/TrashView.svelte';
  import AdminView from './views/AdminView.svelte';
  import { onDestroy } from 'svelte';
  import MusicView from './views/MusicView.svelte';
  import VideosView from './views/VideosView.svelte';
  import { unloadVideos } from '../lib/videos.svelte.js';
  import PlayerBar from './PlayerBar.svelte';
  import PlacesView from './views/PlacesView.svelte';
  import NotesView from './views/NotesView.svelte';
  import PhotosView from './views/PhotosView.svelte';
  import { unloadNotes } from '../lib/notes.svelte.js';
  import { unloadPhotos } from '../lib/photos.svelte.js';
  import FolderIcon from './FolderIcon.svelte';
  import FileIcon from './FileIcon.svelte';
  import { places, loadPlaces, resolvePlaces } from '../lib/places.svelte.js';
  import HealthView from './views/HealthView.svelte';
  import { unloadHealth } from '../lib/health.svelte.js';
  import { modules, loadModules } from '../lib/modules.svelte.js';
  import { t } from '../lib/i18n.svelte.js';
  import { storage, loadStorage, dropStorage } from '../lib/storage.svelte.js';

  const rootId = session.me.keys.root_node_id;
  loadMyAvatar().catch(() => {});
  onDestroy(unloadVideos);
  onDestroy(unloadNotes);
  onDestroy(unloadPhotos);
  onDestroy(unloadHealth);
  onDestroy(dropStorage);
  // Linked storage for the meter; what's kept there changes as files move,
  // so it's asked again now and then while anything is linked.
  $effect(() => {
    loadStorage();
    const every = setInterval(() => storage.info?.accounts.length && loadStorage(), 60_000);
    return () => clearInterval(every);
  });
  loadModules().catch(() => {});

  // Phones get a bottom tab bar; fixed things (toasts, the transfer tray,
  // the selection bar, the + button) sit above it, and the music player,
  // using --bottom-bar.
  $effect(() => {
    document.documentElement.classList.add('has-bottom-bar');
    return () => document.documentElement.classList.remove('has-bottom-bar');
  });

  const nav = $derived([
    { name: 'files', label: t('My files'), short: t('Files'), icon: 'folder', to: () => ({ name: 'files', folderId: rootId }) },
    { name: 'recent', label: t('Recent'), icon: 'clock', to: () => ({ name: 'recent' }) },
    { name: 'favourites', label: t('Favourites'), icon: 'star', to: () => ({ name: 'favourites' }) },
    // Once there's something to show.
    ...(Object.keys(places.tags).length ? [{ name: 'tags', label: t('Tags'), icon: 'tag', to: () => ({ name: 'tags' }) }] : []),
    { name: 'shared-with-me', label: t('Shared with me'), short: t('Shared'), icon: 'inbox', to: () => ({ name: 'shared-with-me' }) },
    { name: 'shared-by-me', label: t('Shared by me'), icon: 'users', to: () => ({ name: 'shared-by-me' }) },
    { name: 'links', label: t('Public links'), icon: 'link', to: () => ({ name: 'links' }) },
    { name: 'notes', label: t('Notes'), icon: 'notebook-pen', to: () => ({ name: 'notes' }) },
    { name: 'photos', label: t('Photos'), icon: 'image', to: () => ({ name: 'photos' }) },
    { name: 'music', label: t('Music'), icon: 'music', to: () => ({ name: 'music' }) },
    { name: 'videos', label: t('Videos'), icon: 'clapperboard', to: () => ({ name: 'videos' }) },
    ...(modules.health ? [{ name: 'health', label: t('Health'), icon: 'heart-pulse', to: () => ({ name: 'health' }) }] : []),
    { name: 'trash', label: t('Trash'), icon: 'trash-2', to: () => ({ name: 'trash' }) },
    { name: 'settings', label: t('Settings'), icon: 'settings', to: () => ({ name: 'settings' }) },
    ...(session.me.is_admin ? [{ name: 'admin', label: t('Admin'), icon: 'shield-check', to: () => ({ name: 'admin' }) }] : []),
  ]);

  // In-app navigation. Uses history.pushState so the browser's back button
  // works, and keeps the section and folder after the # so a reload lands in
  // the same place. Only opaque ids go there, never names: history may be
  // synced to a browser vendor's servers.
  function toHash(v) {
    if (v.name === 'files') return v.folderId === rootId ? '#/files' : `#/files/${v.folderId}`;
    if (v.name === 'music' && v.album) return `#/music/album/${v.album}`;
    if (v.name === 'music' && v.playlist) return `#/music/playlist/${v.playlist}`;
    return `#/${v.name}`;
  }

  function fromHash(hash) {
    const [name, a, b] = hash.replace(/^#\/?/, '').split('/');
    const id = (x) => (x && /^[\w-]+$/.test(x) ? x : null);
    if (name === 'files') return { name, folderId: id(a) ?? rootId };
    if (name === 'music' && a === 'album' && id(b)) return { name, album: b };
    if (name === 'music' && a === 'playlist' && id(b)) return { name, playlist: b };
    // Optional modules aren't known to be on yet at first; their views check.
    if (nav.some((n) => n.name === name) || name === 'health' || name === 'tags') return { name };
    return null;
  }

  let view = $state(fromHash(location.hash) ?? { name: 'files', folderId: rootId });

  function go(next) {
    view = next;
    history.pushState({ ...next }, '', toHash(next));
    scrollTo(0, 0);
  }

  $effect(() => {
    history.replaceState({ ...view }, '', toHash(view));
    // A hash typed into the address bar arrives without a state.
    const onPop = (e) => {
      const next = e.state?.name ? e.state : fromHash(location.hash);
      if (next) view = next;
    };
    addEventListener('popstate', onPop);
    return () => removeEventListener('popstate', onPop);
  });

  /** The sections on the phone tab bar; the others go under More. */
  const MOBILE_TABS = ['files', 'shared-with-me', 'trash', 'settings'];

  // "My files" stays highlighted while browsing own folders; folders reached
  // through a share highlight "Shared with me".
  let inShare = $state(false);
  const current = $derived(view.name === 'files' && inShare ? 'shared-with-me' : view.name);

  // Favourites are listed under their entry in the sidebar, names decrypted
  // here (the list itself holds only ids).
  const SIDEBAR_FAVOURITES = 8;
  let starred = $state([]);
  loadPlaces().catch(() => {});
  $effect(() => {
    const ids = places.favourites.slice(-SIDEBAR_FAVOURITES).reverse();
    let live = true;
    resolvePlaces(ids).then((list) => live && (starred = list.filter((x) => x.entry)));
    return () => (live = false);
  });

  function openStarred(x) {
    if (x.entry.node.kind === 'folder') go({ name: 'files', folderId: x.id });
    else if (x.parentId) go({ name: 'files', folderId: x.parentId, open: x.id });
  }

  // The meter: this server's share, plus each linked extra space (a mirror
  // is a copy, not more room), out of all the room there is.
  const extras = $derived((storage.info?.accounts ?? []).filter((a) => a.mode === 'extra' && !a.broken));
  const meter = $derived.by(() => {
    const server = { key: 'server', label: t('This server'), used: session.me.used_bytes, room: session.me.quota_bytes, cls: 'bg-accent' };
    const linked = extras.map((a) => ({
      key: a.id,
      label: t('Google Drive'),
      used: a.used_bytes,
      room: a.used_bytes + (a.free_bytes ?? 0),
      cls: 'bg-place-google',
    }));
    const parts = [server, ...linked];
    const room = Math.max(1, parts.reduce((n, p) => n + p.room, 0));
    return {
      parts: parts.map((p) => ({ ...p, pct: Math.min(100, (p.used / room) * 100) })),
      used: parts.reduce((n, p) => n + p.used, 0),
      room,
    };
  });

  const themes = $derived([
    ['system', 'monitor', t('System')],
    ['light', 'sun', t('Light')],
    ['dark', 'moon', t('Dark')],
  ]);
</script>

<div class="flex min-h-dvh flex-col">
  <header class="sticky top-0 z-30 border-b border-line bg-bg/80 pt-[var(--safe-top)] backdrop-blur-md">
    <div class="flex h-14 items-center gap-3 px-4 md:px-6">
      <button type="button" class="cursor-pointer" onclick={() => go(nav[0].to())} aria-label={t('thencloud home')}>
        <Wordmark />
      </button>
      <span class="text-line-strong select-none" aria-hidden="true">/</span>
      <span class="truncate text-sm font-medium" dir="auto">{avatar.name ?? session.me.username}</span>
      {#if session.me.is_admin}<span class="badge">{t('Admin')}</span>{/if}

      <div class="ml-auto flex items-center gap-1">
        <div class="mr-1 hidden items-center rounded-full border border-line p-0.5 sm:flex" role="radiogroup" aria-label={t('Theme')}>
          {#each themes as [value, icon, label] (value)}
            <button
              type="button"
              role="radio"
              aria-checked={theme.pref === value}
              aria-label={label}
              title={label}
              class="grid size-6 cursor-pointer place-items-center rounded-full transition-colors {theme.pref === value
                ? 'bg-muted text-fg'
                : 'text-fg-faint hover:text-fg'}"
              onclick={() => setTheme(value)}>
              <Icon name={icon} class="size-3.5" />
            </button>
          {/each}
        </div>
        <Menu
          label={t('Account')}
          buttonClass="grid size-8 cursor-pointer place-items-center rounded-full bg-accent text-xs font-semibold text-accent-fg uppercase ring-offset-2 ring-offset-bg hover:ring-2 hover:ring-line-strong"
          items={[
            { label: t('Settings'), icon: 'settings', onclick: () => go({ name: 'settings' }) },
            'sep',
            { label: t('Sign out'), icon: 'log-out', onclick: logout },
          ]}>
          {#snippet trigger()}{#if avatar.url}<img src={avatar.url} alt="" class="size-8 rounded-full object-cover" />{:else}{[...(avatar.name ?? session.me.username)][0]}{/if}{/snippet}
        </Menu>
      </div>
    </div>

  </header>

  <div class="flex flex-1">
    <aside class="sticky top-[calc(3.5rem+1px)] hidden h-[calc(100dvh-3.5rem-1px-var(--player-bar))] w-60 shrink-0 flex-col border-r border-line px-3 py-4 md:flex">
      <nav class="-mx-1 grid min-h-0 content-start gap-0.5 overflow-y-auto px-1" aria-label={t('Sections')}>
        {#each nav as item (item.name)}
          <button type="button" class="nav-item" aria-current={current === item.name ? 'page' : undefined} onclick={() => go(item.to())}>
            <Icon name={item.icon} />{item.label}
          </button>
          {#if item.name === 'files' && places.searches.length}
            <div class="mb-1 ml-4 grid gap-px border-l border-line pl-2">
              {#each places.searches as s (s.id)}
                <button
                  type="button"
                  class="flex h-7 min-w-0 cursor-pointer items-center gap-2 rounded-md px-2 text-left text-[13px] text-fg-muted hover:bg-muted hover:text-fg"
                  title={s.query}
                  onclick={() => go({ name: 'files', folderId: rootId, search: { query: s.query, scope: s.scope } })}>
                  <Icon name="bookmark" class="size-3.5" />
                  <span class="truncate">{s.name}</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if item.name === 'favourites' && starred.length}
            <div class="mb-1 ml-4 grid gap-px border-l border-line pl-2">
              {#each starred as x (x.id)}
                <button
                  type="button"
                  class="flex h-7 min-w-0 cursor-pointer items-center gap-2 rounded-md px-2 text-left text-[13px] text-fg-muted hover:bg-muted hover:text-fg {view.name === 'files' && view.folderId === x.id ? 'text-fg' : ''}"
                  title={[...x.location, x.entry.meta.name].join(' / ')}
                  onclick={() => openStarred(x)}>
                  {#if x.entry.node.kind === 'folder'}<FolderIcon name={x.entry.meta.name} class="size-3.5" />{:else}<FileIcon meta={x.entry.meta} class="size-3.5" />{/if}
                  <span class="truncate">{x.entry.meta.name}</span>
                </button>
              {/each}
            </div>
          {/if}
        {/each}
      </nav>

      <div class="mt-auto grid shrink-0 gap-2 px-2 pt-3">
        <div class="flex items-baseline justify-between text-xs">
          <span class="font-medium">{t('Storage')}</span>
          <span class="text-fg-muted tabular-nums">{t('{used} of {quota}', { used: formatSize(meter.used), quota: formatSize(meter.room) })}</span>
        </div>
        <div class="progress flex">
          {#each meter.parts as p (p.key)}<div class="rounded-none first:rounded-l-full last:rounded-r-full {p.cls}" style:width="{p.pct}%"></div>{/each}
        </div>
        {#if meter.parts.length > 1}
          <ul class="grid gap-0.5 text-xs text-fg-muted">
            {#each meter.parts as p (p.key)}
              <li class="flex items-center gap-1.5">
                <span class="size-2 shrink-0 rounded-full {p.cls}" aria-hidden="true"></span>
                <span class="truncate">{p.label}</span>
                <span class="ml-auto tabular-nums">{formatSize(p.used)}</span>
              </li>
            {/each}
          </ul>
        {/if}
        <p class="mt-2 flex items-center gap-1.5 text-xs text-fg-muted">
          <Icon name="lock" class="size-3.5" /> {t('End-to-end encrypted')}
        </p>
      </div>
    </aside>

    <main class="min-w-0 flex-1 px-4 pt-6 pb-[calc(var(--bottom-bar)+1.5rem)] md:px-8 md:pt-8 md:pb-[calc(var(--bottom-bar)+2rem)]">
      {#key view.name}
      <div class="mx-auto max-w-5xl animate-enter">
        {#if view.name === 'files'}
          <FilesView folderId={view.folderId} openId={view.open} search={view.search} {go} bind:inShare />
        {:else if view.name === 'recent' || view.name === 'favourites' || view.name === 'tags'}
          <PlacesView mode={view.name} {go} />
        {:else if view.name === 'shared-with-me'}
          <SharedWithMe {go} />
        {:else if view.name === 'shared-by-me'}
          <SharedByMe {go} />
        {:else if view.name === 'links'}
          <LinksView {go} />
        {:else if view.name === 'notes'}
          <NotesView {go} />
        {:else if view.name === 'photos'}
          <PhotosView {go} />
        {:else if view.name === 'music'}
          <MusicView album={view.album} playlist={view.playlist} {go} />
        {:else if view.name === 'videos'}
          <VideosView series={view.series} play={view.play} {go} />
        {:else if view.name === 'health'}
          {#if modules.health}
            <HealthView />
          {:else if modules.loaded}
            <p class="text-[13px] text-fg-muted">{t('Health is turned off. You can turn it on in Settings, under Modules.')} <button type="button" class="link" onclick={() => go({ name: 'settings' })}>{t('Open Settings')}</button></p>
          {/if}
        {:else if view.name === 'trash'}
          <TrashView {go} />
        {:else if view.name === 'settings'}
          <SettingsView />
        {:else if view.name === 'admin' && session.me.is_admin}
          <AdminView />
        {/if}
      </div>
      {/key}
    </main>
  </div>
</div>

<!-- Phones: the main sections as a bottom tab bar; the rest under More. -->
<nav
  class="tab-bar fixed inset-x-0 bottom-0 z-30 flex border-t border-line bg-bg/90 pb-[var(--safe-bottom)] backdrop-blur-md md:hidden"
  aria-label={t('Sections')}>
  {#each nav.filter((n) => MOBILE_TABS.includes(n.name)) as item (item.name)}
    <button
      type="button"
      class="tab-item"
      aria-current={current === item.name ? 'page' : undefined}
      onclick={() => go(item.to())}>
      <Icon name={item.icon} class="size-5" />{item.short ?? item.label}
    </button>
  {/each}
  <Menu
    label={t('More sections')}
    align="end"
    buttonClass="tab-item {MOBILE_TABS.includes(current) ? '' : 'text-fg'}"
    items={nav
      .filter((n) => !MOBILE_TABS.includes(n.name))
      .map((n) => ({ label: n.label, icon: n.icon, onclick: () => go(n.to()) }))}>
    {#snippet trigger()}<Icon name="ellipsis" class="size-5" />{t('More')}{/snippet}
  </Menu>
</nav>

<PlayerBar {go} />
<TransferTray />
