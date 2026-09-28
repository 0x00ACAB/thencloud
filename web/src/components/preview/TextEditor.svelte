<script>
  // Plain-text and code editing: a textarea in the editor font. Tab indents
  // the way the file already does (tabs or two spaces).
  import { onMount, untrack } from 'svelte';
  import { t } from '../../lib/i18n.svelte.js';

  let { text, onchange } = $props();

  let area;
  const indent = untrack(() => (/^\t/m.test(text) ? '\t' : '  '));

  onMount(() => {
    area.value = untrack(() => text);
    area.focus();
    area.setSelectionRange(0, 0);
  });

  function onkeydown(e) {
    if (e.key !== 'Tab' || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    const { selectionStart: from, selectionEnd: to, value } = area;
    const lineStart = value.lastIndexOf('\n', from - 1) + 1;
    if (from === to && !e.shiftKey) {
      area.setRangeText(indent, from, to, 'end');
    } else {
      // Indent or outdent every selected line.
      const block = value.slice(lineStart, to);
      const lines = block.split('\n');
      const next = lines
        .map((l) => (e.shiftKey ? l.replace(new RegExp(`^(\\t| {1,${indent.length}})`), '') : indent + l))
        .join('\n');
      area.setRangeText(next, lineStart, to, 'preserve');
      area.setSelectionRange(lineStart, lineStart + next.length);
    }
    onchange(area.value);
  }
</script>

<textarea
  bind:this={area}
  class="block size-full resize-none bg-bg px-6 py-5 font-mono text-[13px] leading-5 text-fg outline-none"
  spellcheck="false"
  autocapitalize="off"
  autocomplete="off"
  aria-label={t('File contents')}
  wrap="off"
  {onkeydown}
  oninput={() => onchange(area.value)}></textarea>
