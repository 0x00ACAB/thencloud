// A stand-in for Google's OAuth and Drive endpoints, for the browser tests
// (the server is started with --google-test-base pointing here). Consent is
// given at once: /auth sends the popup straight back to the server.
//
//   node e2e/fake-google.mjs 8094
//
// GET /_files lists what the server stored here (base64), so a test can
// check that only ciphertext arrived.
import { createServer } from 'node:http';

const port = Number(process.argv[2] ?? 8094);
const files = new Map(); // id -> { parents, data }
let next = 0;
let access = null;

const send = (res, status, body, type = 'application/json') => {
  res.writeHead(status, { 'content-type': type });
  res.end(typeof body === 'string' || Buffer.isBuffer(body) ? body : JSON.stringify(body));
};

const read = (req) =>
  new Promise((resolve) => {
    const parts = [];
    req.on('data', (c) => parts.push(c));
    req.on('end', () => resolve(Buffer.concat(parts)));
  });

// The file in a multipart/related upload: the second part's body.
function uploaded(body, type) {
  const sep = Buffer.from(`--${type.split('boundary=')[1]}`);
  const first = body.indexOf(sep);
  const second = body.indexOf(sep, first + sep.length);
  const metaStart = body.indexOf('\r\n\r\n', first) + 4;
  const meta = JSON.parse(body.subarray(metaStart, second - 2).toString());
  const dataStart = body.indexOf('\r\n\r\n', second) + 4;
  const end = body.indexOf(Buffer.from(`\r\n${sep}--`), dataStart);
  return { parents: meta.parents ?? [], data: body.subarray(dataStart, end) };
}

createServer(async (req, res) => {
  const url = new URL(req.url, `http://127.0.0.1:${port}`);
  const body = await read(req);
  const path = url.pathname;
  if (path === '/_health') return send(res, 200, 'ok', 'text/plain');
  if (path === '/_files') {
    return send(res, 200, [...files.values()].map((f) => f.data.toString('base64')));
  }
  if (path === '/auth') {
    const back = new URL(url.searchParams.get('redirect_uri'));
    back.searchParams.set('code', 'good-code');
    back.searchParams.set('state', url.searchParams.get('state'));
    res.writeHead(302, { location: back.toString() });
    return res.end();
  }
  if (path === '/token') {
    access = `access-${++next}`;
    return send(res, 200, { access_token: access, refresh_token: 'refresh-e2e', expires_in: 3600 });
  }
  if (path === '/revoke') return send(res, 200, {});
  if (req.headers.authorization !== `Bearer ${access}`) return send(res, 401, {});

  const used = [...files.values()].reduce((n, f) => n + f.data.length, 0);
  if (path === '/drive/v3/about') {
    return send(res, 200, { user: { emailAddress: 'e2e@example.com' }, storageQuota: { limit: String(15 * 2 ** 30), usage: String(used) } });
  }
  if (path === '/drive/v3/files' && req.method === 'POST') {
    const id = `folder-${++next}`;
    files.set(id, { parents: [], data: Buffer.alloc(0) });
    return send(res, 200, { id });
  }
  if (path === '/drive/v3/files' && req.method === 'GET') {
    const parent = (url.searchParams.get('q') ?? '').split("'")[1];
    const list = [...files].filter(([, f]) => f.parents.includes(parent)).map(([id, f]) => ({ id, size: String(f.data.length) }));
    return send(res, 200, { files: list });
  }
  if (path === '/upload/drive/v3/files' && req.method === 'POST') {
    const id = `file-${++next}`;
    files.set(id, uploaded(body, req.headers['content-type']));
    return send(res, 200, { id });
  }
  if (path.startsWith('/drive/v3/files/')) {
    const id = decodeURIComponent(path.slice('/drive/v3/files/'.length));
    const f = files.get(id);
    if (!f) return send(res, 404, {});
    if (req.method === 'DELETE') {
      files.delete(id);
      return send(res, 204, '');
    }
    if (url.searchParams.get('alt') === 'media') return send(res, 200, f.data, 'application/octet-stream');
  }
  send(res, 404, {});
}).listen(port, '127.0.0.1');
