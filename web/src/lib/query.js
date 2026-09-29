// Search box filters: `type:pdf tag:invoices report` is the word "report"
// in the name (or, searching inside files, the text) of PDFs tagged
// "invoices". A value with spaces goes in quotes (tag:"tax 2026"). Several
// types match any of them; several tags must all be there.

const KINDS = new Set(['folder', 'file', 'image', 'video', 'audio', 'pdf', 'text', 'markdown', 'book']);

/** { text, types: [lower case], tags: [as typed] } */
export function parseQuery(input) {
  const out = { text: '', types: [], tags: [] };
  const words = [];
  for (const m of String(input).matchAll(/(\w+):"([^"]*)"?|(\w+):(\S+)|"([^"]*)"?|(\S+)/g)) {
    const key = (m[1] ?? m[3])?.toLowerCase();
    const value = m[2] ?? m[4];
    if (key === 'type' && value) out.types.push(value.toLowerCase().replace(/^\./, ''));
    else if (key === 'tag' && value) out.tags.push(value.trim());
    else words.push(m[5] ?? m[0]);
  }
  out.text = words.join(' ').trim();
  return out;
}

export const hasFilters = (q) => q.types.length > 0 || q.tags.length > 0;

/**
 * Whether `entry` passes the filters (the text is matched elsewhere).
 * `kind(meta)` is the preview kind ('image', 'pdf'...), `ext(name)` the
 * extension, `tagsOf(id)` the item's tags.
 */
export function passes(entry, q, { kind, ext, tagsOf }) {
  if (q.types.length) {
    const folder = entry.node.kind === 'folder';
    const k = folder ? 'folder' : (kind(entry.meta) ?? 'file');
    const e = folder ? '' : ext(entry.meta.name);
    const ok = q.types.some((t) => (KINDS.has(t) ? t === k || (t === 'file' && !folder) : t === e));
    if (!ok) return false;
  }
  if (q.tags.length) {
    const have = tagsOf(entry.node.id).map((t) => t.toLocaleLowerCase());
    if (!q.tags.every((t) => have.includes(t.toLocaleLowerCase()))) return false;
  }
  return true;
}
