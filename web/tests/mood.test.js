// The mood meter's grid: every cell's centre maps back to that cell, the
// corners have the right colours, and any point lands in some cell.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WORDS, SIZE, cellPoint, pointCell, quadrant, wordAt } from '../src/lib/mood.js';

test('mood: cells and points map both ways', () => {
  assert.equal(WORDS.length, SIZE);
  assert.equal(new Set(WORDS.flat()).size, SIZE * SIZE, 'every word once');
  for (let r = 0; r < SIZE; r++) {
    for (let c = 0; c < SIZE; c++) {
      const { x, y } = cellPoint(r, c);
      assert.deepEqual(pointCell(x, y), { row: r, col: c });
      assert.equal(wordAt(x, y), WORDS[r][c]);
    }
  }
});

test('mood: corners', () => {
  assert.equal(quadrant(-1, 1), 'red');
  assert.equal(quadrant(1, 1), 'yellow');
  assert.equal(quadrant(-1, -1), 'blue');
  assert.equal(quadrant(1, -1), 'green');
  assert.equal(wordAt(-1, 1), 'Furious');
  assert.equal(wordAt(1, -1), 'Serene');
});

test('mood: any point, even out of range, lands in a cell', () => {
  for (const [x, y] of [[-5, 5], [5, -5], [0, 0], [NaN, 0.5], [1, 1], [-1, -1]]) {
    const { row, col } = pointCell(x, y);
    assert.ok(row >= 0 && row < SIZE && col >= 0 && col < SIZE, `${x},${y}`);
  }
});
