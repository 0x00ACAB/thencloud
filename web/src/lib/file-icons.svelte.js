// File-type icon packs. "Minimal" is the app's own monochrome Lucide set;
// the others are IDE icon packs built by scripts/file-icons-plugin.mjs. A
// pack's lookup tables are imported only once it's chosen, and the browser
// fetches only the icons it actually shows.

import { previewKind } from './preview.js';

export const ICON_PACKS = [
  { id: 'minimal', name: 'Minimal', credit: 'Lucide' },
  { id: 'material', name: 'Material', credit: 'Material Icon Theme' },
  { id: 'symbols', name: 'Symbols', credit: 'Symbols by Miguel Solorio' },
  { id: 'documents', name: 'Documents', credit: 'file-icon-vectors' },
];

const LOADERS = {
  material: () => import('virtual:file-icons/material'),
  symbols: () => import('virtual:file-icons/symbols'),
  documents: () => import('virtual:file-icons/documents'),
};

const tables = $state({}); // pack id -> tables, once loaded
const pending = {};

function ensure(pack) {
  if (!LOADERS[pack] || tables[pack] || pending[pack]) return;
  pending[pack] = LOADERS[pack]()
    .then((m) => (tables[pack] = m.default))
    .catch(() => delete pending[pack]);
}

/**
 * URL of the icon for a file in `pack`, or null to use the Minimal icon
 * (the pack is Minimal, or its tables are still loading).
 */
export function fileIconUrl(pack, meta, dark) {
  ensure(pack);
  const t = tables[pack];
  if (!t) return null;
  const name = meta.name.toLowerCase();
  let id = t.names[name];
  // Longest extension first: "x.test.d.ts" tries "test.d.ts", "d.ts", "ts".
  for (let i = name.indexOf('.', 1); !id && i !== -1; i = name.indexOf('.', i + 1)) id = t.extensions[name.slice(i + 1)];
  if (!id) {
    const kind = previewKind(meta)?.kind;
    const mime = (meta.mime || '').split('/')[0];
    id = t.mime[kind] ?? t.mime[mime] ?? t.fallback;
  }
  if (!dark && t.light[id]) id = t.light[id];
  return `/file-icons/${pack}/${encodeURIComponent(id)}.svg`;
}
