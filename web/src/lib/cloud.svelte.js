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
import { rememberSession, rememberedSession, forgetSession } from './remember.js';

export const session = $state({
  token: null,
  me: null,
  fingerprint: '',
  /** Signed in with "Keep me signed in on this browser". */
  remembered: false,
});

let mk = null; // master key
let sk = null; // X25519 secret key
const keyCache = new Map(); // node id -> node key

/**
 * Authenticated request. A 401 while signed in means this session was ended
 * (signed out from another device, password changed, expired): reload to
 * drop every key from memory, and let the sign-in screen say why.
 */
async function api(method, path, opts = {}) {
  try {
    return await request(method, path, { ...opts, token: session.token });
  } catch (e) {
    // `invalid_credentials` (a wrong current password) is also a 401, but
    // the session is fine; only `unauthorized` means it's gone.
    if (e?.status === 401 && e.code === 'unauthorized' && session.token) {
      session.token = null;
      await forgetSession();
      try {
        sessionStorage.setItem('signedOut', '1');
      } catch {
        /* the reload still happens */
      }
      location.reload();
      await new Promise(() => {}); // nothing more happens on this page
    }
    throw e;
  }
}

/** "Chrome on Linux". */
const deviceName = () => {
  const ua = navigator.userAgent;
  const browser = /Firefox\//.test(ua) ? 'Firefox' : /Edg\//.test(ua) ? 'Edge' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : 'Browser';
  const os = /Windows/.test(ua) ? 'Windows' : /Mac OS X/.test(ua) ? 'macOS' : /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Linux/.test(ua) ? 'Linux' : '';
  return `${browser}${os ? ` on ${os}` : ''}`;
};

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

export async function register(username, password, invite = null, remember = false) {
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
      invite: invite || undefined,
    },
  });
  kp.free();
  start(s, ak.kek);
  if (remember) await keepSignedIn(s.token);
}

export async function login(username, password, remember = false) {
  const pre = await request('POST', '/api/auth/prelogin', { body: { username } });
  const ak = await deriveAccountKeys(password, unb64(pre.kdf_salt), pre.kdf_params);
  const s = await request('POST', '/api/auth/login', {
    body: { username, auth_key: b64(ak.authKey), device_name: deviceName() },
  });
  start(s, ak.kek);
  if (remember) await keepSignedIn(s.token);
}

function start(s, kek) {
  startWithMasterKey(s, tc.unwrap_master_key(kek, unb64(s.me.keys.enc_master_key)));
}

function startWithMasterKey(s, masterKey) {
  const keys = s.me.keys;
  const secret = tc.unwrap_private_key(masterKey, unb64(keys.enc_private_key));
  const pub = tc.public_key_from_secret(secret);
  if (b64(pub) !== keys.public_key) {
    throw new Error('The public key stored on the server does not match your private key. Refusing to continue.');
  }
  mk = masterKey;
  sk = secret;
  keyCache.clear();
  contacts = null;
  session.token = s.token;
  session.me = s.me;
  session.fingerprint = tc.fingerprint(pub);
}

export async function refreshMe() {
  session.me = await api('GET', '/api/me');
}

// ---------------------------------------------------------------------------
// "Keep me signed in on this browser" (see lib/remember.js)
// ---------------------------------------------------------------------------

async function keepSignedIn(token) {
  try {
    await rememberSession({ userId: session.me.user_id, token, masterKey: mk });
    session.remembered = true;
  } catch {
    // Storage blocked (private window, strict settings): just don't remember.
    session.remembered = false;
  }
}

/**
 * Sign back in from a saved session, if there is one. Returns true if that
 * worked. A session the server no longer knows is forgotten.
 */
export async function resume() {
  const saved = await rememberedSession();
  if (!saved) return false;
  let me;
  try {
    me = await request('GET', '/api/me', { token: saved.token });
  } catch (e) {
    saved.masterKey.fill(0);
    if (e?.status === 401 || e?.status === 403) {
      await forgetSession();
      try {
        sessionStorage.setItem('signedOut', '1');
      } catch {
        /* just no message */
      }
    }
    return false;
  }
  if (me.user_id !== saved.userId) {
    saved.masterKey.fill(0);
    await forgetSession();
    return false;
  }
  try {
    // On success this array *is* the master key in memory; don't zero it.
    startWithMasterKey({ token: saved.token, me }, saved.masterKey);
  } catch (e) {
    saved.masterKey.fill(0);
    await forgetSession();
    throw e;
  }
  session.remembered = true;
  return true;
}

/** Stop keeping this browser signed in (the current session carries on). */
export async function forgetThisBrowser() {
  await forgetSession();
  session.remembered = false;
}

export async function logout() {
  await forgetSession();
  try {
    await api('POST', '/api/auth/logout');
  } catch {
    /* the session is dropped locally either way */
  }
  // A full reload is the most reliable way to drop every key from memory.
  location.reload();
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/** Signed-in sessions, most recently active first. */
export const listSessions = () => api('GET', '/api/sessions');
export const revokeSession = (id) => api('DELETE', `/api/sessions/${encodeURIComponent(id)}`);
export const revokeOtherSessions = () => api('DELETE', '/api/sessions');

/** What the sign-in screen may offer (registration open, invite-only or closed). */
export const authOptions = () => request('GET', '/api/auth/options');

// ---------------------------------------------------------------------------
// Recovery key: an optional second way to unwrap the master key. The key is
// made and shown here and never sent; the server gets a hash of its auth
// part and the master key wrapped under its KEK.
// ---------------------------------------------------------------------------

/** The auth key for `password`, to prove it to the server. */
async function authKeyFor(password) {
  const pre = await request('POST', '/api/auth/prelogin', { body: { username: session.me.username } });
  const ak = await deriveAccountKeys(password, unb64(pre.kdf_salt), pre.kdf_params);
  return b64(ak.authKey);
}

/** Create (or replace) the recovery key. Returns it as text, to show once. */
export async function createRecoveryKey(password) {
  const current = await authKeyFor(password);
  const rk = tc.random_key();
  const d = tc.derive_recovery_keys(rk);
  try {
    session.me = await api('POST', '/api/auth/recovery', {
      body: {
        current_auth_key: current,
        recovery_auth_key: b64(d.auth_key),
        enc_master_key_recovery: b64(tc.wrap_master_key_recovery(d.kek, mk)),
      },
    });
    return tc.encode_recovery_key(rk);
  } finally {
    d.free();
    rk.fill(0);
  }
}

export async function removeRecoveryKey(password) {
  session.me = await api('DELETE', '/api/auth/recovery', { body: { current_auth_key: await authKeyFor(password) } });
}

/** Set a new password with a recovery key, then sign in. */
export async function recoverAccount(username, recoveryKey, newPassword, remember = false) {
  let rk;
  try {
    rk = tc.decode_recovery_key(recoveryKey);
  } catch {
    throw Object.assign(new Error("That recovery key isn't valid. Check it for typos."), { code: 'bad_recovery_key' });
  }
  const d = tc.derive_recovery_keys(rk);
  rk.fill(0);
  try {
    const recoveryAuth = b64(d.auth_key);
    const r = await request('POST', '/api/auth/recovery/unlock', { body: { username, recovery_auth_key: recoveryAuth } });
    const master = tc.unwrap_master_key_recovery(d.kek, unb64(r.enc_master_key_recovery));
    const salt = tc.random_salt();
    const params = JSON.parse(tc.default_kdf_params());
    const nk = await deriveAccountKeys(newPassword, salt, params);
    const s = await request('POST', '/api/auth/recovery/reset', {
      body: {
        username,
        recovery_auth_key: recoveryAuth,
        new_auth_key: b64(nk.authKey),
        new_kdf_salt: b64(salt),
        new_kdf_params: params,
        new_enc_master_key: b64(tc.wrap_master_key(nk.kek, master)),
        device_name: deviceName(),
      },
    });
    master.fill(0);
    start(s, nk.kek);
    if (remember) await keepSignedIn(s.token);
  } finally {
    d.free();
  }
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
  return { id, key };
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

/** Download files and folders as one zip, decrypted and zipped in the browser. */
export async function downloadZip(entries, name, onProgress) {
  const { zipEntries } = await import('./zip.js');
  const blob = await zipEntries(entries, { list: (e) => listFolder(e.node.id, e.key), fetch: fetchEntry, onProgress });
  saveBlob(blob, name);
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
  let node;
  try {
    for (let i = 0; i < chunkCount; i++) {
      const plain = new Uint8Array(await file.slice(i * chunkSize, (i + 1) * chunkSize).arrayBuffer());
      const enc = tc.encrypt_chunk(contentKey, versionId, i, i === chunkCount - 1, plain);
      await api('PUT', `/api/uploads/${up.upload_id}/chunks/${i}`, { raw: enc });
      onProgress?.((i + 1) / chunkCount);
    }
    node = await api('POST', `/api/uploads/${up.upload_id}/finish`);
  } catch (e) {
    api('DELETE', `/api/uploads/${up.upload_id}`).catch(() => {});
    throw e;
  }
  keyCache.set(nodeId, nodeKey);
  return { node, key: nodeKey, meta };
}

/**
 * Save edited text as a new version of `entry`. Fails with a 409 if the file
 * changed since `entry` was loaded. Returns the updated entry.
 */
export function saveText(entry, text) {
  const file = new File([text], entry.meta.name, { type: entry.meta.mime || 'text/markdown', lastModified: Date.now() });
  return upload(file, { existing: entry });
}

// ---------------------------------------------------------------------------
// Sharing
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Verified contacts: public keys you've checked by fingerprint, pinned so a
// later key change is caught. Stored encrypted under the master key and
// bound to your account; the server can't read or alter them.
// ---------------------------------------------------------------------------

let contacts = null; // { data: { [username]: { public_key, verified_at } }, revision }
const enc = new TextEncoder();
const dec = new TextDecoder();

async function loadContacts() {
  if (contacts) return contacts;
  const r = await api('GET', '/api/me/contacts');
  let data = {};
  if (r.data) {
    try {
      data = JSON.parse(dec.decode(tc.decrypt_private_data(mk, session.me.user_id, 'contacts', unb64(r.data))));
    } catch {
      // Never fall back to an empty list: that would silently drop every pin.
      throw new Error("Your verified contacts couldn't be decrypted. They may have been tampered with.");
    }
  }
  contacts = { data, revision: r.revision };
  return contacts;
}

async function saveContacts(change) {
  for (let attempt = 0; ; attempt++) {
    const current = await loadContacts();
    const next = structuredClone(current.data);
    change(next);
    const sealed = tc.encrypt_private_data(mk, session.me.user_id, 'contacts', enc.encode(JSON.stringify(next)));
    try {
      const r = await api('PUT', '/api/me/contacts', { body: { data: b64(sealed), if_revision: current.revision } });
      contacts = { data: next, revision: r.revision };
      return;
    } catch (e) {
      // Changed on another device meanwhile: reload and apply again, once.
      if (e?.status === 409 && attempt === 0) contacts = null;
      else throw e;
    }
  }
}

/**
 * How `user` (from lookupUser) compares with what you verified:
 * { state: 'new' | 'verified' | 'changed', verifiedAt, pinnedFingerprint }.
 */
export async function contactStatus(user) {
  const c = (await loadContacts()).data[user.username];
  if (!c) return { state: 'new' };
  const same = c.public_key === b64(user.publicKey);
  return {
    state: same ? 'verified' : 'changed',
    verifiedAt: c.verified_at,
    pinnedFingerprint: tc.fingerprint(unb64(c.public_key)),
  };
}

/** Remember `user`'s current key as checked. */
export const verifyContact = (user) =>
  saveContacts((d) => (d[user.username] = { public_key: b64(user.publicKey), verified_at: Date.now() }));

export const forgetContact = (username) => saveContacts((d) => delete d[username]);

/** Verified contacts, A to Z, with fingerprints. */
export async function listContacts() {
  const { data } = await loadContacts();
  return Object.entries(data)
    .map(([username, c]) => ({ username, verifiedAt: c.verified_at, fingerprint: tc.fingerprint(unb64(c.public_key)) }))
    .sort((a, b) => a.username.localeCompare(b.username));
}

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

// ---------------------------------------------------------------------------
// Administration (admins only; accounts and counts, never content)
// ---------------------------------------------------------------------------

export const adminUsers = () => api('GET', '/api/admin/users');
export const adminUpdateUser = (id, body) => api('PATCH', `/api/admin/users/${encodeURIComponent(id)}`, { body });
export const adminDeleteUser = (id) => api('DELETE', `/api/admin/users/${encodeURIComponent(id)}`);
export const adminSettings = () => api('GET', '/api/admin/settings');
export const adminUpdateSettings = (body) => api('PATCH', '/api/admin/settings', { body });
export const adminInvites = () => api('GET', '/api/admin/invites');
export const adminCreateInvite = (days) => api('POST', '/api/admin/invites', { body: { days } });
export const adminDeleteInvite = (id) => api('DELETE', `/api/admin/invites/${encodeURIComponent(id)}`);
export const adminStats = () => api('GET', '/api/admin/stats');

/** Invite links carry the token in the fragment, which is never sent to the server. */
export const inviteUrl = (token) => `${location.origin}/#invite=${encodeURIComponent(token)}`;

// ---------------------------------------------------------------------------
// Tools that need the server: the video downloader (off unless an admin
// enables it). The server fetches the video and streams it here; it's then
// encrypted and uploaded like any other file, or just saved.
// ---------------------------------------------------------------------------

let tools = null;
/** { video_downloader, downloader_max_bytes, downloader_can_merge }, fetched once per session. */
export async function toolsInfo() {
  return (tools ??= await api('GET', '/api/tools').catch(() => ({ video_downloader: false })));
}
/** Forget the cached answer (after an admin changes the setting). */
export const resetToolsInfo = () => (tools = null);

export const videoInfo = (url) => api('POST', '/api/tools/video/info', { body: { url } });

/**
 * Download a video (kind 'video') or its audio ('audio') through the server.
 * Resolves to a Blob. `onProgress(bytes)`; stop with `signal`. A download
 * the server cut short (too large, failed) rejects instead of returning a
 * partial file.
 */
export async function downloadVideo(url, kind, { onProgress, signal } = {}) {
  let res;
  try {
    res = await fetch('/api/tools/video/download', {
      method: 'POST',
      headers: { Authorization: `Bearer ${session.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ url, kind }),
      cache: 'no-store',
      referrerPolicy: 'no-referrer',
      signal,
    });
  } catch (e) {
    if (e?.name === 'AbortError') throw e;
    throw new Error('Could not reach the server. Check your connection.');
  }
  if (!res.ok) {
    let body = null;
    try {
      body = await res.json();
    } catch {
      /* not json */
    }
    throw Object.assign(new Error(body?.message || `Download failed (HTTP ${res.status})`), { status: res.status, code: body?.error });
  }
  const parts = [];
  let received = 0;
  const reader = res.body.getReader();
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      parts.push(value);
      received += value.length;
      onProgress?.(received);
    }
  } catch (e) {
    if (e?.name === 'AbortError') throw e;
    throw new Error("The download didn't finish. The video may be too large for this server, or the site stopped sending it.");
  }
  if (!received) throw new Error('The site sent nothing back.');
  return new Blob(parts);
}
