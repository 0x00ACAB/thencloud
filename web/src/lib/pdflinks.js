// A PDF page's links, as pdf.js reports them. Only web and mail links and
// links within the document get through: a shared PDF is untrusted, and a
// javascript:, data: or file: link must never become something to click.
// Kept apart from pdf.js so it can be tested without loading pdf.js.

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
