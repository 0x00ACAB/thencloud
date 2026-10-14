// Building tar files for the tests (archive browsing): ustar headers, with
// GNU long names or pax paths when asked, and sizes that can lie.

const enc = new TextEncoder();

function header(name, size, type = '0', { prefix = '' } = {}) {
  const h = new Uint8Array(512);
  const put = (at, len, s) => h.set(enc.encode(s).subarray(0, len), at);
  put(0, 100, name);
  put(100, 8, '0000644\0');
  put(108, 8, '0000000\0');
  put(116, 8, '0000000\0');
  put(124, 12, size.toString(8).padStart(11, '0') + '\0');
  put(136, 12, '00000000000\0');
  h[156] = type.charCodeAt(0);
  put(257, 6, 'ustar\0');
  put(263, 2, '00');
  put(345, 155, prefix);
  h.fill(0x20, 148, 156);
  const sum = h.reduce((a, b) => a + b, 0);
  put(148, 8, sum.toString(8).padStart(6, '0') + '\0 ');
  return h;
}

const padded = (data) => {
  const out = new Uint8Array(Math.ceil(data.length / 512) * 512);
  out.set(data);
  return out;
};

/**
 * A tar of `files` ({ name: bytes | string }). `long: 'gnu' | 'pax'` writes
 * every name that way; `sizes` can lie about sizes; `types` sets an entry's
 * type flag ('5' a folder, '2' a symlink...).
 */
export function tar(files, { long = null, sizes = {}, types = {} } = {}) {
  const parts = [];
  for (const [name, content] of Object.entries(files)) {
    const data = typeof content === 'string' ? enc.encode(content) : content;
    if (long === 'gnu') {
      const n = enc.encode(name + '\0');
      parts.push(header('././@LongLink', n.length, 'L'), padded(n));
    } else if (long === 'pax') {
      let rec = ` path=${name}\n`;
      let len = rec.length;
      while (String(len).length + rec.length !== len) len = String(len).length + rec.length;
      const body = enc.encode(`${len}${rec}`);
      parts.push(header('PaxHeader', body.length, 'x'), padded(body));
    }
    parts.push(header(long ? 'short' : name, sizes[name] ?? data.length, types[name] ?? '0'), padded(data));
  }
  parts.push(new Uint8Array(1024));
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}
