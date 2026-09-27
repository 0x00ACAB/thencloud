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

const SAFE_URL = /^(https?:|mailto:)/i;

/**
 * The page's links: `{ left, top, width, height }` in percent of the page,
 * plus `url` (http(s) or mailto only) or `page` (1-based, in this document).
 */
export async function pageLinks(doc, page) {
  const vp = page.getViewport({ scale: 1 });
  const out = [];
  for (const a of await page.getAnnotations({ intent: 'display' })) {
    if (a.subtype !== 'Link' || !a.rect) continue;
    const [x1, y1] = vp.convertToViewportPoint(a.rect[0], a.rect[1]);
    const [x2, y2] = vp.convertToViewportPoint(a.rect[2], a.rect[3]);
    const box = {
      left: (Math.min(x1, x2) / vp.width) * 100,
      top: (Math.min(y1, y2) / vp.height) * 100,
      width: (Math.abs(x2 - x1) / vp.width) * 100,
      height: (Math.abs(y2 - y1) / vp.height) * 100,
    };
    if (a.url && SAFE_URL.test(a.url)) {
      out.push({ ...box, url: a.url });
    } else if (a.dest) {
      try {
        const dest = typeof a.dest === 'string' ? await doc.getDestination(a.dest) : a.dest;
        const ref = dest?.[0];
        const index = typeof ref === 'number' ? ref : await doc.getPageIndex(ref);
        out.push({ ...box, page: index + 1 });
      } catch {
        /* a broken destination is just not a link */
      }
    }
  }
  return out;
}
