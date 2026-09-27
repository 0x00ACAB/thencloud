// thencloud proof-of-concept client.
//
// Key handling:
// - The password is stretched with Argon2id in WASM into an auth key (sent
//   to the server) and a key-encryption key (kept here) that unwraps the
//   master key.
// - The master key and private key live only in this page's memory. Nothing
//   is written to localStorage/sessionStorage, so a reload means logging in
//   again.

import {
  tc, ready, api, ApiError, b64, unb64, decryptMeta, encryptMeta, unwrapChild,
  downloadFile, fmtSize, fmtTime, el, $, tick,
} from './common.js';

const S = {
  token: null,
  me: null,
  mk: null, // master key
  sk: null, // X25519 secret key
  cwd: null, // { id, key, node, perm: 'owner' | 'read' | 'write' }
  clipboard: null, // { node, name }
};
const keyCache = new Map(); // node id -> node key (Uint8Array)

const auth = () => ({ token: S.token });

function status(msg) {
  $('status').textContent = msg;
}

/** Run an async UI action, reporting errors in the status line. */
function guard(fn) {
  return async (ev) => {
    ev?.preventDefault?.();
    try {
      await fn(ev);
    } catch (e) {
      console.error(e);
      status(`Error: ${e instanceof ApiError ? e.message : e?.message || e}`);
    }
  };
}

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

const deviceName = () => `web: ${navigator.userAgent}`.slice(0, 100);

async function deriveKeys(password, saltB64, params) {
  status('Deriving keys from password (Argon2id)…');
  await tick();
  return tc.derive_account_keys(password, unb64(saltB64), JSON.stringify(params));
}

async function register(ev) {
  const f = ev.target;
  const username = f.username.value.trim();
  const password = f.password.value;
  if (password !== f.password2.value) throw new Error('passwords do not match');

  const salt = tc.random_salt();
  const params = JSON.parse(tc.default_kdf_params());
  const ak = await deriveKeys(password, b64(salt), params);
  status('Generating keys…');
  const mk = tc.random_key();
  const kp = tc.generate_keypair();
  const rootId = tc.new_id();
  const rootKey = tc.random_key();

  const s = await api('POST', '/api/auth/register', {
    body: {
      username,
      auth_key: b64(ak.auth_key),
      kdf_salt: b64(salt),
      kdf_params: params,
      enc_master_key: b64(tc.wrap_master_key(ak.kek, mk)),
      public_key: b64(kp.public),
      enc_private_key: b64(tc.wrap_private_key(mk, kp.secret)),
      root: {
        id: rootId,
        enc_key: b64(tc.wrap_node_key(mk, rootKey, rootId)),
        enc_metadata: encryptMeta(rootKey, rootId, { name: 'Home', size: 0, mtime: Date.now() }),
      },
      device_name: deviceName(),
    },
  });
  f.reset();
  await startSession(s, ak.kek);
}

async function login(ev) {
  const f = ev.target;
  const username = f.username.value.trim();
  const pre = await api('POST', '/api/auth/prelogin', { body: { username } });
  const ak = await deriveKeys(f.password.value, pre.kdf_salt, pre.kdf_params);
  status('Logging in…');
  const s = await api('POST', '/api/auth/login', {
    body: { username, auth_key: b64(ak.auth_key), device_name: deviceName() },
  });
  f.reset();
  await startSession(s, ak.kek);
}

async function startSession(session, kek) {
  const keys = session.me.keys;
  const mk = tc.unwrap_master_key(kek, unb64(keys.enc_master_key));
  const sk = tc.unwrap_private_key(mk, unb64(keys.enc_private_key));
  const pk = tc.public_key_from_secret(sk);
  if (b64(pk) !== keys.public_key) throw new Error('public key on server does not match your private key!');
  Object.assign(S, { token: session.token, me: session.me, mk, sk });
  $('auth').hidden = true;
  $('app').hidden = false;
  $('me-fp').textContent = tc.fingerprint(pk);
  renderMe();
  await openFolder(keys.root_node_id);
  await refreshLists();
  status('Ready.');
}

function renderMe() {
  $('me-name').textContent = S.me.username + (S.me.is_admin ? ' (admin)' : '');
  $('me-usage').textContent = `${fmtSize(S.me.used_bytes)} of ${fmtSize(S.me.quota_bytes)}`;
}

async function refreshMe() {
  S.me = await api('GET', '/api/me', auth());
  renderMe();
}

async function logout() {
  try { await api('POST', '/api/auth/logout', auth()); } catch { /* ignore */ }
  location.reload(); // drops every key from memory
}

async function changePassword(ev) {
  const f = ev.target;
  if (f.new1.value !== f.new2.value) throw new Error('new passwords do not match');
  const pre = await api('POST', '/api/auth/prelogin', { body: { username: S.me.username } });
  const cur = await deriveKeys(f.current.value, pre.kdf_salt, pre.kdf_params);
  const salt = tc.random_salt();
  const params = JSON.parse(tc.default_kdf_params());
  const next = await deriveKeys(f.new1.value, b64(salt), params);
  await api('POST', '/api/auth/password', {
    ...auth(),
    body: {
      current_auth_key: b64(cur.auth_key),
      new_auth_key: b64(next.auth_key),
      new_kdf_salt: b64(salt),
      new_kdf_params: params,
      new_enc_master_key: b64(tc.wrap_master_key(next.kek, S.mk)),
    },
  });
  f.reset();
  status('Password changed. Other sessions were signed out.');
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/**
 * Fetch the path to a node and derive every key along it. For our own files
 * the chain starts at the master key, for shared ones at the share key.
 * Returns [{ node, key, meta }] from the top down.
 */
async function resolvePath(id) {
  const p = await api('GET', `/api/nodes/${id}/path`, auth());
  let key;
  return p.nodes.map((node, i) => {
    if (i === 0) {
      key = p.share
        ? tc.open_share_key(S.sk, unb64(p.share.wrapped_key), node.id)
        : tc.unwrap_node_key(S.mk, unb64(node.enc_key), node.id);
    } else {
      key = unwrapChild(key, node);
    }
    keyCache.set(node.id, key);
    return { node, key, meta: decryptMeta(key, node), perm: p.share ? p.share.permission : 'owner' };
  });
}

async function keyOf(id) {
  if (!keyCache.has(id)) await resolvePath(id);
  return keyCache.get(id);
}

// ---------------------------------------------------------------------------
// File browser
// ---------------------------------------------------------------------------

async function openFolder(id) {
  const path = await resolvePath(id);
  const here = path[path.length - 1];
  S.cwd = { id, key: here.key, node: here.node, perm: here.perm };

  const crumbs = $('breadcrumbs');
  crumbs.replaceChildren('Location: ');
  path.forEach((p, i) => {
    if (i) crumbs.append(' / ');
    crumbs.append(i === path.length - 1
      ? el('b', {}, p.meta.name)
      : el('button', { onclick: guard(() => openFolder(p.node.id)) }, p.meta.name));
  });
  $('cwd-info').textContent = here.perm === 'owner'
    ? ''
    : `Shared with you by ${here.node.owner} (${here.perm === 'write' ? 'can edit' : 'read only'})`;
  const canWrite = here.perm !== 'read';
  $('mkdir').disabled = !canWrite;
  $('upload-btn').disabled = !canWrite;
  $('paste').disabled = !canWrite;

  const children = await api('GET', `/api/nodes/${id}/children`, auth());
  const rows = children.map((node) => {
    const key = unwrapChild(S.cwd.key, node);
    keyCache.set(node.id, key);
    const meta = decryptMeta(key, node);
    return { node, key, meta };
  });
  rows.sort((a, b) => (a.node.kind === b.node.kind ? a.meta.name.localeCompare(b.meta.name) : a.node.kind === 'folder' ? -1 : 1));

  const tbody = $('listing').querySelector('tbody');
  tbody.replaceChildren(...rows.map((r) => fileRow(r, canWrite)));
  if (!rows.length) tbody.append(el('tr', {}, el('td', { colSpan: 4 }, '(empty folder)')));
}

function fileRow({ node, key, meta }, canWrite) {
  const isFolder = node.kind === 'folder';
  const owner = S.cwd.perm === 'owner';
  const actions = [
    isFolder
      ? el('button', { onclick: guard(() => openFolder(node.id)) }, 'Open')
      : el('button', { onclick: guard(() => download(node, key)) }, 'Download'),
    canWrite && el('button', { onclick: guard(() => rename(node, key, meta)) }, 'Rename'),
    canWrite && el('button', { onclick: () => cut(node, meta.name) }, 'Move'),
    canWrite && !isFolder && el('button', { onclick: guard(() => pickReplacement(node, key, meta)) }, 'Upload new version'),
    canWrite && el('button', { onclick: guard(() => remove(node, meta.name)) }, 'Delete'),
    owner && el('button', { onclick: guard(() => share(node, key, meta.name)) }, 'Share'),
    owner && el('button', { onclick: guard(() => createLink(node, key, meta.name)) }, 'Public link'),
  ].filter(Boolean);
  return el('tr', {},
    el('td', {}, isFolder ? `📁 ${meta.name}/` : meta.name),
    el('td', {}, isFolder ? '' : fmtSize(meta.size)),
    el('td', {}, fmtTime(meta.mtime || node.updated_at * 1000)),
    el('td', {}, ...actions.flatMap((a) => [a, ' '])),
  );
}

const reload = () => openFolder(S.cwd.id);

async function mkdir() {
  const name = prompt('Folder name');
  if (!name) return;
  const id = tc.new_id();
  const key = tc.random_key();
  await api('POST', '/api/nodes/folder', {
    ...auth(),
    body: {
      id,
      parent_id: S.cwd.id,
      enc_key: b64(tc.wrap_node_key(S.cwd.key, key, id)),
      enc_metadata: encryptMeta(key, id, { name, size: 0, mtime: Date.now() }),
    },
  });
  await reload();
  status(`Created folder "${name}".`);
}

async function rename(node, key, meta) {
  const name = prompt('New name', meta.name);
  if (!name || name === meta.name) return;
  await api('PATCH', `/api/nodes/${node.id}`, {
    ...auth(),
    body: { enc_metadata: encryptMeta(key, node.id, { ...meta, name }), if_revision: node.revision },
  });
  await reload();
  status(`Renamed to "${name}".`);
}

function cut(node, name) {
  S.clipboard = { node, name };
  $('clipboard-name').textContent = name;
  $('clipboard').hidden = false;
  status(`Navigate to the destination folder and click "Move here".`);
}

async function paste() {
  const { node, name } = S.clipboard;
  const nodeKey = await keyOf(node.id);
  // The node key is re-wrapped under the destination folder's key.
  await api('PATCH', `/api/nodes/${node.id}`, {
    ...auth(),
    body: {
      parent_id: S.cwd.id,
      enc_key: b64(tc.wrap_node_key(S.cwd.key, nodeKey, node.id)),
      if_revision: node.revision,
    },
  });
  S.clipboard = null;
  $('clipboard').hidden = true;
  await reload();
  status(`Moved "${name}".`);
}

async function remove(node, name) {
  if (!confirm(`Permanently delete "${name}"${node.kind === 'folder' ? ' and everything in it' : ''}?`)) return;
  await api('DELETE', `/api/nodes/${node.id}`, auth());
  await reload();
  await refreshMe();
  status(`Deleted "${name}".`);
}

async function download(node, key) {
  const name = decryptMeta(key, node).name;
  await downloadFile(node, key, (i) => `/api/nodes/${node.id}/chunks/${i}`, auth(),
    (done, total) => status(`Downloading "${name}": chunk ${done}/${total}`));
  status(`Downloaded "${name}".`);
}

// ---------------------------------------------------------------------------
// Uploads
// ---------------------------------------------------------------------------

/**
 * Encrypt and upload `file`. For a new file pass `parentId`; for a new
 * version of an existing file pass `existing` = { node, key, meta }.
 */
async function uploadFile(file, { parentId, parentKey, existing }) {
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
    ...auth(),
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
      await api('PUT', `/api/uploads/${up.upload_id}/chunks/${i}`, { ...auth(), raw: enc });
      $('progress').textContent = `Uploading "${meta.name}": ${Math.round(((i + 1) / chunkCount) * 100)}%`;
    }
    await api('POST', `/api/uploads/${up.upload_id}/finish`, auth());
  } catch (e) {
    api('DELETE', `/api/uploads/${up.upload_id}`, auth()).catch(() => {});
    throw e;
  }
}

async function uploadSelected() {
  const files = [...$('upload-input').files];
  if (!files.length) throw new Error('choose one or more files first');
  for (const file of files) {
    await uploadFile(file, { parentId: S.cwd.id, parentKey: S.cwd.key });
  }
  $('upload-input').value = '';
  $('progress').textContent = '';
  await reload();
  await refreshMe();
  status(`Uploaded ${files.length} file(s).`);
}

async function pickReplacement(node, key, meta) {
  const input = el('input', { type: 'file' });
  input.addEventListener('change', guard(async () => {
    const file = input.files[0];
    if (!file) return;
    await uploadFile(file, { existing: { node, key, meta } });
    $('progress').textContent = '';
    await reload();
    await refreshMe();
    status(`Uploaded a new version of "${meta.name}".`);
  }));
  input.click();
}

// ---------------------------------------------------------------------------
// Sharing
// ---------------------------------------------------------------------------

async function share(node, key, name) {
  const username = prompt(`Share "${name}" with which user?`);
  if (!username) return;
  const u = await api('GET', `/api/users/${encodeURIComponent(username)}/public-key`, auth());
  const pk = unb64(u.public_key);
  const fp = tc.fingerprint(pk);
  if (!confirm(`Share "${name}" with ${u.username}?\n\nTheir key fingerprint is:\n${fp}\n\n`
    + `To be safe from a malicious server, ask ${u.username} to confirm this matches `
    + `the fingerprint shown when they log in.`)) return;
  const write = confirm(`Allow ${u.username} to edit (upload, rename, delete inside)?\n\nOK = can edit, Cancel = read only`);
  await api('POST', '/api/shares', {
    ...auth(),
    body: {
      node_id: node.id,
      recipient: u.username,
      wrapped_key: b64(tc.seal_share_key(pk, key, node.id)),
      permission: write ? 'write' : 'read',
    },
  });
  await refreshLists();
  status(`Shared "${name}" with ${u.username}.`);
}

function linkUrl(link, nodeKey) {
  // The key goes into the fragment (#...), which browsers never send to the
  // server, not into the path or a query parameter.
  return `${location.origin}/s/${link.token}#${b64(nodeKey)}`;
}

async function createLink(node, key, name) {
  const password = prompt(`Public link for "${name}".\n\nOptional password (leave empty for none):`, '');
  if (password === null) return;
  const days = prompt('Expire after how many days? (leave empty for never)', '');
  if (days === null) return;
  const expires_at = days.trim() ? Math.floor(Date.now() / 1000) + Math.round(Number(days) * 86400) : null;
  const link = await api('POST', '/api/links', {
    ...auth(),
    body: { node_id: node.id, password: password || null, expires_at },
  });
  await refreshLists();
  prompt('Public link (copy it now):', linkUrl(link, key));
  status(`Created a public link for "${name}".`);
}

async function refreshLists() {
  await Promise.all([renderIncoming(), renderOutgoing(), renderLinks()]);
}

async function renderIncoming() {
  const shares = await api('GET', '/api/shares/incoming', auth());
  $('incoming').replaceChildren(...shares.map((s) => {
    try {
      const key = tc.open_share_key(S.sk, unb64(s.wrapped_key), s.node.id);
      keyCache.set(s.node.id, key);
      const meta = decryptMeta(key, s.node);
      const isFolder = s.node.kind === 'folder';
      return el('li', {},
        `${isFolder ? '📁 ' : ''}${meta.name} — from ${s.owner} (${s.permission}), their fingerprint `,
        el('code', {}, tc.fingerprint(unb64(s.owner_public_key))), ' ',
        isFolder
          ? el('button', { onclick: guard(() => openFolder(s.node.id)) }, 'Open')
          : el('button', { onclick: guard(() => download(s.node, key)) }, 'Download'),
        ' ',
        el('button', {
          onclick: guard(async () => {
            if (!confirm(`Remove "${meta.name}" from your shares?`)) return;
            await api('DELETE', `/api/shares/${s.id}`, auth());
            await refreshLists();
          }),
        }, 'Leave'),
      );
    } catch (e) {
      return el('li', {}, `(share ${s.id} from ${s.owner} could not be decrypted: ${e.message || e})`);
    }
  }));
  if (!shares.length) $('incoming').append(el('li', {}, '(nothing)'));
}

async function nameOf(nodeId) {
  try {
    const path = await resolvePath(nodeId);
    return path[path.length - 1];
  } catch {
    return null;
  }
}

async function renderOutgoing() {
  const shares = await api('GET', '/api/shares/outgoing', auth());
  const items = await Promise.all(shares.map(async (s) => {
    const n = await nameOf(s.node_id);
    const other = s.permission === 'write' ? 'read' : 'write';
    return el('li', {},
      `${n ? n.meta.name : s.node_id} → ${s.recipient} (${s.permission}) `,
      el('button', {
        onclick: guard(async () => {
          await api('PATCH', `/api/shares/${s.id}`, { ...auth(), body: { permission: other } });
          await refreshLists();
        }),
      }, `Make ${other}`),
      ' ',
      el('button', {
        onclick: guard(async () => {
          await api('DELETE', `/api/shares/${s.id}`, auth());
          await refreshLists();
          status(`Stopped sharing with ${s.recipient}.`);
        }),
      }, 'Revoke'),
    );
  }));
  $('outgoing').replaceChildren(...items);
  if (!items.length) $('outgoing').append(el('li', {}, '(nothing)'));
}

async function renderLinks() {
  const links = await api('GET', '/api/links', auth());
  const items = await Promise.all(links.map(async (l) => {
    const n = await nameOf(l.node_id);
    const extras = [l.has_password && 'password', l.expires_at && `expires ${fmtTime(l.expires_at * 1000)}`].filter(Boolean);
    return el('li', {},
      `${n ? n.meta.name : l.node_id}${extras.length ? ` (${extras.join(', ')})` : ''}: `,
      n ? el('input', { value: linkUrl(l, n.key), readOnly: true, size: 80, onfocus: (e) => e.target.select() }) : '',
      ' ',
      el('button', {
        onclick: guard(async () => {
          await api('DELETE', `/api/links/${l.id}`, auth());
          await refreshLists();
          status('Link deleted.');
        }),
      }, 'Delete'),
    );
  }));
  $('links').replaceChildren(...items);
  if (!items.length) $('links').append(el('li', {}, '(none)'));
}

// ---------------------------------------------------------------------------

async function main() {
  await ready;
  $('login-form').addEventListener('submit', guard(login));
  $('register-form').addEventListener('submit', guard(register));
  $('password-form').addEventListener('submit', guard(changePassword));
  $('logout').addEventListener('click', guard(logout));
  $('go-home').addEventListener('click', guard(() => openFolder(S.me.keys.root_node_id)));
  $('refresh').addEventListener('click', guard(async () => { await reload(); await refreshLists(); await refreshMe(); }));
  $('mkdir').addEventListener('click', guard(mkdir));
  $('upload-btn').addEventListener('click', guard(uploadSelected));
  $('paste').addEventListener('click', guard(paste));
  $('paste-cancel').addEventListener('click', () => { S.clipboard = null; $('clipboard').hidden = true; });
  $('auth').hidden = false;
  status('Ready. Log in or register.');
}

main().catch((e) => status(`Failed to start: ${e.message || e}`));
