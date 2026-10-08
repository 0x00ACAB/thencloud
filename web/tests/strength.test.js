// The password meter's estimate: common passwords and ones built from the
// username score low, long random ones score high.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { passwordScore } from '../src/lib/strength.js';

test('strength: common passwords are weak', async () => {
  for (const p of ['password1', 'qwertyuiop', '1234567890', 'iloveyou2024']) {
    assert.ok((await passwordScore(p)) <= 1, p);
  }
});

test('strength: random passwords are very strong', async () => {
  assert.equal(await passwordScore('ibDc5QTIrlPsqzEr/CvfDvEpz0CR5NQHFPSGvTAzTjo='), 4);
  assert.equal(await passwordScore('correct glacier pebble anthem'), 4);
});

test('strength: leaning on the username counts against it', async () => {
  const p = 'margaretthencloud';
  assert.ok((await passwordScore(p, ['margaret', 'thencloud'])) < (await passwordScore(p)));
});
