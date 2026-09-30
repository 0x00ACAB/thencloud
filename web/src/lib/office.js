// Office documents read in the browser: Word (DOCX), OpenDocument text
// (ODT), Excel (XLSX) and PowerPoint (PPTX). Loaded on demand.
//
// Each is a zip of XML from someone else (unzip.js, xml.js). What comes
// out is plain data, never markup: paragraphs of text runs, tables, sheets
// of cells, slides. OfficeView.svelte renders it as text, so nothing in a
// document can put HTML, styles or scripts on the page. Images inside are
// typed from their bytes and shown as blob: URLs; links are kept only to
// http(s) and mail. Nothing in a document makes the browser fetch anything.
//
// It's a reading preview, not a layout engine: fonts, colours, page layout,
// formulas and number formats aren't shown.

import { openZip, ZipError } from './unzip.js';
import { parseXml, XmlError, kid, kids, descendants, textOf } from './xml.js';
import { imageType, resolvePath } from './books.js';

export { ZipError };

export class OfficeError extends Error {}

/** Limits, whatever the document says. */
export const LIMITS = { blocks: 20_000, cells: 200_000, rows: 5_000, cols: 200, images: 200, slides: 500, text: 5_000_000 };

const FORMATS = ['docx', 'odt', 'xlsx', 'pptx'];
export const isOffice = (format) => FORMATS.includes(format);

const dec = new TextDecoder('utf-8');

/** Links a document may keep. */
export function safeHref(href) {
  if (typeof href !== 'string') return null;
  const h = href.trim();
  return /^(https?:\/\/|mailto:)/i.test(h) && h.length <= 2048 ? h : null;
}

/**
 * Open `bytes` as `format`. Resolves to one of
 * - `{ kind: 'document', blocks, urls }`
 * - `{ kind: 'sheets', sheets: [{ name, rows, truncated }] }`
 * - `{ kind: 'slides', slides: [{ blocks }], urls }`
 * where a block is `{ t: 'p', style, level, ordered, runs }`, `{ t: 'table',
 * rows: [[blocks]] }` or `{ t: 'img', url }`, and a run is `{ text, b, i, u,
 * s, href }`. `urls` are blob: URLs to revoke when done. Throws OfficeError
 * or ZipError.
 */
export async function openOffice(bytes, format) {
  const zip = openZip(bytes);
  const ctx = budget(zip);
  try {
    if (format === 'docx') return await docx(ctx);
    if (format === 'odt') return await odt(ctx);
    if (format === 'xlsx') return await xlsx(ctx);
    if (format === 'pptx') return await pptx(ctx);
  } catch (e) {
    for (const u of ctx.urls) URL.revokeObjectURL(u);
    if (e instanceof XmlError) throw new OfficeError(`This file can't be opened: ${e.message}.`);
    throw e;
  }
  throw new OfficeError(`${format} files can't be previewed.`);
}

/** Shared state while reading one file: its zip, what's been used of the limits, blob: URLs made. */
function budget(zip) {
  const ctx = {
    zip,
    urls: [],
    blocks: 0,
    text: 0,
    images: 0,
    async xml(name) {
      if (!zip.has(name)) return null;
      return parseXml(dec.decode(await zip.read(name)));
    },
    block(b) {
      if (++ctx.blocks > LIMITS.blocks) throw new Stop();
      return b;
    },
    run(r) {
      ctx.text += r.text.length;
      if (ctx.text > LIMITS.text) throw new Stop();
      return r;
    },
    async image(name) {
      if (!name || !zip.has(name) || ctx.images >= LIMITS.images) return null;
      ctx.images++;
      const data = await zip.read(name).catch(() => null);
      const type = data && imageType(data);
      if (!type) return null;
      const url = URL.createObjectURL(new Blob([data], { type }));
      ctx.urls.push(url);
      return url;
    },
  };
  return ctx;
}

/**
 * Give ordered list items their number (`n`): counted per level, and
 * started again after anything that isn't a list item. In tables too.
 */
export function numberLists(blocks) {
  const counts = [];
  for (const b of blocks) {
    if (b.t === 'table') {
      for (const row of b.rows) for (const cell of row) numberLists(cell);
    }
    if (b.t !== 'p' || b.style !== 'li') {
      counts.length = 0;
      continue;
    }
    counts.length = b.level + 1;
    counts[b.level] = (counts[b.level] ?? 0) + 1;
    if (b.ordered) b.n = counts[b.level];
  }
  return blocks;
}

/** Thrown when a limit is reached: what was read so far is kept. */
class Stop extends Error {}

async function upTo(fn) {
  try {
    await fn();
    return false;
  } catch (e) {
    if (e instanceof Stop) return true;
    throw e;
  }
}

/** A part's relationships: id to `{ target (a path in the zip, or the URL), external }`. */
async function rels(ctx, part) {
  const i = part.lastIndexOf('/');
  const name = `${part.slice(0, i + 1)}_rels/${part.slice(i + 1)}.rels`;
  const doc = await ctx.xml(name);
  const out = new Map();
  for (const r of kids(doc, 'Relationship')) {
    const target = r.attrs.Target ?? '';
    const external = r.attrs.TargetMode === 'External';
    out.set(r.attrs.Id, { target: external ? target : resolvePath(part, target), external });
  }
  return out;
}

const on = (el) => el && !['0', 'false', 'off', 'none'].includes(el.attrs['w:val'] ?? el.attrs.val ?? 'true');

// --------------------------------------------------------------------- DOCX

async function docx(ctx) {
  const main = 'word/document.xml';
  const doc = await ctx.xml(main);
  if (!doc) throw new OfficeError("This file can't be opened: it has no document in it.");
  const links = await rels(ctx, main);

  // Heading levels from the styles (their outline level), list numbering a
  // style may carry ("List Number"), and list kinds from the numbering.
  const styles = new Map();
  for (const s of kids(await ctx.xml('word/styles.xml'), 'w:style')) {
    const pPr = kid(s, 'w:pPr');
    const outline = kid(pPr, 'w:outlineLvl')?.attrs['w:val'];
    const name = (kid(s, 'w:name')?.attrs['w:val'] ?? '').toLowerCase();
    let level = outline !== undefined ? Number(outline) + 1 : 0;
    if (!level && name === 'title') level = 1;
    const m = !level && /^heading (\d)$/.exec(name);
    if (m) level = Number(m[1]);
    styles.set(s.attrs['w:styleId'], { level: Math.min(level, 6), numPr: kid(pPr, 'w:numPr') });
  }
  const numbering = await ctx.xml('word/numbering.xml');
  const abstract = new Map();
  for (const a of kids(numbering, 'w:abstractNum')) {
    const lvls = new Map(kids(a, 'w:lvl').map((l) => [l.attrs['w:ilvl'], kid(l, 'w:numFmt')?.attrs['w:val'] !== 'bullet']));
    abstract.set(a.attrs['w:abstractNumId'], lvls);
  }
  const nums = new Map(kids(numbering, 'w:num').map((n) => [n.attrs['w:numId'], abstract.get(kid(n, 'w:abstractNumId')?.attrs['w:val'])]));

  const blocks = [];
  const truncated = await upTo(async () => {
    for (const el of kids(kid(doc, 'w:body'))) await body(el, blocks);
  });
  return { kind: 'document', blocks: numberLists(blocks), urls: ctx.urls, truncated };

  async function body(el, out) {
    if (el.name === 'w:p') await paragraph(el, out);
    else if (el.name === 'w:tbl') out.push(ctx.block({ t: 'table', rows: await table(el) }));
    else if (el.name === 'w:sdt') for (const c of kids(kid(el, 'w:sdtContent'))) await body(c, out);
  }

  async function table(tbl) {
    const rows = [];
    let cells = 0;
    for (const tr of kids(tbl, 'w:tr')) {
      const row = [];
      for (const tc of kids(tr, 'w:tc')) {
        if (++cells > LIMITS.cells) throw new Stop();
        const cell = [];
        for (const c of kids(tc)) await body(c, cell);
        row.push(cell);
      }
      rows.push(row);
    }
    return rows;
  }

  async function paragraph(p, out) {
    const pPr = kid(p, 'w:pPr');
    const outline = kid(pPr, 'w:outlineLvl')?.attrs['w:val'];
    const style = styles.get(kid(pPr, 'w:pStyle')?.attrs['w:val']);
    const level = outline !== undefined ? Math.min(Number(outline) + 1, 6) : style?.level || 0;
    const numPr = kid(pPr, 'w:numPr') ?? style?.numPr;
    const block = { t: 'p', style: level ? `h${level}` : 'p', level: 0, ordered: false, runs: [] };
    if (numPr && !level) {
      const ilvl = kid(numPr, 'w:ilvl')?.attrs['w:val'] ?? '0';
      block.style = 'li';
      block.level = Math.min(Number(ilvl) || 0, 8);
      block.ordered = !!nums.get(kid(numPr, 'w:numId')?.attrs['w:val'])?.get(ilvl);
    }
    const images = [];
    await inline(p, block.runs, null, images);
    if (block.runs.length || !images.length) out.push(ctx.block(block));
    for (const url of images) out.push(ctx.block({ t: 'img', url }));
  }

  async function inline(el, runs, href, images) {
    for (const c of kids(el)) {
      if (c.name === 'w:r') await run(c, runs, href, images);
      else if (c.name === 'w:hyperlink') {
        const rel = links.get(c.attrs['r:id']);
        await inline(c, runs, rel?.external ? safeHref(rel.target) : null, images);
      } else if (['w:ins', 'w:smartTag', 'w:fldSimple', 'w:sdt', 'w:sdtContent'].includes(c.name)) await inline(c, runs, href, images);
    }
  }

  async function run(r, runs, href, images) {
    const rPr = kid(r, 'w:rPr');
    const fmt = { b: on(kid(rPr, 'w:b')), i: on(kid(rPr, 'w:i')), u: on(kid(rPr, 'w:u')), s: on(kid(rPr, 'w:strike')), href };
    let text = '';
    for (const c of kids(r)) {
      if (c.name === 'w:t') text += textOf(c);
      else if (c.name === 'w:tab') text += '\t';
      else if (c.name === 'w:br' || c.name === 'w:cr') text += '\n';
      else if (c.name === 'w:drawing' || c.name === 'w:pict') {
        for (const blip of descendants(c, 'a:blip')) {
          const rel = links.get(blip.attrs['r:embed']);
          const url = rel && !rel.external ? await ctx.image(rel.target) : null;
          if (url) images.push(url);
        }
      }
    }
    if (text) runs.push(ctx.run({ text, ...fmt }));
  }
}

// ---------------------------------------------------------------------- ODT

async function odt(ctx) {
  const content = await ctx.xml('content.xml');
  if (!content) throw new OfficeError("This file can't be opened: it has no content in it.");
  const text = kid(kid(content, 'office:body'), 'office:text');

  // Bold, italic and so on come from named automatic styles.
  const fmts = new Map();
  for (const doc of [content, await ctx.xml('styles.xml')]) {
    for (const s of descendants(doc ?? { children: [] }, 'style:style')) {
      const tp = kid(s, 'style:text-properties');
      if (!tp) continue;
      const a = tp.attrs;
      fmts.set(s.attrs['style:name'], {
        b: a['fo:font-weight'] === 'bold' || Number(a['fo:font-weight']) >= 600,
        i: a['fo:font-style'] === 'italic',
        u: !!a['style:text-underline-style'] && a['style:text-underline-style'] !== 'none',
        s: !!a['style:text-line-through-style'] && a['style:text-line-through-style'] !== 'none',
      });
    }
  }

  const blocks = [];
  const truncated = await upTo(async () => {
    for (const el of kids(text)) await body(el, blocks, 0);
  });
  return { kind: 'document', blocks, urls: ctx.urls, truncated };

  async function body(el, out, depth) {
    if (el.name === 'text:p' || el.name === 'text:h') await paragraph(el, out, el.name === 'text:h' ? Number(el.attrs['text:outline-level'] || 1) : 0, null);
    else if (el.name === 'text:list') await list(el, out, depth);
    else if (el.name === 'table:table') out.push(ctx.block({ t: 'table', rows: await table(el) }));
    else if (el.name === 'text:section') for (const c of kids(el)) await body(c, out, depth);
  }

  async function list(el, out, depth) {
    for (const item of kids(el, 'text:list-item')) {
      for (const c of kids(item)) {
        if (c.name === 'text:list') await list(c, out, depth + 1);
        else if (c.name === 'text:p' || c.name === 'text:h') await paragraph(c, out, 0, Math.min(depth, 8));
      }
    }
  }

  async function table(t) {
    const rows = [];
    let cells = 0;
    const trs = [...kids(t, 'table:table-row'), ...kids(kid(t, 'table:table-header-rows'), 'table:table-row')];
    for (const tr of trs) {
      const row = [];
      for (const tc of kids(tr, 'table:table-cell')) {
        // A cell repeated to the sheet's edge is empty padding.
        const times = Math.min(Number(tc.attrs['table:number-columns-repeated']) || 1, 50);
        for (let k = 0; k < times; k++) {
          if (++cells > LIMITS.cells) throw new Stop();
          const cell = [];
          for (const c of kids(tc)) await body(c, cell, 0);
          row.push(cell);
        }
      }
      rows.push(row);
    }
    return rows;
  }

  async function paragraph(p, out, heading, listLevel) {
    const block = {
      t: 'p',
      style: heading ? `h${Math.min(Math.max(heading, 1), 6)}` : listLevel !== null ? 'li' : 'p',
      level: listLevel ?? 0,
      ordered: false,
      runs: [],
    };
    const images = [];
    await inline(p, block.runs, fmts.get(p.attrs['text:style-name']) ?? {}, null, images);
    if (block.runs.length || !images.length) out.push(ctx.block(block));
    for (const url of images) out.push(ctx.block({ t: 'img', url }));
  }

  async function inline(el, runs, fmt, href, images) {
    for (const c of el.children) {
      if (typeof c === 'string') {
        runs.push(ctx.run({ text: c.replace(/\s+/g, ' '), ...fmt, href }));
        continue;
      }
      if (c.name === 'text:span') await inline(c, runs, { ...fmt, ...fmts.get(c.attrs['text:style-name']) }, href, images);
      else if (c.name === 'text:a') await inline(c, runs, fmt, safeHref(c.attrs['xlink:href']), images);
      else if (c.name === 'text:s') runs.push(ctx.run({ text: ' '.repeat(Math.min(Number(c.attrs['text:c']) || 1, 100)), ...fmt, href }));
      else if (c.name === 'text:tab') runs.push(ctx.run({ text: '\t', ...fmt, href }));
      else if (c.name === 'text:line-break') runs.push(ctx.run({ text: '\n', ...fmt, href }));
      else if (c.name === 'draw:frame') {
        for (const img of kids(c, 'draw:image')) {
          const url = await ctx.image(resolvePath('content.xml', img.attrs['xlink:href'] ?? ''));
          if (url) images.push(url);
        }
      } else if (!c.name.startsWith('office:annotation')) await inline(c, runs, fmt, href, images);
    }
  }
}

// --------------------------------------------------------------------- XLSX

/** "B12" to a zero-based column: 1. */
export function columnOf(ref) {
  const m = /^([A-Z]{1,3})\d*$/.exec(ref ?? '');
  if (!m) return -1;
  let n = 0;
  for (const ch of m[1]) n = n * 26 + (ch.charCodeAt(0) - 64);
  return n - 1;
}

async function xlsx(ctx) {
  const book = 'xl/workbook.xml';
  const wb = await ctx.xml(book);
  if (!wb) throw new OfficeError("This file can't be opened: it has no workbook in it.");
  const links = await rels(ctx, book);
  const shared = kids(await ctx.xml('xl/sharedStrings.xml'), 'si').map((si) =>
    kid(si, 't') ? textOf(kid(si, 't')) : kids(si, 'r').map((r) => textOf(kid(r, 't'))).join(''),
  );

  const sheets = [];
  let cells = 0;
  for (const s of kids(kid(wb, 'sheets'), 'sheet').slice(0, 50)) {
    const rel = links.get(s.attrs['r:id']);
    const doc = rel && !rel.external ? await ctx.xml(rel.target) : null;
    if (!doc) continue;
    const rows = [];
    let truncated = false;
    for (const row of kids(kid(doc, 'sheetData'), 'row')) {
      if (rows.length >= LIMITS.rows) {
        truncated = true;
        break;
      }
      const values = [];
      let next = 0;
      for (const c of kids(row, 'c')) {
        const at = columnOf((c.attrs.r ?? '').replace(/\d+$/, '')) ;
        const col = at >= 0 ? at : next;
        next = col + 1;
        if (col >= LIMITS.cols) {
          truncated = true;
          continue;
        }
        if (++cells > LIMITS.cells) {
          truncated = true;
          break;
        }
        const type = c.attrs.t;
        const v = textOf(kid(c, 'v'));
        const f = kid(c, 'f');
        let text;
        // A formula saved without its result: show the formula.
        if (f && !v) text = `=${textOf(f)}`;
        else if (type === 's') text = shared[Number(v)] ?? '';
        else if (type === 'inlineStr') text = textOf(kid(c, 'is'));
        else if (type === 'b') text = v === '1' ? 'TRUE' : 'FALSE';
        else text = v;
        values[col] = text;
      }
      // Rows may skip numbers; keep them where they are.
      const r = Number(row.attrs.r);
      while (Number.isInteger(r) && r > 0 && rows.length < Math.min(r - 1, LIMITS.rows)) rows.push([]);
      rows.push(Array.from(values, (x) => x ?? ''));
    }
    while (rows.length && rows[rows.length - 1].every((x) => x === '')) rows.pop();
    sheets.push({ name: s.attrs.name || `Sheet ${sheets.length + 1}`, rows, truncated });
  }
  if (!sheets.length) throw new OfficeError("This file can't be opened: it has no sheets in it.");
  return { kind: 'sheets', sheets };
}

// --------------------------------------------------------------------- PPTX

async function pptx(ctx) {
  const pres = 'ppt/presentation.xml';
  const doc = await ctx.xml(pres);
  if (!doc) throw new OfficeError("This file can't be opened: it has no slides in it.");
  const links = await rels(ctx, pres);
  const slides = [];
  const truncated = await upTo(async () => {
    for (const id of kids(kid(doc, 'p:sldIdLst'), 'p:sldId').slice(0, LIMITS.slides)) {
      const rel = links.get(id.attrs['r:id']);
      if (!rel || rel.external) continue;
      const slide = await ctx.xml(rel.target);
      if (!slide) continue;
      const slideLinks = await rels(ctx, rel.target);
      const blocks = [];
      slides.push({ blocks });
      for (const el of descendants(slide, 'p:sp').concat(descendants(slide, 'p:pic'))) {
        if (el.name === 'p:pic') {
          for (const blip of descendants(el, 'a:blip')) {
            const r = slideLinks.get(blip.attrs['r:embed']);
            const url = r && !r.external ? await ctx.image(r.target) : null;
            if (url) blocks.push(ctx.block({ t: 'img', url }));
          }
          continue;
        }
        const ph = descendants(el, 'p:ph')[0];
        const title = ph?.attrs.type === 'title' || ph?.attrs.type === 'ctrTitle';
        // Body placeholders are bulleted by the slide's layout.
        const bulleted = !!ph && (!ph.attrs.type || ph.attrs.type === 'body' || ph.attrs.type === 'obj');
        for (const p of kids(kid(el, 'p:txBody'), 'a:p')) {
          const runs = [];
          for (const c of kids(p)) {
            if (c.name === 'a:r' || c.name === 'a:fld') {
              const pr = kid(c, 'a:rPr')?.attrs ?? {};
              const link = kid(kid(c, 'a:rPr'), 'a:hlinkClick')?.attrs['r:id'];
              const target = link ? slideLinks.get(link) : null;
              const text = textOf(kid(c, 'a:t'));
              if (text) {
                runs.push(ctx.run({ text, b: pr.b === '1', i: pr.i === '1', u: !!pr.u && pr.u !== 'none', s: !!pr.strike && pr.strike !== 'noStrike', href: target?.external ? safeHref(target.target) : null }));
              }
            } else if (c.name === 'a:br') runs.push(ctx.run({ text: '\n' }));
          }
          const pPr = kid(p, 'a:pPr');
          const bullet = !title && !kid(pPr, 'a:buNone') && (bulleted || !!kid(pPr, 'a:buChar') || !!kid(pPr, 'a:buAutoNum'));
          if (runs.length) blocks.push(ctx.block({ t: 'p', style: title ? 'h2' : bullet ? 'li' : 'p', level: Math.min(Number(kid(p, 'a:pPr')?.attrs.lvl) || 0, 8), ordered: false, runs }));
        }
      }
    }
  });
  if (!slides.length) throw new OfficeError("This file can't be opened: it has no slides in it.");
  return { kind: 'slides', slides, urls: ctx.urls, truncated };
}
