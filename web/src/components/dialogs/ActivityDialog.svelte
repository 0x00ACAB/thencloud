<script>
  // Who did what in a folder, newest first. The server keeps who, what kind
  // and which node; names are decrypted here for items we can still reach.
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import Avatar from '../Avatar.svelte';
  import Time from '../Time.svelte';
  import { activity, session } from '../../lib/cloud.svelte.js';
  import { toastError } from '../../lib/ui.svelte.js';
  import { fly } from '../../lib/motion.js';

  // `onopen(event)`: show the item in its folder.
  let { entry, onopen, onclose } = $props();

  const PAGE = 100;
  let list = $state(null);
  let more = $state(false);
  let loading = $state(false);

  async function load() {
    loading = true;
    try {
      const page = await activity(entry, list?.at(-1)?.id ?? null);
      list = [...(list ?? []), ...page];
      more = page.length === PAGE;
    } catch (e) {
      toastError(e);
      list ??= [];
    } finally {
      loading = false;
    }
  }

  onMount(load);

  const VERB = { added: 'added', changed: 'uploaded a new version of', renamed: 'renamed', moved: 'moved', trashed: 'moved to the trash', restored: 'restored' };
  const who = (e) => (e.actor === session.me.username ? 'You' : e.actor);
</script>

<Modal title="Activity" description="In {entry.meta.name} and the folders in it, over the last 90 days. The server records who did what and roughly when, not names." {onclose} class="max-w-lg">
  {#if list === null}
    <div class="grid gap-3" aria-hidden="true">
      {#each [0, 1, 2] as i (i)}
        <div class="flex items-center gap-3"><div class="skeleton size-7 rounded-full"></div><div class="skeleton h-3.5 w-64"></div></div>
      {/each}
    </div>
  {:else if !list.length}
    <p class="text-[13px] text-fg-muted">Nothing has happened here yet.</p>
  {:else}
    <ul class="-mx-1 grid max-h-96 gap-3 overflow-y-auto px-1" in:fly>
      {#each list as e (e.id)}
        <li class="flex items-start gap-3">
          <Avatar username={e.actor} class="mt-0.5 size-7 text-xs" />
          <div class="min-w-0 flex-1 text-sm">
            <p class="break-words">
              <span class="font-medium">{who(e)}</span>
              {VERB[e.kind] ?? e.kind}
              {#if e.name !== null}
                <button type="button" class="link font-medium" onclick={() => onopen(e)}>{e.name}</button>
              {:else}
                <span class="text-fg-muted">{e.folder ? 'a folder' : 'a file'} that's no longer here</span>
              {/if}
            </p>
            <!-- Times are kept to the hour, so the last hour is as close as it gets. -->
            <p class="text-xs text-fg-muted">{#if Date.now() - e.at * 1000 < 3600_000}In the last hour{:else}<Time ms={e.at * 1000} relative />{/if}</p>
          </div>
        </li>
      {/each}
    </ul>
    {#if more}
      <button type="button" class="btn btn-ghost justify-self-start" disabled={loading} onclick={load}>
        {#if loading}<Icon name="loader-circle" class="spinner" />{/if} Show older
      </button>
    {/if}
  {/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>Done</button>
  {/snippet}
</Modal>
