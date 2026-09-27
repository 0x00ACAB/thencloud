// PDF rendering for previews, with pdf.js (loaded on demand). Pages are
// drawn to canvases; there's no text or annotation layer, and PDF
// JavaScript never runs. The worker and pdf.js's font, CMap and decoder
// files are served from our own origin (see vite.config.js).

import { getDocument, GlobalWorkerOptions } from 'pdfjs-dist';
import workerSrc from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

GlobalWorkerOptions.workerSrc = workerSrc;

/** Open a decrypted PDF. Call `.destroy()` on the result when done. */
export function openPdf(bytes) {
  return getDocument({
    data: bytes,
    standardFontDataUrl: '/pdfjs/standard_fonts/',
    cMapUrl: '/pdfjs/cmaps/',
    wasmUrl: '/pdfjs/wasm/',
    iccUrl: '/pdfjs/iccs/',
    enableXfa: false,
  });
}

/** Draw `page` into `canvas` at `width` CSS pixels wide. Returns the render task. */
export function drawPage(page, canvas, width) {
  const base = page.getViewport({ scale: 1 });
  const viewport = page.getViewport({ scale: width / base.width });
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.floor(viewport.width * ratio);
  canvas.height = Math.floor(viewport.height * ratio);
  return page.render({
    canvas,
    viewport,
    transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0],
  });
}
