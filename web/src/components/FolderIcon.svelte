<script>
  // A folder's icon: the plain one, or with "Folder icons by name" on and a
  // pack that has one, the pack's icon for that name.
  import Icon from './Icon.svelte';
  import { folderIconUrl } from '../lib/file-icons.svelte.js';
  import { iconPack, folderIcons, theme } from '../lib/ui.svelte.js';

  let { name = '', pack = null, named = null, class: cls = 'size-4', plainClass = 'text-accent-text' } = $props();

  const url = $derived((named ?? folderIcons.named) ? folderIconUrl(pack ?? iconPack.value, name, theme.dark) : null);
  let broken = $state(false);
  $effect(() => {
    url;
    broken = false;
  });
</script>

{#if url && !broken}
  <img src={url} alt="" class="shrink-0 select-none {cls}" draggable="false" onerror={() => (broken = true)} />
{:else}
  <Icon name="folder" class="shrink-0 {cls} {plainClass}" />
{/if}
