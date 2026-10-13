import test from 'node:test';
import assert from 'node:assert/strict';
import { canRefresh, chooseFileSnapshot, sameHashes, sha256Hex } from '../src/lib/file-cache.js';

const cached = { enc_key: 'wrapped-node-key', version: { id: 'old' } };
const current = { enc_key: 'wrapped-node-key', version: { id: 'new' } };

test('cache refresh is limited to connected, unmetered networks without data saver', () => {
  assert.equal(canRefresh({ connected: true, metered: false, dataSaver: false }), true);
  assert.equal(canRefresh({ connected: false, metered: false, dataSaver: false }), false);
  assert.equal(canRefresh({ connected: true, metered: true, dataSaver: false }), false);
  assert.equal(canRefresh({ connected: true, metered: false, dataSaver: true }), false);
});

test('cache selection keeps the prior version on metered and data-saver connections', () => {
  assert.equal(chooseFileSnapshot(cached, current, false), 'cached-stale');
  assert.equal(chooseFileSnapshot(cached, current, true), 'current');
  assert.equal(chooseFileSnapshot(cached, { ...cached }, false), 'current');
  assert.equal(chooseFileSnapshot(cached, null, false), 'cached-unverified');
  assert.equal(chooseFileSnapshot(null, null, false), 'unavailable');
});

test('a changed wrapped node key or ciphertext fingerprint is not accepted as the same cache version', () => {
  assert.equal(chooseFileSnapshot(cached, { ...cached, enc_key: 'different' }, true), 'invalid');
  assert.equal(sameHashes(['a', 'b'], ['a', 'b']), true);
  assert.equal(sameHashes(['a', 'b'], ['a', 'c']), false);
  assert.equal(sameHashes(['a'], ['a', 'b']), false);
});

test('ciphertext fingerprints use SHA-256', async () => {
  const bytes = new TextEncoder().encode('abc');
  assert.equal(await sha256Hex(bytes), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
});
