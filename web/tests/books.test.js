// Books are zips from someone else: the reader must never throw anything
// but a ZipError, never inflate past its limits, and never let a path out
// of the book.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { openZip, ZipError, MAX_ENTRY } from '../src/lib/unzip.js';
import { openCbz, resolvePath, imageType } from '../src/lib/books.js';
import { random, mutate, RUNS, SEED } from './fuzz.js';
import { zip } from './zipfile.js';

const enc = new TextEncoder();

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
