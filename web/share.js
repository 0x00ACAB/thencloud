// Public link viewer: /s/<token>#<node key>
//
// Only <token> is ever sent to the server. The key after '#' is read from
// location.hash here and used exclusively for local decryption; it never
// appears in any request URL, header or body.

import {
  tc, ready, api, ApiError, unb64, decryptMeta, unwrapChild, downloadFile,
  fmtSize, fmtTime, el, $,
} from './common.js';

const token = decodeURIComponent(location.pathname.split('/').filter(Boolean)[1] || '');
let rootKey = null;
let linkToken = null; // issued by /unlock for password-protected links
let stack = []; // [{ node, key, meta }] for folder navigation

const opts = () => (linkToken ? { headers: { 'X-Link-Token': linkToken } } : {});
const base = `/api/public/${encodeURIComponent(token)}`;

function status(msg) {
  $('status').textContent = msg;
}

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

async function load() {
  let info;
  try {
    info = await api('GET', base, opts());
  } catch (e) {
    if (e.code === 'password_required') {
      $('password-form').hidden = false;
      status('Password required.');
      return;
    }
    if (e.status === 404) throw new Error('this link does not exist or has expired');
    throw e;
  }
  $('password-form').hidden = true;
  let meta;
  try {
    meta = decryptMeta(rootKey, info.node);
  } catch {
    throw new Error('the key in this link is wrong (was the link copied completely?)');
  }
  $('content').hidden = false;
  stack = [{ node: info.node, key: rootKey, meta }];
  await render();
  status(info.expires_at ? `Link expires ${fmtTime(info.expires_at * 1000)}.` : 'Ready.');
}

async function unlock(ev) {
  const r = await api('POST', `${base}/unlock`, { body: { password: ev.target.password.value } });
  linkToken = r.link_token;
  ev.target.reset();
  await load();
}

async function render() {
  const cur = stack[stack.length - 1];
  const crumbs = $('breadcrumbs');
  crumbs.replaceChildren('Location: ');
  stack.forEach((s, i) => {
    if (i) crumbs.append(' / ');
    crumbs.append(i === stack.length - 1
      ? el('b', {}, s.meta.name)
      : el('button', { onclick: guard(async () => { stack = stack.slice(0, i + 1); await render(); }) }, s.meta.name));
  });

  const tbody = $('listing').querySelector('tbody');
  if (cur.node.kind === 'file') {
    tbody.replaceChildren(row(cur));
    return;
  }
  const children = await api('GET', `${base}/nodes/${cur.node.id}/children`, opts());
  const rows = children.map((node) => {
    const key = unwrapChild(cur.key, node);
    return { node, key, meta: decryptMeta(key, node) };
  });
  rows.sort((a, b) => (a.node.kind === b.node.kind ? a.meta.name.localeCompare(b.meta.name) : a.node.kind === 'folder' ? -1 : 1));
  tbody.replaceChildren(...rows.map(row));
  if (!rows.length) tbody.append(el('tr', {}, el('td', { colSpan: 4 }, '(empty folder)')));
}

function row(item) {
  const { node, key, meta } = item;
  const isFolder = node.kind === 'folder';
  return el('tr', {},
    el('td', {}, isFolder ? `📁 ${meta.name}/` : meta.name),
    el('td', {}, isFolder ? '' : fmtSize(meta.size)),
    el('td', {}, fmtTime(meta.mtime)),
    el('td', {}, isFolder
      ? el('button', { onclick: guard(async () => { stack.push(item); await render(); }) }, 'Open')
      : el('button', {
        onclick: guard(async () => {
          await downloadFile(node, key, (i) => `${base}/nodes/${node.id}/chunks/${i}`, opts(),
            (d, t) => status(`Downloading "${meta.name}": chunk ${d}/${t}`));
          status(`Downloaded "${meta.name}".`);
        }),
      }, 'Download')),
  );
}

async function main() {
  await ready;
  $('password-form').addEventListener('submit', guard(unlock));
  const fragment = location.hash.slice(1);
  if (!token || !fragment) throw new Error('incomplete link: the part after # (the key) is missing');
  try {
    rootKey = unb64(fragment);
  } catch {
    throw new Error('the key in this link is malformed');
  }
  await load();
}

main().catch((e) => status(`Error: ${e.message || e}`));
