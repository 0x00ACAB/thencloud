<script>
  // Tags on a file or folder: kept by node id in your encrypted app data,
  // so the server sees neither the tags nor what they're on.
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { tagsOf, setTags, allTags, cleanTag, MAX_TAG } from '../../lib/places.svelte.js';
  import { toastError } from '../../lib/ui.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';

  let { entry, onclose } = $props();

  let list = $state(untrack(() => [...tagsOf(entry.node.id)]));
  let text = $state('');
  let busy = $state(false);

  const lower = (s) => s.toLocaleLowerCase();
  // Tags used elsewhere that this item doesn't have yet, matching what's typed.
  const suggestions = $derived(
    allTags()
      .map((x) => x.tag)
      .filter((tag) => !list.some((x) => lower(x) === lower(tag)) && lower(tag).includes(lower(text.trim())))
      .slice(0, 8),
  );

  function add(tag) {
    const clean = cleanTag(tag);
    if (clean && !list.some((x) => lower(x) === lower(clean))) list = [...list, clean];
    text = '';
  }

  function keydown(e) {
    if ((e.key === 'Enter' || e.key === ',') && text.trim()) {
      e.preventDefault();
      add(text);
    } else if (e.key === 'Backspace' && !text && list.length) {
      list = list.slice(0, -1);
    }
  }

  async function save() {
    busy = true;
    try {
      await setTags(entry.node.id, text.trim() ? [...list, text] : list);
      onclose();
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={t('Tags')} description={t('On {name}. Only you see them.', { name: entry.meta.name })} {onclose} onsubmit={save} class="max-w-md">
  <div class="flex min-h-10 flex-wrap items-center gap-1.5 rounded-md border border-line px-2 py-1.5 focus-within:border-accent">
    {#each list as tag (tag)}
      <span class="badge gap-1 pr-1">
        {tag}
        <button type="button" class="grid size-4 cursor-pointer place-items-center rounded hover:bg-muted" aria-label={t('Remove {tag}', { tag })} onclick={() => (list = list.filter((x) => x !== tag))}>
          <Icon name="x" class="size-3" />
        </button>
      </span>
    {/each}
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="min-w-24 flex-1 bg-transparent text-sm outline-none"
      bind:value={text}
      onkeydown={keydown}
      maxlength={MAX_TAG}
      placeholder={list.length ? '' : t('Add a tag')}
      aria-label={t('Add a tag')}
      autocomplete="off"
      autofocus />
  </div>
  {#if suggestions.length}
    <div class="flex flex-wrap gap-1.5">
      {#each suggestions as tag (tag)}
        <button type="button" class="badge cursor-pointer hover:bg-muted" onclick={() => add(tag)}><Icon name="plus" class="size-3" />{tag}</button>
      {/each}
    </div>
  {/if}
  <p class="hint">{t('Press Enter after each tag.')}</p>
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
    <button class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {t('Save')}
    </button>
  {/snippet}
</Modal>
