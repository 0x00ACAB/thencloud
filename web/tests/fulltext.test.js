// The words kept for searching inside files, and how a query matches them.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { wordsOf, queryWords, matchesWords, MAX_WORDS } from '../src/lib/fulltext.js';
import { random, randomText, RUNS, SEED } from './fuzz.js';

test('fulltext: words are found whatever the case, accents or punctuation', () => {
  const w = wordsOf('Quarterly REPORT: café budgets, e-mail (draft) — and "naïve" plans! a 2026 x');
  assert.equal(w, '2026 and budgets café draft e-mail naïve plans quarterly report');
  // A decomposed é is the same word.
  assert.ok(matchesWords(w, queryWords('café')));
  assert.ok(matchesWords(w, queryWords('quart rep')), 'prefixes of every word');
  assert.ok(!matchesWords(w, queryWords('quart invoice')), 'all words must match');
  assert.ok(!matchesWords(w, queryWords('uarterly')), 'only word starts');
  assert.ok(!matchesWords(w, []));
});

test('fulltext: at most MAX_WORDS words are kept per file', () => {
  const text = Array.from({ length: MAX_WORDS * 2 }, (_, i) => `w${i}`).join(' ');
  assert.equal(wordsOf(text).split(' ').length, MAX_WORDS);
});

test(`fulltext: any text reads, and every word of it then matches (seed ${SEED})`, () => {
  const rnd = random(SEED + 21);
  for (let i = 0; i < Math.min(RUNS, 1000); i++) {
    const text = randomText(rnd, 200);
    const words = wordsOf(text);
    for (const q of queryWords(text).slice(0, 5)) assert.ok(matchesWords(words, [q]), JSON.stringify([text, q]));
  }
});
