<script>
  import { t } from '../../lib/i18n.svelte.js';
  // Asks for a name: "New folder" and "New note". (Renaming happens in place.)
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { errorMessage } from '../../lib/ui.svelte.js';
  import { nameError } from '../../lib/format.js';

  // `initial` is a suggestion when `create` is set; otherwise keeping it unchanged just closes.
  let { title, label = t('Name'), initial = '', confirmLabel = t('Save'), create = false, onsave, onclose } = $props();

  let name = $state(untrack(() => initial));
  let busy = $state(false);
  let error = $state('');
  let input;

  $effect(() => {
    input.focus();
    // Select the name without the extension, like a file manager would.
    const dot = initial.lastIndexOf('.');
    input.setSelectionRange(0, dot > 0 ? dot : initial.length);
  });

  async function submit() {
    const n = name.trim();
    error = nameError(n);
    if (error) return;
    if (n === initial && !create) return onclose();
    busy = true;
    try {
      await onsave(n);
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal {title} {onclose} onsubmit={submit}>
  <div class="field">
    <label class="label" for="name-input">{label}</label>
    <input id="name-input" bind:this={input} class="input" bind:value={name} autocomplete="off" spellcheck="false" />
    {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  </div>
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Cancel')}</button>
    <button class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {confirmLabel}
    </button>
  {/snippet}
</Modal>
