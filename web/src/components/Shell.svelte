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

  const rootId = session.me.keys.root_node_id;
  loadMyAvatar().catch(() => {});

  // Phones get a bottom tab bar; fixed things (toasts, the transfer tray,
  // the selection bar, the + button) sit above it using --bottom-bar.
  $effect(() => {
    document.documentElement.classList.add('has-bottom-bar');
    return () => document.documentElement.classList.remove('has-bottom-bar');
  });

  // In-app navigation. Uses history.pushState so the browser's back button
  // works, but never puts anything in the URL.
  let view = $state({ name: 'files', folderId: rootId });

  function go(next) {
    view = next;
    history.pushState({ ...next }, '');
    scrollTo(0, 0);
  }

  $effect(() => {
    history.replaceState({ ...view }, '');
    const onPop = (e) => e.state?.name && (view = e.state);
    addEventListener('popstate', onPop);
    return () => removeEventListener('popstate', onPop);
  });

  const nav = [
    { name: 'files', label: 'My files', short: 'Files', icon: 'folder', to: () => ({ name: 'files', folderId: rootId }) },
    { name: 'shared-with-me', label: 'Shared with me', short: 'Shared', icon: 'inbox', to: () => ({ name: 'shared-with-me' }) },
    { name: 'shared-by-me', label: 'Shared by me', icon: 'users', to: () => ({ name: 'shared-by-me' }) },
    { name: 'links', label: 'Public links', icon: 'link', to: () => ({ name: 'links' }) },
    { name: 'trash', label: 'Trash', icon: 'trash-2', to: () => ({ name: 'trash' }) },
    { name: 'settings', label: 'Settings', icon: 'settings', to: () => ({ name: 'settings' }) },
    ...(session.me.is_admin ? [{ name: 'admin', label: 'Admin', icon: 'shield-check', to: () => ({ name: 'admin' }) }] : []),
  ];

  /** The sections on the phone tab bar; the others go under More. */
  const MOBILE_TABS = ['files', 'shared-with-me', 'trash', 'settings'];

  // "My files" stays highlighted while browsing own folders; folders reached
  // through a share highlight "Shared with me".
  let inShare = $state(false);
  const current = $derived(view.name === 'files' && inShare ? 'shared-with-me' : view.name);

  const usedPct = $derived(Math.min(100, (session.me.used_bytes / Math.max(1, session.me.quota_bytes)) * 100));

  const themes = [
    ['system', 'monitor', 'System'],
    ['light', 'sun', 'Light'],
    ['dark', 'moon', 'Dark'],
  ];
</script>

<div class="flex min-h-dvh flex-col">
  <header class="sticky top-0 z-30 border-b border-line bg-bg/80 backdrop-blur-md">
    <div class="flex h-14 items-center gap-3 px-4 md:px-6">
      <button type="button" class="cursor-pointer" onclick={() => go(nav[0].to())} aria-label="thencloud home">
        <Wordmark />
      </button>
      <span class="text-line-strong select-none" aria-hidden="true">/</span>
      <span class="truncate text-sm font-medium">{session.me.username}</span>
      {#if session.me.is_admin}<span class="badge">Admin</span>{/if}

      <div class="ml-auto flex items-center gap-1">
        <div class="mr-1 hidden items-center rounded-full border border-line p-0.5 sm:flex" role="radiogroup" aria-label="Theme">
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
          label="Account"
          buttonClass="grid size-8 cursor-pointer place-items-center rounded-full bg-accent text-xs font-semibold text-accent-fg uppercase ring-offset-2 ring-offset-bg hover:ring-2 hover:ring-line-strong"
          items={[
            { label: 'Settings', icon: 'settings', onclick: () => go({ name: 'settings' }) },
            'sep',
            { label: 'Sign out', icon: 'log-out', onclick: logout },
          ]}>
          {#snippet trigger()}{#if avatar.url}<img src={avatar.url} alt="" class="size-8 rounded-full object-cover" />{:else}{session.me.username.slice(0, 1)}{/if}{/snippet}
        </Menu>
      </div>
    </div>

  </header>

  <div class="flex flex-1">
    <aside class="sticky top-14 hidden h-[calc(100dvh-3.5rem)] w-60 shrink-0 flex-col border-r border-line px-3 py-4 md:flex">
      <nav class="grid gap-0.5" aria-label="Sections">
        {#each nav as item (item.name)}
          <button type="button" class="nav-item" aria-current={current === item.name ? 'page' : undefined} onclick={() => go(item.to())}>
            <Icon name={item.icon} />{item.label}
          </button>
        {/each}
      </nav>

      <div class="mt-auto grid gap-2 px-2">
        <div class="flex items-baseline justify-between text-xs">
          <span class="font-medium">Storage</span>
          <span class="text-fg-muted tabular-nums">{formatSize(session.me.used_bytes)} of {formatSize(session.me.quota_bytes)}</span>
        </div>
        <div class="progress"><div style:width="{usedPct}%"></div></div>
        <p class="mt-2 flex items-center gap-1.5 text-xs text-fg-muted">
          <Icon name="lock" class="size-3.5" /> End-to-end encrypted
        </p>
      </div>
    </aside>

    <main class="min-w-0 flex-1 px-4 pt-6 pb-[calc(var(--bottom-bar)+1.5rem)] md:px-8 md:py-8">
      {#key view.name}
      <div class="mx-auto max-w-5xl animate-enter">
        {#if view.name === 'files'}
          <FilesView folderId={view.folderId} openId={view.open} {go} bind:inShare />
        {:else if view.name === 'shared-with-me'}
          <SharedWithMe {go} />
        {:else if view.name === 'shared-by-me'}
          <SharedByMe {go} />
        {:else if view.name === 'links'}
          <LinksView {go} />
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
  class="fixed inset-x-0 bottom-0 z-30 flex border-t border-line bg-bg/90 pb-[env(safe-area-inset-bottom)] backdrop-blur-md md:hidden"
  aria-label="Sections">
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
    label="More sections"
    align="end"
    buttonClass="tab-item {MOBILE_TABS.includes(current) ? '' : 'text-fg'}"
    items={nav
      .filter((n) => !MOBILE_TABS.includes(n.name))
      .map((n) => ({ label: n.label, icon: n.icon, onclick: () => go(n.to()) }))}>
    {#snippet trigger()}<Icon name="ellipsis" class="size-5" />More{/snippet}
  </Menu>
</nav>

<TransferTray />
