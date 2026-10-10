// Linked storage (Google Drive): what's linked, for the storage meter and
// Settings, and linking a new account.
//
// Linking opens Google's consent page in a popup, so this page (and the keys
// it holds in memory) stays put. Google sends the popup back to the server,
// which finishes the link and shows /storage-linked.html; that page posts
// the outcome on a BroadcastChannel and closes. The popup can't be watched
// directly (the opener link is cut when it goes to Google), so the account
// list is also polled until the new account shows up.

import { storageInfo, linkGoogleDrive } from './cloud.svelte.js';

export const storage = $state({ info: null });

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
