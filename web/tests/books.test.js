// Books are zips from someone else: the reader must never throw anything
// but a ZipError, never inflate past its limits, and never let a path out
// of the book.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { deflateRawSync } from 'node:zlib';
import { openZip, ZipError, MAX_ENTRY } from '../src/lib/unzip.js';
import { openCbz, resolvePath, imageType } from '../src/lib/books.js';
import { random, mutate, RUNS, SEED } from './fuzz.js';

const enc = new TextEncoder();

/** A zip of `files` ({ name: bytes | string }), deflated where `deflate` says; `sizes` can lie about sizes. */
function zip(files, { deflate = true, sizes = {} } = {}) {
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

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13]);

test('zip: stored and deflated files read back', async () => {
  for (const deflate of [false, true]) {
    const z = openZip(zip({ 'a.txt': 'hello', 'dir/b.txt': 'world'.repeat(1000) }, { deflate }));
    assert.deepEqual(z.names().sort(), ['a.txt', 'dir/b.txt']);
    assert.equal(new TextDecoder().decode(await z.read('a.txt')), 'hello');
    assert.equal((await z.read('dir/b.txt')).length, 5000);
    await assert.rejects(z.read('missing'), ZipError);
  }
});

test('zip: a file that inflates past what it claims is refused', async () => {
  // A small zip bomb: 20 MB of zeros that says it's 1 KB.
  const z = openZip(zip({ 'bomb.bin': new Uint8Array(20 * 1024 * 1024) }, { sizes: { 'bomb.bin': 1024 } }));
  await assert.rejects(z.read('bomb.bin'), ZipError);
  const big = openZip(zip({ 'big.bin': 'x' }, { sizes: { 'big.bin': MAX_ENTRY + 1 } }));
  await assert.rejects(big.read('big.bin'), ZipError);
});

test('books: paths stay inside the book, and pages go in natural order', () => {
  assert.equal(resolvePath('OEBPS/text/ch1.xhtml', '../images/a%20b.png#x'), 'OEBPS/images/a b.png');
  assert.equal(resolvePath('ch1.xhtml', '../../etc/passwd'), null);
  assert.equal(resolvePath('a/b.xhtml', '/c.png'), 'c.png');
  const cbz = openCbz(zip({ 'page10.png': PNG, 'page2.png': PNG, 'page1.png': PNG, '__MACOSX/._page1.png': PNG, 'notes.txt': 'x' }));
  assert.equal(cbz.count, 3);
  assert.equal(imageType(PNG), 'image/png');
  assert.equal(imageType(enc.encode('<svg onload=alert(1)>')), null);
  assert.throws(() => openCbz(zip({ 'readme.txt': 'no pages' })), ZipError);
});

test(`zip: mutated input only ever fails with a ZipError (seed ${SEED})`, async () => {
  const rnd = random(SEED + 11);
  const good = zip({ 'a.png': PNG, 'b/c.txt': 'text '.repeat(50), 'd.png': PNG });
  for (let i = 0; i < Math.min(RUNS, 2000); i++) {
    const bytes = mutate(good, rnd);
    let z;
    try {
      z = openZip(bytes);
    } catch (e) {
      assert.ok(e instanceof ZipError, String(e));
      continue;
    }
    for (const name of z.names()) {
      try {
        const out = await z.read(name);
        assert.ok(out.length <= MAX_ENTRY);
      } catch (e) {
        assert.ok(e instanceof ZipError, String(e));
      }
    }
  }
});
