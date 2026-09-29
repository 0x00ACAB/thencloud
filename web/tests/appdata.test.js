// App data padding: still the same JSON, sizes fall into buckets, and never
// past what the server takes. Skipped until ../build.sh has built src/wasm.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { padJson } from '../src/lib/appdata.js';

const wasmDir = new URL('../src/wasm/', import.meta.url);
const built = existsSync(new URL('thencloud_wasm_bg.wasm', wasmDir));
let tc;
if (built) {
  tc = await import(new URL('thencloud_wasm.js', wasmDir).href);
  tc.initSync({ module: readFileSync(new URL('thencloud_wasm_bg.wasm', wasmDir)) });
}
const skip = built ? false : 'the WASM build is missing; run ../build.sh';
const dec = new TextDecoder();

test('app data: padded JSON reads back the same, in few distinct sizes', { skip }, () => {
  const sizes = new Set();
  for (let n = 0; n < 3000; n += 7) {
    const data = { words: 'x'.repeat(n), n, pl: 'zażółć' };
    const json = JSON.stringify(data);
    const padded = padJson(json, 'music', tc.padded_size);
    assert.deepEqual(JSON.parse(dec.decode(padded)), data);
    assert.ok(padded.length >= new TextEncoder().encode(json).length);
    assert.equal(padded.length, tc.padded_size(new TextEncoder().encode(json).length), 'the same buckets as files');
    sizes.add(padded.length);
  }
  assert.ok(sizes.size * 5 < 429, `${sizes.size} distinct sizes for 429 inputs`);
});

test('app data: never padded past the server limit', { skip }, () => {
  const near = 'x'.repeat(2 * 1024 * 1024 - 200);
  const padded = padJson(JSON.stringify(near), 'music', tc.padded_size);
  assert.ok(padded.length <= 2 * 1024 * 1024 - 64);
  const search = padJson(JSON.stringify('y'.repeat(9 * 1024 * 1024)), 'search', tc.padded_size);
  assert.ok(search.length > 9 * 1024 * 1024 && search.length <= 12 * 1024 * 1024 - 64);
});
