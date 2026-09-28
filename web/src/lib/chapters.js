// Chapters in an M4B/MP4 audiobook: the Nero `chpl` box in moov/udta, which
// ffmpeg and most audiobook tools write. Files can be shared with us, so
// this is untrusted input: every read is bounds-checked, and titles only
// ever reach the page as text.
//
// readChapters(file) -> [{ start, title }] (seconds, sorted), or [] when
// there are none or they can't be read. `file` is from openEntry:
// { count, chunkSize, read(i) }. The moov box often sits after the audio,
// so pieces are fetched as needed, a few at most.

const MAX_PIECES = 4;
const MAX_MOOV = 16 * 1024 * 1024;
const MAX_CHAPTERS = 2000;

const ascii = (b, s, e) => String.fromCharCode(...b.subarray(s, e));
const be32 = (b, o) => ((b[o] << 24) | (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3]) >>> 0;

export async function readChapters(file) {
  try {
    return await read(file);
  } catch {
    return []; // malformed or out of reach
  }
}

async function read(file) {
  const pieces = new Map();
  const size = file.size ?? file.count * file.chunkSize;
  const bytes = async (off, len) => {
    const out = new Uint8Array(len);
    for (let o = 0; o < len; ) {
      const i = Math.floor((off + o) / file.chunkSize);
      if (!pieces.has(i)) {
        if (pieces.size >= MAX_PIECES || i >= file.count) throw new Error('out of reach');
        pieces.set(i, await file.read(i));
      }
      const piece = pieces.get(i);
      const start = off + o - i * file.chunkSize;
      const n = Math.min(len - o, piece.length - start);
      if (n <= 0) throw new Error('short');
      out.set(piece.subarray(start, start + n), o);
      o += n;
    }
    return out;
  };

  // Top level: find moov, stepping over mdat by its size.
  let moov = null;
  for (let o = 0; o + 8 <= size; ) {
    const h = await bytes(o, 16).catch(() => bytes(o, 8));
    let len = be32(h, 0);
    let header = 8;
    if (len === 1 && h.length >= 16) {
      len = be32(h, 8) * 2 ** 32 + be32(h, 12);
      header = 16;
    } else if (len === 0) break; // runs to the end: that's mdat, not moov
    if (len < header) return [];
    if (ascii(h, 4, 8) === 'moov') {
      if (len > MAX_MOOV) return [];
      moov = await bytes(o + header, len - header);
      break;
    }
    o += len;
  }
  if (!moov) return [];

  const udta = child(moov, 0, moov.length, 'udta');
  const chpl = udta && child(moov, udta.start, udta.end, 'chpl');
  return chpl ? parseChpl(moov.subarray(chpl.start, chpl.end)) : [];
}

/** The first box of `type` directly inside b[start, end). */
function child(b, start, end, type) {
  for (let o = start; o + 8 <= end; ) {
    const len = be32(b, o);
    if (len < 8 || o + len > end) return null;
    if (ascii(b, o + 4, o + 8) === type) return { start: o + 8, end: o + len };
    o += len;
  }
  return null;
}

/** Nero chapters: version, flags, [4 bytes if version 1], count, then (time in 100 ns, title length, title). */
export function parseChpl(b) {
  if (b.length < 5) return [];
  let o = 4 + (b[0] >= 1 ? 4 : 0);
  if (o >= b.length) return [];
  const count = b[o++];
  const dec = new TextDecoder();
  const out = [];
  for (let i = 0; i < count && out.length < MAX_CHAPTERS; i++) {
    if (o + 9 > b.length) break;
    const start = (be32(b, o) * 2 ** 32 + be32(b, o + 4)) / 1e7;
    const n = b[o + 8];
    o += 9;
    if (o + n > b.length) break;
    const title = dec.decode(b.subarray(o, o + n)).replace(/\0[\s\S]*$/, '').trim();
    o += n;
    if (Number.isFinite(start) && start >= 0) out.push({ start, title: title || `Chapter ${out.length + 1}` });
  }
  return out.sort((a, b) => a.start - b.start);
}
