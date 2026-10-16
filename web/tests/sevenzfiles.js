// The files inside the 7z fixtures in tests/fixtures/7z, made by
// `node tests/sevenzfiles.js <dir>` and then 7-Zip (see make7z.sh there).
// The tests rebuild the same contents to compare against.
import { writeFileSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';

/** Bytes that look a little like x86 code: calls and jumps for the BCJ filters to rewrite. */
function code(n) {
  const out = new Uint8Array(n);
  let seed = 7;
  const rand = () => (seed = (seed * 1103515245 + 12345) >>> 0) >>> 16;
  for (let i = 0; i < n; ) {
    const r = rand() % 8;
    if (r < 2 && i + 5 <= n) {
      out[i] = r === 0 ? 0xe8 : 0xe9;
      const target = (rand() % 4096) - 2048 + i;
      out[i + 1] = target & 0xff;
      out[i + 2] = (target >> 8) & 0xff;
      out[i + 3] = (target >> 16) & 0xff;
      out[i + 4] = (target >> 24) & 0xff;
      i += 5;
    } else if (r === 2 && i + 6 <= n) {
      out[i] = 0x0f;
      out[i + 1] = 0x84;
      i += 2;
    } else {
      out[i++] = rand() & 0xff;
    }
  }
  return out;
}

export const FILES = {
  'a.txt': new TextEncoder().encode('hello\n'),
  'empty.txt': new Uint8Array(0),
  'dir/b.txt': new TextEncoder().encode('All work and no play. '.repeat(400)),
  'dir/deeper/code.bin': code(6000),
};

/** Folders with nothing in them, which the listing leaves out. */
export const EMPTY_DIRS = ['nothing'];

// Only when run as a script, never when a test imports it.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href && process.argv[2]) {
  const root = process.argv[2];
  for (const [name, bytes] of Object.entries(FILES)) {
    mkdirSync(dirname(join(root, name)), { recursive: true });
    writeFileSync(join(root, name), bytes);
  }
  for (const d of EMPTY_DIRS) mkdirSync(join(root, d), { recursive: true });
}
