<script>
  // Rendered Markdown. The renderer (marked + DOMPurify) and highlight.js
  // for fenced code are loaded on demand; see lib/markdown.js for what is
  // stripped and why. `loadImage(path)`, when given, turns a relative image
  // path into a blob: URL of a decrypted file (or null). `ontoggle(index,
  // checked)`, when given, makes task list checkboxes clickable.
  import { t } from '../../lib/i18n.svelte.js';

  let { text, loadImage = null, ontoggle = null } = $props();

  let article = $state();
  let failed = $state(false);

  $effect(() => {
    const el = article;
    let live = true;
    const urls = [];
    failed = false;
    Promise.all([import('../../lib/markdown.js'), import('../../lib/highlight.js')])
      .then(([md, hl]) => {
        if (!live) return;
        el.replaceChildren(md.renderMarkdown(text));
        hl.highlightBlocks(el);
        if (loadImage) showImages(el, urls, () => live);
        if (ontoggle) {
          el.querySelectorAll('li > input[type="checkbox"]').forEach((box, i) => {
            box.disabled = false;
            box.onchange = () => ontoggle(i, box.checked);
          });
        }
      })
      .catch(() => live && (failed = true));
    return () => {
      live = false;
      for (const u of urls) URL.revokeObjectURL(u);
    };
  });

  function showImages(el, urls, live) {
    for (const span of el.querySelectorAll('.md-image[data-path]')) {
      span.title = t('Loading image');
      loadImage(span.dataset.path)
        .then((url) => {
          if (!url) return (span.title = t('No image found at {path}', { path: span.dataset.path }));
          if (!live()) return URL.revokeObjectURL(url);
          urls.push(url);
          const img = document.createElement('img');
          img.src = url;
          img.alt = span.dataset.alt ?? '';
          span.replaceWith(img);
        })
        .catch(() => (span.title = t("Couldn't load {path}", { path: span.dataset.path })));
    }
  }
</script>
<div class="h-full overflow-auto">
  {#if failed}
    <p class="p-6 text-center text-[13px] text-fg-muted">{t("Couldn't render this file. Switch to Source to read it.")}</p>
  {/if}
  <article bind:this={article} class="prose mx-auto max-w-3xl px-6 py-10"></article>
</div>
