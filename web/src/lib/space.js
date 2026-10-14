// What takes space under My files, for the storage analyzer. Sizes come
// from each file's encrypted metadata (the plaintext size of its current
// version), decrypted here while walking the tree; nothing is downloaded,
// and the server learns nothing it didn't already have.
import { walkTree } from './cloud.svelte.js';
import { previewKind } from './preview.js';

/** The groups in "By type", in the order of the legend. */
export const KINDS = ['image', 'video', 'audio', 'document', 'archive', 'text', 'other'];

const ARCHIVES = new Set(['zip', '7z', 'rar', 'tar', 'gz', 'tgz', 'bz2', 'xz', 'zst', 'iso', 'dmg']);

export function kindOf(meta) {
  const k = previewKind(meta)?.kind;
  if (k === 'image' || k === 'video' || k === 'audio' || k === 'text') return k;
  if (k === 'pdf' || k === 'office' || k === 'book' || k === 'markdown') return 'document';
  if (k === 'archive') return 'archive';
  const ext = meta.name.includes('.') ? meta.name.split('.').pop().toLowerCase() : '';
  return ARCHIVES.has(ext) ? 'archive' : 'other';
}

/**
 * Walk `top` and build `{ id, name, size, files, children, entry }` for it
 * and every folder below (files are `{ id, name, size, kind, entry }`),
 * folder sizes and file counts summed from what's inside.
 * `onProgress({ files, folders })` as it goes.
 */
export async function measureTree(top, { signal, onProgress } = {}) {
  const root = { id: top.node.id, name: null, entry: top, size: 0, files: 0, children: [] };
  const folders = new Map([[root.id, root]]);
  const pending = []; // entries whose folder hasn't been seen yet
  const count = { files: 0, folders: 0 };
  const attach = (r) => {
    const parent = folders.get(r.parentId);
    if (!parent) return false;
    if (r.node.kind === 'folder') {
      const f = folders.get(r.node.id) ?? { id: r.node.id, size: 0, files: 0, children: [] };
      Object.assign(f, { name: r.meta.name, entry: r });
      folders.set(f.id, f);
      parent.children.push(f);
    } else {
      parent.children.push({ id: r.node.id, name: r.meta.name, size: r.meta.size || 0, kind: kindOf(r.meta), entry: r });
    }
    return true;
  };
  await walkTree(top, {
    signal,
    onEntry: (r) => {
      // walkTree lists a folder only after reporting it, so the parent is
      // always known; keep the fallback anyway rather than drop a branch.
      if (!attach(r)) pending.push(r);
      if (r.node.kind === 'folder') count.folders++;
      else count.files++;
      onProgress?.({ ...count });
    },
  });
  for (const r of pending) attach(r);
  // Sum bottom up without recursion: a deep tree mustn't blow the stack.
  const order = [];
  const stack = [root];
  while (stack.length) {
    const f = stack.pop();
    order.push(f);
    for (const c of f.children) if (c.children) stack.push(c);
  }
  for (let i = order.length - 1; i >= 0; i--) {
    const f = order[i];
    f.size = 0;
    f.files = 0;
    for (const c of f.children) {
      f.size += c.size;
      f.files += c.children ? c.files : 1;
    }
  }
  return root;
}

/** Every file under `folder`, for "By type". */
export function allFiles(folder) {
  const out = [];
  const stack = [folder];
  while (stack.length) {
    for (const c of stack.pop().children) {
      if (c.children) stack.push(c);
      else out.push(c);
    }
  }
  return out;
}

/** `{ kind, size, files: [file] }` per kind under `folder`, largest first, empty kinds left out. */
export function byKind(folder) {
  const groups = new Map(KINDS.map((kind) => [kind, { kind, size: 0, files: [] }]));
  for (const f of allFiles(folder)) {
    const g = groups.get(f.kind);
    g.size += f.size;
    g.files.push(f);
  }
  return [...groups.values()].filter((g) => g.files.length).sort((a, b) => b.size - a.size);
}
