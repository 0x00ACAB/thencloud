// EPUB and comic (CBZ) books, read in the browser. Loaded on demand.
//
// A book is a zip from someone else (see unzip.js), rendered in the app's
// own origin, so an EPUB's chapters only reach the page through DOMPurify,
// like Markdown. Nothing in a book may make the browser fetch anything:
// styles and scripts are dropped, images are served from inside the book as
// blob: URLs typed from their bytes, and links are kept only to other
// chapters, the web (in a new tab) and mail.

import DOMPurify from 'dompurify';
import { openZip, ZipError } from './unzip.js';

export { ZipError };

const IMAGE = /\.(jpe?g|png|webp|gif|avif)$/i;

/** An image's type from its first bytes, or null. Never what the file claims. */
export function imageType(b) {
  if (b.length < 12) return null;
  if (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'image/jpeg';
  if (b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47) return 'image/png';
  if (b[0] === 0x47 && b[1] === 0x49 && b[2] === 0x46) return 'image/gif';
  if (b[0] === 0x52 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x46 && b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50) return 'image/webp';
  if (b[4] === 0x66 && b[5] === 0x74 && b[6] === 0x79 && b[7] === 0x70 && b[8] === 0x61 && b[9] === 0x76 && b[10] === 0x69) return 'image/avif';
  return null;
}

const natural = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
const hidden = (name) => name.startsWith('__MACOSX/') || name.split('/').some((p) => p.startsWith('.'));

/** `path` relative to the file `from` sits in, as a path in the zip. Null if it leaves the zip. */
export function resolvePath(from, path) {
  let p;
  try {
    p = decodeURIComponent(path.split('#')[0].split('?')[0]);
  } catch {
    return null;
  }
  if (!p) return from;
  const parts = p.startsWith('/') ? [] : from.split('/').slice(0, -1);
  for (const seg of p.split('/')) {
    if (seg === '..') {
      if (!parts.length) return null;
      parts.pop();
    } else if (seg && seg !== '.') parts.push(seg);
  }
  return parts.join('/');
}

/** An image in the zip as a blob: URL, or null if it isn't a known image. */
async function imageUrl(zip, name) {
  const bytes = await zip.read(name);
  const type = imageType(bytes);
  return type ? URL.createObjectURL(new Blob([bytes], { type })) : null;
}

// ---------------------------------------------------------------------- CBZ

/** A comic: its pages are the images in the zip, in natural name order. */
export function openCbz(bytes) {
  const zip = openZip(bytes);
  const pages = zip
    .names()
    .filter((n) => IMAGE.test(n) && !hidden(n))
    .sort(natural.compare);
  if (!pages.length) throw new ZipError("This file can't be opened: it has no pages.");
  return { kind: 'cbz', count: pages.length, page: (i) => imageUrl(zip, pages[i]) };
}

// --------------------------------------------------------------------- EPUB

const xml = (text) => {
  const doc = new DOMParser().parseFromString(text, 'application/xml');
  if (doc.getElementsByTagName('parsererror').length) throw new ZipError("This book can't be opened: its contents list is damaged.");
  return doc;
};
const byTag = (el, tag) => [...el.getElementsByTagNameNS('*', tag)];
const dec = new TextDecoder();

// Made on first use: it needs a DOM, which tests of the zip side don't have.
let purify = null;
function sanitizer() {
  if (purify) return purify;
  purify = DOMPurify();
  purify.addHook('afterSanitizeAttributes', (node) => {
    if (node.tagName === 'A') {
      const href = node.getAttribute('href') || '';
      node.removeAttribute('href');
      if (/^(https?:|mailto:)/i.test(href)) {
        node.setAttribute('href', href);
        node.setAttribute('target', '_blank');
        node.setAttribute('rel', 'noopener noreferrer');
      } else if (href && !/^[a-z][a-z0-9+.-]*:/i.test(href)) {
        node.setAttribute('data-book-href', href);
      }
    }
    if (node.tagName === 'IMG') {
      // Found in the zip after sanitising, never fetched from `src`.
      const src = node.getAttribute('src') || '';
      node.removeAttribute('src');
      if (src && !/^[a-z][a-z0-9+.-]*:/i.test(src)) node.setAttribute('data-book-src', src);
    }
  });
  return purify;
}

/**
 * An EPUB: `{ title, count, chapter(i) }`, where `chapter(i)` resolves to
 * `{ fragment, urls, path }`: the sanitised chapter, the blob: URLs made for
 * its images (revoke them when done) and its path, for resolving links.
 */
export async function openEpub(bytes) {
  const zip = openZip(bytes);
  const container = xml(dec.decode(await zip.read('META-INF/container.xml')));
  const opfPath = byTag(container, 'rootfile')[0]?.getAttribute('full-path');
  if (!opfPath || !zip.has(opfPath)) throw new ZipError("This book can't be opened: it has no contents list.");
  const opf = xml(dec.decode(await zip.read(opfPath)));
  const title = byTag(opf, 'title')[0]?.textContent?.trim() || '';
  const items = new Map();
  for (const it of byTag(opf, 'item')) {
    const href = it.getAttribute('href');
    const path = href && resolvePath(opfPath, href);
    if (path) items.set(it.getAttribute('id'), { path, type: it.getAttribute('media-type') || '' });
  }
  const chapters = byTag(opf, 'itemref')
    .map((r) => items.get(r.getAttribute('idref')))
    .filter((it) => it && /html/.test(it.type) && zip.has(it.path))
    .map((it) => it.path);
  if (!chapters.length) throw new ZipError("This book can't be opened: it has no chapters.");

  async function chapter(i) {
    const path = chapters[i];
    const text = dec.decode(await zip.read(path));
    let doc = new DOMParser().parseFromString(text, 'application/xhtml+xml');
    if (doc.getElementsByTagName('parsererror').length) doc = new DOMParser().parseFromString(text, 'text/html');
    const body = doc.getElementsByTagName('body')[0];
    const fragment = sanitizer().sanitize(body ? body.innerHTML : '', {
      RETURN_DOM_FRAGMENT: true,
      FORBID_TAGS: ['style', 'link', 'meta', 'base', 'form', 'button', 'input', 'textarea', 'select', 'iframe', 'object', 'embed', 'video', 'audio', 'source', 'picture', 'svg', 'math'],
      FORBID_ATTR: ['style', 'srcset', 'background', 'poster', 'class', 'id'],
    });
    const urls = [];
    for (const img of fragment.querySelectorAll('img')) {
      const src = img.getAttribute('data-book-src');
      const name = src && resolvePath(path, src);
      img.removeAttribute('data-book-src');
      const url = name && zip.has(name) ? await imageUrl(zip, name).catch(() => null) : null;
      if (url) {
        urls.push(url);
        img.setAttribute('src', url);
        img.setAttribute('loading', 'lazy');
      } else {
        img.remove();
      }
    }
    return { fragment, urls, path };
  }

  /** The chapter a link inside the book points at, or -1. */
  const chapterOf = (from, href) => chapters.indexOf(resolvePath(from, href));

  return { kind: 'epub', title, count: chapters.length, chapter, chapterOf };
}
