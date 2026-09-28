// Reading zip files (EPUB and CBZ books) in the browser. The file comes
// from someone else, so everything is bounds-checked, and nothing may make
// us use more memory than a set limit: the entry count, each entry's size
// once inflated, and the total read are all capped, whatever the headers
// claim. Deflate is the browser's own (DecompressionStream). ZIP64,
// encrypted and other compression methods are refused.

const EOCD = 0x06054b50;
const CENTRAL = 0x02014b50;
const LOCAL = 0x04034b50;
const MAX_ENTRIES = 20_000;
/** Largest single entry once inflated. */
export const MAX_ENTRY = 64 * 1024 * 1024;
/** Most we inflate from one zip, over all reads. */
const MAX_TOTAL = 512 * 1024 * 1024;

export class ZipError extends Error {}

const utf8 = new TextDecoder('utf-8', { fatal: false });
const latin1 = new TextDecoder('latin1');

/** Open a zip held in memory. Throws ZipError if it can't be read. */
export function openZip(bytes) {
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const u16 = (o) => (o >= 0 && o + 2 <= bytes.length ? v.getUint16(o, true) : fail('it ends too soon'));
  const u32 = (o) => (o >= 0 && o + 4 <= bytes.length ? v.getUint32(o, true) : fail('it ends too soon'));

  // The end record is in the last 64 KiB + 22 bytes (it may carry a comment).
  let end = -1;
  for (let i = bytes.length - 22; i >= Math.max(0, bytes.length - 22 - 0xffff); i--) {
    if (v.getUint32(i, true) === EOCD) {
      end = i;
      break;
    }
  }
  if (end < 0) fail("it isn't a zip file");
  const count = u16(end + 10);
  const cdSize = u32(end + 12);
  const cdStart = u32(end + 16);
  if (count === 0xffff || cdStart === 0xffffffff) fail('ZIP64 files are not supported');
  if (count > MAX_ENTRIES) fail('it has too many files in it');
  if (cdStart + cdSize > end) fail('its directory is out of place');

  const entries = new Map();
  let at = cdStart;
  for (let i = 0; i < count; i++) {
    if (u32(at) !== CENTRAL) fail('its directory is damaged');
    const flags = u16(at + 8);
    const method = u16(at + 10);
    const packed = u32(at + 20);
    const size = u32(at + 24);
    const nameLen = u16(at + 28);
    const extraLen = u16(at + 30);
    const commentLen = u16(at + 32);
    const local = u32(at + 42);
    if (at + 46 + nameLen > bytes.length) fail('its directory is damaged');
    const raw = bytes.subarray(at + 46, at + 46 + nameLen);
    const name = (flags & 0x800 ? utf8 : latin1).decode(raw);
    at += 46 + nameLen + extraLen + commentLen;
    if (name.endsWith('/')) continue; // a folder
    entries.set(name, { flags, method, packed, size, local });
  }

  let inflated = 0;

  /** An entry's bytes. Throws ZipError if missing or unreadable. */
  async function read(name) {
    const e = entries.get(name);
    if (!e) fail(`it has no ${name}`);
    if (e.flags & 1) fail('it is password protected');
    if (e.size > MAX_ENTRY) fail('a file in it is too large');
    if (u32(e.local) !== LOCAL) fail('a file in it is damaged');
    const start = e.local + 30 + u16(e.local + 26) + u16(e.local + 28);
    if (start + e.packed > bytes.length) fail('a file in it is cut short');
    const data = bytes.subarray(start, start + e.packed);
    if (e.method === 0) return data.slice();
    if (e.method !== 8) fail('it uses a compression method this browser can\'t read');
    const out = await inflate(data, Math.min(e.size, MAX_ENTRY, MAX_TOTAL - inflated));
    inflated += out.length;
    return out;
  }

  return { names: () => [...entries.keys()], has: (n) => entries.has(n), read };
}

function fail(what) {
  throw new ZipError(`This file can't be opened: ${what}.`);
}

/** Inflate raw deflate data, refusing to produce more than `limit` bytes. */
async function inflate(data, limit) {
  const stream = new Blob([data]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
  const reader = stream.getReader();
  const parts = [];
  let n = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      n += value.length;
      if (n > limit) fail('a file in it is larger than it says');
      parts.push(value);
    }
  } catch (e) {
    if (e instanceof ZipError) throw e;
    fail('a file in it is damaged');
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
