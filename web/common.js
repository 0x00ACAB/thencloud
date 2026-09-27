// Shared helpers for the thencloud proof-of-concept web client.
// All cryptography happens in WASM (thencloud-crypto); this file only does
// networking and DOM plumbing.

import init, * as tc from './pkg/thencloud_wasm.js';

export { tc };
export const ready = init();

export class ApiError extends Error {
  constructor(status, body) {
    super(body?.message || `HTTP ${status}`);
    this.status = status;
    this.code = body?.error;
  }
}

/**
 * Call the API. `body` is sent as JSON, `raw` as bytes. Returns parsed JSON,
 * a Uint8Array for binary responses, or null for 204.
 */
export async function api(method, path, { token, body, raw, headers } = {}) {
  const h = { ...headers };
  if (token) h['Authorization'] = `Bearer ${token}`;
  let payload;
  if (raw !== undefined) {
    payload = raw;
    h['Content-Type'] = 'application/octet-stream';
  } else if (body !== undefined) {
    payload = JSON.stringify(body);
    h['Content-Type'] = 'application/json';
  }
  const res = await fetch(path, {
    method,
    headers: h,
    body: payload,
    cache: 'no-store',
    referrerPolicy: 'no-referrer',
  });
  if (!res.ok) {
    let b = null;
    try { b = await res.json(); } catch { /* not json */ }
    throw new ApiError(res.status, b);
  }
  if (res.status === 204) return null;
  if ((res.headers.get('content-type') || '').includes('application/json')) return res.json();
  return new Uint8Array(await res.arrayBuffer());
}

export const b64 = (bytes) => tc.b64_encode(bytes);
export const unb64 = (s) => tc.b64_decode(s);

export function decryptMeta(nodeKey, node) {
  return JSON.parse(tc.decrypt_metadata(nodeKey, node.id, unb64(node.enc_metadata)));
}

export function encryptMeta(nodeKey, nodeId, meta) {
  return b64(tc.encrypt_metadata(nodeKey, nodeId, JSON.stringify(meta)));
}

export function unwrapChild(parentKey, node) {
  return tc.unwrap_node_key(parentKey, unb64(node.enc_key), node.id);
}

/**
 * Download and decrypt a file node chunk by chunk, then save it.
 * `chunkUrl(i)` builds the URL of chunk i; `opts` are passed to api().
 */
export async function downloadFile(node, nodeKey, chunkUrl, opts, onProgress) {
  const meta = decryptMeta(nodeKey, node);
  const v = node.version;
  const ck = tc.unwrap_content_key(nodeKey, unb64(v.enc_content_key), node.id, v.id);
  const parts = [];
  for (let i = 0; i < v.chunk_count; i++) {
    const enc = await api('GET', chunkUrl(i), opts);
    // Chunks are bound to (version, index, is_last): reordering, truncation
    // or substitution by the server makes this throw.
    parts.push(tc.decrypt_chunk(ck, v.id, i, i === v.chunk_count - 1, enc));
    onProgress?.(i + 1, v.chunk_count);
  }
  const blob = new Blob(parts, { type: meta.mime || 'application/octet-stream' });
  if (blob.size !== meta.size) throw new Error('decrypted size does not match metadata');
  saveBlob(blob, meta.name);
}

export function saveBlob(blob, name) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

export function fmtSize(n) {
  if (n == null) return '';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return `${i ? n.toFixed(1) : n} ${units[i]}`;
}

export function fmtTime(ms) {
  return ms ? new Date(ms).toLocaleString() : '';
}

/** Tiny DOM builder: el('button', { onclick }, 'text'). */
export function el(tag, props = {}, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
    else if (v !== undefined && v !== null && v !== false) e[k] = v;
  }
  for (const c of children.flat()) {
    if (c != null && c !== false) e.append(c instanceof Node ? c : String(c));
  }
  return e;
}

export const $ = (id) => document.getElementById(id);

/** Let the browser repaint before a long synchronous step (e.g. Argon2). */
export const tick = () => new Promise((r) => setTimeout(r, 20));
