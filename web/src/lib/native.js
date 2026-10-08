// The page's side of the Android app's plugin
// (crates/thencloud-app/android-plugin).
//
// Saving files: Android's WebView drops a download of a blob: URL, and has
// no service worker for the app's pages, so there the page hands each
// decrypted piece to the plugin, which writes it to Downloads. The only other
// native calls the app's pages may make are for what music is playing
// (nowplaying.svelte.js; capabilities/android.json).

import { inApp } from './server.svelte.js';
import { t } from './i18n.svelte.js';

export const inAndroidApp = inApp && /\bAndroid\b/.test(navigator.userAgent);

/** True in the Android app, where files are saved through the plugin. */
export const nativeSaves = inAndroidApp && !!globalThis.__TAURI_INTERNALS__;

export const call = (cmd, args) => globalThis.__TAURI_INTERNALS__.invoke(`plugin:thencloud|${cmd}`, args);

// The bridge carries JSON, so pieces go over as base64, in slices that keep
// each message small.
const SLICE = 512 * 1024;

export function base64(bytes) {
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

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
