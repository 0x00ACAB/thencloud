// PDF tools: merge PDFs, and rotate, reorder, drop or extract pages, all in
// this browser with pdf-lib (loaded only when a PDF tool is opened). The
// PDFs are someone else's files: pdf-lib only copies their page objects
// into a new document, nothing in them runs, and a file that doesn't parse
// is reported, not guessed at. The result carries no producer or dates of
// its own, so nothing about the edit is added to it.

import { PDFDocument, degrees } from 'pdf-lib';

/** Largest PDF the tools will open (they're read into memory whole). */
export const MAX_PDF = 200 * 1024 * 1024;

/** Open a PDF for copying pages from. Encrypted (password) PDFs are refused. */
export async function loadPdf(bytes) {
  try {
    const doc = await PDFDocument.load(bytes, { updateMetadata: false });
    // A damaged page tree fails here rather than later, while copying.
    for (const p of doc.getPages()) p.getRotation();
    return doc;
  } catch (e) {
    if (e?.name === 'EncryptedPDFError' || /encrypted/i.test(e?.message)) throw new Error('This PDF is protected with a password, so its pages can\'t be copied.');
    throw new Error("This file couldn't be read as a PDF.");
  }
}

/** Each page's rotation (0, 90, 180 or 270), for showing the original. */
export const rotations = (doc) => doc.getPages().map((p) => ((p.getRotation().angle % 360) + 360) % 360);

/**
 * A new PDF from `pages`, in order: [{ src, page, rotate }] where `src`
 * indexes `docs` (from loadPdf), `page` is 0-based and `rotate` is added to
 * the page's own rotation (a multiple of 90).
 */
export async function buildPdf(docs, pages) {
  if (!pages.length) throw new Error('There are no pages to save.');
  const out = await PDFDocument.create({ updateMetadata: false });
  // Copy each source's pages in one go, so shared resources are copied once.
  const copies = new Map();
  for (const [i, doc] of docs.entries()) {
    const wanted = [...new Set(pages.filter((p) => p.src === i).map((p) => p.page))];
    if (!wanted.length) continue;
    const copied = await out.copyPages(doc, wanted);
    wanted.forEach((n, k) => copies.set(`${i}:${n}`, copied[k]));
  }
  const used = new Set();
  for (const p of pages) {
    let page = copies.get(`${p.src}:${p.page}`);
    // The same page twice needs its own copy.
    if (used.has(page)) [page] = await out.copyPages(docs[p.src], [p.page]);
    used.add(page);
    const angle = (((page.getRotation().angle + (p.rotate ?? 0)) % 360) + 360) % 360;
    page.setRotation(degrees(angle));
    out.addPage(page);
  }
  return out.save({ useObjectStreams: true });
}
