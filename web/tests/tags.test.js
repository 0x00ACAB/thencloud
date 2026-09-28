// Audio tags (ID3v2, FLAC, MP4) and video tags (MP4, Matroska) from files
// that may have been shared with us.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseTags } from '../src/lib/tags.js';
import { readVideoTags } from '../src/lib/videotags.js';
import { random, mutate, timed, concat, be32, le32, box, RUNS, SEED } from './fuzz.js';

const id3 = () => {
  const frame = concat('TIT2', be32(6), [0, 0], [0], 'Title');
  return concat('ID3', [3, 0, 0], [0, 0, 0, frame.length], frame);
};
const flac = () => {
  const comment = concat(le32(3), 'abc', le32(1), le32(9), 'TITLE=Hey');
  return concat('fLaC', [0x84, 0, 0, comment.length], comment);
};
const m4a = () =>
  concat(
    box('ftyp', 'M4A ', be32(0)),
    box('moov', box('udta', box('meta', [0, 0, 0, 0], box('ilst', box('©nam', box('data', be32(1), be32(0), 'Song')))))),
  );

const fileOf = (bytes, chunkSize = 64) => ({
  count: Math.max(1, Math.ceil(bytes.length / chunkSize)),
  chunkSize,
  size: bytes.length,
  read: async (i) => bytes.subarray(i * chunkSize, (i + 1) * chunkSize),
});

function checkTags(t) {
  if (t === null) return;
  assert.equal(typeof t, 'object');
  for (const k of ['title', 'artist', 'album']) if (t[k] !== undefined) assert.equal(typeof t[k], 'string', k);
  if (t.track !== undefined) assert.ok(Number.isInteger(t.track));
  // A cover is only ever a known image type, decided from its bytes.
  if (t.picture !== undefined) assert.match(t.picture.type, /^image\/(jpeg|png|gif|webp)$/);
}

test('audio tags: the samples parse', () => {
  assert.equal(parseTags(id3()).title, 'Title');
  assert.equal(parseTags(flac()).title, 'Hey');
  assert.equal(parseTags(m4a()).title, 'Song');
});

test(`audio tags: mutated input never throws or hangs (seed ${SEED})`, async () => {
  const rnd = random();
  const seeds = [id3(), flac(), m4a()];
  for (let i = 0; i < RUNS; i++) {
    const input = mutate(rnd.pick(seeds), rnd);
    checkTags(await timed(() => parseTags(input)));
  }
});

test(`video tags: mutated input never throws or hangs (seed ${SEED})`, async () => {
  const rnd = random(SEED + 1);
  const mp4 = concat(box('ftyp', 'isom', be32(0)), box('moov', box('udta', box('meta', [0, 0, 0, 0], box('ilst', box('tvsh', box('data', be32(1), be32(0), 'Show')))))));
  const mkv = concat([0x1a, 0x45, 0xdf, 0xa3, 0x84, 0x42, 0x86, 0x81, 0x01], [0x18, 0x53, 0x80, 0x67, 0x88, 0x15, 0x49, 0xa9, 0x66, 0x84, 0x7b, 0xa9, 0x81, 0x41]);
  for (let i = 0; i < RUNS; i++) {
    const input = mutate(rnd.pick([mp4, mkv]), rnd);
    const file = fileOf(input, 1 + rnd.int(256));
    const t = await timed(() => readVideoTags(file, input.subarray(0, file.chunkSize)));
    if (t !== null) {
      assert.equal(typeof t, 'object');
      for (const k of ['series', 'title']) if (t[k] !== undefined) assert.equal(typeof t[k], 'string');
    }
  }
});
