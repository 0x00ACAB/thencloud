// Office documents are zips of XML from someone else. The XML reader must
// never throw anything but an XmlError, never expand an entity it doesn't
// know, and stop at its limits; the office reader must only ever throw an
// OfficeError or ZipError, keep only safe links, and type images from their
// bytes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseXml, XmlError, textOf } from '../src/lib/xml.js';
import { openOffice, OfficeError, ZipError, safeHref, columnOf, numberLists } from '../src/lib/office.js';
import { random, mutate, RUNS, SEED } from './fuzz.js';
import { zip } from './zipfile.js';

// Node has no blob: URLs to revoke; the reader only makes and drops them.
globalThis.URL.createObjectURL ??= () => 'blob:test';
globalThis.URL.revokeObjectURL ??= () => {};

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
const W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="r" xmlns:a="a"';
const rels = (items) =>
  `<?xml version="1.0"?><Relationships>${items.map(([id, target, ext]) => `<Relationship Id="${id}" Target="${target}"${ext ? ' TargetMode="External"' : ''}/>`).join('')}</Relationships>`;

function sampleDocx() {
  return zip({
    'word/document.xml': `<?xml version="1.0"?><w:document ${W}><w:body>
      <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Plans &amp; notes</w:t></w:r></w:p>
      <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r><w:r><w:rPr><w:i w:val="0"/></w:rPr><w:t xml:space="preserve"> plain</w:t></w:r>
        <w:hyperlink r:id="rWeb"><w:r><w:t>web</w:t></w:r></w:hyperlink>
        <w:hyperlink r:id="rJs"><w:r><w:t>trap</w:t></w:r></w:hyperlink></w:p>
      <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>first</w:t></w:r></w:p>
      <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>second</w:t></w:r></w:p>
      <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
      <w:p><w:r><w:drawing><a:blip r:embed="rImg"/></w:drawing></w:r><w:r><w:drawing><a:blip r:embed="rFake"/></w:drawing></w:r></w:p>
    </w:body></w:document>`,
    'word/_rels/document.xml.rels': rels([
      ['rWeb', 'https://example.com/', true],
      ['rJs', 'javascript:alert(1)', true],
      ['rImg', 'media/pic.png'],
      ['rFake', 'media/fake.png'],
    ]),
    'word/styles.xml': `<w:styles ${W}><w:style w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>`,
    'word/numbering.xml': `<w:numbering ${W}><w:abstractNum w:abstractNumId="7"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="7"/></w:num></w:numbering>`,
    'word/media/pic.png': PNG,
    'word/media/fake.png': '<svg onload="alert(1)"/>',
  });
}

function sampleXlsx() {
  return zip({
    'xl/workbook.xml': '<workbook xmlns:r="r"><sheets><sheet name="Budget" r:id="rId1"/></sheets></workbook>',
    'xl/_rels/workbook.xml.rels': rels([['rId1', 'worksheets/sheet1.xml']]),
    'xl/sharedStrings.xml': '<sst><si><t>Rent</t></si><si><r><t>Fo</t></r><r><t>od</t></r></si></sst>',
    'xl/worksheets/sheet1.xml':
      '<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1"><v>1200</v></c></row><row r="3"><c r="B3" t="s"><v>1</v></c><c r="D3" t="b"><v>1</v></c></row></sheetData></worksheet>',
  });
}

function samplePptx() {
  return zip({
    'ppt/presentation.xml': '<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId r:id="rS1"/></p:sldIdLst></p:presentation>',
    'ppt/_rels/presentation.xml.rels': rels([['rS1', 'slides/slide1.xml']]),
    'ppt/slides/slide1.xml':
      '<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Quarterly</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:txBody><a:p><a:pPr lvl="0"><a:buChar char="•"/></a:pPr><a:r><a:rPr b="1"/><a:t>Point</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>',
  });
}

function sampleOdt() {
  return zip({
    'content.xml': `<office:document-content xmlns:office="o" xmlns:text="t" xmlns:style="s" xmlns:fo="f" xmlns:table="tb" xmlns:xlink="x">
      <office:automatic-styles><style:style style:name="B"><style:text-properties fo:font-weight="bold"/></style:style></office:automatic-styles>
      <office:body><office:text>
        <text:h text:outline-level="2">Title</text:h>
        <text:p>Some<text:s text:c="2"/><text:span text:style-name="B">strong</text:span> <text:a xlink:href="mailto:a@example.com">mail</text:a></text:p>
        <text:list><text:list-item><text:p>item</text:p></text:list-item></text:list>
        <table:table><table:table-row><table:table-cell><text:p>cell</text:p></table:table-cell></table:table-row></table:table>
      </office:text></office:body></office:document-content>`,
  });
}

// ----------------------------------------------------------------- XML

test('xml: elements, attributes, text and the standard entities', () => {
  const doc = parseXml('<?xml version="1.0"?><!-- hi --><a x="1 &amp; 2"><b>&lt;t&gt; &#65;&#x42;</b><![CDATA[<raw>]]><c/></a>');
  assert.equal(doc.name, 'a');
  assert.equal(doc.attrs.x, '1 & 2');
  assert.equal(textOf(doc), '<t> AB<raw>');
});

test("xml: declared entities aren't expanded, and malformed input is refused", () => {
  const bomb = '<!DOCTYPE a [<!ENTITY x "xxxxxxxxxx"><!ENTITY y "&x;&x;&x;&x;&x;">]><a>&y;&x;</a>';
  assert.equal(textOf(parseXml(bomb)), '&y;&x;');
  for (const bad of ['', '<a>', '<a></b>', '<a><b></a></b>', 'text', '<a x=1/>', '<a/><b/>', '<a x="1" x', '<!-- open']) {
    assert.throws(() => parseXml(bad), XmlError, bad);
  }
  assert.throws(() => parseXml('<a>'.repeat(300) + '</a>'.repeat(300)), XmlError, 'too deep');
});

test(`xml: random and mutated input only ever fails with an XmlError (seed ${SEED})`, () => {
  const rnd = random();
  const seeds = ['<a x="1"><b>t&amp;</b><![CDATA[x]]><!--c--></a>', '<?xml version="1.0"?><w:p xmlns:w="w"><w:t>hi</w:t></w:p>'].map((s) =>
    new TextEncoder().encode(s),
  );
  for (let i = 0; i < RUNS; i++) {
    const text = new TextDecoder().decode(mutate(rnd.pick(seeds), rnd));
    try {
      parseXml(text);
    } catch (e) {
      assert.ok(e instanceof XmlError, `${e} for ${JSON.stringify(text)}`);
    }
  }
});

// -------------------------------------------------------------- office

test('office: a Word document becomes headings, runs, lists, tables and typed images', async () => {
  const d = await openOffice(sampleDocx(), 'docx');
  assert.equal(d.kind, 'document');
  const [h, p, li1, li2, table, img] = d.blocks;
  assert.equal(h.style, 'h1');
  assert.equal(h.runs[0].text, 'Plans & notes');
  assert.deepEqual(
    p.runs.map((r) => [r.text, !!r.b, r.href]),
    [
      ['bold', true, null],
      [' plain', false, null],
      ['web', false, 'https://example.com/'],
      ['trap', false, null],
    ],
  );
  assert.deepEqual([li1.style, li1.ordered, li1.n, li2.n], ['li', true, 1, 2]);
  assert.equal(textOf({ children: [] }), '');
  assert.equal(table.t, 'table');
  assert.equal(table.rows[0][1][0].runs[0].text, 'B1');
  // The PNG is kept; the "image" that's really SVG isn't.
  assert.equal(img.t, 'img');
  assert.equal(d.blocks.filter((b) => b.t === 'img').length, 1);
});

test('office: a spreadsheet keeps cells where they are', async () => {
  const d = await openOffice(sampleXlsx(), 'xlsx');
  assert.equal(d.sheets[0].name, 'Budget');
  assert.deepEqual(d.sheets[0].rows, [['Rent', '', '1200'], [], ['', 'Food', '', 'TRUE']]);
  assert.equal(columnOf('A'), 0);
  assert.equal(columnOf('AA'), 26);
  assert.equal(columnOf('1A'), -1);
});

test('office: slides and OpenDocument text', async () => {
  const s = await openOffice(samplePptx(), 'pptx');
  assert.equal(s.slides.length, 1);
  assert.deepEqual(
    s.slides[0].blocks.map((b) => [b.style, b.runs[0].text, !!b.runs[0].b]),
    [
      ['h2', 'Quarterly', false],
      ['li', 'Point', true],
    ],
  );
  const o = await openOffice(sampleOdt(), 'odt');
  assert.equal(o.blocks[0].style, 'h2');
  const para = o.blocks[1].runs;
  assert.equal(para.map((r) => r.text).join(''), 'Some  strong mail');
  assert.ok(para.find((r) => r.text === 'strong').b);
  assert.equal(para.find((r) => r.text === 'mail').href, 'mailto:a@example.com');
  assert.equal(o.blocks[2].style, 'li');
  assert.equal(o.blocks[3].t, 'table');
});

test('office: only web and mail links are kept', () => {
  for (const ok of ['https://a.example/x', 'http://b.example', 'mailto:c@example.com']) assert.equal(safeHref(ok), ok);
  for (const bad of ['javascript:alert(1)', 'data:text/html,x', 'file:///etc/passwd', '/relative', 'vbscript:x', ' javascript:x', null]) {
    assert.equal(safeHref(bad), null, bad);
  }
});

test('office: numbered lists restart after other paragraphs, per level', () => {
  const li = (level, ordered = true) => ({ t: 'p', style: 'li', level, ordered, runs: [] });
  const blocks = numberLists([li(0), li(1), li(1), li(0), { t: 'p', style: 'p', runs: [] }, li(0)]);
  assert.deepEqual(
    blocks.map((b) => b.n),
    [1, 1, 2, 2, undefined, 1],
  );
});

test(`office: mutated documents only ever fail with an OfficeError or ZipError (seed ${SEED})`, async () => {
  const rnd = random(SEED + 1);
  const samples = [
    ['docx', sampleDocx()],
    ['xlsx', sampleXlsx()],
    ['pptx', samplePptx()],
    ['odt', sampleOdt()],
  ];
  for (let i = 0; i < Math.max(200, RUNS / 10); i++) {
    const [format, bytes] = rnd.pick(samples);
    try {
      const d = await openOffice(mutate(bytes, rnd), format);
      assert.ok(['document', 'sheets', 'slides'].includes(d.kind));
    } catch (e) {
      assert.ok(e instanceof OfficeError || e instanceof ZipError, `${format}: ${e?.stack ?? e}`);
    }
  }
});

test('office: a stored (not deflated) document of the wrong kind is refused cleanly', async () => {
  await assert.rejects(openOffice(sampleXlsx(), 'docx'), OfficeError);
  await assert.rejects(openOffice(new Uint8Array([1, 2, 3]), 'docx'), ZipError);
});
