// Notes: a folder of Markdown files used as a notebook. Only the folder's
// node id is remembered in this browser (with the user id); the list, and
// any note text read for searching, live in memory and go when the
// signed-in view does. Pinned notes are node ids in the encrypted "notes"
// app data.
import { t } from './i18n.svelte.js';
import { session, resolvePath, walkTree, fetchEntry, saveText, upload, loadAppData, saveAppData, keyOf } from './cloud.svelte.js';
import { errorMessage } from './ui.svelte.js';

const NOTE = /\.(md|markdown)$/i;
/** Notes bigger than this aren't read for searching. */
const MAX_SEARCH = 512 * 1024;

function readRoot() {
  try {
    const v = JSON.parse(localStorage.getItem('notesRoot'));
    return v?.user === session.me?.user_id ? v.id : null;
  } catch {
    return null;
  }
}

/** `list` is [{ ...entry, location: [folder names], parentId }]. */
export const notes = $state({ rootId: null, rootName: '', list: null, scanning: false, error: '', pinned: [] });

const texts = new Map(); // node id -> { revision, text }, for search and opening
let scan = null;
let pinsLoaded = null;

export function openNotes() {
  notes.rootId ??= readRoot();
  pinsLoaded ??= loadAppData('notes')
    .then((d) => (notes.pinned = d.pinned ?? []))
    .catch(() => (pinsLoaded = null));
  if (notes.rootId && !notes.list && !notes.scanning) scanNotes();
}

export function setNotesRoot(id) {
  notes.rootId = id;
  notes.list = null;
  texts.clear();
  try {
    localStorage.setItem('notesRoot', JSON.stringify({ user: session.me.user_id, id }));
  } catch {
    /* private mode */
  }
  return scanNotes();
}

export async function scanNotes() {
  if (!notes.rootId) return;
  scan?.abort();
  const ctl = (scan = new AbortController());
  Object.assign(notes, { scanning: true, error: '' });
  try {
    const { items } = await resolvePath(notes.rootId);
    const root = items[items.length - 1];
    if (root.node.kind !== 'folder') throw new Error(t('The notes folder is not a folder'));
    const found = [];
    await walkTree(root, {
      signal: ctl.signal,
      onEntry: (r) => r.node.kind === 'file' && NOTE.test(r.meta.name) && found.push({ ...r, location: r.location.slice(1) }),
    });
    if (ctl.signal.aborted) return;
    notes.rootName = root.meta.name;
    notes.list = found;
  } catch (e) {
    if (!ctl.signal.aborted) notes.error = errorMessage(e);
  } finally {
    if (scan === ctl) notes.scanning = false;
  }
}

/** A note's text, decrypted (cached until the note changes). */
export async function readNote(entry) {
  const c = texts.get(entry.node.id);
  if (c?.revision === entry.node.revision) return c.text;
  const { blob } = await fetchEntry(entry);
  const text = await blob.text();
  texts.set(entry.node.id, { revision: entry.node.revision, text });
  return text;
}

/** Save `text` as a new version; fails with 409 if the note changed elsewhere. Returns the updated entry. */
export async function writeNote(entry, text) {
  const saved = await saveText(entry, text);
  const updated = { ...entry, node: saved.node, meta: saved.meta };
  texts.set(entry.node.id, { revision: saved.node.revision, text });
  replace(updated);
  return updated;
}

/** The note as it is on the server now (after a conflict). */
export async function refreshNote(entry) {
  const { items } = await resolvePath(entry.node.id);
  const fresh = { ...entry, ...items[items.length - 1] };
  replace(fresh);
  return fresh;
}

function replace(entry) {
  if (!notes.list) return;
  notes.list = notes.list.map((n) => (n.node.id === entry.node.id ? { ...n, node: entry.node, meta: entry.meta } : n));
}

/** Notes whose name or (decrypted) text contains `query`, with a line of context. */
export async function searchNotes(query, { signal } = {}) {
  const q = query.trim().toLowerCase();
  const out = [];
  for (const n of notes.list ?? []) {
    if (signal?.aborted) break;
    if (n.meta.name.toLowerCase().includes(q)) {
      out.push({ entry: n, snippet: '' });
      continue;
    }
    if (n.meta.size > MAX_SEARCH) continue;
    try {
      const text = await readNote(n);
      const at = text.toLowerCase().indexOf(q);
      if (at < 0) continue;
      const start = text.lastIndexOf('\n', at) + 1;
      const end = text.indexOf('\n', at);
      out.push({ entry: n, snippet: text.slice(start, end < 0 ? undefined : end).trim().slice(0, 160) });
    } catch {
      /* unreadable: leave it out */
    }
  }
  return out;
}

/** Make an empty note in the notes folder, under a free name. */
export async function createNote(name = 'Untitled') {
  const taken = new Set((notes.list ?? []).filter((n) => n.parentId === notes.rootId).map((n) => n.meta.name.toLowerCase()));
  let file = `${name}.md`;
  for (let i = 2; taken.has(file.toLowerCase()); i++) file = `${name} ${i}.md`;
  const key = await keyOf(notes.rootId);
  const made = await upload(new File([''], file, { type: 'text/markdown' }), { parentId: notes.rootId, parentKey: key });
  const entry = { ...made, location: [], parentId: notes.rootId };
  notes.list = [...(notes.list ?? []), entry];
  texts.set(made.node.id, { revision: made.node.revision, text: '' });
  return entry;
}

export const isPinned = (id) => notes.pinned.includes(id);

export async function togglePin(id) {
  const on = !isPinned(id);
  const apply = (list = []) => (on ? [...list.filter((x) => x !== id), id] : list.filter((x) => x !== id));
  notes.pinned = apply(notes.pinned);
  const d = await saveAppData('notes', (d) => (d.pinned = apply(d.pinned)));
  notes.pinned = d.pinned;
}

/** Drop everything decrypted: when the signed-in view goes away. */
export function unloadNotes() {
  scan?.abort();
  texts.clear();
  pinsLoaded = null;
  Object.assign(notes, { rootId: null, rootName: '', list: null, scanning: false, error: '', pinned: [] });
}
