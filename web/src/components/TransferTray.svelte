<script>
  import { fly, pop, slide } from '../lib/motion.js';
  import { transfers, clearFinishedTransfers } from '../lib/ui.svelte.js';
  import { formatSize } from '../lib/format.js';
  import Icon from './Icon.svelte';

  let collapsed = $state(false);
  const active = $derived(transfers.filter((t) => t.status === 'active').length);
  const failed = $derived(transfers.filter((t) => t.status === 'error').length);
  const heading = $derived(
    active ? `Encrypting and transferring ${active} ${active === 1 ? 'file' : 'files'}` : failed ? `${failed} failed` : 'All transfers complete',
  );
</script>

{#if transfers.length}
  <section
    in:fly={{ y: 16 }}
    out:fly={{ y: 16, duration: 140 }}
    class="fixed right-4 bottom-4 z-40 w-[calc(100%-2rem)] max-w-sm overflow-hidden rounded-lg border border-line bg-bg shadow-lg shadow-black/5 dark:shadow-black/40">
    <header class="flex h-10 items-center gap-2 border-b border-line bg-subtle pr-1.5 pl-3.5">
      {#if active}<Icon name="loader-circle" class="spinner text-fg-muted" />{/if}
      <h2 class="flex-1 truncate text-[13px] font-medium">{heading}</h2>
      <button type="button" class="btn btn-ghost btn-icon h-7 w-7" aria-label={collapsed ? 'Expand' : 'Collapse'} onclick={() => (collapsed = !collapsed)}>
        <Icon name="chevron-right" class="size-4 transition-transform {collapsed ? '-rotate-90' : 'rotate-90'}" />
      </button>
      {#if !active}
        <button type="button" class="btn btn-ghost btn-icon h-7 w-7" aria-label="Clear" onclick={clearFinishedTransfers}>
          <Icon name="x" />
        </button>
      {/if}
    </header>
    {#if !collapsed}
      <ul class="max-h-64 divide-y divide-line overflow-y-auto">
        {#each transfers as t (t.id)}
          <li class="grid gap-1.5 px-3.5 py-2.5" in:slide out:slide>
            <div class="flex items-center gap-2 text-[13px]">
              <Icon name={t.kind === 'upload' ? 'upload' : 'download'} class="size-3.5 shrink-0 text-fg-faint" />
              <span class="min-w-0 flex-1 truncate">{t.name}</span>
              {#if t.status === 'done'}
                <span in:pop={{ start: 0.6, duration: 200 }}><Icon name="check" class="size-4 text-success" /></span>
              {:else if t.status === 'error'}
                <span class="text-xs text-danger">Failed</span>
              {:else}
                <span class="text-xs text-fg-muted tabular-nums">{Math.round(t.progress * 100)}%</span>
              {/if}
            </div>
            {#if t.status === 'active'}
              <div class="progress"><div style:width="{t.progress * 100}%"></div></div>
            {:else if t.status === 'error'}
              <p class="text-xs text-fg-muted">{t.error}</p>
            {:else}
              <p class="text-xs text-fg-faint">{formatSize(t.size)}</p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}
