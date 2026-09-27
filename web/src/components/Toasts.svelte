<script>
  import { fly, flip, flipParams } from '../lib/motion.js';
  import { toasts, dismissToast } from '../lib/ui.svelte.js';
  import Icon from './Icon.svelte';

  const icons = { success: 'check', error: 'circle-alert', info: 'check' };
</script>

<div class="pointer-events-none fixed inset-x-0 bottom-4 z-[60] flex flex-col items-center gap-2 px-4" aria-live="polite">
  {#each toasts as t (t.id)}
    <div
      in:fly={{ y: 12 }}
      out:fly={{ y: 6, duration: 120 }}
      animate:flip={flipParams()}
      class="pointer-events-auto flex max-w-md items-start gap-2.5 rounded-lg bg-fg px-3.5 py-2.5 text-[13px] text-bg shadow-lg shadow-black/10 dark:border dark:border-line-strong dark:bg-muted dark:text-fg dark:shadow-black/50">
      <Icon name={t.icon ?? icons[t.kind]} class="mt-px size-4 shrink-0 {t.kind === 'error' ? 'text-danger' : t.kind === 'success' ? 'text-success' : 'opacity-70'}" />
      <span class="leading-5">{t.message}</span>
      {#if t.action}
        <button
          type="button"
          class="-my-0.5 ml-2 cursor-pointer rounded px-1.5 py-0.5 font-medium underline-offset-4 hover:underline"
          onclick={() => {
            dismissToast(t.id);
            t.action.onclick();
          }}>{t.action.label}</button>
      {/if}
      <button type="button" class="-mr-1 ml-1 cursor-pointer rounded opacity-60 hover:opacity-100" aria-label="Dismiss" onclick={() => dismissToast(t.id)}>
        <Icon name="x" class="size-4" />
      </button>
    </div>
  {/each}
</div>
