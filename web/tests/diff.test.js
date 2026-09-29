// Line diffs for the version history: the edit script turns one text into
// the other, is as short as possible, and stays quick on large or very
// different files (which give up instead of taking long).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { diffLines, hunks, splitLines, MAX_LINES } from '../src/lib/diff.js';
import { random, timed, RUNS, SEED } from './fuzz.js';

/** Fewest insertions and deletions, by the textbook LCS table. */
function shortest(a, b) {
  const t = Array.from({ length: a.length + 1 }, () => new Array(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) t[i][j] = a[i] === b[j] ? t[i + 1][j + 1] + 1 : Math.max(t[i + 1][j], t[i][j + 1]);
  }
  return a.length + b.length - 2 * t[0][0];
}

function check(a, b) {
  const ops = diffLines(a, b);
  assert.ok(ops, 'small inputs always get a diff');
  assert.deepEqual(ops.filter((o) => o.op !== '+').map((o) => o.text), a, 'keeps and deletions give the old text');
  assert.deepEqual(ops.filter((o) => o.op !== '-').map((o) => o.text), b, 'keeps and insertions give the new text');
  for (const o of ops) {
    if (o.op !== '+') assert.equal(a[o.a], o.text);
    if (o.op !== '-') assert.equal(b[o.b], o.text);
  }
  assert.equal(ops.filter((o) => o.op !== '=').length, shortest(a, b), 'as few changes as possible');
  return ops;
}

test('diff: small cases', () => {
  check([], []);
  check(['a'], []);
  check([], ['a']);
  check(['a', 'b', 'c'], ['a', 'x', 'c']);
  check(['a', 'b', 'c', 'a', 'b', 'b', 'a'], ['c', 'b', 'a', 'b', 'a', 'c']);
  assert.deepEqual(splitLines('a\nb\n'), ['a', 'b']);
  assert.deepEqual(splitLines('a\r\nb'), ['a', 'b']);
  assert.deepEqual(splitLines(''), ['']);
});

test(`diff: random line lists match the shortest edit (seed ${SEED})`, () => {
  const rnd = random(SEED + 11);
  const words = ['a', 'b', 'c', '', '  x'];
  const list = () => Array.from({ length: rnd.int(14) }, () => rnd.pick(words));
  for (let i = 0; i < RUNS; i++) check(list(), list());
});

test('diff: hunks keep a few lines of context and count what they skip', () => {
  const a = Array.from({ length: 40 }, (_, i) => `line ${i}`);
  const b = [...a];
  b[5] = 'changed';
  b.splice(30, 1);
  const hs = hunks(diffLines(a, b), 3);
  assert.equal(hs.length, 2);
  assert.equal(hs[0].skippedBefore, 2);
  assert.equal(hs[0].lines.filter((o) => o.op === '=').length, 6);
  assert.equal(hs[1].lines.filter((o) => o.op === '-').length, 1);
  assert.deepEqual(hunks(diffLines(a, a)), []);
});

test('diff: large files are quick, and very different ones give up', async () => {
  const big = Array.from({ length: MAX_LINES }, (_, i) => `row ${i}`);
  const edited = big.map((l, i) => (i % 5000 === 0 ? `${l} edited` : l));
  const ops = await timed(() => diffLines(big, edited), 2000);
  assert.equal(ops.filter((o) => o.op === '-').length, 10);
  const other = big.map((l) => `${l}!`);
  assert.equal(await timed(() => diffLines(big, other), 2000), null);
  assert.equal(diffLines([...big, 'one more'], big), null, 'past the line limit');
});
