// Favourites, recently opened files and tags. Only node ids are kept, in the
// encrypted app data ("files"), so the server learns neither which files
// they are nor their names; names are decrypted here by resolving each id's
// path, and kept in memory only.
import { resolvePath, loadAppData, saveAppData, folderLabel } from './cloud.svelte.js';

const MAX_RECENT = 30;

/** `favourites`: [node id]; `recent`: [{ id, at }], newest first; `tags`: { node id: [tag] }. */
export const places = $state({ favourites: [], recent: [], tags: {}, loaded: false });

let loading = null;

export function loadPlaces() {
  loading ??= loadAppData('files').then(
    (d) => {
      places.favourites = d.favourites ?? [];
      places.recent = d.recent ?? [];
      places.tags = d.tags ?? {};
      places.loaded = true;
    },
    (e) => {
      loading = null;
      throw e;
    },
  );
  return loading;
}

export const isFavourite = (id) => places.favourites.includes(id);

/** Star or unstar a node. Resolves to whether it's now a favourite. */
export async function toggleFavourite(id) {
  const on = !isFavourite(id);
  const apply = (list = []) => (on ? [...list.filter((x) => x !== id), id] : list.filter((x) => x !== id));
  places.favourites = apply(places.favourites);
  try {
    const d = await saveAppData('files', (d) => (d.favourites = apply(d.favourites)));
    places.favourites = d.favourites;
  } catch (e) {
    places.favourites = on ? places.favourites.filter((x) => x !== id) : [...places.favourites, id];
    throw e;
  }
  return on;
}

/** Note that a file was opened. Saved in the background; failures don't matter. */
export function noteRecent(id) {
  const apply = (list = []) => [{ id, at: Date.now() }, ...list.filter((r) => r.id !== id)].slice(0, MAX_RECENT);
  places.recent = apply(places.recent);
  saveAppData('files', (d) => (d.recent = apply(d.recent))).catch(() => {});
}

/** Drop ids that no longer lead anywhere (deleted, or no longer shared). */
export async function forgetPlaces(ids) {
  const gone = new Set(ids);
  if (!gone.size) return;
  places.favourites = places.favourites.filter((x) => !gone.has(x));
  places.recent = places.recent.filter((r) => !gone.has(r.id));
  const untag = (tags = {}) => Object.fromEntries(Object.entries(tags).filter(([id]) => !gone.has(id)));
  places.tags = untag(places.tags);
  await saveAppData('files', (d) => {
    d.favourites = (d.favourites ?? []).filter((x) => !gone.has(x));
    d.recent = (d.recent ?? []).filter((r) => !gone.has(r.id));
    d.tags = untag(d.tags);
  }).catch(() => {});
}

/**
 * Resolve node ids to what the views show: { id, entry, parentId,
 * location: [folder names] }, with `entry` null for ones that are gone.
 */
export function resolvePlaces(ids) {
  return Promise.all(
    ids.map(async (id) => {
      try {
        const { items } = await resolvePath(id);
        return { id, entry: items.at(-1), parentId: items.at(-2)?.node.id ?? null, location: items.slice(0, -1).map(folderLabel) };
      } catch (e) {
        // Only a missing node is forgotten; a network error isn't proof.
        return { id, entry: null, missing: e?.status === 404 };
      }
    }),
  );
}

// ---------------------------------------------------------------- tags

export const MAX_TAG = 40;
const MAX_TAGS_PER_ITEM = 20;

/** A tag as kept: trimmed, inner spaces collapsed, no control characters. '' if nothing's left. */
export function cleanTag(tag) {
  return String(tag)
    .normalize('NFC')
    .replace(/[\p{Cc}\p{Cf}]/gu, '')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, MAX_TAG);
}

const same = (a, b) => a.localeCompare(b, undefined, { sensitivity: 'accent' }) === 0;

/** An item's tags, [] if none. */
export const tagsOf = (id) => places.tags[id] ?? [];

/** Every tag in use, with how many items carry it, most used first. */
export function allTags() {
  const count = new Map();
  for (const list of Object.values(places.tags)) {
    for (const tag of list) {
      const known = [...count.keys()].find((k) => same(k, tag)) ?? tag;
      count.set(known, (count.get(known) ?? 0) + 1);
    }
  }
  return [...count].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([tag, n]) => ({ tag, n }));
}

/** Ids of the items tagged `tag` (whatever the case). */
export const taggedWith = (tag) => Object.entries(places.tags).flatMap(([id, list]) => (list.some((x) => same(x, tag)) ? [id] : []));

/** Replace an item's tags. Duplicates (ignoring case) and empty ones are dropped. */
export async function setTags(id, tags) {
  const list = [];
  for (const tag of tags.map(cleanTag)) if (tag && !list.some((x) => same(x, tag))) list.push(tag);
  const apply = (all = {}) => {
    const next = { ...all };
    if (list.length) next[id] = list.slice(0, MAX_TAGS_PER_ITEM);
    else delete next[id];
    return next;
  };
  const before = places.tags;
  places.tags = apply(places.tags);
  try {
    const d = await saveAppData('files', (d) => (d.tags = apply(d.tags)));
    places.tags = d.tags;
  } catch (e) {
    places.tags = before;
    throw e;
  }
}
