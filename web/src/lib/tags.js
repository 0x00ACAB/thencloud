// Title, artist, album and cover art from the start of an audio file: ID3v2
// (MP3), FLAC and MP4 (M4A) tags. Files can be shared with us, so this is
// untrusted input: every read is bounds-checked, strings only ever reach
// the page as text, and a picture is kept only if its bytes are a known
// image format, typed from that, never from the tag.
//
// parseTags(bytes) -> { title, artist, album, track, picture: Blob } (any
// may be missing) or null.

const IMAGES = [
  [[0xff, 0xd8, 0xff], 'image/jpeg'],
  [[0x89, 0x50, 0x4e, 0x47], 'image/png'],
  [[0x47, 0x49, 0x46, 0x38], 'image/gif'],
];

function picture(bytes) {
  if (!bytes?.length) return undefined;
  const webp = bytes.length > 12 && text(bytes.subarray(0, 4)) === 'RIFF' && text(bytes.subarray(8, 12)) === 'WEBP';
  const type = webp ? 'image/webp' : IMAGES.find(([magic]) => magic.every((b, i) => bytes[i] === b))?.[1];
  return type ? new Blob([bytes.slice()], { type }) : undefined;
}

// Up to the first NUL: ID3v2.4 separates several values with one.
const text = (b, enc = 'latin1') => new TextDecoder(enc).decode(b).replace(/\0[\s\S]*$/, '').trim();
const be32 = (b, o) => ((b[o] << 24) | (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3]) >>> 0;
const le32 = (b, o) => (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) >>> 0;
const syncsafe = (b, o) => ((b[o] & 0x7f) << 21) | ((b[o + 1] & 0x7f) << 14) | ((b[o + 2] & 0x7f) << 7) | (b[o + 3] & 0x7f);
const trackNo = (v) => parseInt(v, 10) || undefined;

export function parseTags(bytes) {
  try {
    if (text(bytes.subarray(0, 3)) === 'ID3') return id3(bytes);
    if (text(bytes.subarray(0, 4)) === 'fLaC') return flac(bytes);
    if (text(bytes.subarray(4, 8)) === 'ftyp') return mp4(bytes);
  } catch {
    /* malformed: no tags */
  }
  return null;
}

// ------------------------------------------------------------------ ID3v2

const ENCODINGS = ['latin1', 'utf-16', 'utf-16be', 'utf-8'];

function id3(b) {
  const major = b[3];
  if (major < 3 || major > 4) return null;
  const flags = b[5];
  const end = Math.min(b.length, 10 + syncsafe(b, 6));
  let tag = b.subarray(0, end);
  // Whole-tag unsynchronisation (v2.3): FF 00 -> FF.
  if (flags & 0x80 && major === 3) {
    const out = [];
    for (let i = 0; i < tag.length; i++) if (!(tag[i] === 0 && tag[i - 1] === 0xff)) out.push(tag[i]);
    tag = new Uint8Array(out);
  }
  let o = 10;
  if (flags & 0x40) o += major === 4 ? syncsafe(tag, o) : be32(tag, o) + 4;
  const out = {};
  while (o + 10 <= tag.length) {
    const id = text(tag.subarray(o, o + 4));
    if (!/^[A-Z0-9]{4}$/.test(id)) break;
    const size = major === 4 ? syncsafe(tag, o + 4) : be32(tag, o + 4);
    const body = tag.subarray(o + 10, o + 10 + size);
    o += 10 + size;
    if (body.length < size || !body.length) break;
    let enc = ENCODINGS[body[0]] ?? 'latin1';
    if (enc === 'utf-16' && body[1] === 0xfe && body[2] === 0xff) enc = 'utf-16be';
    if (id === 'TIT2') out.title = text(body.subarray(1), enc);
    else if (id === 'TPE1') out.artist ??= text(body.subarray(1), enc);
    else if (id === 'TPE2') out.albumArtist = text(body.subarray(1), enc);
    else if (id === 'TALB') out.album = text(body.subarray(1), enc);
    else if (id === 'TRCK') out.track = trackNo(text(body.subarray(1), enc));
    else if (id === 'APIC' && !out.picture) out.picture = apic(body, enc);
  }
  out.artist ||= out.albumArtist;
  delete out.albumArtist;
  return out;
}

function apic(body, enc) {
  let i = body.indexOf(0, 1) + 1; // after the MIME type
  if (!i) return undefined;
  i++; // picture type
  // Description, ended by one NUL (two, aligned, in UTF-16).
  if (enc.startsWith('utf-16')) {
    while (i + 1 < body.length && (body[i] || body[i + 1])) i += 2;
    i += 2;
  } else {
    while (i < body.length && body[i]) i++;
    i++;
  }
  return picture(body.subarray(i));
}

// ------------------------------------------------------------------- FLAC

function flac(b) {
  const out = {};
  let o = 4;
  for (let last = false; !last && o + 4 <= b.length; ) {
    last = !!(b[o] & 0x80);
    const type = b[o] & 0x7f;
    const size = (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3];
    const body = b.subarray(o + 4, o + 4 + size);
    o += 4 + size;
    if (body.length < size) break;
    if (type === 4) Object.assign(out, vorbis(body));
    else if (type === 6 && !out.picture) {
      let p = 4;
      p += 4 + be32(body, p); // MIME type
      p += 4 + be32(body, p); // description
      p += 16; // width, height, depth, colours
      const len = be32(body, p);
      if (p + 4 + len <= body.length) out.picture = picture(body.subarray(p + 4, p + 4 + len));
    }
  }
  return out;
}

function vorbis(b) {
  const out = {};
  let o = 4 + le32(b, 0); // vendor string
  const count = le32(b, o);
  o += 4;
  for (let n = 0; n < count && o + 4 <= b.length; n++) {
    const len = le32(b, o);
    const s = new TextDecoder().decode(b.subarray(o + 4, o + 4 + len));
    o += 4 + len;
    const eq = s.indexOf('=');
    const [k, v] = [s.slice(0, eq).toUpperCase(), s.slice(eq + 1).trim()];
    if (!v) continue;
    if (k === 'TITLE') out.title ??= v;
    else if (k === 'ARTIST') out.artist ??= v;
    else if (k === 'ALBUMARTIST') out.artist ??= v;
    else if (k === 'ALBUM') out.album ??= v;
    else if (k === 'TRACKNUMBER') out.track ??= trackNo(v);
  }
  return out;
}

// -------------------------------------------------------------------- MP4

const CONTAINERS = new Set(['moov', 'udta', 'meta', 'ilst']);

function mp4(b) {
  const out = {};
  // The start of the file may end inside moov; walk what's there.
  const walk = (start, end, parent) => {
    for (let o = start; o + 8 <= end; ) {
      let size = be32(b, o);
      const type = text(b.subarray(o + 4, o + 8));
      if (size === 0) size = end - o;
      if (size < 8) return;
      // meta is a full box: 4 bytes of version and flags before its children.
      if (CONTAINERS.has(type)) walk(o + 8 + (type === 'meta' ? 4 : 0), Math.min(o + size, end), type);
      else if (parent === 'ilst' && o + size <= end) item(type, b.subarray(o + 8, o + size), out);
      o += size;
    }
  };
  walk(0, b.length, null);
  return out;
}

function item(type, body, out) {
  // Each item holds a 'data' box: size, 'data', type, locale, then the value.
  if (body.length < 16 || text(body.subarray(4, 8)) !== 'data') return;
  const value = body.subarray(16, Math.min(body.length, be32(body, 0)));
  if (type === '©nam') out.title = text(value, 'utf-8');
  else if (type === '©ART') out.artist ??= text(value, 'utf-8');
  else if (type === 'aART') out.artist ??= text(value, 'utf-8');
  else if (type === '©alb') out.album = text(value, 'utf-8');
  else if (type === 'trkn' && value.length >= 4) out.track = ((value[2] << 8) | value[3]) || undefined;
  else if (type === 'covr') out.picture ??= picture(value);
}
