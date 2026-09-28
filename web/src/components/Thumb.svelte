<script>
  // A file's encrypted thumbnail, decrypted here, or its icon when it has none.
  import FileIcon from './FileIcon.svelte';
  import { thumbnailUrl } from '../lib/cloud.svelte.js';

  // `fetchThumb(nodeId)`: where the sealed thumbnail comes from (a public link has its own route).
  let { entry, fetchThumb = undefined, iconClass = 'size-10' } = $props();

  let url = $state(null);
  $effect(() => {
    let live = true;
    url = null;
    thumbnailUrl(entry, fetchThumb).then((u) => live && (url = u));
    return () => (live = false);
  });
</script>

{#if url}
  <img src={url} alt="" class="size-full object-cover select-none" draggable="false" />
{:else}
  <FileIcon meta={entry.meta} class={iconClass} strokeWidth={1.5} />
{/if}
