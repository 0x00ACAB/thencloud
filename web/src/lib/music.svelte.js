// The music library and player. A folder is picked as the music root; its
// tree is walked (names are decrypted here, like any listing) and grouped
// into albums by folder: Artist/Album/01 Track.mp3 is the usual layout.
// Tracks stream through the service worker, decrypted piece by piece, and
// the first piece is read for tags (title, artist, cover) as it plays.
// Everything is in memory; only the root folder's id is remembered.
// Playlists and edited titles are kept in the encrypted app data ('music');
// an album cover picked here is uploaded as cover.jpg into its folder.

import { SvelteMap } from 'svelte/reactivity';
import { session, resolvePath, walkTree, openEntry, fetchEntry, loadAppData, saveAppData, folderLabel } from './cloud.svelte.js';
import { putFolderImage } from './cover.js';
import { previewKind } from './preview.js';
import { parseTags } from './tags.js';
import { readChapters } from './chapters.js';
import { toast, errorMessage } from './ui.svelte.js';
import { t } from './i18n.svelte.js';
import { showNowPlaying, syncNowPlaying } from './nowplaying.svelte.js';

// ---------------------------------------------------------------- library

function readRoot() {
  try {
    const v = JSON.parse(localStorage.getItem('musicRoot'));
    return v?.user === session.me?.user_id ? v.id : null;
  } catch {
    return null;
  }
}

export const music = $state({ rootId: null, rootName: '', scanning: false, found: 0, error: '', dataError: '' });
/** `value` is { albums: [album], tracks: [track] } once scanned. */
export const library = new (class {
  value = $state.raw(null);
})();
/** Tags read from files as they play, by node id: { title, artist, album, track, cover }. */
export const tags = new SvelteMap();
/** Cover image URLs by album (folder) id; null when there is none. */
export const covers = new SvelteMap();

let scan = null;

/** Called by the Music view: scan the saved folder the first time. */
export function openLibrary() {
  loadSaved();
  music.rootId ??= readRoot();
  if (music.rootId && !library.value && !music.scanning && !music.error) scanLibrary();
}

export function setMusicRoot(id) {
  music.rootId = id;
  music.error = '';
  library.value = null;
  try {
    localStorage.setItem('musicRoot', JSON.stringify({ user: session.me.user_id, id }));
  } catch {
    /* private mode */
  }
  return scanLibrary();
}

const DISC = /^(cd|dis[ck])\s*\d+$/i;
const COVER = /^(cover|folder|front|album|albumart\w*|artwork)\.(jpe?g|png|webp|avif|gif)$/i;
// Ours (see setAlbumCover) beats the others.
const coverRank = (name) => (/^cover\.jpg$/i.test(name) ? 2 : COVER.test(name) ? 1 : 0);
const COVER_TYPES = new Set(['image/jpeg', 'image/png', 'image/webp', 'image/avif', 'image/gif']);
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Track number, title and maybe artist from a file name like "01 - Artist - Title.mp3". */
function fromName(name) {
  const base = name.replace(/\.[^.]+$/, '');
  const m = base.match(/^(?:\d{1,2}[-.])?(\d{1,3})(?:\s*[-._)]\s*|\s+)(\S.*)$/);
  let title = m ? m[2] : base;
  let artist;
  const dash = title.match(/^(.+?) - (.+)$/);
  if (dash) [, artist, title] = dash;
  return { trackNo: m ? +m[1] : undefined, title: title.replace(/_/g, ' '), artist };
}

/** A track for a file entry; `album` and `artist` come from its folders when known. */
export function makeTrack(entry, { albumId = null, album, artist, disc = '' } = {}) {
  const n = fromName(entry.meta.name);
  return { id: entry.node.id, entry, albumId, album, disc, trackNo: n.trackNo, title: n.title, artist: artist ?? n.artist };
}

/**
 * A track as shown: your edits, else what the file's tags say, else what its
 * name and folders say. `edited: false` leaves out the track's own edits.
 */
export function info(track, { edited = true } = {}) {
  const tag = tags.get(track.id);
  const e = edited ? saved.value.tracks[track.id] : null;
  const a = saved.value.albums[track.albumId];
  const folderCover = covers.get(track.albumId);
  return {
    title: e?.title || tag?.title || track.title,
    artist: e?.artist || tag?.artist || a?.artist || track.artist || t('Unknown artist'),
    album: e?.album || tag?.album || a?.name || track.album || '',
    trackNo: e?.trackNo ?? tag?.track ?? track.trackNo,
    cover: (a?.cover && folderCover) || tag?.cover || folderCover || null,
  };
}

/** An album's name and artist, with your edits. */
export function albumInfo(album) {
  const e = saved.value.albums[album.id];
  return { name: e?.name || album.name, artist: e?.artist || album.artist };
}

/** An album's tracks in order, by disc and (edited) track number. */
export function albumTracks(album) {
  const no = (track) => info(track).trackNo ?? 1e9;
  return album.tracks.toSorted((a, b) => collator.compare(a.disc, b.disc) || no(a) - no(b) || collator.compare(a.entry.meta.name, b.entry.meta.name));
}

// ------------------------------------------------- playlists and edits

const NO_DATA = { playlists: [], tracks: {}, albums: {}, positions: {} };
/**
 * `value` is { playlists: [{ id, name, tracks: [node id], at }], tracks: { id: edits },
 * albums: { id: edits }, positions: { id: { t, at } } } (where long tracks were left off).
 */
export const saved = new (class {
  value = $state.raw(NO_DATA);
})();
let savedLoad = null;

function loadSaved() {
  savedLoad ??= loadAppData('music')
    .then((d) => ((saved.value = { ...NO_DATA, ...d }), (music.dataError = '')))
    .catch((e) => ((savedLoad = null), (music.dataError = errorMessage(e))));
  return savedLoad;
}

async function update(fn) {
  await loadSaved();
  if (music.dataError) throw new Error(music.dataError);
  const d = await saveAppData('music', (d) => {
    for (const k in NO_DATA) d[k] ??= structuredClone(NO_DATA[k]);
    fn(d);
  });
  saved.value = { ...NO_DATA, ...d };
}

const TRACK_FIELDS = ['title', 'artist', 'album', 'trackNo'];

/** Save `fields` ({ title, artist, album, trackNo }) as edits: only what differs from the file. */
export function editTrack(track, fields) {
  const base = info(track, { edited: false });
  const e = {};
  for (const k of TRACK_FIELDS) if (fields[k] !== '' && fields[k] != null && fields[k] !== base[k]) e[k] = fields[k];
  return update((d) => (Object.keys(e).length ? (d.tracks[track.id] = e) : delete d.tracks[track.id]));
}

export const isEdited = (track) => !!saved.value.tracks[track.id];
export const resetTrack = (track) => update((d) => delete d.tracks[track.id]);

/** Save an album's name and artist; empty means the folder's. */
export function editAlbum(album, { name, artist }) {
  return update((d) => {
    const e = { ...d.albums[album.id] };
    for (const [k, v, base] of [['name', name, album.name], ['artist', artist, album.artist]]) {
      if (v && v !== base) e[k] = v;
      else delete e[k];
    }
    if (Object.keys(e).length) d.albums[album.id] = e;
    else delete d.albums[album.id];
  });
}

/** Upload `file` as the album's cover.jpg and show it over the tracks' own pictures. */
export async function setAlbumCover(album, file) {
  const { entry, blob } = await putFolderImage(album.id, 'cover.jpg', file, album.cover);
  album.cover = entry;
  const old = covers.get(album.id);
  if (old) URL.revokeObjectURL(old);
  covers.set(album.id, URL.createObjectURL(blob));
  await update((d) => (d.albums[album.id] = { ...d.albums[album.id], cover: true }));
}

/** Library tracks by node id. */
let byId = { lib: null, map: new Map() };
function trackById(id) {
  if (byId.lib !== library.value) byId = { lib: library.value, map: new Map(library.value?.tracks.map((track) => [track.id, track])) };
  return byId.map.get(id);
}

/** A playlist's tracks that are in the library (others may have been moved or deleted). */
export const playlistTracks = (p) => p.tracks.map(trackById).filter(Boolean);

const findList = (d, id) => d.playlists.find((p) => p.id === id) ?? { tracks: [] };

export async function createPlaylist(name, tracks = []) {
  const id = crypto.randomUUID();
  await update((d) => d.playlists.push({ id, name, tracks: [...new Set(tracks.map((track) => track.id))], at: Date.now() }));
  return id;
}

export const renamePlaylist = (id, name) => update((d) => (findList(d, id).name = name));
export const deletePlaylist = (id) => update((d) => (d.playlists = d.playlists.filter((p) => p.id !== id)));

/** Add tracks not already in the playlist; resolves to how many were added. */
export async function addToPlaylist(id, tracks) {
  let added = 0;
  await update((d) => {
    const p = findList(d, id);
    const have = new Set(p.tracks);
    added = 0;
    for (const track of tracks) {
      if (have.has(track.id)) continue;
      have.add(track.id);
      p.tracks.push(track.id);
      added++;
    }
    p.at = Date.now();
  });
  return added;
}

export const removeFromPlaylist = (id, track) =>
  update((d) => {
    const p = findList(d, id);
    p.tracks = p.tracks.filter((id) => id !== track.id);
  });

/** Swap two tracks in the playlist (a track and its neighbour, to move it). */
export const moveInPlaylist = (id, track, other) =>
  update((d) => {
    const p = findList(d, id);
    const [i, j] = [p.tracks.indexOf(track.id), p.tracks.indexOf(other?.id)];
    if (i >= 0 && j >= 0) [p.tracks[i], p.tracks[j]] = [p.tracks[j], p.tracks[i]];
  });

export async function scanLibrary() {
  if (!music.rootId) return;
  scan?.abort();
  const ctl = (scan = new AbortController());
  Object.assign(music, { scanning: true, found: 0, error: '' });
  try {
    const { items } = await resolvePath(music.rootId);
    const root = items[items.length - 1];
    if (root.node.kind !== 'folder') throw new Error(t('The music folder is not a folder'));
    const rootId = root.node.id;
    const folders = new Map([[rootId, { name: folderLabel(root), parentId: null }]]);
    const images = new Map(); // folder id -> best cover entry
    const files = [];
    await walkTree(root, {
      signal: ctl.signal,
      onEntry: (r) => {
        if (r.node.kind === 'folder') return folders.set(r.node.id, { name: r.meta.name, parentId: r.parentId });
        const kind = previewKind(r.meta);
        if (kind?.kind === 'audio') {
          files.push(r);
          music.found++;
        } else if (COVER_TYPES.has(kind?.type) && r.meta.size < 16 << 20) {
          // Any image in the folder, but cover.jpg and the like first.
          const best = images.get(r.parentId);
          if (!best || coverRank(r.meta.name) > coverRank(best.meta.name)) images.set(r.parentId, r);
        }
      },
    });
    if (ctl.signal.aborted) return;

    const albums = new Map();
    for (const r of files) {
      let albumId = r.parentId;
      let disc = '';
      if (albumId !== rootId && DISC.test(folders.get(albumId).name)) {
        disc = folders.get(albumId).name;
        albumId = folders.get(albumId).parentId;
      }
      const folder = folders.get(albumId);
      const artist = albumId !== rootId && folder.parentId !== rootId ? folders.get(folder.parentId)?.name : undefined;
      const track = makeTrack(r, { albumId, album: folder.name, artist, disc });
      if (!albums.has(albumId)) albums.set(albumId, { id: albumId, name: folder.name, artist, tracks: [], cover: images.get(albumId) ?? images.get(r.parentId) ?? null });
      albums.get(albumId).tracks.push(track);
    }
    const byTrack = (a, b) => collator.compare(a.disc, b.disc) || (a.trackNo ?? 1e9) - (b.trackNo ?? 1e9) || collator.compare(a.entry.meta.name, b.entry.meta.name);
    const list = [...albums.values()];
    for (const a of list) {
      a.tracks.sort(byTrack);
      // An album without a folder artist takes one its tracks' names agree on.
      a.artist ??= a.tracks.every((track) => track.artist && track.artist === a.tracks[0].artist) ? a.tracks[0].artist : undefined;
    }
    list.sort((a, b) => collator.compare(a.artist ?? '￿', b.artist ?? '￿') || collator.compare(a.name, b.name));
    music.rootName = folderLabel(root);
    library.value = { albums: list, tracks: list.flatMap((a) => a.tracks) };
  } catch (e) {
    if (!ctl.signal.aborted) music.error = errorMessage(e);
  } finally {
    if (scan === ctl) music.scanning = false;
  }
}

// Covers are fetched as album cards come into view, two at a time.
const coverQueue = [];
let coversLoading = 0;

export function loadCover(album) {
  if (!album.cover || covers.has(album.id) || coverQueue.includes(album)) return;
  coverQueue.push(album);
  pumpCovers();
}

async function pumpCovers() {
  while (coversLoading < 2 && coverQueue.length) {
    const album = coverQueue.shift();
    if (covers.has(album.id)) continue;
    coversLoading++;
    fetchEntry(album.cover)
      .then(({ blob }) => covers.set(album.id, URL.createObjectURL(new Blob([blob], { type: previewKind(album.cover.meta).type }))))
      .catch(() => covers.set(album.id, null))
      .finally(() => {
        coversLoading--;
        pumpCovers();
      });
  }
}

function learn(track, bytes) {
  if (tags.has(track.id)) return;
  const found = parseTags(bytes) ?? {};
  const cover = found.picture ? URL.createObjectURL(found.picture) : undefined;
  tags.set(track.id, { title: found.title, artist: found.artist, album: found.album, track: found.track, cover });
  if (track === current()) updateSession();
}

// ----------------------------------------------------------------- player

function readRate() {
  try {
    const v = parseFloat(localStorage.getItem('playbackRate'));
    return RATES.includes(v) ? v : 1;
  } catch {
    return 1;
  }
}

export const RATES = [0.75, 1, 1.25, 1.5, 1.75, 2];

// Audiobooks, podcasts and other long tracks remember where they were left
// off, and pick up there next time.
const LONG = 20 * 60;
const MAX_POSITIONS = 300;
const isLong = (track, duration) => /\.m4b$/i.test(track.entry.meta.name) || duration >= LONG;

function readVolume() {
  try {
    const v = parseFloat(localStorage.getItem('volume'));
    return v >= 0 && v <= 1 ? v : 1;
  } catch {
    return 1;
  }
}

// The queue holds library tracks as they are (not proxied), so they can be
// compared; it is replaced, never changed in place.
export const player = new (class {
  queue = $state.raw([]);
  index = $state(-1);
  playing = $state(false);
  buffering = $state(false);
  time = $state(0);
  duration = $state(0);
  shuffle = $state(false);
  repeat = $state('off'); // off | all | one
  volume = $state(readVolume());
  muted = $state(false);
  rate = $state(readRate());
  chapters = $state.raw([]); // [{ start, title }] of the current track
})();

export const current = () => player.queue[player.index] ?? null;

let audio = null;
let served = null; // { close } for the loaded track
let original = []; // queue order before shuffling
let loadSeq = 0;
let failures = 0;

function element() {
  if (audio) return audio;
  audio = new Audio();
  audio.preload = 'auto';
  audio.volume = player.volume;
  audio.addEventListener('timeupdate', () => {
    player.time = audio.currentTime;
    if (Date.now() - lastRemembered > 60_000) remember();
  });
  audio.addEventListener('loadedmetadata', resume);
  audio.addEventListener('durationchange', () => {
    player.duration = Number.isFinite(audio.duration) ? audio.duration : 0;
    updatePosition();
  });
  audio.addEventListener('play', () => (player.playing = true));
  audio.addEventListener('pause', () => {
    player.playing = false;
    remember();
  });
  audio.addEventListener('waiting', () => (player.buffering = true));
  audio.addEventListener('playing', () => {
    player.buffering = false;
    failures = 0;
  });
  audio.addEventListener('ended', ended);
  audio.addEventListener('error', () => audio.getAttribute('src') && failed());
  audio.addEventListener('seeked', updatePosition);
  if ('mediaSession' in navigator) {
    for (const [action, fn] of Object.entries(sessionActions)) {
      try {
        navigator.mediaSession.setActionHandler(action, fn);
      } catch {
        /* not supported here */
      }
    }
  }
  return audio;
}

// The media keys, the browser's media controls and, in the Android app, the
// notification and lock screen (nowplaying.svelte.js).
const sessionActions = {
  play: () => (!audio || audio.paused) && toggle(),
  pause: () => audio?.pause(),
  previoustrack: () => previous(),
  nexttrack: () => next(),
  seekto: (d) => seek(d.seekTime),
  seekbackward: (d) => seek((audio?.currentTime ?? 0) - (d.seekOffset || 10)),
  seekforward: (d) => seek((audio?.currentTime ?? 0) + (d.seekOffset || 10)),
};

showNowPlaying(
  () => {
    const track = current();
    if (!track) return null;
    const { title, artist, album, cover } = info(track);
    const { duration, rate, playing, buffering } = player;
    return { title, artist, album, cover, duration, rate, playing, buffering };
  },
  () => audio?.currentTime ?? 0,
  sessionActions,
);

function release() {
  served?.close();
  served = null;
}

async function load(autoplay = true) {
  remember(); // the track being left, before anything changes
  const track = current();
  const seq = ++loadSeq;
  const a = element();
  loaded = track;
  player.chapters = [];
  // Detach the old stream first, or its closing reads as an error here.
  a.pause();
  a.removeAttribute('src');
  a.load();
  release();
  Object.assign(player, { time: 0, duration: 0, buffering: true });
  updateSession();
  try {
    const type = previewKind(track.entry.meta).type;
    // Where long tracks were left off (also when playing from My files).
    await loadSaved();
    const { streamsAvailable, serveFile } = await import('./stream.js');
    let src;
    if (await streamsAvailable()) {
      const f = openEntry(track.entry);
      const s = serveFile({
        size: f.size,
        chunkSize: f.chunkSize,
        type,
        read: async (i) => {
          const piece = await f.read(i);
          if (i === 0) learn(track, piece); // before the piece is handed on
          return piece;
        },
      });
      src = s.url;
      served = s;
    } else {
      // No stream worker: the whole file in memory.
      const { blob } = await fetchEntry(track.entry);
      learn(track, new Uint8Array(await blob.slice(0, 4 << 20).arrayBuffer()));
      src = URL.createObjectURL(new Blob([blob], { type }));
      served = { close: () => URL.revokeObjectURL(src) };
    }
    if (seq !== loadSeq) return;
    a.src = src;
    a.defaultPlaybackRate = a.playbackRate = player.rate;
    findChapters(track, seq);
    if (autoplay) await a.play();
    else player.buffering = false;
  } catch (e) {
    if (seq === loadSeq && e?.name !== 'AbortError') failed(e);
  }
}

function failed(e) {
  const track = current();
  if (!track) return;
  toast(e ? t('Could not play {title}: {error}', { title: info(track).title, error: errorMessage(e) }) : t('Could not play {title}', { title: info(track).title }), { kind: 'error' });
  player.buffering = false;
  // Skip ahead, but not round and round a queue that won't play.
  if (++failures < player.queue.length && (player.index < player.queue.length - 1 || player.repeat === 'all')) next();
}

function ended() {
  forget(current());
  if (player.repeat === 'one') {
    audio.currentTime = 0;
    audio.play();
  } else if (player.index < player.queue.length - 1 || player.repeat === 'all') next();
  else {
    player.playing = false;
    seek(0);
  }
}

function shuffled(list) {
  const out = [...list];
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

/** Replace the queue with `tracks` and play from `start`. */
export function play(tracks, start = 0, { shuffle = player.shuffle } = {}) {
  if (!tracks.length) return;
  original = [...tracks];
  player.shuffle = shuffle;
  if (shuffle) {
    const first = tracks[start] ?? tracks[0];
    player.queue = [first, ...shuffled(tracks.filter((track) => track !== first))];
    player.index = 0;
  } else {
    player.queue = [...tracks];
    player.index = Math.max(0, start);
  }
  failures = 0;
  load();
}

export function toggle() {
  if (!current()) return;
  if (!audio?.getAttribute('src')) return load();
  if (audio.paused) audio.play().catch(() => {});
  else audio.pause();
}

export function next() {
  if (!player.queue.length) return;
  player.index = (player.index + 1) % player.queue.length;
  load();
}

export function previous() {
  if (!player.queue.length) return;
  if ((audio?.currentTime ?? 0) > 3 || (player.index === 0 && player.repeat !== 'all')) return seek(0);
  player.index = (player.index - 1 + player.queue.length) % player.queue.length;
  load();
}

export function jump(i) {
  if (i < 0 || i >= player.queue.length) return;
  player.index = i;
  load();
}

export function seek(t) {
  if (!audio) return;
  audio.currentTime = Math.max(0, Math.min(t, player.duration || 0));
  player.time = audio.currentTime;
  updatePosition();
}

export function setVolume(v) {
  player.volume = Math.max(0, Math.min(1, v));
  player.muted = false;
  if (audio) {
    audio.volume = player.volume;
    audio.muted = false;
  }
  try {
    localStorage.setItem('volume', String(player.volume));
  } catch {
    /* private mode */
  }
}

export function toggleMute() {
  player.muted = !player.muted;
  if (audio) audio.muted = player.muted;
}

export function toggleShuffle() {
  const now = current();
  player.shuffle = !player.shuffle;
  if (!now) return;
  if (player.shuffle) {
    original = [...player.queue];
    player.queue = [...player.queue.slice(0, player.index + 1), ...shuffled(player.queue.slice(player.index + 1))];
  } else {
    const kept = new Set(player.queue);
    player.queue = original.filter((track) => kept.has(track));
    player.index = player.queue.indexOf(now);
  }
}

export function cycleRepeat() {
  player.repeat = { off: 'all', all: 'one', one: 'off' }[player.repeat];
}

/** Play `tracks` right after the current one. */
export function playNext(tracks) {
  if (!current()) return play(tracks);
  player.queue = player.queue.toSpliced(player.index + 1, 0, ...tracks);
  original.push(...tracks);
}

export function enqueue(tracks) {
  if (!current()) return play(tracks);
  player.queue = [...player.queue, ...tracks];
  original.push(...tracks);
}

export function removeFromQueue(i) {
  if (i === player.index) return;
  original = original.filter((track) => track !== player.queue[i]);
  player.queue = player.queue.toSpliced(i, 1);
  if (i < player.index) player.index--;
}

/** Stop and empty the queue. */
export function stop() {
  remember();
  loaded = null;
  loadSeq++;
  release();
  if (audio) {
    audio.pause();
    audio.removeAttribute('src');
    audio.load();
  }
  Object.assign(player, { queue: [], index: -1, playing: false, buffering: false, time: 0, duration: 0, chapters: [] });
  original = [];
  if ('mediaSession' in navigator) navigator.mediaSession.metadata = null;
}

/** Drop everything decrypted: on sign-out, or when the signed-in view goes away. */
export function unloadMusic() {
  stop();
  scan?.abort();
  coverQueue.length = 0;
  library.value = null;
  for (const tag of tags.values()) if (tag.cover) URL.revokeObjectURL(tag.cover);
  for (const url of covers.values()) if (url) URL.revokeObjectURL(url);
  tags.clear();
  covers.clear();
  saved.value = NO_DATA;
  savedLoad = null;
  Object.assign(music, { rootId: null, rootName: '', scanning: false, found: 0, error: '', dataError: '' });
}

function updateSession() {
  const track = current();
  if (!('mediaSession' in navigator) || !track || typeof MediaMetadata === 'undefined') return;
  const i = info(track);
  navigator.mediaSession.metadata = new MediaMetadata({ title: i.title, artist: i.artist, album: i.album, artwork: i.cover ? [{ src: i.cover }] : [] });
}

let loaded = null; // the track the element holds
let lastRemembered = 0;

/** Save where a long track is, so it can pick up there next time. */
function remember() {
  const track = loaded;
  lastRemembered = Date.now();
  if (!track || !audio || !player.duration || !isLong(track, player.duration)) return;
  const secs = Math.floor(audio.currentTime);
  // Near either end there's nothing worth coming back to.
  if (secs < 15 || secs > player.duration - 30) return forget(track);
  if (saved.value.positions?.[track.id]?.t === secs) return;
  savePosition(track.id, { t: secs, at: Date.now() });
}

function forget(track) {
  if (track && saved.value.positions?.[track.id]) savePosition(track.id, null);
}

function savePosition(id, pos) {
  update((d) => {
    if (pos) d.positions[id] = pos;
    else delete d.positions[id];
    // Keep the most recent few hundred.
    const all = Object.entries(d.positions);
    if (all.length > MAX_POSITIONS) {
      all.sort((a, b) => b[1].at - a[1].at);
      d.positions = Object.fromEntries(all.slice(0, MAX_POSITIONS));
    }
  }).catch(() => {});
}

function resume() {
  const track = loaded;
  const pos = track && saved.value.positions?.[track.id];
  if (!pos || !isLong(track, audio.duration) || pos.t >= audio.duration - 30) return;
  audio.currentTime = pos.t;
  toast(t('Picked up where you left off, at {time}', { time: formatTime(pos.t) }), { icon: 'play' });
}

/** Chapters from an M4B/MP4, read from its moov box (a few pieces at most). */
async function findChapters(track, seq) {
  if (!/\.(m4b|m4a|mp4)$/i.test(track.entry.meta.name)) return;
  const list = await readChapters(openEntry(track.entry));
  if (seq === loadSeq) player.chapters = list;
}

/** Whether the current track is long enough for speed and chapters to matter. */
export const longForm = () => !!current() && (isLong(current(), player.duration) || player.chapters.length > 0);

/** The chapter playing at `time`, as an index into player.chapters, or -1. */
export function chapterAt(time) {
  const c = player.chapters;
  let i = -1;
  while (i + 1 < c.length && c[i + 1].start <= time + 0.5) i++;
  return i;
}

export function setRate(r) {
  player.rate = r;
  if (audio) audio.defaultPlaybackRate = audio.playbackRate = r;
  updatePosition();
  try {
    localStorage.setItem('playbackRate', String(r));
  } catch {
    /* private mode */
  }
}

function updatePosition() {
  syncNowPlaying();
  if (!('mediaSession' in navigator) || !player.duration) return;
  try {
    navigator.mediaSession.setPositionState({ duration: player.duration, position: Math.min(player.time, player.duration), playbackRate: player.rate });
  } catch {
    /* not supported here */
  }
}

/** m:ss, or h:mm:ss. */
export function formatTime(s) {
  if (!Number.isFinite(s) || s < 0) s = 0;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(Math.floor(s % 60)).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}
