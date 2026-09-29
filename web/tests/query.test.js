// Search filters: type: and tag: are taken out of the words, quoted values
// keep their spaces, and anything else is left as text.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseQuery, passes, hasFilters } from '../src/lib/query.js';
import { random, randomText, RUNS, SEED } from './fuzz.js';

test('query: filters come out of the words', () => {
  assert.deepEqual(parseQuery('type:pdf tag:invoices report'), { text: 'report', types: ['pdf'], tags: ['invoices'] });
  assert.deepEqual(parseQuery('tag:"tax 2026" TYPE:.JPG  holiday photos '), { text: 'holiday photos', types: ['jpg'], tags: ['tax 2026'] });
  assert.deepEqual(parseQuery('"exact words" note:x'), { text: 'exact words note:x', types: [], tags: [] });
  assert.deepEqual(parseQuery('tag: type:'), { text: 'tag: type:', types: [], tags: [] });
  assert.equal(hasFilters(parseQuery('plain')), false);
});

test('query: types and tags filter entries', () => {
  const tags = { a: ['Invoices', '2026'], b: ['invoices'] };
  const env = { kind: (m) => (m.name.endsWith('.pdf') ? 'pdf' : m.name.endsWith('.png') ? 'image' : null), ext: (n) => n.split('.').pop().toLowerCase(), tagsOf: (id) => tags[id] ?? [] };
  const file = (id, name) => ({ node: { id, kind: 'file' }, meta: { name } });
  const a = file('a', 'bill.pdf');
  const b = file('b', 'scan.png');
  const c = { node: { id: 'c', kind: 'folder' }, meta: { name: 'Taxes' } };
  const q = (s) => parseQuery(s);
  assert.ok(passes(a, q('type:pdf tag:invoices'), env));
  assert.ok(!passes(b, q('type:pdf tag:invoices'), env));
  assert.ok(passes(b, q('type:pdf type:image tag:INVOICES'), env), 'any type, tags ignore case');
  assert.ok(!passes(b, q('tag:invoices tag:2026'), env), 'every tag');
  assert.ok(passes(a, q('type:PDF'), env) && passes(a, q('type:file'), env));
  assert.ok(passes(c, q('type:folder'), env) && !passes(c, q('type:file'), env));
  assert.ok(passes(b, q('type:png'), env), 'by extension');
});

test(`query: any input parses into strings (seed ${SEED})`, () => {
  const rnd = random(SEED + 21);
  for (let i = 0; i < RUNS; i++) {
    const s = randomText(rnd, 80);
    const p = parseQuery(s);
    assert.equal(typeof p.text, 'string');
    for (const x of [...p.types, ...p.tags]) assert.equal(typeof x, 'string');
  }
});
