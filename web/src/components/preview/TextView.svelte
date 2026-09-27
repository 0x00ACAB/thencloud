<script>
  // Plain text and source code, with line numbers. Shown unhighlighted at
  // first, then highlighted once highlight.js has loaded; very large files
  // stay plain so the tab doesn't lock up.
  import { languageFor } from '../../lib/languages.js';

  let { text, name } = $props();

  const HIGHLIGHT_LIMIT = 300 * 1024;

  let html = $state(null);
  const lines = $derived(text.endsWith('\n') ? text.slice(0, -1).split('\n').length : text.split('\n').length);
  const numbers = $derived(Array.from({ length: lines }, (_, i) => i + 1).join('\n'));

  $effect(() => {
    html = null;
    const lang = languageFor(name);
    if (!lang || text.length > HIGHLIGHT_LIMIT) return;
    let live = true;
    import('../../lib/highlight.js').then((m) => live && (html = m.highlight(text, lang)));
    return () => (live = false);
  });
</script>

<div class="h-full overflow-auto">
  <div class="flex min-w-max py-4 font-mono text-[13px] leading-5">
    <pre class="sticky left-0 shrink-0 border-r border-line bg-bg pr-3 pl-4 text-right text-fg-faint select-none" aria-hidden="true">{numbers}</pre>
    {#if html !== null}
      <pre class="code pr-6 pl-4"><code>{@html html}</code></pre>
    {:else}
      <pre class="code pr-6 pl-4"><code>{text}</code></pre>
    {/if}
  </div>
</div>
