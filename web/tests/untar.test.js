// Tar files for archive browsing come from someone else: the reader must
// never throw anything but a TarError, never read past the bytes it holds,
// and a gzipped one never inflates past its cap.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { gzipSync } from 'node:zlib';
import { openTar, isTar, isGzip, gunzip, TarError } from '../src/lib/untar.js';
import { random, mutate, RUNS, SEED, timed } from './fuzz.js';
import { tar } from './tarfile.js';
import { zip } from './zipfile.js';

const text = (b) => new TextDecoder().decode(b);
const sorted = (list) => [...list].sort((a, b) => a.name.localeCompare(b.name));

test('tar: files read back, with folders, long names and pax paths', async () => {
  const long = `${'deep/'.repeat(40)}file.txt`;
  for (const mode of [null, 'gnu', 'pax']) {
    const files = mode ? { [long]: 'far down', 'a.txt': 'hello' } : { 'a.txt': 'hello', 'dir/b.txt': 'world'.repeat(300) };
    const t = openTar(tar(files, { long: mode }));
    assert.deepEqual(t.names().sort(), Object.keys(files).sort());
    for (const [name, content] of Object.entries(files)) assert.equal(text(await t.read(name)), content);
    await assert.rejects(t.read('missing'), TarError);
  }
  const t = openTar(tar({ 'a.txt': 'hello', 'dir/b.txt': 'x'.repeat(1000) }));
  assert.deepEqual(sorted(t.list()), [
    { name: 'a.txt', size: 5 },
    { name: 'dir/b.txt', size: 1000 },
  ]);
});

test('tar: folders, links and odd names are left out or tidied', () => {
  const t = openTar(tar({ 'dir/': '', 'link': '', './x/./y.txt': 'y', '/abs.txt': 'a', '': 'nothing' }, { types: { 'dir/': '5', link: '2' } }));
  assert.deepEqual(t.names().sort(), ['abs.txt', 'x/y.txt']);
});

test('tar: a size past the end is refused, and so is a bad checksum', () => {
  assert.throws(() => openTar(tar({ 'a.txt': 'hi' }, { sizes: { 'a.txt': 10_000 } })), TarError);
  const bad = tar({ 'a.txt': 'hi' });
  bad[0] ^= 1;
  assert.throws(() => openTar(bad), TarError);
  assert.equal(isTar(bad), false);
});

test('tar: telling tar, gzip and zip apart', async () => {
  const plain = tar({ 'a.txt': 'hello' });
  assert.equal(isTar(plain), true);
  assert.equal(isTar(zip({ 'a.txt': 'hello' })), false);
  assert.equal(isTar(new Uint8Array(1024)), false, 'all zeros is no tar');
  const gz = gzipSync(plain);
  assert.equal(isGzip(gz), true);
  assert.equal(isGzip(plain), false);
  const back = await gunzip(gz);
  assert.deepEqual(openTar(back).names(), ['a.txt']);
});

test('tar: a gzip bomb stops at the cap', async () => {
  const bomb = gzipSync(new Uint8Array(8 * 1024 * 1024));
  await assert.rejects(gunzip(bomb, 1024 * 1024), TarError);
  await assert.rejects(gunzip(Uint8Array.from([0x1f, 0x8b, 8, 0, 1, 2, 3])), TarError);
});

test(`tar: mutated input only ever fails with a TarError (seed ${SEED})`, async () => {
  const rnd = random(SEED);
  const samples = [
    tar({ 'a.txt': 'hello', 'dir/b.txt': 'world'.repeat(200) }),
    tar({ [`${'z'.repeat(120)}/c.txt`]: 'long' }, { long: 'gnu' }),
    tar({ [`${'p'.repeat(120)}/d.txt`]: 'pax' }, { long: 'pax' }),
  ];
  for (let i = 0; i < RUNS; i++) {
    const bytes = mutate(rnd.pick(samples), rnd);
    try {
      await timed(async () => {
        if (!isTar(bytes)) return;
        const t = openTar(bytes);
        for (const { name, size } of t.list()) {
          assert.ok(size <= bytes.length, 'an entry is never bigger than the archive');
          assert.equal((await t.read(name)).length, size);
        }
      });
    } catch (e) {
      if (!(e instanceof TarError)) throw new Error(`run ${i}: ${e?.stack ?? e}`);
    }
  }
});
