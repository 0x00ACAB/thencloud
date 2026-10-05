// The video library. A folder is picked as the videos root; its tree is
// walked (names are decrypted here, like any listing) and each video is
// sorted into a series by its name and folders (Show/Season 1/Show S01E02
// Title.mkv), its MP4 or Matroska tags, or your edits; the rest are movies.
// Videos stream through the service worker, decrypted piece by piece.
// Edits, tags already read and how far you got in each video are kept in
// the encrypted app data ('videos'); posters are uploaded as images next to
// the videos. Only the root folder's id is remembered in this browser.

import { SvelteMap } from 'svelte/reactivity';
import { session, resolvePath, walkTree, openEntry, fetchEntry, loadAppData, saveAppData, rename, folderLabel } from './cloud.svelte.js';
import { isSubtitle, matchSubtitles } from './subtitles.js';
import { previewKind, extension, MAX_PREVIEW } from './preview.js';
import { parseEpisode, episodeLabel, safeName } from './episodes.js';
import { readVideoTags } from './videotags.js';
import { putFolderImage } from './cover.js';
import { errorMessage } from './ui.svelte.js';
import { t, language } from './i18n.svelte.js';

// ---------------------------------------------------------------- library

function readRoot() {
  try {
    const v = JSON.parse(localStorage.getItem('videosRoot'));
    return v?.user === session.me?.user_id ? v.id : null;
  } catch {
    return null;
  }
}

export const videos = $state({ rootId: null, rootName: '', scanning: false, found: 0, error: '', dataError: '', reading: null });
/** `value` is { rootId, items: [video], images: Map(folder id -> entry), named: Map(folder/name -> entry) } once scanned. */
export const scanned = new (class {
  value = $state.raw(null);
})();
/** Poster URLs by image node id; null when it couldn't be read. */
export const posters = new SvelteMap();

const NO_DATA = { edits: {}, found: {}, progress: {} };
/** `value` is { edits: { id: fields }, found: { id: tags }, progress: { id: { t, d, at } } }. */
export const saved = new (class {
  value = $state.raw(NO_DATA);
})();
let savedLoad = null;
let scan = null;

function loadSaved() {
  savedLoad ??= loadAppData('videos')
    .then((d) => ((saved.value = { ...NO_DATA, ...d }), (videos.dataError = '')))
    .catch((e) => ((savedLoad = null), (videos.dataError = errorMessage(e))));
  return savedLoad;
}

async function update(fn) {
  await loadSaved();
  if (videos.dataError) throw new Error(videos.dataError);
  const d = await saveAppData('videos', (d) => {
    for (const k in NO_DATA) d[k] ??= {};
    fn(d);
  });
  saved.value = { ...NO_DATA, ...d };
}

/** Called by the Videos view: scan the saved folder the first time. */
export function openVideos() {
  loadSaved();
  videos.rootId ??= readRoot();
  if (videos.rootId && !scanned.value && !videos.scanning && !videos.error) scanVideos();
}

export function setVideosRoot(id) {
  videos.rootId = id;
  videos.error = '';
  scanned.value = null;
  try {
    localStorage.setItem('videosRoot', JSON.stringify({ user: session.me.user_id, id }));
  } catch {
    /* private mode */
  }
  return scanVideos();
}

const POSTER = /^(poster|folder|cover|show|season)\.(jpe?g|png|webp|avif)$/i;
const POSTER_TYPES = new Set(['image/jpeg', 'image/png', 'image/webp', 'image/avif']);
const posterRank = (name) => (/^poster\.jpg$/i.test(name) ? 2 : 1);
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
const bare = (name) => name.replace(/\.[^.]+$/, '');
const stem = (name) => bare(name).toLowerCase();

/** The Blob type to play a file as, or null if it isn't a video. Matroska plays where the browser can. */
export function videoType(meta) {
  const k = previewKind(meta);
  if (k?.kind === 'video') return k.type;
  return extension(meta.name) === 'mkv' ? 'video/webm' : null;
}

export async function scanVideos() {
  if (!videos.rootId) return;
  scan?.abort();
  const ctl = (scan = new AbortController());
  Object.assign(videos, { scanning: true, found: 0, error: '' });
  try {
    const { items: path } = await resolvePath(videos.rootId);
    const root = path[path.length - 1];
    if (root.node.kind !== 'folder') throw new Error(t('The videos folder is not a folder'));
    const rootId = root.node.id;
    const folders = new Map([[rootId, { path: [], names: [] }]]);
    const images = new Map(); // folder id -> poster.jpg and the like
    const named = new Map(); // "folder id/stem" -> image named after a video
    const subtitles = new Map(); // folder id -> .srt and .vtt files in it
    const files = [];
    await walkTree(root, {
      signal: ctl.signal,
      onEntry: (r) => {
        const up = folders.get(r.parentId);
        if (r.node.kind === 'folder') return folders.set(r.node.id, { path: [...up.path, r.node.id], names: [...up.names, r.meta.name] });
        if (videoType(r.meta)) {
          files.push(r);
          videos.found++;
        } else if (isSubtitle(r.meta.name)) {
          if (!subtitles.has(r.parentId)) subtitles.set(r.parentId, []);
          subtitles.get(r.parentId).push(r);
        } else if (POSTER_TYPES.has(previewKind(r.meta)?.type) && r.meta.size < 16 << 20) {
          named.set(`${r.parentId}/${stem(r.meta.name).replace(/-poster$/, '')}`, r);
          const best = images.get(r.parentId);
          if (POSTER.test(r.meta.name) && (!best || posterRank(r.meta.name) > posterRank(best.meta.name))) images.set(r.parentId, r);
        }
      },
    });
    if (ctl.signal.aborted) return;
    const items = files.map((r) => {
      const f = folders.get(r.parentId);
      return { id: r.node.id, entry: r, parentId: r.parentId, path: [rootId, ...f.path], guess: parseEpisode(r.meta.name, f.names) };
    });
    videos.rootName = folderLabel(root);
    scanned.value = { rootId, items, images, named, subtitles };
  } catch (e) {
    if (!ctl.signal.aborted) videos.error = errorMessage(e);
  } finally {
    if (scan === ctl) videos.scanning = false;
  }
}

/**
 * A video as shown: { series, season, episode, title, label }, from your
 * edits, else its tags, else its name and folders. Movies have no episode.
 * `edited: false` leaves out your edits.
 */
export function details(v, { edited = true } = {}) {
  const e = edited ? saved.value.edits[v.id] : null;
  const f = saved.value.found[v.id];
  const tagged = f?.episode != null ? { ...v.guess, ...f } : { ...v.guess, title: f?.title || v.guess.title };
  const d = { ...tagged, ...e };
  if (d.episode == null || d.episode === '') {
    const title = d.title || bare(v.entry.meta.name);
    return { series: '', season: null, episode: null, title, label: title };
  }
  const ep = { series: d.series || t('Unknown series'), season: d.season ?? 1, episode: +d.episode, title: d.title || '' };
  return { ...ep, label: episodeLabel(ep, t('Episode {n}', { n: ep.episode })) };
}

const seasonOrder = (s) => (s === 0 ? 1e6 : s); // specials last
const byEpisode = (a, b) => {
  const [x, y] = [details(a), details(b)];
  return seasonOrder(x.season) - seasonOrder(y.season) || x.episode - y.episode || collator.compare(a.entry.meta.name, b.entry.meta.name);
};

/** The deepest folder all of `list` is in, unless that's the root. */
function commonFolder(list, rootId) {
  let common = list[0].path;
  for (const v of list) {
    let i = 0;
    while (i < common.length && common[i] === v.path[i]) i++;
    common = common.slice(0, i);
  }
  const id = common.at(-1);
  return id && id !== rootId ? id : null;
}

function build(s) {
  if (!s) return null;
  const groups = new Map();
  const movies = [];
  for (const v of s.items) {
    const d = details(v);
    if (d.episode == null) {
      movies.push(v);
      continue;
    }
    const key = d.series.toLowerCase();
    if (!groups.has(key)) groups.set(key, { key, name: d.series, episodes: [] });
    groups.get(key).episodes.push(v);
  }
  const series = [...groups.values()].map((g) => {
    g.episodes.sort(byEpisode);
    g.seasons = [...new Set(g.episodes.map((v) => details(v).season))].sort((a, b) => seasonOrder(a) - seasonOrder(b));
    g.folderId = commonFolder(g.episodes, s.rootId);
    g.poster = (g.folderId && s.images.get(g.folderId)) || g.episodes.map((v) => s.images.get(v.parentId)).find(Boolean) || null;
    return g;
  });
  series.sort((a, b) => collator.compare(a.name, b.name));
  movies.sort((a, b) => collator.compare(details(a).label, details(b).label));
  return { series, movies, items: s.items };
}

/** { series: [{ key, name, episodes, seasons, folderId, poster }], movies: [video], items } once scanned. */
export const catalogue = new (class {
  value = $derived(build(scanned.value));
})();

/** A movie's poster: an image named after it, or the only video's folder poster. */
export function moviePoster(v) {
  const s = scanned.value;
  const own = s?.named.get(`${v.parentId}/${stem(v.entry.meta.name)}`);
  if (own || !s || v.parentId === s.rootId) return own ?? null;
  return s.items.filter((x) => x.parentId === v.parentId).length === 1 ? (s.images.get(v.parentId) ?? null) : null;
}

// Posters are fetched as cards come into view, two at a time.
const posterQueue = [];
let postersLoading = 0;

export function loadPoster(entry) {
  if (!entry || posters.has(entry.node.id) || posterQueue.includes(entry)) return;
  posterQueue.push(entry);
  pumpPosters();
}

function pumpPosters() {
  while (postersLoading < 2 && posterQueue.length) {
    const entry = posterQueue.shift();
    if (posters.has(entry.node.id)) continue;
    postersLoading++;
    fetchEntry(entry)
      .then(({ blob }) => posters.set(entry.node.id, URL.createObjectURL(new Blob([blob], { type: previewKind(entry.meta).type }))))
      .catch(() => posters.set(entry.node.id, null))
      .finally(() => {
        postersLoading--;
        pumpPosters();
      });
  }
}

/** Upload `file` as the poster of a series (poster.jpg in its folder) or a movie (name-poster.jpg next to it). */
export async function setPoster(target, file) {
  const s = scanned.value;
  const series = !!target.episodes;
  const folderId = series ? target.folderId : target.parentId;
  if (!folderId) throw new Error(t('Put the episodes in a folder of their own first'));
  const name = series ? 'poster.jpg' : `${bare(target.entry.meta.name)}-poster.jpg`;
  const existing = series ? target.poster : moviePoster(target);
  const { entry, blob } = await putFolderImage(folderId, name, file, existing);
  posters.set(entry.node.id, URL.createObjectURL(blob));
  const next = { ...s, images: new Map(s.images), named: new Map(s.named) };
  if (series) next.images.set(folderId, entry);
  else next.named.set(`${folderId}/${stem(target.entry.meta.name)}`, entry);
  scanned.value = next;
}

// ------------------------------------------------------------------ edits

const FIELDS = ['series', 'season', 'episode', 'title'];

/** Save `fields` ({ series, season, episode, title }; no episode for a movie) as edits: only what differs. */
export function editVideo(v, fields) {
  const base = details(v, { edited: false });
  const e = {};
  for (const k of FIELDS) if ((fields[k] ?? null) !== (base[k] ?? null) && !(k === 'series' && fields.episode == null)) e[k] = fields[k] ?? null;
  return update((d) => (Object.keys(e).length ? (d.edits[v.id] = e) : delete d.edits[v.id]));
}

export const isEdited = (v) => !!saved.value.edits[v.id];
export const resetVideo = (v) => update((d) => delete d.edits[v.id]);

/** Give every episode of a series another series name. */
export const renameSeries = (series, name) =>
  update((d) => {
    for (const v of series.episodes) d.edits[v.id] = { ...d.edits[v.id], series: name, episode: details(v).episode };
  });

/**
 * Rename the files of `list` to "{Episode Name} S01E02.ext". What each one
 * shows now is kept as your edits first, so nothing is lost if the new
 * names read differently. Resolves to how many were renamed.
 */
export async function renameFiles(list) {
  const todo = list
    .map((v) => {
      const d = details(v);
      return { v, d, name: `${safeName(d.label)}.${extension(v.entry.meta.name)}` };
    })
    .filter((x) => x.name !== x.v.entry.meta.name);
  if (!todo.length) return 0;
  await update((d) => {
    for (const { v, d: x } of todo) d.edits[v.id] = { series: x.series, season: x.season, episode: x.episode, title: x.title };
  });
  let n = 0;
  try {
    for (const { v, name } of todo) {
      const subs = subtitlesFor(v);
      await rename(v.entry, name);
      n++;
      // Subtitles keep going with it: "Old.en.srt" becomes "New.en.srt".
      const oldBase = v.entry.meta.name.replace(/\.[^.]+$/, '').length;
      const newBase = name.replace(/\.[^.]+$/, '');
      for (const sub of subs) await rename(sub.entry, newBase + sub.entry.meta.name.slice(oldBase)).catch(() => {});
    }
  } finally {
    if (n) await scanVideos();
  }
  return n;
}

// Tags: read from the first piece as a video plays, or from every video not
// yet read when asked (MP4s with their index at the end need a few more pieces).
function learn(v, head, chunkSize) {
  if (v.id in saved.value.found || isEdited(v)) return;
  const noMore = { size: v.entry.meta.size, chunkSize, count: 1, read: () => Promise.reject(new Error('head only')) };
  readVideoTags(noMore, head)
    .then((tags) => tags && update((d) => (d.found[v.id] = tags)))
    .catch(() => {});
}

let reader = null;

export async function readAllTags() {
  if (videos.reading || !scanned.value) return;
  const todo = scanned.value.items.filter((v) => !(v.id in saved.value.found));
  const ctl = (reader = new AbortController());
  videos.reading = { done: 0, total: todo.length };
  let results = {};
  const flush = async () => {
    const r = results;
    results = {};
    if (Object.keys(r).length) await update((d) => Object.assign(d.found, r));
  };
  let next = 0;
  const worker = async () => {
    while (next < todo.length && !ctl.signal.aborted) {
      const v = todo[next++];
      try {
        results[v.id] = (await readVideoTags(openEntry(v.entry))) ?? {};
      } catch {
        results[v.id] = {};
      }
      if (reader === ctl) videos.reading.done++;
      if (Object.keys(results).length >= 25) await flush();
    }
  };
  try {
    await Promise.all([worker(), worker()]);
    await flush();
  } finally {
    if (reader === ctl) {
      reader = null;
      videos.reading = null;
    }
  }
}

export function stopReading() {
  reader?.abort();
  reader = null;
  videos.reading = null;
}

// --------------------------------------------------------------- progress

const WATCHED = 0.92;

/** How far into `v` you got, 0 to 1. */
export function progressOf(v) {
  const p = saved.value.progress[v.id];
  return p?.d ? Math.min(1, p.t / p.d) : 0;
}

export const isWatched = (v) => progressOf(v) >= WATCHED;

/** Where to pick `v` up again, in seconds, or 0. */
export function resumeAt(v) {
  const p = saved.value.progress[v.id];
  return p && p.t > 10 && p.t < p.d * WATCHED ? p.t : 0;
}

const KEEP_PROGRESS = 1000;

export function saveProgress(v, at, duration) {
  if (!duration || !Number.isFinite(duration)) return;
  const p = saved.value.progress[v.id];
  if (p && Math.abs(p.t - at) < 5) return;
  update((d) => {
    d.progress[v.id] = { t: Math.round(at), d: Math.round(duration), at: Date.now() };
    const ids = Object.keys(d.progress);
    if (ids.length > KEEP_PROGRESS) {
      ids.sort((a, b) => d.progress[a].at - d.progress[b].at);
      for (const id of ids.slice(0, ids.length - KEEP_PROGRESS)) delete d.progress[id];
    }
  }).catch(() => {});
}

export const setWatched = (list, yes) =>
  update((d) => {
    for (const v of list) {
      if (yes) d.progress[v.id] = { t: 1, d: 1, at: Date.now() };
      else delete d.progress[v.id];
    }
  });

/** Videos started but not finished, most recent first. */
export function continueWatching(limit = 12) {
  const items = catalogue.value?.items ?? [];
  const p = saved.value.progress;
  return items.filter((v) => resumeAt(v)).sort((a, b) => p[b.id].at - p[a.id].at).slice(0, limit);
}

/** The episode to play next: one in progress, else the first unwatched after the last watched. */
export function nextUp(series) {
  const eps = series.episodes;
  const started = eps.filter(resumeAt).sort((a, b) => saved.value.progress[b.id].at - saved.value.progress[a.id].at)[0];
  if (started) return started;
  const last = eps.findLastIndex(isWatched);
  return eps[last + 1] ?? eps[0];
}

/** The series `v` is in, if any. */
export const seriesOf = (v) => catalogue.value?.series.find((s) => s.episodes.includes(v)) ?? null;

// ----------------------------------------------------------------- player

/** Subtitle files named after `v`, in its folder: [{ entry, lang, label }]. */
export function subtitlesFor(v) {
  return matchSubtitles(v.entry.meta.name, scanned.value?.subtitles?.get(v.parentId) ?? [], { locale: language(), unnamed: t('Subtitles') });
}

/** A URL `v` plays from: streamed through the worker, else the whole file in memory. { url, close }. */
export async function openVideo(v) {
  const type = videoType(v.entry.meta);
  const { streamsAvailable, serveFile } = await import('./stream.js');
  if (await streamsAvailable()) {
    const f = openEntry(v.entry);
    return serveFile({
      size: f.size,
      chunkSize: f.chunkSize,
      type,
      read: async (i) => {
        const piece = await f.read(i);
        if (i === 0) learn(v, piece.slice(), f.chunkSize); // a copy: the piece is handed on
        return piece;
      },
    });
  }
  if (v.entry.meta.size > MAX_PREVIEW) throw new Error(t('This video is too large to play without streaming. Reload the page and try again'));
  const { blob } = await fetchEntry(v.entry);
  const url = URL.createObjectURL(new Blob([blob], { type }));
  return { url, close: () => URL.revokeObjectURL(url) };
}

/** Drop everything decrypted: when the signed-in view goes away. */
export function unloadVideos() {
  scan?.abort();
  stopReading();
  posterQueue.length = 0;
  scanned.value = null;
  saved.value = NO_DATA;
  savedLoad = null;
  for (const url of posters.values()) if (url) URL.revokeObjectURL(url);
  posters.clear();
  Object.assign(videos, { rootId: null, rootName: '', scanning: false, found: 0, error: '', dataError: '', reading: null });
}
