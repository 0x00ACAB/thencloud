// The page's side of the Android app's plugin
// (crates/thencloud-app/android-plugin).
//
// Saving files: Android's WebView drops a download of a blob: URL, and has
// no service worker for the app's pages, so there the page hands each
// decrypted piece to the plugin, which writes it to Downloads. Sharing a
// file to another app works the same way, in reverse (shareNative): pieces
// go to the plugin, which writes them into our own cache and hands the
// result to the system share sheet. Sharing one in from another app
// (importShare) has no pieces to start from: the plugin only has a
// content:// Uri, which no web API can read, so the page pulls the
// plaintext back out of it a piece at a time instead. The only other
// native calls the app's pages may make are for what music is playing
// (nowplaying.svelte.js; capabilities/android.json).

import { inApp } from './server.svelte.js';
import { t } from './i18n.svelte.js';

export const inAndroidApp = inApp && /\bAndroid\b/.test(navigator.userAgent);

/** True in the Android app, where files are saved through the plugin. */
export const nativeSaves = inAndroidApp && !!globalThis.__TAURI_INTERNALS__;

export const call = (cmd, args) => globalThis.__TAURI_INTERNALS__.invoke(`plugin:thencloud|${cmd}`, args);

/** Android's current connectivity and whether background data use is restricted. */
export const networkStatus = () => call('network_status');

// The bridge carries JSON, so pieces go over as base64, in slices that keep
// each message small.
const SLICE = 512 * 1024;

export function base64(bytes) {
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

const unbase64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));

const cancelled = () => Object.assign(new Error('The download was cancelled.'), { name: 'AbortError' });

/**
 * Save what `parts` (an async iterable of Uint8Array) yields as one file.
 * Rejects with an AbortError if the save dialog (Android 9 and older) is
 * cancelled; a failed save leaves no partial file behind.
 */
export async function savePieces(parts, name, type) {
  // Android's own messages aren't translated; say it in the user's language.
  const failed = (e) => (console.error(e), new Error(t("Couldn't save {name} on this device.", { name })));
  const { id } = await call('save_begin', { name, mime: type || null }).catch((e) => {
    throw failed(e);
  });
  if (id == null) {
    await parts.return?.();
    throw cancelled();
  }
  let ok = false;
  try {
    for await (const piece of parts) {
      for (let i = 0; i < piece.length; i += SLICE) {
        await call('save_write', { id, data: base64(piece.subarray(i, i + SLICE)) }).catch((e) => {
          throw failed(e);
        });
      }
    }
    ok = true;
  } finally {
    await call('save_end', { id, ok }).catch(() => {});
  }
}

/** A Blob in pieces, read one at a time. */
async function* blobPieces(blob) {
  const step = 4 * 1024 * 1024;
  for (let at = 0; at < blob.size; at += step) yield new Uint8Array(await blob.slice(at, at + step).arrayBuffer());
}

export const saveBlobNative = (blob, name) => savePieces(blobPieces(blob), name, blob.type);

/** A file opened with openFile (crypto.js), piece by piece. */
export async function* filePieces(file, onProgress) {
  for (let i = 0; i < file.count; i++) {
    yield await file.read(i);
    onProgress?.((i + 1) / file.count);
  }
}

/**
 * Share what `parts` (an async iterable of Uint8Array) yields as one file,
 * to another app through the system share sheet. The plugin writes it into
 * its own cache first, since the share sheet needs a content: URI, not
 * bytes; that copy is cleaned up opportunistically, not right after.
 */
export async function shareNative(parts, name, type) {
  const failed = (e) => (console.error(e), new Error(t("Couldn't share {name} from this device.", { name })));
  const { id } = await call('share_out_begin', { name, mime: type || null }).catch((e) => {
    throw failed(e);
  });
  let ok = false;
  try {
    for await (const piece of parts) {
      for (let i = 0; i < piece.length; i += SLICE) {
        await call('share_out_write', { id, data: base64(piece.subarray(i, i + SLICE)) }).catch((e) => {
          throw failed(e);
        });
      }
    }
    ok = true;
  } finally {
    await call('share_out_end', { id, ok }).catch(() => {});
  }
}

export const shareBlobNative = (blob, name) => shareNative(blobPieces(blob), name, blob.type);

/** Files shared in from another app ("Share to thencloud"), not yet imported. */
export const pendingShares = () => call('share_pending').then((r) => r.items);

/**
 * Read a pending share's whole plaintext into memory as a real File: the
 * plugin only has a content: URI, which the page can't read lazily by
 * itself, so there's no streaming this one the way a local file upload
 * streams from disk. Tells the plugin it's done (closing the stream,
 * whether this finished, failed, or was cancelled) either way.
 */
export async function importShare(item, onProgress) {
  const pieces = [];
  let got = 0;
  try {
    for (;;) {
      const { data, eof } = await call('share_read', { id: item.id });
      const bytes = unbase64(data);
      if (bytes.length) {
        pieces.push(bytes);
        got += bytes.length;
        onProgress?.(item.size > 0 ? Math.min(1, got / item.size) : 0);
      }
      if (eof) break;
    }
  } finally {
    await call('share_done', { id: item.id }).catch(() => {});
  }
  return new File(pieces, item.name, { type: item.mime, lastModified: Date.now() });
}

/** Decline a pending share without reading it (it was never opened with share_read, so there's no stream to close, only the queue entry). */
export const cancelShare = (id) => call('share_done', { id }).catch(() => {});

// Android's back gesture (crates/thencloud-app/android-plugin) first asks the
// page, through this, to close what's on top: a menu, a dialog, then other
// things that close on Escape (marked data-escape). Only when there's
// nothing does it go back in history, or leave the app.
if (inAndroidApp) {
  const escape = (el) => el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
  globalThis.thencloudBack = () => {
    const menu = document.querySelector('[role=menu][data-escape]');
    if (menu) return escape(menu), true;
    const dialog = [...document.querySelectorAll('dialog[open]')].pop();
    if (dialog) {
      const e = new Event('cancel', { cancelable: true });
      if (dialog.dispatchEvent(e)) dialog.close();
      return true;
    }
    const other = [...document.querySelectorAll('[data-escape]')].pop();
    if (other) return escape(other), true;
    return false;
  };
}
