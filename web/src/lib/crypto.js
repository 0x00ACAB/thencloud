// All cryptography comes from the Rust `thencloud-crypto` crate, compiled to
// WASM. This module only adapts it for the UI.

import init, * as tc from '../wasm/thencloud_wasm.js';

export { tc };
export const ready = init();

export const b64 = (bytes) => tc.b64_encode(bytes);
export const unb64 = (s) => tc.b64_decode(s);

export function decryptMeta(nodeKey, node) {
  return JSON.parse(tc.decrypt_metadata(nodeKey, node.id, unb64(node.enc_metadata)));
}

/**
 * Encrypt a node's metadata, stamping it with when it changed (`meta.changed`
 * is set on the object passed in, so the caller's copy has it too). The
 * server records times to the hour only; this one is exact.
 */
export function encryptMeta(nodeKey, nodeId, meta) {
  meta.changed = Date.now();
  return b64(tc.encrypt_metadata(nodeKey, nodeId, JSON.stringify(meta)));
}

export function unwrapChild(parentKey, node) {
  return tc.unwrap_node_key(parentKey, unb64(node.enc_key), node.id);
}

/** Decrypt the listing of a folder whose key we hold. */
export function decryptChildren(folderKey, nodes) {
  return nodes.map((node) => {
    const key = unwrapChild(folderKey, node);
    return { node, key, meta: decryptMeta(key, node) };
  });
}

// Argon2id runs in a worker so the page stays responsive while it works.
let worker;
let seq = 0;
const pending = new Map();

export function deriveAccountKeys(password, salt, params) {
  if (!worker) {
    worker = new Worker(new URL('./kdf.worker.js', import.meta.url), { type: 'module' });
    worker.onmessage = ({ data }) => {
      const p = pending.get(data.id);
      pending.delete(data.id);
      if (data.error) p.reject(new Error(data.error));
      else p.resolve({ authKey: data.authKey, kek: data.kek });
    };
  }
  const id = ++seq;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, password, salt, params: JSON.stringify(params) });
  });
}

/**
 * A link password's auth key (what the server checks) and KEK (what the
 * node key is wrapped under), from the password and the link's secret (the
 * owner's identity for a file drop). Argon2 runs in the worker.
 */
export async function deriveLinkKeys(secret, password) {
  const params = JSON.parse(tc.default_kdf_params());
  const k = await deriveAccountKeys(password, tc.link_password_salt(secret), params);
  const r = tc.derive_link_keys(secret, k.authKey, k.kek);
  const out = { auth: r.auth_key, kek: r.kek };
  r.free();
  return out;
}

/**
 * Download and decrypt a file chunk by chunk. Chunks are bound to
 * (version, index, is_last), so a server that reorders, truncates or swaps
 * them makes decryption fail.
 */
export async function fetchFile(node, nodeKey, getChunk, onProgress) {
  const meta = decryptMeta(nodeKey, node);
  const v = node.version;
  const ck = tc.unwrap_content_key(nodeKey, unb64(v.enc_content_key), node.id, v.id);
  const parts = [];
  for (let i = 0; i < v.chunk_count; i++) {
    const enc = await getChunk(i);
    parts.push(tc.decrypt_chunk(ck, v.id, i, i === v.chunk_count - 1, enc));
    onProgress?.((i + 1) / v.chunk_count);
  }
  // Contents are padded with zeros past the real size (see padded_size).
  const all = new Blob(parts);
  if (all.size < meta.size) throw new Error('Decrypted size does not match the file metadata.');
  const blob = all.slice(0, meta.size, meta.mime || 'application/octet-stream');
  return { blob, meta };
}

/** Encrypt piece `i` of `file`, padded with zeros up to `padded` bytes in all. */
export async function encryptPiece(file, i, padded, contentKey, versionId, count) {
  const size = tc.chunk_size();
  const plain = new Uint8Array(Math.min(size, padded - i * size));
  plain.set(new Uint8Array(await file.slice(i * size, (i + 1) * size).arrayBuffer()));
  return tc.encrypt_chunk(contentKey, versionId, i, i === count - 1, plain);
}

/**
 * A file to read one plaintext piece at a time (for streaming):
 * { meta, size, count, chunkSize, read(index) -> Uint8Array }. Each piece is
 * checked against the size in the metadata, so a stream can't come out
 * longer or shorter than announced.
 */
export function openFile(node, nodeKey, getChunk) {
  const meta = decryptMeta(nodeKey, node);
  const v = node.version;
  const ck = tc.unwrap_content_key(nodeKey, unb64(v.enc_content_key), node.id, v.id);
  const chunkSize = tc.chunk_size();
  const size = meta.size;
  return {
    meta,
    size,
    count: v.chunk_count,
    chunkSize,
    async read(i) {
      const last = i === v.chunk_count - 1;
      const plain = tc.decrypt_chunk(ck, v.id, i, last, await getChunk(i));
      // Every piece but the last is full; the file ends inside the padding.
      if ((!last && plain.length !== chunkSize) || (last && i * chunkSize + plain.length < size)) {
        throw new Error('Decrypted size does not match the file metadata.');
      }
      return plain.subarray(0, Math.max(0, Math.min(plain.length, size - i * chunkSize)));
    },
  };
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
