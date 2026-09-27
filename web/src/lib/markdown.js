// Markdown rendering for previews. Loaded on demand.
//
// A shared .md file is untrusted input rendered inside the app's own origin,
// where the keys live, so the HTML marked produces always goes through
// DOMPurify. Images aren't loaded: a remote one would tell its host that
// the file was opened (the CSP blocks them anyway), and there's no way yet
// to point at a sibling file in the encrypted tree.

import { Marked } from 'marked';
import DOMPurify from 'dompurify';

const marked = new Marked({ gfm: true, breaks: false });

const purify = DOMPurify();
purify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    const href = node.getAttribute('href') || '';
    if (/^(https?:|mailto:)/i.test(href)) {
      node.setAttribute('target', '_blank');
      node.setAttribute('rel', 'noopener noreferrer');
    } else if (!href.startsWith('#')) {
      node.removeAttribute('href');
    }
  }
  if (node.tagName === 'INPUT') {
    // Task list checkboxes; nothing else survives as an input.
    if (node.getAttribute('type') !== 'checkbox') node.remove();
    else node.setAttribute('disabled', '');
  }
});

/** Render Markdown to a sanitised DocumentFragment. */
export function renderMarkdown(text) {
  const html = marked.parse(text, { async: false });
  const frag = purify.sanitize(html, {
    RETURN_DOM_FRAGMENT: true,
    FORBID_TAGS: ['style', 'form', 'button', 'textarea', 'select', 'iframe', 'video', 'audio', 'source', 'picture'],
    FORBID_ATTR: ['style', 'srcset'],
  });
  for (const img of frag.querySelectorAll('img')) {
    const span = document.createElement('span');
    span.className = 'md-image';
    const alt = img.getAttribute('alt');
    span.textContent = alt ? `Image: ${alt}` : 'Image';
    span.title = "Images in Markdown files aren't loaded";
    img.replaceWith(span);
  }
  return frag;
}
