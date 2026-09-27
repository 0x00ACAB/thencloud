// PDF rendering for previews, with pdf.js (loaded on demand). Pages are
// drawn to canvases, with pdf.js's text layer on top for selecting text and
// our own links layer (only http(s), mailto and links within the document;
// nothing is fetched until the user clicks). PDF JavaScript never runs. The worker and pdf.js's font, CMap and decoder
// files are served from our own origin (see vite.config.js).

import './upsert.js';
import { getDocument, GlobalWorkerOptions, TextLayer } from 'pdfjs-dist';

// One worker for the session, started on the first PDF.
GlobalWorkerOptions.workerPort = new Worker(new URL('./pdf.worker.js', import.meta.url), { type: 'module' });

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

/** Fill `container` with the page's selectable text. Positions are in percent, so it scales with the page. */
export async function drawText(page, container) {
  const layer = new TextLayer({ textContentSource: page.streamTextContent(), container, viewport: page.getViewport({ scale: 1 }) });
  await layer.render();
  return layer;
}

export { pageLinks } from './pdflinks.js';
