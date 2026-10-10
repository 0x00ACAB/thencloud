// Linked storage (Google Drive): what's linked, for the storage meter and
// Settings, and linking a new account.
//
// Linking opens Google's consent page in a popup, so this page (and the keys
// it holds in memory) stays put. Google sends the popup back to the server,
// which finishes the link and shows /storage-linked.html; that page posts
// the outcome on a BroadcastChannel and closes. The popup can't be watched
// directly (the opener link is cut when it goes to Google), so the account
// list is also polled until the new account shows up.

import { storageInfo, linkGoogleDrive, moveStorage } from './cloud.svelte.js';

// `moving`: { id, to, done, total } while files move to or from an account.
export const storage = $state({ info: null, moving: null });

export async function loadStorage() {
  try {
    storage.info = await storageInfo();
  } catch {
    // The meter just shows the server's share.
  }
  return storage.info;
}

export function dropStorage() {
  storage.info = null;
  storage.moving = null;
}

/**
 * Move files between this server and account `id`, a batch per request,
 * until none are left. `to` is 'server' or 'linked'. Resolves with
 * 'done', 'full' (the destination ran out of room) or 'stopped'.
 */
export async function moveFiles(id, to) {
  if (storage.moving) return 'stopped';
  const moving = { id, to, done: 0, total: 0 };
  storage.moving = moving;
  stopRequested = false;
  try {
    for (;;) {
      const r = await moveStorage(id, to);
      moving.done += r.moved_bytes;
      moving.total = moving.done + r.left_bytes;
      storage.moving = { ...moving };
      if (r.full) return 'full';
      if (r.left === 0) return 'done';
      // Nothing moved but no reason given: don't spin.
      if (r.moved === 0) return 'stopped';
      if (stopRequested) return 'stopped';
    }
  } finally {
    storage.moving = null;
    await loadStorage();
  }
}

let stopRequested = false;

/** Stop after the batch that's moving now. */
export function stopMoving() {
  stopRequested = true;
}

const CHANNEL = 'thencloud-storage-link';
const WAIT_MS = 10 * 60 * 1000;

/**
 * Link a Google Drive as `mode` ('mirror' or 'extra'). Must be called from
 * a click, so the popup may open. Resolves with 'linked', 'denied',
 * 'expired', 'failed' or 'closed' (gave up waiting).
 */
export async function linkDrive(mode) {
  const before = new Set((storage.info?.accounts ?? []).map((a) => a.id));
  // Opened before anything is awaited, or the browser blocks it.
  const popup = window.open('about:blank', 'thencloud-link', 'popup,width=520,height=680');
  // Going to Google in this tab would drop the keys held in memory.
  if (!popup) throw Object.assign(new Error('popup blocked'), { code: 'popup_blocked' });
  let url;
  try {
    url = await linkGoogleDrive(mode);
  } catch (e) {
    popup?.close();
    throw e;
  }
  popup.location.href = url;

  return new Promise((resolve) => {
    let done = false;
    const channel = 'BroadcastChannel' in window ? new BroadcastChannel(CHANNEL) : null;
    const finish = async (outcome) => {
      if (done) return;
      done = true;
      channel?.close();
      clearInterval(poll);
      clearTimeout(giveUp);
      await loadStorage();
      resolve(outcome);
    };
    channel?.addEventListener('message', (e) => {
      if (typeof e.data === 'string') finish(e.data);
    });
    const poll = setInterval(async () => {
      const info = await storageInfo().catch(() => null);
      if (info?.accounts.some((a) => !before.has(a.id))) finish('linked');
    }, 3000);
    const giveUp = setTimeout(() => finish('closed'), WAIT_MS);
  });
}
