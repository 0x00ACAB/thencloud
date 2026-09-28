// Photos: a folder of images shown as a timeline by date taken, and as
// albums by folder. Only the folder's node id is remembered in this browser
// (with the user id); the list lives in memory and goes when the signed-in
// view does. Dates come from the encrypted metadata: `taken`, read from
// EXIF at upload, else the file's modified time. Nothing is downloaded to
// build the timeline; tiles use the encrypted thumbnails.
import { session, resolvePath, walkTree } from './cloud.svelte.js';
import { errorMessage } from './ui.svelte.js';
import { previewKind } from './preview.js';

function readRoot() {
  try {
    const v = JSON.parse(localStorage.getItem('photosRoot'));
    return v?.user === session.me?.user_id ? v.id : null;
  } catch {
    return null;
  }
}

/** `list` is [{ ...entry, location: [folder names below the root], at }], newest first. */
export const photos = $state({ rootId: null, rootName: '', list: null, scanning: false, error: '' });

let scan = null;

/** When a photo was taken, as best we know (ms). */
export const takenAt = (entry) => entry.meta.taken ?? entry.meta.mtime ?? entry.node.created_at * 1000;

export function openPhotos() {
  photos.rootId ??= readRoot();
  if (photos.rootId && !photos.list && !photos.scanning) scanPhotos();
}

export function setPhotosRoot(id) {
  photos.rootId = id;
  photos.list = null;
  try {
    localStorage.setItem('photosRoot', JSON.stringify({ user: session.me.user_id, id }));
  } catch {
    /* private mode */
  }
  return scanPhotos();
}

export async function scanPhotos() {
  if (!photos.rootId) return;
  scan?.abort();
  const ctl = (scan = new AbortController());
  Object.assign(photos, { scanning: true, error: '' });
  try {
    const { items } = await resolvePath(photos.rootId);
    const root = items[items.length - 1];
    if (root.node.kind !== 'folder') throw new Error('The photos folder is not a folder');
    const found = [];
    await walkTree(root, {
      signal: ctl.signal,
      onEntry: (r) => {
        if (r.node.kind === 'file' && previewKind(r.meta).kind === 'image') found.push({ ...r, location: r.location.slice(1), at: takenAt(r) });
      },
    });
    if (ctl.signal.aborted) return;
    photos.rootName = root.meta.name;
    photos.list = found.sort((a, b) => b.at - a.at);
  } catch (e) {
    if (!ctl.signal.aborted) photos.error = errorMessage(e);
  } finally {
    if (scan === ctl) photos.scanning = false;
  }
}

const monthFmt = new Intl.DateTimeFormat(undefined, { month: 'long', year: 'numeric' });

/** Photos by month, newest first: [{ key, label, items }]. */
export function byMonth(list) {
  const groups = [];
  for (const p of list) {
    const d = new Date(p.at);
    const key = `${d.getFullYear()}-${d.getMonth()}`;
    if (groups.at(-1)?.key !== key) groups.push({ key, label: monthFmt.format(d), items: [] });
    groups.at(-1).items.push(p);
  }
  return groups;
}

/** Albums: one per folder that holds photos, most recent first. [{ key, name, location, items }] */
export function albums(list) {
  const map = new Map();
  for (const p of list) {
    const key = p.location.join('/');
    if (!map.has(key)) map.set(key, { key, name: p.location.at(-1) ?? photos.rootName, location: p.location, items: [] });
    map.get(key).items.push(p);
  }
  return [...map.values()];
}

export function unloadPhotos() {
  scan?.abort();
  Object.assign(photos, { rootId: null, rootName: '', list: null, scanning: false, error: '' });
}
