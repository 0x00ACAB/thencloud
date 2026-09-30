// A small XML reader for office documents (lib/office.js). They come from
// someone else, so this is deliberately plain: elements, attributes and
// text, with the five standard entities and character references and
// nothing else. No DTDs, no entity definitions (so no entity bombs), no
// fetching. The node count and depth are capped, and anything malformed
// throws an XmlError instead of being guessed at.
//
// The browser's DOMParser would do, but this also runs under Node, so the
// fuzz tests in web/tests/office.test.js can reach it.

export class XmlError extends Error {}

const MAX_DEPTH = 256;
const MAX_NODES = 2_000_000;

const NAMED = { lt: '<', gt: '>', amp: '&', quot: '"', apos: "'" };

function decodeEntities(s) {
  if (!s.includes('&')) return s;
  return s.replace(/&(#x[0-9a-fA-F]{1,6}|#[0-9]{1,7}|[a-z]{2,4});/g, (m, e) => {
    if (e[0] === '#') {
      const n = e[1] === 'x' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
      // Not a character, or a surrogate half: leave it as written.
      if (n > 0x10ffff || (n >= 0xd800 && n <= 0xdfff)) return m;
      return String.fromCodePoint(n);
    }
    return NAMED[e] ?? m;
  });
}

/**
 * Parse `text` into `{ name, attrs, children }` (children are elements or
 * strings; names keep their prefix, like `w:p`). Throws XmlError.
 */
export function parseXml(text) {
  const root = { name: '#document', attrs: {}, children: [] };
  const stack = [root];
  let nodes = 0;
  let i = 0;
  const n = text.length;
  const fail = (what) => {
    throw new XmlError(`not readable XML: ${what}`);
  };

  while (i < n) {
    const lt = text.indexOf('<', i);
    if (lt < 0) {
      addText(text.slice(i));
      break;
    }
    if (lt > i) addText(text.slice(i, lt));
    if (text.startsWith('<!--', lt)) {
      const end = text.indexOf('-->', lt + 4);
      if (end < 0) fail('a comment never ends');
      i = end + 3;
    } else if (text.startsWith('<![CDATA[', lt)) {
      const end = text.indexOf(']]>', lt + 9);
      if (end < 0) fail('a CDATA section never ends');
      addRaw(text.slice(lt + 9, end));
      i = end + 3;
    } else if (text.startsWith('<?', lt)) {
      const end = text.indexOf('?>', lt + 2);
      if (end < 0) fail('a processing instruction never ends');
      i = end + 2;
    } else if (text.startsWith('<!', lt)) {
      // A DOCTYPE, maybe with an internal subset in brackets: skipped whole,
      // and nothing it declares is used.
      let j = lt + 2;
      let depth = 0;
      for (; j < n; j++) {
        const c = text[j];
        if (c === '[') depth++;
        else if (c === ']') depth--;
        else if (c === '>' && depth <= 0) break;
      }
      if (j >= n) fail('a declaration never ends');
      i = j + 1;
    } else if (text[lt + 1] === '/') {
      const end = text.indexOf('>', lt + 2);
      if (end < 0) fail('a closing tag never ends');
      const name = text.slice(lt + 2, end).trim();
      const open = stack.pop();
      if (stack.length === 0 || open.name !== name) fail(`</${name.slice(0, 40)}> doesn't match`);
      i = end + 1;
    } else {
      i = openTag(lt);
    }
  }
  if (stack.length !== 1) fail('an element is never closed');
  const top = root.children.filter((c) => typeof c !== 'string');
  if (top.length !== 1) fail('it has no single root element');
  return top[0];

  function count() {
    if (++nodes > MAX_NODES) fail('it is too large');
  }

  function addRaw(s) {
    if (!s) return;
    const parent = stack[stack.length - 1];
    const last = parent.children.length - 1;
    if (last >= 0 && typeof parent.children[last] === 'string') parent.children[last] += s;
    else {
      count();
      parent.children.push(s);
    }
  }

  function addText(s) {
    addRaw(decodeEntities(s));
  }

  // Reads `<name attr="v" ...>` or `<name .../>` from `lt`; returns where it ends.
  function openTag(lt) {
    let j = lt + 1;
    const nameStart = j;
    while (j < n && !/[\s/>]/.test(text[j])) j++;
    const name = text.slice(nameStart, j);
    if (!name || /[<&"'=]/.test(name)) fail('an element has no proper name');
    const attrs = {};
    for (;;) {
      while (j < n && /\s/.test(text[j])) j++;
      if (j >= n) fail('a tag never ends');
      if (text[j] === '>' || text.startsWith('/>', j)) break;
      const aStart = j;
      while (j < n && !/[\s=/>]/.test(text[j])) j++;
      const aName = text.slice(aStart, j);
      while (j < n && /\s/.test(text[j])) j++;
      if (!aName || text[j] !== '=') fail('an attribute has no value');
      j++;
      while (j < n && /\s/.test(text[j])) j++;
      const q = text[j];
      if (q !== '"' && q !== "'") fail('an attribute value has no quotes');
      const end = text.indexOf(q, j + 1);
      if (end < 0) fail('an attribute value never ends');
      if (!Object.hasOwn(attrs, aName)) attrs[aName] = decodeEntities(text.slice(j + 1, end));
      j = end + 1;
    }
    count();
    const el = { name, attrs, children: [] };
    stack[stack.length - 1].children.push(el);
    if (text[j] === '>') {
      if (stack.length > MAX_DEPTH) fail('it is nested too deeply');
      stack.push(el);
      return j + 1;
    }
    return j + 2;
  }
}

/** The element's children that are elements named `name` (any, with no name). */
export function kids(el, name) {
  const out = [];
  for (const c of el?.children ?? []) if (typeof c !== 'string' && (!name || c.name === name)) out.push(c);
  return out;
}

/** The first child element named `name`, or null. */
export function kid(el, name) {
  for (const c of el?.children ?? []) if (typeof c !== 'string' && c.name === name) return c;
  return null;
}

/** Every element named `name` below `el`, in document order. */
export function descendants(el, name, out = []) {
  const stack = [el];
  // Iterative, depth-first, in order.
  while (stack.length) {
    const cur = stack.pop();
    if (!cur || typeof cur === 'string') continue;
    if (cur !== el && cur.name === name) out.push(cur);
    for (let k = cur.children.length - 1; k >= 0; k--) stack.push(cur.children[k]);
  }
  return out;
}

/** All the text below `el`. */
export function textOf(el) {
  if (typeof el === 'string') return el;
  let s = '';
  for (const c of el?.children ?? []) s += textOf(c);
  return s;
}
