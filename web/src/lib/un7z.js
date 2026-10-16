// Reading 7z archives for archive browsing. Like lib/unzip.js and
// lib/untar.js, the file comes from someone else: the start and next
// headers are checked against their CRCs, every count and size is checked
// against the bytes we hold, entries are capped, and a block is unpacked
// only if what it declares fits under a set limit. Files compressed with
// LZMA, LZMA2 or Deflate, or stored, open (with the BCJ x86 filter in front,
// as 7-Zip uses for programs); the listing works whatever they use, and
// an entry with another method (PPMd, BZip2, BCJ2, a password) says so
// when it's read. Same shape as openZip's result.

import { decodeLzma, decodeLzma2, LzmaError } from './lzma.js';

const MAX_ENTRIES = 20_000;
/** Most a block (a "folder", often the whole archive when it's solid) may unpack to. */
export const MAX_UNPACK = 512 * 1024 * 1024;
/** Most the header itself may unpack to. */
const MAX_HEADER = 64 * 1024 * 1024;
const MAX_CODERS = 4;

export class SevenZipError extends Error {}

function fail(what) {
  throw new SevenZipError(`This archive can't be read: ${what}.`);
}

const SIGNATURE = [0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c];

/** Whether `bytes` start with the 7z signature. */
export const is7z = (bytes) => bytes.length >= 32 && SIGNATURE.every((b, i) => bytes[i] === b);

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

export function crc32(bytes) {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// Property ids in the header.
const K = {
  End: 0x00,
  Header: 0x01,
  ArchiveProperties: 0x02,
  AdditionalStreamsInfo: 0x03,
  MainStreamsInfo: 0x04,
  FilesInfo: 0x05,
  PackInfo: 0x06,
  UnpackInfo: 0x07,
  SubStreamsInfo: 0x08,
  Size: 0x09,
  CRC: 0x0a,
  Folder: 0x0b,
  CodersUnpackSize: 0x0c,
  NumUnpackStream: 0x0d,
  EmptyStream: 0x0e,
  EmptyFile: 0x0f,
  Name: 0x11,
  WinAttributes: 0x15,
  EncodedHeader: 0x17,
  Dummy: 0x19,
};

const METHOD = {
  COPY: '00',
  DELTA: '03',
  BCJ: '03030103',
  BCJ2: '0303011b',
  LZMA: '030101',
  LZMA2: '21',
  DEFLATE: '040108',
  AES: '06f10701',
};

/** A cursor over header bytes. */
class Reader {
  constructor(bytes) {
    this.b = bytes;
    this.at = 0;
  }

  byte() {
    if (this.at >= this.b.length) fail('its header ends too soon');
    return this.b[this.at++];
  }

  bytes(n) {
    if (n > this.b.length - this.at) fail('its header ends too soon');
    const out = this.b.subarray(this.at, this.at + n);
    this.at += n;
    return out;
  }

  u32() {
    const b = this.bytes(4);
    return (b[0] | (b[1] << 8) | (b[2] << 16) | (b[3] << 24)) >>> 0;
  }

  /** 7z's variable-length number: the first byte's high bits say how many follow. */
  number() {
    const first = this.byte();
    let mask = 0x80;
    let value = 0;
    for (let i = 0; i < 8; i++) {
      if ((first & mask) === 0) {
        const high = first & (mask - 1);
        value += high * 2 ** (8 * i);
        if (!Number.isSafeInteger(value)) fail('a number in it is out of range');
        return value;
      }
      value += this.byte() * 2 ** (8 * i);
      mask >>>= 1;
    }
    if (!Number.isSafeInteger(value)) fail('a number in it is out of range');
    return value;
  }

  /** A count of things that each take at least one byte of what's left. */
  count() {
    const n = this.number();
    if (n > this.b.length - this.at) fail('a count in it is out of range');
    return n;
  }

  bits(n) {
    const out = new Array(n);
    let mask = 0;
    let byte = 0;
    for (let i = 0; i < n; i++) {
      if (mask === 0) {
        byte = this.byte();
        mask = 0x80;
      }
      out[i] = (byte & mask) !== 0;
      mask >>>= 1;
    }
    return out;
  }

  /** A bit vector that may be "all set" (a leading 1) instead. */
  allOrBits(n) {
    return this.byte() ? new Array(n).fill(true) : this.bits(n);
  }

  digests(n) {
    const defined = this.allOrBits(n);
    return defined.map((d) => (d ? this.u32() : null));
  }

  expect(id) {
    if (this.byte() !== id) fail('its header is out of order');
  }
}

function readPackInfo(r) {
  const packPos = r.number();
  const n = r.count();
  const sizes = new Array(n).fill(0);
  for (;;) {
    const id = r.byte();
    if (id === K.End) break;
    if (id === K.Size) for (let i = 0; i < n; i++) sizes[i] = r.number();
    else if (id === K.CRC) r.digests(n);
    else fail('its pack info is damaged');
  }
  return { packPos, sizes };
}

function readFolder(r) {
  const numCoders = r.count();
  if (numCoders < 1 || numCoders > MAX_CODERS) fail('a block has too many steps');
  const coders = [];
  let numIn = 0;
  let numOut = 0;
  for (let i = 0; i < numCoders; i++) {
    const flags = r.byte();
    if (flags & 0xc0) fail('a block uses an unknown kind of step');
    const id = [...r.bytes(flags & 0x0f)].map((b) => b.toString(16).padStart(2, '0')).join('');
    let ins = 1;
    let outs = 1;
    if (flags & 0x10) {
      ins = r.count();
      outs = r.count();
      if (ins > 8 || outs > 8) fail('a block has too many streams');
    }
    const props = flags & 0x20 ? r.bytes(r.count()) : new Uint8Array(0);
    coders.push({ id, ins, outs, props, firstIn: numIn, firstOut: numOut });
    numIn += ins;
    numOut += outs;
  }
  if (numOut < 1) fail('a block has no output');
  const bindPairs = [];
  for (let i = 0; i < numOut - 1; i++) {
    const inIndex = r.number();
    const outIndex = r.number();
    if (inIndex >= numIn || outIndex >= numOut) fail('a block is wired wrong');
    bindPairs.push({ inIndex, outIndex });
  }
  const numPacked = numIn - bindPairs.length;
  if (numPacked < 1) fail('a block has no input');
  let packed = [];
  if (numPacked === 1) {
    for (let i = 0; i < numIn; i++) {
      if (!bindPairs.some((p) => p.inIndex === i)) {
        packed.push(i);
        break;
      }
    }
    if (!packed.length) fail('a block is wired wrong');
  } else {
    for (let i = 0; i < numPacked; i++) {
      const at = r.number();
      if (at >= numIn) fail('a block is wired wrong');
      packed.push(at);
    }
  }
  return { coders, bindPairs, packed, numIn, numOut, sizes: [], crc: null };
}

function readUnpackInfo(r) {
  r.expect(K.Folder);
  const n = r.count();
  if (r.byte() !== 0) fail('it keeps its blocks somewhere else');
  const folders = [];
  for (let i = 0; i < n; i++) folders.push(readFolder(r));
  r.expect(K.CodersUnpackSize);
  for (const f of folders) for (let i = 0; i < f.numOut; i++) f.sizes.push(r.number());
  for (;;) {
    const id = r.byte();
    if (id === K.End) break;
    if (id === K.CRC) r.digests(n).forEach((d, i) => (folders[i].crc = d));
    else fail('its block info is damaged');
  }
  return folders;
}

/** The output stream of a block nothing else reads: what it unpacks to. */
function mainOut(f) {
  for (let i = 0; i < f.numOut; i++) if (!f.bindPairs.some((p) => p.outIndex === i)) return i;
  fail('a block is wired wrong');
}

const unpackSize = (f) => f.sizes[mainOut(f)];

function readSubStreams(r, folders) {
  const counts = folders.map(() => 1);
  let id = r.byte();
  if (id === K.NumUnpackStream) {
    for (let i = 0; i < folders.length; i++) counts[i] = r.count();
    id = r.byte();
  }
  const sizes = [];
  for (let i = 0; i < folders.length; i++) {
    const n = counts[i];
    if (n === 0) continue;
    let sum = 0;
    if (id === K.Size) {
      for (let j = 0; j < n - 1; j++) {
        const s = r.number();
        sizes.push(s);
        sum += s;
      }
    }
    const total = unpackSize(folders[i]);
    if (sum > total) fail('its file sizes add up to more than their block');
    if (id !== K.Size && n > 1) fail('its file sizes are missing');
    sizes.push(total - sum);
  }
  if (id === K.Size) id = r.byte();
  // CRCs of the streams whose block has no single CRC of its own.
  const digests = [];
  let missing = 0;
  folders.forEach((f, i) => {
    if (!(counts[i] === 1 && f.crc !== null)) missing += counts[i];
  });
  while (id !== K.End) {
    if (id === K.CRC) {
      const got = r.digests(missing);
      let k = 0;
      folders.forEach((f, i) => {
        if (counts[i] === 1 && f.crc !== null) digests.push(f.crc);
        else for (let j = 0; j < counts[i]; j++) digests.push(got[k++]);
      });
    } else {
      fail('its file info is damaged');
    }
    id = r.byte();
  }
  if (!digests.length) folders.forEach((f, i) => {
    for (let j = 0; j < counts[i]; j++) digests.push(counts[i] === 1 ? f.crc : null);
  });
  return { counts, sizes, digests };
}

function readStreamsInfo(r) {
  let pack = { packPos: 0, sizes: [] };
  let folders = [];
  let sub = null;
  for (;;) {
    const id = r.byte();
    if (id === K.End) break;
    if (id === K.PackInfo) pack = readPackInfo(r);
    else if (id === K.UnpackInfo) folders = readUnpackInfo(r);
    else if (id === K.SubStreamsInfo) sub = readSubStreams(r, folders);
    else fail('its stream info is damaged');
  }
  if (!sub) sub = { counts: folders.map(() => 1), sizes: folders.map(unpackSize), digests: folders.map((f) => f.crc) };
  return { pack, folders, sub };
}

function readNames(data, n) {
  const names = [];
  let at = 0;
  for (let i = 0; i < n; i++) {
    let s = '';
    for (;;) {
      if (at + 1 >= data.length) fail('a name in it is cut short');
      const c = data[at] | (data[at + 1] << 8);
      at += 2;
      if (c === 0) break;
      s += String.fromCharCode(c);
      if (s.length > 4096) fail('a name in it is too long');
    }
    names.push(s);
  }
  return names;
}

function readFiles(r) {
  const n = r.count();
  const files = Array.from({ length: n }, () => ({ name: '', empty: false, emptyFile: false, dir: false }));
  let emptyCount = 0;
  for (;;) {
    const type = r.byte();
    if (type === K.End) break;
    const size = r.number();
    if (size > r.b.length - r.at) fail('its file info is damaged');
    const data = new Reader(r.bytes(size));
    switch (type) {
      case K.EmptyStream: {
        const bits = data.bits(n);
        bits.forEach((b, i) => (files[i].empty = b));
        emptyCount = bits.filter(Boolean).length;
        break;
      }
      case K.EmptyFile: {
        const bits = data.bits(emptyCount);
        let k = 0;
        for (const f of files) if (f.empty) f.emptyFile = bits[k++];
        break;
      }
      case K.Name:
        if (data.byte() !== 0) fail('it keeps its names somewhere else');
        readNames(data.b.subarray(1), n).forEach((s, i) => (files[i].name = s));
        break;
      case K.WinAttributes: {
        const defined = data.allOrBits(n);
        if (data.byte() !== 0) fail('it keeps its attributes somewhere else');
        defined.forEach((d, i) => {
          if (d && data.u32() & 0x10) files[i].dir = true;
        });
        break;
      }
      default:
        break; // times, padding and the like: nothing we show
    }
  }
  return files;
}

/** Undo the BCJ x86 filter in place (the LZMA SDK's x86_Convert, decoding). */
function bcjX86(data) {
  const allowed = [true, true, true, false, true, false, false, false];
  const bitNumber = [0, 1, 2, 2, 3, 3, 3, 3];
  const msb = (b) => b === 0 || b === 0xff;
  const size = data.length;
  if (size < 5) return data;
  let prevMask = 0;
  let prevPos = -1;
  let pos = 0;
  const limit = size - 4;
  while (pos < limit) {
    if ((data[pos] & 0xfe) !== 0xe8) {
      pos++;
      continue;
    }
    const gap = pos - prevPos;
    if (gap > 3) {
      prevMask = 0;
    } else {
      prevMask = (prevMask << (gap - 1)) & 7;
      if (prevMask !== 0) {
        const b = data[pos + 4 - bitNumber[prevMask]];
        if (!allowed[prevMask] || msb(b)) {
          prevPos = pos;
          prevMask = ((prevMask << 1) & 7) | 1;
          pos++;
          continue;
        }
      }
    }
    prevPos = pos;
    if (msb(data[pos + 4])) {
      let src = (data[pos + 1] | (data[pos + 2] << 8) | (data[pos + 3] << 16) | (data[pos + 4] << 24)) >>> 0;
      let dest;
      for (;;) {
        dest = (src - (pos + 5)) >>> 0;
        if (prevMask === 0) break;
        const index = bitNumber[prevMask] * 8;
        const b = (dest >>> (24 - index)) & 0xff;
        if (!msb(b)) break;
        src = (dest ^ (2 ** (32 - index) - 1)) >>> 0;
      }
      data[pos + 4] = (dest >>> 24) & 1 ? 0xff : 0;
      data[pos + 3] = (dest >>> 16) & 0xff;
      data[pos + 2] = (dest >>> 8) & 0xff;
      data[pos + 1] = dest & 0xff;
      pos += 5;
    } else {
      prevMask = ((prevMask << 1) & 7) | 1;
      pos++;
    }
  }
  return data;
}

/**
 * Undo BCJ2 (the LZMA SDK's Bcj2_Decode): four inputs, the main stream with
 * x86 call and jump targets taken out, the call targets, the jump targets,
 * and a range-coded stream saying which E8/E9/0F8x bytes were converted.
 */
function bcj2([main, call, jump, rc], size) {
  // Every output byte comes from the main stream or a call or jump target.
  if (size > main.length + call.length + jump.length) fail('a file in it is damaged');
  const out = new Uint8Array(size);
  const probs = new Uint16Array(258).fill(1024);
  let rcAt = 0;
  const rcByte = () => {
    if (rcAt >= rc.length) fail('a file in it is damaged');
    return rc[rcAt++];
  };
  let code = 0;
  let range = 0xffffffff;
  for (let i = 0; i < 5; i++) code = (code * 256 + rcByte()) % 2 ** 32;
  let callAt = 0;
  let jumpAt = 0;
  let inAt = 0;
  let outAt = 0;
  let prev = 0;
  const isJump = (b0, b1) => (b1 & 0xfe) === 0xe8 || (b0 === 0x0f && (b1 & 0xf0) === 0x80);
  while (outAt < size) {
    if (inAt >= main.length) fail('a file in it is cut short');
    const b = main[inAt++];
    out[outAt++] = b;
    if (!isJump(prev, b)) {
      prev = b;
      continue;
    }
    if (outAt === size) break;
    const i = b === 0xe8 ? prev : b === 0xe9 ? 256 : 257;
    const p = probs[i];
    const bound = (range >>> 11) * p;
    if (code < bound) {
      range = bound;
      probs[i] = p + ((2048 - p) >>> 5);
      prev = b;
    } else {
      range -= bound;
      code -= bound;
      probs[i] = p - (p >>> 5);
      const src = b === 0xe8 ? call : jump;
      let at = b === 0xe8 ? callAt : jumpAt;
      if (at + 4 > src.length) fail('a file in it is damaged');
      const addr = ((src[at] << 24) | (src[at + 1] << 16) | (src[at + 2] << 8) | src[at + 3]) >>> 0;
      at += 4;
      if (b === 0xe8) callAt = at;
      else jumpAt = at;
      const dest = (addr - (outAt + 4)) >>> 0;
      for (let k = 0; k < 4 && outAt < size; k++) out[outAt++] = (dest >>> (8 * k)) & 0xff;
      prev = dest >>> 24;
    }
    if (range < 0x1000000) {
      range *= 256;
      code = code * 256 + rcByte();
    }
  }
  return out;
}

function delta(props, data) {
  const dist = (props[0] ?? 0) + 1;
  for (let i = dist; i < data.length; i++) data[i] = (data[i] + data[i - dist]) & 0xff;
  return data;
}

async function inflateRaw(input, size) {
  const reader = new Blob([input]).stream().pipeThrough(new DecompressionStream('deflate-raw')).getReader();
  const parts = [];
  let n = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value.length > size - n) fail('a file in it unpacks to more than it says');
      parts.push(value);
      n += value.length;
    }
  } catch (e) {
    if (e instanceof SevenZipError) throw e;
    fail('a file in it is damaged');
  } finally {
    reader.cancel().catch(() => {});
  }
  if (n !== size) fail('a file in it is cut short');
  const out = new Uint8Array(size);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

/** An archive (or entry) we can't unpack; `reason` is `password` or `method`, for the view to say which. */
function unsupported(id) {
  const e = new SevenZipError(
    id === METHOD.AES
      ? "This archive is protected with a password, which the preview can't open."
      : "This archive uses a compression method the preview can't unpack.",
  );
  e.reason = id === METHOD.AES ? 'password' : 'method';
  throw e;
}

/** What one step of a block makes of its inputs. */
async function runCoder(coder, inputs, size) {
  if (coder.id === METHOD.BCJ2) {
    if (coder.ins !== 4 || coder.outs !== 1) fail('a block is wired wrong');
    return bcj2(inputs, size);
  }
  if (coder.ins !== 1 || coder.outs !== 1) unsupported(coder.id);
  const [input] = inputs;
  try {
    switch (coder.id) {
      case METHOD.COPY:
        if (input.length < size) fail('a file in it is cut short');
        return input.slice(0, size);
      case METHOD.LZMA:
        return decodeLzma(coder.props, input, size);
      case METHOD.LZMA2:
        return decodeLzma2(coder.props, input, size);
      case METHOD.DEFLATE:
        return await inflateRaw(input, size);
      case METHOD.BCJ:
        if (input.length !== size) fail('a filter changes the size');
        return bcjX86(input.slice());
      case METHOD.DELTA:
        if (input.length !== size) fail('a filter changes the size');
        return delta(coder.props, input.slice());
      default:
        return unsupported(coder.id);
    }
  } catch (e) {
    if (e instanceof LzmaError) fail('a file in it is damaged');
    throw e;
  }
}

/** Unpack a whole block from the archive's bytes. */
async function unpackFolder(bytes, f, packStart, packSizes, cap) {
  const total = unpackSize(f);
  if (total > cap) fail('it unpacks to more than this browser will hold');
  for (const s of f.sizes) if (s > cap) fail('it unpacks to more than this browser will hold');
  // Where each packed input of the block starts in the file.
  const packed = new Map();
  let at = packStart;
  f.packed.forEach((inIndex, i) => {
    const size = packSizes[i];
    if (size > bytes.length - at) fail('it is cut short');
    packed.set(inIndex, bytes.subarray(at, at + size));
    at += size;
  });
  const coderOf = (outIndex) => f.coders.find((c) => outIndex >= c.firstOut && outIndex < c.firstOut + c.outs);
  const decode = async (outIndex, depth) => {
    if (depth > MAX_CODERS) fail('a block is wired in a loop');
    const coder = coderOf(outIndex);
    if (!coder) fail('a block is wired wrong');
    if (coder.outs !== 1) unsupported(coder.id);
    const inputs = [];
    for (let k = 0; k < coder.ins; k++) {
      const inIndex = coder.firstIn + k;
      const bound = f.bindPairs.find((p) => p.inIndex === inIndex);
      const input = bound ? await decode(bound.outIndex, depth + 1) : packed.get(inIndex);
      if (!input) fail('a block is wired wrong');
      inputs.push(input);
    }
    return runCoder(coder, inputs, f.sizes[outIndex]);
  };
  return decode(mainOut(f), 0);
}

/** The header, decoding it first if it's packed (it usually is). */
async function readHeader(bytes, r) {
  let id = r.byte();
  while (id === K.EncodedHeader) {
    const info = readStreamsInfo(r);
    if (info.folders.length !== 1) fail('its header is damaged');
    const f = info.folders[0];
    const start = 32 + info.pack.packPos;
    if (!Number.isSafeInteger(start) || start > bytes.length) fail('it is cut short');
    const header = await unpackFolder(bytes, f, start, info.pack.sizes, MAX_HEADER);
    if (f.crc !== null && crc32(header) !== f.crc) fail('its header is damaged');
    r = new Reader(header);
    id = r.byte();
  }
  if (id !== K.Header) fail('its header is damaged');
  return r;
}

function skipProperties(r) {
  for (;;) {
    if (r.byte() === K.End) return;
    r.bytes(r.number());
  }
}

// A name as listed: `/` between folders (7-Zip on Windows may store `\`),
// no leading `/` or `./`, no empty or `.` segments.
function cleanName(name) {
  return name
    .replace(/\\/g, '/')
    .split('/')
    .filter((s) => s && s !== '.')
    .join('/');
}

/** Open a 7z held in memory. Same shape as openZip's result. Throws SevenZipError if it can't be read. */
export async function open7z(bytes) {
  if (!is7z(bytes)) fail("it isn't a 7z file");
  if (bytes[6] !== 0) fail('it is from a newer 7z format');
  const head = new Reader(bytes.subarray(8, 32));
  const startCrc = head.u32();
  if (crc32(bytes.subarray(12, 32)) !== startCrc) fail('its start is damaged');
  const offset = head.u32() + head.u32() * 2 ** 32;
  const size = head.u32() + head.u32() * 2 ** 32;
  const nextCrc = head.u32();

  const entries = new Map();
  let main = null;
  if (size > 0) {
    if (offset > bytes.length - 32 || size > bytes.length - 32 - offset) fail('it is cut short');
    const raw = bytes.subarray(32 + offset, 32 + offset + size);
    if (crc32(raw) !== nextCrc) fail('its header is damaged');
    const r = await readHeader(bytes, new Reader(raw));
    let id = r.byte();
    if (id === K.ArchiveProperties) {
      skipProperties(r);
      id = r.byte();
    }
    if (id === K.AdditionalStreamsInfo) {
      readStreamsInfo(r);
      id = r.byte();
    }
    if (id === K.MainStreamsInfo) {
      main = readStreamsInfo(r);
      id = r.byte();
    }
    let files = [];
    if (id === K.FilesInfo) {
      files = readFiles(r);
      id = r.byte();
    }
    if (id !== K.End) fail('its header is damaged');

    // Each file with content is the next stream of the blocks, in order.
    const streams = [];
    if (main) {
      let packIndex = 0;
      let packAt = 32 + main.pack.packPos;
      let k = 0;
      main.folders.forEach((f, fi) => {
        const sizes = main.pack.sizes.slice(packIndex, packIndex + f.packed.length);
        if (sizes.length !== f.packed.length) fail('its blocks and packed streams disagree');
        const folder = { f, start: packAt, sizes };
        packIndex += f.packed.length;
        packAt += sizes.reduce((a, b) => a + b, 0);
        let within = 0;
        for (let j = 0; j < main.sub.counts[fi]; j++, k++) {
          streams.push({ folder, offset: within, size: main.sub.sizes[k], crc: main.sub.digests[k] ?? null });
          within += main.sub.sizes[k];
        }
      });
    }
    let next = 0;
    for (const file of files) {
      let entry;
      if (file.empty) {
        if (!file.emptyFile && !file.dir) continue; // a folder
        entry = { size: 0 };
      } else {
        if (next >= streams.length) fail('it lists more files than it holds');
        entry = streams[next++];
      }
      if (file.dir) continue;
      const name = cleanName(file.name);
      if (!name) continue;
      if (!entries.has(name) && entries.size >= MAX_ENTRIES) fail('it has too many files in it');
      entries.set(name, entry);
    }
  }

  // The last block unpacked, since a solid archive keeps many files in one.
  let cached = null;
  const block = async (folder) => {
    if (cached?.folder !== folder) {
      cached = null;
      cached = { folder, data: await unpackFolder(bytes, folder.f, folder.start, folder.sizes, MAX_UNPACK) };
    }
    return cached.data;
  };

  return {
    names: () => [...entries.keys()],
    has: (n) => entries.has(n),
    /** An entry's bytes (a copy). Throws SevenZipError if missing, damaged or in a method we can't unpack. */
    async read(name) {
      const e = entries.get(name);
      if (!e) fail(`it has no ${name}`);
      if (e.size === 0) return new Uint8Array(0);
      const data = await block(e.folder);
      if (e.offset + e.size > data.length) fail('a file in it is cut short');
      const out = data.slice(e.offset, e.offset + e.size);
      if (e.crc !== null && crc32(out) !== e.crc) fail('a file in it is damaged');
      return out;
    },
    /** Every entry's name and size, for listing without reading them. */
    list: () => [...entries].map(([name, e]) => ({ name, size: e.size })),
  };
}
