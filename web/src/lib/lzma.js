// LZMA and LZMA2 decoding, for 7z archives (lib/un7z.js). The input comes
// from someone else, so every read is bounds-checked, a match may only
// reach back into what has been decoded, and the output is exactly the
// size the caller asked for (which it checks against its own cap first).
// Follows the LZMA SDK's reference decoder (LzmaSpec.cpp), decoding into
// one flat buffer, so the whole output is the dictionary. The buffer grows
// as it fills rather than starting at the declared size, so damaged input
// that claims a lot but holds little costs little.

export class LzmaError extends Error {}

function fail(what) {
  throw new LzmaError(`The compressed data is damaged: ${what}.`);
}

const PROB_INIT = 1024;
const STATES = 12;
const POS_STATES_MAX = 16;
const END_POS_MODEL = 14;
const FULL_DISTANCES = 128;
const ALIGN_BITS = 4;
const MATCH_MIN = 2;
const MATCH_MAX = 273;

class Range {
  constructor(input, at, end) {
    this.input = input;
    this.at = at;
    this.end = end;
    if (this.byte() !== 0) fail('a range coder starts wrong');
    this.code = 0;
    this.range = 0xffffffff;
    for (let i = 0; i < 4; i++) this.code = this.code * 256 + this.byte();
    if (this.code === this.range) fail('a range coder starts wrong');
  }

  byte() {
    if (this.at >= this.end) fail('it ends too soon');
    return this.input[this.at++];
  }

  normalize() {
    if (this.range < 0x1000000) {
      this.range = this.range * 256;
      this.code = this.code * 256 + this.byte();
    }
  }

  bit(probs, i) {
    const p = probs[i];
    const bound = (this.range >>> 11) * p;
    let bit;
    if (this.code < bound) {
      this.range = bound;
      probs[i] = p + ((2048 - p) >>> 5);
      bit = 0;
    } else {
      this.range -= bound;
      this.code -= bound;
      probs[i] = p - (p >>> 5);
      bit = 1;
    }
    this.normalize();
    return bit;
  }

  direct(count) {
    let res = 0;
    for (; count > 0; count--) {
      this.range = this.range >>> 1;
      let bit = 0;
      if (this.code >= this.range) {
        this.code -= this.range;
        bit = 1;
      }
      res = res * 2 + bit;
      this.normalize();
    }
    return res;
  }

  tree(probs, base, bits) {
    let m = 1;
    for (let i = 0; i < bits; i++) m = (m << 1) + this.bit(probs, base + m);
    return m - (1 << bits);
  }

  reverse(probs, base, bits) {
    let m = 1;
    let sym = 0;
    for (let i = 0; i < bits; i++) {
      const bit = this.bit(probs, base + m);
      m = (m << 1) + bit;
      sym |= bit << i;
    }
    return sym;
  }
}

/** lc, lp and pb from a properties byte. */
export function lzmaProps(d) {
  if (d >= 9 * 5 * 5) fail('its properties are out of range');
  const lc = d % 9;
  d = (d - lc) / 9;
  const lp = d % 5;
  return { lc, lp, pb: (d - lp) / 5 };
}

class LenDecoder {
  constructor() {
    this.choice = new Uint16Array(2).fill(PROB_INIT);
    this.low = new Uint16Array(POS_STATES_MAX << 3).fill(PROB_INIT);
    this.mid = new Uint16Array(POS_STATES_MAX << 3).fill(PROB_INIT);
    this.high = new Uint16Array(256).fill(PROB_INIT);
  }

  decode(rc, posState) {
    if (rc.bit(this.choice, 0) === 0) return rc.tree(this.low, posState << 3, 3);
    if (rc.bit(this.choice, 1) === 0) return 8 + rc.tree(this.mid, posState << 3, 3);
    return 16 + rc.tree(this.high, 0, 8);
  }
}

/**
 * The decoder's state, kept across LZMA2 chunks that don't reset it. It
 * writes into `out` at `pos`; `dictStart` is how far back a match may reach
 * (an LZMA2 dictionary reset moves it up).
 */
class Lzma {
  constructor(size, input) {
    this.size = size;
    this.out = new Uint8Array(Math.min(size, input.length * 4 + 65536));
    this.pos = 0;
    this.dictStart = 0;
  }

  /** Make room for `n` more bytes (never past the declared size). */
  room(n) {
    const want = Math.min(this.size, this.pos + n);
    if (want <= this.out.length) return;
    const grown = new Uint8Array(Math.min(this.size, Math.max(want, this.out.length * 2)));
    grown.set(this.out.subarray(0, this.pos));
    this.out = grown;
  }

  /** The output, once it holds all `size` bytes. */
  result() {
    if (this.pos !== this.size) fail('it ends before its size');
    return this.out.length === this.size ? this.out : this.out.slice(0, this.size);
  }

  setProps({ lc, lp, pb }) {
    this.lc = lc;
    this.lp = lp;
    this.pb = pb;
    this.literals = new Uint16Array(0x300 << (lc + lp));
    this.reset();
  }

  reset() {
    this.literals.fill(PROB_INIT);
    this.isMatch = new Uint16Array(STATES << 4).fill(PROB_INIT);
    this.isRep = new Uint16Array(STATES).fill(PROB_INIT);
    this.isRepG0 = new Uint16Array(STATES).fill(PROB_INIT);
    this.isRepG1 = new Uint16Array(STATES).fill(PROB_INIT);
    this.isRepG2 = new Uint16Array(STATES).fill(PROB_INIT);
    this.isRep0Long = new Uint16Array(STATES << 4).fill(PROB_INIT);
    this.posSlot = new Uint16Array(4 << 6).fill(PROB_INIT);
    this.posDecoders = new Uint16Array(1 + FULL_DISTANCES - END_POS_MODEL).fill(PROB_INIT);
    this.align = new Uint16Array(1 << ALIGN_BITS).fill(PROB_INIT);
    this.len = new LenDecoder();
    this.repLen = new LenDecoder();
    this.state = 0;
    this.reps = [0, 0, 0, 0];
  }

  distance(rc, len) {
    const lenState = Math.min(len, 3);
    const slot = rc.tree(this.posSlot, lenState << 6, 6);
    if (slot < 4) return slot;
    const directBits = (slot >>> 1) - 1;
    let dist = (2 | (slot & 1)) * 2 ** directBits;
    if (slot < END_POS_MODEL) {
      dist += rc.reverse(this.posDecoders, dist - slot, directBits);
    } else {
      dist += rc.direct(directBits - ALIGN_BITS) * 16;
      dist += rc.reverse(this.align, 0, ALIGN_BITS);
    }
    return dist;
  }

  /**
   * Decode until the output holds `until` bytes. With `endMarker`, a stream
   * may also end early with its end marker (7z's LZMA allows one).
   */
  run(rc, until, endMarker) {
    let out = this.out;
    const { lc, lp, pb } = this;
    const pbMask = (1 << pb) - 1;
    const lpMask = (1 << lp) - 1;
    const reps = this.reps;
    let state = this.state;
    let pos = this.pos;
    while (pos < until) {
      if (pos + MATCH_MAX > out.length && out.length < this.size) {
        this.pos = pos;
        this.room(MATCH_MAX);
        out = this.out;
      }
      const posState = pos & pbMask;
      if (rc.bit(this.isMatch, (state << 4) + posState) === 0) {
        const prev = pos > this.dictStart ? out[pos - 1] : 0;
        const base = 0x300 * (((pos & lpMask) << lc) + (prev >>> (8 - lc)));
        let sym = 1;
        if (state >= 7) {
          if (reps[0] >= pos - this.dictStart) fail('a match reaches back too far');
          let matchByte = out[pos - reps[0] - 1];
          do {
            const matchBit = (matchByte >>> 7) & 1;
            matchByte <<= 1;
            const bit = rc.bit(this.literals, base + ((1 + matchBit) << 8) + sym);
            sym = (sym << 1) | bit;
            if (matchBit !== bit) break;
          } while (sym < 0x100);
        }
        while (sym < 0x100) sym = (sym << 1) | rc.bit(this.literals, base + sym);
        out[pos++] = sym - 0x100;
        state = state < 4 ? 0 : state < 10 ? state - 3 : state - 6;
        continue;
      }

      let len;
      if (rc.bit(this.isRep, state) === 0) {
        reps[3] = reps[2];
        reps[2] = reps[1];
        reps[1] = reps[0];
        len = this.len.decode(rc, posState);
        state = state < 7 ? 7 : 10;
        reps[0] = this.distance(rc, len);
        if (reps[0] === 0xffffffff) {
          if (!endMarker) fail('it has an end marker where none belongs');
          this.pos = pos;
          this.state = state;
          return true;
        }
      } else {
        if (rc.bit(this.isRepG0, state) === 0) {
          if (rc.bit(this.isRep0Long, (state << 4) + posState) === 0) {
            if (reps[0] >= pos - this.dictStart) fail('a match reaches back too far');
            state = state < 7 ? 9 : 11;
            out[pos] = out[pos - reps[0] - 1];
            pos++;
            continue;
          }
        } else {
          let dist;
          if (rc.bit(this.isRepG1, state) === 0) {
            dist = reps[1];
          } else {
            if (rc.bit(this.isRepG2, state) === 0) {
              dist = reps[2];
            } else {
              dist = reps[3];
              reps[3] = reps[2];
            }
            reps[2] = reps[1];
          }
          reps[1] = reps[0];
          reps[0] = dist;
        }
        len = this.repLen.decode(rc, posState);
        state = state < 7 ? 8 : 11;
      }
      len += MATCH_MIN;
      const dist = reps[0];
      if (dist >= pos - this.dictStart) fail('a match reaches back too far');
      if (len > until - pos) fail('a match runs past the end');
      let from = pos - dist - 1;
      for (let i = 0; i < len; i++) out[pos++] = out[from++];
    }
    this.pos = pos;
    this.state = state;
    return false;
  }
}

/**
 * Decode a 7z LZMA stream: `props` is the coder's 5 property bytes, and
 * `size` the unpacked size it declares.
 */
export function decodeLzma(props, input, size) {
  if (props.length < 5) fail('its properties are missing');
  const lz = new Lzma(size, input);
  lz.setProps(lzmaProps(props[0]));
  if (size === 0) return lz.result();
  lz.run(new Range(input, 0, input.length), size, true);
  return lz.result();
}

/** Decode a 7z LZMA2 stream of `size` unpacked bytes. */
export function decodeLzma2(props, input, size) {
  if (props.length < 1 || props[0] > 40) fail('its properties are out of range');
  const lz = new Lzma(size, input);
  let at = 0;
  let needProps = true;
  let needDictReset = true;
  const byte = () => {
    if (at >= input.length) fail('it ends too soon');
    return input[at++];
  };
  for (;;) {
    const control = byte();
    if (control === 0) break;
    if (control === 1 || control === 2) {
      // Stored: a big-endian size less one, then the bytes.
      if (control === 2 && needDictReset) fail('it starts without a dictionary reset');
      if (control === 1) lz.dictStart = lz.pos;
      needDictReset = false;
      const n = ((byte() << 8) | byte()) + 1;
      if (n > size - lz.pos) fail('a chunk runs past the end');
      if (n > input.length - at) fail('it ends too soon');
      lz.room(n);
      lz.out.set(input.subarray(at, at + n), lz.pos);
      lz.pos += n;
      at += n;
      continue;
    }
    if (control < 0x80) fail('a chunk has an unknown kind');
    const reset = (control >>> 5) & 3;
    const unpacked = (control & 0x1f) * 65536 + ((byte() << 8) | byte()) + 1;
    const packed = ((byte() << 8) | byte()) + 1;
    if (reset === 3) {
      lz.dictStart = lz.pos;
      needDictReset = false;
    } else if (needDictReset) {
      fail('it starts without a dictionary reset');
    }
    if (reset >= 2) {
      const p = lzmaProps(byte());
      if (p.lc + p.lp > 4) fail('its properties are out of range');
      lz.setProps(p);
      needProps = false;
    } else if (needProps) {
      fail('a chunk comes before any properties');
    } else if (reset === 1) {
      lz.reset();
    }
    if (unpacked > size - lz.pos) fail('a chunk runs past the end');
    if (packed > input.length - at) fail('it ends too soon');
    const rc = new Range(input, at, at + packed);
    lz.run(rc, lz.pos + unpacked, false);
    at += packed;
  }
  return lz.result();
}
