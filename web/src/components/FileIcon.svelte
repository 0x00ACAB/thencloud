<script>
  // The icon for a file, from the icon pack chosen in Settings.
  import Icon from './Icon.svelte';
  import { fileIcon } from '../lib/format.js';
  import { fileIconUrl } from '../lib/file-icons.svelte.js';
  import { iconPack, theme } from '../lib/ui.svelte.js';

  /** `pack` overrides the chosen pack (Settings uses it for its previews). */
  let { meta, pack = null, class: cls = 'size-4', minimalClass = 'text-fg-muted', strokeWidth = 2 } = $props();

  const url = $derived(fileIconUrl(pack ?? iconPack.value, meta, theme.dark));
  let broken = $state(false);
  $effect(() => {
    url;
    broken = false;
  });
</script>

{#if url && !broken}
  <img src={url} alt="" class="shrink-0 select-none {cls}" draggable="false" onerror={() => (broken = true)} />
{:else}
  <Icon name={fileIcon(meta)} class="shrink-0 {cls} {minimalClass}" {strokeWidth} />
{/if}
