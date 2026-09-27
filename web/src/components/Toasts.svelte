<script>
  import { fly } from 'svelte/transition';
  import { toasts, dismissToast } from '../lib/ui.svelte.js';
  import Icon from './Icon.svelte';

  const icons = { success: 'check', error: 'circle-alert', info: 'shield-check' };
</script>

<div class="pointer-events-none fixed inset-x-0 bottom-4 z-[60] flex flex-col items-center gap-2 px-4" aria-live="polite">
  {#each toasts as t (t.id)}
    <div
      transition:fly={{ y: 12, duration: 150 }}
      class="pointer-events-auto flex max-w-md items-start gap-2.5 rounded-lg bg-fg px-3.5 py-2.5 text-[13px] text-bg shadow-lg shadow-black/10">
      <Icon name={icons[t.kind]} class="mt-px size-4 shrink-0 {t.kind === 'error' ? 'text-danger' : t.kind === 'success' ? 'text-success' : 'opacity-70'}" />
      <span class="leading-5">{t.message}</span>
      <button type="button" class="-mr-1 ml-1 cursor-pointer rounded opacity-60 hover:opacity-100" aria-label="Dismiss" onclick={() => dismissToast(t.id)}>
        <Icon name="x" class="size-4" />
      </button>
    </div>
  {/each}
</div>
