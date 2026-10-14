// The storage analyzer's treemap: tiles fill the rectangle without
// overlapping, each in proportion to its value, and stay roughly square.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { squarify, topItems } from '../src/lib/treemap.js';

const RUNS = Number(process.env.FUZZ_RUNS || 300);
let seed = Number(process.env.FUZZ_SEED || 1);
const rand = () => ((seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31);

const EPS = 1e-6;

function check(values, rect) {
  const items = values.map((value, i) => ({ value, i }));
  const tiles = squarify(items, rect);
  const positive = values.filter((v) => v > 0);
  assert.equal(tiles.length, positive.length, 'one tile per positive value');
  const total = positive.reduce((a, b) => a + b, 0);
  let area = 0;
  for (const t of tiles) {
    assert.ok(t.w >= -EPS && t.h >= -EPS, 'no negative sides');
    assert.ok(t.x >= rect.x - EPS && t.y >= rect.y - EPS, 'inside, top left');
    assert.ok(t.x + t.w <= rect.x + rect.w + 1e-4 && t.y + t.h <= rect.y + rect.h + 1e-4, 'inside, bottom right');
    const want = (t.item.value / total) * rect.w * rect.h;
    assert.ok(Math.abs(t.w * t.h - want) <= Math.max(1e-3, want * 1e-3), `area in proportion (${t.w * t.h} vs ${want})`);
    area += t.w * t.h;
  }
  assert.ok(Math.abs(area - rect.w * rect.h) <= rect.w * rect.h * 1e-6, 'fills the rectangle');
  for (let a = 0; a < tiles.length; a++) {
    for (let b = a + 1; b < tiles.length; b++) {
      const p = tiles[a];
      const q = tiles[b];
      const ox = Math.min(p.x + p.w, q.x + q.w) - Math.max(p.x, q.x);
      const oy = Math.min(p.y + p.h, q.y + q.h) - Math.max(p.y, q.y);
      assert.ok(ox <= 1e-4 || oy <= 1e-4, 'tiles do not overlap');
    }
  }
  return tiles;
}

test('treemap: random layouts fill without overlap', () => {
  for (let run = 0; run < RUNS; run++) {
    const n = 1 + Math.floor(rand() * 60);
    const values = Array.from({ length: n }, () => (rand() < 0.1 ? 0 : Math.floor(rand() ** 3 * 1e9)));
    check(values, { x: rand() * 50, y: rand() * 50, w: 1 + rand() * 1200, h: 1 + rand() * 800 });
  }
});

test('treemap: equal values in a square are squares', () => {
  const tiles = check([1, 1, 1, 1], { x: 0, y: 0, w: 100, h: 100 });
  for (const t of tiles) {
    assert.ok(Math.abs(t.w - 50) < 1e-9 && Math.abs(t.h - 50) < 1e-9);
  }
});

test('treemap: empty input and empty rectangles', () => {
  assert.deepEqual(squarify([], { x: 0, y: 0, w: 10, h: 10 }), []);
  assert.deepEqual(squarify([{ value: 0 }], { x: 0, y: 0, w: 10, h: 10 }), []);
  assert.deepEqual(squarify([{ value: 5 }], { x: 0, y: 0, w: 0, h: 10 }), []);
  const [one] = squarify([{ value: 5 }], { x: 3, y: 4, w: 10, h: 20 });
  assert.deepEqual([one.x, one.y, one.w, one.h], [3, 4, 10, 20]);
});

test('treemap: largest first, and the rest folded into one', () => {
  const items = [3, 9, 1, 7, 5].map((value) => ({ value }));
  const value = (it) => it.value;
  const fold = (rest) => ({ value: rest.reduce((n, it) => n + it.value, 0), rest: rest.length });
  assert.deepEqual(topItems(items, 10, value, fold).map(value), [9, 7, 5, 3, 1]);
  const top = topItems(items, 3, value, fold);
  assert.deepEqual(top.map(value), [9, 7, 9]);
  assert.equal(top[2].rest, 3);
});
