// Reading tar files (.tar, and .tar.gz / .tgz through the browser's own
// gunzip) for archive browsing. Like lib/unzip.js, the file comes from
// someone else: every header is checked (its checksum, sizes in range of
// the bytes we hold), the entry count is capped, and a gzipped tar is
// inflated only up to a set limit, whatever it claims. Understands POSIX
// ustar names (with their prefix), GNU long names and pax `path` and `size`
// records; links, devices and the like are listed as nothing.

const BLOCK = 512;
const MAX_ENTRIES = 20_000;
/** Most a .tar.gz may inflate to. */
export const MAX_GUNZIP = 512 * 1024 * 1024;
const MAX_NAME = 4096;
const MAX_PAX = 64 * 1024;

export class TarError extends Error {}

function fail(what) {
  throw new TarError(`This archive can't be read: ${what}.`);
}

const utf8 = new TextDecoder('utf-8', { fatal: false });

/** Whether `bytes` start like gzip. */
export const isGzip = (bytes) => bytes.length >= 2 && bytes[0] === 0x1f && bytes[1] === 0x8b;

/** Whether `bytes` start with a tar header (ustar magic, or a valid old-style checksum). */
export function isTar(bytes) {
  if (bytes.length < BLOCK) return false;
  const h = bytes.subarray(0, BLOCK);
  return checksumOk(h) && h.some((b) => b !== 0);
}

/** Inflate a gzip file, up to `limit` bytes. Throws TarError past it or if damaged. */
export async function gunzip(bytes, limit = MAX_GUNZIP) {
  const reader = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip')).getReader();
  const parts = [];
  let n = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      n += value.length;
      if (n > limit) fail('it unpacks to more than this browser will hold');
      parts.push(value);
    }
  } catch (e) {
    if (e instanceof TarError) throw e;
    fail('it is damaged');
  } finally {
    reader.cancel().catch(() => {});
  }
  const out = new Uint8Array(n);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

const cstr = (b) => {
  const end = b.indexOf(0);
  return utf8.decode(end < 0 ? b : b.subarray(0, end));
};

function octal(b, what) {
  // Base-256 (GNU, for large numbers): a set top bit, then big-endian.
  if (b[0] & 0x80) {
    let n = b[0] & 0x7f;
    for (let i = 1; i < b.length; i++) {
      n = n * 256 + b[i];
      if (n > Number.MAX_SAFE_INTEGER) fail(`its ${what} is out of range`);
    }
    return n;
  }
  const s = String.fromCharCode(...b)
    .replace(/[\0 ]+$/, '')
    .replace(/^ +/, '');
  if (!/^[0-7]*$/.test(s)) fail(`its ${what} is damaged`);
  return s ? parseInt(s, 8) : 0;
}

function checksumOk(h) {
  const want = h.subarray(148, 156);
  const s = String.fromCharCode(...want)
    .replace(/[\0 ]+$/, '')
    .replace(/^ +/, '');
  if (!/^[0-7]+$/.test(s)) return false;
  let sum = 0;
  for (let i = 0; i < BLOCK; i++) sum += i >= 148 && i < 156 ? 0x20 : h[i];
  return sum === parseInt(s, 8);
}

// `len key=value\n` records; only the ones we use are kept.
function pax(data) {
  const out = {};
  let at = 0;
  while (at < data.length) {
    const space = data.indexOf(0x20, at);
    if (space < 0 || space - at > 20) break;
    const len = Number(String.fromCharCode(...data.subarray(at, space)));
    if (!Number.isInteger(len) || len <= space - at || at + len > data.length) break;
    const rec = utf8.decode(data.subarray(space + 1, at + len - 1));
    const eq = rec.indexOf('=');
    if (eq > 0) {
      const key = rec.slice(0, eq);
      if (key === 'path' || key === 'size') out[key] = rec.slice(eq + 1);
    }
    at += len;
  }
  return out;
}

// A name as listed: no leading `/` or `./`, no empty or `.` segments.
function cleanName(name) {
  return name
    .split('/')
    .filter((s) => s && s !== '.')
    .join('/');
}

/** Open a tar held in memory. Same shape as openZip's result. Throws TarError if it can't be read. */
export function openTar(bytes) {
  const entries = new Map();
  let at = 0;
  let longName = null;
  let extended = null;
  while (at + BLOCK <= bytes.length) {
    const h = bytes.subarray(at, at + BLOCK);
    if (h.every((b) => b === 0)) break; // the end
    if (!checksumOk(h)) fail('a header is damaged');
    const type = String.fromCharCode(h[156]);
    let size = octal(h.subarray(124, 136), 'size');
    if (extended?.size !== undefined) {
      const n = Number(extended.size);
      if (!Number.isSafeInteger(n) || n < 0) fail('its size is out of range');
      size = n;
    }
    const start = at + BLOCK;
    if (size > bytes.length - start) fail('a file in it is cut short');
    const data = bytes.subarray(start, start + size);
    at = start + Math.ceil(size / BLOCK) * BLOCK;

    if (type === 'L') {
      if (size > MAX_NAME) fail('a name in it is too long');
      longName = cstr(data);
      continue;
    }
    if (type === 'x') {
      if (size > MAX_PAX) fail('its extended header is too large');
      extended = pax(data);
      continue;
    }
    if (type === 'g') continue; // global pax header: nothing we use

    let name = cstr(h.subarray(0, 100));
    if (utf8.decode(h.subarray(257, 262)) === 'ustar') {
      const prefix = cstr(h.subarray(345, 500));
      if (prefix) name = `${prefix}/${name}`;
    }
    if (longName !== null) name = longName;
    if (extended?.path) name = extended.path;
    longName = null;
    extended = null;

    // Regular files only ('0', or NUL from old tars, '7' contiguous).
    if (type !== '0' && type !== '\0' && type !== '7') continue;
    name = cleanName(name);
    if (!name) continue;
    if (!entries.has(name) && entries.size >= MAX_ENTRIES) fail('it has too many files in it');
    entries.set(name, data);
  }

  return {
    names: () => [...entries.keys()],
    has: (n) => entries.has(n),
    /** An entry's bytes (a copy). Throws TarError if missing. */
    async read(name) {
      const data = entries.get(name);
      if (!data) fail(`it has no ${name}`);
      return data.slice();
    },
    /** Every entry's name and size, for listing without reading them. */
    list: () => [...entries].map(([name, data]) => ({ name, size: data.length })),
  };
}
