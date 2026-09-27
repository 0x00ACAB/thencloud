// Zip downloads of folders and selections, built in the browser: every file
// is decrypted here and stored uncompressed (most files people keep are
// compressed already, and storing keeps it fast). The zip is held in memory
// until it's saved, like any other download for now.

import { Zip, ZipPassThrough } from 'fflate';

/** Plain zip has 32-bit sizes and offsets. */
const ZIP_LIMIT = 4 * 1024 ** 3 - 1024 ** 2;

/**
 * A name that's safe as one path segment inside a zip. Names come from
 * whoever uploaded the file, so "../x" or "a/b" must not become paths that
 * escape the folder when someone unzips it.
 */
function segment(name) {
  const clean = name.replace(/[/\\\u0000]/g, '_').trim();
  return !clean || clean === '.' || clean === '..' ? '_' : clean;
}

/** `name`, or "name (2)", "name (3)"... if it's already in `taken`. */
function unique(name, taken) {
  let candidate = name;
  const dot = name.lastIndexOf('.');
  const [base, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ''];
  for (let i = 2; taken.has(candidate.toLowerCase()); i++) candidate = `${base} (${i})${ext}`;
  taken.add(candidate.toLowerCase());
  return candidate;
}

/**
 * Zip `entries` (files and folders, each { node, key, meta }).
 * `list(folderEntry)` returns a folder's children and `fetch(entry,
 * onProgress)` a file's decrypted { blob }. Reports progress from 0 to 1 by
 * bytes. Resolves to a Blob.
 */
export async function zipEntries(entries, { list, fetch, onProgress }) {
  // Walk the tree first, so progress can count bytes.
  const files = []; // { entry, path }
  const dirs = []; // paths of folders, so empty ones survive
  async function walk(items, prefix) {
    const taken = new Set();
    for (const entry of items) {
      const path = prefix + unique(segment(entry.meta.name), taken);
      if (entry.node.kind === 'folder') {
        dirs.push(`${path}/`);
        await walk(await list(entry), `${path}/`);
      } else {
        files.push({ entry, path });
      }
    }
  }
  await walk(entries, '');

  const total = files.reduce((n, f) => n + f.entry.meta.size, 0);
  if (total > ZIP_LIMIT) throw new Error('That is more than 4 GB, which is too much for one zip. Download it in parts.');

  const chunks = [];
  let failed = null;
  const zip = new Zip((err, data) => {
    if (err) failed = err;
    else chunks.push(data);
  });
  const add = (path, data, mtime) => {
    const f = new ZipPassThrough(path);
    if (mtime) f.mtime = new Date(mtime);
    zip.add(f);
    f.push(data, true);
  };

  for (const d of dirs) add(d, new Uint8Array(0));
  let done = 0;
  for (const { entry, path } of files) {
    const { blob } = await fetch(entry, (p) => onProgress?.(total ? (done + p * entry.meta.size) / total : 0));
    add(path, new Uint8Array(await blob.arrayBuffer()), entry.meta.mtime);
    done += entry.meta.size;
    onProgress?.(total ? done / total : 1);
    if (failed) throw failed;
  }
  zip.end();
  if (failed) throw failed;
  return new Blob(chunks, { type: 'application/zip' });
}
