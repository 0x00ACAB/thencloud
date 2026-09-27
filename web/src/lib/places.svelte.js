// Favourites and recently opened files. Only node ids are kept, in the
// encrypted app data ("files"), so the server learns neither which files
// they are nor their names; names are decrypted here by resolving each id's
// path, and kept in memory only.
import { resolvePath, loadAppData, saveAppData } from './cloud.svelte.js';

const MAX_RECENT = 30;

/** `favourites`: [node id]; `recent`: [{ id, at }], newest first. */
export const places = $state({ favourites: [], recent: [], loaded: false });

let loading = null;

export function loadPlaces() {
  loading ??= loadAppData('files').then(
    (d) => {
      places.favourites = d.favourites ?? [];
      places.recent = d.recent ?? [];
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
  await saveAppData('files', (d) => {
    d.favourites = (d.favourites ?? []).filter((x) => !gone.has(x));
    d.recent = (d.recent ?? []).filter((r) => !gone.has(r.id));
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
        return { id, entry: items.at(-1), parentId: items.at(-2)?.node.id ?? null, location: items.slice(0, -1).map((i) => i.meta.name) };
      } catch (e) {
        // Only a missing node is forgotten; a network error isn't proof.
        return { id, entry: null, missing: e?.status === 404 };
      }
    }),
  );
}
