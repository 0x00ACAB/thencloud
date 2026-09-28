// Links in shared PDFs and file names in zips: both come from other people.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { pageLinks } from '../src/lib/pdflinks.js';
import { zipParts } from '../src/lib/zip.js';
import { random, randomText, RUNS, SEED } from './fuzz.js';

const page = (annotations) => ({
  getViewport: () => ({ width: 100, height: 100, convertToViewportPoint: (x, y) => [x, y] }),
  getAnnotations: async () => annotations,
});
const doc = { getDestination: async () => [3], getPageIndex: async () => 3 };
const link = (url) => ({ subtype: 'Link', rect: [0, 0, 10, 10], url });

test('PDF links: only http(s) and mailto get through', async () => {
  const bad = ['javascript:alert(1)', 'JaVaScRiPt:alert(1)', ' javascript:x', 'data:text/html,x', 'file:///etc/passwd', 'vbscript:x', '//evil', 'blob:x', 'http\n:x'];
  const good = ['https://example.com', 'HTTP://example.com', 'mailto:a@b.c'];
  const out = await pageLinks(doc, page([...bad, ...good].map(link)));
  assert.deepEqual(out.map((l) => l.url), good);
});

test(`PDF links: random URLs never let another scheme through (seed ${SEED})`, async () => {
  const rnd = random(SEED + 6);
  const schemes = ['javascript:', 'data:', 'file:', 'http:', 'https:', 'mailto:', '', ' ', '\t'];
  for (let i = 0; i < RUNS; i++) {
    const url = rnd.pick(schemes) + randomText(rnd, 30);
    const out = await pageLinks(doc, page([link(url), { subtype: 'Link', rect: [1, 2, 3, 4], dest: randomText(rnd, 5) }]));
    for (const l of out) if (l.url !== undefined) assert.match(l.url, /^(https?:|mailto:)/i);
  }
});

/** Names in a zip's central directory. */
function zipNames(bytes) {
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const names = [];
  const dec = new TextDecoder();
  for (let o = 0; o + 46 <= bytes.length; o++) {
    if (v.getUint32(o, true) !== 0x02014b50) continue;
    const n = v.getUint16(o + 28, true);
    names.push(dec.decode(bytes.subarray(o + 46, o + 46 + n)));
  }
  return names;
}

async function zip(entries, children = () => []) {
  const parts = [];
  for await (const p of zipParts(entries, { list: async (e) => children(e), open: () => ({ count: 1, read: async () => new Uint8Array([1, 2, 3]) }) })) parts.push(p);
  return Uint8Array.from(parts.flatMap((p) => [...p]));
}

const file = (name) => ({ node: { kind: 'file', id: name }, meta: { name, size: 3, mtime: 0 } });
const folder = (name) => ({ node: { kind: 'folder', id: name }, meta: { name, size: 0, mtime: 0 } });

test('zip: names from other people can\'t leave the zip', async () => {
  const names = zipNames(await zip([file('../../etc/passwd'), file('/abs'), file('..'), file('a\\..\\b'), file('\u0000x'), folder('..')], () => [file('../up')]));
  assert.equal(names.length, 7);
  for (const n of names) {
    assert.ok(!n.startsWith('/'), n);
    assert.ok(!n.split('/').includes('..'), n);
    assert.ok(!n.includes('\\') && !n.includes('\u0000'), n);
  }
});

test(`zip: random names stay inside and unique (seed ${SEED})`, async () => {
  const rnd = random(SEED + 7);
  for (let i = 0; i < Math.min(RUNS, 300); i++) {
    const entries = Array.from({ length: 1 + rnd.int(5) }, () => (rnd.int(4) ? file : folder)(rnd.pick(['..', '.', '', '/', 'a']) + randomText(rnd, 12)));
    const names = zipNames(await zip(entries, () => [file(randomText(rnd, 8))]));
    for (const n of names) assert.ok(!n.startsWith('/') && !n.split('/').includes('..') && !n.includes('\\'), JSON.stringify(n));
    assert.equal(new Set(names.map((n) => n.toLowerCase())).size, names.length, 'names are unique');
  }
});

test("zip: an export's data folder doesn't clash with a folder of the same name", async () => {
  const data = { ...folder('thencloud-data'), children: [file('music.json')] };
  const names = zipNames(await zip([folder('thencloud-data'), data], (e) => e.children ?? [file('mine.txt')]));
  assert.equal(new Set(names).size, names.length, 'names are unique');
  assert.ok(names.some((n) => n.endsWith('/music.json')) && names.some((n) => n.endsWith('/mine.txt')));
});
