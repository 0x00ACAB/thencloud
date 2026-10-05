// Streams decrypted files to the browser (downloads, video and audio, zips)
// without holding them in memory. This worker never sees a key: for each
// /_stream/<id> request it asks the open pages which one serves that id,
// and that page decrypts each piece and hands it over through a
// MessageChannel.
//
// It also tells people when the app this server sends them changes (see
// "Pins" below). A page can't vouch for itself, since the server sends it,
// but this worker was installed on an earlier visit and still runs that
// visit's code when the next page is fetched. It remembers the SHA-256 of
// each app page (/, the share page, /auth) and of each file under /assets/
// (content-hashed names, so the same name must always be the same bytes):
// - a page that changed shows a notice first, with the old and new version
//   and a way to go on;
// - an asset that changed under the same name is refused outright.
// Only hashes are stored, in the Cache API. It's a tripwire, not proof: a
// server that also replaces this worker can get past it. `thencloud
// verify-web` checks a server properly.

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

// In the desktop and Android apps (crates/thencloud-app) the pages come
// from the app itself, not a server, and change only when it's updated:
// nothing to watch, and a notice on every update would only be noise.
const BUNDLED = self.location.protocol === 'tauri:' || self.location.hostname === 'tauri.localhost';

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);
  if (url.origin !== self.location.origin || e.request.method !== 'GET') return;
  if (url.pathname.startsWith('/_stream/')) {
    e.respondWith(respond(url.pathname.slice('/_stream/'.length), e.request));
  } else if (BUNDLED) {
    return;
  } else if (e.request.mode === 'navigate' && pageKind(url.pathname)) {
    e.respondWith(checkPage(e.request, url).catch(() => fetch(e.request)));
  } else if (/^\/assets\/[^/]+\.(js|mjs|css|wasm)$/.test(url.pathname)) {
    e.respondWith(checkAsset(e.request, url).catch(() => fetch(e.request)));
  }
});

// --------------------------------------------------------------------- Pins

const PINS = 'thencloud-pins';
/** Bigger assets (ffmpeg's core) aren't hashed on every load. */
const MAX_PINNED = 16 * 1024 * 1024;

function pageKind(path) {
  if (path === '/' || path === '/index.html') return 'app';
  if (path.startsWith('/s/') || path === '/share.html') return 'share';
  if (path === '/auth' || path === '/auth.html') return 'auth';
  return null;
}

async function sha256(buf) {
  const d = new Uint8Array(await crypto.subtle.digest('SHA-256', buf));
  return Array.from(d, (b) => b.toString(16).padStart(2, '0')).join('');
}

async function readPin(key) {
  const res = await (await caches.open(PINS)).match(key);
  return res ? res.json() : null;
}

async function writePin(key, value) {
  await (await caches.open(PINS)).put(key, new Response(JSON.stringify(value)));
}

async function checkAsset(request, url) {
  const res = await fetch(request);
  if (!res.ok) return res;
  const buf = await res.clone().arrayBuffer();
  if (buf.byteLength > MAX_PINNED) return res;
  const hash = await sha256(buf);
  const key = `/__pin/asset${url.pathname}`;
  const pin = await readPin(key);
  if (!pin) {
    await writePin(key, { hash });
    return res;
  }
  if (pin.hash === hash) return res;
  // The same content-hashed name with other bytes is never an update.
  await writePin('/__pin/tampered', { path: url.pathname, at: Date.now() });
  return new Response('Refused: this file changed on the server under the same name.', { status: 502 });
}

const versionOf = (html) => /<meta name="thencloud-version" content="([\w.+-]*)"/.exec(html)?.[1] || '?';

async function checkPage(request, url) {
  const accept = url.searchParams.get('thencloud-accept');
  if (accept) url.searchParams.delete('thencloud-accept');
  const res = await fetch(accept ? new Request(url, { credentials: 'same-origin' }) : request);
  if (!res.ok || !(res.headers.get('content-type') || '').startsWith('text/html')) return res;
  const buf = await res.clone().arrayBuffer();
  const html = new TextDecoder().decode(buf);
  // The development server rewrites pages on every change.
  if (html.includes('/@vite/client')) return res;
  const hash = await sha256(buf);
  const version = versionOf(html);
  const key = `/__pin/page/${pageKind(url.pathname)}`;
  const pin = await readPin(key);
  const tampered = await readPin('/__pin/tampered');
  if (accept && accept === hash) {
    await writePin(key, { hash, version });
    if (tampered) {
      // Going on anyway: the files that changed are taken as they are now.
      const cache = await caches.open(PINS);
      for (const k of await cache.keys()) if (new URL(k.url).pathname.startsWith('/__pin/asset/')) await cache.delete(k);
      await cache.delete('/__pin/tampered');
    }
    return Response.redirect(url.href, 303);
  }
  if (!pin && !tampered) {
    await writePin(key, { hash, version });
    return res;
  }
  if (pin?.hash === hash && !tampered) return res;
  return notice(pin, { hash, version }, tampered);
}

const TEXT = {
  en: {
    title: 'The app this server sends has changed',
    updated: (a, b) => `Since your last visit, this server's web app changed from version ${a} to ${b}. That's normal after an update.`,
    same: (a) => `Since your last visit, this server's web app changed, but it still says it's version ${a}. A real update comes with a new version, so this may mean the server, or something between you and it, changed the app.`,
    files: 'Some of the app\'s code files changed on the server without changing their names, which an update never does. They were refused.',
    why: 'The app runs in your browser and handles your keys, so a changed app could read your files. If you didn\'t expect a change, ask whoever runs this server, and check it with thencloud verify-web against the signed release before you sign in.',
    limit: 'This notice comes from the copy of the app your browser kept from before. A server that also replaces that copy could get past it.',
    go: 'Continue to the new version',
    anyway: 'Open it anyway',
  },
  pl: {
    title: 'Aplikacja wysyłana przez ten serwer się zmieniła',
    updated: (a, b) => `Od Twojej ostatniej wizyty aplikacja tego serwera zmieniła się z wersji ${a} na ${b}. Po aktualizacji to normalne.`,
    same: (a) => `Od Twojej ostatniej wizyty aplikacja tego serwera się zmieniła, ale nadal podaje wersję ${a}. Prawdziwa aktualizacja ma nową wersję, więc może to znaczyć, że aplikację zmienił serwer albo coś pomiędzy Tobą a nim.`,
    files: 'Niektóre pliki z kodem aplikacji zmieniły się na serwerze bez zmiany nazw, czego aktualizacja nigdy nie robi. Zostały odrzucone.',
    why: 'Aplikacja działa w przeglądarce i obsługuje Twoje klucze, więc zmieniona aplikacja mogłaby czytać Twoje pliki. Jeśli nie spodziewałeś się zmiany, zapytaj osobę prowadzącą serwer i sprawdź go poleceniem thencloud verify-web z podpisanym wydaniem, zanim się zalogujesz.',
    limit: 'To powiadomienie pochodzi z kopii aplikacji, którą przeglądarka zachowała wcześniej. Serwer, który podmieni także tę kopię, mógłby je obejść.',
    go: 'Przejdź do nowej wersji',
    anyway: 'Otwórz mimo to',
  },
  de: {
    title: 'Die App, die dieser Server sendet, hat sich geändert',
    updated: (a, b) => `Seit deinem letzten Besuch hat sich die Web-App dieses Servers von Version ${a} auf ${b} geändert. Nach einem Update ist das normal.`,
    same: (a) => `Seit deinem letzten Besuch hat sich die Web-App dieses Servers geändert, sie nennt aber weiter Version ${a}. Ein echtes Update bringt eine neue Version, also hat vielleicht der Server oder etwas zwischen dir und ihm die App verändert.`,
    files: 'Einige Code-Dateien der App haben sich auf dem Server geändert, ohne ihren Namen zu ändern, was ein Update nie tut. Sie wurden abgelehnt.',
    why: 'Die App läuft in deinem Browser und verwaltet deine Schlüssel, eine veränderte App könnte also deine Dateien lesen. Wenn du keine Änderung erwartet hast, frag, wer den Server betreibt, und prüfe ihn mit thencloud verify-web gegen das signierte Release, bevor du dich anmeldest.',
    limit: 'Dieser Hinweis kommt aus der Kopie der App, die dein Browser von vorher behalten hat. Ein Server, der auch diese Kopie ersetzt, könnte ihn umgehen.',
    go: 'Weiter zur neuen Version',
    anyway: 'Trotzdem öffnen',
  },
};

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

async function notice(pin, now, tampered) {
  const lang = (self.navigator.language || 'en').slice(0, 2);
  const t = TEXT[lang] || TEXT.en;
  const updated = !tampered && pin && pin.version !== now.version;
  const what = tampered ? t.files : updated ? t.updated(esc(pin.version), esc(now.version)) : t.same(esc(now.version));
  // Keeps what's after # (a share link's key) when going on.
  const script = `document.getElementById('go').addEventListener('click',function(){var u=new URL(location.href);u.searchParams.set('thencloud-accept','${now.hash}');location.replace(u.pathname+u.search+location.hash)})`;
  const scriptHash = btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(script)))));
  const html = `<!doctype html><html lang="${esc(lang)}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${esc(t.title)}</title>
<style>:root{color-scheme:light dark;font:15px/1.6 system-ui,sans-serif}body{max-width:34rem;margin:12vh auto;padding:0 16px}h1{font-size:1.15rem}p{opacity:.85}button{font:inherit;padding:.5rem 1rem;border-radius:6px;border:1px solid #888;background:transparent;color:inherit;cursor:pointer}small{opacity:.7}</style>
</head><body><h1>${esc(t.title)}</h1><p>${what}</p><p>${esc(t.why)}</p><p><button id="go" type="button">${esc(updated ? t.go : t.anyway)}</button></p><p><small>${esc(t.limit)}</small></p>
<script>${script}</script></body></html>`;
  return new Response(html, {
    headers: {
      'Content-Type': 'text/html; charset=utf-8',
      'Cache-Control': 'no-store',
      'Content-Security-Policy': `default-src 'none'; style-src 'unsafe-inline'; script-src 'sha256-${scriptHash}'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`,
      'Referrer-Policy': 'no-referrer',
    },
  });
}
