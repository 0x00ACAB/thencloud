// Audiobook chapters (the Nero chpl box) from files that may be shared with us.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readChapters } from '../src/lib/chapters.js';
import { random, mutate, timed, concat, be32, box, RUNS, SEED } from './fuzz.js';

const time = (s) => [...be32(Math.floor((s * 1e7) / 2 ** 32)), ...be32((s * 1e7) % 2 ** 32)];
const chpl = (chapters) =>
  box('chpl', [1, 0, 0, 0], be32(0), [chapters.length], ...chapters.flatMap(([s, t]) => [time(s), [t.length], t]));
// Audio first and moov last, as audiobooks often are.
const book = () => concat(box('ftyp', 'M4B ', be32(0)), box('mdat', new Uint8Array(3000)), box('moov', box('udta', chpl([[0, 'One'], [30.5, 'Two'], [75, 'Three']]))));

const fileOf = (bytes, chunkSize) => ({
  size: bytes.length,
  chunkSize,
  count: Math.max(1, Math.ceil(bytes.length / chunkSize)),
  read: async (i) => bytes.subarray(i * chunkSize, (i + 1) * chunkSize),
});

test('chapters are read from a moov at the end', async () => {
  const c = await readChapters(fileOf(book(), 1024));
  assert.deepEqual(c, [
    { start: 0, title: 'One' },
    { start: 30.5, title: 'Two' },
    { start: 75, title: 'Three' },
  ]);
});

test(`chapters: mutated input never throws or hangs (seed ${SEED})`, async () => {
  const rnd = random(SEED + 8);
  const seed = book();
  for (let i = 0; i < RUNS; i++) {
    const input = mutate(seed, rnd);
    const c = await timed(() => readChapters(fileOf(input, 256 + rnd.int(2048))));
    assert.ok(Array.isArray(c));
    for (const x of c) {
      assert.ok(Number.isFinite(x.start) && x.start >= 0);
      assert.equal(typeof x.title, 'string');
    }
  }
});
