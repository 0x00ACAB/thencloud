// App data (library data, the search index, prefs...) is JSON sealed under
// the master key. Before sealing it's padded with spaces, which JSON ignores,
// to the same Padme buckets as files, so its stored size shows only roughly
// how much there is (the search index above all). Never past what the
// server takes for that name (routes/app_data.rs); the sealed box adds its
// nonce and tag on top.

const MAX = { search: 12 * 1024 * 1024 };
const DEFAULT_MAX = 2 * 1024 * 1024;
const SEALED_OVERHEAD = 64;
const enc = new TextEncoder();

/** `json` as UTF-8, padded with spaces; `paddedSize` is the crypto crate's padded_size. */
export function padJson(json, name, paddedSize) {
  const bytes = enc.encode(json);
  const cap = (MAX[name] ?? DEFAULT_MAX) - SEALED_OVERHEAD;
  const size = Math.max(bytes.length, Math.min(paddedSize(bytes.length), cap));
  if (size === bytes.length) return bytes;
  const out = new Uint8Array(size).fill(0x20);
  out.set(bytes);
  return out;
}
