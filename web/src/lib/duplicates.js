// Finding duplicate files: identical plaintext content under different
// names or folders. The server only ever sees ciphertext, unique per file
// even for identical content, so this can only be worked out here, by
// reading and hashing each candidate.
import { walkTree, fetchEntry } from './cloud.svelte.js';

/** Files larger than this are skipped: hashing reads the whole file into memory first. */
export const MAX_HASH_SIZE = 512 * 1024 * 1024;

async function sha256(blob) {
  const digest = await crypto.subtle.digest('SHA-256', await blob.arrayBuffer());
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('');
}

/**
 * Groups of files under `top` with identical content, largest first; each
 * group is sorted oldest first. `onProgress(done, total)` counts files
 * actually hashed: only ones that share their size with at least one
 * other file can be duplicates, so most of a folder is never downloaded.
 * Files over MAX_HASH_SIZE, or that fail to read, are left out.
 */
export async function findDuplicates(top, { signal, onProgress } = {}) {
  const bySize = new Map();
  await walkTree(top, {
    signal,
    onEntry: (r) => {
      if (r.node.kind === 'folder' || !r.meta.size || r.meta.size > MAX_HASH_SIZE) return;
      let group = bySize.get(r.meta.size);
      if (!group) bySize.set(r.meta.size, (group = []));
      group.push(r);
    },
  });
  const candidates = [...bySize.values()].filter((g) => g.length > 1).flat();
  const byHash = new Map();
  let done = 0;
  for (const entry of candidates) {
    if (signal?.aborted) break;
    try {
      const { blob } = await fetchEntry(entry);
      const hash = await sha256(blob);
      let group = byHash.get(hash);
      if (!group) byHash.set(hash, (group = []));
      group.push(entry);
    } catch {
      // Unreadable: not counted as a duplicate of anything.
    }
    onProgress?.(++done, candidates.length);
  }
  return [...byHash.values()]
    .filter((g) => g.length > 1)
    .map((g) => g.sort((a, b) => a.meta.mtime - b.meta.mtime))
    .sort((a, b) => b[0].meta.size - a[0].meta.size);
}
