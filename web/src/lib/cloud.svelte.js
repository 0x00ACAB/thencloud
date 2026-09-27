// Session state and every operation the app performs against the server.
//
// Decrypted keys (master key, private key, node keys) live only in memory,
// in this module. Nothing is written to localStorage or sessionStorage, so a
// reload signs you out.

import { request } from './api.js';
import {
  tc, b64, unb64, decryptMeta, encryptMeta, unwrapChild, decryptChildren,
  deriveAccountKeys, fetchFile, saveBlob,
} from './crypto.js';
import { sortEntries } from './format.js';

export const session = $state({
  token: null,
  me: null,
  fingerprint: '',
});

let mk = null; // master key
let sk = null; // X25519 secret key
const keyCache = new Map(); // node id -> node key

const api = (method, path, opts = {}) => request(method, path, { ...opts, token: session.token });

const deviceName = () => {
  const ua = navigator.userAgent;
  const browser = /Firefox\//.test(ua) ? 'Firefox' : /Edg\//.test(ua) ? 'Edge' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : 'Browser';
  const os = /Windows/.test(ua) ? 'Windows' : /Mac OS X/.test(ua) ? 'macOS' : /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Linux/.test(ua) ? 'Linux' : '';
  return `${browser}${os ? ` on ${os}` : ''}`;
};

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

export async function register(username, password) {
  const salt = tc.random_salt();
  const params = JSON.parse(tc.default_kdf_params());
  const ak = await deriveAccountKeys(password, salt, params);
  const masterKey = tc.random_key();
  const kp = tc.generate_keypair();
  const rootId = tc.new_id();
  const rootKey = tc.random_key();
  const s = await request('POST', '/api/auth/register', {
    body: {
      username,
      auth_key: b64(ak.authKey),
      kdf_salt: b64(salt),
      kdf_params: params,
      enc_master_key: b64(tc.wrap_master_key(ak.kek, masterKey)),
      public_key: b64(kp.public),
      enc_private_key: b64(tc.wrap_private_key(masterKey, kp.secret)),
      root: {
        id: rootId,
        enc_key: b64(tc.wrap_node_key(masterKey, rootKey, rootId)),
        enc_metadata: encryptMeta(rootKey, rootId, { name: 'My files', size: 0, mtime: Date.now() }),
      },
      device_name: deviceName(),
    },
  });
  kp.free();
  start(s, ak.kek);
}

export async function login(username, password) {
  const pre = await request('POST', '/api/auth/prelogin', { body: { username } });
  const ak = await deriveAccountKeys(password, unb64(pre.kdf_salt), pre.kdf_params);
  const s = await request('POST', '/api/auth/login', {
    body: { username, auth_key: b64(ak.authKey), device_name: deviceName() },
  });
  start(s, ak.kek);
}

function start(s, kek) {
  const keys = s.me.keys;
  const masterKey = tc.unwrap_master_key(kek, unb64(keys.enc_master_key));
  const secret = tc.unwrap_private_key(masterKey, unb64(keys.enc_private_key));
  const pub = tc.public_key_from_secret(secret);
  if (b64(pub) !== keys.public_key) {
    throw new Error('The public key stored on the server does not match your private key. Refusing to continue.');
  }
  mk = masterKey;
  sk = secret;
  keyCache.clear();
  session.token = s.token;
  session.me = s.me;
  session.fingerprint = tc.fingerprint(pub);
}

export async function refreshMe() {
  session.me = await api('GET', '/api/me');
}

export async function logout() {
  try {
    await api('POST', '/api/auth/logout');
  } catch {
    /* the session is dropped locally either way */
  }
  // A full reload is the most reliable way to drop every key from memory.
  location.reload();
}

export async function changePassword(current, next) {
  const pre = await request('POST', '/api/auth/prelogin', { body: { username: session.me.username } });
  const cur = await deriveAccountKeys(current, unb64(pre.kdf_salt), pre.kdf_params);
  const salt = tc.random_salt();
  const params = JSON.parse(tc.default_kdf_params());
  const nk = await deriveAccountKeys(next, salt, params);
  await api('POST', '/api/auth/password', {
    body: {
      current_auth_key: b64(cur.authKey),
      new_auth_key: b64(nk.authKey),
      new_kdf_salt: b64(salt),
      new_kdf_params: params,
      new_enc_master_key: b64(tc.wrap_master_key(nk.kek, mk)),
    },
  });
}

// ---------------------------------------------------------------------------
// Keys and browsing
// ---------------------------------------------------------------------------

/**
 * Fetch the path to a node and derive every key along it: from the master
 * key for our own files, from the share key for files shared with us.
 * Returns `{ items: [{ node, key, meta }], share }` from the top down.
 */
export async function resolvePath(id) {
  const p = await api('GET', `/api/nodes/${id}/path`);
  let key;
  const items = p.nodes.map((node, i) => {
    if (i === 0) {
      key = p.share
        ? tc.open_share_key(sk, unb64(p.share.wrapped_key), node.id)
        : tc.unwrap_node_key(mk, unb64(node.enc_key), node.id);
    } else {
      key = unwrapChild(key, node);
    }
    keyCache.set(node.id, key);
    return { node, key, meta: decryptMeta(key, node) };
  });
  return { items, share: p.share };
}

export async function keyOf(id) {
  if (!keyCache.has(id)) await resolvePath(id);
  return keyCache.get(id);
}

export async function listFolder(id, key) {
  const nodes = await api('GET', `/api/nodes/${id}/children`);
  const rows = decryptChildren(key, nodes);
  for (const r of rows) keyCache.set(r.node.id, r.key);
  return sortEntries(rows);
}

export async function createFolder(parentId, parentKey, name) {
  const id = tc.new_id();
  const key = tc.random_key();
  await api('POST', '/api/nodes/folder', {
    body: {
      id,
      parent_id: parentId,
      enc_key: b64(tc.wrap_node_key(parentKey, key, id)),
      enc_metadata: encryptMeta(key, id, { name, size: 0, mtime: Date.now() }),
    },
  });
  keyCache.set(id, key);
}

export async function rename(entry, name) {
  const { node, key, meta } = entry;
  await api('PATCH', `/api/nodes/${node.id}`, {
    body: { enc_metadata: encryptMeta(key, node.id, { ...meta, name }), if_revision: node.revision },
  });
}

/** Move a node: its key is re-wrapped under the destination folder's key. */
export async function move(entry, targetId, targetKey) {
  const { node, key } = entry;
  await api('PATCH', `/api/nodes/${node.id}`, {
    body: {
      parent_id: targetId,
      enc_key: b64(tc.wrap_node_key(targetKey, key, node.id)),
      if_revision: node.revision,
    },
  });
}

/** Move to the trash. Keys stay wrapped as they are, so it can come back. */
export async function trash(entry) {
  await api('DELETE', `/api/nodes/${entry.node.id}`);
}

// ---------------------------------------------------------------------------
// Trash
// ---------------------------------------------------------------------------

/**
 * Items in our trash. Each comes with its path from our root folder, which
 * is how we unwrap its key (trashed nodes can't be reached through /path).
 */
export async function trashItems() {
  const items = await api('GET', '/api/trash');
  return items.map((it) => {
    try {
      let key;
      const chain = it.path.map((node, i) => {
        key = i === 0 ? tc.unwrap_node_key(mk, unb64(node.enc_key), node.id) : unwrapChild(key, node);
        return { node, key, meta: decryptMeta(key, node) };
      });
      const entry = chain[chain.length - 1];
      return { ...it, entry, location: chain.slice(0, -1).map((c) => c.meta.name) };
    } catch (e) {
      return { ...it, error: String(e?.message || e) };
    }
  });
}

/**
 * Restore to the original folder. If that folder is gone or in the trash
 * too, restore into the root instead, re-wrapping the key for it.
 * Returns the name of the folder it went back into, or null for "original".
 */
export async function restoreFromTrash(item) {
  const id = item.node.id;
  try {
    await api('POST', `/api/trash/${id}/restore`, { body: {} });
    return null;
  } catch (e) {
    if (e.code !== 'parent_unavailable') throw e;
  }
  const rootId = session.me.keys.root_node_id;
  const rootKey = await keyOf(rootId);
  await api('POST', `/api/trash/${id}/restore`, {
    body: { parent_id: rootId, enc_key: b64(tc.wrap_node_key(rootKey, item.entry.key, id)) },
  });
  return 'My files';
}

/** Undo a delete we just did (owner only). */
export const untrash = (entry) => api('POST', `/api/trash/${entry.node.id}/restore`, { body: {} });

export const purgeFromTrash = (id) => api('DELETE', `/api/trash/${id}`);
export const emptyTrash = () => api('DELETE', '/api/trash');

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

/** Version history, newest first, each with its decrypted metadata. */
export async function versions(entry) {
  const list = await api('GET', `/api/nodes/${entry.node.id}/versions`);
  return list.map((v) => {
    let meta = null;
    try {
      meta = JSON.parse(tc.decrypt_metadata(entry.key, entry.node.id, unb64(v.enc_metadata)));
    } catch {
      /* shown as unreadable */
    }
    return { ...v, meta };
  });
}

/** A version as a node-shaped object that fetchFile() understands. */
const versionNode = (entry, v) => ({
  ...entry.node,
  enc_metadata: v.enc_metadata,
  version: { id: v.id, enc_content_key: v.enc_content_key, chunk_count: v.chunk_count, size: v.size, created_at: v.created_at },
});

export async function downloadVersion(entry, v, onProgress) {
  const { blob, meta } = await fetchFile(
    versionNode(entry, v),
    entry.key,
    (i) => api('GET', `/api/nodes/${entry.node.id}/versions/${v.id}/chunks/${i}`),
    onProgress,
  );
  saveBlob(blob, meta.name);
}

/** Make `v` current. The name stays as it is now; size and mtime come from the version. */
export async function restoreVersion(entry, v) {
  const meta = { ...entry.meta, size: v.meta.size, mtime: v.meta.mtime, mime: v.meta.mime ?? entry.meta.mime };
  return api('POST', `/api/nodes/${entry.node.id}/versions/${v.id}/restore`, {
    body: { enc_metadata: encryptMeta(entry.key, entry.node.id, meta), if_revision: entry.node.revision },
  });
}

export const deleteVersion = (entry, v) => api('DELETE', `/api/nodes/${entry.node.id}/versions/${v.id}`);

/** Download and decrypt a file's current version into memory. */
export function fetchEntry(entry, onProgress) {
  const { node, key } = entry;
  return fetchFile(node, key, (i) => api('GET', `/api/nodes/${node.id}/chunks/${i}`), onProgress);
}

export async function download(entry, onProgress) {
  const { blob, meta } = await fetchEntry(entry, onProgress);
  saveBlob(blob, meta.name);
}

/**
 * Encrypt and upload a File. For a new file pass `{ parentId, parentKey }`,
 * for a new version pass `{ existing: entry }`.
 */
export async function upload(file, { parentId, parentKey, existing }, onProgress) {
  const nodeId = existing ? existing.node.id : tc.new_id();
  const nodeKey = existing ? existing.key : tc.random_key();
  const versionId = tc.new_id();
  const contentKey = tc.random_key();
  const chunkCount = tc.chunk_count(file.size);
  const chunkSize = tc.chunk_size();
  const meta = {
    name: existing ? existing.meta.name : file.name,
    mime: file.type || null,
    size: file.size,
    mtime: file.lastModified || Date.now(),
  };

  const up = await api('POST', '/api/uploads', {
    body: {
      node_id: nodeId,
      parent_id: existing ? undefined : parentId,
      enc_key: existing ? undefined : b64(tc.wrap_node_key(parentKey, nodeKey, nodeId)),
      enc_metadata: encryptMeta(nodeKey, nodeId, meta),
      version_id: versionId,
      enc_content_key: b64(tc.wrap_content_key(nodeKey, contentKey, nodeId, versionId)),
      chunk_count: chunkCount,
      if_revision: existing ? existing.node.revision : undefined,
    },
  });
  try {
    for (let i = 0; i < chunkCount; i++) {
      const plain = new Uint8Array(await file.slice(i * chunkSize, (i + 1) * chunkSize).arrayBuffer());
      const enc = tc.encrypt_chunk(contentKey, versionId, i, i === chunkCount - 1, plain);
      await api('PUT', `/api/uploads/${up.upload_id}/chunks/${i}`, { raw: enc });
      onProgress?.((i + 1) / chunkCount);
    }
    await api('POST', `/api/uploads/${up.upload_id}/finish`);
  } catch (e) {
    api('DELETE', `/api/uploads/${up.upload_id}`).catch(() => {});
    throw e;
  }
  keyCache.set(nodeId, nodeKey);
}

// ---------------------------------------------------------------------------
// Sharing
// ---------------------------------------------------------------------------

export async function lookupUser(username) {
  const u = await api('GET', `/api/users/${encodeURIComponent(username.trim())}/public-key`);
  const publicKey = unb64(u.public_key);
  return { username: u.username, publicKey, fingerprint: tc.fingerprint(publicKey) };
}

export async function share(entry, user, permission) {
  await api('POST', '/api/shares', {
    body: {
      node_id: entry.node.id,
      recipient: user.username,
      wrapped_key: b64(tc.seal_share_key(user.publicKey, entry.key, entry.node.id)),
      permission,
    },
  });
}

export async function incomingShares() {
  const shares = await api('GET', '/api/shares/incoming');
  return shares.map((s) => {
    try {
      const key = tc.open_share_key(sk, unb64(s.wrapped_key), s.node.id);
      keyCache.set(s.node.id, key);
      return { ...s, entry: { node: s.node, key, meta: decryptMeta(key, s.node) }, ownerFingerprint: tc.fingerprint(unb64(s.owner_public_key)) };
    } catch (e) {
      return { ...s, error: String(e?.message || e) };
    }
  });
}

/** Resolve a node id to a decrypted entry, or null if it's gone. */
async function entryFor(nodeId) {
  try {
    const { items } = await resolvePath(nodeId);
    return items[items.length - 1];
  } catch {
    return null;
  }
}

export async function outgoingShares(nodeId) {
  const q = nodeId ? `?node_id=${encodeURIComponent(nodeId)}` : '';
  const shares = await api('GET', `/api/shares/outgoing${q}`);
  return Promise.all(shares.map(async (s) => ({ ...s, entry: nodeId ? null : await entryFor(s.node_id) })));
}

export const setSharePermission = (id, permission) => api('PATCH', `/api/shares/${id}`, { body: { permission } });
export const deleteShare = (id) => api('DELETE', `/api/shares/${id}`);

/**
 * The key goes in the URL fragment (after #). Browsers never send the
 * fragment to the server, so it only ever exists on the client.
 */
export function linkUrl(token, nodeKey) {
  return `${location.origin}/s/${token}#${b64(nodeKey)}`;
}

export async function createLink(entry, { password, expiresAt }) {
  const link = await api('POST', '/api/links', {
    body: { node_id: entry.node.id, password: password || null, expires_at: expiresAt ?? null },
  });
  return { ...link, url: linkUrl(link.token, entry.key) };
}

export async function links(nodeId) {
  const q = nodeId ? `?node_id=${encodeURIComponent(nodeId)}` : '';
  const list = await api('GET', `/api/links${q}`);
  return Promise.all(
    list.map(async (l) => {
      const entry = await entryFor(l.node_id);
      return { ...l, entry, url: entry ? linkUrl(l.token, entry.key) : null };
    }),
  );
}

export const deleteLink = (id) => api('DELETE', `/api/links/${id}`);
