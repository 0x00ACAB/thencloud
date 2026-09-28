// Searching inside files. Each indexed file is kept as the set of words in
// it (lower case, Unicode NFC, one space between them, sorted), in the
// encrypted "search" app data; the server never sees them. A search matches
// a file when every word typed starts some word in it.

/** Words longer than this are cut (they're rarely searched for in full). */
const MAX_WORD = 32;
/** Words kept per file: plenty for finding it, and keeps the index small. */
export const MAX_WORDS = 4000;

const WORD = /[\p{L}\p{N}][\p{L}\p{N}\p{M}'’_-]*/gu;

/** The distinct words in `text`, sorted, joined by spaces. */
export function wordsOf(text) {
  const seen = new Set();
  for (const m of text.normalize('NFC').toLowerCase().matchAll(WORD)) {
    const w = m[0].replace(/['’_-]+$/, '').slice(0, MAX_WORD);
    if (w.length < 2) continue;
    seen.add(w);
    if (seen.size >= MAX_WORDS) break;
  }
  return [...seen].sort().join(' ');
}

/** The words of a query, as wordsOf reads them (without the cap; one-letter words aren't kept either). */
export function queryWords(query) {
  const out = [];
  for (const m of query.normalize('NFC').toLowerCase().matchAll(WORD)) {
    const w = m[0].replace(/['’_-]+$/, '').slice(0, MAX_WORD);
    if (w.length >= 2 && !out.includes(w)) out.push(w);
  }
  return out;
}

/** True when each query word starts a word in `words` (from wordsOf). */
export function matchesWords(words, query) {
  if (!query.length) return false;
  const hay = ` ${words}`;
  return query.every((q) => hay.includes(` ${q}`));
}
