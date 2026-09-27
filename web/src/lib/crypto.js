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

export function encryptMeta(nodeKey, nodeId, meta) {
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
  const blob = new Blob(parts, { type: meta.mime || 'application/octet-stream' });
  if (blob.size !== meta.size) throw new Error('Decrypted size does not match the file metadata.');
  return { blob, meta };
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
