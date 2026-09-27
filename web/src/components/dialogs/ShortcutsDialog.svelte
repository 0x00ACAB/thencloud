<script>
  import Modal from '../Modal.svelte';

  let { onclose } = $props();

  const groups = [
    [
      'Files',
      [
        [['/'], 'Search this folder'],
        [['j', '↓'], 'Next item'],
        [['k', '↑'], 'Previous item'],
        [['Enter'], 'Open or preview'],
        [['x'], 'Select or unselect'],
        [['Esc'], 'Clear the selection'],
        [['Backspace'], 'Up to the parent folder'],
        [['n'], 'New folder'],
        [['u'], 'Upload files'],
        [['Delete'], 'Move the selection, or this item, to trash'],
      ],
    ],
    [
      'Preview',
      [
        [['←', '→'], 'Previous or next file'],
        [['Ctrl', 'S'], 'Save while editing'],
        [['Esc'], 'Close'],
      ],
    ],
  ];
</script>

<Modal title="Keyboard shortcuts" {onclose}>
  <div class="grid gap-5">
    {#each groups as [title, items] (title)}
      <section class="grid gap-2">
        <h3 class="text-xs font-medium text-fg-muted">{title}</h3>
        <dl class="grid gap-1.5">
          {#each items as [keys, what] (what)}
            <div class="flex items-center justify-between gap-4 text-sm">
              <dt>{what}</dt>
              <dd class="flex gap-1">
                {#each keys as k (k)}<kbd class="kbd">{k}</kbd>{/each}
              </dd>
            </div>
          {/each}
        </dl>
      </section>
    {/each}
  </div>
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>Done</button>
  {/snippet}
</Modal>
