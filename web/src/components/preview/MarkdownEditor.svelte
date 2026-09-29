<script>
  // Rich-text Markdown editing (lib/editor.js, loaded on demand). The parent
  // owns saving; this reports every change through `onchange`.
  import { onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { t } from '../../lib/i18n.svelte.js';

  let { text, onchange, onready } = $props();

  let root;
  let editor = $state(null);
  let failed = $state(false);
  let active = $state({});
  let linking = $state(false);
  let href = $state('');
  let linkInput = $state();

  onMount(() => {
    let live = true;
    let handle;
    import('../../lib/editor.js')
      .then((m) =>
        m.createEditor(root, text, {
          onChange: (md) => onchange(md),
          onSelection: () => handle && (active = handle.state()),
        }),
      )
      .then((h) => {
        handle = h;
        if (!live) return h.destroy();
        editor = h;
        active = h.state();
        onready?.(h);
        h.focus();
      })
      .catch(() => live && (failed = true));
    return () => {
      live = false;
      handle?.destroy();
    };
  });

  const run = (name, payload) => editor?.run(name, payload);

  function startLink() {
    if (active.link) return run('link');
    linking = true;
    href = 'https://';
    queueMicrotask(() => linkInput?.select());
  }

  function applyLink(e) {
    e.preventDefault();
    linking = false;
    if (/^(https?:|mailto:)/i.test(href)) run('link', { href });
  }

  const groups = $derived([
    [
      ['undo', 'undo-2', t('Undo')],
      ['redo', 'redo-2', t('Redo')],
    ],
    [
      ['h1', 'heading-1', t('Heading 1')],
      ['h2', 'heading-2', t('Heading 2')],
      ['h3', 'heading-3', t('Heading 3')],
    ],
    [
      ['bold', 'bold', t('Bold')],
      ['italic', 'italic', t('Italic')],
      ['strike', 'strikethrough', t('Strikethrough')],
      ['code', 'code', t('Inline code')],
      ['link', 'link', t('Link')],
    ],
    [
      ['bullets', 'list', t('Bulleted list')],
      ['numbers', 'list-ordered', t('Numbered list')],
      ['task', 'list-checks', t('Task list')],
      ['quote', 'text-quote', t('Quote')],
      ['codeBlock', 'square-code', t('Code block')],
      ['rule', 'minus', t('Divider')],
      ['table', 'table', t('Table')],
    ],
  ]);

  // Shown while the cursor is in a table.
  const tableTools = $derived([
    ['addRow', 'between-horizontal-end', t('Add row below')],
    ['addCol', 'between-vertical-end', t('Add column to the right')],
    ['deleteRow', 'grid-2x2-x', t('Delete row')],
    ['deleteCol', 'columns-2', t('Delete column')],
    ['deleteTable', 'trash-2', t('Delete table')],
  ]);

  function press(id) {
    if (id === 'link') return startLink();
    const level = { h1: 1, h2: 2, h3: 3 }[id];
    if (level) return active.heading === level ? run('paragraph') : run('heading', level);
    if (id === 'table') return active.table || run('table', { row: 3, col: 3 });
    run(id);
  }

  function isOn(id) {
    const level = { h1: 1, h2: 2, h3: 3 }[id];
    if (level) return active.heading === level;
    if (id === 'bullets') return active.list === 'bullet_list';
    if (id === 'numbers') return active.list === 'ordered_list';
    if (id === 'task') return active.list === 'task';
    return !!active[id];
  }
</script>

<div class="flex h-full flex-col">
  <div class="flex shrink-0 flex-wrap items-center gap-1 border-b border-line px-3 py-1.5" role="toolbar" aria-label={t('Formatting')}>
    {#each groups as group, g (g)}
      {#if g}<span class="mx-1 h-5 w-px bg-line" aria-hidden="true"></span>{/if}
      {#each group as [id, icon, label] (id)}
        <button
          type="button"
          class="btn btn-ghost btn-icon h-7 w-7 {isOn(id) ? 'bg-muted text-fg' : ''}"
          aria-label={label}
          aria-pressed={['undo', 'redo', 'rule'].includes(id) ? undefined : isOn(id)}
          title={label}
          disabled={!editor}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => press(id)}>
          <Icon name={icon} />
        </button>
      {/each}
    {/each}
    {#if active.table}
      <span class="mx-1 h-5 w-px bg-line" aria-hidden="true"></span>
      {#each tableTools as [id, icon, label] (id)}
        <button
          type="button"
          class="btn btn-ghost btn-icon h-7 w-7"
          aria-label={label}
          title={label}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => run(id)}>
          <Icon name={icon} />
        </button>
      {/each}
    {/if}
    {#if linking}
      <form class="ml-2 flex items-center gap-1" onsubmit={applyLink}>
        <input bind:this={linkInput} class="input h-7 w-64 text-[13px]" bind:value={href} aria-label={t('Link address')} onkeydown={(e) => e.key === 'Escape' && (e.stopPropagation(), e.preventDefault(), (linking = false), editor?.focus())} />
        <button class="btn btn-secondary h-7 px-2.5 text-[13px]">{t('Add link')}</button>
      </form>
    {/if}
  </div>
  <div class="min-h-0 flex-1 overflow-auto">
    {#if failed}
      <p class="p-6 text-center text-[13px] text-fg-muted">{t("The editor couldn't be loaded. Your file hasn't changed.")}</p>
    {/if}
    <div bind:this={root} class="editor prose mx-auto max-w-3xl px-6 py-10"></div>
  </div>
</div>
