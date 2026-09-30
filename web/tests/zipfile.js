// Building zip files for the tests (books, office documents).
import { deflateRawSync } from 'node:zlib';

const enc = new TextEncoder();

/** A zip of `files` ({ name: bytes | string }), deflated where `deflate` says; `sizes` can lie about sizes. */
export function zip(files, { deflate = true, sizes = {} } = {}) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const [name, content] of Object.entries(files)) {
    const data = typeof content === 'string' ? enc.encode(content) : content;
    const packed = deflate ? deflateRawSync(data) : data;
    const n = enc.encode(name);
    const head = new Uint8Array(30 + n.length);
    const hv = new DataView(head.buffer);
    hv.setUint32(0, 0x04034b50, true);
    hv.setUint16(6, 0x800, true);
    hv.setUint16(8, deflate ? 8 : 0, true);
    hv.setUint32(18, packed.length, true);
    hv.setUint32(22, sizes[name] ?? data.length, true);
    hv.setUint16(26, n.length, true);
    head.set(n, 30);
    const cd = new Uint8Array(46 + n.length);
    const cv = new DataView(cd.buffer);
    cv.setUint32(0, 0x02014b50, true);
    cv.setUint16(8, 0x800, true);
    cv.setUint16(10, deflate ? 8 : 0, true);
    cv.setUint32(20, packed.length, true);
    cv.setUint32(24, sizes[name] ?? data.length, true);
    cv.setUint16(28, n.length, true);
    cv.setUint32(42, offset, true);
    cd.set(n, 46);
    locals.push(head, packed);
    central.push(cd);
    offset += head.length + packed.length;
  }
  const cdSize = central.reduce((s, c) => s + c.length, 0);
  const end = new Uint8Array(22);
  const ev = new DataView(end.buffer);
  ev.setUint32(0, 0x06054b50, true);
  ev.setUint16(8, central.length, true);
  ev.setUint16(10, central.length, true);
  ev.setUint32(12, cdSize, true);
  ev.setUint32(16, offset, true);
  const parts = [...locals, ...central, end];
  const out = new Uint8Array(parts.reduce((s, p) => s + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}
