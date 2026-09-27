// WYSIWYG Markdown editing with Milkdown (ProseMirror + remark), loaded on
// demand. Everything stays in the browser: the text is read from the
// decrypted file and saved back as a new encrypted version by the caller.
//
// Raw HTML in a file is kept as an inert text node (Milkdown's `html` node
// renders its source as text), and pasted HTML is parsed through the
// editor's schema, so nothing outside it reaches the DOM.
//
// Images are shown as a placeholder, never loaded: a relative path would be
// requested from our own server, which would leak a plaintext file name,
// and a remote one would tell its host the file was opened. The Markdown
// keeps the image as written.

import { Editor, rootCtx, defaultValueCtx, editorViewCtx } from '@milkdown/kit/core';
import {
  commonmark,
  imageSchema,
  toggleStrongCommand,
  toggleEmphasisCommand,
  toggleInlineCodeCommand,
  wrapInHeadingCommand,
  wrapInBulletListCommand,
  wrapInOrderedListCommand,
  wrapInBlockquoteCommand,
  createCodeBlockCommand,
  insertHrCommand,
  toggleLinkCommand,
  turnIntoTextCommand,
} from '@milkdown/kit/preset/commonmark';
import { gfm, toggleStrikethroughCommand } from '@milkdown/kit/preset/gfm';
import { history, undoCommand, redoCommand } from '@milkdown/kit/plugin/history';
import { listener, listenerCtx } from '@milkdown/kit/plugin/listener';
import { clipboard } from '@milkdown/kit/plugin/clipboard';
import { callCommand, getMarkdown } from '@milkdown/kit/utils';

const safeImage = imageSchema.extendSchema((prev) => (ctx) => {
  const base = prev(ctx);
  return {
    ...base,
    toDOM: (node) => [
      'span',
      { class: 'md-image', title: "Images in Markdown files aren't loaded" },
      node.attrs.alt ? `Image: ${node.attrs.alt}` : 'Image',
    ],
    parseMarkdown: {
      ...base.parseMarkdown,
      // Milkdown passes null for a missing title, which its own schema rejects.
      runner: (state, node, type) => state.addNode(type, { src: node.url ?? '', alt: node.alt ?? '', title: node.title ?? '' }),
    },
  };
});

const COMMANDS = {
  undo: undoCommand,
  redo: redoCommand,
  bold: toggleStrongCommand,
  italic: toggleEmphasisCommand,
  strike: toggleStrikethroughCommand,
  code: toggleInlineCodeCommand,
  heading: wrapInHeadingCommand,
  paragraph: turnIntoTextCommand,
  bullets: wrapInBulletListCommand,
  numbers: wrapInOrderedListCommand,
  quote: wrapInBlockquoteCommand,
  codeBlock: createCodeBlockCommand,
  rule: insertHrCommand,
  link: toggleLinkCommand,
};

const MARKS = { bold: 'strong', italic: 'emphasis', strike: 'strike_through', code: 'inlineCode', link: 'link' };

/**
 * Mount an editor in `root` with `text`. `onChange(markdown)` fires on
 * every edit; `onSelection()` when the cursor moves (to refresh the
 * toolbar). Returns a handle.
 */
export async function createEditor(root, text, { onChange, onSelection }) {
  const editor = await Editor.make()
    .config((ctx) => {
      ctx.set(rootCtx, root);
      ctx.set(defaultValueCtx, text);
      const l = ctx.get(listenerCtx);
      l.markdownUpdated((_ctx, markdown, prev) => markdown !== prev && onChange?.(markdown));
      l.selectionUpdated(() => onSelection?.());
      l.updated(() => onSelection?.());
    })
    .use(commonmark)
    .use(safeImage)
    .use(gfm)
    .use(history)
    .use(listener)
    .use(clipboard)
    .create();

  const view = () => editor.ctx.get(editorViewCtx);

  return {
    /** Run a toolbar command, e.g. run('heading', 2). */
    run(name, payload) {
      editor.action(callCommand(COMMANDS[name].key, payload));
      view().focus();
    },
    markdown: () => editor.action(getMarkdown()),
    /** Which marks and block types the cursor is in, for the toolbar. */
    state() {
      const { state } = view();
      const { from, to, empty, $from } = state.selection;
      const marks = {};
      for (const [name, type] of Object.entries(MARKS)) {
        const mark = state.schema.marks[type];
        marks[name] = !!mark && (empty ? !!mark.isInSet(state.storedMarks || $from.marks()) : state.doc.rangeHasMark(from, to, mark));
      }
      const block = $from.parent;
      let list = null;
      for (let d = $from.depth; d > 0; d--) {
        const n = $from.node(d).type.name;
        if (n === 'bullet_list' || n === 'ordered_list') {
          list = n;
          break;
        }
      }
      return { ...marks, heading: block.type.name === 'heading' ? block.attrs.level : 0, codeBlock: block.type.name === 'code_block', list };
    },
    focus: () => view().focus(),
    destroy: () => editor.destroy(),
  };
}
