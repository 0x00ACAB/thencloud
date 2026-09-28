// A small mutation fuzzer for the tests: a seeded random source (so a
// failure can be replayed with FUZZ_SEED) and byte-level mutations of
// sample inputs. Parsers of untrusted files get thousands of these.

export const SEED = Number(process.env.FUZZ_SEED ?? Date.now() % 1e9);
export const RUNS = Number(process.env.FUZZ_RUNS ?? 3000);

export function random(seed = SEED) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const int = (n) => Math.floor(next() * n);
  return { next, int, pick: (list) => list[int(list.length)] };
}

// Values that tend to break length and offset arithmetic.
const INTERESTING = [0, 1, 0x7f, 0x80, 0xff, 0xfe];
const INTERESTING32 = [0, 1, 0xffffffff, 0x7fffffff, 0x80000000, 0x10000, 8, 16];

/** A mutated copy of `bytes`. */
export function mutate(bytes, rnd) {
  let b = Uint8Array.from(bytes);
  for (let n = 1 + rnd.int(4); n > 0; n--) {
    const at = b.length ? rnd.int(b.length) : 0;
    switch (rnd.int(7)) {
      case 0: // flip a bit
        if (b.length) b[at] ^= 1 << rnd.int(8);
        break;
      case 1: // an interesting byte
        if (b.length) b[at] = rnd.pick(INTERESTING);
        break;
      case 2: // an interesting 32-bit value, either byte order
        if (b.length >= 4) {
          const v = rnd.pick(INTERESTING32);
          const o = rnd.int(b.length - 3);
          new DataView(b.buffer).setUint32(o, v, rnd.int(2) === 0);
        }
        break;
      case 3: // cut it short
        b = b.subarray(0, at);
        break;
      case 4: // insert random bytes
        b = Uint8Array.from([...b.subarray(0, at), ...Array.from({ length: 1 + rnd.int(16) }, () => rnd.int(256)), ...b.subarray(at)]);
        break;
      case 5: // delete some
        b = Uint8Array.from([...b.subarray(0, at), ...b.subarray(at + 1 + rnd.int(16))]);
        break;
      case 6: // duplicate a slice
        b = Uint8Array.from([...b.subarray(0, at), ...b.subarray(at, at + rnd.int(32)), ...b.subarray(at)]);
        break;
    }
  }
  return b;
}

/** Random text, weighted towards characters parsers care about. */
export function randomText(rnd, max = 200) {
  const alphabet = ['a', 'Z', '0', '9', '.', ',', '"', '\n', '\r', '\t', ' ', '-', '_', '/', '\\', '(', ')', 'S', 'E', 'x', 'é', '\u0000', '﻿', '👍'];
  return Array.from({ length: rnd.int(max) }, () => rnd.pick(alphabet)).join('');
}

/** Run `fn` and fail if one call takes longer than `ms` (a hang or catastrophic backtracking). */
export async function timed(fn, ms = 250) {
  const t = performance.now();
  const out = await fn();
  const took = performance.now() - t;
  if (took > ms) throw new Error(`took ${Math.round(took)} ms`);
  return out;
}

const enc = new TextEncoder();
export const bytesOf = (s) => enc.encode(s);
export const concat = (...parts) => Uint8Array.from(parts.flatMap((p) => [...(typeof p === 'string' ? bytesOf(p) : p)]));
export const be32 = (n) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
export const le32 = (n) => [n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255];
/** An MP4 box. Its type is four Latin-1 bytes ("©nam" included). */
export const box = (type, ...children) => {
  const body = concat(...children);
  return concat(be32(8 + body.length), [...type].map((c) => c.charCodeAt(0)), body);
};
