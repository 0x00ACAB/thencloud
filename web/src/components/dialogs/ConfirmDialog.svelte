<script>
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import { errorMessage } from '../../lib/ui.svelte.js';

  let { title, description, confirmLabel = 'Confirm', danger = false, disabled = false, onconfirm, onclose, children } = $props();

  let busy = $state(false);
  let error = $state('');

  async function submit() {
    busy = true;
    error = '';
    try {
      await onconfirm();
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal {title} {description} {onclose} onsubmit={submit}>
  {@render children?.()}
  {#if error}<p class="text-[13px] text-danger">{error}</p>{/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>Cancel</button>
    <button class="btn {danger ? 'btn-danger' : 'btn-primary'}" disabled={busy || disabled}>
      {#if busy}<Icon name="loader-circle" class="spinner" />{/if}
      {confirmLabel}
    </button>
  {/snippet}
</Modal>
