// Episode details from a video file: MP4 (iTunes-style tvsh, tvsn, tves and
// ©nam tags) and Matroska/WebM (the segment title). Files can be shared with
// us, so this is untrusted input: every read is bounds-checked and strings
// only ever reach the page as text.
//
// readVideoTags(file, head) -> { series, season, episode, title } (any may
// be missing) or null. `file` is from openEntry; `head` is its first piece
// if already read. For an MP4 with its index at the end, a few more pieces
// are fetched to reach it.

const text = (b) => new TextDecoder().decode(b).replace(/\0[\s\S]*$/, '').trim();
const ascii = (b, s, e) => String.fromCharCode(...b.subarray(s, e));
const be32 = (b, o) => ((b[o] << 24) | (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3]) >>> 0;

export async function readVideoTags(file, head) {
  head ??= await file.read(0);
  try {
    if (head.length >= 8 && ascii(head, 4, 8) === 'ftyp') return await mp4(file, head);
    if (head.length >= 4 && be32(head, 0) === 0x1a45dfa3) return mkv(head);
  } catch {
    /* malformed or out of reach: no tags */
  }
  return null;
}

// -------------------------------------------------------------------- MP4

const MAX_PIECES = 4;

async function mp4(file, head) {
  const pieces = new Map([[0, head]]);
  const bytes = async (off, len) => {
    const out = new Uint8Array(len);
    for (let o = 0; o < len; ) {
      const i = Math.floor((off + o) / file.chunkSize);
      if (!pieces.has(i)) {
        if (pieces.size > MAX_PIECES || i >= file.count) throw new Error('out of reach');
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
  async function* boxes(start, end) {
    for (let o = start; o + 8 <= end; ) {
      const h = await bytes(o, Math.min(16, end - o));
      let size = be32(h, 0);
      let header = 8;
      if (size === 1 && h.length >= 16) {
        size = be32(h, 8) * 2 ** 32 + be32(h, 12);
        header = 16;
      } else if (size === 0) size = end - o;
      if (size < header) return;
      yield { type: ascii(h, 4, 8), start: o + header, end: Math.min(o + size, end) };
      o += size;
    }
  }
  const find = async (start, end, type) => {
    for await (const b of boxes(start, end)) if (b.type === type) return b;
    return null;
  };

  const moov = await find(0, file.size, 'moov');
  const udta = moov && (await find(moov.start, moov.end, 'udta'));
  const meta = udta && (await find(udta.start, udta.end, 'meta'));
  // meta is a full box: 4 bytes of version and flags before its children.
  const ilst = meta && (await find(meta.start + 4, meta.end, 'ilst'));
  if (!ilst) return null;
  const out = {};
  for await (const item of boxes(ilst.start, ilst.end)) {
    if (!['tvsh', 'tvsn', 'tves', '©nam'].includes(item.type) || item.end - item.start > 4096) continue;
    // Each item holds a 'data' box: size, 'data', type, locale, then the value.
    const body = await bytes(item.start, item.end - item.start);
    if (body.length < 16 || ascii(body, 4, 8) !== 'data') continue;
    const value = body.subarray(16, Math.min(body.length, be32(body, 0)));
    const int = () => (value.length >= 4 ? be32(value, value.length - 4) : value[0]);
    if (item.type === 'tvsh') out.series = text(value);
    else if (item.type === '©nam') out.title = text(value);
    else if (item.type === 'tvsn') out.season = int();
    else if (item.type === 'tves') out.episode = int();
  }
  if (out.episode == null) delete out.season;
  return Object.keys(out).length ? out : null;
}

// --------------------------------------------------------------- Matroska

/** A variable-length integer at `o`: [value, length]. IDs keep their marker bit. */
function vint(b, o, id = false) {
  const first = b[o];
  let len = 1;
  while (len <= 8 && !(first & (0x80 >> (len - 1)))) len++;
  if (len > 8 || o + len > b.length) throw new Error('bad vint');
  let v = id ? first : first & (0xff >> len);
  let unknown = v === 0xff >> len;
  for (let i = 1; i < len; i++) {
    v = v * 256 + b[o + i];
    unknown &&= b[o + i] === 0xff;
  }
  return [unknown && !id ? Infinity : v, len];
}

const SEGMENT = 0x18538067;
const INFO = 0x1549a966;
const TITLE = 0x7ba9;
const CLUSTER = 0x1f43b675;

function mkv(b) {
  const walk = (start, end) => {
    for (let o = start; o < end; ) {
      const [id, il] = vint(b, o, true);
      const [size, sl] = vint(b, o + il);
      const body = o + il + sl;
      const stop = Math.min(end, body + size);
      if (id === CLUSTER) return null;
      if (id === SEGMENT || id === INFO) return walk(body, stop);
      if (id === TITLE) {
        const title = text(b.subarray(body, stop));
        return title ? { title } : null;
      }
      if (size === Infinity) return null;
      o = body + size;
    }
    return null;
  };
  return walk(0, b.length);
}
