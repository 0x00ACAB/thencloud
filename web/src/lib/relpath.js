// Find a file by a path relative to a folder, the way a Markdown image like
// ![](img/photo.png) points at a sibling. Names are compared after
// decryption, in the browser; the path itself is never sent anywhere.

/**
 * `trail`: folder entries from the top down to the one the path is relative
 * to. `list(folderEntry)` resolves to its children. Resolves to the file's
 * entry, or null (absolute paths and links to other sites never match).
 */
export async function findRelative(trail, path, list) {
  let clean = path.split(/[?#]/)[0];
  try {
    clean = decodeURIComponent(clean);
  } catch {
    return null;
  }
  if (!clean || clean.startsWith('/') || /^[a-z][a-z0-9+.-]*:/i.test(clean)) return null;
  const parts = clean.split('/').filter((p) => p && p !== '.');
  const stack = trail.slice();
  for (const [i, part] of parts.entries()) {
    if (part === '..') {
      if (stack.length < 2) return null;
      stack.pop();
      continue;
    }
    const kids = await list(stack[stack.length - 1]);
    const hit = kids.find((k) => k.meta.name === part) ?? kids.find((k) => k.meta.name.toLowerCase() === part.toLowerCase());
    if (!hit) return null;
    if (i === parts.length - 1) return hit.node.kind === 'file' ? hit : null;
    if (hit.node.kind !== 'folder') return null;
    stack.push(hit);
  }
  return null;
}
