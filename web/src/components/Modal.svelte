<script>
  // A native <dialog>, opened as soon as it mounts. The parent unmounts it
  // from `onclose` (fired on Esc, backdrop click, or programmatic close).
  import { onMount } from 'svelte';

  let { title, description, onclose, onsubmit, children, footer, class: cls = '' } = $props();

  let dlg;
  const titleId = `modal-${Math.random().toString(36).slice(2)}`;

  onMount(() => dlg.showModal());

  function submit(e) {
    e.preventDefault();
    onsubmit?.(e);
  }
</script>

{#snippet content()}
  <div class="modal-body">
    <div class="grid gap-1">
      <h2 id={titleId} class="text-base font-semibold tracking-tight">{title}</h2>
      {#if description}<p class="hint">{description}</p>{/if}
    </div>
    {@render children?.()}
  </div>
  {#if footer}
    <div class="modal-footer">{@render footer()}</div>
  {/if}
{/snippet}

<dialog
  bind:this={dlg}
  class="modal {cls}"
  aria-labelledby={titleId}
  onclose={() => onclose?.()}
  onclick={(e) => e.target === dlg && dlg.close()}>
  {#if onsubmit}
    <form onsubmit={submit}>{@render content()}</form>
  {:else}
    <div>{@render content()}</div>
  {/if}
</dialog>
