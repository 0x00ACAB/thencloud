// Which track gets ready next, and how much of it is decrypted ahead.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { upcomingIndex, piecesAhead, PRECACHE_WHOLE, PRECACHE_START } from '../src/lib/precache.js';

test('precache: the next track in the queue', () => {
  assert.equal(upcomingIndex(5, 0, 'off'), 1);
  assert.equal(upcomingIndex(5, 3, 'off'), 4);
  assert.equal(upcomingIndex(5, 4, 'off'), -1, 'the end of the queue');
  assert.equal(upcomingIndex(5, 4, 'all'), 0, 'repeat-all wraps round');
  assert.equal(upcomingIndex(1, 0, 'all'), -1, 'a queue of one repeats itself, nothing to get ready');
  assert.equal(upcomingIndex(5, 2, 'one'), -1, 'repeat-one plays the same track again');
  assert.equal(upcomingIndex(0, -1, 'off'), -1);
  assert.equal(upcomingIndex(3, 7, 'off'), -1);
});

test('precache: how much of it to decrypt', () => {
  const MB = 1024 * 1024;
  assert.equal(piecesAhead({ size: 9 * MB, count: 3 }), 3, 'a whole song');
  assert.equal(piecesAhead({ size: PRECACHE_WHOLE, count: 12 }), 12);
  assert.equal(piecesAhead({ size: PRECACHE_WHOLE + 1, count: 13 }), PRECACHE_START, 'an audiobook: enough to start');
  assert.equal(piecesAhead({ size: 900 * MB, count: 225 }), PRECACHE_START);
  assert.equal(piecesAhead({ size: 9 * MB, count: 3 }, { saveData: true }), 1, 'Data Saver: the first piece only');
  assert.equal(piecesAhead({ size: 0, count: 0 }), 0);
});
