// Session state and every operation the app performs against the server.
//
// Decrypted keys (master key, private key, node keys) live only in memory,
// in this module. Nothing is written to localStorage or sessionStorage, so a
// reload signs you out.

import { request, allChildren } from './api.js';
import { t } from './i18n.svelte.js';
import {
  tc, b64, unb64, decryptMeta, encryptMeta, unwrapChild, decryptChildren,
  deriveAccountKeys, deriveLinkKeys, fetchFile, openFile, encryptPiece, saveBlob,
} from './crypto.js';
import { sortEntries } from './format.js';
import { canThumbnail, makeThumbnail, isJpeg } from './thumbnail.js';
import { photoTaken } from './exif.js';
import { wordsOf, queryWords, matchesWords } from './fulltext.js';
import { previewKind } from './preview.js';
import { rememberSession, rememberedSession, forgetSession } from './remember.js';
import { streamsAvailable, streamDownload } from './stream.js';
import { createPasskey, usePasskey, passkeysSupported } from './passkeys.js';

export const session = $state({
  token: null,
  me: null,
  fingerprint: '',
  /** Signed in with "Keep me signed in on this browser". */
  remembered: false,
});

let mk = null; // master key
let sk = null; // X25519 secret key, followed by the ML-KEM seed when there is one
const keyCache = new Map(); // node id -> node key

/**
 * Authenticated request. A 401 while signed in means this session was ended
 * (signed out from another device, password changed, expired): reload to
 * drop every key from memory, and let the sign-in screen say why.
 */
async function api(method, path, opts = {}) {
  try {
    const out = await request(method, path, { ...opts, token: session.token });
    // Any change may move, add or remove something the search index holds.
    if (method !== 'GET') index.clear();
    return out;
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
  const pq = tc.generate_pq_keypair();
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
      pq_public_key: b64(pq.public),
      enc_pq_private_key: b64(tc.wrap_pq_private_key(masterKey, pq.secret)),
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
  pq.free();
  start(s, ak.kek);
  if (remember) await keepSignedIn(s.token);
}

/**
 * Sign in with a password. Returns null when that's done, or, when the
 * account asks for a second factor, `{ totp, passkey, withCode, withPasskey }`
 * to finish with one.
 */
export async function login(username, password, remember = false) {
  const pre = await request('POST', '/api/auth/prelogin', { body: { username } });
  const ak = await deriveAccountKeys(password, unb64(pre.kdf_salt), pre.kdf_params);
  const s = await request('POST', '/api/auth/login', {
    body: { username, auth_key: b64(ak.authKey), device_name: deviceName() },
  });
  if (!s.second_factor) {
    start(s, ak.kek);
    if (remember) await keepSignedIn(s.token);
    return null;
  }
  const sf = s.second_factor;
  const finish = async (body) => {
    const r = await request('POST', '/api/auth/login/second-factor', { body: { ticket: sf.ticket, ...body } });
    start(r, ak.kek);
    if (remember) await keepSignedIn(r.token);
  };
  return {
    totp: sf.totp,
    passkey: !!sf.passkey && passkeysSupported(),
    withCode: (code) => finish({ totp_code: code }),
    withPasskey: async () => {
      const a = await usePasskey({ challenge: unb64(sf.passkey.challenge), allow: sf.passkey.allow_credentials.map(unb64) });
      await finish({ passkey: assertionBody(a) });
    },
  };
}

const assertionBody = (a) => ({
  credential_id: b64(a.id),
  client_data_json: b64(a.clientDataJSON),
  authenticator_data: b64(a.authenticatorData),
  signature: b64(a.signature),
  user_handle: a.userHandle ? b64(a.userHandle) : undefined,
});

/**
 * Sign in with a passkey alone. Its PRF output unwraps the master key; the
 * server hands out the wrapped key only after checking the passkey.
 */
export async function loginWithPasskey(remember = false) {
  const o = await request('POST', '/api/auth/passkey/options');
  const a = await usePasskey({ challenge: unb64(o.challenge), prfSalt: tc.passkey_prf_salt(), verify: 'required' });
  if (!a.prf) {
    throw new Error("This browser or passkey can't unlock your files on its own. Sign in with your password.");
  }
  try {
    const s = await request('POST', '/api/auth/passkey/login', {
      body: { challenge: o.challenge, assertion: assertionBody(a), device_name: deviceName() },
    });
    startWithMasterKey(s, tc.unwrap_master_key_passkey(a.prf, unb64(s.enc_master_key), unb64(s.credential_id)));
    if (remember) await keepSignedIn(s.token);
  } finally {
    a.prf.fill(0);
  }
}

function start(s, kek) {
  startWithMasterKey(s, tc.unwrap_master_key(kek, unb64(s.me.keys.enc_master_key)));
}

const concat = (a, b) => {
  const out = new Uint8Array(a.length + b.length);
  out.set(a);
  out.set(b, a.length);
  return out;
};

function startWithMasterKey(s, masterKey) {
  const keys = s.me.keys;
  const secret = tc.unwrap_private_key(masterKey, unb64(keys.enc_private_key));
  const pub = tc.public_key_from_secret(secret);
  const mismatch = () => new Error('The public key stored on the server does not match your private key. Refusing to continue.');
  if (b64(pub) !== keys.public_key) throw mismatch();
  let seed = null;
  if (keys.enc_pq_private_key) {
    seed = tc.unwrap_pq_private_key(masterKey, unb64(keys.enc_pq_private_key));
    if (b64(tc.pq_public_key_from_seed(seed)) !== keys.pq_public_key) throw mismatch();
  }
  mk = masterKey;
  sk = seed ? concat(secret, seed) : secret;
  keyCache.clear();
  contacts = null;
  appData.clear();
  contentIndex = null;
  session.token = s.token;
  session.me = s.me;
  session.fingerprint = tc.fingerprint(myIdentity());
  if (!seed) addPqKey().catch(() => {});
}

/** What our fingerprint covers (see `identity` in the crypto crate). */
const myIdentity = () => {
  const k = session.me.keys;
  return tc.identity(unb64(k.public_key), k.pq_public_key ? unb64(k.pq_public_key) : new Uint8Array());
};

/**
 * Accounts made before post-quantum keys get an ML-KEM key on their first
 * sign-in; shares to them are then sealed with both. Once set, the server
 * never replaces it.
 */
async function addPqKey() {
  const pq = tc.generate_pq_keypair();
  try {
    session.me = await api('PUT', '/api/me/pq-key', {
      body: { pq_public_key: b64(pq.public), enc_pq_private_key: b64(tc.wrap_pq_private_key(mk, pq.secret)) },
    });
    sk = concat(sk, pq.secret);
    session.fingerprint = tc.fingerprint(myIdentity());
  } finally {
    pq.free();
  }
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
  // The next person to sign in here starts at their own files, not in our
  // last folder.
  history.replaceState(null, '', location.pathname);
  location.reload();
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/** Signed-in sessions, most recently active first. */
export const listSessions = () => api('GET', '/api/sessions');
/** Daily transfer limits an admin set for us, and today's use. */
export const myTransfer = () => api('GET', '/api/me/transfer');
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

/** Delete the account and everything in it, for good, then start over. */
export async function deleteAccount(password) {
  await api('POST', '/api/me/delete', { body: { current_auth_key: await authKeyFor(password) } });
  await forgetSession();
  history.replaceState(null, '', location.pathname);
  location.reload();
  await new Promise(() => {});
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

// ---------------------------------------------------------------------------
// App passwords: for sync clients and other devices. Each is 32 random bytes
// made here, wrapping its own copy of the master key; the server keeps a hash
// of its auth half. Shown once, in the same format as the recovery key.
// ---------------------------------------------------------------------------

export const listAppPasswords = () => api('GET', '/api/app-passwords');
export const deleteAppPassword = (id) => api('DELETE', `/api/app-passwords/${encodeURIComponent(id)}`);

/** Returns the new app password as text, to show once. */
export async function createAppPassword(password, name, scope) {
  const current = await authKeyFor(password);
  const secret = tc.random_key();
  const d = tc.derive_app_password_keys(secret);
  const id = tc.new_id();
  try {
    await api('POST', '/api/app-passwords', {
      body: {
        id,
        name,
        scope,
        current_auth_key: current,
        auth_key: b64(d.auth_key),
        enc_master_key: b64(tc.wrap_master_key_app(d.kek, mk, id)),
      },
    });
    return tc.encode_recovery_key(secret);
  } finally {
    d.free();
    secret.fill(0);
  }
}

// ---------------------------------------------------------------------------
// Two-step sign-in: an authenticator app (TOTP) and passkeys. Either one is
// then asked for after the password. A passkey whose authenticator supports
// PRF also gets its own wrapped copy of the master key, so it can sign in
// without the password.
// ---------------------------------------------------------------------------

/** A new TOTP secret to show, confirmed with `enableTotp`. */
export async function startTotp(password) {
  return api('POST', '/api/auth/totp/setup', { body: { current_auth_key: await authKeyFor(password) } });
}

export async function enableTotp(setupId, code) {
  session.me = await api('POST', '/api/auth/totp', { body: { setup_id: setupId, code } });
}

export async function disableTotp(password) {
  session.me = await api('DELETE', '/api/auth/totp', { body: { current_auth_key: await authKeyFor(password) } });
}

export const listPasskeys = () => api('GET', '/api/passkeys');

export async function removePasskey(id, password) {
  await api('DELETE', `/api/passkeys/${encodeURIComponent(id)}`, { body: { current_auth_key: await authKeyFor(password) } });
}

/**
 * Make a passkey and register it. The browser prompt comes first, while the
 * click still counts as one; the password is checked after.
 */
export async function addPasskey(name, password) {
  const o = await api('POST', '/api/passkeys/options');
  const prfSalt = tc.passkey_prf_salt();
  const c = await createPasskey({
    challenge: unb64(o.challenge),
    userHandle: unb64(o.user_handle),
    exclude: o.exclude_credentials.map(unb64),
    username: session.me.username,
    prfSalt,
  });
  let prf = c.prf;
  if (!prf && c.prfEnabled) {
    // Most authenticators only give the PRF output when signing in.
    try {
      prf = (await usePasskey({ challenge: tc.random_key(), allow: [c.id], prfSalt })).prf;
    } catch {
      prf = null; // then it's a second step only
    }
  }
  const body = {
    registration_id: o.registration_id,
    name,
    current_auth_key: await authKeyFor(password),
    client_data_json: b64(c.clientDataJSON),
    attestation_object: b64(c.attestationObject),
  };
  if (prf) {
    body.enc_master_key = b64(tc.wrap_master_key_passkey(prf, mk, c.id));
    prf.fill(0);
  }
  return api('POST', '/api/passkeys', { body });
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

/**
 * A folder's name as shown: the root folder's own name was set in English
 * when the account was made, so it's shown in the current language.
 */
export const folderLabel = (e) => (e.node.id === session.me?.keys.root_node_id ? t('My files') : e.meta.name);

/** A folder's child nodes (still encrypted), however many there are. */
const children = (id) => allChildren((p) => api('GET', p), `/api/nodes/${id}/children`);

export async function listFolder(id, key) {
  await adoptDrops();
  const nodes = await children(id);
  const rows = decryptChildren(key, nodes);
  for (const r of rows) keyCache.set(r.node.id, r.key);
  index.set(id, { at: Date.now(), rows });
  backfillTags(id, key, rows);
  return sortEntries(rows);
}

// ---------------------------------------------------------------------------
// Name tags: a keyed hash of each name under its folder's key, so the server
// can refuse duplicate names in a folder without learning them.
// ---------------------------------------------------------------------------

const tagFor = (folderKey, name) => b64(tc.name_tag(folderKey, name));

/** Lower-cased names already in a folder (straight from the server, no adoption). */
async function namesIn(folderId, folderKey) {
  try {
    const nodes = await children(folderId);
    return new Set(decryptChildren(folderKey, nodes).map((r) => r.meta.name.toLowerCase()));
  } catch {
    return new Set();
  }
}

/** `name`, or "name (2)", "name (3)"... if that's taken. */
export function freeName(name, taken) {
  const dot = name.lastIndexOf('.');
  const [base, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ''];
  let n = name;
  for (let i = 2; taken.has(n.toLowerCase()); i++) n = `${base} (${i})${ext}`;
  return n;
}

// Items made before name tags get theirs the first time we list their
// folder (and can write there; otherwise the server says no, which is fine).
const tagged = new Set();
function backfillTags(folderId, folderKey, rows) {
  const missing = rows.filter((r) => !r.node.name_tagged);
  if (!missing.length || tagged.has(folderId)) return;
  tagged.add(folderId);
  api('POST', `/api/nodes/${folderId}/name-tags`, {
    body: { tags: missing.map((r) => ({ id: r.node.id, name_tag: tagFor(folderKey, r.meta.name) })) },
  }).catch(() => {});
}

// Search across folders: names are only readable here, so the index is
// built in memory from folder listings, as you browse and when you search.
const index = new Map(); // folder id -> { at, rows }; emptied by every change (see api)
const INDEX_TTL = 2 * 60 * 1000;

/**
 * Visit everything under `top` (a folder entry), a few folders at a time:
 * `onEntry({ ...entry, location: [folder names], parentId })` for each item.
 * Unreadable folders are skipped; stops early when `signal` aborts.
 */
export async function walkTree(top, { onEntry, signal } = {}) {
  const queue = [{ entry: top, location: [folderLabel(top)] }];
  const worker = async () => {
    while (queue.length && !signal?.aborted) {
      const { entry, location } = queue.shift();
      const cached = index.get(entry.node.id);
      let rows;
      try {
        rows = cached && Date.now() - cached.at < INDEX_TTL ? cached.rows : await listFolder(entry.node.id, entry.key);
      } catch {
        continue; // unreadable or gone: skip it
      }
      if (signal?.aborted) return;
      for (const r of rows) {
        onEntry?.({ ...r, location, parentId: entry.node.id });
        if (r.node.kind === 'folder') queue.push({ entry: r, location: [...location, r.meta.name] });
      }
    }
  };
  // Each worker picks up folders the others find.
  do {
    await Promise.all([worker(), worker(), worker(), worker()]);
  } while (queue.length && !signal?.aborted);
}

/**
 * Check that everything under `top` decrypts: every key and name, and every
 * piece of every file's current version (the ciphertext's tags prove it
 * hasn't been changed). Downloads all of it. Calls `onProgress({ files,
 * folders, bytes })` as it goes and `onProblem({ location, name, id,
 * error })` for each item that fails; `name` is null when it can't be read.
 */
export async function verifyTree(top, { signal, onProgress, onProblem } = {}) {
  const queue = [{ entry: top, location: [folderLabel(top)] }];
  const done = { files: 0, folders: 0, bytes: 0 };
  const problem = (location, node, name, e) =>
    onProblem?.({ location, name, id: node.id, error: typeof e === 'string' ? e : String(e?.message || e) });
  while (queue.length && !signal?.aborted) {
    const { entry, location } = queue.shift();
    let nodes;
    try {
      nodes = await children(entry.node.id);
    } catch (e) {
      problem(location.slice(0, -1), entry.node, entry.meta.name, e);
      continue;
    }
    for (const node of nodes) {
      if (signal?.aborted) return done;
      let key, meta;
      try {
        key = unwrapChild(entry.key, node);
        meta = decryptMeta(key, node);
      } catch {
        problem(location, node, null, "Its key or name can't be decrypted.");
        continue;
      }
      if (node.kind === 'folder') {
        done.folders++;
        queue.push({ entry: { node, key, meta }, location: [...location, meta.name] });
      } else if (!node.version) {
        problem(location, node, meta.name, 'It has no content.');
      } else {
        try {
          const f = openEntry({ node, key });
          for (let i = 0; i < f.count && !signal?.aborted; i++) {
            done.bytes += (await f.read(i)).length;
            onProgress?.({ ...done });
          }
          done.files++;
        } catch (e) {
          problem(location, node, meta.name, e?.status ? "The server couldn't send all of it." : "It doesn't decrypt: it was damaged or changed on the server.");
        }
      }
      onProgress?.({ ...done });
    }
  }
  return done;
}

/** Entries under `top` whose name contains `query`, as walkTree finds them. */
export function searchTree(top, query, { onResult, signal } = {}) {
  const q = query.trim().toLowerCase();
  return walkTree(top, { signal, onEntry: (r) => r.meta.name.toLowerCase().includes(q) && onResult?.(r) });
}

// ---------------------------------------------------------------------------
// Searching inside files. The words in each text, Markdown and PDF file are
// kept in the encrypted "search" app data (see fulltext.js), by node id and
// the version they came from. Text is indexed when it's uploaded, when we
// have it anyway; anything else is read when a search needs it. The server
// never sees a word.
// ---------------------------------------------------------------------------

/** Files bigger than this aren't read just to index them. */
const INDEX_TEXT_MAX = 2 * 1024 * 1024;
const INDEX_PDF_MAX = 30 * 1024 * 1024;
/** Keep the index under the server's limit for it, with room for the encryption. */
const INDEX_MAX_CHARS = 8 * 1024 * 1024;

let contentIndex = null; // { docs: { [nodeId]: { v: version id, w: words, t: when indexed } } }
let indexSave = null;

/** How a file's words are read, or null if it isn't indexed. */
function indexKind(meta) {
  const k = previewKind(meta)?.kind;
  if ((k === 'text' || k === 'markdown') && meta.size <= INDEX_TEXT_MAX) return 'text';
  if (k === 'pdf' && meta.size <= INDEX_PDF_MAX) return 'pdf';
  return null;
}

async function loadIndex() {
  contentIndex ??= { docs: (await loadAppData('search')).docs ?? {} };
  return contentIndex;
}

function saveIndexSoon() {
  clearTimeout(indexSave);
  indexSave = setTimeout(() => {
    const docs = contentIndex?.docs;
    if (!docs) return;
    // Oldest first out, until it fits.
    let size = JSON.stringify(docs).length;
    if (size > INDEX_MAX_CHARS) {
      for (const [id, d] of Object.entries(docs).sort((a, b) => a[1].t - b[1].t)) {
        if (size <= INDEX_MAX_CHARS) break;
        size -= d.w.length + id.length + 80;
        delete docs[id];
      }
    }
    saveAppData('search', (d) => (d.docs = docs)).catch(() => {});
  }, 3000);
}

/** Read and keep the words of `entry` (a file), from `blob` if we have it. */
async function indexFile(entry, blob = null) {
  const kind = indexKind(entry.meta);
  if (!kind || !entry.node.version) return null;
  blob ??= (await fetchEntry(entry)).blob;
  let text;
  if (kind === 'pdf') {
    const { pdfText } = await import('./pdf.js');
    text = await pdfText(new Uint8Array(await blob.arrayBuffer()));
  } else {
    text = await blob.text();
  }
  const idx = await loadIndex();
  const doc = { v: entry.node.version.id, w: wordsOf(text), t: Date.now() };
  idx.docs[entry.node.id] = doc;
  saveIndexSoon();
  return doc;
}

/**
 * Files under `top` with every word of `query` in them. Files not yet in the
 * index, or changed since, are downloaded and read first: `onProgress(done,
 * total)` counts them. A full search of My files also drops what's gone.
 */
export async function searchContents(top, query, { onResult, onProgress, signal } = {}) {
  const q = queryWords(query);
  if (!q.length) return;
  const idx = await loadIndex();
  const files = [];
  await walkTree(top, { signal, onEntry: (r) => r.node.kind === 'file' && indexKind(r.meta) && files.push(r) });
  if (signal?.aborted) return;
  let done = 0;
  for (const r of files) {
    if (signal?.aborted) return;
    let doc = idx.docs[r.node.id];
    if (doc?.v !== r.node.version?.id) doc = await indexFile(r).catch(() => null);
    if (doc && matchesWords(doc.w, q)) onResult?.(r);
    onProgress?.(++done, files.length);
  }
  if (top.node.id === session.me.keys.root_node_id) {
    const here = new Set(files.map((r) => r.node.id));
    for (const id of Object.keys(idx.docs)) if (!here.has(id)) delete idx.docs[id];
    saveIndexSoon();
  }
}

// Files dropped through upload-only links arrive with their key sealed to
// our public key. Open it, wrap it under the folder key and take the file in.
let dropsChecked = 0;
let adopting = null;

/** Files dropped through a link that no longer exists, or whose key doesn't open, for the owner to review. */
export const strayDrops = $state({ list: [] }); // [{ drop, folder, meta | null }]

/**
 * Take a dropped file in: open its sealed key, give it a fresh node key
 * (re-wrapping its content key, so the visitor's key stops mattering) and
 * wrap that under the folder key. A taken name becomes "name (2)".
 */
async function adoptDrop(d, folderKey) {
  const id = d.node.id;
  const v = d.node.version;
  const visitorKey = tc.open_drop_key(sk, unb64(d.sealed_key), id, d.node.parent_id);
  const meta = decryptMeta(visitorKey, d.node);
  const ck = tc.unwrap_content_key(visitorKey, unb64(v.enc_content_key), id, v.id);
  const key = tc.random_key();
  const base = {
    enc_key: b64(tc.wrap_node_key(folderKey, key, id)),
    enc_content_key: b64(tc.wrap_content_key(key, ck, id, v.id)),
  };
  const named = (name) => ({ ...base, enc_metadata: encryptMeta(key, id, { ...meta, name }), name_tag: tagFor(folderKey, name) });
  try {
    await api('POST', `/api/drops/${id}/adopt`, { body: named(meta.name) });
  } catch (e) {
    if (e?.code !== 'name_taken') throw e;
    await api('POST', `/api/drops/${id}/adopt`, { body: named(freeName(meta.name, await namesIn(d.node.parent_id, folderKey))) });
  }
  keyCache.set(id, key);
}

export function adoptDrops() {
  if (adopting) return adopting;
  if (Date.now() - dropsChecked < 5000) return Promise.resolve(0);
  adopting = (async () => {
    let n = 0;
    const stray = [];
    try {
      const drops = await api('GET', '/api/drops');
      // Taken in automatically only into folders with a drop link now;
      // anything else waits for the owner to look at it.
      const open = drops.length ? new Set((await api('GET', '/api/links')).filter((l) => l.upload_only).map((l) => l.node_id)) : null;
      for (const d of drops) {
        let folder = null;
        try {
          folder = { key: await keyOf(d.node.parent_id), id: d.node.parent_id };
          if (open.has(d.node.parent_id)) {
            await adoptDrop(d, folder.key);
            n++;
            continue;
          }
        } catch {
          /* listed for review below */
        }
        let meta = null;
        try {
          meta = decryptMeta(tc.open_drop_key(sk, unb64(d.sealed_key), d.node.id, d.node.parent_id), d.node);
        } catch {
          /* its key doesn't open: it can only be deleted */
        }
        stray.push({ drop: d, folder, meta });
      }
    } catch {
      /* the listing still works without it */
    }
    strayDrops.list = stray;
    dropsChecked = Date.now();
    adopting = null;
    return n;
  })();
  return adopting;
}

/** Take in a stray drop the owner chose to keep. */
export async function keepStrayDrop(item) {
  await adoptDrop(item.drop, item.folder.key);
  strayDrops.list = strayDrops.list.filter((x) => x !== item);
}

export async function deleteStrayDrop(item) {
  await api('DELETE', `/api/drops/${item.drop.node.id}`);
  strayDrops.list = strayDrops.list.filter((x) => x !== item);
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
      name_tag: tagFor(parentKey, name),
    },
  });
  keyCache.set(id, key);
  return { id, key };
}

export async function rename(entry, name) {
  const { node, key, meta } = entry;
  const parentKey = await keyOf(node.parent_id);
  await api('PATCH', `/api/nodes/${node.id}`, {
    body: { enc_metadata: encryptMeta(key, node.id, { ...meta, name }), if_revision: node.revision, name_tag: tagFor(parentKey, name) },
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
      name_tag: tagFor(targetKey, entry.meta.name),
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
      return { ...it, entry, chain, location: chain.slice(0, -1).map(folderLabel) };
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
  const { key, meta } = item.entry;
  // If the name has been taken meanwhile, come back as "name (2)".
  const renamed = async (folderId, folderKey) => {
    const name = freeName(meta.name, await namesIn(folderId, folderKey));
    return { enc_metadata: encryptMeta(key, id, { ...meta, name }), name_tag: tagFor(folderKey, name) };
  };
  try {
    await api('POST', `/api/trash/${id}/restore`, { body: {} });
    return null;
  } catch (e) {
    if (e.code === 'name_taken') {
      const parentKey = item.chain[item.chain.length - 2].key;
      await api('POST', `/api/trash/${id}/restore`, { body: await renamed(item.node.parent_id, parentKey) });
      return null;
    }
    if (e.code !== 'parent_unavailable') throw e;
  }
  const rootId = session.me.keys.root_node_id;
  const rootKey = await keyOf(rootId);
  const move = { parent_id: rootId, enc_key: b64(tc.wrap_node_key(rootKey, key, id)) };
  try {
    await api('POST', `/api/trash/${id}/restore`, { body: { ...move, name_tag: tagFor(rootKey, meta.name) } });
  } catch (e) {
    if (e.code !== 'name_taken') throw e;
    await api('POST', `/api/trash/${id}/restore`, { body: { ...move, ...(await renamed(rootId, rootKey)) } });
  }
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

/** Download and decrypt one version into memory: { blob, meta }. */
export const fetchVersion = (entry, v, onProgress) =>
  fetchFile(versionNode(entry, v), entry.key, (i) => api('GET', `/api/nodes/${entry.node.id}/versions/${v.id}/chunks/${i}`), onProgress);

export async function downloadVersion(entry, v, onProgress) {
  const { blob, meta } = await fetchVersion(entry, v, onProgress);
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

/** A file's decrypted pieces, one at a time (see openFile in crypto.js). */
export function openEntry(entry) {
  const { node, key } = entry;
  return openFile(node, key, (i) => api('GET', `/api/nodes/${node.id}/chunks/${i}`));
}

/** Download files and folders as one zip, decrypted and zipped in the browser (and streamed to disk where possible). */
export async function downloadZip(entries, name, onProgress) {
  const { saveZip } = await import('./zip.js');
  await saveZip(entries, name, { list: (e) => listFolder(e.node.id, e.key), open: openEntry, onProgress });
}

/**
 * Everything you own, decrypted into one zip: My files as they are, and a
 * `thencloud-data` folder with your library data (playlists, pins, where
 * you left off) and verified contacts as JSON. Shared-with-me items belong
 * to someone else and aren't included. Streamed to disk where possible.
 */
export async function exportAccount(onProgress) {
  const { items } = await resolvePath(session.me.keys.root_node_id);
  const root = items[0];
  const now = Date.now();
  const json = (name, value) => {
    const bytes = enc.encode(JSON.stringify(value, null, 2));
    return { node: { kind: 'file' }, meta: { name, size: bytes.length, mtime: now }, bytes };
  };
  const data = [];
  for (const name of ['music', 'videos', 'files', 'notes', 'books', 'health', 'prefs']) {
    const d = await loadAppData(name);
    if (Object.keys(d).length) data.push(json(`${name}.json`, d));
  }
  const { data: pinned } = await loadContacts();
  if (Object.keys(pinned).length) data.push(json('contacts.json', pinned));
  const entries = await listFolder(root.node.id, root.key);
  if (data.length) entries.push({ node: { kind: 'folder' }, meta: { name: 'thencloud-data', mtime: now }, children: data });
  const { saveZip } = await import('./zip.js');
  const day = new Date(now).toISOString().slice(0, 10);
  await saveZip(entries, `thencloud-${session.me.username}-${day}.zip`, {
    list: (e) => e.children ?? listFolder(e.node.id, e.key),
    open: (e) => (e.bytes ? { count: 1, read: async () => e.bytes } : openEntry(e)),
    onProgress,
  });
}

/** Files larger than this are streamed to disk instead of decrypted into memory first. */
const STREAM_FROM = 16 * 1024 * 1024;

export async function download(entry, onProgress) {
  if (entry.meta.size > STREAM_FROM && (await streamsAvailable())) return streamDownload(openEntry(entry), onProgress);
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
  const padded = tc.padded_size(file.size);
  const chunkCount = tc.chunk_count(padded);
  const meta = {
    name: existing ? existing.meta.name : file.name,
    mime: file.type || null,
    size: file.size,
    mtime: file.lastModified || Date.now(),
  };
  // When a photo was taken, for the Photos timeline. Read from the file as
  // uploaded: a photo whose details were removed keeps no date either.
  const taken = file.type.startsWith('image/') ? await photoTaken(file) : null;
  if (taken) meta.taken = taken;

  const start = () =>
    api('POST', '/api/uploads', {
      body: {
        node_id: nodeId,
        parent_id: existing ? undefined : parentId,
        enc_key: existing ? undefined : b64(tc.wrap_node_key(parentKey, nodeKey, nodeId)),
        enc_metadata: encryptMeta(nodeKey, nodeId, meta),
        version_id: versionId,
        enc_content_key: b64(tc.wrap_content_key(nodeKey, contentKey, nodeId, versionId)),
        chunk_count: chunkCount,
        if_revision: existing ? existing.node.revision : undefined,
        name_tag: existing ? undefined : tagFor(parentKey, meta.name),
      },
    });
  let up;
  try {
    up = await start();
  } catch (e) {
    // A new file with a name that's already here: keep both.
    if (existing || e?.code !== 'name_taken') throw e;
    meta.name = freeName(meta.name, await namesIn(parentId, parentKey));
    up = await start();
  }
  let node;
  try {
    for (let i = 0; i < chunkCount; i++) {
      const enc = await encryptPiece(file, i, padded, contentKey, versionId, chunkCount);
      await api('PUT', `/api/uploads/${up.upload_id}/chunks/${i}`, { raw: enc });
      onProgress?.((i + 1) / chunkCount);
    }
    try {
      node = await api('POST', `/api/uploads/${up.upload_id}/finish`);
    } catch (e) {
      // The name was taken while the chunks went up: keep both.
      if (existing || e?.code !== 'name_taken') throw e;
      meta.name = freeName(meta.name, await namesIn(parentId, parentKey));
      node = await api('POST', `/api/uploads/${up.upload_id}/finish`, {
        body: { enc_metadata: encryptMeta(nodeKey, nodeId, meta), name_tag: tagFor(parentKey, meta.name) },
      });
    }
  } catch (e) {
    api('DELETE', `/api/uploads/${up.upload_id}`).catch(() => {});
    throw e;
  }
  keyCache.set(nodeId, nodeKey);
  // We have the text now: index it for searching inside files.
  if (indexKind(meta) === 'text') indexFile({ node, key: nodeKey, meta }, file).catch(() => {});
  if (canThumbnail(file) && (await uploadThumbnail(file, node, nodeKey))) {
    node = { ...node, version: { ...node.version, has_thumbnail: true } };
  }
  return { node, key: nodeKey, meta };
}

/** Make, encrypt and store a thumbnail for `node`'s current version. True if it worked; a file without one is fine. */
async function uploadThumbnail(file, node, nodeKey) {
  try {
    const image = await makeThumbnail(file);
    if (!image) return false;
    const v = node.version.id;
    await api('PUT', `/api/nodes/${node.id}/versions/${v}/thumbnail`, { raw: tc.encrypt_thumbnail(nodeKey, node.id, v, image) });
    return true;
  } catch {
    return false;
  }
}

// Decrypted thumbnails as blob: URLs, by version (each is bound to one).
const thumbs = new Map(); // version id -> Promise<url | null>
const MAX_THUMBS = 400;

/**
 * A file's thumbnail as a blob: URL, or null. Only used when the bytes are a
 * JPEG, and typed as one whatever they claim to be: a shared file's
 * thumbnail was made by someone else.
 */
export function thumbnailUrl(entry, fetchThumb = (id) => api('GET', `/api/nodes/${id}/thumbnail`)) {
  const v = entry.node.version;
  if (!v?.has_thumbnail) return Promise.resolve(null);
  if (!thumbs.has(v.id)) {
    if (thumbs.size >= MAX_THUMBS) {
      const [oldest, url] = thumbs.entries().next().value;
      thumbs.delete(oldest);
      url.then((u) => u && URL.revokeObjectURL(u));
    }
    const url = fetchThumb(entry.node.id)
      .then((sealed) => {
        const image = tc.decrypt_thumbnail(entry.key, entry.node.id, v.id, sealed);
        return isJpeg(image) ? URL.createObjectURL(new Blob([image], { type: 'image/jpeg' })) : null;
      })
      .catch(() => null);
    thumbs.set(v.id, url);
  }
  return thumbs.get(v.id);
}

/**
 * Save edited text as a new version of `entry`. Fails with a 409 if the file
 * changed since `entry` was loaded. Returns the updated entry.
 */
export function saveText(entry, text) {
  const file = new File([text], entry.meta.name, { type: entry.meta.mime || '', lastModified: Date.now() });
  return upload(file, { existing: entry });
}

// ---------------------------------------------------------------------------
// Drafts: unsaved edits, kept on the server encrypted under the master key and
// bound to this account and the file, so a closed tab doesn't lose them.
// ---------------------------------------------------------------------------

const draftLabel = (entry) => `draft:${entry.node.id}`;

/** { text, baseRevision, updatedAt }, or null if there's none. */
export async function loadDraft(entry) {
  const d = await api('GET', `/api/nodes/${entry.node.id}/draft`);
  if (!d) return null;
  const text = dec.decode(tc.decrypt_private_data(mk, session.me.user_id, draftLabel(entry), unb64(d.data)));
  return { text, baseRevision: d.base_revision, updatedAt: d.updated_at };
}

export const storeDraft = (entry, text) =>
  api('PUT', `/api/nodes/${entry.node.id}/draft`, {
    body: { data: b64(tc.encrypt_private_data(mk, session.me.user_id, draftLabel(entry), enc.encode(text))), base_revision: entry.node.revision },
  });

export const dropDraft = (entry) => api('DELETE', `/api/nodes/${entry.node.id}/draft`).catch(() => {});

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

// ---------------------------------------------------------------------------
// App data: the music and video libraries' playlists, edits and progress.
// One JSON blob per name, encrypted under the master key like the contacts.
// ---------------------------------------------------------------------------

const appData = new Map(); // name -> { data, revision }
const appSaves = new Map(); // name -> the last save, so saves run one at a time

export async function loadAppData(name) {
  if (appData.has(name)) return appData.get(name).data;
  const r = await api('GET', `/api/me/data/${name}`);
  let data = {};
  if (r.data) {
    try {
      data = JSON.parse(dec.decode(tc.decrypt_private_data(mk, session.me.user_id, name, unb64(r.data))));
    } catch {
      throw new Error(`Your ${name} library data couldn't be decrypted. It may have been tampered with.`);
    }
  }
  appData.set(name, { data, revision: r.revision });
  return data;
}

// Where books were left, in the "books" app data: node id -> { at, frac, t }
// (`at` is the page or chapter, `frac` how far down a chapter).
const MAX_BOOKS = 500;
let bookSave = null;

export const bookProgress = {
  async load(entry) {
    return (await loadAppData('books')).positions?.[entry.node.id] ?? null;
  },
  /** Kept a moment after the reader stops moving, so turning pages doesn't save each one. */
  save(entry, pos) {
    clearTimeout(bookSave);
    bookSave = setTimeout(() => {
      saveAppData('books', (d) => {
        d.positions ??= {};
        d.positions[entry.node.id] = { ...pos, t: Date.now() };
        const ids = Object.keys(d.positions);
        if (ids.length > MAX_BOOKS) {
          ids.sort((a, b) => d.positions[a].t - d.positions[b].t);
          for (const id of ids.slice(0, ids.length - MAX_BOOKS)) delete d.positions[id];
        }
      }).catch(() => {});
    }, 1500);
  },
};

/** Apply `change` to a copy of the data and save it. Resolves to the new data. */
export function saveAppData(name, change) {
  const run = async () => {
    for (let attempt = 0; ; attempt++) {
      await loadAppData(name);
      const current = appData.get(name);
      const next = structuredClone(current.data);
      change(next);
      const sealed = tc.encrypt_private_data(mk, session.me.user_id, name, enc.encode(JSON.stringify(next)));
      try {
        const r = await api('PUT', `/api/me/data/${name}`, { body: { data: b64(sealed), if_revision: current.revision } });
        appData.set(name, { data: next, revision: r.revision });
        return next;
      } catch (e) {
        // Changed on another device meanwhile: reload and apply again, once.
        if (e?.status === 409 && attempt === 0) appData.delete(name);
        else throw e;
      }
    }
  };
  const p = (appSaves.get(name) ?? Promise.resolve()).catch(() => {}).then(run);
  appSaves.set(name, p);
  return p;
}

/**
 * A user's keys as the server gives them: `publicKey` is what to seal to
 * (X25519, then ML-KEM when they have one), `identity` what the fingerprint
 * and a verified contact's pin cover.
 */
function userKeys(username, publicKey, pqPublicKey) {
  const x = unb64(publicKey);
  const pq = pqPublicKey ? unb64(pqPublicKey) : null;
  const identity = tc.identity(x, pq ?? new Uint8Array());
  return { username, publicKey: pq ? concat(x, pq) : x, identity, fingerprint: tc.fingerprint(identity) };
}

/**
 * Whether `user`'s keys are the ones pinned for them. A pin from before
 * post-quantum keys covers only X25519: when that half still matches, the
 * new ML-KEM half is pinned the first time it's seen.
 */
async function pinMatches(user) {
  const c = (await loadContacts()).data[user.username];
  if (!c) return null;
  const id = b64(user.identity);
  if (c.public_key === id) return true;
  const upgraded = user.identity.length === 64 && c.public_key === b64(user.identity.slice(0, 32));
  if (upgraded) await saveContacts((d) => (d[user.username] = { ...d[user.username], public_key: id }));
  return upgraded;
}

/**
 * How `user` (from lookupUser) compares with what you verified:
 * { state: 'new' | 'verified' | 'changed', verifiedAt, pinnedFingerprint }.
 */
export async function contactStatus(user) {
  const same = await pinMatches(user);
  if (same === null) return { state: 'new' };
  const c = contacts.data[user.username];
  return {
    state: same ? 'verified' : 'changed',
    verifiedAt: c.verified_at,
    pinnedFingerprint: tc.fingerprint(unb64(c.public_key)),
  };
}

/** Remember `user`'s current key as checked. */
export const verifyContact = (user) =>
  saveContacts((d) => (d[user.username] = { public_key: b64(user.identity), verified_at: Date.now() }));

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
  return userKeys(u.username, u.public_key, u.pq_public_key);
}

export async function share(entry, user, permission, expiresAt = null) {
  await api('POST', '/api/shares', {
    body: {
      node_id: entry.node.id,
      recipient: user.username,
      wrapped_key: b64(tc.seal_share_key(user.publicKey, entry.key, entry.node.id)),
      permission,
      expires_at: expiresAt,
    },
  });
  grantAvatar(user).catch(() => {});
}

export async function incomingShares() {
  const shares = await api('GET', '/api/shares/incoming');
  // People who share with us see our picture too.
  const owners = shares.map((s) => userKeys(s.owner, s.owner_public_key, s.owner_pq_public_key));
  for (const o of owners) grantAvatar(o).catch(() => {});
  return shares.map((s, i) => {
    try {
      const key = tc.open_share_key(sk, unb64(s.wrapped_key), s.node.id);
      keyCache.set(s.node.id, key);
      return { ...s, entry: { node: s.node, key, meta: decryptMeta(key, s.node) }, ownerFingerprint: owners[i].fingerprint };
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
 * fragment to the server, so it only ever exists on the client. A link with
 * a password carries `p.` and a secret instead: the node key is wrapped
 * under the secret and the password together, so the link alone opens
 * nothing, and neither does a server that skips the password check.
 */
export function linkUrl(token, fragment, withPassword = false) {
  return `${location.origin}/s/${token}#${withPassword ? 'p.' : ''}${b64(fragment)}`;
}

/**
 * An upload-only link carries our public key instead of the folder key, so
 * visitors can seal files to us without being able to read anything. With
 * an ML-KEM key that's too long for a link, so it carries its hash: the
 * page gets the key from the server and checks it.
 */
function urlFor(link, entry) {
  if (link.upload_only) return linkUrl(link.token, myIdentity());
  if (!link.has_password) return linkUrl(link.token, entry.key);
  // Password links made before passwords were part of the key have no secret.
  if (!link.enc_link_secret) return null;
  try {
    return linkUrl(link.token, tc.decrypt_link_secret(entry.key, unb64(link.enc_link_secret), link.node_id), true);
  } catch {
    return null;
  }
}

export async function createLink(entry, { password, expiresAt, uploadOnly = false, maxOpens = null }) {
  const body = { node_id: entry.node.id, expires_at: expiresAt ?? null, upload_only: uploadOnly, max_opens: maxOpens };
  if (password && uploadOnly) {
    // Nothing to wrap in a file drop; the password only lets visitors in.
    body.password_auth = b64((await deriveLinkKeys(myIdentity(), password)).auth);
  } else if (password) {
    const secret = tc.random_key();
    const k = await deriveLinkKeys(secret, password);
    body.password_auth = b64(k.auth);
    body.enc_link_key = b64(tc.wrap_link_key(k.kek, entry.key, entry.node.id));
    body.enc_link_secret = b64(tc.encrypt_link_secret(entry.key, secret, entry.node.id));
  }
  const link = await api('POST', '/api/links', { body });
  return { ...link, url: urlFor(link, entry) };
}

export async function links(nodeId) {
  const q = nodeId ? `?node_id=${encodeURIComponent(nodeId)}` : '';
  const list = await api('GET', `/api/links${q}`);
  return Promise.all(
    list.map(async (l) => {
      const entry = await entryFor(l.node_id);
      return { ...l, entry, url: entry ? urlFor(l, entry) : null };
    }),
  );
}

export const deleteLink = (id) => api('DELETE', `/api/links/${id}`);

// ---------------------------------------------------------------------------
// Live updates: the server streams the ids of nodes that change in a folder
// (Server-Sent Events). Read with fetch, not EventSource, so the session
// token goes in a header rather than the URL.
// ---------------------------------------------------------------------------

/**
 * Call `onChange(nodeId)` whenever something changes in folder `id` or
 * below (`nodeId` is null when some were missed). Reconnects after errors,
 * slower each time. Returns a function that stops watching.
 */
export function watchFolder(id, onChange) {
  const ctl = new AbortController();
  (async () => {
    let wait = 2000;
    while (!ctl.signal.aborted) {
      try {
        const res = await fetch(`/api/nodes/${id}/changes`, {
          headers: { Authorization: `Bearer ${session.token}` },
          cache: 'no-store',
          signal: ctl.signal,
        });
        // Signed out, or the folder is gone or no longer ours to see.
        if ([401, 403, 404].includes(res.status)) return;
        if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
        wait = 2000;
        const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
        let buf = '';
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          buf += value;
          let end;
          while ((end = buf.indexOf('\n\n')) >= 0) {
            const block = buf.slice(0, end);
            buf = buf.slice(end + 2);
            if (!/^event: change$/m.test(block)) continue;
            let nodeId = null;
            try {
              nodeId = JSON.parse(block.match(/^data: (.*)$/m)?.[1] ?? '{}').node_id ?? null;
            } catch {
              /* a change all the same */
            }
            onChange(nodeId);
          }
        }
      } catch {
        if (ctl.signal.aborted) return;
      }
      await new Promise((r) => setTimeout(r, wait));
      wait = Math.min(wait * 2, 60_000);
    }
  })();
  return () => ctl.abort();
}

// ---------------------------------------------------------------------------
// Activity: who added, changed, renamed, moved, trashed or restored what in a
// folder. The server keeps node ids; names are found and decrypted here, for
// the items we can still reach.
// ---------------------------------------------------------------------------

/**
 * A page of a folder's activity, newest first: [{ ...event, name, parentId }]
 * with `name` null for items that are gone or out of reach.
 */
export async function activity(folder, before = null) {
  const q = before ? `?before=${before}` : '';
  const events = await api('GET', `/api/nodes/${folder.node.id}/activity${q}`);
  const ids = [...new Set(events.map((e) => e.node_id))];
  const found = new Map();
  // A few at a time: each is a request.
  for (let i = 0; i < ids.length; i += 6) {
    await Promise.all(
      ids.slice(i, i + 6).map(async (id) => {
        try {
          const { items } = await resolvePath(id);
          const it = items[items.length - 1];
          found.set(id, { name: it.meta.name, parentId: it.node.parent_id });
        } catch {
          found.set(id, null);
        }
      }),
    );
  }
  return events.map((e) => ({ ...e, name: found.get(e.node_id)?.name ?? null, parentId: found.get(e.node_id)?.parentId ?? null }));
}

// ---------------------------------------------------------------------------
// Comments: encrypted under the node key, so whoever can open the node can
// read them. Each is bound to the node, its id and its author, so the server
// can't move one or put it in someone else's name.
// ---------------------------------------------------------------------------

/** Longest comment, in characters. */
export const MAX_COMMENT = 4000;

/** A node's comments, oldest first, with `text` (null if it doesn't open) and the exact time `at`. */
export async function comments(entry) {
  const list = await api('GET', `/api/nodes/${entry.node.id}/comments`);
  return list.map((c) => {
    try {
      const body = JSON.parse(dec.decode(tc.decrypt_comment(entry.key, entry.node.id, c.id, c.author_id, unb64(c.enc_body))));
      return { ...c, text: String(body.text ?? ''), at: Number(body.at) || c.created_at * 1000 };
    } catch {
      return { ...c, text: null, at: c.created_at * 1000 };
    }
  });
}

export async function addComment(entry, text) {
  const id = tc.new_id();
  const at = Date.now();
  const body = enc.encode(JSON.stringify({ text, at }));
  const sealed = tc.encrypt_comment(entry.key, entry.node.id, id, session.me.user_id, body);
  const c = await api('POST', `/api/nodes/${entry.node.id}/comments`, { body: { id, enc_body: b64(sealed) } });
  return { ...c, text, at };
}

export const deleteComment = (id) => api('DELETE', `/api/comments/${id}`);

// ---------------------------------------------------------------------------
// Profile pictures, display names and pronouns: encrypted under our avatar
// key, which is sealed to each person we share with (either way round). The
// server can't see any of them.
// ---------------------------------------------------------------------------

export const avatar = $state({ url: null, name: null, details: {} }); // our own picture (a blob: URL), display name and person details
let avatarState = null; // { key, grantees: Set, picture: bytes | null, name: string | null, details: object }
const profiles = new Map(); // username -> Promise<{ url, name }>

const imageType = (b) =>
  b[0] === 0x89 ? 'image/png' : b[0] === 0x52 && b[8] === 0x57 ? 'image/webp' : 'image/jpeg';

function avatarBlobUrl(bytes) {
  return URL.createObjectURL(new Blob([bytes], { type: imageType(bytes) }));
}

/** Load our own picture and display name (once per session). */
export async function loadMyAvatar() {
  if (avatarState) return avatarState;
  const r = await api('GET', '/api/me/avatar');
  const me = session.me;
  let key = null;
  let picture = null;
  let name = null;
  let details = {};
  if (r.enc_key) {
    key = tc.decrypt_private_data(mk, me.user_id, 'avatar-key', unb64(r.enc_key));
    if (r.data) picture = tc.decrypt_avatar(key, me.username, unb64(r.data));
    if (r.name) name = tc.decrypt_display_name(key, me.username, unb64(r.name));
    if (r.details) details = JSON.parse(tc.decrypt_person_details(key, me.username, unb64(r.details)));
  }
  avatarState = { key, grantees: new Set(r.grantees), picture, name, details };
  avatar.url = picture ? avatarBlobUrl(picture) : null;
  avatar.name = name;
  avatar.details = details;
  return avatarState;
}

/**
 * Give `username` our avatar key, if we have a picture or name and haven't
 * yet. Only to a verified contact whose key still matches: the same rule as
 * sharing, so a key the server swapped in never gets it.
 */
async function grantAvatar(user) {
  const { username } = user;
  const a = await loadMyAvatar();
  if (!a.key || a.grantees.has(username) || username === session.me.username) return;
  if (!(await pinMatches(user))) return;
  a.grantees.add(username);
  await api('PUT', `/api/avatar-grants/${encodeURIComponent(username)}`, {
    body: { sealed_key: b64(tc.seal_avatar_key(user.publicKey, a.key, session.me.username, username)) },
  });
}

/** Everyone we share with, either way round. */
async function sharePartners() {
  const partners = new Set();
  for (const s of await api('GET', '/api/shares/incoming')) partners.add(s.owner);
  for (const s of await api('GET', '/api/shares/outgoing')) partners.add(s.recipient);
  return partners;
}

const hasDetails = (d) => !!(d?.subject || d?.object || d?.possessive || d?.gender);

/**
 * Store our picture, name and person details, changing those given in
 * `change` (null removes one; all gone removes the lot). A new key each
 * time, given to the people we share with now, so someone we've stopped
 * sharing with never gets a later one.
 */
async function saveProfile(change) {
  const a = await loadMyAvatar();
  const me = session.me;
  const { picture, name, details } = { picture: a.picture, name: a.name, details: a.details, ...change };
  if (!picture && !name && !hasDetails(details)) {
    await api('DELETE', '/api/me/avatar');
    Object.assign(a, { key: null, grantees: new Set(), picture: null, name: null, details: {} });
  } else {
    const key = tc.random_key();
    await api('PUT', '/api/me/avatar', {
      body: {
        data: picture ? b64(tc.encrypt_avatar(key, me.username, picture)) : null,
        name: name ? b64(tc.encrypt_display_name(key, me.username, name)) : null,
        details: hasDetails(details) ? b64(tc.encrypt_person_details(key, me.username, JSON.stringify(details))) : null,
        enc_key: b64(tc.encrypt_private_data(mk, me.user_id, 'avatar-key', key)),
      },
    });
    Object.assign(a, { key, grantees: new Set(), picture, name, details: details ?? {} });
  }
  if (avatar.url) URL.revokeObjectURL(avatar.url);
  avatar.url = a.picture ? avatarBlobUrl(a.picture) : null;
  avatar.name = a.name;
  avatar.details = a.details;
  if (!a.key) return;
  const pinned = (await loadContacts()).data;
  for (const username of await sharePartners()) {
    if (pinned[username]) await grantAvatar(await lookupUser(username)).catch(() => {});
  }
}

/** Set our picture from an image file: cropped square, 256 px, encrypted here. */
export async function setAvatar(file) {
  const bitmap = await createImageBitmap(file);
  const side = Math.min(bitmap.width, bitmap.height);
  const canvas = new OffscreenCanvas(256, 256);
  canvas.getContext('2d').drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, 256, 256);
  bitmap.close();
  let blob = await canvas.convertToBlob({ type: 'image/webp', quality: 0.85 });
  if (blob.type !== 'image/webp') blob = await canvas.convertToBlob({ type: 'image/jpeg', quality: 0.85 });
  const bytes = new Uint8Array(await blob.arrayBuffer());
  await saveProfile({ picture: bytes });
}

/** Remove our picture (the display name and pronouns stay). */
export async function removeAvatar() {
  await saveProfile({ picture: null });
}

/** The name as it would be stored, or null if it can't be one. */
export const cleanDisplayName = (name) => tc.clean_display_name(name) ?? null;

/** Set our display name, or remove it with an empty one. */
export async function setDisplayName(name) {
  const clean = name.trim() ? cleanDisplayName(name) : null;
  if (name.trim() && !clean) throw new Error('That name has characters that can\'t be used.');
  await saveProfile({ name: clean });
}

/** A pronoun as it would be stored, or null if it can't be one. */
export const cleanPronoun = (p) => tc.clean_pronoun(p) ?? null;

/**
 * Set how we like to be referred to: { subject, object, possessive } (English
 * pronouns, empty for "not said") and gender ('neuter', 'feminine',
 * 'masculine' or null).
 */
export async function setPersonDetails({ subject = '', object = '', possessive = '', gender = null }) {
  const d = {};
  for (const [k, v] of Object.entries({ subject, object, possessive })) {
    const clean = cleanPronoun(v);
    if (clean === null) throw new Error(t("Those pronouns have characters that can't be used."));
    if (clean) d[k] = clean;
  }
  if (gender) d.gender = gender;
  await saveProfile({ details: d });
}

/**
 * Someone's picture (a blob: URL), display name and person details ({}
 * when not said), each empty if they haven't given us their avatar key.
 */
export function profileOf(username) {
  if (username === session.me?.username) return loadMyAvatar().then(() => ({ url: avatar.url, name: avatar.name, details: avatar.details }));
  if (!profiles.has(username)) {
    profiles.set(
      username,
      api('GET', `/api/users/${encodeURIComponent(username)}/avatar`)
        .then((r) => {
          if (!r) return { url: null, name: null, details: {} };
          const key = tc.open_avatar_key(sk, unb64(r.sealed_key), username, session.me.username);
          const url = r.data ? avatarBlobUrl(tc.decrypt_avatar(key, username, unb64(r.data))) : null;
          let name = null;
          let details = {};
          try {
            if (r.name) name = tc.decrypt_display_name(key, username, unb64(r.name));
          } catch {}
          try {
            if (r.details) details = JSON.parse(tc.decrypt_person_details(key, username, unb64(r.details)));
          } catch {}
          return { url, name, details };
        })
        .catch(() => ({ url: null, name: null, details: {} })),
    );
  }
  return profiles.get(username);
}

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
 * Download a video (kind 'video', at `quality`: '480' | '720' | '1080' | 'best')
 * or its audio ('audio') through the server.
 * Resolves to a Blob. `onProgress(bytes)`; stop with `signal`. A download
 * the server cut short (too large, failed) rejects instead of returning a
 * partial file.
 */
export async function downloadVideo(url, kind, { quality, onProgress, signal } = {}) {
  let res;
  try {
    res = await fetch('/api/tools/video/download', {
      method: 'POST',
      headers: { Authorization: `Bearer ${session.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ url, kind, quality }),
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
