<script>
  import Modal from '../Modal.svelte';
  import { t } from '../../lib/i18n.svelte.js';

  let { onclose } = $props();

  const groups = $derived([
    [
      t('Files'),
      [
        [['/'], t('Search this folder')],
        [['j', '↓'], t('Next item')],
        [['k', '↑'], t('Previous item')],
        [['Enter'], t('Open or preview')],
        [['x'], t('Select or unselect')],
        [['Esc'], t('Clear the selection')],
        [['F2'], t('Rename')],
        [['Backspace'], t('Up to the parent folder')],
        [['n'], t('New folder')],
        [['u'], t('Upload files')],
        [['Delete'], t('Move the selection, or this item, to trash')],
      ],
    ],
    [
      t('Preview'),
      [
        [['←', '→'], t('Previous or next file')],
        [['Ctrl', 'S'], t('Save while editing')],
        [['Esc'], t('Close')],
      ],
    ],
  ]);
</script>

<Modal title={t('Keyboard shortcuts')} {onclose}>
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
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Done')}</button>
  {/snippet}
</Modal>
