<script>
  // Used for "New folder" and "Rename".
  import { untrack } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { errorMessage } from '../../lib/ui.svelte.js';

  // `initial` is a suggestion when `create` is set; otherwise keeping it unchanged just closes.
  let { title, label = 'Name', initial = '', confirmLabel = 'Save', create = false, onsave, onclose } = $props();

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

  function validate(n) {
    if (!n) return 'Enter a name.';
    if (n === '.' || n === '..' || n.includes('/')) return 'Names cannot contain "/" or be "." or "..".';
    if (n.length > 255) return 'That name is too long.';
    return '';
  }

  async function submit() {
    const n = name.trim();
    error = validate(n);
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
    <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
    <button class="btn btn-primary" disabled={busy}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {confirmLabel}
    </button>
  {/snippet}
</Modal>
