// PDF tools: pages come out in the order and rotation asked for, and PDFs
// that are damaged or hostile either load or are refused, quickly.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PDFDocument, degrees } from 'pdf-lib';
import { loadPdf, buildPdf, rotations } from '../src/lib/pdfedit.js';
import { random, mutate, timed, RUNS, SEED } from './fuzz.js';

/** A PDF with `n` pages of different widths (to tell them apart). */
async function sample(n, rotate = 0) {
  const doc = await PDFDocument.create();
  for (let i = 0; i < n; i++) doc.addPage([100 + i * 10, 200]).setRotation(degrees(rotate));
  return doc.save();
}

const widths = (doc) => doc.getPages().map((p) => p.getWidth());

test('pdfedit: merge, reorder, rotate, repeat', async () => {
  const a = await loadPdf(await sample(3));
  const b = await loadPdf(await sample(2, 90));
  assert.deepEqual(rotations(b), [90, 90]);
  const bytes = await buildPdf(
    [a, b],
    [
      { src: 1, page: 1, rotate: 0 },
      { src: 0, page: 2, rotate: 90 },
      { src: 0, page: 0, rotate: -90 },
      { src: 1, page: 0, rotate: 270 },
      { src: 0, page: 2, rotate: 0 },
    ],
  );
  const out = await PDFDocument.load(bytes, { updateMetadata: false });
  assert.deepEqual(widths(out), [110, 120, 100, 100, 120]);
  assert.deepEqual(rotations(out), [90, 90, 270, 0, 0]);
  // Nothing of ours is written into it.
  assert.equal(out.getProducer(), undefined);
  assert.equal(out.getCreationDate(), undefined);
});

test('pdfedit: no pages is refused', async () => {
  await assert.rejects(buildPdf([await loadPdf(await sample(1))], []));
});

test(`pdfedit: damaged PDFs load or are refused, quickly (seed ${SEED})`, async () => {
  const rnd = random(SEED + 31);
  const base = await sample(3);
  for (let i = 0; i < Math.min(RUNS, 400); i++) {
    const bytes = mutate(base, rnd);
    let doc = null;
    try {
      doc = await timed(() => loadPdf(bytes), 2000);
    } catch (e) {
      assert.match(e.message, /couldn't be read|password|took/, e.message);
      if (/took/.test(e.message)) throw e;
      continue;
    }
    const n = doc.getPageCount();
    if (!n) continue;
    // Once it has loaded, every page copies.
    const out = await timed(() => buildPdf([doc], Array.from({ length: n }, (_, page) => ({ src: 0, page, rotate: 90 }))), 2000);
    assert.equal((await PDFDocument.load(out)).getPageCount(), n);
  }
});
