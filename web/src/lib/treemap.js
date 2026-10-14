// Treemap layout for the storage analyzer: squarified (Bruls, Huizing and
// van Wijk), so tiles stay close to square and sizes are easy to compare.
// Pure functions, tested in tests/treemap.test.js.

/**
 * Lay `items` out in the rectangle `{ x, y, w, h }`, each tile's area in
 * proportion to `value(item)`. Items with no positive value are left out.
 * Returns `[{ item, x, y, w, h }]`, largest first.
 */
export function squarify(items, rect, value = (it) => it.value) {
  const list = items.map((item) => ({ item, v: value(item) })).filter((e) => e.v > 0);
  list.sort((a, b) => b.v - a.v);
  const total = list.reduce((n, e) => n + e.v, 0);
  const out = [];
  if (!total || rect.w <= 0 || rect.h <= 0) return out;
  const scale = (rect.w * rect.h) / total;
  for (const e of list) e.a = e.v * scale;

  let { x, y, w, h } = rect;
  let i = 0;
  while (i < list.length) {
    const side = Math.min(w, h);
    // Grow the row while that makes its worst tile more square.
    let row = [list[i]];
    let sum = list[i].a;
    let j = i + 1;
    while (j < list.length) {
      const next = sum + list[j].a;
      if (worst(row, sum, side) < worst([...row, list[j]], next, side)) break;
      row.push(list[j]);
      sum = next;
      j++;
    }
    // Lay the row along the shorter side.
    const thick = sum / side;
    let at = 0;
    const last = j === list.length;
    for (const e of row) {
      const len = e.a / thick;
      if (w >= h) out.push({ item: e.item, x, y: y + at, w: last ? w : thick, h: len });
      else out.push({ item: e.item, x: x + at, y, w: len, h: last ? h : thick });
      at += len;
    }
    if (w >= h) {
      x += thick;
      w -= thick;
    } else {
      y += thick;
      h -= thick;
    }
    i = j;
  }
  return out;
}

// The largest aspect ratio in a row of tiles with total area `sum` laid
// along a side of length `side`.
function worst(row, sum, side) {
  let max = 0;
  let min = Infinity;
  for (const e of row) {
    if (e.a > max) max = e.a;
    if (e.a < min) min = e.a;
  }
  const s2 = side * side;
  const sum2 = sum * sum;
  return Math.max((s2 * max) / sum2, sum2 / (s2 * min));
}

/**
 * The largest `limit` items, and the rest folded into one entry made by
 * `fold(rest)` (or nothing if none are left over). Keeps a folder of
 * thousands of small files from turning into thousands of slivers.
 */
export function topItems(items, limit, value, fold) {
  const sorted = [...items].sort((a, b) => value(b) - value(a));
  if (sorted.length <= limit) return sorted;
  const rest = sorted.slice(limit - 1);
  return [...sorted.slice(0, limit - 1), fold(rest)];
}
