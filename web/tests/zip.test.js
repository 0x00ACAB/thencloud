import test from 'node:test';
import assert from 'node:assert/strict';
import { zipParts } from '../src/lib/zip.js';

function entry(name, chunks) {
  const size = chunks.reduce((n, c) => n + c.length, 0);
  return {
    item: { node: { id: name, kind: 'file' }, meta: { name, size, mtime: Date.now() }, key: null },
    open: async () => ({ count: chunks.length, read: async (i) => chunks[i] }),
  };
}

async function buildZip(entries, list = async () => []) {
  const parts = [];
  for await (const part of zipParts(
    entries.map((e) => e.item),
    { list, open: (e) => entries.find((x) => x.item === e).open(e), onProgress: () => {} },
  )) {
    parts.push(part);
  }
  return Buffer.concat(parts.map((p) => Buffer.from(p)));
}

/** Pulls a stored (uncompressed) file's bytes out of the zip by its name, given the known size. */
function extractStored(zip, name, size) {
  const n = Buffer.from(name, 'utf8');
  for (let i = 0; i < zip.length - 30; i++) {
    if (zip.readUInt32LE(i) !== 0x04034b50) continue;
    const nameLen = zip.readUInt16LE(i + 26);
    const extraLen = zip.readUInt16LE(i + 28);
    const start = i + 30;
    if (nameLen === n.length && zip.subarray(start, start + nameLen).equals(n)) {
      const dataStart = start + nameLen + extraLen;
      return zip.subarray(dataStart, dataStart + size);
    }
  }
  throw new Error(`entry ${name} not found`);
}

test('a file read through several async chunks lands whole in the zip (regression: open() must be awaited)', async () => {
  const chunks = [new Uint8Array([1, 2, 3]), new Uint8Array([4, 5, 6, 7]), new Uint8Array([8])];
  const zip = await buildZip([entry('piece.bin', chunks)]);
  const extracted = extractStored(zip, 'piece.bin', 8);
  assert.deepEqual([...extracted], [1, 2, 3, 4, 5, 6, 7, 8]);
});

test('several files each keep their own content', async () => {
  const a = entry('a.bin', [new Uint8Array([1, 1])]);
  const b = entry('b.bin', [new Uint8Array([2, 2, 2])]);
  const zip = await buildZip([a, b]);
  assert.deepEqual([...extractStored(zip, 'a.bin', 2)], [1, 1]);
  assert.deepEqual([...extractStored(zip, 'b.bin', 3)], [2, 2, 2]);
});

test('an empty file produces a zero-length entry, not a missing one', async () => {
  const zip = await buildZip([entry('empty.bin', [])]);
  assert.deepEqual([...extractStored(zip, 'empty.bin', 0)], []);
});
