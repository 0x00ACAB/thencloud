<script>
  // The music player: a bar along the bottom while something is queued, a
  // queue panel, and on phones a full-screen "now playing" sheet.
  import { onDestroy } from 'svelte';
  import Icon from './Icon.svelte';
  import { fly, pop, portal } from '../lib/motion.js';
  import { player, current, info, toggle, next, previous, seek, jump, removeFromQueue, setVolume, toggleMute, toggleShuffle, cycleRepeat, stop, formatTime, library, unloadMusic, RATES, setRate, chapterAt, longForm } from '../lib/music.svelte.js';
  import Menu from './Menu.svelte';

  let { go } = $props();

  let queueOpen = $state(false);
  let sheet = $state(false); // phones: full-screen player
  let scrub = $state(null); // seek position while dragging
  let queueBox = $state();
  let queueBtn = $state();

  const track = $derived(current());
  const it = $derived(track && info(track));
  const shown = $derived(scrub ?? player.time);
  const pct = $derived(player.duration ? (shown / player.duration) * 100 : 0);
  const upNext = $derived(player.queue.slice(player.index + 1));
  const chapter = $derived(player.chapters[chapterAt(shown)] ?? null);
  const showSpeed = $derived(longForm() || player.rate !== 1);
  const rateLabel = (r) => `${r}x`;

  onDestroy(unloadMusic);

  $effect(() => {
    const root = document.documentElement.classList;
    root.toggle('has-player', !!track);
    return () => root.remove('has-player');
  });

  $effect(() => {
    if (!track) {
      queueOpen = false;
      sheet = false;
    }
  });

  // The queue panel closes on Esc or a click elsewhere; Esc also closes the sheet.
  $effect(() => {
    if (!queueOpen && !sheet) return;
    const key = (e) => e.key === 'Escape' && (queueOpen ? (queueOpen = false) : (sheet = false));
    const outside = (e) => !sheet && !queueBox?.contains(e.target) && !queueBtn?.contains(e.target) && (queueOpen = false);
    addEventListener('keydown', key);
    addEventListener('pointerdown', outside, true);
    return () => {
      removeEventListener('keydown', key);
      removeEventListener('pointerdown', outside, true);
    };
  });

  function showAlbum() {
    if (!track) return;
    sheet = false;
    if (library.value?.albums.some((a) => a.id === track.albumId)) go({ name: 'music', album: track.albumId });
    else if (track.entry.parentId) go({ name: 'files', folderId: track.entry.parentId });
  }

  const commit = () => {
    if (scrub !== null) seek(scrub);
    scrub = null;
  };

  const volumeIcon = $derived(player.muted || player.volume === 0 ? 'volume-x' : player.volume < 0.5 ? 'volume-1' : 'volume-2');
</script>

{#snippet art(cls)}
  <div class="grid shrink-0 place-items-center overflow-hidden rounded-md border border-line bg-muted text-fg-faint {cls}">
    {#if it?.cover}<img src={it.cover} alt="" class="size-full object-cover" draggable="false" />{:else}<Icon name="music" class="size-2/5" strokeWidth={1.5} />{/if}
  </div>
{/snippet}

{#snippet seekbar()}
  <div class="flex w-full items-center gap-2 text-[11px] text-fg-muted tabular-nums">
    <span class="w-10 text-right">{formatTime(shown)}</span>
    <input
      type="range"
      class="range flex-1"
      min="0"
      max={player.duration || 0}
      step="any"
      value={shown}
      style:--pct="{pct}%"
      aria-label="Position"
      disabled={!player.duration}
      oninput={(e) => (scrub = +e.currentTarget.value)}
      onchange={commit} />
    <span class="w-10">{formatTime(player.duration)}</span>
  </div>
{/snippet}

{#snippet controls(big)}
  {@const b = big ? 'size-11 [&_svg]:size-5' : 'size-8'}
  <div class="flex items-center justify-center gap-1 {big ? 'gap-4' : ''}">
    <button type="button" class="btn btn-ghost btn-icon {b} {player.shuffle ? 'text-accent-text hover:text-accent-text' : ''}" aria-label="Shuffle" aria-pressed={player.shuffle} title="Shuffle" onclick={toggleShuffle}>
      <Icon name="shuffle" />
    </button>
    <button type="button" class="btn btn-ghost btn-icon {b} text-fg" aria-label="Previous" title="Previous" onclick={previous}><Icon name="skip-back" /></button>
    <button
      type="button"
      class="btn btn-icon rounded-full bg-fg text-bg hover:bg-fg/85 {big ? 'size-16 [&_svg]:size-7' : 'size-9'}"
      aria-label={player.playing ? 'Pause' : 'Play'}
      title={player.playing ? 'Pause' : 'Play'}
      onclick={toggle}>
      {#if player.buffering && !player.playing}<Icon name="loader-circle" class="spinner" />{:else}<Icon name={player.playing ? 'pause' : 'play'} />{/if}
    </button>
    <button type="button" class="btn btn-ghost btn-icon {b} text-fg" aria-label="Next" title="Next" onclick={next}><Icon name="skip-forward" /></button>
    <button
      type="button"
      class="btn btn-ghost btn-icon {b} {player.repeat !== 'off' ? 'text-accent-text hover:text-accent-text' : ''}"
      aria-label={{ off: 'Repeat', all: 'Repeat all', one: 'Repeat one' }[player.repeat]}
      title={{ off: 'Repeat', all: 'Repeat all', one: 'Repeat one' }[player.repeat]}
      onclick={cycleRepeat}>
      <Icon name={player.repeat === 'one' ? 'repeat-1' : 'repeat'} />
    </button>
  </div>
{/snippet}

{#snippet speed()}
  <Menu
    label="Playback speed"
    buttonClass="btn btn-ghost h-8 px-2 text-xs font-medium tabular-nums {player.rate !== 1 ? 'text-accent-text hover:text-accent-text' : ''}"
    items={RATES.map((r) => ({ label: r === 1 ? 'Normal' : rateLabel(r), checked: player.rate === r, onclick: () => setRate(r) }))}>
    {#snippet trigger()}{rateLabel(player.rate)}{/snippet}
  </Menu>
{/snippet}

{#snippet chapterList()}
  {@const now = chapterAt(shown)}
  <div>
    <p class="px-2 pb-1 text-xs font-medium text-fg-muted">Chapters</p>
    {#each player.chapters as c, i (i)}
      <button type="button" class="flex w-full cursor-pointer items-baseline gap-3 rounded-md px-2 py-1.5 text-left hover:bg-muted" onclick={() => seek(c.start)}>
        <span class="w-14 shrink-0 text-xs text-fg-muted tabular-nums">{formatTime(c.start)}</span>
        <span class="truncate text-sm {i === now ? 'font-medium text-accent-text' : ''}">{c.title}</span>
      </button>
    {/each}
  </div>
{/snippet}

{#snippet queueList()}
  <div class="grid gap-3">
    {#if player.chapters.length}{@render chapterList()}{/if}
    <div>
      <p class="px-2 pb-1 text-xs font-medium text-fg-muted">Now playing</p>
      {@render row(track, player.index, true)}
    </div>
    <div>
      <div class="flex items-center px-2 pb-1">
        <p class="mr-auto text-xs font-medium text-fg-muted">Next up</p>
        {#if upNext.length}<button type="button" class="cursor-pointer text-xs text-fg-muted hover:text-fg" onclick={() => (player.queue = player.queue.slice(0, player.index + 1))}>Clear</button>{/if}
      </div>
      {#each upNext as t, i (i + ':' + t.id)}
        {@render row(t, player.index + 1 + i, false)}
      {:else}
        <p class="px-2 py-3 text-[13px] text-fg-muted">Nothing after this{player.repeat === 'all' ? '; the queue starts over' : ''}.</p>
      {/each}
    </div>
  </div>
{/snippet}

{#snippet row(t, i, now)}
  {@const ti = info(t)}
  <div class="group flex items-center gap-1 rounded-md hover:bg-muted">
    <button type="button" class="flex min-w-0 flex-1 cursor-pointer items-center gap-2 px-2 py-1.5 text-left" onclick={() => !now && jump(i)}>
      <span class="grid min-w-0">
        <span class="truncate text-sm {now ? 'font-medium text-accent-text' : ''}">{ti.title}</span>
        <span class="truncate text-xs text-fg-muted">{ti.artist}</span>
      </span>
    </button>
    {#if !now}
      <button type="button" class="btn btn-ghost btn-icon mr-1 size-7 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100" aria-label="Remove {ti.title} from the queue" onclick={() => removeFromQueue(i)}>
        <Icon name="x" />
      </button>
    {/if}
  </div>
{/snippet}

{#if track}
  <div class="player-bar fixed inset-x-0 z-30 border-t border-line bg-bg/95 backdrop-blur-md" transition:fly={{ y: 16 }}>
    <!-- Phones: a thin progress line along the top. -->
    <div class="absolute inset-x-0 top-0 h-0.5 bg-line md:hidden"><div class="h-full bg-fg" style:width="{pct}%"></div></div>
    <div class="flex h-16 items-center gap-3 px-3 md:h-20 md:px-4">
      <div class="flex min-w-0 flex-1 items-center gap-3 md:w-[30%] md:flex-none">
        <button type="button" class="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left md:hidden" onclick={() => ((queueOpen = false), (sheet = true))} aria-label="Open player">
          {@render art('size-10')}
          <span class="grid min-w-0">
            <span class="truncate text-sm font-medium">{it.title}</span>
            <span class="truncate text-xs text-fg-muted">{chapter?.title ?? it.artist}</span>
          </span>
        </button>
        <div class="hidden min-w-0 items-center gap-3 md:flex">
          {@render art('size-14')}
          <span class="grid min-w-0">
            <button type="button" class="cursor-pointer truncate text-left text-sm font-medium hover:underline" onclick={showAlbum}>{it.title}</button>
            <span class="truncate text-xs text-fg-muted">{chapter ? `${it.artist ? `${it.artist} · ` : ''}${chapter.title}` : it.artist}</span>
          </span>
        </div>
      </div>

      <div class="hidden max-w-xl flex-1 flex-col items-center gap-1 md:flex">
        {@render controls(false)}
        {@render seekbar()}
      </div>

      <div class="hidden w-[30%] items-center justify-end gap-1 md:flex">
        {#if showSpeed}{@render speed()}{/if}
        <button
          bind:this={queueBtn}
          type="button"
          class="btn btn-ghost btn-icon {queueOpen ? 'text-accent-text hover:text-accent-text' : ''}"
          aria-label="Queue"
          aria-expanded={queueOpen}
          title="Queue"
          onclick={() => (queueOpen = !queueOpen)}>
          <Icon name="list-music" />
        </button>
        <button type="button" class="btn btn-ghost btn-icon" aria-label={player.muted ? 'Unmute' : 'Mute'} title={player.muted ? 'Unmute' : 'Mute'} onclick={toggleMute}>
          <Icon name={volumeIcon} />
        </button>
        <input
          type="range"
          class="range w-24"
          min="0"
          max="1"
          step="0.01"
          value={player.muted ? 0 : player.volume}
          style:--pct="{(player.muted ? 0 : player.volume) * 100}%"
          aria-label="Volume"
          oninput={(e) => setVolume(+e.currentTarget.value)} />
        <button type="button" class="btn btn-ghost btn-icon ml-1" aria-label="Stop and clear the queue" title="Stop" onclick={stop}><Icon name="x" /></button>
      </div>

      <div class="flex items-center md:hidden">
        <button type="button" class="btn btn-ghost btn-icon size-10 text-fg [&_svg]:size-5" aria-label={player.playing ? 'Pause' : 'Play'} onclick={toggle}>
          {#if player.buffering && !player.playing}<Icon name="loader-circle" class="spinner" />{:else}<Icon name={player.playing ? 'pause' : 'play'} />{/if}
        </button>
        <button type="button" class="btn btn-ghost btn-icon size-10 text-fg [&_svg]:size-5" aria-label="Next" onclick={next}><Icon name="skip-forward" /></button>
      </div>
    </div>
  </div>

  {#if queueOpen}
    <div
      bind:this={queueBox}
      class="menu bottom-[calc(var(--bottom-bar)+0.5rem)] right-4 hidden max-h-[min(32rem,60vh)] w-80 overflow-y-auto p-2 md:block"
      role="dialog"
      aria-label="Queue"
      use:portal
      transition:pop>
      {@render queueList()}
    </div>
  {/if}

  {#if sheet}
    <div class="fixed inset-0 z-50 flex flex-col bg-bg px-5 pt-[max(env(safe-area-inset-top),1rem)] pb-[max(env(safe-area-inset-bottom),1.5rem)] md:hidden" role="dialog" aria-label="Now playing" use:portal transition:fly={{ y: 48 }}>
      <div class="flex items-center">
        <button type="button" class="btn btn-ghost btn-icon size-10 [&_svg]:size-5" aria-label="Close player" onclick={() => (sheet = false)}><Icon name="chevron-down" /></button>
        <p class="flex-1 text-center text-xs font-medium text-fg-muted">{queueOpen ? 'Queue' : 'Now playing'}</p>
        <button type="button" class="btn btn-ghost btn-icon size-10 [&_svg]:size-5 {queueOpen ? 'text-accent-text' : ''}" aria-label="Queue" aria-pressed={queueOpen} onclick={() => (queueOpen = !queueOpen)}>
          <Icon name="list-music" />
        </button>
      </div>
      {#if queueOpen}
        <div class="-mx-2 mt-4 min-h-0 flex-1 overflow-y-auto">{@render queueList()}</div>
      {:else}
        <div class="grid min-h-0 flex-1 place-items-center py-6">{@render art('aspect-square w-full max-w-sm')}</div>
      {/if}
      <div class="grid gap-4">
        <button type="button" class="grid min-w-0 cursor-pointer text-left" onclick={showAlbum}>
          <span class="truncate text-lg font-semibold tracking-tight">{it.title}</span>
          <span class="truncate text-sm text-fg-muted">{chapter ? chapter.title : `${it.artist}${it.album ? ` · ${it.album}` : ''}`}</span>
        </button>
        {#if showSpeed}<div class="-my-2 flex justify-end">{@render speed()}</div>{/if}
        {@render seekbar()}
        {@render controls(true)}
      </div>
    </div>
  {/if}
{/if}
