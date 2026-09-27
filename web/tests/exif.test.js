// Location and camera details in photos.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { photoInfo, hasDetails } from '../src/lib/exif.js';
import { random, mutate, timed, concat, be32, le32, RUNS, SEED } from './fuzz.js';

const TIFF = () =>
  concat('MM', [0, 42], be32(8), [0, 3],
    [0x01, 0x0f, 0, 2], be32(4), 'Acm\0',
    [0x01, 0x12, 0, 3], be32(1), [0, 6, 0, 0],
    [0x88, 0x25, 0, 4], be32(1), be32(0), be32(0));

// PNG: IHDR, eXIf, a text chunk, image data (CRCs aren't checked here).
function png() {
  const chunk = (type, body) => concat(be32(body.length), type, body, be32(0));
  return concat([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], chunk('IHDR', new Uint8Array(13)), chunk('eXIf', TIFF()), chunk('tEXt', 'Comment\0home'), chunk('IDAT', [1, 2, 3]), chunk('IEND', []));
}

// WebP: VP8X (with the EXIF flag), image data, EXIF and XMP chunks.
function webp() {
  const chunk = (type, body) => {
    const b = concat(body);
    return concat(type, le32(b.length), b, b.length & 1 ? [0] : []);
  };
  const body = concat('WEBP', chunk('VP8X', [0x0c, 0, 0, 0, 0, 0, 0, 0, 0, 0]), chunk('VP8 ', [1, 2, 3, 4]), chunk('EXIF', TIFF()), chunk('XMP ', '<x/>'));
  return concat('RIFF', le32(body.length), body);
}

// A JPEG with EXIF (make, orientation 6, GPS pointer) and XMP; no pixels
// needed for the metadata code.
function jpeg() {
  const app1 = concat('Exif\0\0', TIFF());
  const xmp = concat('http://ns.adobe.com/xap/1.0/\0', '<x/>');
  const seg = (m, body) => concat([0xff, m, (body.length + 2) >> 8, (body.length + 2) & 255], body);
  return concat([0xff, 0xd8], seg(0xe0, concat('JFIF\0', [1, 1, 0, 0, 1, 0, 1, 0, 0])), seg(0xe1, app1), seg(0xe1, xmp), [0xff, 0xda, 0, 2], [1, 2, 3], [0xff, 0xd9]);
}

test('JPEG, PNG and WebP with a location are found and cleaned, keeping the orientation', () => {
  for (const [name, bytes] of [['jpeg', jpeg()], ['png', png()], ['webp', webp()]]) {
    const info = photoInfo(bytes);
    assert.ok(info.gps && info.camera && info.other, name);
    const clean = photoInfo(info.strip());
    assert.ok(!hasDetails(clean), name);
    assert.equal(clean.orientation, 6, name);
  }
});

test(`photos: mutated input never throws, and cleaning always cleans (seed ${SEED})`, async () => {
  const rnd = random(SEED + 2);
  const seeds = [jpeg(), png(), webp()];
  // Real files from the dev machine, when present, make better seeds.
  for (const f of ['p.jpg', 'p.png', 'p.webp']) {
    const path = `${process.env.FUZZ_SAMPLES ?? '/nonexistent'}/${f}`;
    if (existsSync(path)) seeds.push(new Uint8Array(readFileSync(path)));
  }
  for (let i = 0; i < RUNS; i++) {
    const input = mutate(rnd.pick(seeds), rnd);
    const info = await timed(() => photoInfo(input));
    if (!info) continue;
    const out = await timed(() => info.strip());
    assert.ok(out instanceof Uint8Array);
    const again = photoInfo(out);
    // What comes out must parse and have nothing left to remove.
    if (again) assert.ok(!hasDetails(again), 'details left after cleaning');
  }
});
