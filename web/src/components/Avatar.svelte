<script>
  // Someone's profile picture (decrypted in the browser), or their initial.
  import { avatarUrl } from '../lib/cloud.svelte.js';

  let { username, class: cls = 'size-7 text-xs' } = $props();

  let url = $state(null);
  $effect(() => {
    let live = true;
    url = null;
    if (username) avatarUrl(username).then((u) => live && (url = u));
    return () => (live = false);
  });
</script>

{#if url}
  <img src={url} alt="" class="shrink-0 rounded-full object-cover select-none {cls}" draggable="false" />
{:else}
  <span class="grid shrink-0 place-items-center rounded-full bg-muted font-semibold uppercase {cls}" aria-hidden="true">{username?.slice(0, 1) ?? ''}</span>
{/if}
