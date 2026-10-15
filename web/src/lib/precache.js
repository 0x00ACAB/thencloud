// Getting the next track ready before the current one ends (music.svelte.js):
// which track that is, and how much of it to decrypt ahead of time. Pure, so
// tests/precache.test.js can run them under Node.

/** A whole track is decrypted ahead when it's at most this big... */
export const PRECACHE_WHOLE = 48 * 1024 * 1024;
/** ...otherwise (audiobooks, long mixes) only this many pieces: enough to start. */
export const PRECACHE_START = 2;

/**
 * The queue index that plays when the one at `index` ends, or -1 for none:
 * repeat-one plays the same track again (nothing to get ready), and only
 * repeat-all wraps round to the start.
 */
export function upcomingIndex(length, index, repeat) {
  if (index < 0 || index >= length || repeat === 'one') return -1;
  if (index + 1 < length) return index + 1;
  return repeat === 'all' && length > 1 ? 0 : -1;
}

/**
 * How many of a file's `count` pieces to decrypt ahead: all of a file up to
 * PRECACHE_WHOLE, the first PRECACHE_START of a bigger one, and only the
 * first when the device asks to save data.
 */
export function piecesAhead({ size, count }, { saveData = false } = {}) {
  if (count <= 0) return 0;
  if (saveData) return 1;
  return size <= PRECACHE_WHOLE ? count : Math.min(count, PRECACHE_START);
}
