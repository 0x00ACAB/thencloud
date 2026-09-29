// Line diffs between two versions of a text file, for "Compare" in the
// version history. Myers' algorithm (the shortest edit script), after the
// common start and end are trimmed off, with a cap on how many edits it
// looks for: past that the texts are too different to be worth showing
// line by line, and the answer is null rather than a long wait. Both texts
// are decrypted in the browser; nothing about them leaves it.

export const MAX_LINES = 50_000;
const MAX_EDITS = 4_000;

/** "a\nb\n" -> ['a', 'b']: a final newline doesn't make an empty last line. */
export function splitLines(text) {
  const lines = text.split(/\r\n|\n|\r/);
  if (lines.length > 1 && lines.at(-1) === '') lines.pop();
  return lines;
}

/**
 * The edit script from `a` to `b` (arrays of lines) as a list of
 * { op: '=' | '-' | '+', text, a, b }, with `a` and `b` the 0-based line
 * numbers on each side (only the side the line exists on). Null when more
 * than `maxEdits` lines would have to change, or either side is too long.
 */
export function diffLines(a, b, { maxEdits = MAX_EDITS } = {}) {
  if (a.length > MAX_LINES || b.length > MAX_LINES) return null;
  // Lines as small numbers, so the inner loop compares integers.
  const ids = new Map();
  const id = (line) => {
    let n = ids.get(line);
    if (n === undefined) ids.set(line, (n = ids.size));
    return n;
  };
  const A = Int32Array.from(a, id);
  const B = Int32Array.from(b, id);

  let start = 0;
  while (start < A.length && start < B.length && A[start] === B[start]) start++;
  let endA = A.length;
  let endB = B.length;
  while (endA > start && endB > start && A[endA - 1] === B[endB - 1]) endA--, endB--;

  const middle = myers(A.subarray(start, endA), B.subarray(start, endB), maxEdits);
  if (!middle) return null;
  const out = [];
  for (let i = 0; i < start; i++) out.push({ op: '=', text: a[i], a: i, b: i });
  for (const [op, x, y] of middle) {
    if (op === '=') out.push({ op, text: a[start + x], a: start + x, b: start + y });
    else if (op === '-') out.push({ op, text: a[start + x], a: start + x });
    else out.push({ op, text: b[start + y], b: start + y });
  }
  for (let i = endA, j = endB; i < a.length; i++, j++) out.push({ op: '=', text: a[i], a: i, b: j });
  return out;
}

/** [[op, indexInA, indexInB]], or null past `maxD` edits. */
function myers(A, B, maxD) {
  const n = A.length;
  const m = B.length;
  const max = n + m;
  const off = max + 1;
  const v = new Int32Array(2 * max + 3);
  const trace = [];
  for (let d = 0; d <= max; d++) {
    if (d > maxD) return null;
    // Where each diagonal reached after d - 1 edits, for the walk back.
    trace.push(v.slice(off - d - 1, off + d + 2));
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[off + k - 1] < v[off + k + 1]) ? v[off + k + 1] : v[off + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && A[x] === B[y]) x++, y++;
      v[off + k] = x;
      if (x >= n && y >= m) return walkBack(trace, n, m);
    }
  }
  return walkBack(trace, n, m);
}

function walkBack(trace, n, m) {
  const ops = [];
  let x = n;
  let y = m;
  for (let d = trace.length - 1; d > 0; d--) {
    const v = trace[d];
    const at = (k) => v[k + d + 1];
    const k = x - y;
    const prevK = k === -d || (k !== d && at(k - 1) < at(k + 1)) ? k + 1 : k - 1;
    const prevX = at(prevK);
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) ops.push(['=', --x, --y]);
    if (x === prevX) ops.push(['+', x, --y]);
    else ops.push(['-', --x, y]);
  }
  while (x > 0 && y > 0) ops.push(['=', --x, --y]);
  return ops.reverse();
}

/**
 * The changes with `context` unchanged lines around each, as hunks:
 * [{ lines: [...ops], skippedBefore }] where skippedBefore counts the
 * unchanged lines left out ahead of it.
 */
export function hunks(ops, context = 3) {
  const changed = ops.map((o, i) => (o.op === '=' ? -1 : i)).filter((i) => i >= 0);
  if (!changed.length) return [];
  const out = [];
  let from = Math.max(0, changed[0] - context);
  let to = Math.min(ops.length, changed[0] + context + 1);
  let shown = 0;
  const flush = () => {
    out.push({ skippedBefore: from - shown, lines: ops.slice(from, to) });
    shown = to;
  };
  for (const i of changed.slice(1)) {
    if (i - context <= to) to = Math.min(ops.length, i + context + 1);
    else {
      flush();
      from = i - context;
      to = Math.min(ops.length, i + context + 1);
    }
  }
  flush();
  return out;
}
