<script>
  import { session, logout } from '../lib/cloud.svelte.js';
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

  const rootId = session.me.keys.root_node_id;

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
    { name: 'files', label: 'My files', icon: 'folder', to: () => ({ name: 'files', folderId: rootId }) },
    { name: 'shared-with-me', label: 'Shared with me', icon: 'inbox', to: () => ({ name: 'shared-with-me' }) },
    { name: 'shared-by-me', label: 'Shared by me', icon: 'users', to: () => ({ name: 'shared-by-me' }) },
    { name: 'links', label: 'Public links', icon: 'link', to: () => ({ name: 'links' }) },
    { name: 'trash', label: 'Trash', icon: 'trash-2', to: () => ({ name: 'trash' }) },
    { name: 'settings', label: 'Settings', icon: 'settings', to: () => ({ name: 'settings' }) },
  ];

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
          {#snippet trigger()}{session.me.username.slice(0, 1)}{/snippet}
        </Menu>
      </div>
    </div>

    <nav class="flex gap-1 overflow-x-auto px-3 pb-2 md:hidden" aria-label="Sections">
      {#each nav as item (item.name)}
        <button type="button" class="nav-item w-auto shrink-0" aria-current={current === item.name ? 'page' : undefined} onclick={() => go(item.to())}>
          <Icon name={item.icon} />{item.label}
        </button>
      {/each}
    </nav>
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

    <main class="min-w-0 flex-1 px-4 py-6 md:px-8 md:py-8">
      {#key view.name}
      <div class="mx-auto max-w-5xl animate-enter">
        {#if view.name === 'files'}
          <FilesView folderId={view.folderId} {go} bind:inShare />
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
        {/if}
      </div>
      {/key}
    </main>
  </div>
</div>

<TransferTray />
