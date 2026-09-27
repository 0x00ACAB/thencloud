<script>
  // Rendered Markdown. The renderer (marked + DOMPurify) and highlight.js
  // for fenced code are loaded on demand; see lib/markdown.js for what is
  // stripped and why.
  let { text } = $props();

  let article = $state();
  let failed = $state(false);

  $effect(() => {
    const el = article;
    let live = true;
    failed = false;
    Promise.all([import('../../lib/markdown.js'), import('../../lib/highlight.js')])
      .then(([md, hl]) => {
        if (!live) return;
        el.replaceChildren(md.renderMarkdown(text));
        hl.highlightBlocks(el);
      })
      .catch(() => live && (failed = true));
    return () => (live = false);
  });
</script>

<div class="h-full overflow-auto">
  {#if failed}
    <p class="p-6 text-center text-[13px] text-fg-muted">Couldn't render this file. Switch to Source to read it.</p>
  {/if}
  <article bind:this={article} class="prose mx-auto max-w-3xl px-6 py-10"></article>
</div>
