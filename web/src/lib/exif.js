// Location and camera details in photos: found and removed in the browser,
// before a photo is encrypted and uploaded (or shared). Handles JPEG, PNG
// and WebP. EXIF, XMP and IPTC are dropped whole; only the orientation is
// kept (as a minimal EXIF block), so photos don't turn sideways.
//
// The input is untrusted: every read is bounds-checked, and anything that
// doesn't parse is left alone rather than guessed at.

const JPEG = 'image/jpeg';
const PNG = 'image/png';
const WEBP = 'image/webp';

/** The format to treat `bytes` as, from its first bytes (not its name). */
export function photoFormat(bytes) {
  if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return JPEG;
  if (bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47) return PNG;
  if (ascii(bytes, 0, 4) === 'RIFF' && ascii(bytes, 8, 4) === 'WEBP') return WEBP;
  return null;
}

/** Whether a file may be a photo worth looking at, before reading it. */
export const maybePhoto = (file) => /^image\/(jpeg|png|webp)$/.test(file.type) || /\.(jpe?g|png|webp)$/i.test(file.name);

function ascii(b, at, n) {
  if (at + n > b.length) return '';
  return String.fromCharCode(...b.subarray(at, at + n));
}

// ------------------------------------------------------------------- TIFF

/**
 * "2024:07:14 18:03:59", as EXIF writes dates, in ms. EXIF has no time
 * zone, so it's read as local time, as the camera's clock was set.
 */
function exifDate(s) {
  const m = /^(\d{4}):(\d{2}):(\d{2}) (\d{2}):(\d{2}):(\d{2})/.exec(s);
  if (!m) return null;
  const [y, mo, d, h, mi, se] = m.slice(1).map(Number);
  if (y < 1900 || y > 2200 || mo < 1 || mo > 12 || d < 1 || d > 31 || h > 23 || mi > 59 || se > 60) return null;
  return new Date(y, mo - 1, d, h, mi, se).getTime();
}

/**
 * What an EXIF (TIFF) block says: { gps, camera, orientation, taken }.
 * `gps` is true when it has a GPS section, `camera` when it names a make or
 * model, `taken` is when the photo was taken (ms) or null.
 */
export function readTiff(t) {
  const out = { gps: false, camera: false, orientation: 1, taken: null };
  if (t.length < 8) return out;
  const le = t[0] === 0x49 && t[1] === 0x49;
  if (!le && !(t[0] === 0x4d && t[1] === 0x4d)) return out;
  const v = new DataView(t.buffer, t.byteOffset, t.byteLength);
  const u16 = (o) => (o + 2 <= t.length ? v.getUint16(o, le) : null);
  const u32 = (o) => (o + 4 <= t.length ? v.getUint32(o, le) : null);
  // An ASCII date entry's value: 20 bytes, so always at an offset.
  const dateAt = (e) => {
    const n = u32(e + 4);
    const at = u32(e + 8);
    return n !== null && n >= 19 && n <= 64 && at !== null ? exifDate(ascii(t, at, 19)) : null;
  };
  const ifd = u32(4);
  const count = ifd === null ? null : u16(ifd);
  if (count === null) return out;
  let changed = null;
  let exifIfd = null;
  for (let i = 0; i < count; i++) {
    const e = ifd + 2 + i * 12;
    const tag = u16(e);
    if (tag === null) break;
    if (tag === 0x8825) out.gps = true;
    else if (tag === 0x010f || tag === 0x0110) out.camera = true;
    else if (tag === 0x0112) {
      const o = u16(e + 8);
      if (o >= 1 && o <= 8) out.orientation = o;
    } else if (tag === 0x0132) changed = dateAt(e);
    else if (tag === 0x8769) exifIfd = u32(e + 8);
  }
  // DateTimeOriginal lives in the Exif sub-IFD; DateTime in IFD0 is when
  // the file was last changed, the next best thing.
  const sub = exifIfd === null || exifIfd === ifd ? null : u16(exifIfd);
  for (let i = 0; sub !== null && i < Math.min(sub, 1000); i++) {
    const e = exifIfd + 2 + i * 12;
    if (u16(e) === 0x9003) {
      out.taken = dateAt(e);
      break;
    }
  }
  out.taken ??= changed;
  return out;
}

/** A TIFF block holding nothing but the orientation. */
function orientationTiff(orientation) {
  // Big-endian header, one IFD entry (0x0112, SHORT, 1), no next IFD.
  return new Uint8Array([0x4d, 0x4d, 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, orientation, 0, 0, 0, 0, 0, 0]);
}

// ------------------------------------------------------------------- JPEG

const EXIF_HEADER = [0x45, 0x78, 0x69, 0x66, 0, 0]; // "Exif\0\0"

/** JPEG segments before the image data: [{ marker, start, end, body }]. */
function jpegSegments(b) {
  const segs = [];
  let i = 2;
  while (i + 4 <= b.length && b[i] === 0xff) {
    const marker = b[i + 1];
    if (marker === 0xda || marker === 0xd9) break; // start of scan / end
    const len = (b[i + 2] << 8) | b[i + 3];
    if (len < 2 || i + 2 + len > b.length) return null;
    segs.push({ marker, start: i, end: i + 2 + len, body: b.subarray(i + 4, i + 2 + len) });
    i += 2 + len;
  }
  return i + 2 <= b.length && b[i] === 0xff ? segs : null;
}

const isExif = (s) => s.marker === 0xe1 && EXIF_HEADER.every((c, k) => s.body[k] === c);
const isXmp = (s) => s.marker === 0xe1 && ascii(s.body, 0, 28) === 'http://ns.adobe.com/xap/1.0/';
const isIptc = (s) => s.marker === 0xed; // APP13: Photoshop / IPTC

function jpegInfo(b) {
  const segs = jpegSegments(b);
  if (!segs) return null;
  const exif = segs.find(isExif);
  const tiff = exif ? readTiff(exif.body.subarray(6)) : { gps: false, camera: false, orientation: 1, taken: null };
  return { ...tiff, other: segs.some((s) => isXmp(s) || isIptc(s)), strip: () => stripJpeg(b, segs, tiff.orientation) };
}

function stripJpeg(b, segs, orientation) {
  let exif = null;
  if (orientation !== 1) {
    const body = [...EXIF_HEADER, ...orientationTiff(orientation)];
    const len = body.length + 2;
    exif = new Uint8Array([0xff, 0xe1, len >> 8, len & 0xff, ...body]);
  }
  const parts = [b.subarray(0, 2)];
  // EXIF goes right after a JFIF header when there is one, else first.
  if (exif && segs[0]?.marker !== 0xe0) parts.push(exif);
  for (const [k, s] of segs.entries()) {
    if (!isExif(s) && !isXmp(s) && !isIptc(s)) parts.push(b.subarray(s.start, s.end));
    if (exif && k === 0 && s.marker === 0xe0) parts.push(exif);
  }
  // The image data, untouched (segments sit back to back up to it).
  parts.push(b.subarray(segs.length ? segs[segs.length - 1].end : 2));
  return concat(parts);
}

// -------------------------------------------------------------------- PNG

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(bytes) {
  let c = 0xffffffff;
  for (const x of bytes) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function pngChunks(b) {
  const chunks = [];
  let i = 8;
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  while (i + 12 <= b.length) {
    const len = v.getUint32(i);
    const type = ascii(b, i + 4, 4);
    if (i + 12 + len > b.length) return null;
    chunks.push({ type, start: i, end: i + 12 + len, body: b.subarray(i + 8, i + 8 + len) });
    i += 12 + len;
    if (type === 'IEND') return chunks;
  }
  return null;
}

// Text chunks carry XMP and free-form notes (sometimes a location).
const PNG_META = new Set(['eXIf', 'tEXt', 'zTXt', 'iTXt', 'tIME']);

function pngInfo(b) {
  const chunks = pngChunks(b);
  if (!chunks) return null;
  const exif = chunks.find((c) => c.type === 'eXIf');
  const tiff = exif ? readTiff(exif.body) : { gps: false, camera: false, orientation: 1, taken: null };
  return { ...tiff, other: chunks.some((c) => PNG_META.has(c.type) && c.type !== 'eXIf'), strip: () => stripPng(b, chunks, tiff.orientation) };
}

function pngChunk(type, body) {
  const out = new Uint8Array(12 + body.length);
  const v = new DataView(out.buffer);
  v.setUint32(0, body.length);
  for (let k = 0; k < 4; k++) out[4 + k] = type.charCodeAt(k);
  out.set(body, 8);
  v.setUint32(8 + body.length, crc32(out.subarray(4, 8 + body.length)));
  return out;
}

function stripPng(b, chunks, orientation) {
  const parts = [b.subarray(0, 8)];
  for (const c of chunks) {
    if (PNG_META.has(c.type)) continue;
    parts.push(b.subarray(c.start, c.end));
    // eXIf must come before the image data.
    if (c.type === 'IHDR' && orientation !== 1) parts.push(pngChunk('eXIf', orientationTiff(orientation)));
  }
  return concat(parts);
}

// ------------------------------------------------------------------- WebP

function webpChunks(b) {
  const chunks = [];
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  let i = 12;
  while (i + 8 <= b.length) {
    const type = ascii(b, i, 4);
    const len = v.getUint32(i + 4, true);
    const end = i + 8 + len + (len & 1);
    if (i + 8 + len > b.length) return null;
    chunks.push({ type, start: i, end: Math.min(end, b.length), body: b.subarray(i + 8, i + 8 + len) });
    i = end;
  }
  return chunks;
}

function webpInfo(b) {
  const chunks = webpChunks(b);
  if (!chunks) return null;
  const exif = chunks.find((c) => c.type === 'EXIF');
  // Some writers keep the "Exif\0\0" prefix inside the chunk.
  const body = exif && EXIF_HEADER.every((c, k) => exif.body[k] === c) ? exif.body.subarray(6) : exif?.body;
  const tiff = body ? readTiff(body) : { gps: false, camera: false, orientation: 1, taken: null };
  return { ...tiff, other: chunks.some((c) => c.type === 'XMP '), strip: () => stripWebp(b, chunks, tiff.orientation) };
}

function stripWebp(b, chunks, orientation) {
  const keep = [];
  for (const c of chunks) {
    if (c.type === 'EXIF' || c.type === 'XMP ') continue;
    let bytes = b.slice(c.start, c.end);
    // VP8X flags say which optional chunks follow: EXIF 0x08, XMP 0x04.
    if (c.type === 'VP8X' && bytes.length > 8) bytes[8] = (bytes[8] & ~0x0c) | (orientation !== 1 ? 0x08 : 0);
    keep.push(bytes);
  }
  // Orientation can only be kept in an extended (VP8X) file.
  if (orientation !== 1 && keep[0] && ascii(keep[0], 0, 4) === 'VP8X') {
    const tiff = orientationTiff(orientation);
    const chunk = new Uint8Array(8 + tiff.length);
    chunk.set([0x45, 0x58, 0x49, 0x46]); // "EXIF"
    new DataView(chunk.buffer).setUint32(4, tiff.length, true);
    chunk.set(tiff, 8);
    keep.push(chunk); // EXIF goes after the image data
  }
  const body = concat(keep);
  const out = new Uint8Array(12 + body.length);
  out.set(b.subarray(0, 12));
  new DataView(out.buffer).setUint32(4, 4 + body.length, true);
  out.set(body, 12);
  return out;
}

// ------------------------------------------------------------------- API

function concat(parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

/**
 * What a photo carries: { gps, camera, other, strip() } where `strip()`
 * returns the photo without it, or null for anything that isn't a JPEG,
 * PNG or WebP that parses.
 */
export function photoInfo(bytes) {
  try {
    const format = photoFormat(bytes);
    if (format === JPEG) return jpegInfo(bytes);
    if (format === PNG) return pngInfo(bytes);
    if (format === WEBP) return webpInfo(bytes);
  } catch {
    /* malformed: leave it alone */
  }
  return null;
}

/** True when there's anything worth removing: a location, camera details or other metadata. */
export const hasDetails = (info) => !!info && (info.gps || info.camera || info.other);

/**
 * What a photo file carries (see photoInfo), reading as little as it can:
 * a JPEG's metadata sits at its start. Null for anything else.
 */
export async function fileInfo(file) {
  if (!maybePhoto(file) || file.size > MAX_PHOTO) return null;
  const head = file.size > HEAD ? new Uint8Array(await file.slice(0, HEAD).arrayBuffer()) : null;
  if (head && photoFormat(head) === JPEG) {
    const info = photoInfo(head);
    if (info) return { gps: info.gps, camera: info.camera, other: info.other, taken: info.taken };
  }
  const info = photoInfo(new Uint8Array(await file.arrayBuffer()));
  return info && { gps: info.gps, camera: info.camera, other: info.other, taken: info.taken };
}

/** When a photo file says it was taken (ms), or null. */
export async function photoTaken(file) {
  try {
    return (await fileInfo(file))?.taken ?? null;
  } catch {
    return null;
  }
}

const HEAD = 512 * 1024;
const MAX_PHOTO = 128 * 1024 * 1024;

/** `file` without location and camera details, or `file` itself when there are none. */
export async function stripFile(file) {
  const info = photoInfo(new Uint8Array(await file.arrayBuffer()));
  if (!hasDetails(info)) return file;
  return new File([info.strip()], file.name, { type: file.type, lastModified: file.lastModified });
}
