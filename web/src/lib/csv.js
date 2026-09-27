// CSV and TSV parsing for the table preview (RFC 4180: quoted fields may
// hold the separator, newlines and doubled quotes). The file is untrusted;
// cells only ever reach the page as text.

/** The separator to use for a file name: tab for .tsv, else comma. */
export function separatorFor(name) {
  return /\.tsv$/i.test(name) ? '\t' : ',';
}

/**
 * Split `text` into rows of cells. Stops after `maxRows` rows and says so
 * with `truncated`. A trailing newline doesn't make an empty last row.
 */
export function parseCsv(text, sep = ',', maxRows = Infinity) {
  const rows = [];
  let row = [];
  let cell = '';
  let quoted = false;
  let i = 0;
  if (text.charCodeAt(0) === 0xfeff) i = 1; // byte order mark
  const n = text.length;
  const endRow = () => {
    row.push(cell);
    rows.push(row);
    row = [];
    cell = '';
  };
  for (; i < n; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"') {
        if (text[i + 1] === '"') {
          cell += '"';
          i++;
        } else quoted = false;
      } else cell += c;
    } else if (c === '"' && cell === '') quoted = true;
    else if (c === sep) {
      row.push(cell);
      cell = '';
    } else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++;
      endRow();
      if (rows.length >= maxRows) return { rows, truncated: i + 1 < n };
    } else cell += c;
  }
  if (cell !== '' || row.length) endRow();
  return { rows, truncated: false };
}

const NUMBER = /^[-+]?(\d[\d,]*)?(\.\d+)?(e[-+]?\d+)?%?$/i;

/** A cell as a number for sorting, or null if it isn't one ("1,234.5", "12%"). */
export function numeric(cell) {
  const s = cell.trim();
  if (!s || !/\d/.test(s) || !NUMBER.test(s)) return null;
  const v = Number(s.replace(/[,%]/g, ''));
  return Number.isFinite(v) ? v : null;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Compare two cells: numbers by value and before text, empty cells last. */
export function compareCells(a = '', b = '') {
  if (!a !== !b) return a ? -1 : 1;
  const x = numeric(a);
  const y = numeric(b);
  if (x !== null && y !== null) return x - y;
  if (x !== null) return -1;
  if (y !== null) return 1;
  return collator.compare(a, b);
}
