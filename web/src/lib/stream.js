// Serve decrypted data to the browser as a stream, through the service
// worker in public/sw.js: for downloads that don't fit in memory, video and
// audio that play while they load, and zips. Decryption happens here, in
// the page; the worker only relays the plaintext pieces this page hands it
// and never sees a key. Where there's no worker (a private window in some
// browsers, a hard reload), callers fall back to building a Blob.

import { nativeSaves, savePieces, filePieces } from './native.js';

let ready = null;

/** Resolves to true once this page is controlled by the stream worker. */
export function streamsAvailable() {
  return (ready ??= (async () => {
    if (!('serviceWorker' in navigator) || !window.isSecureContext) return false;
    try {
      await navigator.serviceWorker.register('/sw.js', { scope: '/' });
      if (navigator.serviceWorker.controller) return true;
      // First visit: the worker claims this page when it activates.
      return await new Promise((resolve) => {
        const t = setTimeout(() => resolve(false), 4000);
        navigator.serviceWorker.addEventListener('controllerchange', () => (clearTimeout(t), resolve(true)), { once: true });
      });
    } catch {
      return false;
    }
  })());
}

// Streams this page serves, by id. The worker asks for one by posting
// { type: 'thencloud-stream', id } with a port; that port then carries the
// piece requests for one response.
const served = new Map(); // id -> { info, onRequest }

if ('serviceWorker' in navigator) {
  navigator.serviceWorker.addEventListener('message', (e) => {
    if (e.data?.type !== 'thencloud-stream' || !e.ports[0]) return;
    const port = e.ports[0];
    const s = served.get(e.data.id);
    if (!s) return port.postMessage({ unknown: true });
    port.onmessage = async (m) => {
      const msg = m.data;
      if (msg.cancel) return s.onRequest(msg);
      try {
        const r = await s.onRequest(msg);
        if (r?.done) port.postMessage({ done: true });
        else {
          const buf = r.buffer.byteLength === r.byteLength && r.byteOffset === 0 ? r.buffer : r.slice().buffer;
          port.postMessage({ data: buf }, [buf]);
        }
      } catch (err) {
        port.postMessage({ error: String(err?.message || err) });
      }
    };
    port.postMessage({ info: s.info });
  });
  navigator.serviceWorker.startMessages();
}

function register(info, onRequest) {
  const id = crypto.randomUUID();
  served.set(id, { info, onRequest });
  return {
    url: `/_stream/${id}`,
    close: () => served.delete(id),
  };
}

/**
 * A file of `size` bytes in pieces of `chunkSize` (the last may be
 * shorter), read with `read(index)` -> Uint8Array. Supports range requests,
 * so video can seek. Returns { url, close }.
 */
export function serveFile({ size, chunkSize, type, name = 'file', download = false, read, oncancel }) {
  // Keep the last couple of pieces: players often re-read around a seek.
  const recent = new Map();
  return register({ size, chunkSize, type, name, download }, async ({ index, cancel }) => {
    if (cancel) return oncancel?.();
    if (!recent.has(index)) {
      recent.set(index, read(index));
      if (recent.size > 3) recent.delete(recent.keys().next().value);
    }
    try {
      return new Uint8Array(await recent.get(index));
    } catch (e) {
      recent.delete(index);
      throw e;
    }
  });
}

/**
 * Whether streamDownload and streamParts can save without holding the whole
 * file in memory: through the stream worker, or the Android app's plugin.
 */
export const canSaveStreams = async () => nativeSaves || (await streamsAvailable());

/** A stream of unknown length from `next()` -> Uint8Array, or null at the end. */
export function serveSequence({ type, name, next, oncancel }) {
  return register({ size: null, type, name, download: true }, async (m) => {
    if (m.cancel) return oncancel?.();
    const part = await next();
    return part ? part : { done: true };
  });
}

/**
 * Start a download of a served stream. A hidden frame loads it (a link with
 * `download` would skip the worker in some browsers); the response says
 * "attachment", so the browser saves it instead of showing it.
 */
export function startDownload(url) {
  const frame = document.createElement('iframe');
  frame.hidden = true;
  frame.src = url;
  document.body.append(frame);
  setTimeout(() => frame.remove(), 120_000);
}

/**
 * Download `file` (see openFile in crypto.js) as a stream. Resolves once the
 * browser has taken the last piece; rejects if it fails or is cancelled.
 */
export function streamDownload(file, onProgress) {
  if (nativeSaves) return savePieces(filePieces(file, onProgress), file.meta.name);
  return new Promise((resolve, reject) => {
    let done = false;
    const finish = (err) => {
      if (done) return;
      done = true;
      setTimeout(() => s.close(), 60_000);
      if (err) reject(err);
      else resolve();
    };
    const s = serveFile({
      size: file.size,
      chunkSize: file.chunkSize,
      type: 'application/octet-stream',
      name: file.meta.name,
      download: true,
      read: async (i) => {
        try {
          const data = await file.read(i);
          onProgress?.((i + 1) / file.count);
          if ((i + 1) * file.chunkSize >= file.size) queueMicrotask(() => finish());
          return data;
        } catch (e) {
          finish(e);
          throw e;
        }
      },
      oncancel: () => finish(Object.assign(new Error('The download was cancelled.'), { name: 'AbortError' })),
    });
    startDownload(s.url);
  });
}

/** Save the pieces an async iterator yields as one download. Resolves when it's all been taken. */
export function streamParts(parts, name, type = 'application/octet-stream') {
  if (nativeSaves) return savePieces(parts, name, type);
  return new Promise((resolve, reject) => {
    let finished = false;
    const finish = (err) => {
      if (finished) return;
      finished = true;
      setTimeout(() => s.close(), 60_000);
      if (err) reject(err);
      else resolve();
    };
    const s = serveSequence({
      type,
      name,
      next: async () => {
        try {
          const r = await parts.next();
          if (r.done) finish();
          return r.done ? null : r.value;
        } catch (e) {
          finish(e);
          throw e;
        }
      },
      oncancel: () => {
        parts.return?.();
        finish(Object.assign(new Error('The download was cancelled.'), { name: 'AbortError' }));
      },
    });
    startDownload(s.url);
  });
}
