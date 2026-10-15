// 7z files for archive browsing come from someone else: the reader must
// never throw anything but a SevenZipError, never read past the bytes it
// holds, and never unpack past its cap. The fixtures are real 7-Zip
// archives (tests/fixtures/7z/make7z.sh) of the files in sevenzfiles.js.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { open7z, is7z, crc32, SevenZipError } from '../src/lib/un7z.js';
import { decodeLzma, decodeLzma2, LzmaError } from '../src/lib/lzma.js';
import { FILES } from './sevenzfiles.js';
import { random, mutate, RUNS, SEED, timed } from './fuzz.js';
import { tar } from './tarfile.js';
import { zip } from './zipfile.js';

const dir = new URL('./fixtures/7z/', import.meta.url);
const fixture = (name) => new Uint8Array(readFileSync(new URL(name, dir)));
const all = readdirSync(dir).filter((n) => n.endsWith('.7z'));
const unpackable = ['lzma2.7z', 'lzma-blocks.7z', 'stored.7z', 'deflate.7z', 'bcj.7z', 'bcj2.7z', 'delta.7z'];
const sorted = (list) => [...list].sort((a, b) => a.name.localeCompare(b.name));
const expected = sorted(Object.entries(FILES).map(([name, b]) => ({ name, size: b.length })));

test('7z: every method we unpack gives back the same files', async () => {
  for (const name of unpackable) {
    const a = await open7z(fixture(name));
    assert.deepEqual(sorted(a.list()), expected, name);
    for (const [file, bytes] of Object.entries(FILES)) {
      assert.deepEqual(await a.read(file), bytes, `${name}: ${file}`);
    }
    await assert.rejects(a.read('missing'), SevenZipError);
  }
});

test('7z: other methods and passwords still list, and say why they cannot be read', async () => {
  const ppmd = await open7z(fixture('ppmd.7z'));
  assert.deepEqual(sorted(ppmd.list()), expected);
  assert.equal((await ppmd.read('empty.txt')).length, 0);
  await assert.rejects(ppmd.read('a.txt'), (e) => e instanceof SevenZipError && e.reason === 'method');

  const locked = await open7z(fixture('password.7z'));
  assert.deepEqual(sorted(locked.list()), expected);
  await assert.rejects(locked.read('dir/b.txt'), (e) => e instanceof SevenZipError && e.reason === 'password');

  await assert.rejects(open7z(fixture('password-names.7z')), (e) => e instanceof SevenZipError && e.reason === 'password');
});

test('7z: telling 7z, zip and tar apart', () => {
  assert.equal(is7z(fixture('lzma2.7z')), true);
  assert.equal(is7z(zip({ 'a.txt': 'hello' })), false);
  assert.equal(is7z(tar({ 'a.txt': 'hello' })), false);
  assert.equal(is7z(fixture('lzma2.7z').subarray(0, 20)), false);
});

test('7z: a damaged header or file is refused', async () => {
  const bad = fixture('stored.7z');
  bad[bad.length - 5] ^= 1; // in the header, after the stored files
  await assert.rejects(open7z(bad), SevenZipError);

  // A stored file's bytes changed: the listing opens, the file's CRC doesn't match.
  const changed = fixture('stored.7z');
  changed[40] ^= 1;
  const a = await open7z(changed);
  const results = await Promise.allSettled(a.list().map(({ name }) => a.read(name)));
  assert.ok(results.some((r) => r.status === 'rejected' && r.reason instanceof SevenZipError));
});

// With the CRCs put right after mutating, the damage reaches the header
// parser and the decoders instead of stopping at the first check.
function fixCrcs(bytes) {
  if (bytes.length < 32) return bytes;
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const offset = v.getUint32(12, true) + v.getUint32(16, true) * 2 ** 32;
  const size = v.getUint32(20, true) + v.getUint32(24, true) * 2 ** 32;
  if (32 + offset + size <= bytes.length) v.setUint32(28, crc32(bytes.subarray(32 + offset, 32 + offset + size)), true);
  v.setUint32(8, crc32(bytes.subarray(12, 32)), true);
  return bytes;
}

test(`7z: mutated input only ever fails with a SevenZipError (seed ${SEED})`, async () => {
  const rnd = random(SEED);
  const samples = all.map(fixture);
  for (let i = 0; i < RUNS; i++) {
    let bytes = mutate(rnd.pick(samples), rnd);
    if (rnd.int(2)) bytes = fixCrcs(bytes);
    try {
      await timed(async () => {
        if (!is7z(bytes)) return;
        const a = await open7z(bytes);
        for (const { name, size } of a.list()) {
          try {
            assert.equal((await a.read(name)).length, size);
          } catch (e) {
            if (!(e instanceof SevenZipError)) throw e;
          }
        }
      }, 1000);
    } catch (e) {
      if (!(e instanceof SevenZipError)) throw new Error(`run ${i}: ${e?.stack ?? e}`);
    }
  }
});

test(`7z: the LZMA decoders only ever fail with an LzmaError (seed ${SEED})`, async () => {
  const rnd = random(SEED + 1);
  const bytes = (n) => Uint8Array.from({ length: n }, () => rnd.int(256));
  for (let i = 0; i < RUNS; i++) {
    const input = bytes(rnd.int(300));
    if (rnd.int(2) === 0) input[0] = 0; // past the range coder's first check
    const size = rnd.pick([0, 1, 100, 5000, 1 << 20]);
    for (const run of [
      () => decodeLzma([rnd.int(256), 0, 0, 1, 0], input, size),
      () => decodeLzma2([rnd.int(41)], Uint8Array.from([rnd.pick([1, 0x80, 0xe0, 0xc0]), ...input]), size),
    ]) {
      try {
        await timed(run);
      } catch (e) {
        if (!(e instanceof LzmaError)) throw new Error(`run ${i}: ${e?.stack ?? e}`);
      }
    }
  }
});
