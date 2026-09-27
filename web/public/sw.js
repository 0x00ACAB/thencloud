// Streams decrypted files to the browser (downloads, video and audio, zips)
// without holding them in memory. This worker never sees a key and keeps no
// state: for each /_stream/<id> request it asks the open pages which one
// serves that id, and that page decrypts each piece and hands it over
// through a MessageChannel. Everything else goes to the network as usual.
// Nothing is cached or stored.

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));

/** Find the page serving stream `id`: { info, port }, or null. */
async function findStream(id) {
  const pages = await self.clients.matchAll({ type: 'window' });
  return new Promise((resolve) => {
    let left = pages.length;
    const timer = setTimeout(() => resolve(null), 3000);
    if (!left) return resolve(null);
    for (const page of pages) {
      const { port1, port2 } = new MessageChannel();
      port1.onmessage = (e) => {
        if (e.data?.info) {
          clearTimeout(timer);
          port1.onmessage = null;
          resolve({ info: e.data.info, port: port1 });
        } else if (--left === 0) {
          clearTimeout(timer);
          resolve(null);
        }
      };
      page.postMessage({ type: 'thencloud-stream', id }, [port2]);
    }
  });
}

/** Ask the page for a piece: { index } of a file, or the next piece of a sequential stream. */
function ask(port, msg) {
  return new Promise((resolve, reject) => {
    port.onmessage = (e) => (e.data.error ? reject(new Error(e.data.error)) : resolve(e.data));
    port.postMessage(msg);
  });
}

// The server's headers don't reach responses made here, so set the
// important ones: never sniff the type, and nothing may run or load.
const SAFE = { 'X-Content-Type-Options': 'nosniff', 'Content-Security-Policy': "default-src 'none'; sandbox" };

function disposition(info) {
  if (!info.download) return {};
  const ascii = info.name.replace(/[^\x20-\x7e]|["\\]/g, '_');
  return { 'Content-Disposition': `attachment; filename="${ascii}"; filename*=UTF-8''${encodeURIComponent(info.name)}` };
}

function fileResponse({ info, port }, request) {
  const { size, chunkSize } = info;
  let start = 0;
  let end = size - 1;
  let status = 200;
  const range = request.headers.get('range');
  const m = range && /^bytes=(\d*)-(\d*)$/.exec(range.trim());
  if (m && (m[1] || m[2])) {
    if (m[1] === '') start = Math.max(0, size - Number(m[2]));
    else {
      start = Number(m[1]);
      if (m[2]) end = Math.min(Number(m[2]), size - 1);
    }
    if (start >= size || start > end) return new Response(null, { status: 416, headers: { 'Content-Range': `bytes */${size}` } });
    status = 206;
  }
  let pos = start;
  const body = new ReadableStream({
    async pull(ctrl) {
      if (pos > end || size === 0) {
        port.close();
        return ctrl.close();
      }
      const index = Math.floor(pos / chunkSize);
      try {
        const { data } = await ask(port, { index });
        const bytes = new Uint8Array(data);
        const from = pos - index * chunkSize;
        const to = Math.min(bytes.length, end - index * chunkSize + 1);
        ctrl.enqueue(bytes.subarray(from, to));
        pos = index * chunkSize + to;
      } catch (e) {
        ctrl.error(e);
      }
    },
    cancel() {
      port.postMessage({ cancel: true });
      port.close();
    },
  });
  const headers = {
    'Content-Type': info.type,
    'Content-Length': String(size ? end - start + 1 : 0),
    'Accept-Ranges': 'bytes',
    'Cache-Control': 'no-store',
    ...SAFE,
    ...disposition(info),
  };
  if (status === 206) headers['Content-Range'] = `bytes ${start}-${end}/${size}`;
  return new Response(body, { status, headers });
}

function sequentialResponse({ info, port }) {
  const body = new ReadableStream({
    async pull(ctrl) {
      try {
        const { data, done } = await ask(port, { next: true });
        if (done) {
          port.close();
          ctrl.close();
        } else ctrl.enqueue(new Uint8Array(data));
      } catch (e) {
        ctrl.error(e);
      }
    },
    cancel() {
      port.postMessage({ cancel: true });
      port.close();
    },
  });
  return new Response(body, { headers: { 'Content-Type': info.type, 'Cache-Control': 'no-store', ...SAFE, ...disposition(info) } });
}

async function respond(id, request) {
  const s = await findStream(id);
  if (!s) return new Response('This stream has ended.', { status: 404 });
  return s.info.size == null ? sequentialResponse(s) : fileResponse(s, request);
}

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);
  if (url.origin !== self.location.origin || !url.pathname.startsWith('/_stream/')) return;
  e.respondWith(respond(url.pathname.slice('/_stream/'.length), e.request));
});
