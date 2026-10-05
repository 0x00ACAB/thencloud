// Zip downloads of folders and selections, built in the browser: every file
// is decrypted here and stored uncompressed (most files people keep are
// compressed already, and storing keeps it fast). The zip is produced piece
// by piece, so it can be streamed to disk (see stream.js) instead of being
// held in memory, and uses ZIP64 where sizes or offsets need it, so there's
// no 4 GB limit.

/**
 * A name that's safe as one path segment inside a zip. Names come from
 * whoever uploaded the file, so "../x" or "a/b" must not become paths that
 * escape the folder when someone unzips it.
 */
function segment(name) {
  const clean = name.replace(/[/\\\u0000]/g, '_').trim();
  return !clean || clean === '.' || clean === '..' ? '_' : clean;
}

/** `name`, or "name (2)", "name (3)"... if it's already in `taken`. */
function unique(name, taken) {
  let candidate = name;
  const dot = name.lastIndexOf('.');
  const [base, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ''];
  for (let i = 2; taken.has(candidate.toLowerCase()); i++) candidate = `${base} (${i})${ext}`;
  taken.add(candidate.toLowerCase());
  return candidate;
}

const CRC_TABLE = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});

function crc32(crc, bytes) {
  let c = ~crc;
  for (let i = 0; i < bytes.length; i++) c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return ~c >>> 0;
}

/** MS-DOS date and time, as zip headers want them. */
function dosTime(ms) {
  const d = new Date(ms || Date.now());
  const year = Math.max(1980, d.getFullYear());
  return {
    time: (d.getHours() << 11) | (d.getMinutes() << 5) | (d.getSeconds() >> 1),
    date: ((year - 1980) << 9) | ((d.getMonth() + 1) << 5) | d.getDate(),
  };
}

const MAX32 = 0xffffffff;
const enc = new TextEncoder();

/** Little-endian writer for a header of `n` bytes. */
function header(n) {
  const buf = new Uint8Array(n);
  const v = new DataView(buf.buffer);
  let o = 0;
  const w = {
    u16: (x) => (v.setUint16(o, x, true), (o += 2), w),
    u32: (x) => (v.setUint32(o, x >>> 0, true), (o += 4), w),
    u64: (x) => (v.setBigUint64(o, BigInt(x), true), (o += 8), w),
    bytes: (b) => (buf.set(b, o), (o += b.length), w),
    buf,
  };
  return w;
}

/**
 * Zip `entries` (files and folders, each { node, key, meta }), yielding the
 * zip's bytes piece by piece. `list(folderEntry)` returns a folder's
 * children; `open(entry)` returns { count, read(i) } for a file's decrypted
 * pieces. Reports progress from 0 to 1 by bytes.
 */
export async function* zipParts(entries, { list, open, onProgress }) {
  // Walk the tree first, so progress can count bytes.
  const items = []; // { entry, path, dir }
  async function walk(list_, prefix) {
    const taken = new Set();
    for (const entry of list_) {
      const path = prefix + unique(segment(entry.meta.name), taken);
      if (entry.node.kind === 'folder') {
        items.push({ entry, path: `${path}/`, dir: true });
        await walk(await list(entry), `${path}/`);
      } else {
        items.push({ entry, path, dir: false });
      }
    }
  }
  await walk(entries, '');
  const total = items.reduce((n, f) => n + (f.dir ? 0 : f.entry.meta.size), 0);

  const central = [];
  let offset = 0;
  let done = 0;
  for (const { entry, path, dir } of items) {
    const name = enc.encode(path);
    const size = dir ? 0 : entry.meta.size;
    const big = size >= MAX32;
    const { time, date } = dosTime(entry.meta.mtime);
    // Local header; CRC and sizes follow the data (flag bit 3), in a ZIP64
    // descriptor when the file is 4 GB or more.
    const local = header(30 + name.length + (big ? 20 : 0))
      .u32(0x04034b50)
      .u16(big ? 45 : 20)
      .u16(0x0808)
      .u16(0)
      .u16(time)
      .u16(date)
      .u32(0)
      .u32(big ? MAX32 : 0)
      .u32(big ? MAX32 : 0)
      .u16(name.length)
      .u16(big ? 20 : 0)
      .bytes(name);
    if (big) local.u16(1).u16(16).u64(0).u64(0);
    const start = offset;
    // Lengths are taken before yielding: the consumer may transfer (and so
    // empty) the buffer.
    offset += local.buf.length;
    yield local.buf;

    let crc = 0;
    let written = 0;
    if (!dir) {
      const file = open(entry);
      for (let i = 0; i < file.count; i++) {
        const piece = await file.read(i);
        crc = crc32(crc, piece);
        written += piece.length;
        if (piece.length) yield piece;
        onProgress?.(total ? (done + written) / total : 1);
      }
      done += written;
    }
    offset += written;
    const desc = big ? header(24).u32(0x08074b50).u32(crc).u64(written).u64(written) : header(16).u32(0x08074b50).u32(crc).u32(written).u32(written);
    offset += desc.buf.length;
    yield desc.buf;
    central.push({ name, crc, size: written, offset: start, time, date, dir });
  }

  // Central directory, with ZIP64 extras where a size or offset needs them.
  const cdStart = offset;
  for (const c of central) {
    const needSize = c.size >= MAX32;
    const needOffset = c.offset >= MAX32;
    const extraLen = needSize || needOffset ? 4 + (needSize ? 16 : 0) + (needOffset ? 8 : 0) : 0;
    const h = header(46 + c.name.length + extraLen)
      .u32(0x02014b50)
      .u16(45)
      .u16(extraLen ? 45 : 20)
      .u16(0x0808)
      .u16(0)
      .u16(c.time)
      .u16(c.date)
      .u32(c.crc)
      .u32(needSize ? MAX32 : c.size)
      .u32(needSize ? MAX32 : c.size)
      .u16(c.name.length)
      .u16(extraLen)
      .u16(0)
      .u16(0)
      .u16(0)
      .u32(c.dir ? 0x10 : 0)
      .u32(needOffset ? MAX32 : c.offset)
      .bytes(c.name);
    if (extraLen) {
      h.u16(1).u16(extraLen - 4);
      if (needSize) h.u64(c.size).u64(c.size);
      if (needOffset) h.u64(c.offset);
    }
    offset += h.buf.length;
    yield h.buf;
  }
  const cdSize = offset - cdStart;
  const zip64 = central.length >= 0xffff || cdStart >= MAX32 || cdSize >= MAX32;
  if (zip64) {
    const eocd64 = header(56 + 20)
      .u32(0x06064b50)
      .u64(44)
      .u16(45)
      .u16(45)
      .u32(0)
      .u32(0)
      .u64(central.length)
      .u64(central.length)
      .u64(cdSize)
      .u64(cdStart)
      // Locator
      .u32(0x07064b50)
      .u32(0)
      .u64(offset)
      .u32(1);
    yield eocd64.buf;
  }
  yield header(22)
    .u32(0x06054b50)
    .u16(0)
    .u16(0)
    .u16(zip64 ? 0xffff : central.length)
    .u16(zip64 ? 0xffff : central.length)
    .u32(zip64 ? MAX32 : cdSize)
    .u32(zip64 ? MAX32 : cdStart)
    .u16(0).buf;
}

/** Without the stream worker the whole zip is built in memory first. */
const MEMORY_LIMIT = 2 * 1024 ** 3;

/**
 * Zip `entries` and save it as `name`: streamed to disk through the service
 * worker (or the Android app's plugin) when there is one, else built in
 * memory. Resolves when done.
 */
export async function saveZip(entries, name, opts) {
  const { canSaveStreams, streamParts } = await import('./stream.js');
  const parts = zipParts(entries, opts);
  if (await canSaveStreams()) return streamParts(parts, name, 'application/zip');
  const chunks = [];
  let size = 0;
  for await (const p of parts) {
    size += p.length;
    if (size > MEMORY_LIMIT) throw new Error('That is too much to zip in memory in this browser. Download it in parts.');
    chunks.push(p);
  }
  const { saveBlob } = await import('./crypto.js');
  await saveBlob(new Blob(chunks, { type: 'application/zip' }), name);
}
