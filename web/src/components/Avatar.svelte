<script>
  // Someone's profile picture (decrypted in the browser), or their initial.
  import { profileOf } from '../lib/cloud.svelte.js';

  let { username, class: cls = 'size-7 text-xs' } = $props();

  let profile = $state(null);
  $effect(() => {
    let live = true;
    profile = null;
    if (username) profileOf(username).then((p) => live && (profile = p));
    return () => (live = false);
  });
  const initial = $derived([...(profile?.name ?? username ?? '')][0] ?? '');
</script>

{#if profile?.url}
  <img src={profile.url} alt="" class="shrink-0 rounded-full object-cover select-none {cls}" draggable="false" />
{:else}
  <span class="grid shrink-0 place-items-center rounded-full bg-muted font-semibold uppercase {cls}" aria-hidden="true">{initial}</span>
{/if}
